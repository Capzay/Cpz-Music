//! Desktop only: the system's media controls (MPRIS on Linux, which is what
//! media keys and `playerctl` talk to; the media overlay on Windows) and the
//! Discord status. Lives on the UI thread, because the Windows controls are
//! tied to the window.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use cpz_core::player::Command;
use cpz_core::realtime::HubState;
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use tokio::sync::{mpsc, watch};

use crate::AppWindow;
use crate::discord::{self, Presence};

/// How far the "skip forward/back" buttons some media overlays have jump.
const SEEK_STEP: f64 = 10.0;
/// The controls guess the position from the last one they were told. Tell them
/// again only when it is off by more than this, which means a seek.
const DRIFT: f64 = 2.0;

struct Desktop {
    events: mpsc::UnboundedSender<MediaControlEvent>,
    /// Made once the window exists, since Windows needs its handle.
    controls: Option<MediaControls>,
    tried: bool,
    track: Option<i32>,
    playing: Option<bool>,
    /// The position last reported, and when.
    reported: (f64, Instant),
    discord: watch::Sender<Option<Presence>>,
    discord_on: bool,
}

thread_local! {
    static DESKTOP: RefCell<Option<Desktop>> = const { RefCell::new(None) };
}

/// Starts Discord and returns what the media keys ask for, turned into player
/// commands. Call on the UI thread, inside the Tokio runtime.
pub fn start(discord_on: bool) -> mpsc::UnboundedReceiver<MediaControlEvent> {
    let (events, receiver) = mpsc::unbounded_channel();
    DESKTOP.with(|slot| {
        *slot.borrow_mut() = Some(Desktop {
            events,
            controls: None,
            tried: false,
            track: None,
            playing: None,
            reported: (0.0, Instant::now()),
            discord: discord::spawn(),
            discord_on,
        });
    });
    receiver
}

/// The command a media key means, given where playback is now.
pub fn command(event: MediaControlEvent, position: f64) -> Option<Command> {
    let step = |direction: SeekDirection, by: f64| {
        let time = match direction {
            SeekDirection::Forward => position + by,
            SeekDirection::Backward => (position - by).max(0.0),
        };
        Command::Seek { time }
    };
    Some(match event {
        MediaControlEvent::Play => Command::Play { track: None },
        MediaControlEvent::Pause | MediaControlEvent::Stop => Command::Pause,
        MediaControlEvent::Toggle => Command::Toggle,
        MediaControlEvent::Next => Command::Next,
        MediaControlEvent::Previous => Command::Prev,
        MediaControlEvent::Seek(direction) => step(direction, SEEK_STEP),
        MediaControlEvent::SeekBy(direction, by) => step(direction, by.as_secs_f64()),
        MediaControlEvent::SetPosition(MediaPosition(at)) => Command::Seek {
            time: at.as_secs_f64(),
        },
        MediaControlEvent::SetVolume(volume) => Command::SetVolume {
            volume: volume.clamp(0.0, 1.0) as f32,
        },
        _ => return None,
    })
}

pub fn set_discord(on: bool) {
    DESKTOP.with(|slot| {
        if let Some(desktop) = slot.borrow_mut().as_mut() {
            desktop.discord_on = on;
            if !on {
                desktop.discord.send_replace(None);
            }
        }
    });
}

#[cfg(target_os = "windows")]
fn hwnd(ui: &AppWindow) -> Option<*mut std::ffi::c_void> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::ComponentHandle;
    let window = ui.window().window_handle();
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as *mut std::ffi::c_void),
        _ => None,
    }
}

#[cfg(not(target_os = "windows"))]
fn hwnd(_ui: &AppWindow) -> Option<*mut std::ffi::c_void> {
    None
}

impl Desktop {
    fn controls(&mut self, ui: &AppWindow) -> Option<&mut MediaControls> {
        if !self.tried {
            self.tried = true;
            let hwnd = hwnd(ui);
            if cfg!(target_os = "windows") && hwnd.is_none() {
                // The window is not up yet; try again on the next change.
                self.tried = false;
                return None;
            }
            let config = PlatformConfig {
                display_name: "Cpz Music",
                dbus_name: "cpz_music",
                hwnd,
            };
            let events = self.events.clone();
            match MediaControls::new(config).and_then(|mut controls| {
                controls.attach(move |event| {
                    let _ = events.send(event);
                })?;
                Ok(controls)
            }) {
                Ok(controls) => self.controls = Some(controls),
                Err(err) => tracing::warn!("no system media controls: {err:?}"),
            }
        }
        self.controls.as_mut()
    }
}

/// Mirrors what is playing onto the media controls and Discord. Called on
/// every hub change, so it only passes on what changed.
pub fn show(ui: &AppWindow, state: &HubState, server: &str) {
    DESKTOP.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(desktop) = slot.as_mut() else {
            return;
        };
        let player = &state.player;
        let current = player.current();
        let id = current.map(|t| t.id);
        let artwork = current
            .filter(|t| t.album.has_artwork)
            .map(|t| format!("{server}/api/artwork/{}", t.album.id))
            .unwrap_or_default();

        // Discord: what is playing on any of your devices, while it plays.
        let presence = current
            .filter(|_| player.playing && desktop.discord_on)
            .map(|t| Presence {
                title: t.title.clone(),
                artist: t.artist.name.clone(),
                album: t.album.title.clone(),
                artwork: artwork.clone(),
            });
        desktop.discord.send_if_modified(|shown| {
            let changed = *shown != presence;
            *shown = presence;
            changed
        });

        let (old_track, old_playing, (old_pos, at)) =
            (desktop.track, desktop.playing, desktop.reported);
        let expected = if old_playing == Some(true) {
            old_pos + at.elapsed().as_secs_f64()
        } else {
            old_pos
        };
        let new_track = old_track != id;
        let moved = (player.position - expected).abs() > DRIFT;
        if !new_track && old_playing == Some(player.playing) && !moved {
            return;
        }
        desktop.track = id;
        desktop.playing = Some(player.playing);
        desktop.reported = (player.position, Instant::now());

        let Some(controls) = desktop.controls(ui) else {
            return;
        };
        if new_track {
            let duration = player
                .duration
                .or(current.and_then(|t| t.duration))
                .filter(|d| d.is_finite() && *d > 0.0)
                .map(Duration::from_secs_f64);
            let metadata = match current {
                Some(t) => MediaMetadata {
                    title: Some(&t.title),
                    artist: Some(&t.artist.name),
                    album: Some(&t.album.title),
                    cover_url: (!artwork.is_empty()).then_some(artwork.as_str()),
                    duration,
                },
                None => MediaMetadata::default(),
            };
            if let Err(err) = controls.set_metadata(metadata) {
                tracing::debug!("media controls: {err:?}");
            }
        }
        let progress = Some(MediaPosition(Duration::from_secs_f64(
            player.position.max(0.0),
        )));
        let playback = match (current, player.playing) {
            (None, _) => MediaPlayback::Stopped,
            (Some(_), true) => MediaPlayback::Playing { progress },
            (Some(_), false) => MediaPlayback::Paused { progress },
        };
        if let Err(err) = controls.set_playback(playback) {
            tracing::debug!("media controls: {err:?}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_keys_become_player_commands() {
        assert_eq!(
            command(MediaControlEvent::Toggle, 0.0),
            Some(Command::Toggle)
        );
        assert_eq!(
            command(MediaControlEvent::Seek(SeekDirection::Backward), 4.0),
            Some(Command::Seek { time: 0.0 })
        );
        assert_eq!(
            command(
                MediaControlEvent::SeekBy(SeekDirection::Forward, Duration::from_secs(5)),
                30.0
            ),
            Some(Command::Seek { time: 35.0 })
        );
        assert_eq!(
            command(MediaControlEvent::SetVolume(3.0), 0.0),
            Some(Command::SetVolume { volume: 1.0 })
        );
        assert_eq!(command(MediaControlEvent::Raise, 0.0), None);
    }
}

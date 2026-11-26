//! What the screens show, kept on the UI thread. Background work hands its
//! results here through `slint::invoke_from_event_loop`.

use std::cell::RefCell;
use std::rc::Rc;

use cpz_core::realtime::HubState;
use cpz_core::types::{AlbumSummary, ArtistCard, ArtistHit, PlayerTrack, RepeatMode};
use slint::{ComponentHandle, Image, Model, ModelRc, VecModel};

use crate::art::Pixels;
use crate::{AlbumItem, App, AppWindow, ArtistItem, DeviceItem, Playback, TrackItem};

pub fn time(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

/// "48 min", or "1 hr 12 min" for something long.
pub fn length(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} hr"),
        (h, m) => format!("{h} hr {m} min"),
    }
}

pub fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The lists a page shows, and the models the UI draws them from. A model is
/// changed in place, so the UI only redraws the rows that changed.
#[derive(Default)]
pub struct View {
    pub albums: Rc<VecModel<AlbumItem>>,
    pub artists: Rc<VecModel<ArtistItem>>,
    pub album_tracks: Rc<VecModel<TrackItem>>,
    pub artist_albums: Rc<VecModel<AlbumItem>>,
    pub artist_tracks: Rc<VecModel<TrackItem>>,
    pub search_tracks: Rc<VecModel<TrackItem>>,
    pub search_albums: Rc<VecModel<AlbumItem>>,
    pub search_artists: Rc<VecModel<ArtistItem>>,
    pub queue: Rc<VecModel<TrackItem>>,
    pub devices: Rc<VecModel<DeviceItem>>,

    queue_ids: Vec<i32>,
    current: Option<i32>,
    playback_album: Option<i32>,
}

thread_local! {
    static VIEW: RefCell<Option<View>> = const { RefCell::new(None) };
}

/// Runs `change` with the view, on the UI thread.
pub fn with<R>(change: impl FnOnce(&mut View) -> R) -> R {
    VIEW.with(|view| {
        change(
            view.borrow_mut()
                .as_mut()
                .expect("the view is installed at startup"),
        )
    })
}

pub fn install(ui: &AppWindow) {
    let view = View::default();
    let app = ui.global::<App>();
    app.set_albums(ModelRc::from(view.albums.clone()));
    app.set_artists(ModelRc::from(view.artists.clone()));
    app.set_album_tracks(ModelRc::from(view.album_tracks.clone()));
    app.set_artist_albums(ModelRc::from(view.artist_albums.clone()));
    app.set_artist_tracks(ModelRc::from(view.artist_tracks.clone()));
    app.set_search_tracks(ModelRc::from(view.search_tracks.clone()));
    app.set_search_albums(ModelRc::from(view.search_albums.clone()));
    app.set_search_artists(ModelRc::from(view.search_artists.clone()));
    let playback = ui.global::<Playback>();
    playback.set_queue(ModelRc::from(view.queue.clone()));
    playback.set_devices(ModelRc::from(view.devices.clone()));
    VIEW.with(|slot| *slot.borrow_mut() = Some(view));
}

pub fn album_item(album: &AlbumSummary, art: Option<Pixels>) -> AlbumItem {
    AlbumItem {
        id: album.id,
        title: album.title.as_str().into(),
        artist: album.artist.as_str().into(),
        year: album.year.map(|y| y.to_string()).unwrap_or_default().into(),
        art: art.map(Image::from_rgb8).unwrap_or_default(),
    }
}

pub fn artist_item(artist: &ArtistCard) -> ArtistItem {
    ArtistItem {
        id: artist.id,
        name: artist.name.as_str().into(),
        detail: format!(
            "{} · {}",
            plural(artist.albums as usize, "album", "albums"),
            plural(artist.tracks as usize, "song", "songs")
        )
        .into(),
    }
}

pub fn artist_hit(artist: &ArtistHit) -> ArtistItem {
    ArtistItem {
        id: artist.id,
        name: artist.name.as_str().into(),
        detail: "Artist".into(),
    }
}

/// `number` is the track number, or the position in the list when there is
/// none, as in search results.
pub fn track_item(
    track: &PlayerTrack,
    position: usize,
    numbered: bool,
    current: Option<i32>,
    offline: bool,
) -> TrackItem {
    let number = if numbered {
        track
            .track_number
            .map_or_else(|| (position + 1).to_string(), |n| n.to_string())
    } else {
        (position + 1).to_string()
    };
    TrackItem {
        id: track.id,
        title: track.title.as_str().into(),
        artist: track.artist.name.as_str().into(),
        album: track.album.title.as_str().into(),
        number: number.into(),
        duration: track.duration.map(time).unwrap_or_default().into(),
        current: current == Some(track.id),
        offline,
    }
}

impl View {
    fn track_models(&self) -> [&Rc<VecModel<TrackItem>>; 3] {
        [&self.album_tracks, &self.artist_tracks, &self.search_tracks]
    }

    /// Moves the "playing" highlight in every list when the track changes.
    fn highlight(&mut self, current: Option<i32>) {
        if current == self.current {
            return;
        }
        self.current = current;
        for model in self.track_models() {
            for row in 0..model.row_count() {
                let mut item = model.row_data(row).expect("row is in range");
                let now = current == Some(item.id);
                if item.current != now {
                    item.current = now;
                    model.set_row_data(row, item);
                }
            }
        }
    }

    /// A cover arrived: show it wherever that album appears.
    pub fn art(&mut self, ui: &AppWindow, album_id: i32, pixels: Pixels) {
        let image = Image::from_rgb8(pixels);
        for model in [&self.albums, &self.artist_albums, &self.search_albums] {
            for row in 0..model.row_count() {
                let mut item = model.row_data(row).expect("row is in range");
                if item.id == album_id {
                    item.art = image.clone();
                    model.set_row_data(row, item);
                }
            }
        }
        let app = ui.global::<App>();
        let mut album = app.get_album();
        if album.id == album_id {
            album.art = image.clone();
            app.set_album(album);
        }
        if self.playback_album == Some(album_id) {
            ui.global::<Playback>().set_art(image);
        }
    }

    /// Mirrors the hub onto the player bar, queue and device list. Called a few
    /// times a second, so only what changed is touched.
    pub fn hub(
        &mut self,
        ui: &AppWindow,
        state: &HubState,
        offline: impl Fn(i32) -> bool,
        art: impl FnOnce(i32),
    ) {
        let playback = ui.global::<Playback>();
        let player = &state.player;
        let current = player.current();

        playback.set_has_track(current.is_some());
        if let Some(track) = current {
            playback.set_title(track.title.as_str().into());
            playback.set_artist(track.artist.name.as_str().into());
            playback.set_album_title(track.album.title.as_str().into());
            playback.set_album_id(track.album.id);
            if self.playback_album != Some(track.album.id) {
                self.playback_album = Some(track.album.id);
                playback.set_art(Image::default());
                if track.album.has_artwork {
                    art(track.album.id);
                }
            }
        }
        let duration = player
            .duration
            .or(current.and_then(|t| t.duration))
            .unwrap_or(0.0);
        playback.set_playing(player.playing);
        playback.set_loading(player.loading);
        playback.set_position(player.position as f32);
        playback.set_duration(duration as f32);
        playback.set_position_text(time(player.position).into());
        playback.set_duration_text(time(duration).into());
        playback.set_buffered(player.buffered);
        playback.set_error(player.error.clone().unwrap_or_default().into());
        playback.set_shuffle(player.queue.shuffle);
        playback.set_repeat(match player.queue.repeat {
            RepeatMode::Off => 0,
            RepeatMode::All => 1,
            RepeatMode::One => 2,
        });
        playback.set_volume(player.volume);
        playback.set_queue_index(player.queue.index as i32);

        let ids: Vec<i32> = player.queue.tracks.iter().map(|t| t.id).collect();
        if ids != self.queue_ids {
            let items: Vec<TrackItem> = player
                .queue
                .tracks
                .iter()
                .enumerate()
                .map(|(i, t)| track_item(t, i, false, None, offline(t.id)))
                .collect();
            self.queue.set_vec(items);
            self.queue_ids = ids;
        }
        self.highlight(current.map(|t| t.id));

        let active_name = state
            .devices
            .iter()
            .find(|d| Some(&d.device_id) == state.active_id.as_ref())
            .map(|d| d.name.clone())
            .unwrap_or_default();
        playback.set_remote(!state.is_active);
        playback.set_active_device(active_name.into());
        playback.set_connected(state.connected);
        let devices: Vec<DeviceItem> = state
            .devices
            .iter()
            .map(|d| DeviceItem {
                id: d.device_id.as_str().into(),
                name: d.name.as_str().into(),
                platform: d.platform.as_str().into(),
                active: Some(&d.device_id) == state.active_id.as_ref(),
                me: d.device_id == state.me,
            })
            .collect();
        if devices.len() != self.devices.row_count()
            || devices
                .iter()
                .enumerate()
                .any(|(i, d)| self.devices.row_data(i).as_ref() != Some(d))
        {
            self.devices.set_vec(devices);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_times_and_lengths() {
        assert_eq!(time(0.0), "0:00");
        assert_eq!(time(65.9), "1:05");
        assert_eq!(time(3725.0), "1:02:05");
        assert_eq!(time(-3.0), "0:00");
        assert_eq!(length(2880.0), "48 min");
        assert_eq!(length(3600.0), "1 hr");
        assert_eq!(length(4320.0), "1 hr 12 min");
        assert_eq!(plural(1, "song", "songs"), "1 song");
        assert_eq!(plural(3, "song", "songs"), "3 songs");
    }
}

//! The player as the UI sees it: a queue, a play button, and a snapshot of what
//! is happening, with the audio engine doing the work underneath.
//!
//! One thread owns all of it. The UI sends commands and watches the state, so
//! nothing the UI does can wait on audio or the network.

use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;
use tokio::sync::watch;

use crate::api::Api;
use crate::audio::{Engine, EngineEvent, Media, Output};
use crate::downloads::Downloads;
use crate::listens::Listens;
use crate::queue::{Queue, QueueView, Step};
use crate::store::Store;
use crate::types::PlayerTrack;

const TICK: Duration = Duration::from_millis(250);
/// A jump in position bigger than this between ticks was a seek, not listening.
const MAX_TICK_GAIN: f64 = 1.5;
const SAVE_EVERY: Duration = Duration::from_secs(10);
const SAVED_KEY: &str = "player";
/// While playing, the overlay is re-sent this often so a seek shows up there.
const ANNOUNCE_EVERY: Duration = Duration::from_secs(15);

/// Everything the player can be asked to do. Serializable so a remote device can
/// send the same commands to whichever device is playing. The JSON is the old
/// web app's format.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Command {
    /// Resume, or play this track: jump to it if queued, otherwise queue only it.
    Play {
        #[serde(default)]
        track: Option<PlayerTrack>,
    },
    Pause,
    Toggle,
    Next,
    Prev,
    SetQueue {
        tracks: Vec<PlayerTrack>,
        #[serde(rename = "startIndex", default)]
        start: usize,
    },
    PlayNext {
        track: PlayerTrack,
    },
    #[serde(rename = "addToQueue")]
    Append {
        track: PlayerTrack,
    },
    #[serde(rename = "removeFromQueue")]
    Remove {
        index: usize,
    },
    JumpTo {
        index: usize,
    },
    Seek {
        time: f64,
    },
    ToggleShuffle,
    CycleRepeat,
    SetVolume {
        volume: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub queue: QueueView,
    /// What the user asked for, which stays true while a track is loading.
    pub playing: bool,
    pub loading: bool,
    pub position: f64,
    pub duration: Option<f64>,
    pub buffered: f32,
    pub volume: f32,
    /// The current track plays from a download rather than the network.
    pub local: bool,
    pub error: Option<String>,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            queue: QueueView::default(),
            playing: false,
            loading: false,
            position: 0.0,
            duration: None,
            buffered: 0.0,
            volume: 1.0,
            local: false,
            error: None,
        }
    }
}

impl PlayerState {
    pub fn current(&self) -> Option<&PlayerTrack> {
        self.queue.tracks.get(self.queue.index)
    }
}

/// What is kept between runs so the app reopens where it left off, paused.
#[derive(Serialize, Deserialize)]
struct Saved {
    queue: QueueView,
    position: f64,
    volume: f32,
}

enum Input {
    Command(Command),
    Engine(EngineEvent),
    Adopt {
        queue: QueueView,
        position: f64,
        playing: bool,
    },
    Active(bool),
    Shutdown,
}

pub struct Player {
    inputs: Sender<Input>,
    state: watch::Receiver<PlayerState>,
    thread: Option<thread::JoinHandle<()>>,
}

pub struct PlayerDeps {
    pub api: Api,
    pub store: Arc<Store>,
    pub downloads: Arc<Downloads>,
    pub listens: Arc<Listens>,
    pub cache_dir: std::path::PathBuf,
    pub output: Output,
}

impl Player {
    /// Must be called from inside a Tokio runtime.
    pub fn start(deps: PlayerDeps) -> Self {
        let (engine, engine_events) =
            Engine::start(deps.api.clone(), deps.cache_dir.clone(), deps.output);
        let (inputs, inbox) = mpsc::channel();

        let forward = inputs.clone();
        thread::spawn(move || {
            for event in engine_events {
                if forward.send(Input::Engine(event)).is_err() {
                    return;
                }
            }
        });

        let (publish, state) = watch::channel(PlayerState::default());
        let mut core = Core {
            engine,
            queue: Queue::default(),
            state: PlayerState::default(),
            publish,
            deps,
            runtime: Handle::current(),
            generation: 0,
            loaded: None,
            finished: false,
            listened: 0.0,
            last_position: 0.0,
            last_save: Instant::now(),
            active: true,
            last_announce: Instant::now(),
        };
        core.restore();

        let thread = thread::Builder::new()
            .name("cpz-player".into())
            .spawn(move || {
                loop {
                    match inbox.recv_timeout(TICK) {
                        Ok(Input::Command(command)) => core.command(command),
                        Ok(Input::Engine(event)) => core.engine_event(event),
                        Ok(Input::Adopt {
                            queue,
                            position,
                            playing,
                        }) => core.adopt(queue, position, playing),
                        Ok(Input::Active(active)) => core.set_active(active),
                        Err(RecvTimeoutError::Timeout) => {}
                        Ok(Input::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                            core.leave_track(false);
                            core.save();
                            return;
                        }
                    }
                    core.tick();
                }
            })
            .expect("the OS can start a thread");

        Self {
            inputs,
            state,
            thread: Some(thread),
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.inputs.send(Input::Command(command));
    }

    pub fn state(&self) -> PlayerState {
        self.state.borrow().clone()
    }

    /// Takes over what another device was playing, from where it had got to.
    pub fn adopt(&self, queue: QueueView, position: f64, playing: bool) {
        let _ = self.inputs.send(Input::Adopt {
            queue,
            position,
            playing,
        });
    }

    /// An inactive device is a remote: it stays quiet and stops telling the
    /// server what it is playing, since something else is.
    pub fn set_active(&self, active: bool) {
        let _ = self.inputs.send(Input::Active(active));
    }

    pub fn subscribe(&self) -> watch::Receiver<PlayerState> {
        self.state.clone()
    }
}

/// The event forwarder holds a sender too, so dropping this one alone would never
/// close the channel. Saves the queue and stops the audio on the way out.
impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.inputs.send(Input::Shutdown);
        // Waits for the queue to be saved, so quitting right after loses nothing.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Core {
    engine: Engine,
    queue: Queue,
    state: PlayerState,
    publish: watch::Sender<PlayerState>,
    deps: PlayerDeps,
    runtime: Handle,
    /// The engine load the events we care about belong to.
    generation: u64,
    /// The track id the engine has open.
    loaded: Option<i32>,
    /// The loaded track played to its end, so the engine has nothing to resume.
    finished: bool,
    /// Seconds of the current track actually heard, for the listening stats.
    listened: f64,
    last_position: f64,
    last_save: Instant,
    active: bool,
    last_announce: Instant,
}

impl Core {
    fn adopt(&mut self, queue: QueueView, position: f64, playing: bool) {
        self.leave_track(false);
        self.queue.adopt(queue);
        self.state.playing = playing;
        self.loaded = None;
        if self.queue.current().is_some() {
            self.load(position, playing);
        } else {
            self.engine.stop();
        }
        self.state.position = position;
        self.announce();
        self.save();
        self.publish();
    }

    fn set_active(&mut self, active: bool) {
        if !active && self.state.playing {
            self.state.playing = false;
            self.engine.pause();
            self.publish();
        }
        self.active = active;
    }

    fn restore(&mut self) {
        let Ok(Some(saved)) = self.deps.store.get::<Saved>(SAVED_KEY) else {
            return;
        };
        self.queue.adopt(saved.queue);
        self.state.volume = saved.volume.clamp(0.0, 1.0);
        self.engine.set_volume(self.state.volume);
        if self.queue.current().is_some() {
            self.state.position = saved.position;
            self.load(saved.position, false);
        }
        self.publish();
    }

    fn save(&mut self) {
        self.last_save = Instant::now();
        let saved = Saved {
            queue: self.queue.view(),
            position: self.state.position,
            volume: self.state.volume,
        };
        if let Err(err) = self.deps.store.set(SAVED_KEY, &saved) {
            tracing::warn!("could not save the queue: {err}");
        }
    }

    fn publish(&mut self) {
        self.state.queue = self.queue.view();
        self.publish.send_replace(self.state.clone());
    }

    /// Tells the server what is playing, for the OBS overlay. Best effort.
    fn announce(&mut self) {
        if !self.active {
            return;
        }
        self.last_announce = Instant::now();
        let api = self.deps.api.clone();
        let track = self.queue.current().cloned();
        let (playing, position) = (self.state.playing, self.state.position);
        self.runtime.spawn(async move {
            let _ = api.now_playing(track.as_ref(), playing, position).await;
        });
    }

    /// Reports the listen for the track being left, if enough of it was heard.
    fn leave_track(&mut self, completed: bool) {
        let heard = std::mem::take(&mut self.listened);
        let Some(track_id) = self.loaded else { return };
        if heard <= 0.0 && !completed {
            return;
        }
        let (api, listens) = (self.deps.api.clone(), self.deps.listens.clone());
        self.runtime.spawn(async move {
            listens.report(&api, track_id, heard, completed).await;
            listens.flush(&api).await;
        });
    }

    fn load(&mut self, start_at: f64, autoplay: bool) {
        let Some(track) = self.queue.current() else {
            self.engine.stop();
            self.loaded = None;
            self.state.playing = false;
            return;
        };
        let id = track.id;
        let media = match self.deps.downloads.local_file(id) {
            Some(path) => Media::File(path),
            None => Media::Stream(id),
        };
        self.state.local = matches!(media, Media::File(_));
        self.state.duration = track.duration;
        self.state.position = start_at;
        self.state.loading = true;
        self.state.error = None;
        self.last_position = start_at;
        self.finished = false;
        self.loaded = Some(id);
        self.generation = self.engine.load(media, start_at, autoplay);
    }

    /// Brings the engine in line with the queue after a command: a different
    /// current track means leaving the old one and opening the new one.
    fn follow_queue(&mut self, autoplay: bool) {
        let current = self.queue.current().map(|track| track.id);
        if current != self.loaded {
            self.leave_track(false);
            self.state.playing = autoplay && current.is_some();
            self.load(0.0, self.state.playing);
            self.announce();
            self.save();
        }
    }

    fn restart(&mut self, at: f64) {
        if self.finished || self.loaded.is_none() {
            self.load(at, self.state.playing);
        } else {
            self.engine.seek(at);
            self.state.position = at;
            self.last_position = at;
        }
    }

    fn step(&mut self, step: Step) {
        match step {
            Step::Changed => self.follow_queue(true),
            Step::Restart => {
                self.leave_track(false);
                self.state.playing = true;
                self.engine.play();
                self.restart(0.0);
                self.announce();
            }
            Step::Stop => {
                self.state.playing = false;
                self.engine.pause();
                self.announce();
            }
        }
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Play { track: Some(track) } => {
                self.queue.play(track);
                self.state.playing = true;
                self.follow_queue(true);
                self.resume();
            }
            Command::Play { track: None } => self.resume(),
            Command::Pause => {
                self.state.playing = false;
                self.engine.pause();
                self.announce();
                self.save();
            }
            Command::Toggle if self.state.playing => {
                self.state.playing = false;
                self.engine.pause();
                self.announce();
                self.save();
            }
            Command::Toggle => self.resume(),
            Command::Next => {
                let step = self.queue.advance();
                self.step(step);
            }
            Command::Prev => {
                let step = self.queue.back(self.state.position);
                self.step(step);
            }
            Command::SetQueue { tracks, start } => {
                if self.queue.set(tracks, start) {
                    // The same track at the head of a new queue still starts over.
                    self.leave_track(false);
                    self.loaded = None;
                }
                self.follow_queue(true);
            }
            Command::PlayNext { track } => self.queue.play_next(track),
            Command::Append { track } => {
                let was_empty = self.queue.tracks().is_empty();
                self.queue.append(track);
                if was_empty {
                    self.follow_queue(false);
                }
            }
            Command::Remove { index } => {
                self.queue.remove(index);
            }
            Command::JumpTo { index } => {
                if self.queue.jump_to(index) {
                    self.follow_queue(true);
                    self.resume();
                }
            }
            Command::Seek { time } => {
                let time = match self.state.duration {
                    Some(duration) => time.clamp(0.0, duration),
                    None => time.max(0.0),
                };
                self.restart(time);
                self.announce();
            }
            Command::ToggleShuffle => self.queue.toggle_shuffle(),
            Command::CycleRepeat => self.queue.cycle_repeat(),
            Command::SetVolume { volume } => {
                self.state.volume = volume.clamp(0.0, 1.0);
                self.engine.set_volume(self.state.volume);
            }
        }
        self.publish();
    }

    fn resume(&mut self) {
        if self.queue.current().is_none() {
            return;
        }
        self.state.playing = true;
        if self.finished || self.loaded.is_none() {
            // Finished at the end of the queue: play starts it again from the top.
            let at = if self.finished {
                0.0
            } else {
                self.state.position
            };
            self.load(at, true);
        } else {
            self.engine.play();
        }
        self.announce();
    }

    fn engine_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::Loaded {
                generation,
                duration,
            } if generation == self.generation => {
                self.state.loading = false;
                if duration.is_some() {
                    self.state.duration = duration;
                }
            }
            EngineEvent::Ended { generation } if generation == self.generation => {
                self.finished = true;
                self.leave_track(true);
                // The engine has nothing open now, so whatever comes next loads
                // fresh, even when it is this same track again.
                self.loaded = None;
                let step = self.queue.advance();
                self.step(step);
                if step == Step::Stop {
                    self.state.position = self.state.duration.unwrap_or(self.state.position);
                }
            }
            EngineEvent::Failed {
                generation,
                message,
            } if generation == self.generation => {
                tracing::warn!("playback failed: {message}");
                self.leave_track(false);
                // Play retries from here.
                self.loaded = None;
                self.state.loading = false;
                self.state.playing = false;
                self.state.error = Some(message);
            }
            EngineEvent::DeviceLost {
                generation,
                position,
            } if generation == self.generation => {
                self.load(position, self.state.playing);
            }
            _ => return,
        }
        self.publish();
    }

    fn tick(&mut self) {
        if self.loaded.is_none() || self.finished {
            return;
        }
        let status = self.engine.status();
        if status.generation != self.generation || status.loading {
            return;
        }
        let gained = status.position - self.last_position;
        if self.state.playing && gained > 0.0 && gained < MAX_TICK_GAIN {
            self.listened += gained;
        }
        self.last_position = status.position;

        let changed = (status.position - self.state.position).abs() > 0.2
            || status.buffered != self.state.buffered;
        self.state.position = status.position;
        self.state.buffered = status.buffered;
        if changed {
            self.publish();
        }
        if self.state.playing && self.last_save.elapsed() > SAVE_EVERY {
            self.save();
        }
        if self.state.playing && self.last_announce.elapsed() > ANNOUNCE_EVERY {
            self.announce();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_round_trip_through_json() {
        let commands = [
            Command::Play { track: None },
            Command::Seek { time: 12.5 },
            Command::SetQueue {
                tracks: Vec::new(),
                start: 0,
            },
            Command::ToggleShuffle,
        ];
        for command in commands {
            let json = serde_json::to_string(&command).unwrap();
            assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), command);
        }
        assert_eq!(
            serde_json::to_value(Command::Seek { time: 3.0 }).unwrap(),
            serde_json::json!({ "type": "seek", "time": 3.0 })
        );
    }

    #[test]
    fn reads_commands_the_web_app_sends() {
        let parse = |json: serde_json::Value| serde_json::from_value::<Command>(json).unwrap();
        assert_eq!(
            parse(serde_json::json!({ "type": "play" })),
            Command::Play { track: None }
        );
        assert_eq!(parse(serde_json::json!({ "type": "next" })), Command::Next);
        assert_eq!(
            parse(serde_json::json!({ "type": "removeFromQueue", "index": 2 })),
            Command::Remove { index: 2 }
        );
        assert_eq!(
            parse(serde_json::json!({ "type": "setQueue", "tracks": [], "startIndex": 3 })),
            Command::SetQueue {
                tracks: Vec::new(),
                start: 3
            }
        );
        assert_eq!(
            parse(serde_json::json!({ "type": "setQueue", "tracks": [] })),
            Command::SetQueue {
                tracks: Vec::new(),
                start: 0
            }
        );
        assert_eq!(
            parse(serde_json::json!({ "type": "cycleRepeat" })),
            Command::CycleRepeat
        );
    }
}

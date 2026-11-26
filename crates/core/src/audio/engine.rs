//! Plays one track at a time on the sound card.
//!
//! Everything audio lives on one thread of its own, because the output stream is
//! not `Send` on every platform and because opening a track over the network can
//! take a while: callers send commands and never wait on audio.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rodio::{CpalErrorKind, Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use tokio::runtime::Handle;

use super::remote::{Remote, RemoteWatcher};
use crate::api::Api;

const TICK: Duration = Duration::from_millis(100);

/// Where a track's bytes come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Media {
    /// Downloaded to the device.
    File(PathBuf),
    /// Streamed from the server by track id.
    Stream(i32),
}

#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    /// The track is decoded and playing, or paused at its start position.
    Loaded {
        generation: u64,
        duration: Option<f64>,
    },
    /// It played to the end.
    Ended { generation: u64 },
    /// It could not be opened, or stopped arriving.
    Failed { generation: u64, message: String },
    /// The sound card went away, such as headphones unplugged. Load again at
    /// `position` to carry on through whatever the default device is now.
    DeviceLost { generation: u64, position: f64 },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EngineStatus {
    pub generation: u64,
    pub loading: bool,
    pub playing: bool,
    pub position: f64,
    pub duration: Option<f64>,
    /// How much of a streamed track has arrived, 0 to 1. Always 1 for a file.
    pub buffered: f32,
}

/// Where samples go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    /// The system's default sound device.
    Device,
    /// Pulled in real time and thrown away. For tests and machines with no sound.
    Null,
}

type Loaded = Result<(Box<dyn Source + Send>, Option<RemoteWatcher>), String>;

enum Command {
    Load {
        generation: u64,
        media: Media,
        start_at: f64,
        autoplay: bool,
    },
    Ready {
        generation: u64,
        loaded: Loaded,
    },
    Play,
    Pause,
    Seek(f64),
    Volume(f32),
    Stop {
        generation: u64,
    },
    Shutdown,
}

pub struct Engine {
    commands: Sender<Command>,
    status: Arc<Mutex<EngineStatus>>,
    generations: AtomicU64,
}

/// The audio thread keeps a sender of its own for loaders to reply on, so the
/// channel never closes by itself: it has to be told.
impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

impl Engine {
    /// Must be called from inside a Tokio runtime, which streams are downloaded on.
    pub fn start(api: Api, cache_dir: PathBuf, output: Output) -> (Self, Receiver<EngineEvent>) {
        let (commands, inbox) = mpsc::channel();
        let (events, outbox) = mpsc::channel();
        let status = Arc::new(Mutex::new(EngineStatus::default()));
        let worker = Worker {
            inbox,
            commands: commands.clone(),
            events,
            status: status.clone(),
            api,
            cache_dir,
            runtime: Handle::current(),
            output,
            device: None,
            device_lost: Arc::new(AtomicBool::new(false)),
            current: None,
            generation: 0,
            start_at: 0.0,
            want_playing: false,
            volume: 1.0,
        };
        thread::Builder::new()
            .name("cpz-audio".into())
            .spawn(move || worker.run())
            .expect("the OS can start a thread");
        let engine = Self {
            commands,
            status,
            generations: AtomicU64::new(0),
        };
        (engine, outbox)
    }

    fn send(&self, command: Command) {
        // Only fails once the audio thread is gone, when there is nothing to do.
        let _ = self.commands.send(command);
    }

    /// Starts opening a track, dropping whatever was playing. Events about it
    /// carry the returned generation, so the caller can ignore stale ones.
    pub fn load(&self, media: Media, start_at: f64, autoplay: bool) -> u64 {
        let generation = self.generations.fetch_add(1, Ordering::AcqRel) + 1;
        self.send(Command::Load {
            generation,
            media,
            start_at,
            autoplay,
        });
        generation
    }

    pub fn play(&self) {
        self.send(Command::Play);
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    pub fn seek(&self, seconds: f64) {
        self.send(Command::Seek(seconds.max(0.0)));
    }

    /// 0 to 1.
    pub fn set_volume(&self, volume: f32) {
        self.send(Command::Volume(volume.clamp(0.0, 1.0)));
    }

    pub fn stop(&self) {
        let generation = self.generations.fetch_add(1, Ordering::AcqRel) + 1;
        self.send(Command::Stop { generation });
    }

    pub fn status(&self) -> EngineStatus {
        self.status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

struct Current {
    player: Player,
    watcher: Option<RemoteWatcher>,
    finished: bool,
    /// For `Output::Null`, the thread standing in for a sound card.
    _pump: Option<Pump>,
}

struct Pump(Arc<AtomicBool>);

impl Drop for Pump {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Reads a player's output at the speed a sound card would.
fn pump(mut output: rodio::queue::SourcesQueueOutput) -> Pump {
    let done = Arc::new(AtomicBool::new(false));
    let flag = done.clone();
    thread::spawn(move || {
        while !flag.load(Ordering::Acquire) {
            let per_10ms =
                output.sample_rate().get() as usize * output.channels().get() as usize / 100;
            for _ in 0..per_10ms {
                output.next();
            }
            thread::sleep(Duration::from_millis(10));
        }
    });
    Pump(done)
}

struct Worker {
    inbox: Receiver<Command>,
    commands: Sender<Command>,
    events: Sender<EngineEvent>,
    status: Arc<Mutex<EngineStatus>>,
    api: Api,
    cache_dir: PathBuf,
    runtime: Handle,
    output: Output,
    device: Option<MixerDeviceSink>,
    device_lost: Arc<AtomicBool>,
    current: Option<Current>,
    generation: u64,
    start_at: f64,
    want_playing: bool,
    volume: f32,
}

fn open(media: Media, api: Api, cache_dir: PathBuf, runtime: Handle) -> Loaded {
    match media {
        Media::File(path) => {
            let file = File::open(&path).map_err(|err| format!("{}: {err}", path.display()))?;
            let len = file.metadata().map_err(|err| err.to_string())?.len();
            let decoder = Decoder::builder()
                .with_data(BufReader::new(file))
                .with_byte_len(len)
                .build()
                .map_err(|err| err.to_string())?;
            Ok((Box::new(decoder), None))
        }
        Media::Stream(track_id) => {
            let remote = runtime
                .block_on(Remote::open(api, track_id, &cache_dir))
                .map_err(|err| err.to_string())?;
            let watcher = remote.watcher();
            let len = remote.len();
            let mime = remote.mime().map(str::to_owned);
            let mut builder = Decoder::builder().with_data(remote).with_byte_len(len);
            if let Some(mime) = &mime {
                builder = builder.with_mime_type(mime);
            }
            let decoder = builder.build().map_err(|err| match watcher.failure() {
                Some(failure) => failure,
                None => err.to_string(),
            })?;
            Ok((Box::new(decoder), Some(watcher)))
        }
    }
}

impl Worker {
    fn run(mut self) {
        loop {
            match self.inbox.recv_timeout(TICK) {
                Ok(Command::Shutdown) => return,
                Ok(command) => self.handle(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.tick();
        }
    }

    fn emit(&self, event: EngineEvent) {
        let _ = self.events.send(event);
    }

    fn update(&self, change: impl FnOnce(&mut EngineStatus)) {
        change(
            &mut self
                .status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Load {
                generation,
                media,
                start_at,
                autoplay,
            } => {
                self.current = None;
                self.generation = generation;
                self.start_at = start_at;
                self.want_playing = autoplay;
                self.update(|status| {
                    *status = EngineStatus {
                        generation,
                        loading: true,
                        playing: autoplay,
                        position: start_at,
                        duration: None,
                        buffered: 0.0,
                    };
                });
                let (api, dir, runtime) = (
                    self.api.clone(),
                    self.cache_dir.clone(),
                    self.runtime.clone(),
                );
                let reply = self.commands.clone();
                thread::spawn(move || {
                    let loaded = open(media, api, dir, runtime);
                    let _ = reply.send(Command::Ready { generation, loaded });
                });
            }
            Command::Ready { generation, loaded } => {
                if generation != self.generation {
                    return;
                }
                match loaded {
                    Ok((source, watcher)) => self.begin(source, watcher),
                    Err(message) => {
                        self.update(|status| {
                            status.loading = false;
                            status.playing = false;
                        });
                        self.emit(EngineEvent::Failed {
                            generation,
                            message,
                        });
                    }
                }
            }
            Command::Play => {
                self.want_playing = true;
                if let Some(current) = &self.current {
                    current.player.play();
                }
                self.update(|status| status.playing = true);
            }
            Command::Pause => {
                self.want_playing = false;
                if let Some(current) = &self.current {
                    current.player.pause();
                }
                self.update(|status| status.playing = false);
            }
            Command::Seek(seconds) => {
                match &mut self.current {
                    Some(current) => {
                        if let Err(err) = current.player.try_seek(Duration::from_secs_f64(seconds))
                        {
                            tracing::warn!("seek failed: {err}");
                        }
                        current.finished = false;
                    }
                    // Still opening: start there instead.
                    None => self.start_at = seconds,
                }
                self.update(|status| status.position = seconds);
            }
            Command::Volume(volume) => {
                self.volume = volume;
                if let Some(current) = &self.current {
                    current.player.set_volume(volume);
                }
            }
            Command::Shutdown => {}
            Command::Stop { generation } => {
                self.generation = generation;
                self.current = None;
                self.want_playing = false;
                self.update(|status| {
                    *status = EngineStatus {
                        generation,
                        ..EngineStatus::default()
                    }
                });
            }
        }
    }

    fn device(&mut self) -> Result<&MixerDeviceSink, String> {
        if self.device_lost.swap(false, Ordering::AcqRel) {
            self.device = None;
        }
        if self.device.is_none() {
            let lost = self.device_lost.clone();
            let mut device = DeviceSinkBuilder::from_default_device()
                .map_err(|err| err.to_string())?
                .with_error_callback(move |err: rodio::CpalError| {
                    if matches!(
                        err.kind(),
                        CpalErrorKind::DeviceNotAvailable | CpalErrorKind::HostUnavailable
                    ) {
                        lost.store(true, Ordering::Release);
                    } else {
                        tracing::debug!("audio stream: {err}");
                    }
                })
                .open_stream()
                .map_err(|err| err.to_string())?;
            device.log_on_drop(false);
            self.device = Some(device);
        }
        Ok(self.device.as_ref().expect("opened just above"))
    }

    fn begin(&mut self, source: Box<dyn Source + Send>, watcher: Option<RemoteWatcher>) {
        let generation = self.generation;
        let duration = source.total_duration().map(|d| d.as_secs_f64());

        let (player, pump) = match self.output {
            Output::Null => {
                let (player, output) = Player::new();
                (player, Some(pump(output)))
            }
            Output::Device => match self.device() {
                Ok(device) => (Player::connect_new(device.mixer()), None),
                Err(message) => {
                    self.update(|status| {
                        status.loading = false;
                        status.playing = false;
                    });
                    self.emit(EngineEvent::Failed {
                        generation,
                        message: format!("no sound device: {message}"),
                    });
                    return;
                }
            },
        };

        player.set_volume(self.volume);
        if !self.want_playing {
            player.pause();
        }
        player.append(source);
        if self.start_at > 0.0
            && let Err(err) = player.try_seek(Duration::from_secs_f64(self.start_at))
        {
            tracing::warn!("could not start at {}s: {err}", self.start_at);
        }

        let buffered = if watcher.is_some() { 0.0 } else { 1.0 };
        self.current = Some(Current {
            player,
            watcher,
            finished: false,
            _pump: pump,
        });
        let playing = self.want_playing;
        self.update(|status| {
            status.loading = false;
            status.playing = playing;
            status.duration = duration;
            status.buffered = buffered;
        });
        self.emit(EngineEvent::Loaded {
            generation,
            duration,
        });
    }

    fn tick(&mut self) {
        let generation = self.generation;

        if self.output == Output::Device
            && self.device_lost.load(Ordering::Acquire)
            && self.current.is_some()
        {
            let position = self
                .current
                .as_ref()
                .map_or(0.0, |c| c.player.get_pos().as_secs_f64());
            self.current = None;
            self.device = None;
            self.device_lost.store(false, Ordering::Release);
            self.update(|status| status.loading = true);
            self.emit(EngineEvent::DeviceLost {
                generation,
                position,
            });
            return;
        }

        let Some(current) = &mut self.current else {
            return;
        };
        let position = current.player.get_pos().as_secs_f64();
        let buffered = current
            .watcher
            .as_ref()
            .map_or(1.0, RemoteWatcher::progress);

        let mut event = None;
        if current.player.empty() && !current.finished {
            current.finished = true;
            event = Some(
                match current.watcher.as_ref().and_then(RemoteWatcher::failure) {
                    Some(message) => EngineEvent::Failed {
                        generation,
                        message,
                    },
                    None => EngineEvent::Ended { generation },
                },
            );
        }
        let finished = current.finished;
        self.update(|status| {
            if !finished {
                status.position = position;
            }
            status.buffered = buffered;
            if finished {
                status.playing = false;
            }
        });
        if let Some(event) = event {
            self.emit(event);
        }
    }
}

//! Several devices, one of them playing. The server holds no playback state:
//! devices announce themselves through presence and talk over broadcast on the
//! private `player` channel.
//!
//! The active device is a pure function of presence: the highest claim time,
//! device id breaking ties. Every device works it out alike, there is no election
//! message to lose, and when the active device drops out of presence the next one
//! takes over by itself.
//!
//! The messages are the JSON the old web app used, which the server's jam
//! broadcasts still send.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use tokio::time::{MissedTickBehavior, interval};

use super::channel::{Channel, ChannelEvent};
use crate::http::now_ms;
use crate::player::{Command, Player, PlayerState};
use crate::queue::QueueView;
use crate::store::Store;
use crate::types::{PlayerTrack, RepeatMode};

pub const PLAYER_TOPIC: &str = "player";
/// How often the active device republishes its position while playing.
const STATE_EVERY: Duration = Duration::from_secs(1);
const MAX_NAME: usize = 40;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePresence {
    pub device_id: String,
    pub name: String,
    pub platform: String,
    /// When this device last claimed playback, in Unix milliseconds. Zero until
    /// it does.
    #[serde(default)]
    pub claimed_at: i64,
}

/// Highest claim wins; device id breaks ties so every device agrees.
pub fn elect_active(devices: &[DevicePresence]) -> Option<&DevicePresence> {
    devices.iter().min_by(|a, b| {
        b.claimed_at
            .cmp(&a.claimed_at)
            .then_with(|| a.device_id.cmp(&b.device_id))
    })
}

/// What the active device publishes for the remotes to show.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedState {
    pub track: Option<PlayerTrack>,
    pub queue: Vec<PlayerTrack>,
    pub index: usize,
    pub is_playing: bool,
    pub current_time: f64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub sent_at: i64,
}

impl SharedState {
    fn of(state: &PlayerState) -> Self {
        Self {
            track: state.current().cloned(),
            queue: state.queue.tracks.clone(),
            index: state.queue.index,
            is_playing: state.playing,
            current_time: state.position,
            shuffle: state.queue.shuffle,
            repeat: state.queue.repeat,
            sent_at: now_ms(),
        }
    }

    fn queue(&self) -> QueueView {
        QueueView {
            tracks: self.queue.clone(),
            index: self.index,
            shuffle: self.shuffle,
            repeat: self.repeat,
            order: Vec::new(),
        }
    }
}

/// Who this device is, kept between runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub id: String,
    pub name: String,
    pub platform: String,
}

impl Identity {
    /// The saved identity, or a new one named after the platform.
    pub fn load(store: &Store, platform: &str) -> Self {
        let id = store
            .get::<String>("device-id")
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                let id = uuid::Uuid::new_v4().to_string();
                let _ = store.set("device-id", &id);
                id
            });
        let name = store
            .get::<String>("device-name")
            .ok()
            .flatten()
            .unwrap_or_else(|| platform.to_owned());
        Self {
            id,
            name,
            platform: platform.to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HubState {
    pub me: String,
    pub devices: Vec<DevicePresence>,
    pub active_id: Option<String>,
    /// Whether the sound comes out of this device.
    pub is_active: bool,
    /// On the channel. Offline, a device plays on its own.
    pub connected: bool,
    /// What to show: this device's own player, or the active device's mirrored.
    pub player: PlayerState,
    /// Bumped when the jam's participants change, so a jam panel knows to reload.
    pub jam_version: u64,
}

enum Input {
    Command(Command),
    Claim,
    Rename(String),
}

/// The player, wrapped so that commands go to whichever device is playing.
pub struct Hub {
    inputs: mpsc::UnboundedSender<Input>,
    state: watch::Receiver<HubState>,
}

impl Hub {
    /// With no channel, the device simply plays on its own. Must be called inside
    /// a Tokio runtime.
    pub fn start(
        player: Player,
        channel: Option<(Channel, mpsc::UnboundedReceiver<ChannelEvent>)>,
        identity: Identity,
        store: std::sync::Arc<Store>,
    ) -> Self {
        let (inputs, inbox) = mpsc::unbounded_channel();
        let initial = HubState {
            me: identity.id.clone(),
            devices: Vec::new(),
            active_id: None,
            is_active: true,
            connected: false,
            player: player.state(),
            jam_version: 0,
        };
        let (publish, state) = watch::channel(initial);
        let (channel, events) = match channel {
            Some((channel, events)) => (Some(channel), Some(events)),
            None => (None, None),
        };
        let worker = Worker {
            player_state: player.subscribe(),
            player,
            channel,
            identity,
            store,
            claimed_at: 0,
            devices: Vec::new(),
            connected: false,
            mirror: None,
            last_sent: None,
            jam_version: 0,
            publish,
        };
        tokio::spawn(worker.run(inbox, events));
        Self { inputs, state }
    }

    pub fn send(&self, command: Command) {
        let _ = self.inputs.send(Input::Command(command));
    }

    /// Moves playback to this device.
    pub fn claim(&self) {
        let _ = self.inputs.send(Input::Claim);
    }

    /// Renames this device everywhere, including in other devices' pickers.
    pub fn rename(&self, name: &str) {
        let _ = self.inputs.send(Input::Rename(name.to_owned()));
    }

    pub fn state(&self) -> HubState {
        self.state.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<HubState> {
        self.state.clone()
    }
}

/// The parts of the player state that make remotes redraw at once rather than
/// waiting for the next tick.
#[derive(PartialEq)]
struct Shape {
    ids: Vec<i32>,
    index: usize,
    playing: bool,
    shuffle: bool,
    repeat: RepeatMode,
    position: f64,
}

impl Shape {
    fn of(state: &PlayerState) -> Self {
        Self {
            ids: state.queue.tracks.iter().map(|t| t.id).collect(),
            index: state.queue.index,
            playing: state.playing,
            shuffle: state.queue.shuffle,
            repeat: state.queue.repeat,
            position: state.position,
        }
    }

    /// Same queue and transport, and the position moved as time does: no seek.
    fn same_as(&self, other: &Self, elapsed: f64) -> bool {
        self.ids == other.ids
            && self.index == other.index
            && self.playing == other.playing
            && self.shuffle == other.shuffle
            && self.repeat == other.repeat
            && (self.position - (other.position + if other.playing { elapsed } else { 0.0 })).abs()
                < 2.0
    }
}

fn plays_here(command: &Command) -> bool {
    matches!(
        command,
        Command::Play { .. } | Command::Toggle | Command::SetQueue { .. } | Command::JumpTo { .. }
    )
}

struct Worker {
    player: Player,
    player_state: watch::Receiver<PlayerState>,
    channel: Option<Channel>,
    identity: Identity,
    store: std::sync::Arc<Store>,
    claimed_at: i64,
    devices: Vec<DevicePresence>,
    connected: bool,
    /// The active device's last published state, and when it arrived.
    mirror: Option<(SharedState, Instant)>,
    last_sent: Option<(Shape, Instant)>,
    jam_version: u64,
    publish: watch::Sender<HubState>,
}

impl Worker {
    async fn run(
        mut self,
        mut inbox: mpsc::UnboundedReceiver<Input>,
        mut events: Option<mpsc::UnboundedReceiver<ChannelEvent>>,
    ) {
        let mut tick = interval(STATE_EVERY);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                input = inbox.recv() => match input {
                    Some(input) => self.input(input),
                    None => return,
                },
                event = async { events.as_mut()?.recv().await }, if events.is_some() => match event {
                    Some(event) => self.channel_event(event),
                    None => events = None,
                },
                changed = self.player_state.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    self.player_changed();
                }
                _ = tick.tick() => self.tick(),
            }
            self.refresh();
        }
    }

    fn active_id(&self) -> Option<String> {
        elect_active(&self.devices).map(|device| device.device_id.clone())
    }

    /// Offline, or alone, a device plays for itself.
    fn is_active(&self) -> bool {
        !self.connected || self.active_id().is_none_or(|id| id == self.identity.id)
    }

    fn presence(&self) -> DevicePresence {
        DevicePresence {
            device_id: self.identity.id.clone(),
            name: self.identity.name.clone(),
            platform: self.identity.platform.clone(),
            claimed_at: self.claimed_at,
        }
    }

    fn track_presence(&mut self) {
        let me = self.presence();
        // Applied locally at once rather than waiting for the echo, so a claim
        // takes effect on this screen immediately.
        if self.connected {
            self.devices
                .retain(|device| device.device_id != me.device_id);
            self.devices.push(me.clone());
        }
        if let Some(channel) = &self.channel {
            channel.track(serde_json::to_value(&me).expect("presence serializes"));
        }
    }

    /// Runs `change`, then works out whether this device gained or lost playback.
    fn transition(&mut self, change: impl FnOnce(&mut Self)) {
        let was_active = self.is_active();
        change(self);
        let now_active = self.is_active();
        if was_active && !now_active {
            self.mirror = None;
            self.player.set_active(false);
        } else if !was_active && now_active {
            self.player.set_active(true);
            if let Some((shared, at)) = self.mirror.take() {
                // Carry on from where the other device was. When this device lost
                // its own connection, though, the other may still be playing, so
                // pick up the queue but stay quiet.
                let playing = shared.is_playing && self.connected;
                self.player
                    .adopt(shared.queue(), extrapolate(&shared, at), playing);
            }
        }
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::Command(command) => {
                // Volume belongs to the device in your hand.
                if self.is_active() || matches!(command, Command::SetVolume { .. }) {
                    if plays_here(&command) && self.claimed_at == 0 {
                        // First play on this device: claim it, so a device opened
                        // later with no claim of its own never takes over.
                        self.transition(|worker| {
                            worker.claimed_at = now_ms();
                            worker.track_presence();
                        });
                    }
                    self.player.send(command);
                } else if let (Some(channel), Some(target)) = (&self.channel, self.active_id()) {
                    channel.broadcast("cmd", json!({ "target": target, "command": command }));
                }
            }
            Input::Claim => self.transition(|worker| {
                worker.claimed_at = now_ms();
                worker.track_presence();
            }),
            Input::Rename(name) => {
                let name: String = name.trim().chars().take(MAX_NAME).collect();
                if name.is_empty() {
                    return;
                }
                let _ = self.store.set("device-name", &name);
                self.identity.name = name;
                self.track_presence();
            }
        }
    }

    fn channel_event(&mut self, event: ChannelEvent) {
        match event {
            ChannelEvent::Joined => {
                self.transition(|worker| {
                    worker.connected = true;
                    worker.devices.clear();
                });
                self.track_presence();
            }
            ChannelEvent::Disconnected { reason } => {
                tracing::debug!("player channel lost: {reason}");
                self.transition(|worker| {
                    worker.connected = false;
                    worker.devices.clear();
                });
            }
            ChannelEvent::Presence(entries) => {
                let mut devices: Vec<DevicePresence> = entries
                    .into_iter()
                    .filter_map(|entry| serde_json::from_value(entry).ok())
                    .collect();
                // Our own entry as we know it now, which may be ahead of the echo.
                devices.retain(|device| device.device_id != self.identity.id);
                devices.push(self.presence());
                devices.sort_by(|a, b| {
                    a.name
                        .cmp(&b.name)
                        .then_with(|| a.device_id.cmp(&b.device_id))
                });
                self.transition(|worker| worker.devices = devices);
            }
            ChannelEvent::Broadcast { event, payload } => self.broadcast(&event, payload),
        }
    }

    fn broadcast(&mut self, event: &str, payload: Value) {
        match event {
            // Only remotes mirror; the active device is the one producing it.
            "state" if !self.is_active() => {
                if let Ok(shared) = serde_json::from_value::<SharedState>(payload) {
                    self.mirror = Some((shared, Instant::now()));
                }
            }
            "cmd" if self.is_active() => {
                if payload["target"] != json!(self.identity.id) {
                    return;
                }
                match serde_json::from_value::<Command>(payload["command"].clone()) {
                    Ok(command) => self.player.send(command),
                    Err(err) => tracing::debug!("ignored a command: {err}"),
                }
            }
            // Sent by the server for a jam guest, who cannot broadcast. Play next,
            // not last: at a party the queue is long and the end never comes.
            "jam-add" if self.is_active() => {
                if let Ok(track) = serde_json::from_value::<PlayerTrack>(payload["track"].clone()) {
                    self.player.send(Command::PlayNext { track });
                }
            }
            "jam" => self.jam_version += 1,
            _ => {}
        }
    }

    fn send_state(&mut self) {
        let (Some(channel), true) = (&self.channel, self.connected) else {
            return;
        };
        let state = self.player_state.borrow().clone();
        channel.broadcast(
            "state",
            serde_json::to_value(SharedState::of(&state)).expect("state serializes"),
        );
        self.last_sent = Some((Shape::of(&state), Instant::now()));
    }

    fn player_changed(&mut self) {
        if !self.is_active() {
            return;
        }
        let shape = Shape::of(&self.player_state.borrow());
        let unchanged = self
            .last_sent
            .as_ref()
            .is_some_and(|(sent, at)| shape.same_as(sent, at.elapsed().as_secs_f64()));
        if !unchanged {
            self.send_state();
        }
    }

    fn tick(&mut self) {
        if self.is_active() && self.player_state.borrow().playing {
            self.send_state();
        }
    }

    /// What the UI sees.
    fn refresh(&mut self) {
        let local = self.player_state.borrow().clone();
        let player = match (&self.mirror, self.is_active()) {
            (Some((shared, at)), false) => PlayerState {
                queue: shared.queue(),
                playing: shared.is_playing,
                loading: false,
                position: extrapolate(shared, *at),
                duration: shared.track.as_ref().and_then(|t| t.duration),
                buffered: 1.0,
                volume: local.volume,
                local: false,
                error: None,
            },
            _ => local,
        };
        let state = HubState {
            me: self.identity.id.clone(),
            devices: self.devices.clone(),
            active_id: self.active_id(),
            is_active: self.is_active(),
            connected: self.connected,
            player,
            jam_version: self.jam_version,
        };
        self.publish.send_if_modified(|current| {
            let changed = *current != state;
            *current = state;
            changed
        });
    }
}

/// Where the active device has got to by now, timed from when its state arrived
/// rather than when it was sent, so two clocks that disagree do not matter.
fn extrapolate(shared: &SharedState, received: Instant) -> f64 {
    let position = if shared.is_playing {
        shared.current_time + received.elapsed().as_secs_f64()
    } else {
        shared.current_time
    };
    match shared.track.as_ref().and_then(|t| t.duration) {
        Some(duration) => position.min(duration),
        None => position,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, claimed_at: i64) -> DevicePresence {
        DevicePresence {
            device_id: id.into(),
            name: id.into(),
            platform: "Linux".into(),
            claimed_at,
        }
    }

    #[test]
    fn the_latest_claim_wins_and_ties_go_to_the_lowest_id() {
        assert!(elect_active(&[]).is_none());
        let devices = [device("b", 5), device("a", 9), device("c", 9)];
        assert_eq!(elect_active(&devices).unwrap().device_id, "a");
        let fresh = [device("z", 0), device("y", 0)];
        assert_eq!(elect_active(&fresh).unwrap().device_id, "y");
    }

    #[test]
    fn presence_and_state_match_the_web_app() {
        let web = json!({ "deviceId": "d", "name": "Browser", "platform": "Browser", "claimedAt": 17, "phx_ref": "x" });
        assert_eq!(
            serde_json::from_value::<DevicePresence>(web).unwrap(),
            DevicePresence {
                device_id: "d".into(),
                name: "Browser".into(),
                platform: "Browser".into(),
                claimed_at: 17,
            }
        );
        let state = SharedState::of(&PlayerState::default());
        let value = serde_json::to_value(&state).unwrap();
        for key in [
            "track",
            "queue",
            "index",
            "isPlaying",
            "currentTime",
            "shuffle",
            "repeat",
            "sentAt",
        ] {
            assert!(value.get(key).is_some(), "{key}");
        }
    }

    #[test]
    fn extrapolates_only_while_playing_and_never_past_the_end() {
        let mut shared = SharedState::of(&PlayerState::default());
        shared.current_time = 10.0;
        let earlier = Instant::now() - Duration::from_secs(3);
        assert!((extrapolate(&shared, earlier) - 10.0).abs() < 0.01);
        shared.is_playing = true;
        assert!((extrapolate(&shared, earlier) - 13.0).abs() < 0.1);
        shared.track = Some(PlayerTrack {
            id: 1,
            title: "t".into(),
            duration: Some(11.0),
            track_number: None,
            disc_number: 1,
            artist: crate::types::TrackArtist {
                id: 1,
                name: "a".into(),
            },
            album: crate::types::TrackAlbum {
                id: 1,
                title: "b".into(),
                has_artwork: false,
            },
        });
        assert!((extrapolate(&shared, earlier) - 11.0).abs() < 0.01);
    }

    #[test]
    fn a_seek_counts_as_a_change_but_time_passing_does_not() {
        let mut state = PlayerState {
            playing: true,
            position: 10.0,
            ..PlayerState::default()
        };
        let sent = Shape::of(&state);
        state.position = 11.0;
        assert!(Shape::of(&state).same_as(&sent, 1.0));
        state.position = 40.0;
        assert!(!Shape::of(&state).same_as(&sent, 1.0));
        state.position = 11.0;
        state.playing = false;
        assert!(!Shape::of(&state).same_as(&sent, 1.0));
    }
}

//! One Supabase Realtime channel over a websocket: the Phoenix protocol, version
//! 1.0.0 (JSON objects), with presence and broadcast.
//!
//! The connection looks after itself. It reconnects with backoff, re-announces
//! presence after every reconnect, keeps the socket's token fresh, and reports
//! what happened as events. Callers only ever send and listen.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::{Instant, MissedTickBehavior, interval, timeout};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, connect_async_tls_with_config};

use crate::auth::Credentials;
use crate::types::ClientConfig;

const HEARTBEAT: Duration = Duration::from_secs(25);
const TOKEN_CHECK: Duration = Duration::from_secs(60);
const JOIN_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A connection that lasted this long was healthy, so the next failure starts
/// the backoff from the bottom again.
const HEALTHY: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq)]
pub enum ChannelEvent {
    Joined,
    Disconnected {
        reason: String,
    },
    /// Everyone present now: the first presence entry under each key.
    Presence(Vec<Value>),
    Broadcast {
        event: String,
        payload: Value,
    },
}

enum Outgoing {
    Broadcast { event: String, payload: Value },
    Track(Value),
}

#[derive(Clone)]
pub struct Channel {
    outgoing: UnboundedSender<Outgoing>,
}

pub struct ChannelConfig {
    pub supabase: ClientConfig,
    /// Without the `realtime:` prefix, as the app names it.
    pub topic: String,
    pub presence_key: String,
}

impl Channel {
    /// Connects in the background; must be called inside a Tokio runtime. The
    /// connection closes once every `Channel` clone is dropped.
    pub fn spawn(
        config: ChannelConfig,
        credentials: Arc<Credentials>,
    ) -> (Self, UnboundedReceiver<ChannelEvent>) {
        let (outgoing, inbox) = mpsc::unbounded_channel();
        let (events, listener) = mpsc::unbounded_channel();
        tokio::spawn(run(config, credentials, inbox, events));
        (Self { outgoing }, listener)
    }

    /// Dropped while disconnected: a broadcast describes the moment it is sent.
    pub fn broadcast(&self, event: &str, payload: Value) {
        let _ = self.outgoing.send(Outgoing::Broadcast {
            event: event.to_owned(),
            payload,
        });
    }

    /// Replaces this client's presence. Kept and re-sent after a reconnect.
    pub fn track(&self, meta: Value) {
        let _ = self.outgoing.send(Outgoing::Track(meta));
    }
}

pub fn websocket_url(config: &ClientConfig) -> String {
    let base = config.supabase_url.trim_end_matches('/');
    let base = base
        .strip_prefix("https://")
        .map(|rest| format!("wss://{rest}"))
        .or_else(|| {
            base.strip_prefix("http://")
                .map(|rest| format!("ws://{rest}"))
        })
        .unwrap_or_else(|| base.to_owned());
    let key: String = url::form_urlencoded::byte_serialize(config.anon_key.as_bytes()).collect();
    format!("{base}/realtime/v1/websocket?apikey={key}&vsn=1.0.0")
}

async fn run(
    config: ChannelConfig,
    credentials: Arc<Credentials>,
    mut inbox: UnboundedReceiver<Outgoing>,
    events: UnboundedSender<ChannelEvent>,
) {
    let mut tracked: Option<Value> = None;
    let mut backoff = Duration::from_secs(1);

    loop {
        let started = Instant::now();
        let reason = match connect(&config, &credentials, &mut inbox, &events, &mut tracked).await {
            Ok(()) => return,
            Err(reason) => reason,
        };
        tracing::debug!("realtime disconnected: {reason}");
        if events.send(ChannelEvent::Disconnected { reason }).is_err() {
            return;
        }
        if started.elapsed() > HEALTHY {
            backoff = Duration::from_secs(1);
        }

        // Wait out the backoff, still taking presence updates so the latest one is
        // announced on reconnect, and noticing if the app has hung up.
        let wake = Instant::now() + backoff;
        loop {
            tokio::select! {
                () = tokio::time::sleep_until(wake) => break,
                next = inbox.recv() => match next {
                    Some(Outgoing::Track(meta)) => tracked = Some(meta),
                    Some(Outgoing::Broadcast { .. }) => {}
                    None => return,
                },
            }
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

struct Refs(u64);

impl Refs {
    fn next(&mut self) -> String {
        self.0 += 1;
        self.0.to_string()
    }
}

/// Phoenix presence, applied from `presence_state` and `presence_diff`.
#[derive(Default)]
struct Presence(BTreeMap<String, Vec<Value>>);

impl Presence {
    fn metas(entry: &Value) -> Vec<Value> {
        entry["metas"].as_array().cloned().unwrap_or_default()
    }

    fn replace(&mut self, state: &Value) {
        self.0.clear();
        if let Some(keys) = state.as_object() {
            for (key, entry) in keys {
                self.0.insert(key.clone(), Self::metas(entry));
            }
        }
    }

    fn apply_diff(&mut self, diff: &Value) {
        if let Some(joins) = diff["joins"].as_object() {
            for (key, entry) in joins {
                self.0
                    .entry(key.clone())
                    .or_default()
                    .extend(Self::metas(entry));
            }
        }
        if let Some(leaves) = diff["leaves"].as_object() {
            for (key, entry) in leaves {
                let gone: Vec<Value> = Self::metas(entry)
                    .iter()
                    .map(|meta| meta["phx_ref"].clone())
                    .collect();
                if let Some(metas) = self.0.get_mut(key) {
                    metas.retain(|meta| !gone.contains(&meta["phx_ref"]));
                    if metas.is_empty() {
                        self.0.remove(key);
                    }
                }
            }
        }
    }

    /// The newest entry per key: a device that re-tracks gets a fresh one, and the
    /// old one leaves a moment later.
    fn current(&self) -> Vec<Value> {
        self.0
            .values()
            .filter_map(|metas| metas.last().cloned())
            .collect()
    }
}

fn frame(
    topic: &str,
    event: &str,
    payload: Value,
    reference: Option<String>,
    join_ref: Option<&str>,
) -> Message {
    let mut message =
        json!({ "topic": topic, "event": event, "payload": payload, "ref": reference });
    if let Some(join_ref) = join_ref {
        message["join_ref"] = json!(join_ref);
    }
    Message::text(message.to_string())
}

/// One connection, start to finish. `Ok` means the app hung up; `Err` says why
/// the connection was lost.
async fn connect(
    config: &ChannelConfig,
    credentials: &Credentials,
    inbox: &mut UnboundedReceiver<Outgoing>,
    events: &UnboundedSender<ChannelEvent>,
    tracked: &mut Option<Value>,
) -> Result<(), String> {
    let mut token = credentials.access_token().await.ok_or("signed out")?;
    let url = websocket_url(&config.supabase);
    let connector = url
        .starts_with("wss://")
        .then(|| Connector::Rustls(Arc::new(crate::http::tls_config())));
    let (socket, _) = timeout(
        JOIN_TIMEOUT,
        connect_async_tls_with_config(url.as_str(), None, true, connector),
    )
    .await
    .map_err(|_| "timed out connecting".to_owned())?
    .map_err(|err| err.to_string())?;
    let (mut sink, mut stream) = socket.split();

    let topic = format!("realtime:{}", config.topic);
    let mut refs = Refs(0);
    let join_ref = refs.next();
    let join = json!({
        "config": {
            "broadcast": { "self": false, "ack": false },
            "presence": { "key": config.presence_key, "enabled": true },
            "postgres_changes": [],
            "private": true,
        },
        "access_token": token,
    });
    sink.send(frame(
        &topic,
        "phx_join",
        join,
        Some(join_ref.clone()),
        Some(&join_ref),
    ))
    .await
    .map_err(|err| err.to_string())?;

    // Nothing else is meaningful until the join is answered.
    timeout(JOIN_TIMEOUT, async {
        while let Some(message) = stream.next().await {
            let Ok(Message::Text(text)) = message else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if value["event"] == "phx_reply" && value["ref"] == json!(join_ref) {
                return match value["payload"]["status"].as_str() {
                    Some("ok") => Ok(()),
                    _ => Err(format!("join refused: {}", value["payload"]["response"])),
                };
            }
        }
        Err("closed while joining".to_owned())
    })
    .await
    .map_err(|_| "timed out joining".to_owned())??;

    let _ = events.send(ChannelEvent::Joined);
    let mut presence = Presence::default();

    let presence_frame = |meta: &Value, refs: &mut Refs| {
        frame(
            &topic,
            "presence",
            json!({ "type": "presence", "event": "track", "payload": meta }),
            Some(refs.next()),
            Some(&join_ref),
        )
    };
    if let Some(meta) = tracked.as_ref() {
        sink.send(presence_frame(meta, &mut refs))
            .await
            .map_err(|err| err.to_string())?;
    }

    let mut heartbeat = interval(HEARTBEAT);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    heartbeat.reset();
    let mut token_check = interval(TOKEN_CHECK);
    token_check.reset();
    let mut unanswered: Option<String> = None;

    loop {
        tokio::select! {
            message = stream.next() => {
                let text = match message {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Close(frame))) => return Err(format!("closed by server: {frame:?}")),
                    Some(Ok(_)) => continue,
                    Some(Err(err)) => return Err(err.to_string()),
                    None => return Err("connection closed".into()),
                };
                let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };
                let event = value["event"].as_str().unwrap_or_default();
                let payload = &value["payload"];

                if value["topic"] == "phoenix" {
                    if event == "phx_reply" && unanswered.as_deref().is_some_and(|r| value["ref"] == json!(r)) {
                        unanswered = None;
                    }
                    continue;
                }
                if value["topic"] != json!(topic) {
                    continue;
                }
                match event {
                    "broadcast" => {
                        let name = payload["event"].as_str().unwrap_or_default().to_owned();
                        let _ = events.send(ChannelEvent::Broadcast { event: name, payload: payload["payload"].clone() });
                    }
                    "presence_state" => {
                        presence.replace(payload);
                        let _ = events.send(ChannelEvent::Presence(presence.current()));
                    }
                    "presence_diff" => {
                        presence.apply_diff(payload);
                        let _ = events.send(ChannelEvent::Presence(presence.current()));
                    }
                    "phx_error" | "phx_close" => return Err(format!("channel {event}")),
                    "system" if payload["status"] == "error" => {
                        return Err(format!("realtime: {}", payload["message"]));
                    }
                    "phx_reply" if payload["status"] == "error" => {
                        tracing::warn!("realtime refused a message: {}", payload["response"]);
                    }
                    _ => {}
                }
            }
            next = inbox.recv() => {
                let message = match next {
                    None => {
                        let _ = sink.send(frame(&topic, "phx_leave", json!({}), Some(refs.next()), Some(&join_ref))).await;
                        let _ = sink.close().await;
                        return Ok(());
                    }
                    Some(Outgoing::Track(meta)) => {
                        let message = presence_frame(&meta, &mut refs);
                        *tracked = Some(meta);
                        message
                    }
                    Some(Outgoing::Broadcast { event, payload }) => frame(
                        &topic,
                        "broadcast",
                        json!({ "type": "broadcast", "event": event, "payload": payload }),
                        Some(refs.next()),
                        Some(&join_ref),
                    ),
                };
                sink.send(message).await.map_err(|err| err.to_string())?;
            }
            _ = heartbeat.tick() => {
                if unanswered.is_some() {
                    return Err("heartbeat went unanswered".into());
                }
                let reference = refs.next();
                sink.send(frame("phoenix", "heartbeat", json!({}), Some(reference.clone()), None))
                    .await
                    .map_err(|err| err.to_string())?;
                unanswered = Some(reference);
            }
            _ = token_check.tick() => {
                // Realtime drops a channel whose token expires, so hand it each
                // fresh one as the credentials refresh.
                let Some(fresh) = credentials.access_token().await else {
                    return Err("signed out".into());
                };
                if fresh != token {
                    sink.send(frame(&topic, "access_token", json!({ "access_token": fresh }), Some(refs.next()), Some(&join_ref)))
                        .await
                        .map_err(|err| err.to_string())?;
                    token = fresh;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_websocket_url() {
        let config = ClientConfig {
            supabase_url: "https://abc.supabase.co/".into(),
            anon_key: "a.b+c".into(),
            public_url: None,
        };
        assert_eq!(
            websocket_url(&config),
            "wss://abc.supabase.co/realtime/v1/websocket?apikey=a.b%2Bc&vsn=1.0.0"
        );
        let local = ClientConfig {
            supabase_url: "http://127.0.0.1:54321".into(),
            anon_key: "k".into(),
            public_url: None,
        };
        assert!(websocket_url(&local).starts_with("ws://127.0.0.1:54321/realtime/v1/websocket"));
    }

    #[test]
    fn follows_presence_state_and_diffs() {
        let mut presence = Presence::default();
        presence.replace(&json!({
            "a": { "metas": [{ "phx_ref": "1", "deviceId": "a" }] },
            "b": { "metas": [{ "phx_ref": "2", "deviceId": "b" }] },
        }));
        assert_eq!(presence.current().len(), 2);

        // A re-track: new entry joins, the old one leaves, newest wins meanwhile.
        presence.apply_diff(&json!({ "joins": { "a": { "metas": [{ "phx_ref": "3", "deviceId": "a", "name": "new" }] } }, "leaves": {} }));
        assert_eq!(presence.current()[0]["name"], "new");
        presence.apply_diff(
            &json!({ "joins": {}, "leaves": { "a": { "metas": [{ "phx_ref": "1" }] } } }),
        );
        assert_eq!(presence.current()[0]["name"], "new");

        presence.apply_diff(
            &json!({ "joins": {}, "leaves": { "b": { "metas": [{ "phx_ref": "2" }] } } }),
        );
        assert_eq!(presence.current().len(), 1);
        assert!(!presence.0.contains_key("b"));
    }
}

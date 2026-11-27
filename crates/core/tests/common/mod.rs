//! A stand-in for the Cpz Music server and for Supabase Realtime, for tests
//! that need the network but not the real thing.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use cpz_core::api::Api;
use cpz_core::audio::Output;
use cpz_core::auth::{Credentials, Session, SupabaseAuth};
use cpz_core::downloads::Downloads;
use cpz_core::listens::Listens;
use cpz_core::player::{Player, PlayerDeps, PlayerState};
use cpz_core::store::Store;
use cpz_core::types::{ClientConfig, PlayerTrack, TrackAlbum, TrackArtist};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

/// The only token the fake Realtime accepts.
pub const TOKEN: &str = "token";

/// A mono 8-bit WAV of `secs` seconds whose samples are a recognisable ramp.
pub fn wav(secs: f32) -> Vec<u8> {
    let rate: u32 = 8000;
    let data: Vec<u8> = (0..(rate as f32 * secs) as usize)
        .map(|i| (i % 251) as u8)
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&8u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

#[derive(Default)]
pub struct Fake {
    pub files: Mutex<Vec<Vec<u8>>>,
    /// Sleep this long between 16 KiB chunks, to make the download lag.
    pub throttle: Mutex<Duration>,
    pub requests: AtomicUsize,
    pub range_starts: Mutex<Vec<u64>>,
    pub listens: Mutex<Vec<serde_json::Value>>,
    pub now_playing: AtomicUsize,
    pub realtime: Realtime,
}

async fn stream(
    State(fake): State<Arc<Fake>>,
    Path(id): Path<usize>,
    headers: HeaderMap,
) -> Response {
    fake.requests.fetch_add(1, Ordering::SeqCst);
    let Some(file) = fake.files.lock().unwrap().get(id).cloned() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let len = file.len() as u64;
    let start: u64 = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes="))
        .and_then(|v| v.trim_end_matches('-').parse().ok())
        .unwrap_or(0);
    fake.range_starts.lock().unwrap().push(start);

    let throttle = *fake.throttle.lock().unwrap();
    let chunks: Vec<Vec<u8>> = file[start as usize..]
        .chunks(16 * 1024)
        .map(<[u8]>::to_vec)
        .collect();
    let body = Body::from_stream(futures_util::stream::unfold(
        chunks.into_iter(),
        move |mut rest| async move {
            let chunk = rest.next()?;
            if !throttle.is_zero() {
                tokio::time::sleep(throttle).await;
            }
            Some((Ok::<_, std::io::Error>(chunk), rest))
        },
    ));
    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, "audio/wav")
        .header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{len}", len - 1),
        )
        .header(header::CONTENT_LENGTH, len - start)
        .body(body)
        .unwrap()
}

async fn listen(State(fake): State<Arc<Fake>>, body: axum::Json<serde_json::Value>) -> StatusCode {
    fake.listens.lock().unwrap().push(body.0);
    StatusCode::NO_CONTENT
}

async fn now_playing(State(fake): State<Arc<Fake>>) -> StatusCode {
    fake.now_playing.fetch_add(1, Ordering::SeqCst);
    StatusCode::NO_CONTENT
}

pub async fn serve(files: Vec<Vec<u8>>) -> (Arc<Fake>, Api) {
    let fake = Arc::new(Fake::default());
    *fake.files.lock().unwrap() = files;
    let router = Router::new()
        .route("/api/tracks/{id}/stream", get(stream))
        .route("/api/stats/listen", post(listen))
        .route("/api/now-playing", post(now_playing))
        .route("/realtime/v1/websocket", get(realtime))
        .with_state(fake.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let api = client(&url, TOKEN);
    (fake, api)
}

/// A client of the fake server, signed in with `token`. Realtime goes to the
/// same fake.
pub fn client(url: &str, token: &str) -> Api {
    let http = cpz_core::http::client();
    let config = ClientConfig {
        supabase_url: url.to_owned(),
        anon_key: "anon".into(),
    };
    let session = Session {
        access_token: token.into(),
        refresh_token: "refresh".into(),
        expires_at: i64::MAX / 2,
    };
    let credentials = Credentials::new(
        SupabaseAuth::new(http.clone(), config),
        Some(session),
        Box::new(|_| {}),
    );
    Api::new(http, url, credentials)
}

pub fn track(id: i32) -> PlayerTrack {
    PlayerTrack {
        id,
        title: format!("Track {id}"),
        duration: Some(1.0),
        track_number: Some(id),
        disc_number: 1,
        artist: TrackArtist {
            id: 1,
            name: "A".into(),
        },
        album: TrackAlbum {
            id: 1,
            title: "B".into(),
            has_artwork: false,
        },
    }
}

pub async fn wait_until(
    player: &Player,
    secs: u64,
    check: impl Fn(&PlayerState) -> bool,
) -> PlayerState {
    let mut watch = player.subscribe();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        let state = watch.borrow_and_update().clone();
        if check(&state) {
            return state;
        }
        if tokio::time::timeout_at(deadline, watch.changed())
            .await
            .is_err()
        {
            panic!("timed out; last state: {state:?}");
        }
    }
}

pub fn deps(api: &Api, store: &Arc<Store>, dir: &std::path::Path) -> PlayerDeps {
    PlayerDeps {
        api: api.clone(),
        store: store.clone(),
        downloads: Arc::new(Downloads::new(store.clone(), dir.join("downloads"))),
        listens: Arc::new(Listens::new(store.clone())),
        cache_dir: dir.join("cache"),
        output: Output::Null,
    }
}

/// Just enough of Supabase Realtime: Phoenix joins checked against `TOKEN`,
/// presence with state and diffs, broadcasts to everyone but the sender, and
/// heartbeats.
#[derive(Default)]
pub struct Realtime {
    clients: Mutex<HashMap<u64, Client>>,
    ids: AtomicU64,
    refs: AtomicU64,
    /// Every broadcast a client sent, as `(event, payload)`.
    pub broadcasts: Mutex<Vec<(String, Value)>>,
    pub refused: AtomicUsize,
}

struct Client {
    send: mpsc::UnboundedSender<String>,
    topic: Option<String>,
    key: String,
    meta: Option<Value>,
}

fn message(topic: &str, event: &str, payload: Value, reference: &Value) -> String {
    json!({ "topic": topic, "event": event, "payload": payload, "ref": reference }).to_string()
}

impl Realtime {
    fn presence_entry(key: &str, meta: &Value) -> Value {
        json!({ key: { "metas": [meta] } })
    }

    /// Pushes a broadcast from the server itself, as the jam endpoints do.
    pub fn server_broadcast(&self, event: &str, payload: Value) {
        for client in self.clients.lock().unwrap().values() {
            if let Some(topic) = &client.topic {
                let body = json!({ "type": "broadcast", "event": event, "payload": payload });
                let _ = client
                    .send
                    .send(message(topic, "broadcast", body, &Value::Null));
            }
        }
    }

    /// Drops one device's connection, as its network blipping would.
    pub fn disconnect(&self, key: &str) {
        let ids: Vec<u64> = self
            .clients
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, client)| client.key == key)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.leave(id);
        }
    }

    pub fn connected(&self) -> usize {
        self.clients
            .lock()
            .unwrap()
            .values()
            .filter(|c| c.topic.is_some())
            .count()
    }

    fn leave(&self, id: u64) {
        let mut clients = self.clients.lock().unwrap();
        let Some(gone) = clients.remove(&id) else {
            return;
        };
        if let (Some(topic), Some(meta)) = (gone.topic, gone.meta) {
            let diff = json!({ "joins": {}, "leaves": Self::presence_entry(&gone.key, &meta) });
            for client in clients.values() {
                let _ =
                    client
                        .send
                        .send(message(&topic, "presence_diff", diff.clone(), &Value::Null));
            }
        }
    }

    fn handle(&self, id: u64, text: &str) {
        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return;
        };
        let topic = frame["topic"].as_str().unwrap_or_default().to_owned();
        let reference = frame["ref"].clone();
        let payload = &frame["payload"];
        let mut clients = self.clients.lock().unwrap();

        match frame["event"].as_str().unwrap_or_default() {
            "heartbeat" => {
                let reply = message(
                    "phoenix",
                    "phx_reply",
                    json!({ "status": "ok", "response": {} }),
                    &reference,
                );
                let _ = clients[&id].send.send(reply);
            }
            "phx_join" => {
                if payload["access_token"] != TOKEN || payload["config"]["private"] != true {
                    self.refused.fetch_add(1, Ordering::SeqCst);
                    let reply =
                        json!({ "status": "error", "response": { "reason": "Unauthorized" } });
                    let _ = clients[&id]
                        .send
                        .send(message(&topic, "phx_reply", reply, &reference));
                    return;
                }
                let state: serde_json::Map<String, Value> = clients
                    .values()
                    .filter(|c| c.topic.as_deref() == Some(&topic))
                    .filter_map(|c| Some((c.key.clone(), json!({ "metas": [c.meta.clone()?] }))))
                    .collect();
                let client = clients.get_mut(&id).unwrap();
                client.topic = Some(topic.clone());
                client.key = payload["config"]["presence"]["key"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let _ = client.send.send(message(
                    &topic,
                    "phx_reply",
                    json!({ "status": "ok", "response": {} }),
                    &reference,
                ));
                let _ = client.send.send(message(
                    &topic,
                    "presence_state",
                    Value::Object(state),
                    &Value::Null,
                ));
            }
            "presence" => {
                let mut meta = payload["payload"].clone();
                meta["phx_ref"] = json!(self.refs.fetch_add(1, Ordering::SeqCst).to_string());
                let client = clients.get_mut(&id).unwrap();
                let key = client.key.clone();
                let leaves = client
                    .meta
                    .replace(meta.clone())
                    .map_or(json!({}), |old| Self::presence_entry(&key, &old));
                let diff = json!({ "joins": Self::presence_entry(&key, &meta), "leaves": leaves });
                for client in clients
                    .values()
                    .filter(|c| c.topic.as_deref() == Some(&topic))
                {
                    let _ = client.send.send(message(
                        &topic,
                        "presence_diff",
                        diff.clone(),
                        &Value::Null,
                    ));
                }
            }
            "broadcast" => {
                self.broadcasts.lock().unwrap().push((
                    payload["event"].as_str().unwrap_or_default().to_owned(),
                    payload["payload"].clone(),
                ));
                for (other, client) in clients.iter() {
                    if *other != id && client.topic.as_deref() == Some(&topic) {
                        let _ = client.send.send(message(
                            &topic,
                            "broadcast",
                            payload.clone(),
                            &Value::Null,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}

async fn realtime(State(fake): State<Arc<Fake>>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| connection(fake, socket))
}

async fn connection(fake: Arc<Fake>, socket: WebSocket) {
    let realtime = &fake.realtime;
    let id = realtime.ids.fetch_add(1, Ordering::SeqCst);
    let (send, mut outbox) = mpsc::unbounded_channel::<String>();
    realtime.clients.lock().unwrap().insert(
        id,
        Client {
            send,
            topic: None,
            key: String::new(),
            meta: None,
        },
    );
    let (mut sink, mut stream) = socket.split();

    loop {
        tokio::select! {
            out = outbox.recv() => match out {
                Some(text) => if sink.send(Message::Text(text.into())).await.is_err() { break },
                // Dropped by `disconnect_all`.
                None => break,
            },
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => realtime.handle(id, &text),
                Some(Ok(_)) => {}
                _ => break,
            },
        }
    }
    let _ = sink.close().await;
    realtime.leave(id);
}

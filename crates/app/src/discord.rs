//! Discord status: "Listening to …" while a song plays.
//!
//! Discord's local IPC is length-prefixed JSON over a Unix socket (a named pipe
//! on Windows): an op code and a length, both little-endian u32, then the JSON.
//! Small enough that a crate for it would be more to audit than the protocol.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, watch};

/// The Discord application whose name and icons the status shows.
const CLIENT_ID: &str = "1493213988664774796";
/// Discord may simply not be running; look again this often.
const RETRY: Duration = Duration::from_secs(15);
/// Discord's frames are a few kilobytes; anything near this is not Discord.
const MAX_FRAME: usize = 1 << 20;

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;

#[derive(Clone, Debug, PartialEq)]
pub struct Presence {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// The cover's URL. Discord only fetches https, so anything else shows the
    /// app's own icon instead.
    pub artwork: String,
}

trait Pipe: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Pipe for T {}

/// Keeps Discord showing whatever the returned sender last held. `None`
/// clears the status. Stops when the sender is dropped.
pub fn spawn() -> watch::Sender<Option<Presence>> {
    let client_id = std::env::var("CPZ_DISCORD_CLIENT_ID").unwrap_or_else(|_| CLIENT_ID.into());
    let (sender, receiver) = watch::channel(None);
    tokio::spawn(run(client_id, receiver));
    sender
}

async fn run(client_id: String, mut wanted: watch::Receiver<Option<Presence>>) {
    loop {
        // No point holding a connection open with nothing to show.
        while wanted.borrow_and_update().is_none() {
            if wanted.changed().await.is_err() {
                return;
            }
        }
        match connect().await {
            Some(pipe) => {
                if let Err(err) = session(pipe, &client_id, &mut wanted).await {
                    tracing::debug!("discord: {err}");
                }
            }
            None => tracing::debug!("discord isn't running"),
        }
        if wanted.has_changed().is_err() {
            return;
        }
        tokio::time::sleep(RETRY).await;
    }
}

#[cfg(unix)]
async fn connect() -> Option<Box<dyn Pipe>> {
    let base = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .unwrap_or_else(|| "/tmp".into());
    // Discord numbers its sockets when several clients run side by side, and
    // the Flatpak and Snap builds keep theirs in a folder of their own.
    for dir in ["", "app/com.discordapp.Discord/", "snap.discord/"] {
        for i in 0..10 {
            let path = format!("{base}/{dir}discord-ipc-{i}");
            if let Ok(stream) = tokio::net::UnixStream::connect(&path).await {
                return Some(Box::new(stream));
            }
        }
    }
    None
}

#[cfg(windows)]
async fn connect() -> Option<Box<dyn Pipe>> {
    use tokio::net::windows::named_pipe::ClientOptions;
    for i in 0..10 {
        if let Ok(pipe) = ClientOptions::new().open(format!(r"\\?\pipe\discord-ipc-{i}")) {
            return Some(Box::new(pipe));
        }
    }
    None
}

fn encode(op: u32, payload: &Value) -> Vec<u8> {
    let data = payload.to_string().into_bytes();
    let mut frame = Vec::with_capacity(8 + data.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
    frame.extend_from_slice(&data);
    frame
}

async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> std::io::Result<(u32, Value)> {
    let mut header = [0u8; 8];
    reader.read_exact(&mut header).await?;
    let op = u32::from_le_bytes(header[..4].try_into().expect("four bytes"));
    let len = u32::from_le_bytes(header[4..].try_into().expect("four bytes")) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::other("frame too large"));
    }
    let mut data = vec![0u8; len];
    reader.read_exact(&mut data).await?;
    Ok((op, serde_json::from_slice(&data).unwrap_or(Value::Null)))
}

fn activity(presence: Option<&Presence>) -> Value {
    let Some(p) = presence else {
        return Value::Null;
    };
    let cap = |text: &str| text.chars().take(128).collect::<String>();
    let mut activity = json!({
        // 2 is "Listening to".
        "type": 2,
        "name": cap(if p.artist.is_empty() { "Cpz Music" } else { &p.artist }),
        "details": cap(&p.title),
        "assets": {
            "large_image": if p.artwork.starts_with("https://") { p.artwork.as_str() } else { "icon" },
            "large_text": cap(if p.album.is_empty() { &p.title } else { &p.album }),
            "small_image": "play",
            "small_text": "Playing",
        },
        "instance": false,
    });
    if !p.artist.is_empty() {
        activity["state"] = json!(cap(&format!("by {}", p.artist)));
    }
    activity
}

/// One connection, from the handshake until Discord goes away or nothing is
/// wanted any more.
async fn session(
    pipe: Box<dyn Pipe>,
    client_id: &str,
    wanted: &mut watch::Receiver<Option<Presence>>,
) -> std::io::Result<()> {
    let (mut reader, mut writer) = tokio::io::split(pipe);
    writer
        .write_all(&encode(
            OP_HANDSHAKE,
            &json!({ "v": 1, "client_id": client_id }),
        ))
        .await?;

    // Reading a frame is not safe to cancel halfway, so it gets a task of its
    // own instead of a branch in the `select!` below.
    let (frames, mut incoming) = mpsc::channel(8);
    let reading = tokio::spawn(async move {
        while let Ok(frame) = read_frame(&mut reader).await {
            if frames.send(frame).await.is_err() {
                break;
            }
        }
    });
    let _stop_reading = AbortOnDrop(reading);

    let mut ready = false;
    let mut nonce = 0u64;
    loop {
        let send = tokio::select! {
            frame = incoming.recv() => match frame {
                None => return Err(std::io::Error::other("discord closed the connection")),
                Some((OP_CLOSE, _)) => return Ok(()),
                Some((OP_PING, payload)) => {
                    writer.write_all(&encode(OP_PONG, &payload)).await?;
                    false
                }
                Some((OP_FRAME, message)) if message["evt"] == "READY" => {
                    ready = true;
                    true
                }
                Some(_) => false,
            },
            changed = wanted.changed() => {
                if changed.is_err() {
                    return Ok(());
                }
                ready
            }
        };
        if send {
            nonce += 1;
            let presence = wanted.borrow_and_update().clone();
            let message = json!({
                "cmd": "SET_ACTIVITY",
                "args": { "pid": std::process::id(), "activity": activity(presence.as_ref()) },
                "nonce": nonce.to_string(),
            });
            writer.write_all(&encode(OP_FRAME, &message)).await?;
        }
    }
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_round_trip() {
        let payload = json!({ "cmd": "DISPATCH", "evt": "READY" });
        let mut bytes = encode(OP_FRAME, &payload);
        bytes.extend(encode(OP_PING, &json!({ "n": 1 })));
        let mut reader = bytes.as_slice();
        assert_eq!(read_frame(&mut reader).await.unwrap(), (OP_FRAME, payload));
        assert_eq!(read_frame(&mut reader).await.unwrap().0, OP_PING);
        assert!(read_frame(&mut reader).await.is_err());
    }

    #[test]
    fn falls_back_to_the_app_icon_off_https() {
        let presence = Presence {
            title: "Song".into(),
            artist: "".into(),
            album: "".into(),
            artwork: "http://pi:3006/api/artwork/1".into(),
        };
        let activity = activity(Some(&presence));
        assert_eq!(activity["assets"]["large_image"], "icon");
        assert_eq!(activity["name"], "Cpz Music");
        assert_eq!(activity["assets"]["large_text"], "Song");
        assert!(activity.get("state").is_none());
        assert_eq!(super::activity(None), Value::Null);
    }
}

//! A track on the server, readable and seekable as if it were a local file.
//!
//! rodio decodes on the audio thread, so a decoder reading straight off the
//! network would stall the sound card on every slow packet. Instead a background
//! task downloads into a temporary file and the decoder reads from that, only
//! waiting when the bytes it needs have genuinely not arrived yet.
//!
//! Seeking past what has arrived restarts the download at the new spot; once it
//! reaches the end, the gaps left behind are filled in. Formats that keep their
//! index at the end of the file (some MP4s) work the same way.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use reqwest::{Method, StatusCode};
use tokio::runtime::Handle;

use crate::api::{Api, ApiError};

/// A read this far ahead of the download is left to the download to reach; any
/// further and the download jumps there instead.
const JUMP_AHEAD: u64 = 512 * 1024;
/// How long a read waits for missing bytes before giving up on the track.
const READ_TIMEOUT: Duration = Duration::from_secs(20);
const RETRIES: u32 = 4;

/// Sorted, non-overlapping, non-adjacent byte ranges, end exclusive.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RangeSet(Vec<(u64, u64)>);

impl RangeSet {
    pub fn insert(&mut self, start: u64, end: u64) {
        if start >= end {
            return;
        }
        let (mut start, mut end) = (start, end);
        let mut kept = Vec::with_capacity(self.0.len() + 1);
        let mut placed = false;
        for &(s, e) in &self.0 {
            if e < start {
                kept.push((s, e));
            } else if s > end {
                if !placed {
                    kept.push((start, end));
                    placed = true;
                }
                kept.push((s, e));
            } else {
                start = start.min(s);
                end = end.max(e);
            }
        }
        if !placed {
            kept.push((start, end));
        }
        self.0 = kept;
    }

    /// The end of the range holding `at`, if it is held.
    pub fn end_of(&self, at: u64) -> Option<u64> {
        self.0
            .iter()
            .find(|&&(s, e)| s <= at && at < e)
            .map(|&(_, e)| e)
    }

    /// The first byte not held, from `from` on and below `len`.
    pub fn first_gap(&self, from: u64, len: u64) -> Option<u64> {
        let mut at = from;
        for &(s, e) in &self.0 {
            if e <= at {
                continue;
            }
            if s > at {
                break;
            }
            at = e;
        }
        (at < len).then_some(at)
    }

    pub fn covers(&self, len: u64) -> bool {
        self.first_gap(0, len).is_none()
    }
}

#[derive(Default)]
struct State {
    have: RangeSet,
    /// Where the download is writing now.
    head: u64,
    /// A read is waiting at this offset, far from the head.
    wanted: Option<u64>,
    error: Option<String>,
    closed: bool,
}

struct Shared {
    len: u64,
    file: File,
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The reading end. Dropping it stops the download and deletes the file.
pub struct Remote {
    shared: Arc<Shared>,
    pos: u64,
    mime: Option<String>,
}

fn total_length(response: &reqwest::Response) -> Option<u64> {
    let headers = response.headers();
    if let Some(range) = headers.get(CONTENT_RANGE).and_then(|v| v.to_str().ok()) {
        return range.rsplit('/').next()?.parse().ok();
    }
    headers.get(CONTENT_LENGTH)?.to_str().ok()?.parse().ok()
}

async fn request_from(api: &Api, path: &str, from: u64) -> Result<reqwest::Response, ApiError> {
    let response = api
        .request(Method::GET, path)
        .await?
        .header(RANGE, format!("bytes={from}-"))
        .send()
        .await?;
    match response.status() {
        StatusCode::OK if from == 0 => Ok(response),
        StatusCode::PARTIAL_CONTENT => Ok(response),
        status => Err(ApiError::Status(status)),
    }
}

impl Remote {
    /// Starts downloading a track and returns once its length is known. Temporary
    /// files go in `cache_dir`; Android has no usable system temp directory.
    pub async fn open(api: Api, track_id: i32, cache_dir: &Path) -> Result<Self, ApiError> {
        let path = format!("/api/tracks/{track_id}/stream");
        let response = request_from(&api, &path, 0).await?;
        let len = total_length(&response).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "the server sent no length")
        })?;
        let mime = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        std::fs::create_dir_all(cache_dir)?;
        // Unlinked on Unix and delete-on-close on Windows, so nothing is left
        // behind even if the app is killed mid-track.
        let file = tempfile::tempfile_in(cache_dir)?;
        file.set_len(len)?;
        let shared = Arc::new(Shared {
            len,
            file,
            state: Mutex::new(State::default()),
            changed: Condvar::new(),
        });

        Handle::current().spawn(download(api, path, shared.clone(), response));
        Ok(Self {
            shared,
            pos: 0,
            mime,
        })
    }

    pub fn len(&self) -> u64 {
        self.shared.len
    }

    pub fn is_empty(&self) -> bool {
        self.shared.len == 0
    }

    pub fn mime(&self) -> Option<&str> {
        self.mime.as_deref()
    }

    /// Whether the download failed for good, as opposed to the track ending.
    pub fn failure(&self) -> Option<String> {
        self.shared.state().error.clone()
    }

    /// A handle that can tell whether the download failed after the reader has
    /// been given away to the decoder.
    pub fn watcher(&self) -> RemoteWatcher {
        RemoteWatcher(self.shared.clone())
    }

    /// Blocks until `at` is downloaded or the wait times out, returning how many
    /// bytes from `at` on can be read without waiting.
    fn wait_for(&self, at: u64) -> io::Result<u64> {
        let deadline = Instant::now() + READ_TIMEOUT;
        let mut state = self.shared.state();
        loop {
            if let Some(end) = state.have.end_of(at) {
                return Ok(end - at);
            }
            if let Some(error) = &state.error {
                return Err(io::Error::other(error.clone()));
            }
            let far = at < state.head || at > state.head + JUMP_AHEAD;
            if far && state.wanted != Some(at) {
                state.wanted = Some(at);
                self.shared.changed.notify_all();
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the track stopped arriving",
                ));
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }
}

pub struct RemoteWatcher(Arc<Shared>);

impl RemoteWatcher {
    pub fn failure(&self) -> Option<String> {
        self.0.state().error.clone()
    }

    /// Fraction of the file downloaded, for a buffering bar.
    pub fn progress(&self) -> f32 {
        let state = self.0.state();
        let have: u64 = state.have.0.iter().map(|(s, e)| e - s).sum();
        if self.0.len == 0 {
            1.0
        } else {
            have as f32 / self.0.len as f32
        }
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        self.shared.state().closed = true;
        self.shared.changed.notify_all();
    }
}

impl Read for Remote {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.shared.len || buf.is_empty() {
            return Ok(0);
        }
        let available = self.wait_for(self.pos)?;
        let n = (available.min(buf.len() as u64)) as usize;
        let n = read_at(&self.shared.file, &mut buf[..n], self.pos)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Remote {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(delta) => self.shared.len.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
        };
        self.pos = target
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before the start"))?;
        Ok(self.pos)
    }
}

#[cfg(unix)]
fn read_at(file: &File, buf: &mut [u8], at: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buf, at)
}

#[cfg(windows)]
fn read_at(file: &File, buf: &mut [u8], at: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buf, at)
}

#[cfg(unix)]
fn write_at(file: &File, buf: &[u8], at: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, buf, at)
}

#[cfg(windows)]
fn write_at(file: &File, mut buf: &[u8], mut at: u64) -> io::Result<()> {
    while !buf.is_empty() {
        let n = std::os::windows::fs::FileExt::seek_write(file, buf, at)?;
        buf = &buf[n..];
        at += n as u64;
    }
    Ok(())
}

enum Pass {
    /// Got to the end of what this response carries.
    Finished,
    /// A reader wants somewhere else; start again from there.
    Jump(u64),
    Closed,
}

/// Writes one response into the file until it ends, a reader asks to jump, or the
/// reader goes away.
async fn drain(shared: &Shared, response: reqwest::Response, from: u64) -> Result<Pass, String> {
    let mut at = from;
    shared.state().head = at;
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|err| err.to_string())?;
        // Never past the length the server first gave.
        let take = (chunk.len() as u64).min(shared.len.saturating_sub(at)) as usize;
        write_at(&shared.file, &chunk[..take], at).map_err(|err| err.to_string())?;
        let end = at + take as u64;

        let mut state = shared.state();
        state.have.insert(at, end);
        state.head = end;
        at = end;
        shared.changed.notify_all();
        if state.closed {
            return Ok(Pass::Closed);
        }
        if let Some(wanted) = state.wanted.take()
            && state.have.end_of(wanted).is_none()
            && (wanted < at || wanted > at + JUMP_AHEAD)
        {
            return Ok(Pass::Jump(wanted));
        }
        if at >= shared.len {
            break;
        }
    }
    Ok(Pass::Finished)
}

async fn download(api: Api, path: String, shared: Arc<Shared>, first: reqwest::Response) {
    let mut response = Some(first);
    let mut from = 0;
    let mut failures = 0;

    loop {
        let current = match response.take() {
            Some(current) => Ok(current),
            None => request_from(&api, &path, from)
                .await
                .map_err(|err| err.to_string()),
        };
        let pass = match current {
            Ok(current) => drain(&shared, current, from).await,
            Err(err) => Err(err),
        };

        let next = {
            let mut state = shared.state();
            if state.closed {
                return;
            }
            match pass {
                Ok(Pass::Closed) => return,
                Ok(Pass::Jump(to)) => {
                    failures = 0;
                    Some(to)
                }
                // A response that carried nothing would otherwise be asked for again
                // and again.
                Ok(Pass::Finished) if state.head == from && from < shared.len => {
                    failures += 1;
                    if failures > RETRIES {
                        state.error = Some("the server stopped sending the track".into());
                        shared.changed.notify_all();
                        return;
                    }
                    Some(from)
                }
                Ok(Pass::Finished) => {
                    failures = 0;
                    // Fill what an earlier jump skipped, starting where a reader is
                    // waiting if one is.
                    let wanted = state
                        .wanted
                        .take()
                        .filter(|&w| state.have.end_of(w).is_none());
                    wanted.or_else(|| state.have.first_gap(0, shared.len))
                }
                Err(err) => {
                    failures += 1;
                    if failures > RETRIES {
                        tracing::warn!("giving up on {path}: {err}");
                        state.error = Some(err);
                        shared.changed.notify_all();
                        return;
                    }
                    tracing::debug!("retrying {path}: {err}");
                    state.have.first_gap(from.min(state.head), shared.len)
                }
            }
        };

        let Some(next) = next else { return };
        if failures > 0 {
            tokio::time::sleep(Duration::from_millis(250 << failures)).await;
        }
        from = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_overlapping_and_touching_ranges() {
        let mut set = RangeSet::default();
        set.insert(10, 20);
        set.insert(30, 40);
        assert_eq!(set.0, vec![(10, 20), (30, 40)]);
        set.insert(20, 30);
        assert_eq!(set.0, vec![(10, 40)]);
        set.insert(0, 5);
        set.insert(50, 60);
        set.insert(3, 55);
        assert_eq!(set.0, vec![(0, 60)]);
        set.insert(7, 7);
        assert_eq!(set.0, vec![(0, 60)]);
    }

    #[test]
    fn inserts_in_order() {
        let mut set = RangeSet::default();
        set.insert(50, 60);
        set.insert(10, 20);
        set.insert(30, 40);
        assert_eq!(set.0, vec![(10, 20), (30, 40), (50, 60)]);
    }

    #[test]
    fn finds_what_is_held_and_what_is_missing() {
        let mut set = RangeSet::default();
        set.insert(0, 10);
        set.insert(20, 30);
        assert_eq!(set.end_of(5), Some(10));
        assert_eq!(set.end_of(10), None);
        assert_eq!(set.end_of(29), Some(30));
        assert_eq!(set.first_gap(0, 30), Some(10));
        assert_eq!(set.first_gap(20, 30), None);
        assert_eq!(set.first_gap(20, 40), Some(30));
        assert!(!set.covers(30));
        set.insert(10, 20);
        assert!(set.covers(30));
        assert!(!set.covers(31));
    }
}

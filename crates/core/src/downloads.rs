//! Tracks saved to the device for offline play.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures_util::StreamExt;
use futures_util::stream;

use crate::api::{Api, ApiResult};
use crate::http::now_ms;
use crate::store::{Download, Store};
use crate::types::PlayerTrack;

/// Two at a time keeps an album download quick without starving playback of
/// bandwidth on the same connection.
const CONCURRENCY: usize = 2;

pub struct Downloads {
    store: Arc<Store>,
    dir: PathBuf,
}

impl Downloads {
    pub fn new(store: Arc<Store>, dir: PathBuf) -> Self {
        Self { store, dir }
    }

    /// Named by id: the server never says where a file lives on its disk, and
    /// the decoder probes the format from the bytes, not the extension.
    fn path_for(&self, track_id: i32) -> PathBuf {
        self.dir.join(format!("{track_id}.audio"))
    }

    /// The local file for a track, if it is downloaded and still on disk.
    pub fn local_file(&self, track_id: i32) -> Option<PathBuf> {
        let path = PathBuf::from(self.store.download_path(track_id).ok()??);
        path.is_file().then_some(path)
    }

    pub fn is_downloaded(&self, track_id: i32) -> bool {
        self.local_file(track_id).is_some()
    }

    pub fn list(&self) -> Vec<Download> {
        self.store.downloads().unwrap_or_default()
    }

    pub fn total_bytes(&self) -> u64 {
        self.list().iter().map(|download| download.bytes).sum()
    }

    pub async fn download(&self, api: &Api, track: &PlayerTrack) -> ApiResult<()> {
        if self.is_downloaded(track.id) {
            return Ok(());
        }
        let path = self.path_for(track.id);
        let bytes = api.download(track.id, &path).await?;
        self.store
            .put_download(&Download {
                track: track.clone(),
                path: path.to_string_lossy().into_owned(),
                bytes,
                at: now_ms(),
            })
            .map_err(|err| std::io::Error::other(err.to_string()))?;
        Ok(())
    }

    /// Downloads what it can and returns the tracks that failed, so the UI can
    /// offer to retry just those.
    pub async fn download_all(&self, api: &Api, tracks: &[PlayerTrack]) -> Vec<PlayerTrack> {
        stream::iter(tracks)
            .map(|track| async move { (track, self.download(api, track).await) })
            .buffer_unordered(CONCURRENCY)
            .filter_map(|(track, result)| async move {
                result
                    .inspect_err(|err| tracing::warn!("download of {} failed: {err}", track.id))
                    .err()
                    .map(|_| track.clone())
            })
            .collect()
            .await
    }

    pub fn remove(&self, track_id: i32) {
        if let Ok(Some(path)) = self.store.delete_download(track_id) {
            remove_file(Path::new(&path));
        }
    }

    pub fn clear(&self) {
        for download in self.list() {
            self.remove(download.track.id);
        }
    }
}

fn remove_file(path: &Path) {
    if let Err(err) = std::fs::remove_file(path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!("could not delete {}: {err}", path.display());
    }
}

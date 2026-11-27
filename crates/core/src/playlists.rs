//! Local-first playlists, one document per playlist.
//!
//! One host, several devices: the whole playlist is the conflict unit, and the
//! later `updated_at` wins. Edits apply on the device at once and reach the server
//! whenever it is reachable, so playlists work offline.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::api::{Api, ApiResult};
use crate::http::now_ms;
use crate::queue::move_item;
use crate::store::Store;
use crate::types::{PlayerTrack, PlaylistSnapshot};

pub const MAX_NAME: usize = 120;
pub const MAX_TRACKS: usize = 2000;
/// The server refuses more than this per sync.
pub const MAX_SYNC_DOCS: usize = 100;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LocalPlaylist {
    pub uuid: String,
    /// The server's id, once it has seen this playlist.
    pub id: Option<i32>,
    pub name: String,
    pub updated_at: i64,
    pub tracks: Vec<PlayerTrack>,
    /// A delete waiting for the server to hear about it.
    #[serde(default)]
    pub deleted: bool,
    /// Changed here since the server last acknowledged it.
    pub dirty: bool,
}

impl LocalPlaylist {
    /// What the sync endpoint takes. A tombstone carries no tracks.
    pub fn to_dirty(&self) -> Value {
        let mut doc = json!({
            "uuid": self.uuid,
            "name": self.name,
            "updatedAt": self.updated_at,
            "trackIds": if self.deleted { Vec::new() } else { self.tracks.iter().map(|t| t.id).collect() },
        });
        if self.deleted {
            doc["deleted"] = json!(true);
        }
        doc
    }
}

pub fn clean_name(raw: &str) -> String {
    raw.trim().chars().take(MAX_NAME).collect()
}

/// Reconciles a server snapshot with local docs it has not acknowledged yet.
/// Dirty local docs win; everything else is replaced by the snapshot, and a clean
/// local playlist missing from it was deleted on another device.
pub fn merge_snapshot(
    local: Vec<LocalPlaylist>,
    remote: Vec<PlaylistSnapshot>,
) -> Vec<LocalPlaylist> {
    let mut dirty: HashMap<String, LocalPlaylist> = local
        .into_iter()
        .filter(|playlist| playlist.dirty)
        .map(|playlist| (playlist.uuid.clone(), playlist))
        .collect();

    let mut merged: Vec<LocalPlaylist> = remote
        .into_iter()
        .map(|snapshot| {
            dirty.remove(&snapshot.uuid).unwrap_or(LocalPlaylist {
                uuid: snapshot.uuid,
                id: Some(snapshot.id),
                name: snapshot.name,
                updated_at: snapshot.updated_at,
                tracks: snapshot.tracks,
                deleted: false,
                dirty: false,
            })
        })
        .collect();
    merged.extend(dirty.into_values());
    merged.sort_by_key(|doc| std::cmp::Reverse(doc.updated_at));
    merged
}

/// Strictly later than the previous write, even if the clock went backwards, or
/// the server would treat the edit as stale and skip it.
fn bump(previous: i64) -> i64 {
    now_ms().max(previous + 1)
}

pub struct Playlists {
    store: Arc<Store>,
    docs: Mutex<Vec<LocalPlaylist>>,
    syncing: AtomicBool,
}

impl Playlists {
    pub fn load(store: Arc<Store>) -> Self {
        let mut docs: Vec<LocalPlaylist> = store.playlists().unwrap_or_else(|err| {
            tracing::warn!("could not read saved playlists: {err}");
            Vec::new()
        });
        docs.sort_by_key(|doc| std::cmp::Reverse(doc.updated_at));
        Self {
            store,
            docs: Mutex::new(docs),
            syncing: AtomicBool::new(false),
        }
    }

    fn docs(&self) -> std::sync::MutexGuard<'_, Vec<LocalPlaylist>> {
        self.docs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn persist(&self, docs: &[LocalPlaylist]) {
        let rows: Vec<(String, &LocalPlaylist)> =
            docs.iter().map(|doc| (doc.uuid.clone(), doc)).collect();
        if let Err(err) = self.store.replace_playlists(&rows) {
            tracing::warn!("could not save playlists: {err}");
        }
    }

    /// What the UI shows: everything but pending deletes, newest first.
    pub fn list(&self) -> Vec<LocalPlaylist> {
        self.docs()
            .iter()
            .filter(|doc| !doc.deleted)
            .cloned()
            .collect()
    }

    /// By uuid, or by the server's numeric id, which older links carry.
    pub fn find(&self, id: &str) -> Option<LocalPlaylist> {
        self.docs()
            .iter()
            .find(|doc| {
                !doc.deleted && (doc.uuid == id || doc.id.is_some_and(|n| n.to_string() == id))
            })
            .cloned()
    }

    fn edit(&self, uuid: &str, change: impl FnOnce(&mut LocalPlaylist)) -> bool {
        let mut docs = self.docs();
        let Some(doc) = docs.iter_mut().find(|doc| doc.uuid == uuid && !doc.deleted) else {
            return false;
        };
        change(doc);
        doc.dirty = true;
        doc.updated_at = bump(doc.updated_at);
        self.persist(&docs);
        true
    }

    /// Returns the new playlist's uuid, or `None` for a blank name.
    pub fn create(&self, name: &str) -> Option<String> {
        let name = clean_name(name);
        if name.is_empty() {
            return None;
        }
        let uuid = uuid::Uuid::new_v4().to_string();
        let mut docs = self.docs();
        docs.insert(
            0,
            LocalPlaylist {
                uuid: uuid.clone(),
                id: None,
                name,
                updated_at: now_ms(),
                tracks: Vec::new(),
                deleted: false,
                dirty: true,
            },
        );
        self.persist(&docs);
        Some(uuid)
    }

    pub fn rename(&self, uuid: &str, name: &str) -> bool {
        let name = clean_name(name);
        !name.is_empty() && self.edit(uuid, |doc| doc.name = name)
    }

    /// Leaves a tombstone until the server acknowledges the delete.
    pub fn delete(&self, uuid: &str) -> bool {
        self.edit(uuid, |doc| doc.deleted = true)
    }

    /// Tracks past the server's limit are dropped rather than refused, so adding
    /// an album to a nearly full playlist adds what fits.
    pub fn add_tracks(&self, uuid: &str, tracks: &[PlayerTrack]) -> bool {
        !tracks.is_empty()
            && self.edit(uuid, |doc| {
                let room = MAX_TRACKS.saturating_sub(doc.tracks.len());
                doc.tracks.extend(tracks.iter().take(room).cloned());
            })
    }

    pub fn remove_track(&self, uuid: &str, index: usize) -> bool {
        self.edit(uuid, |doc| {
            if index < doc.tracks.len() {
                doc.tracks.remove(index);
            }
        })
    }

    pub fn move_track(&self, uuid: &str, from: usize, to: usize) -> bool {
        self.edit(uuid, |doc| move_item(&mut doc.tracks, from, to))
    }

    /// Pushes local edits and takes the server's copy of everything else. Safe to
    /// call whenever: a sync already running makes this a no-op, and a failure
    /// keeps every local edit for next time.
    pub async fn sync(&self, api: &Api) -> ApiResult<()> {
        if self.syncing.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let result = self.sync_once(api).await;
        self.syncing.store(false, Ordering::Release);
        result
    }

    async fn sync_once(&self, api: &Api) -> ApiResult<()> {
        // Batched to the server's per-request limit; each answer is the full
        // snapshot, so only the last one is merged.
        let dirty: Vec<LocalPlaylist> = self
            .docs()
            .iter()
            .filter(|doc| doc.dirty)
            .cloned()
            .collect();
        let mut remote = None;
        for batch in dirty.chunks(MAX_SYNC_DOCS) {
            let docs: Vec<Value> = batch.iter().map(LocalPlaylist::to_dirty).collect();
            remote = Some(api.sync_playlists(&Value::Array(docs)).await?);
        }
        let remote = match remote {
            Some(remote) => remote,
            None => api.playlists().await?,
        };

        // Anything edited again while the request was out is newer than what was
        // sent, so it stays dirty and goes in the next sync.
        let sent: HashMap<&str, i64> = dirty
            .iter()
            .map(|doc| (doc.uuid.as_str(), doc.updated_at))
            .collect();
        let mut docs = self.docs();
        let still_dirty: Vec<LocalPlaylist> = docs
            .drain(..)
            .filter(|doc| doc.dirty && sent.get(doc.uuid.as_str()) != Some(&doc.updated_at))
            .collect();
        *docs = merge_snapshot(still_dirty, remote);
        self.persist(&docs);
        Ok(())
    }

    /// Track ids in any playlist, for keeping downloads of them.
    pub fn track_ids(&self) -> HashSet<i32> {
        self.docs()
            .iter()
            .filter(|doc| !doc.deleted)
            .flat_map(|doc| doc.tracks.iter().map(|track| track.id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{TrackAlbum, TrackArtist};

    const UUID_A: &str = "11111111-2222-4333-8444-555555555555";
    const UUID_B: &str = "aaaaaaaa-2222-4333-8444-555555555555";

    fn track(id: i32) -> PlayerTrack {
        PlayerTrack {
            id,
            title: format!("T{id}"),
            duration: None,
            track_number: None,
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

    fn local(uuid: &str, updated_at: i64, dirty: bool) -> LocalPlaylist {
        LocalPlaylist {
            uuid: uuid.into(),
            id: None,
            name: "Local".into(),
            updated_at,
            tracks: vec![track(1)],
            deleted: false,
            dirty,
        }
    }

    fn remote(uuid: &str, updated_at: i64) -> PlaylistSnapshot {
        PlaylistSnapshot {
            uuid: uuid.into(),
            id: 7,
            name: "Remote".into(),
            updated_at,
            tracks: vec![track(2)],
        }
    }

    #[test]
    fn cleans_names() {
        assert_eq!(clean_name("  Road trip  "), "Road trip");
        assert_eq!(clean_name(&"x".repeat(300)).len(), MAX_NAME);
        assert_eq!(clean_name("   "), "");
    }

    #[test]
    fn takes_the_server_copy_when_local_is_clean() {
        let merged = merge_snapshot(vec![local(UUID_A, 5, false)], vec![remote(UUID_A, 1)]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].name, "Remote");
        assert_eq!(merged[0].id, Some(7));
        assert!(!merged[0].dirty);
    }

    #[test]
    fn keeps_a_dirty_local_edit_and_an_unseen_create() {
        let merged = merge_snapshot(
            vec![local(UUID_A, 5, true), local(UUID_B, 9, true)],
            vec![remote(UUID_A, 1)],
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].uuid, UUID_B);
        assert_eq!(merged[1].name, "Local");
    }

    #[test]
    fn drops_a_clean_playlist_the_server_no_longer_has() {
        assert!(merge_snapshot(vec![local(UUID_A, 5, false)], Vec::new()).is_empty());
    }

    #[test]
    fn keeps_a_tombstone_until_the_server_acknowledges_it() {
        let mut gone = local(UUID_A, 5, true);
        gone.deleted = true;
        let merged = merge_snapshot(vec![gone], vec![remote(UUID_A, 1)]);
        assert!(merged[0].deleted);
    }

    #[test]
    fn serialises_a_tombstone_without_its_tracks() {
        let mut doc = local(UUID_A, 5, true);
        assert_eq!(doc.to_dirty()["trackIds"], json!([1]));
        assert!(doc.to_dirty().get("deleted").is_none());
        doc.deleted = true;
        assert_eq!(
            doc.to_dirty(),
            json!({ "uuid": UUID_A, "name": "Local", "updatedAt": 5, "trackIds": [], "deleted": true })
        );
    }

    #[test]
    fn edits_mark_dirty_bump_the_clock_and_persist() {
        let store = Arc::new(Store::in_memory().unwrap());
        let playlists = Playlists::load(store.clone());
        assert!(playlists.create("   ").is_none());
        let uuid = playlists.create("Mix").unwrap();
        let created = playlists.find(&uuid).unwrap().updated_at;

        assert!(playlists.add_tracks(&uuid, &[track(1), track(2), track(3)]));
        assert!(playlists.move_track(&uuid, 0, 2));
        assert!(playlists.remove_track(&uuid, 0));
        let doc = playlists.find(&uuid).unwrap();
        assert_eq!(
            doc.tracks.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![3, 1]
        );
        assert!(doc.dirty);
        assert!(doc.updated_at > created);

        assert!(playlists.rename(&uuid, "Renamed"));
        assert!(!playlists.rename(&uuid, "  "));
        assert!(playlists.delete(&uuid));
        assert!(playlists.list().is_empty());
        assert!(!playlists.rename(&uuid, "Back"));

        let reloaded = Playlists::load(store);
        assert!(reloaded.list().is_empty());
        assert_eq!(reloaded.docs().len(), 1);
        assert!(reloaded.docs()[0].deleted);
    }

    #[test]
    fn finds_by_uuid_or_server_id() {
        let playlists = Playlists::load(Arc::new(Store::in_memory().unwrap()));
        *playlists.docs() = merge_snapshot(Vec::new(), vec![remote(UUID_A, 1)]);
        assert!(playlists.find(UUID_A).is_some());
        assert!(playlists.find("7").is_some());
        assert!(playlists.find("8").is_none());
    }

    #[test]
    fn caps_tracks_at_the_server_limit() {
        let playlists = Playlists::load(Arc::new(Store::in_memory().unwrap()));
        let uuid = playlists.create("Big").unwrap();
        let many: Vec<PlayerTrack> = (0..MAX_TRACKS as i32 + 10).map(track).collect();
        playlists.add_tracks(&uuid, &many);
        assert_eq!(playlists.find(&uuid).unwrap().tracks.len(), MAX_TRACKS);
    }
}

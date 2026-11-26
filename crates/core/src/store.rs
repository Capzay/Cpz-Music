//! What the device keeps between runs: the session, playlists, downloads, and
//! listens waiting to reach the server. One SQLite file in the app's data folder.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::types::PlayerTrack;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS kv (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS playlists (
    uuid TEXT PRIMARY KEY,
    doc  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS listens (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id      INTEGER NOT NULL,
    duration_secs INTEGER NOT NULL,
    at            INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS downloads (
    track_id INTEGER PRIMARY KEY,
    track    TEXT NOT NULL,
    path     TEXT NOT NULL,
    bytes    INTEGER NOT NULL,
    at       INTEGER NOT NULL
);
";

/// More than this and the oldest unsent listens are dropped. Stats are
/// best-effort, and a device offline for months should not grow without bound.
const MAX_QUEUED_LISTENS: i64 = 500;

#[derive(Clone, Debug, PartialEq)]
pub struct QueuedListen {
    pub id: i64,
    pub track_id: i32,
    pub duration_secs: i64,
    pub at: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Download {
    pub track: PlayerTrack,
    pub path: String,
    pub bytes: u64,
    pub at: i64,
}

pub type StoreResult<T> = Result<T, rusqlite::Error>;

/// Every call is a few rows at most, so a plain mutex around one connection is
/// plenty and nothing here needs to be async.
pub struct Store {
    conn: Mutex<Connection>,
}

fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).expect("our own types always serialize")
}

fn from_json<T: DeserializeOwned>(text: &str) -> Option<T> {
    serde_json::from_str(text).ok()
}

impl Store {
    pub fn open(path: &Path) -> StoreResult<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn in_memory() -> StoreResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> StoreResult<Self> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        // A panic mid-call leaves SQLite itself consistent, so carry on.
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn get<T: DeserializeOwned>(&self, key: &str) -> StoreResult<Option<T>> {
        let text: Option<String> = self
            .conn()
            .query_row("SELECT value FROM kv WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        Ok(text.as_deref().and_then(from_json))
    }

    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> StoreResult<()> {
        self.conn().execute(
            "INSERT INTO kv (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, to_json(value)],
        )?;
        Ok(())
    }

    pub fn remove(&self, key: &str) -> StoreResult<()> {
        self.conn()
            .execute("DELETE FROM kv WHERE key = ?1", [key])?;
        Ok(())
    }

    /// Unreadable rows, say from an older app version, are skipped rather than
    /// failing the whole list.
    pub fn playlists<T: DeserializeOwned>(&self) -> StoreResult<Vec<T>> {
        let conn = self.conn();
        let mut statement = conn.prepare("SELECT doc FROM playlists")?;
        let docs = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(Result::ok)
            .filter_map(|doc| from_json(&doc))
            .collect();
        Ok(docs)
    }

    /// Replaces every playlist in one transaction, so a crash never leaves half
    /// an old list and half a new one.
    pub fn replace_playlists<T: Serialize>(&self, docs: &[(String, T)]) -> StoreResult<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM playlists", [])?;
        {
            let mut insert = tx.prepare("INSERT INTO playlists (uuid, doc) VALUES (?1, ?2)")?;
            for (uuid, doc) in docs {
                insert.execute(params![uuid, to_json(doc)])?;
            }
        }
        tx.commit()
    }

    pub fn queue_listen(&self, track_id: i32, duration_secs: i64, at: i64) -> StoreResult<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO listens (track_id, duration_secs, at) VALUES (?1, ?2, ?3)",
            params![track_id, duration_secs, at],
        )?;
        conn.execute(
            "DELETE FROM listens WHERE id NOT IN
                 (SELECT id FROM listens ORDER BY id DESC LIMIT ?1)",
            [MAX_QUEUED_LISTENS],
        )?;
        Ok(())
    }

    /// Oldest first, the order they are replayed in.
    pub fn queued_listens(&self) -> StoreResult<Vec<QueuedListen>> {
        let conn = self.conn();
        let mut statement =
            conn.prepare("SELECT id, track_id, duration_secs, at FROM listens ORDER BY id ASC")?;
        let rows = statement.query_map([], |row| {
            Ok(QueuedListen {
                id: row.get(0)?,
                track_id: row.get(1)?,
                duration_secs: row.get(2)?,
                at: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    pub fn delete_listen(&self, id: i64) -> StoreResult<()> {
        self.conn()
            .execute("DELETE FROM listens WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn put_download(&self, download: &Download) -> StoreResult<()> {
        self.conn().execute(
            "INSERT INTO downloads (track_id, track, path, bytes, at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (track_id) DO UPDATE SET
                 track = excluded.track, path = excluded.path,
                 bytes = excluded.bytes, at = excluded.at",
            params![
                download.track.id,
                to_json(&download.track),
                download.path,
                download.bytes as i64,
                download.at
            ],
        )?;
        Ok(())
    }

    pub fn downloads(&self) -> StoreResult<Vec<Download>> {
        let conn = self.conn();
        let mut statement =
            conn.prepare("SELECT track, path, bytes, at FROM downloads ORDER BY at DESC")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        Ok(rows
            .filter_map(Result::ok)
            .filter_map(|(track, path, bytes, at)| {
                Some(Download {
                    track: from_json(&track)?,
                    path,
                    bytes: bytes as u64,
                    at,
                })
            })
            .collect())
    }

    pub fn download_path(&self, track_id: i32) -> StoreResult<Option<String>> {
        self.conn()
            .query_row(
                "SELECT path FROM downloads WHERE track_id = ?1",
                [track_id],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn delete_download(&self, track_id: i32) -> StoreResult<Option<String>> {
        let path = self.download_path(track_id)?;
        self.conn()
            .execute("DELETE FROM downloads WHERE track_id = ?1", [track_id])?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{TrackAlbum, TrackArtist};

    fn track(id: i32) -> PlayerTrack {
        PlayerTrack {
            id,
            title: "T".into(),
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
                has_artwork: true,
            },
        }
    }

    #[test]
    fn keeps_values_by_key() {
        let store = Store::in_memory().unwrap();
        assert_eq!(store.get::<String>("server").unwrap(), None);
        store.set("server", &"https://a").unwrap();
        store.set("server", &"https://b").unwrap();
        assert_eq!(
            store.get::<String>("server").unwrap().as_deref(),
            Some("https://b")
        );
        store.remove("server").unwrap();
        assert_eq!(store.get::<String>("server").unwrap(), None);
    }

    #[test]
    fn caps_the_listen_queue_keeping_the_newest() {
        let store = Store::in_memory().unwrap();
        for at in 0..(MAX_QUEUED_LISTENS + 5) {
            store.queue_listen(1, 60, at).unwrap();
        }
        let queued = store.queued_listens().unwrap();
        assert_eq!(queued.len() as i64, MAX_QUEUED_LISTENS);
        assert_eq!(queued[0].at, 5);
        store.delete_listen(queued[0].id).unwrap();
        assert_eq!(store.queued_listens().unwrap()[0].at, 6);
    }

    #[test]
    fn replaces_playlists_wholesale_and_skips_unreadable_rows() {
        let store = Store::in_memory().unwrap();
        store
            .replace_playlists(&[("a".to_owned(), 1), ("b".to_owned(), 2)])
            .unwrap();
        store
            .conn()
            .execute("INSERT INTO playlists VALUES ('c', 'not json')", [])
            .unwrap();
        let mut docs: Vec<i32> = store.playlists().unwrap();
        docs.sort_unstable();
        assert_eq!(docs, vec![1, 2]);
        store.replace_playlists(&[("z".to_owned(), 9)]).unwrap();
        assert_eq!(store.playlists::<i32>().unwrap(), vec![9]);
    }

    #[test]
    fn tracks_downloads() {
        let store = Store::in_memory().unwrap();
        let download = Download {
            track: track(4),
            path: "/x/4.flac".into(),
            bytes: 10,
            at: 1,
        };
        store.put_download(&download).unwrap();
        assert_eq!(store.downloads().unwrap(), vec![download]);
        assert_eq!(
            store.download_path(4).unwrap().as_deref(),
            Some("/x/4.flac")
        );
        assert_eq!(
            store.delete_download(4).unwrap().as_deref(),
            Some("/x/4.flac")
        );
        assert!(store.downloads().unwrap().is_empty());
        assert_eq!(store.delete_download(4).unwrap(), None);
    }

    #[test]
    fn survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/cpz.db");
        Store::open(&path).unwrap().set("k", &5).unwrap();
        assert_eq!(
            Store::open(&path).unwrap().get::<i32>("k").unwrap(),
            Some(5)
        );
    }
}

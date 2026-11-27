use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, Result};
use futures_util::{StreamExt, stream};
use notify::{EventKind, RecursiveMode, Watcher};
use sqlx::PgPool;
use tokio::sync::{RwLock, mpsc};
use tokio::task::{AbortHandle, spawn_blocking};

use crate::media::MediaCache;
use crate::metadata::{TrackMetadata, extract_metadata, mime_for_file};

/// Writes arrive in bursts (tag editors, file copies). Wait for quiet before
/// reading, or metadata gets parsed from a half-written file.
const WATCH_DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct Scanner {
    db: PgPool,
    root: PathBuf,
    artwork_dir: PathBuf,
    media: Arc<MediaCache>,
    /// Pruning deletes albums with no tracks, which is exactly what an album looks
    /// like between its row being written and its first track being written.
    /// Writers share this; pruning takes it alone.
    prune_lock: Arc<RwLock<()>>,
}

/// `fs.Stats.mtimeMs`, bit for bit. Every stored mtime came from Node, and any
/// disagreement would reparse the whole library on first boot.
fn mtime_ms(metadata: &fs::Metadata) -> Option<f64> {
    let elapsed = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(elapsed.as_secs() as f64 * 1000.0 + f64::from(elapsed.subsec_nanos()) / 1_000_000.0)
}

/// `complete` is false when any directory could not be read. A partial listing
/// must never be treated as the truth about what is on disk: the caller deletes
/// everything it does not see, and that cascades into playlists and stats.
fn walk(dir: &Path, files: &mut Vec<String>, complete: &mut bool) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::error!("cannot read {}: {err}", dir.display());
            *complete = false;
            return;
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            *complete = false;
            continue;
        };
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            walk(&path, files, complete);
        } else if mime_for_file(&path).is_some() {
            match path.to_str() {
                Some(text) => files.push(text.to_owned()),
                // Postgres text cannot hold it, so it was never in the database.
                None => tracing::warn!("skipping non-UTF-8 path {}", path.display()),
            }
        }
    }
}

struct Parsed {
    meta: TrackMetadata,
    mtime: f64,
    size: Option<i32>,
}

/// Blocking. None when the file is unchanged or has gone away.
fn parse_if_changed(file: &str, root: &Path, known_mtime: Option<f64>) -> Result<Option<Parsed>> {
    let path = Path::new(file);
    let Ok(stat) = fs::metadata(path) else {
        return Ok(None);
    };
    let Some(mtime) = mtime_ms(&stat) else {
        return Ok(None);
    };
    if known_mtime == Some(mtime) {
        return Ok(None);
    }
    let meta = extract_metadata(path, root)?;
    Ok(Some(Parsed {
        meta,
        mtime,
        size: i32::try_from(stat.len()).ok(),
    }))
}

impl Scanner {
    pub fn new(db: PgPool, root: PathBuf, artwork_dir: PathBuf, media: Arc<MediaCache>) -> Self {
        Self {
            db,
            root,
            artwork_dir,
            media,
            prune_lock: Arc::new(RwLock::new(())),
        }
    }

    async fn save_artwork(&self, album_id: i32, data: &[u8], mime_type: &str) -> Result<String> {
        let extension = match crate::media::image_type(data) {
            Some((extension, _)) => extension,
            None if mime_type.contains("png") => "png",
            None => "jpg",
        };
        // A cover saved earlier under another extension would be left behind.
        for old in ["jpg", "png", "webp", "gif"] {
            if old != extension {
                let _ = tokio::fs::remove_file(self.artwork_dir.join(format!("{album_id}.{old}")))
                    .await;
            }
        }
        let dest = self.artwork_dir.join(format!("{album_id}.{extension}"));
        tokio::fs::create_dir_all(&self.artwork_dir).await?;
        tokio::fs::write(&dest, data).await?;
        dest.to_str()
            .map(str::to_owned)
            .context("artwork directory is not valid UTF-8")
    }

    async fn upsert_artist(&self, name: &str) -> Result<i32, sqlx::Error> {
        // The no-op update is what makes RETURNING hand back an existing row.
        sqlx::query_scalar(
            r#"INSERT INTO "Artist" (name) VALUES ($1)
               ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name
               RETURNING id"#,
        )
        .bind(name)
        .fetch_one(&self.db)
        .await
    }

    async fn store(&self, file: &str, parsed: Parsed) -> Result<()> {
        let Parsed { meta, mtime, size } = parsed;
        let _writing = self.prune_lock.read().await;

        let artist_id = self.upsert_artist(&meta.artist).await?;
        let album_artist_id = if meta.album_artist == meta.artist {
            artist_id
        } else {
            self.upsert_artist(&meta.album_artist).await?
        };

        let (album_id, artwork_path): (i32, Option<String>) = sqlx::query_as(
            r#"INSERT INTO "Album" (title, "artistId", year) VALUES ($1, $2, $3)
               ON CONFLICT (title, "artistId") DO UPDATE SET title = EXCLUDED.title
               RETURNING id, "artworkPath""#,
        )
        .bind(&meta.album)
        .bind(album_artist_id)
        .bind(meta.year)
        .fetch_one(&self.db)
        .await?;

        // Also re-saves when the row points at a file that is no longer there, so a
        // cover lost to a cleared artwork directory comes back on the next scan.
        if let Some(artwork) = &meta.artwork {
            let present = match &artwork_path {
                Some(path) => tokio::fs::try_exists(path).await.unwrap_or(false),
                None => false,
            };
            if !present {
                let saved = self
                    .save_artwork(album_id, &artwork.data, &artwork.mime_type)
                    .await?;
                sqlx::query(r#"UPDATE "Album" SET "artworkPath" = $2 WHERE id = $1"#)
                    .bind(album_id)
                    .bind(saved)
                    .execute(&self.db)
                    .await?;
            }
        }

        sqlx::query(
            r#"INSERT INTO "Track" ("filePath", title, "artistId", "albumId", "trackNumber",
                   "discNumber", duration, "fileSize", "mimeType", bitrate, "sampleRate", mtime)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
               ON CONFLICT ("filePath") DO UPDATE SET
                   title = EXCLUDED.title, "artistId" = EXCLUDED."artistId",
                   "albumId" = EXCLUDED."albumId", "trackNumber" = EXCLUDED."trackNumber",
                   "discNumber" = EXCLUDED."discNumber", duration = EXCLUDED.duration,
                   "fileSize" = EXCLUDED."fileSize", "mimeType" = EXCLUDED."mimeType",
                   bitrate = EXCLUDED.bitrate, "sampleRate" = EXCLUDED."sampleRate",
                   mtime = EXCLUDED.mtime"#,
        )
        .bind(file)
        .bind(&meta.title)
        .bind(artist_id)
        .bind(album_id)
        .bind(meta.track_number)
        .bind(meta.disc_number)
        .bind(meta.duration)
        .bind(size)
        .bind(mime_for_file(Path::new(file)))
        .bind(meta.bitrate)
        .bind(meta.sample_rate)
        .bind(mtime)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// One bad file is logged and skipped, never allowed to stop a scan. Returns
    /// whether anything was written.
    async fn process_file(&self, file: String, known_mtime: Option<f64>) -> bool {
        let root = self.root.clone();
        let path = file.clone();
        let parsed = spawn_blocking(move || parse_if_changed(&path, &root, known_mtime)).await;

        let outcome = match parsed {
            Ok(Ok(Some(parsed))) => self.store(&file, parsed).await.map(|()| true),
            Ok(Ok(None)) => Ok(false),
            Ok(Err(err)) => Err(err),
            Err(err) => Err(err.into()),
        };
        outcome.unwrap_or_else(|err| {
            tracing::error!("failed to process {file}: {err:#}");
            false
        })
    }

    /// Removes albums and artists left with nothing after tracks disappear, along
    /// with the cover files of the albums going away. Leaving those behind fills
    /// the artwork directory with images no album will ever claim again.
    async fn prune_empty(&self) -> Result<(), sqlx::Error> {
        let _pruning = self.prune_lock.write().await;

        let covers: Vec<Option<String>> = sqlx::query_scalar(
            r#"DELETE FROM "Album" al
               WHERE NOT EXISTS (SELECT 1 FROM "Track" t WHERE t."albumId" = al.id)
               RETURNING "artworkPath""#,
        )
        .fetch_all(&self.db)
        .await?;
        for path in covers.into_iter().flatten() {
            let _ = tokio::fs::remove_file(path).await;
        }

        sqlx::query(
            r#"DELETE FROM "Artist" a
               WHERE NOT EXISTS (SELECT 1 FROM "Track" t WHERE t."artistId" = a.id)
                 AND NOT EXISTS (SELECT 1 FROM "Album" al WHERE al."artistId" = a.id)"#,
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    /// Bounded, because each task can hold a decoded cover image, and because the
    /// connection pool is the real limit on how fast rows can be written.
    fn concurrency() -> usize {
        std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .clamp(2, 8)
    }

    async fn process_all(&self, files: Vec<String>, known: &HashMap<String, (i32, f64)>) -> usize {
        let total = files.len();
        let mut done = 0;
        let mut changed = 0;

        let mut results = stream::iter(files)
            .map(|file| {
                let known_mtime = known.get(&file).map(|(_, mtime)| *mtime);
                self.process_file(file, known_mtime)
            })
            .buffer_unordered(Self::concurrency());

        while let Some(wrote) = results.next().await {
            done += 1;
            changed += usize::from(wrote);
            if done % 100 == 0 {
                tracing::info!("{done}/{total}");
            }
        }
        changed
    }

    /// Albums with no cover file on disk get one of their tracks read again, even
    /// though it has not changed: a scan only re-reads changed files, so a cover
    /// that failed to save, or was deleted, would otherwise never come back.
    /// Albums that simply have no art cost one tag read per scan.
    async fn retry_missing_covers(&self) -> Result<(), sqlx::Error> {
        let albums: Vec<(i32, Option<String>, String)> = sqlx::query_as(
            r#"SELECT DISTINCT ON (al.id) al.id, al."artworkPath", t."filePath"
               FROM "Album" al JOIN "Track" t ON t."albumId" = al.id
               ORDER BY al.id, t."discNumber" ASC NULLS LAST, t."trackNumber" ASC NULLS LAST"#,
        )
        .fetch_all(&self.db)
        .await?;
        let mut missing = Vec::new();
        for (_, cover, track) in albums {
            let present = match &cover {
                Some(path) => tokio::fs::try_exists(path).await.unwrap_or(false),
                None => false,
            };
            // A track gone from disk is the cleanup's business, not this.
            if !present && tokio::fs::try_exists(&track).await.unwrap_or(false) {
                missing.push(track);
            }
        }
        if missing.is_empty() {
            return Ok(());
        }
        let count = missing.len();
        let mut found = 0usize;
        for track in missing {
            // `None`: read it whatever its mtime says.
            if self.process_file(track, None).await {
                found += 1;
            }
        }
        tracing::info!("re-read {found} of {count} albums with no cover file");
        self.media.invalidate();
        Ok(())
    }

    /// Fails only on database errors, which the boot loop retries.
    pub async fn scan_library(&self) -> Result<(), sqlx::Error> {
        tracing::info!("scanning {}", self.root.display());
        let root = self.root.clone();
        let (files, complete) = spawn_blocking(move || {
            let mut files = Vec::new();
            let mut complete = true;
            walk(&root, &mut files, &mut complete);
            (files, complete)
        })
        .await
        .expect("directory walk does not panic");
        tracing::info!("found {} audio files", files.len());

        // One query for what the database knows, where asking per file would cost
        // a round trip to the database for every track in the library.
        let rows: Vec<(i32, String, f64)> =
            sqlx::query_as(r#"SELECT id, "filePath", mtime FROM "Track""#)
                .fetch_all(&self.db)
                .await?;
        let known: HashMap<String, (i32, f64)> = rows
            .into_iter()
            .map(|(id, path, mtime)| (path, (id, mtime)))
            .collect();

        let on_disk: HashSet<&String> = files.iter().collect();
        let stale: Vec<i32> = known
            .iter()
            .filter(|(path, _)| !on_disk.contains(path))
            .map(|(_, (id, _))| *id)
            .collect();

        let file_count = files.len();
        let changed = self.process_all(files, &known).await;
        tracing::info!("{changed} tracks added or updated");
        self.retry_missing_covers().await?;

        // Deleting a track takes its playlist entries and listening history with
        // it, so the two ways a scan can be wrong about the disk both stop here: a
        // directory that would not open, and a library that came back empty when
        // the database says it should not be. Both mean a broken mount or a bad
        // MUSIC_DIR far more often than they mean somebody deleted their music.
        if !complete || (file_count == 0 && !known.is_empty()) {
            let reason = if complete {
                "the library scanned empty but the database holds tracks"
            } else {
                "part of the library could not be read"
            };
            tracing::error!(
                "skipping cleanup: {reason}. {} tracks left untouched",
                stale.len()
            );
            self.media.invalidate();
            return Ok(());
        }

        if !stale.is_empty() {
            sqlx::query(r#"DELETE FROM "Track" WHERE id = ANY($1)"#)
                .bind(&stale)
                .execute(&self.db)
                .await?;
            tracing::info!("removed {} tracks no longer on disk", stale.len());
        }

        self.prune_empty().await?;
        self.media.invalidate();
        tracing::info!("scan complete");
        Ok(())
    }

    /// False when the library root is missing or empty, which is what an unmounted
    /// disk looks like from here. Same reasoning as the guard in `scan_library`.
    async fn root_is_healthy(&self) -> bool {
        let root = self.root.clone();
        spawn_blocking(move || fs::read_dir(root).is_ok_and(|mut entries| entries.next().is_some()))
            .await
            .unwrap_or(false)
    }

    async fn remove_path(&self, path: &str) -> Result<(), sqlx::Error> {
        if !self.root_is_healthy().await {
            tracing::error!("ignoring removal of {path}: the library root looks unmounted");
            return Ok(());
        }
        // One path may be a whole directory moved away, which inotify reports as a
        // single event rather than one per file inside it.
        let removed = sqlx::query(
            r#"DELETE FROM "Track" WHERE "filePath" = $1 OR starts_with("filePath", $1 || '/')"#,
        )
        .bind(path)
        .execute(&self.db)
        .await?
        .rows_affected();

        if removed > 0 {
            self.prune_empty().await?;
            self.media.invalidate();
        }
        Ok(())
    }

    /// Brings the database in line with whatever is at `path` now. Looking at the
    /// disk, rather than trusting the event, is what lets one code path cover
    /// creates, edits, deletes, renames and whole directories moving in or out.
    async fn reconcile(&self, path: PathBuf) -> Result<(), sqlx::Error> {
        let Some(text) = path.to_str().map(str::to_owned) else {
            return Ok(());
        };

        match tokio::fs::metadata(&path).await {
            Err(_) => self.remove_path(&text).await,
            Ok(stat) if stat.is_dir() => {
                let (files, _) = spawn_blocking(move || {
                    let mut files = Vec::new();
                    let mut complete = true;
                    walk(&path, &mut files, &mut complete);
                    (files, complete)
                })
                .await
                .expect("directory walk does not panic");

                let rows: Vec<(i32, String, f64)> = sqlx::query_as(
                    r#"SELECT id, "filePath", mtime FROM "Track" WHERE "filePath" = ANY($1)"#,
                )
                .bind(&files)
                .fetch_all(&self.db)
                .await?;
                let known = rows
                    .into_iter()
                    .map(|(id, p, mtime)| (p, (id, mtime)))
                    .collect();

                if self.process_all(files, &known).await > 0 {
                    self.media.invalidate();
                }
                Ok(())
            }
            Ok(_) if mime_for_file(&path).is_some() => {
                let known: Option<f64> =
                    sqlx::query_scalar(r#"SELECT mtime FROM "Track" WHERE "filePath" = $1"#)
                        .bind(&text)
                        .fetch_optional(&self.db)
                        .await?;
                if self.process_file(text, known).await {
                    self.media.invalidate();
                }
                Ok(())
            }
            Ok(_) => Ok(()),
        }
    }

    /// Runs until the process exits.
    pub async fn watch_library(self) -> notify::Result<()> {
        let (sender, mut events) = mpsc::unbounded_channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })?;
        watcher.watch(&self.root, RecursiveMode::Recursive)?;
        tracing::info!("watching {}", self.root.display());

        let mut pending: HashMap<PathBuf, AbortHandle> = HashMap::new();

        while let Some(event) = events.recv().await {
            let event: notify::Event = match event {
                Ok(event) => event,
                Err(err) => {
                    tracing::error!("watch error: {err}");
                    continue;
                }
            };
            if matches!(event.kind, EventKind::Access(_)) {
                continue;
            }

            pending.retain(|_, task| !task.is_finished());
            for path in event.paths {
                if let Some(previous) = pending.remove(&path) {
                    previous.abort();
                }
                let scanner = self.clone();
                let target = path.clone();
                let task = tokio::spawn(async move {
                    tokio::time::sleep(WATCH_DEBOUNCE).await;
                    if let Err(err) = scanner.reconcile(target.clone()).await {
                        tracing::error!("failed to update {}: {err}", target.display());
                    }
                });
                pending.insert(path, task.abort_handle());
            }
        }
        Ok(())
    }

    /// Runs the first scan, retrying while the database is still unreachable, then
    /// watches. Boot is the likely time for that: DNS and the network come up
    /// after the service does.
    pub async fn boot(self) {
        let delays = [2, 5, 15, 30].map(Duration::from_secs);
        let mut attempts = delays.iter();

        loop {
            match self.scan_library().await {
                Ok(()) => break,
                Err(err) => {
                    let transient = matches!(
                        err,
                        sqlx::Error::Io(_) | sqlx::Error::Tls(_) | sqlx::Error::PoolTimedOut
                    );
                    match attempts.next().filter(|_| transient) {
                        Some(delay) => {
                            tracing::warn!(
                                "database unreachable, retrying scan in {delay:?}: {err}"
                            );
                            tokio::time::sleep(*delay).await;
                        }
                        None => {
                            tracing::error!("library scan failed: {err}");
                            return;
                        }
                    }
                }
            }
        }

        if let Err(err) = self.watch_library().await {
            tracing::error!("could not watch the library: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_finds_audio_and_ignores_the_rest() {
        let dir = std::env::temp_dir().join(format!("cpz-walk-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("Artist/Album")).unwrap();
        fs::write(dir.join("Artist/Album/01.MP3"), b"").unwrap();
        fs::write(dir.join("Artist/Album/cover.jpg"), b"").unwrap();

        let (mut files, mut complete) = (Vec::new(), true);
        walk(&dir, &mut files, &mut complete);
        assert!(complete);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("01.MP3"));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn walk_reports_an_unreadable_directory_as_incomplete() {
        let (mut files, mut complete) = (Vec::new(), true);
        walk(Path::new("/definitely/not/here"), &mut files, &mut complete);
        assert!(!complete);
    }
}

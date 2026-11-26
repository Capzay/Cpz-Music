use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use moka::future::Cache;
use sqlx::PgPool;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::paths::is_inside;
use crate::range::{ParsedRange, parse_range};

/// Large enough that a track takes few trips through the blocking pool, small
/// enough that a burst of concurrent streams costs little memory.
const READ_CHUNK: usize = 256 * 1024;

#[derive(Clone, Debug)]
pub struct TrackFile {
    pub path: Arc<str>,
    pub mime_type: Arc<str>,
}

/// Where tracks and covers live on disk. A playing song sends a Range request for
/// every seek and buffer refill, and each would otherwise wait on a database that
/// is a round trip across the internet away. The scanner clears this whenever the
/// library changes; the TTL only covers a second writer, such as the Next.js
/// scanner during a migration.
pub struct MediaCache {
    tracks: Cache<i32, TrackFile>,
    artwork: Cache<i32, Arc<str>>,
}

fn cache<V: Clone + Send + Sync + 'static>() -> Cache<i32, V> {
    Cache::builder()
        .max_capacity(100_000)
        .time_to_live(Duration::from_secs(300))
        .build()
}

impl Default for MediaCache {
    fn default() -> Self {
        Self {
            tracks: cache(),
            artwork: cache(),
        }
    }
}

impl MediaCache {
    pub fn invalidate(&self) {
        self.tracks.invalidate_all();
        self.artwork.invalidate_all();
    }

    pub async fn track(&self, db: &PgPool, id: i32) -> Result<Option<TrackFile>, sqlx::Error> {
        if let Some(file) = self.tracks.get(&id).await {
            return Ok(Some(file));
        }
        let row: Option<(String, Option<String>)> =
            sqlx::query_as(r#"SELECT "filePath", "mimeType" FROM "Track" WHERE id = $1"#)
                .bind(id)
                .fetch_optional(db)
                .await?;
        let Some((path, mime_type)) = row else {
            return Ok(None);
        };

        let file = TrackFile {
            path: path.into(),
            mime_type: mime_type
                .as_deref()
                .unwrap_or("application/octet-stream")
                .into(),
        };
        self.tracks.insert(id, file.clone()).await;
        Ok(Some(file))
    }

    pub async fn artwork(
        &self,
        db: &PgPool,
        album_id: i32,
    ) -> Result<Option<Arc<str>>, sqlx::Error> {
        if let Some(path) = self.artwork.get(&album_id).await {
            return Ok(Some(path));
        }
        let path: Option<Option<String>> =
            sqlx::query_scalar(r#"SELECT "artworkPath" FROM "Album" WHERE id = $1"#)
                .bind(album_id)
                .fetch_optional(db)
                .await?;
        let Some(path) = path.flatten() else {
            return Ok(None);
        };

        let path: Arc<str> = path.into();
        self.artwork.insert(album_id, path.clone()).await;
        Ok(Some(path))
    }
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not found").into_response()
}

/// Serves a file from disk with Range support.
///
/// `root` is the only directory this will ever read from. Paths come from the
/// database rather than the URL, but the guard means a corrupted row or a future
/// caller still cannot turn this into an arbitrary file reader.
pub async fn file_response(
    path: &str,
    root: &Path,
    content_type: &str,
    range: Option<&str>,
    cache_control: &'static str,
) -> Response {
    if !is_inside(root, Path::new(path)) {
        tracing::error!("refused path outside {}: {path}", root.display());
        return not_found();
    }

    let Ok(mut file) = File::open(path).await else {
        return not_found();
    };
    let Ok(stat) = file.metadata().await else {
        return not_found();
    };
    if !stat.is_file() {
        return not_found();
    }

    let size = stat.len();
    let parsed = parse_range(range, size);

    let (status, start, length) = match parsed {
        ParsedRange::Unsatisfiable => {
            return (
                StatusCode::RANGE_NOT_SATISFIABLE,
                [
                    (header::CONTENT_RANGE, format!("bytes */{size}")),
                    (header::ACCEPT_RANGES, "bytes".to_owned()),
                ],
            )
                .into_response();
        }
        ParsedRange::Whole => (StatusCode::OK, 0, size),
        ParsedRange::Range { start, end } => (StatusCode::PARTIAL_CONTENT, start, end - start + 1),
    };

    if start > 0 && file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return not_found();
    }

    let body = Body::from_stream(ReaderStream::with_capacity(file.take(length), READ_CHUNK));
    let mut response = (status, body).into_response();

    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(content_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control),
    );
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    if let ParsedRange::Range { start, end } = parsed {
        let value = format!("bytes {start}-{end}/{size}");
        headers.insert(
            header::CONTENT_RANGE,
            value.parse().expect("digits are valid header text"),
        );
    }
    response
}

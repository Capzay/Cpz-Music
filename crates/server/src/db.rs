use std::time::Duration;

use serde::Serialize;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgRow};
use sqlx::{FromRow, PgPool, Row};

/// Connects on first use, so the server can start while the network is still
/// coming up at boot.
pub fn pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let options: PgConnectOptions = database_url.parse()?;
    Ok(PgPoolOptions::new()
        // Session-mode pooler: every connection held here is a slot held there.
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(240))
        .max_lifetime(Duration::from_secs(1800))
        // A ping before every query would double each round trip to the database.
        .test_before_acquire(false)
        .connect_lazy_with(options))
}

/// What the browser is allowed to know about a track. Deliberately excludes
/// `filePath`, which would tell anyone with dev tools how the disk is laid out.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerTrack {
    pub id: i32,
    pub title: String,
    pub duration: Option<f64>,
    pub track_number: Option<i32>,
    pub disc_number: i32,
    pub artist: TrackArtist,
    pub album: TrackAlbum,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrackArtist {
    pub id: i32,
    pub name: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAlbum {
    pub id: i32,
    pub title: String,
    pub has_artwork: bool,
}

impl FromRow<'_, PgRow> for PlayerTrack {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: row.try_get("id")?,
            title: row.try_get("title")?,
            duration: row.try_get("duration")?,
            track_number: row.try_get("trackNumber")?,
            disc_number: row.try_get("discNumber")?,
            artist: TrackArtist {
                id: row.try_get("artistId")?,
                name: row.try_get("artistName")?,
            },
            album: TrackAlbum {
                id: row.try_get("albumId")?,
                title: row.try_get("albumTitle")?,
                has_artwork: row.try_get("hasArtwork")?,
            },
        })
    }
}

/// One query for a track with its artist and album, where Prisma took three.
macro_rules! track_query {
    ($tail:literal) => {
        $crate::db::track_query!("", $tail)
    };
    ($extra_columns:literal, $tail:literal) => {
        concat!(
            r#"SELECT t.id, t.title, t.duration, t."trackNumber", t."discNumber",
                      ar.id AS "artistId", ar.name AS "artistName",
                      al.id AS "albumId", al.title AS "albumTitle",
                      al."artworkPath" IS NOT NULL AS "hasArtwork" "#,
            $extra_columns,
            r#" FROM "Track" t
               JOIN "Artist" ar ON ar.id = t."artistId"
               JOIN "Album" al ON al.id = t."albumId" "#,
            $tail
        )
    };
}
pub(crate) use track_query;

#[derive(Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumSummary {
    pub id: i32,
    pub title: String,
    pub year: Option<i32>,
    pub artist: String,
    #[sqlx(rename = "hasArtwork")]
    pub has_artwork: bool,
}

macro_rules! album_query {
    ($tail:literal) => {
        concat!(
            r#"SELECT al.id, al.title, al.year, ar.name AS artist,
                      al."artworkPath" IS NOT NULL AS "hasArtwork"
               FROM "Album" al
               JOIN "Artist" ar ON ar.id = al."artistId" "#,
            $tail
        )
    };
}

#[derive(Debug, FromRow, Serialize)]
pub struct ArtistHit {
    pub id: i32,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct SearchResults {
    pub tracks: Vec<PlayerTrack>,
    pub albums: Vec<AlbumSummary>,
    pub artists: Vec<ArtistHit>,
}

/// `%` and `_` in a search box mean themselves, not wildcards.
fn contains_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', r"\\")
        .replace('%', r"\%")
        .replace('_', r"\_");
    format!("%{escaped}%")
}

pub async fn search_library(db: &PgPool, query: &str) -> Result<SearchResults, sqlx::Error> {
    let pattern = contains_pattern(query);

    let (tracks, albums, artists) = tokio::try_join!(
        sqlx::query_as(track_query!(
            r#"WHERE t.title ILIKE $1 ORDER BY t.title ASC LIMIT 40"#
        ))
        .bind(&pattern)
        .fetch_all(db),
        sqlx::query_as(album_query!(
            r#"WHERE al.title ILIKE $1 ORDER BY al.title ASC LIMIT 20"#
        ))
        .bind(&pattern)
        .fetch_all(db),
        sqlx::query_as(
            r#"SELECT a.id, a.name FROM "Artist" a
               WHERE a.name ILIKE $1
                 AND EXISTS (SELECT 1 FROM "Track" t WHERE t."artistId" = a.id)
               ORDER BY a.name ASC LIMIT 20"#,
        )
        .bind(&pattern)
        .fetch_all(db),
    )?;

    Ok(SearchResults {
        tracks,
        albums,
        artists,
    })
}

pub async fn tracks_for_album(db: &PgPool, album_id: i32) -> Result<Vec<PlayerTrack>, sqlx::Error> {
    sqlx::query_as(track_query!(
        r#"WHERE t."albumId" = $1 ORDER BY t."discNumber" ASC, t."trackNumber" ASC, t.title ASC"#
    ))
    .bind(album_id)
    .fetch_all(db)
    .await
}

pub async fn tracks_for_artist(
    db: &PgPool,
    artist_id: i32,
) -> Result<Vec<PlayerTrack>, sqlx::Error> {
    sqlx::query_as(track_query!(
        r#"WHERE t."artistId" = $1
           ORDER BY al.year ASC, al.title ASC, t."discNumber" ASC, t."trackNumber" ASC"#
    ))
    .bind(artist_id)
    .fetch_all(db)
    .await
}

pub async fn all_tracks(db: &PgPool) -> Result<Vec<PlayerTrack>, sqlx::Error> {
    sqlx::query_as(track_query!(
        r#"ORDER BY ar.name ASC, al.year ASC, al.title ASC, t."discNumber" ASC, t."trackNumber" ASC"#
    ))
    .fetch_all(db)
    .await
}

pub async fn all_albums(db: &PgPool) -> Result<Vec<AlbumSummary>, sqlx::Error> {
    sqlx::query_as(album_query!(
        r#"ORDER BY ar.name ASC, al.year ASC, al.title ASC"#
    ))
    .fetch_all(db)
    .await
}

#[derive(Debug, FromRow, Serialize)]
pub struct ArtistCard {
    pub id: i32,
    pub name: String,
    pub albums: i64,
    pub tracks: i64,
}

pub async fn all_artists(db: &PgPool) -> Result<Vec<ArtistCard>, sqlx::Error> {
    sqlx::query_as(
        r#"SELECT a.id, a.name,
                  (SELECT COUNT(*) FROM "Album" al WHERE al."artistId" = a.id) AS albums,
                  (SELECT COUNT(*) FROM "Track" t WHERE t."artistId" = a.id) AS tracks
           FROM "Artist" a
           WHERE EXISTS (SELECT 1 FROM "Track" t WHERE t."artistId" = a.id)
           ORDER BY a.name ASC"#,
    )
    .fetch_all(db)
    .await
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDetail {
    pub id: i32,
    pub title: String,
    pub year: Option<i32>,
    pub has_artwork: bool,
    pub artist: TrackArtist,
    pub tracks: Vec<PlayerTrack>,
}

pub async fn album_detail(db: &PgPool, id: i32) -> Result<Option<AlbumDetail>, sqlx::Error> {
    let album = sqlx::query(
        r#"SELECT al.title, al.year, al."artworkPath" IS NOT NULL AS "hasArtwork",
                  ar.id AS "artistId", ar.name AS "artistName"
           FROM "Album" al JOIN "Artist" ar ON ar.id = al."artistId"
           WHERE al.id = $1"#,
    )
    .bind(id)
    .fetch_optional(db);

    let (album, tracks) = tokio::try_join!(album, tracks_for_album(db, id))?;
    let Some(row) = album else { return Ok(None) };

    Ok(Some(AlbumDetail {
        id,
        title: row.try_get("title")?,
        year: row.try_get("year")?,
        has_artwork: row.try_get("hasArtwork")?,
        artist: TrackArtist {
            id: row.try_get("artistId")?,
            name: row.try_get("artistName")?,
        },
        tracks,
    }))
}

#[derive(Debug, Serialize)]
pub struct ArtistDetail {
    pub id: i32,
    pub name: String,
    pub albums: Vec<AlbumSummary>,
    pub tracks: Vec<PlayerTrack>,
}

pub async fn artist_detail(db: &PgPool, id: i32) -> Result<Option<ArtistDetail>, sqlx::Error> {
    let artist = sqlx::query_scalar::<_, String>(r#"SELECT name FROM "Artist" WHERE id = $1"#)
        .bind(id)
        .fetch_optional(db);

    // Albums they are credited on as album artist, plus albums holding a track of
    // theirs, so features on other people's records still show up here.
    let albums = sqlx::query_as(album_query!(
        r#"WHERE al."artistId" = $1
              OR EXISTS (SELECT 1 FROM "Track" t WHERE t."albumId" = al.id AND t."artistId" = $1)
           ORDER BY al.year ASC, al.title ASC"#
    ))
    .bind(id)
    .fetch_all(db);

    let (artist, albums, tracks) = tokio::try_join!(artist, albums, tracks_for_artist(db, id))?;
    Ok(artist.map(|name| ArtistDetail {
        id,
        name,
        albums,
        tracks,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_like_wildcards() {
        assert_eq!(contains_pattern("50%_off"), r"%50\%\_off%");
        assert_eq!(contains_pattern(r"a\b"), r"%a\\b%");
    }
}

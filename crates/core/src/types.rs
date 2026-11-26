//! The JSON the server sends, as the server's own `db.rs` and routes write it.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackArtist {
    pub id: i32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackAlbum {
    pub id: i32,
    pub title: String,
    pub has_artwork: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumSummary {
    pub id: i32,
    pub title: String,
    pub year: Option<i32>,
    pub artist: String,
    pub has_artwork: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArtistCard {
    pub id: i32,
    pub name: String,
    pub albums: i64,
    pub tracks: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArtistHit {
    pub id: i32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumDetail {
    pub id: i32,
    pub title: String,
    pub year: Option<i32>,
    pub has_artwork: bool,
    pub artist: TrackArtist,
    pub tracks: Vec<PlayerTrack>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArtistDetail {
    pub id: i32,
    pub name: String,
    pub albums: Vec<AlbumSummary>,
    pub tracks: Vec<PlayerTrack>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchResults {
    pub tracks: Vec<PlayerTrack>,
    #[serde(default)]
    pub albums: Vec<AlbumSummary>,
    #[serde(default)]
    pub artists: Vec<ArtistHit>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSnapshot {
    pub uuid: String,
    pub id: i32,
    pub name: String,
    pub updated_at: i64,
    pub tracks: Vec<PlayerTrack>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StatsPeriod {
    Week,
    Month,
    #[default]
    Year,
    All,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsTotals {
    pub listen_secs: i64,
    pub play_count: i64,
    pub unique_tracks: i64,
    pub unique_artists: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopTrack {
    pub id: i32,
    pub title: String,
    pub album_id: i32,
    pub has_artwork: bool,
    pub artist: String,
    pub listen_secs: i64,
    pub play_count: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopArtist {
    pub artist_id: i32,
    pub artist_name: String,
    pub listen_secs: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub period: StatsPeriod,
    pub totals: StatsTotals,
    pub top_tracks: Vec<TopTrack>,
    pub top_artists: Vec<TopArtist>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    /// Off, then all, then one, as the repeat button cycles.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

/// From `GET /api/config`: all a native app needs to sign in with Supabase.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientConfig {
    pub supabase_url: String,
    pub anon_key: String,
}

//! The server's HTTP API, with the caller's token attached.

use std::path::Path;
use std::sync::Arc;

use futures_util::StreamExt;
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::auth::Credentials;
use crate::types::{
    AlbumDetail, AlbumSummary, ArtistCard, ArtistDetail, ClientConfig, PlayerTrack,
    PlaylistSnapshot, SearchResults, Settings, Stats, StatsPeriod,
};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not signed in")]
    SignedOut,
    #[error("the server answered {0}")]
    Status(StatusCode),
    #[error("could not reach the server: {0}")]
    Network(#[from] reqwest::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl ApiError {
    /// Worth trying again later, as opposed to a request the server will never
    /// accept. Queued work keeps the first kind and drops the second.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::SignedOut | Self::Network(_) | Self::Io(_) => true,
            Self::Status(status) => status.is_server_error() || *status == StatusCode::UNAUTHORIZED,
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// The newest desktop build the server holds for a platform.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ClientRelease {
    pub version: String,
    pub sha256: String,
}

/// The public settings a server hands out before anyone signs in.
pub async fn fetch_config(http: &reqwest::Client, server: &str) -> ApiResult<ClientConfig> {
    let response = http
        .get(format!("{}/api/config", server.trim_end_matches('/')))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(ApiError::Status(response.status()));
    }
    Ok(response.json().await?)
}

#[derive(Deserialize)]
struct Tracks {
    tracks: Vec<PlayerTrack>,
}

#[derive(Deserialize)]
struct Albums {
    albums: Vec<AlbumSummary>,
}

#[derive(Deserialize)]
struct Artists {
    artists: Vec<ArtistCard>,
}

#[derive(Deserialize)]
pub struct Playlists {
    pub playlists: Vec<PlaylistSnapshot>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnerStamp {
    refresh: bool,
}

#[derive(Clone)]
pub struct Api {
    http: reqwest::Client,
    server: Arc<str>,
    credentials: Arc<Credentials>,
}

impl Api {
    pub fn new(http: reqwest::Client, server: &str, credentials: Arc<Credentials>) -> Self {
        Self {
            http,
            server: server.trim_end_matches('/').into(),
            credentials,
        }
    }

    pub fn server(&self) -> &str {
        &self.server
    }

    pub fn credentials(&self) -> &Arc<Credentials> {
        &self.credentials
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server)
    }

    pub fn stream_url(&self, track_id: i32) -> String {
        self.url(&format!("/api/tracks/{track_id}/stream"))
    }

    /// Public: Discord and OBS fetch it with no session.
    pub fn artwork_url(&self, album_id: i32) -> String {
        self.url(&format!("/api/artwork/{album_id}"))
    }

    pub async fn request(&self, method: Method, path: &str) -> ApiResult<RequestBuilder> {
        let token = self
            .credentials
            .access_token()
            .await
            .ok_or(ApiError::SignedOut)?;
        Ok(self.http.request(method, self.url(path)).bearer_auth(token))
    }

    async fn send(request: RequestBuilder) -> ApiResult<reqwest::Response> {
        let response = request.send().await?;
        if response.status().is_success() {
            Ok(response)
        } else {
            Err(ApiError::Status(response.status()))
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> ApiResult<T> {
        Ok(Self::send(self.request(Method::GET, path).await?)
            .await?
            .json()
            .await?)
    }

    async fn post<T: DeserializeOwned>(&self, path: &str, body: &Value) -> ApiResult<T> {
        let request = self.request(Method::POST, path).await?.json(body);
        Ok(Self::send(request).await?.json().await?)
    }

    async fn post_empty(&self, path: &str, body: &Value) -> ApiResult<()> {
        Self::send(self.request(Method::POST, path).await?.json(body)).await?;
        Ok(())
    }

    /// Call once after signing in. Stamps the owner flag the Realtime policies
    /// check, then refreshes the token if the flag was new so it carries it.
    pub async fn claim_owner(&self) -> ApiResult<()> {
        let stamp: OwnerStamp = self.post("/api/auth/owner", &json!({})).await?;
        if stamp.refresh {
            self.credentials
                .refresh_now()
                .await
                .map_err(|_| ApiError::SignedOut)?;
        }
        Ok(())
    }

    pub async fn albums(&self) -> ApiResult<Vec<AlbumSummary>> {
        Ok(self.get::<Albums>("/api/library/albums").await?.albums)
    }

    pub async fn album(&self, id: i32) -> ApiResult<AlbumDetail> {
        self.get(&format!("/api/library/albums/{id}")).await
    }

    pub async fn artists(&self) -> ApiResult<Vec<ArtistCard>> {
        Ok(self.get::<Artists>("/api/library/artists").await?.artists)
    }

    pub async fn artist(&self, id: i32) -> ApiResult<ArtistDetail> {
        self.get(&format!("/api/library/artists/{id}")).await
    }

    pub async fn tracks(&self) -> ApiResult<Vec<PlayerTrack>> {
        Ok(self.get::<Tracks>("/api/library/tracks").await?.tracks)
    }

    pub async fn search(&self, query: &str) -> ApiResult<SearchResults> {
        let query: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
        self.get(&format!("/api/search?q={query}")).await
    }

    pub async fn stats(&self, period: StatsPeriod) -> ApiResult<Stats> {
        let period = serde_json::to_value(period).unwrap_or_default();
        let period = period.as_str().unwrap_or("year");
        self.get(&format!("/api/stats?period={period}")).await
    }

    pub async fn record_listen(&self, track_id: i32, duration_secs: i64, at: i64) -> ApiResult<()> {
        let body = json!({ "trackId": track_id, "durationSecs": duration_secs, "at": at });
        self.post_empty("/api/stats/listen", &body).await
    }

    pub async fn playlists(&self) -> ApiResult<Vec<PlaylistSnapshot>> {
        Ok(self
            .get::<Playlists>("/api/playlists/sync")
            .await?
            .playlists)
    }

    /// Sends local edits and gets every playlist back, edits applied.
    pub async fn sync_playlists(&self, dirty: &Value) -> ApiResult<Vec<PlaylistSnapshot>> {
        let body = json!({ "playlists": dirty });
        Ok(self
            .post::<Playlists>("/api/playlists/sync", &body)
            .await?
            .playlists)
    }

    /// Mirrors what this device plays for the OBS overlay.
    pub async fn now_playing(
        &self,
        track: Option<&PlayerTrack>,
        is_playing: bool,
        current_time: f64,
    ) -> ApiResult<()> {
        let body = json!({ "track": track, "isPlaying": is_playing, "currentTime": current_time });
        self.post_empty("/api/now-playing", &body).await
    }

    /// Settings page data: jam state and the OBS overlay link.
    pub async fn settings(&self) -> ApiResult<Settings> {
        self.get("/api/settings").await
    }

    /// One of the host's `/api/jam/...` buttons.
    pub async fn jam_action(&self, path: &str, body: &Value) -> ApiResult<()> {
        self.post_empty(path, body).await
    }

    /// Streams a track into `dest`, through a temporary file so a dropped
    /// connection never leaves a truncated file that looks downloaded.
    /// Returns the size in bytes.
    pub async fn download(&self, track_id: i32, dest: &Path) -> ApiResult<u64> {
        self.download_to(&format!("/api/tracks/{track_id}/stream"), dest)
            .await
    }

    /// The newest desktop build for `platform` (such as `linux-x86_64`), or
    /// `None` when the server has none.
    pub async fn latest_client(&self, platform: &str) -> ApiResult<Option<ClientRelease>> {
        match self.get(&format!("/api/client/latest/{platform}")).await {
            Ok(release) => Ok(Some(release)),
            Err(ApiError::Status(StatusCode::NOT_FOUND)) => Ok(None),
            Err(err) => Err(err),
        }
    }

    pub async fn download_client(&self, platform: &str, dest: &Path) -> ApiResult<u64> {
        self.download_to(&format!("/api/client/download/{platform}"), dest)
            .await
    }

    /// Streams a GET into `dest`, through a `.part` file so a cut-off
    /// download never looks finished.
    async fn download_to(&self, path: &str, dest: &Path) -> ApiResult<u64> {
        let request = self.request(Method::GET, path).await?;
        let response = Self::send(request).await?;

        if let Some(dir) = dest.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        let partial = dest.with_extension("part");
        let mut file = tokio::fs::File::create(&partial).await?;
        let mut bytes = 0u64;
        let mut body = response.bytes_stream();
        while let Some(chunk) = body.next().await {
            let chunk = chunk?;
            bytes += chunk.len() as u64;
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&partial, dest).await?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_what_to_retry() {
        assert!(ApiError::SignedOut.is_retryable());
        assert!(ApiError::Status(StatusCode::BAD_GATEWAY).is_retryable());
        assert!(ApiError::Status(StatusCode::UNAUTHORIZED).is_retryable());
        assert!(!ApiError::Status(StatusCode::CONFLICT).is_retryable());
        assert!(!ApiError::Status(StatusCode::BAD_REQUEST).is_retryable());
    }
}

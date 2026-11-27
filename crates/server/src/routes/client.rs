//! Updates for the desktop app. Each platform's folder under `RELEASES_DIR`
//! holds one build and a `latest.json` describing it:
//!
//! ```json
//! { "version": "0.2.0", "file": "cpz-music", "sha256": "…" }
//! ```
//!
//! `scripts/publish-client.sh` writes both. Both routes sit behind the session
//! gate like the rest of the API, and jam guests are kept out of them.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::no_store;
use crate::Shared;
use crate::media::file_response;

#[derive(Debug, Deserialize, Serialize)]
struct Release {
    version: String,
    file: String,
    sha256: String,
}

/// `linux-x86_64` and the like: a folder name, never a path.
fn valid_platform(platform: &str) -> bool {
    !platform.is_empty()
        && platform.len() <= 40
        && platform
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

async fn release(state: &Shared, platform: &str) -> Option<Release> {
    if !valid_platform(platform) {
        return None;
    }
    let manifest = state.config.releases_dir.join(platform).join("latest.json");
    let bytes = tokio::fs::read(manifest).await.ok()?;
    let release: Release = serde_json::from_slice(&bytes).ok()?;
    // The file must sit in the platform's own folder.
    let plain = !release.file.is_empty()
        && !release.file.contains(['/', '\\'])
        && release.file != ".."
        && release.file != ".";
    plain.then_some(release)
}

pub async fn latest(State(state): State<Shared>, Path(platform): Path<String>) -> Response {
    match release(&state, &platform).await {
        Some(release) => no_store(release),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub async fn download(
    State(state): State<Shared>,
    Path(platform): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(release) = release(&state, &platform).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = state
        .config
        .releases_dir
        .join(&platform)
        .join(&release.file);
    file_response(
        &path.to_string_lossy(),
        &state.config.releases_dir,
        "application/octet-stream",
        headers.get(header::RANGE).and_then(|v| v.to_str().ok()),
        "no-store",
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::valid_platform;

    #[test]
    fn platforms_are_plain_folder_names() {
        assert!(valid_platform("linux-x86_64"));
        assert!(valid_platform("windows-x86_64"));
        assert!(!valid_platform(""));
        assert!(!valid_platform(".."));
        assert!(!valid_platform("linux/../../etc"));
        assert!(!valid_platform("Linux"));
    }
}

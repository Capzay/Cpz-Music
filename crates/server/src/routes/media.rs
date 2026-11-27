use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use super::parse_id;
use crate::Shared;
use crate::auth::{Identity, Resolved};
use crate::error::{ApiError, ApiResult};
use crate::media::{file_response, image_type_of};

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "Not found").into_response()
}

fn range(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok())
}

pub async fn artwork(
    State(state): State<Shared>,
    Path(album_id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let Some(id) = parse_id(&album_id) else {
        return Ok(not_found());
    };
    let Some(path) = state.media.artwork(&state.db, id).await? else {
        return Ok(not_found());
    };

    // From the bytes, not the extension: covers saved before this check went
    // in can be WebP in a file called .jpg.
    let content_type = image_type_of(&path).await.unwrap_or("image/jpeg");

    // Short: the URL is keyed by album id, so a rescan replaces the image behind
    // it. Public because the route is: Discord's proxy caches it on the way to the card.
    Ok(file_response(
        &path,
        &state.config.artwork_dir,
        content_type,
        range(&headers),
        "public, max-age=86400",
    )
    .await)
}

pub async fn stream(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    // The gate lets guests this far by role alone. A guest still waiting to be let
    // in holds the same session, and must not be able to pull audio with it.
    if let Identity::Guest { pid, .. } = &caller.identity
        && !state.auth.guest_admitted(&state.db, pid).await?
    {
        return Err(ApiError::Status(StatusCode::FORBIDDEN, "Not admitted"));
    }

    let Some(id) = parse_id(&id) else {
        return Ok(not_found());
    };
    let Some(track) = state.media.track(&state.db, id).await? else {
        return Ok(not_found());
    };
    let Some(music_dir) = &state.config.music_dir else {
        return Ok(not_found());
    };

    // Immutable: a changed file gets a new mtime and is rescanned, and the audio
    // element re-requests by track id anyway.
    Ok(file_response(
        &track.path,
        music_dir,
        &track.mime_type,
        range(&headers),
        "private, max-age=31536000, immutable",
    )
    .await)
}

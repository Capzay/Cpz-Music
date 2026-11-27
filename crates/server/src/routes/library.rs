use std::sync::Arc;

use axum::Extension;
use axum::extract::{Path, Query, State};
use axum::response::Response;
use serde_json::json;

use super::{Params, no_store, parse_id};
use crate::Shared;
use crate::auth::Resolved;
use crate::db;
use crate::error::{ApiError, ApiResult};

/// A positive id, or a 400. Past Int range is valid input that matches nothing.
fn search_id(raw: &str) -> ApiResult<i32> {
    let id: f64 = raw.parse().map_err(|_| ApiError::bad_request())?;
    if id.fract() != 0.0 || id <= 0.0 {
        return Err(ApiError::bad_request());
    }
    Ok(parse_id(raw).unwrap_or(0))
}

/// Library search for the add-to-playlist picker.
///
///   GET /api/search?q=foo
///   GET /api/search?album=12
///   GET /api/search?artist=34
pub async fn search(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    Query(params): Query<Params>,
) -> ApiResult<Response> {
    caller.require_host()?;

    if let Some(album) = params.get("album") {
        let tracks = db::tracks_for_album(&state.db, search_id(album)?).await?;
        return Ok(no_store(json!({ "tracks": tracks })));
    }
    if let Some(artist) = params.get("artist") {
        let tracks = db::tracks_for_artist(&state.db, search_id(artist)?).await?;
        return Ok(no_store(json!({ "tracks": tracks })));
    }

    let query = params.get("q").map_or("", |q| q.trim());
    if query.is_empty() {
        return Ok(no_store(
            json!({ "tracks": [], "albums": [], "artists": [] }),
        ));
    }
    Ok(no_store(db::search_library(&state.db, query).await?))
}

pub async fn tracks(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
) -> ApiResult<Response> {
    caller.require_host()?;
    Ok(no_store(
        json!({ "tracks": db::all_tracks(&state.db).await? }),
    ))
}

pub async fn albums(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
) -> ApiResult<Response> {
    caller.require_host()?;
    Ok(no_store(
        json!({ "albums": db::all_albums(&state.db).await? }),
    ))
}

pub async fn album(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    caller.require_host()?;
    let id = parse_id(&id).ok_or_else(ApiError::not_found)?;
    let album = db::album_detail(&state.db, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(no_store(album))
}

pub async fn artists(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
) -> ApiResult<Response> {
    caller.require_host()?;
    Ok(no_store(
        json!({ "artists": db::all_artists(&state.db).await? }),
    ))
}

pub async fn artist(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    caller.require_host()?;
    let id = parse_id(&id).ok_or_else(ApiError::not_found)?;
    let artist = db::artist_detail(&state.db, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(no_store(artist))
}

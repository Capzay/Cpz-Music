use std::sync::Arc;
use std::time::Instant;

use axum::Extension;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::{Params, no_store, parse_body};
use crate::Shared;
use crate::auth::Resolved;
use crate::error::{ApiError, ApiResult};

/// A read-only mirror of what the active device is playing, for the OBS overlay.
///
/// The overlay cannot join the Realtime channel, so the active device posts here
/// and the overlay polls with a signed token that grants exactly this endpoint.
/// In memory: it resets on restart, and the overlay repopulates on the next state
/// change a second later.
pub struct NowPlaying {
    track: Value,
    is_playing: bool,
    current_time: f64,
    updated_at: Instant,
}

impl Default for NowPlaying {
    fn default() -> Self {
        Self {
            track: Value::Null,
            is_playing: false,
            current_time: 0.0,
            updated_at: Instant::now(),
        }
    }
}

/// JavaScript's `Boolean(value)`.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        _ => true,
    }
}

pub async fn write(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    body: Bytes,
) -> ApiResult<StatusCode> {
    caller.require_host()?;

    let body = parse_body(&body);
    if !(body.is_object() || body.is_array()) {
        return Err(ApiError::bad_request());
    }

    let next = NowPlaying {
        track: body.get("track").cloned().unwrap_or(Value::Null),
        is_playing: body.get("isPlaying").is_some_and(truthy),
        current_time: body
            .get("currentTime")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        updated_at: Instant::now(),
    };
    *state.now_playing.write().unwrap() = next;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn read(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    Query(params): Query<Params>,
) -> Response {
    // Either the host reading it directly, or an overlay holding a valid token.
    let token = params.get("token").map(String::as_str);
    if !caller.is_host() && state.tokens.verify(token, "obs").is_none() {
        return ApiError::unauthorized().into_response();
    }

    no_store(snapshot(&state))
}

/// What is playing now, with the clock run forward to this moment. Also what a
/// jam guest's page shows along the bottom.
pub fn snapshot(state: &Shared) -> Value {
    let playing = state.now_playing.read().unwrap();

    // The device posts only when something changes, so run the clock forward here
    // rather than making it post a position every second.
    let elapsed = if playing.is_playing {
        playing.updated_at.elapsed().as_secs_f64()
    } else {
        0.0
    };
    let duration = playing
        .track
        .get("duration")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let limit = if duration > 0.0 {
        duration
    } else {
        f64::INFINITY
    };
    let current_time = (playing.current_time + elapsed).min(limit);

    json!({
        "track": playing.track,
        "isPlaying": playing.is_playing,
        "currentTime": current_time,
    })
}

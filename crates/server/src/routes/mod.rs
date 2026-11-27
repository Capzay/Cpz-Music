mod auth;
mod client;
mod jam;
mod library;
mod media;
mod now_playing;
pub use now_playing::NowPlaying;
use now_playing::snapshot as now_playing_snapshot;
mod pages;
mod playlists;
mod stats;

use std::collections::HashMap;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::{Router, middleware};
use serde::Serialize;
use serde_json::{Value, json};
use tower_http::compression::CompressionLayer;

use crate::Shared;

/// 100 playlists of 2000 tracks, the most one sync may carry, fit with room to spare.
const SYNC_BODY_LIMIT: usize = 8 * 1024 * 1024;

fn no_store(value: impl Serialize) -> Response {
    let mut response = Json(value).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Whatever the client sent, or null. Malformed bodies are answered per route.
fn parse_body(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

type Params = HashMap<String, String>;

/// `Number(id)` then `Number.isInteger`, narrowed to what the column can hold.
fn parse_id(raw: &str) -> Option<i32> {
    let id: f64 = raw.parse().ok()?;
    (id.fract() == 0.0 && id.abs() <= f64::from(i32::MAX)).then_some(id as i32)
}

/// What a native app needs to sign in, so it only has to be told the server URL.
/// Both values ship in every Supabase client by design; RLS is what protects data.
async fn client_config(State(state): State<Shared>) -> Json<Value> {
    Json(json!({
        "supabaseUrl": state.config.supabase_url,
        "anonKey": state.config.anon_key,
    }))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Applied to every response. The policy allows no script or style at all; the
/// two pages in `pages` set their own, which this leaves in place.
async fn security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    if !headers.contains_key(header::CONTENT_SECURITY_POLICY) {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
        );
    }
    for (name, value) in [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
        (
            "strict-transport-security",
            "max-age=63072000; includeSubDomains; preload",
        ),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

pub fn router(state: Shared) -> Router {
    // Kept out of the compression layer: audio and images are already compressed,
    // and gzipping a Range response would corrupt the byte offsets it promises.
    let media = Router::new()
        .route("/api/artwork/{album_id}", get(media::artwork))
        .route("/api/tracks/{id}/stream", get(media::stream))
        .route("/api/client/download/{platform}", get(client::download));

    let api = Router::new()
        .route("/api/health", get(health))
        .route("/api/config", get(client_config))
        .route("/api/client/latest/{platform}", get(client::latest))
        .route("/api/auth/owner", post(auth::owner))
        .route("/api/search", get(library::search))
        .route("/api/library/tracks", get(library::tracks))
        .route("/api/library/albums", get(library::albums))
        .route("/api/library/albums/{id}", get(library::album))
        .route("/api/library/artists", get(library::artists))
        .route("/api/library/artists/{id}", get(library::artist))
        .route(
            "/api/now-playing",
            get(now_playing::read).post(now_playing::write),
        )
        .route(
            "/api/playlists/sync",
            get(playlists::read)
                .post(playlists::sync)
                .layer(DefaultBodyLimit::max(SYNC_BODY_LIMIT)),
        )
        .route("/api/stats", get(stats::summary))
        .route("/api/stats/listen", post(stats::listen))
        .route("/api/settings", get(jam::settings))
        .route("/api/jam/guest", get(jam::guest))
        .route("/api/jam/add", post(jam::add))
        .route("/api/jam/leave", post(jam::leave))
        .route("/api/jam/start", post(jam::start))
        .route("/api/jam/stop", post(jam::stop))
        .route("/api/jam/invite", post(jam::new_invite))
        .route("/api/jam/participants/{pid}/admit", post(jam::admit))
        .route("/api/jam/participants/{pid}/kick", post(jam::kick))
        .layer(CompressionLayer::new());

    let auth_routes = Router::new()
        .route("/auth/callback", get(auth::callback))
        .route("/jam/join", get(auth::join))
        .route("/jam", get(pages::jam))
        .route("/obs", get(pages::obs));

    Router::new()
        .merge(media)
        .merge(api)
        .merge(auth_routes)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::gate,
        ))
        .layer(middleware::map_response(security_headers))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::parse_id;

    #[test]
    fn parses_ids_like_number_is_integer() {
        assert_eq!(parse_id("12"), Some(12));
        assert_eq!(parse_id("12.0"), Some(12));
        assert_eq!(parse_id("12.5"), None);
        assert_eq!(parse_id("abc"), None);
        assert_eq!(parse_id("inf"), None);
        assert_eq!(parse_id("NaN"), None);
        assert_eq!(parse_id("99999999999"), None);
    }
}

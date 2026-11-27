use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::Extension;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::future::join_all;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::FromRow;

use super::{no_store, parse_body};
use crate::Shared;
use crate::auth::{Identity, Resolved, apply_cookies, cookies};
use crate::db::{PlayerTrack, track_query};
use crate::error::{ApiError, ApiResult};
use crate::jam::{self, sign_invite};
use crate::origin::request_origin;
use crate::playlist_sync::js_integer;
use crate::rate_limit::client_key;

/// How many tracks one guest may add per minute.
const GUEST_ADD_LIMIT: u32 = 5;
const MINUTE: Duration = Duration::from_secs(60);
/// A year: an OBS scene is set up once and left alone. Rotating APP_SECRET
/// invalidates every outstanding overlay link at once.
const OVERLAY_TTL_SECONDS: i64 = 365 * 24 * 60 * 60;

/// Tells every jam client the participant list moved. Both sides re-read the
/// server rather than trusting the payload, so one event covers every case. Sent
/// in the background: nobody's response should wait on it.
pub fn broadcast_change(state: &Shared) {
    broadcast(state, "jam", json!({}));
}

fn broadcast(state: &Shared, event: &'static str, payload: Value) {
    let state = state.clone();
    tokio::spawn(async move { state.auth.supabase.broadcast(event, payload).await });
}

fn slow_down() -> ApiError {
    ApiError::Status(StatusCode::TOO_MANY_REQUESTS, "Slow down")
}

pub async fn add(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<StatusCode> {
    let Identity::Guest {
        jam_id, pid, name, ..
    } = &caller.identity
    else {
        return Err(ApiError::forbidden());
    };

    // Keyed on the participant, not the address, so one guest cannot flood the queue
    // by switching networks, and guests behind one NAT do not share a quota.
    if !state
        .limiter
        .check(&format!("jam-add:{pid}"), GUEST_ADD_LIMIT, MINUTE)
    {
        return Err(slow_down());
    }
    if !state
        .limiter
        .check(&format!("jam-add-ip:{}", client_key(&headers)), 30, MINUTE)
    {
        return Err(slow_down());
    }

    // Being in a jam is not the same as being let into it.
    if jam::participant_status(&state.db, pid).await?.as_deref() != Some("admitted") {
        return Err(ApiError::Status(StatusCode::FORBIDDEN, "Not admitted"));
    }

    let body = parse_body(&body);
    let track_id = body
        .get("trackId")
        .and_then(js_integer)
        .ok_or_else(ApiError::bad_request)?;
    let unknown = || ApiError::Status(StatusCode::NOT_FOUND, "Unknown track");
    let track_id = i32::try_from(track_id).map_err(|_| unknown())?;

    let track: PlayerTrack = sqlx::query_as(track_query!("WHERE t.id = $1"))
        .bind(track_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(unknown)?;

    sqlx::query(r#"INSERT INTO "JamQueueAdd" ("jamId", pid, "trackId") VALUES ($1, $2, $3)"#)
        .bind(jam_id)
        .bind(pid)
        .bind(track.id)
        .execute(&state.db)
        .await?;

    broadcast(
        &state,
        "jam-add",
        json!({ "track": track, "addedBy": name }),
    );
    Ok(StatusCode::NO_CONTENT)
}

pub async fn leave(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
) -> ApiResult<Response> {
    let Identity::Guest { user_id, .. } = &caller.identity else {
        return Err(ApiError::forbidden());
    };

    // Strip the claims as well as the session, so a stale token cached anywhere
    // stops carrying jam access the moment it is refreshed.
    clear_claims(&state, user_id).await;

    if let Some(session) = &caller.session
        && let Err(err) = state.auth.supabase.sign_out(session.access_token()).await
    {
        tracing::warn!("sign-out failed: {err:#}");
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    let expired = cookies::clear_session(&caller.cookies, state.auth.storage_key());
    apply_cookies(&mut response, &expired);
    Ok(response)
}

/// Best effort, as a guest who cannot be cleared must not stop the host ending
/// the jam. Their participant row is the backstop.
async fn clear_claims(state: &Shared, user_id: &str) {
    let result = state
        .auth
        .supabase
        .update_app_metadata(user_id, json!({ "jam": null }))
        .await;
    if let Err(err) = result {
        tracing::warn!("could not clear jam claims: {err:#}");
    }
    state.auth.revoke_cached();
}

pub async fn settings(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    caller.require_host()?;

    let origin = request_origin(&headers);
    let overlay_url = state
        .tokens
        .sign("obs", json!({}), OVERLAY_TTL_SECONDS)
        .map(|token| format!("{origin}/obs?token={token}"));

    let active = jam::active_jam(&state.db).await?;
    let participants = match &active {
        Some(jam) => jam::list_participants(&state.db, &jam.id).await?,
        None => Vec::new(),
    };
    let invite_url = active
        .as_ref()
        .and_then(|jam| sign_invite(&state.tokens, &jam.id, jam.invite_epoch))
        .map(|token| format!("{origin}/jam/join?token={token}"));

    Ok(no_store(json!({
        "overlayUrl": overlay_url,
        "secretMissing": !state.tokens.can_sign(),
        "jam": active,
        "inviteUrl": invite_url,
        "participants": participants,
    })))
}

pub async fn start(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    body: Bytes,
) -> ApiResult<StatusCode> {
    caller.require_host()?;

    let body = parse_body(&body);
    let host_name: String = body
        .get("hostName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .chars()
        .take(40)
        .collect();

    jam::create_jam(
        &state.db,
        Some(host_name.as_str()).filter(|name| !name.is_empty()),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn stop(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
) -> ApiResult<StatusCode> {
    caller.require_host()?;
    let Some(active) = jam::active_jam(&state.db).await? else {
        return Ok(StatusCode::NO_CONTENT);
    };

    jam::end_jam(&state.db, &active.id).await?;

    // Revoke every guest session's claims, so ending a jam actually locks guests
    // out rather than leaving valid sessions floating around until they expire.
    let users = jam::participant_user_ids(&state.db, &active.id).await?;
    join_all(users.iter().map(|user_id| clear_claims(&state, user_id))).await;
    state.auth.revoke_cached();

    broadcast_change(&state);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn new_invite(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
) -> ApiResult<StatusCode> {
    caller.require_host()?;
    if let Some(active) = jam::active_jam(&state.db).await? {
        jam::rotate_invite(&state.db, &active.id).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn admit(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    Path(pid): Path<String>,
) -> ApiResult<StatusCode> {
    caller.require_host()?;
    if let Some(active) = jam::active_jam(&state.db).await? {
        jam::set_participant_status(&state.db, &active.id, &pid, "admitted").await?;
        state.auth.revoke_cached();
    }
    broadcast_change(&state);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn kick(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    Path(pid): Path<String>,
) -> ApiResult<StatusCode> {
    caller.require_host()?;
    let Some(active) = jam::active_jam(&state.db).await? else {
        return Ok(StatusCode::NO_CONTENT);
    };

    let user = jam::set_participant_status(&state.db, &active.id, &pid, "kicked").await?;
    state.auth.revoke_cached();
    if let Some(Some(user_id)) = user {
        clear_claims(&state, &user_id).await;
    }

    broadcast_change(&state);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(FromRow)]
struct GuestAlbumRow {
    id: i32,
    title: String,
    artist: String,
    #[sqlx(rename = "hasArtwork")]
    has_artwork: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GuestAlbum {
    id: i32,
    title: String,
    artist: String,
    has_artwork: bool,
    tracks: Vec<PlayerTrack>,
}

/// Everything the guest screen needs, for whichever state the guest is in.
///
/// Guests browse a capped slice rather than the whole library: someone else's party
/// guest has no reason to be handed a full inventory of it.
pub async fn guest(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
) -> ApiResult<Response> {
    let Identity::Guest {
        jam_id, pid, name, ..
    } = &caller.identity
    else {
        return Err(ApiError::forbidden());
    };

    let (active, status) = tokio::try_join!(
        jam::active_jam(&state.db),
        jam::participant_status(&state.db, pid),
    )?;

    let Some(active) = active.filter(|jam| jam.id == *jam_id) else {
        return Ok(no_store(json!({ "status": "ended" })));
    };
    match status.as_deref() {
        Some("kicked") => return Ok(no_store(json!({ "status": "removed" }))),
        Some("admitted") => {}
        _ => return Ok(no_store(json!({ "status": "pending", "name": name }))),
    }

    let albums: Vec<GuestAlbumRow> = sqlx::query_as(
        r#"SELECT al.id, al.title, ar.name AS artist,
                  al."artworkPath" IS NOT NULL AS "hasArtwork"
           FROM "Album" al JOIN "Artist" ar ON ar.id = al."artistId"
           ORDER BY al.title ASC LIMIT 60"#,
    )
    .fetch_all(&state.db)
    .await?;

    let ids: Vec<i32> = albums.iter().map(|album| album.id).collect();
    let tracks: Vec<PlayerTrack> = sqlx::query_as(
        r#"SELECT c.* FROM unnest($1::int[]) AS picked(id)
           CROSS JOIN LATERAL (
               SELECT t.id, t.title, t.duration, t."trackNumber", t."discNumber",
                      ar.id AS "artistId", ar.name AS "artistName",
                      al.id AS "albumId", al.title AS "albumTitle",
                      al."artworkPath" IS NOT NULL AS "hasArtwork"
               FROM "Track" t
               JOIN "Artist" ar ON ar.id = t."artistId"
               JOIN "Album" al ON al.id = t."albumId"
               WHERE t."albumId" = picked.id
               ORDER BY t."discNumber" ASC, t."trackNumber" ASC, t.title ASC
               LIMIT 30
           ) c"#,
    )
    .bind(&ids)
    .fetch_all(&state.db)
    .await?;

    let mut by_album: HashMap<i32, Vec<PlayerTrack>> = HashMap::new();
    for track in tracks {
        by_album.entry(track.album.id).or_default().push(track);
    }

    let albums: Vec<GuestAlbum> = albums
        .into_iter()
        .map(|album| GuestAlbum {
            tracks: by_album.remove(&album.id).unwrap_or_default(),
            id: album.id,
            title: album.title,
            artist: album.artist,
            has_artwork: album.has_artwork,
        })
        .collect();

    Ok(no_store(json!({
        "status": "admitted",
        "name": name,
        "hostName": active.host_name,
        "albums": albums,
        "nowPlaying": super::now_playing_snapshot(&state),
    })))
}

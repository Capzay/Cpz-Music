use std::sync::Arc;
use std::time::Duration;

use axum::Extension;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Json, Redirect, Response};
use serde_json::{Value, json};

use super::Params;
use super::jam::broadcast_change;
use crate::Shared;
use crate::auth::session::Session;
use crate::auth::{Identity, Resolved, User, apply_cookies, cookies};
use crate::error::{ApiError, ApiResult};
use crate::jam::{invite_open, join_jam, read_invite};
use crate::origin::request_origin;
use crate::rate_limit::client_key;

fn redirect(url: String, set_cookies: &[HeaderValue]) -> Response {
    let mut response = Redirect::temporary(&url).into_response();
    apply_cookies(&mut response, set_cookies);
    response
}

/// Mirrors "this account is the owner" into `app_metadata`, where the Realtime
/// policies on the player channel can see it. Those policies run inside Postgres,
/// which cannot read OWNER_GITHUB_ID, so the comparison stays here and only its
/// result crosses over.
///
/// A non-owner is stamped false rather than skipped: an account that loses owner
/// status when OWNER_GITHUB_ID changes must not keep a stale true.
async fn stamp_owner(state: &Shared, session: Session) -> Session {
    let Ok(user) = serde_json::from_value::<User>(session.user().clone()) else {
        return session;
    };

    let is_owner = matches!(state.auth.identity_of(Some(&user)), Identity::Host { .. });
    if user.app_metadata["owner"] == Value::Bool(is_owner) {
        return session;
    }

    let supabase = &state.auth.supabase;
    if let Err(err) = supabase
        .update_app_metadata(&user.id, json!({ "owner": is_owner }))
        .await
    {
        tracing::error!("could not stamp owner flag: {err:#}");
        return session;
    }

    // The token in hand predates the metadata write, so mint a fresh one or the
    // owner's devices would be refused the player channel until it expired.
    supabase
        .refresh(session.refresh_token())
        .await
        .unwrap_or(session)
}

/// Native apps sign in with Supabase directly and never pass through `callback`,
/// so they call this once afterwards. Host-only, so it can only ever stamp true;
/// the app then refreshes its token to pick the flag up.
pub async fn owner(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
) -> ApiResult<Json<Value>> {
    caller.require_host()?;
    let Some(user) = &caller.user else {
        return Err(ApiError::unauthorized());
    };
    let stamped = user.app_metadata["owner"] == Value::Bool(true);
    if !stamped {
        state
            .auth
            .supabase
            .update_app_metadata(&user.id, json!({ "owner": true }))
            .await
            .map_err(|err| {
                tracing::error!("could not stamp owner flag: {err:#}");
                ApiError::Status(
                    axum::http::StatusCode::BAD_GATEWAY,
                    "Could not reach auth server",
                )
            })?;
        state.auth.revoke_cached();
    }
    Ok(Json(json!({ "owner": true, "refresh": !stamped })))
}

pub async fn callback(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> Response {
    let origin = request_origin(&headers);

    // Only same-origin relative paths, so ?next= cannot be used as an open redirect.
    let destination = params
        .get("next")
        .map(String::as_str)
        .filter(|next| next.starts_with('/') && !next.starts_with("//") && !next.contains('\\'))
        .unwrap_or("/");

    let Some(code) = params.get("code") else {
        return redirect(format!("{origin}/login?error=missing_code"), &[]);
    };

    let key = state.auth.storage_key();
    let clear_verifier = cookies::clear_code_verifier(key);

    let exchanged = match cookies::read_code_verifier(&caller.cookies, key) {
        Some(verifier) => state.auth.supabase.exchange_code(code, &verifier).await,
        None => Err(anyhow::anyhow!("no PKCE verifier cookie")),
    };
    let session = match exchanged {
        Ok(session) => session,
        Err(err) => {
            // The URL only ever carries a generic code; the reason goes to the server
            // log, where it is readable without handing it to whoever is at the browser.
            tracing::error!("code exchange failed: {err:#}");
            return redirect(
                format!("{origin}/login?error=exchange_failed"),
                &[clear_verifier],
            );
        }
    };

    let session = stamp_owner(&state, session).await;

    let mut set_cookies = cookies::write_session(&caller.cookies, key, &session);
    set_cookies.push(clear_verifier);
    redirect(format!("{origin}{destination}"), &set_cookies)
}

/// Redeems an invite link: verify the signature, create a Supabase anonymous
/// session, then write the jam claims into `app_metadata`, which only the service
/// role can set. The guest ends up holding a real session whose scope cannot be
/// forged from the browser.
pub async fn join(
    State(state): State<Shared>,
    Extension(caller): Extension<Arc<Resolved>>,
    Query(params): Query<Params>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let origin = request_origin(&headers);
    let bounce = |error: &str, set_cookies: &[HeaderValue]| {
        redirect(format!("{origin}/jam?error={error}"), set_cookies)
    };

    // A newcomer's link without a name gets the page that asks for one, which
    // comes back here with it. Only while the link still works: a dead one, the
    // owner's own, or a guest reopening theirs goes straight to its answer.
    if !params.contains_key("name")
        && caller.user.is_none()
        && let Some(claims) = read_invite(&state.tokens, params.get("token").map(String::as_str))
        && invite_open(&state.db, &claims).await?
    {
        return Ok(super::pages::jam().await);
    }

    let key = format!("jam-join:{}", client_key(&headers));
    if !state.limiter.check(&key, 10, Duration::from_secs(60)) {
        return Ok(bounce("rate_limited", &[]));
    }

    let raw_name = params.get("name").map_or("", |name| name.trim());
    let name: String = raw_name.chars().take(40).collect();
    let name = if name.is_empty() {
        "Guest".to_owned()
    } else {
        name
    };

    let Some(claims) = read_invite(&state.tokens, params.get("token").map(String::as_str)) else {
        return Ok(bounce("invalid_invite", &[]));
    };

    // The owner testing their own invite link must not have guest claims written
    // onto their GitHub account. The page tells them to use the app instead.
    if caller.is_host() {
        return Ok(bounce("host", &[]));
    }

    // Reuse an existing session where there is one, so refreshing the invite link
    // does not strand the guest with a second anonymous account.
    let storage_key = state.auth.storage_key();
    let supabase = &state.auth.supabase;

    let (user_id, session) = match (&caller.user, &caller.session) {
        (Some(user), Some(session)) => (user.id.clone(), (**session).clone()),
        _ => {
            let Ok(session) = supabase.sign_in_anonymously().await else {
                return Ok(bounce("signin_failed", &[]));
            };
            let Some(id) = session.user()["id"].as_str().map(str::to_owned) else {
                return Ok(bounce("signin_failed", &[]));
            };
            (id, session)
        }
    };
    let session_cookies = cookies::write_session(&caller.cookies, storage_key, &session);

    let Some(participant) = join_jam(&state.db, &claims, &user_id, &name).await? else {
        return Ok(bounce("jam_closed", &session_cookies));
    };
    if participant.status == "kicked" {
        return Ok(bounce("removed", &session_cookies));
    }

    let claim =
        json!({ "jam": { "jamId": participant.jam_id, "pid": participant.id, "name": name } });
    if let Err(err) = supabase.update_app_metadata(&user_id, claim).await {
        tracing::error!("could not write jam claims: {err:#}");
        return Ok(bounce("signin_failed", &session_cookies));
    }
    state.auth.revoke_cached();

    // The token in hand predates the metadata write, so mint a fresh one or the
    // guest's very next request would look like an ordinary anonymous user.
    let session = supabase
        .refresh(session.refresh_token())
        .await
        .unwrap_or(session);
    let set_cookies = cookies::write_session(&caller.cookies, storage_key, &session);

    broadcast_change(&state);
    Ok(redirect(format!("{origin}/jam"), &set_cookies))
}

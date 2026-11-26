pub mod cookies;
pub mod session;
pub mod supabase;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::Utc;
use moka::future::Cache;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use url::Url;

use crate::Shared;
use crate::config::Config;
use crate::error::ApiError;
use session::Session;
use supabase::Supabase;

/// Every authorisation decision is made in this module. There is no gateway
/// injecting identity headers, so nothing upstream can be spoofed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Identity {
    Host {
        user_id: String,
    },
    Guest {
        user_id: String,
        jam_id: String,
        pid: String,
        name: String,
    },
    None,
}

#[derive(Clone, Debug, Deserialize)]
pub struct User {
    pub id: String,
    #[serde(default)]
    pub app_metadata: Value,
    #[serde(default)]
    pub user_metadata: Value,
    #[serde(default)]
    pub identities: Value,
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

/// GitHub's immutable numeric account id, however Supabase happens to expose it.
fn github_id(user: &User) -> Option<String> {
    let github = user.identities.as_array().and_then(|list| {
        list.iter()
            .find(|identity| identity["provider"] == "github")
    });

    [
        github.map(|gh| &gh["identity_data"]["provider_id"]),
        github.map(|gh| &gh["id"]),
        Some(&user.user_metadata["provider_id"]),
        Some(&user.user_metadata["sub"]),
    ]
    .into_iter()
    .flatten()
    .find_map(scalar_string)
}

/// Guest claims live in `app_metadata`, which only the service role can write. A
/// guest cannot edit their own session to promote themselves or hop jams.
fn jam_claim(user: &User) -> Option<(String, String, String)> {
    let jam = &user.app_metadata["jam"];
    let name = jam["name"].as_str().unwrap_or("Guest");
    Some((
        jam["jamId"].as_str()?.to_owned(),
        jam["pid"].as_str()?.to_owned(),
        name.to_owned(),
    ))
}

pub fn identity_from_user(user: Option<&User>, owner_github_id: Option<&str>) -> Identity {
    let Some(user) = user else {
        return Identity::None;
    };

    // Refuse rather than fall open. An unset owner id must not grant access.
    if owner_github_id.is_some() && github_id(user).as_deref() == owner_github_id {
        return Identity::Host {
            user_id: user.id.clone(),
        };
    }
    match jam_claim(user) {
        Some((jam_id, pid, name)) => Identity::Guest {
            user_id: user.id.clone(),
            jam_id,
            pid,
            name,
        },
        None => Identity::None,
    }
}

/// Paths a jam guest may reach. Anything absent is denied, so a new endpoint is
/// host-only until somebody deliberately adds it here.
pub fn is_guest_allowed(method: &Method, path: &str) -> bool {
    if method == Method::GET || method == Method::HEAD {
        return path.starts_with("/api/artwork/")
            || path.starts_with("/api/tracks/")
            || path == "/api/jam/guest";
    }
    method == Method::POST && (path == "/api/jam/add" || path == "/api/jam/leave")
}

/// Reachable without a session. `/api/now-playing` is not unauthenticated: it
/// carries its own signed-token check, because an OBS browser source cannot
/// complete an OAuth flow, so the handler does the work itself.
const PUBLIC_PATHS: [&str; 5] = [
    "/auth/callback",
    "/api/health",
    "/jam",
    "/api/now-playing",
    "/api/artwork",
];

fn is_public(path: &str) -> bool {
    PUBLIC_PATHS.iter().any(|public| {
        path.strip_prefix(public)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
}

/// Never read the caller's identity, so they skip the auth server entirely. Cover
/// art is fetched by Discord's media proxy and OBS, which have no session to send.
fn ignores_identity(path: &str) -> bool {
    path == "/api/health" || path.starts_with("/api/artwork/")
}

/// Who is calling, plus what the response must do to keep their session alive.
#[derive(Debug)]
pub struct Resolved {
    pub identity: Identity,
    pub user: Option<Arc<User>>,
    pub session: Option<Arc<Session>>,
    pub cookies: Vec<(String, String)>,
    pub set_cookies: Vec<HeaderValue>,
}

impl Resolved {
    pub fn is_host(&self) -> bool {
        matches!(self.identity, Identity::Host { .. })
    }

    pub fn require_host(&self) -> Result<(), ApiError> {
        if self.is_host() {
            Ok(())
        } else {
            Err(ApiError::unauthorized())
        }
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

type TokenHash = [u8; 32];

/// Caches are keyed on a hash, so a key can never be replayed as a credential.
fn hash(token: &str) -> TokenHash {
    Sha256::digest(token.as_bytes()).into()
}

/// auth-js treats a session as expired this long early, so a token cannot lapse
/// between being checked here and being checked by Supabase.
const EXPIRY_MARGIN_SECS: i64 = 90;

pub struct Auth {
    pub supabase: Supabase,
    storage_key: String,
    owner_github_id: Option<String>,
    users: Cache<TokenHash, Option<Arc<User>>>,
    refreshes: Cache<TokenHash, Arc<Session>>,
    admitted: Cache<String, bool>,
}

impl Auth {
    pub fn new(config: &Config) -> Result<Self> {
        let host = Url::parse(&config.supabase_url)?
            .host_str()
            .unwrap_or_default()
            .to_owned();
        let project_ref = host.split('.').next().unwrap_or_default();

        Ok(Self {
            supabase: Supabase::new(config)?,
            storage_key: format!("sb-{project_ref}-auth-token"),
            owner_github_id: config.owner_github_id.clone(),
            // The Next.js app asked Supabase on every request, so each Range request
            // of a playing song waited on a trip across the internet. Revalidating
            // every 30s keeps a deleted or banned user's window short, and every
            // revocation this server performs clears the cache immediately.
            users: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(Duration::from_secs(30))
                .build(),
            // Refresh tokens are single-use. A page firing parallel requests with one
            // expired session must spend it once and share the result, and Supabase
            // allows the same 10s for a slow client to catch up.
            refreshes: Cache::builder()
                .max_capacity(1_000)
                .time_to_live(Duration::from_secs(10))
                .build(),
            admitted: Cache::builder()
                .max_capacity(1_000)
                .time_to_live(Duration::from_secs(30))
                .build(),
        })
    }

    pub fn storage_key(&self) -> &str {
        &self.storage_key
    }

    pub fn identity_of(&self, user: Option<&User>) -> Identity {
        identity_from_user(user, self.owner_github_id.as_deref())
    }

    async fn user(&self, access_token: &str) -> Option<Arc<User>> {
        let lookup = async {
            self.supabase
                .get_user(access_token)
                .await
                .map(|user| user.map(Arc::new))
        };
        match self.users.try_get_with(hash(access_token), lookup).await {
            Ok(user) => user,
            Err(err) => {
                tracing::warn!("could not validate session: {err:#}");
                None
            }
        }
    }

    async fn refresh(&self, refresh_token: &str) -> Option<Arc<Session>> {
        let attempt = async { self.supabase.refresh(refresh_token).await.map(Arc::new) };
        match self
            .refreshes
            .try_get_with(hash(refresh_token), attempt)
            .await
        {
            Ok(session) => Some(session),
            Err(err) => {
                tracing::warn!("could not refresh session: {err:#}");
                None
            }
        }
    }

    pub async fn resolve(&self, headers: &HeaderMap) -> Resolved {
        let cookies = cookies::parse(headers);

        // Native apps hold their own Supabase session and refresh it themselves, so
        // a bearer token is checked as-is and never refreshed here.
        if let Some(token) = bearer(headers) {
            let user = self.user(token).await;
            return Resolved {
                identity: self.identity_of(user.as_deref()),
                user,
                session: None,
                cookies,
                set_cookies: Vec::new(),
            };
        }

        let Some(stored) = cookies::read_session(&cookies, &self.storage_key) else {
            return Resolved {
                identity: Identity::None,
                user: None,
                session: None,
                cookies,
                set_cookies: Vec::new(),
            };
        };

        let mut session = Arc::new(stored);
        let mut set_cookies = Vec::new();

        if session.expires_at() - Utc::now().timestamp() < EXPIRY_MARGIN_SECS
            && let Some(fresh) = self.refresh(session.refresh_token()).await
        {
            set_cookies = cookies::write_session(&cookies, &self.storage_key, &fresh);
            session = fresh;
        }

        // Validated by the auth server, never by decoding the cookie: its contents
        // are whatever the caller chose to send.
        let user = self.user(session.access_token()).await;
        let identity = self.identity_of(user.as_deref());
        Resolved {
            identity,
            user,
            session: Some(session),
            cookies,
            set_cookies,
        }
    }

    /// Whether a guest's participant row has been let in. Having a session is not
    /// the same thing: a guest waiting for approval holds one too.
    pub async fn guest_admitted(&self, db: &PgPool, pid: &str) -> Result<bool> {
        if let Some(admitted) = self.admitted.get(pid).await {
            return Ok(admitted);
        }
        let admitted =
            crate::jam::participant_status(db, pid).await?.as_deref() == Some("admitted");
        self.admitted.insert(pid.to_owned(), admitted).await;
        Ok(admitted)
    }

    /// Call after anything that changes what a guest may do, so the change takes
    /// effect now rather than when the cached answers expire.
    pub fn revoke_cached(&self) {
        self.users.invalidate_all();
        self.admitted.invalidate_all();
    }
}

/// Supabase's own advice for any response carrying auth cookies: a shared cache
/// must never hand one person's session to somebody else.
pub fn apply_cookies(response: &mut Response, set_cookies: &[HeaderValue]) {
    if set_cookies.is_empty() {
        return;
    }
    let headers = response.headers_mut();
    for cookie in set_cookies {
        headers.append(header::SET_COOKIE, cookie.clone());
    }
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-cache, no-store, must-revalidate, max-age=0"),
    );
    headers.insert(header::EXPIRES, HeaderValue::from_static("0"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
}

pub async fn gate(State(state): State<Shared>, mut request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let public = is_public(&path);

    // Nothing but the API and the two auth routes exists here, so anything else
    // falls through to the 404 without paying for an identity lookup.
    if ignores_identity(&path) || (!public && !path.starts_with("/api/")) {
        return next.run(request).await;
    }

    let resolved = Arc::new(state.auth.resolve(request.headers()).await);

    if !public {
        let denied = match &resolved.identity {
            Identity::None => Some(ApiError::unauthorized()),
            Identity::Guest { .. } if !is_guest_allowed(request.method(), &path) => {
                Some(ApiError::Status(
                    axum::http::StatusCode::FORBIDDEN,
                    "Forbidden in jam guest mode",
                ))
            }
            _ => None,
        };
        if let Some(error) = denied {
            let mut response = error.into_response();
            apply_cookies(&mut response, &resolved.set_cookies);
            return response;
        }
    }

    request.extensions_mut().insert(resolved.clone());
    let mut response = next.run(request).await;

    // A handler that wrote its own session has newer cookies than this refresh.
    if !response.headers().contains_key(header::SET_COOKIE) {
        apply_cookies(&mut response, &resolved.set_cookies);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const OWNER: Option<&str> = Some("123");

    fn user(value: Value) -> User {
        serde_json::from_value(value).unwrap()
    }

    fn github_user(id: Value) -> User {
        user(json!({
            "id": "u1",
            "identities": [{ "provider": "github", "identity_data": { "provider_id": id } }],
        }))
    }

    #[test]
    fn nobody_is_nobody() {
        assert_eq!(identity_from_user(None, OWNER), Identity::None);
    }

    #[test]
    fn recognises_the_owner_by_github_id_as_string_or_number() {
        for id in [json!("123"), json!(123)] {
            let identity = identity_from_user(Some(&github_user(id)), OWNER);
            assert_eq!(
                identity,
                Identity::Host {
                    user_id: "u1".into()
                }
            );
        }
    }

    #[test]
    fn falls_back_to_user_metadata() {
        let user = user(json!({ "id": "u1", "user_metadata": { "provider_id": "123" } }));
        assert!(matches!(
            identity_from_user(Some(&user), OWNER),
            Identity::Host { .. }
        ));
    }

    #[test]
    fn an_unset_owner_refuses_everyone() {
        assert_eq!(
            identity_from_user(Some(&github_user(json!("123"))), None),
            Identity::None
        );
        assert_eq!(
            identity_from_user(Some(&user(json!({ "id": "u1" }))), None),
            Identity::None
        );
    }

    #[test]
    fn another_github_account_is_nobody() {
        assert_eq!(
            identity_from_user(Some(&github_user(json!("999"))), OWNER),
            Identity::None
        );
    }

    #[test]
    fn reads_guest_claims_from_app_metadata_only() {
        let guest = user(json!({
            "id": "u2",
            "app_metadata": { "jam": { "jamId": "j", "pid": "p", "name": "Sam" } },
        }));
        assert_eq!(
            identity_from_user(Some(&guest), OWNER),
            Identity::Guest {
                user_id: "u2".into(),
                jam_id: "j".into(),
                pid: "p".into(),
                name: "Sam".into(),
            }
        );

        let forged = user(json!({
            "id": "u2",
            "user_metadata": { "jam": { "jamId": "j", "pid": "p" } },
        }));
        assert_eq!(identity_from_user(Some(&forged), OWNER), Identity::None);
    }

    #[test]
    fn malformed_jam_claims_are_nobody() {
        let guest = user(json!({ "id": "u2", "app_metadata": { "jam": { "jamId": 1 } } }));
        assert_eq!(identity_from_user(Some(&guest), OWNER), Identity::None);
    }

    #[test]
    fn guests_are_denied_by_default() {
        assert!(is_guest_allowed(&Method::GET, "/api/tracks/1/stream"));
        assert!(is_guest_allowed(&Method::HEAD, "/api/artwork/1"));
        assert!(is_guest_allowed(&Method::GET, "/api/jam/guest"));
        assert!(is_guest_allowed(&Method::POST, "/api/jam/add"));
        assert!(is_guest_allowed(&Method::POST, "/api/jam/leave"));

        assert!(!is_guest_allowed(&Method::POST, "/api/tracks/1/stream"));
        assert!(!is_guest_allowed(&Method::GET, "/api/playlists/sync"));
        assert!(!is_guest_allowed(&Method::GET, "/api/search"));
        assert!(!is_guest_allowed(&Method::POST, "/api/stats/listen"));
        assert!(!is_guest_allowed(&Method::POST, "/api/jam/stop"));
        assert!(!is_guest_allowed(&Method::DELETE, "/api/jam/add"));
    }

    #[test]
    fn public_paths_match_on_a_segment_boundary() {
        assert!(is_public("/jam/join"));
        assert!(is_public("/api/artwork/1"));
        assert!(!is_public("/jamboree"));
        assert!(!is_public("/api/jam/add"));
        assert!(!is_public("/api/artworks"));
    }
}

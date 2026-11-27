//! Signing in with GitHub through Supabase, PKCE flow.
//!
//! The browser does the GitHub part and is redirected back to the app with a
//! one-time code: to a loopback address on desktop, or a deep link on Android.
//! The code is useless without the verifier, which never leaves this process.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use url::Url;

use crate::http::now_ms;
use crate::types::ClientConfig;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// The auth server said no: the session is gone and the user must sign in.
    #[error("the session was rejected")]
    Rejected,
    /// The auth server could not be reached. Try again later; do not sign out.
    #[error("could not reach the auth server: {0}")]
    Unreachable(#[from] reqwest::Error),
    #[error("{0}")]
    Other(String),
}

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("the OS has a random number generator");
        let verifier = URL_SAFE_NO_PAD.encode(bytes);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

/// Where to send the browser. Supabase must list `redirect_to` under
/// Authentication → URL Configuration → Redirect URLs, or it falls back to the
/// site URL and the app never hears back.
pub fn authorize_url(config: &ClientConfig, redirect_to: &str, pkce: &Pkce) -> String {
    let mut url = Url::parse(&format!("{}/auth/v1/authorize", config.supabase_url))
        .expect("the Supabase URL came from the server and parses");
    url.query_pairs_mut()
        .append_pair("provider", "github")
        .append_pair("redirect_to", redirect_to)
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "s256");
    url.into()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
}

impl Session {
    fn from_response(value: &Value) -> Option<Self> {
        let expires_at = value["expires_at"]
            .as_i64()
            .unwrap_or_else(|| now_ms() / 1000 + value["expires_in"].as_i64().unwrap_or(0));
        Some(Self {
            access_token: value["access_token"].as_str()?.to_owned(),
            refresh_token: value["refresh_token"].as_str()?.to_owned(),
            expires_at,
        })
    }

    /// Refreshed this long early, so a token cannot lapse between the check here
    /// and the check on the server.
    fn is_stale(&self) -> bool {
        self.expires_at - now_ms() / 1000 < 90
    }
}

pub struct SupabaseAuth {
    http: reqwest::Client,
    config: ClientConfig,
}

impl SupabaseAuth {
    pub fn new(http: reqwest::Client, config: ClientConfig) -> Self {
        Self { http, config }
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    async fn token(&self, grant: &str, body: Value) -> Result<Session, AuthError> {
        let url = format!(
            "{}/auth/v1/token?grant_type={grant}",
            self.config.supabase_url
        );
        let response = self
            .http
            .post(url)
            .header("apikey", &self.config.anon_key)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        if status.is_client_error() {
            return Err(AuthError::Rejected);
        }
        if !status.is_success() {
            return Err(AuthError::Other(format!("auth server answered {status}")));
        }
        let value: Value = response.json().await?;
        Session::from_response(&value)
            .ok_or_else(|| AuthError::Other("the auth server sent no session".into()))
    }

    pub async fn exchange(&self, code: &str, verifier: &str) -> Result<Session, AuthError> {
        self.token(
            "pkce",
            json!({ "auth_code": code, "code_verifier": verifier }),
        )
        .await
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<Session, AuthError> {
        self.token("refresh_token", json!({ "refresh_token": refresh_token }))
            .await
    }

    pub async fn sign_out(&self, access_token: &str) {
        let url = format!("{}/auth/v1/logout", self.config.supabase_url);
        let request = self
            .http
            .post(url)
            .header("apikey", &self.config.anon_key)
            .bearer_auth(access_token)
            .send();
        if let Err(err) = request.await {
            tracing::warn!("sign-out did not reach the auth server: {err}");
        }
    }
}

/// Called whenever the session changes, including to `None` on sign-out, so the
/// app can persist it.
pub type OnSessionChange = Box<dyn Fn(Option<&Session>) + Send + Sync>;

/// Holds the session and hands out fresh access tokens.
///
/// Refresh tokens are single use, so the lock is held across a refresh: several
/// requests finding the token stale at once spend it once and share the result.
pub struct Credentials {
    auth: SupabaseAuth,
    session: Mutex<Option<Session>>,
    on_change: OnSessionChange,
}

impl Credentials {
    pub fn new(
        auth: SupabaseAuth,
        session: Option<Session>,
        on_change: OnSessionChange,
    ) -> Arc<Self> {
        Arc::new(Self {
            auth,
            session: Mutex::new(session),
            on_change,
        })
    }

    pub fn auth(&self) -> &SupabaseAuth {
        &self.auth
    }

    pub async fn is_signed_in(&self) -> bool {
        self.session.lock().await.is_some()
    }

    pub async fn set(&self, session: Option<Session>) {
        let mut slot = self.session.lock().await;
        (self.on_change)(session.as_ref());
        *slot = session;
    }

    /// A token good for at least the next minute and a half, or `None` when signed
    /// out. An unreachable auth server hands back the stale token rather than
    /// signing out: the server may still accept it, and offline is not signed out.
    pub async fn access_token(&self) -> Option<String> {
        let mut slot = self.session.lock().await;
        let session = slot.as_ref()?;
        if !session.is_stale() {
            return Some(session.access_token.clone());
        }
        match self.auth.refresh(&session.refresh_token).await {
            Ok(fresh) => {
                (self.on_change)(Some(&fresh));
                let token = fresh.access_token.clone();
                *slot = Some(fresh);
                Some(token)
            }
            Err(AuthError::Rejected) => {
                (self.on_change)(None);
                *slot = None;
                None
            }
            Err(err) => {
                tracing::warn!("could not refresh the session: {err}");
                Some(session.access_token.clone())
            }
        }
    }

    /// Forces a refresh, for when the server changed the account's metadata and
    /// the token in hand predates it.
    pub async fn refresh_now(&self) -> Result<(), AuthError> {
        let mut slot = self.session.lock().await;
        let Some(session) = slot.as_ref() else {
            return Err(AuthError::Rejected);
        };
        let fresh = self.auth.refresh(&session.refresh_token).await?;
        (self.on_change)(Some(&fresh));
        *slot = Some(fresh);
        Ok(())
    }

    pub async fn sign_out(&self) {
        let mut slot = self.session.lock().await;
        if let Some(session) = slot.take() {
            self.auth.sign_out(&session.access_token).await;
        }
        (self.on_change)(None);
    }
}

/// The fixed loopback address the desktop app listens on for the redirect. Fixed
/// rather than random because Supabase matches redirect URLs against a list.
pub const LOOPBACK_PORT: u16 = 53682;

pub fn loopback_redirect(port: u16) -> String {
    format!("http://127.0.0.1:{port}/auth/callback")
}

/// The code from a redirect URL, or why there is none.
pub fn code_from_redirect(url: &Url) -> Result<String, AuthError> {
    let mut code = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "error_description" => error = Some(value.into_owned()),
            "error" if error.is_none() => error = Some(value.into_owned()),
            _ => {}
        }
    }
    match (code, error) {
        (Some(code), _) if !code.is_empty() => Ok(code),
        (_, Some(error)) => Err(AuthError::Other(error)),
        _ => Err(AuthError::Other("the redirect carried no code".into())),
    }
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Cpz Music</title>\
<body style=\"font-family:sans-serif;background:#111;color:#eee;display:grid;place-items:center;height:100vh;margin:0\">\
<p>Signed in. You can close this tab and go back to Cpz Music.</p>";

/// Waits for the browser to land on the loopback redirect and returns its code.
/// Anything else the browser asks for, such as a favicon, gets a 404 and the wait
/// carries on.
pub async fn receive_code(listener: TcpListener) -> Result<String, AuthError> {
    loop {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|err| AuthError::Other(err.to_string()))?;

        let mut buffer = vec![0u8; 8192];
        let mut read = 0;
        while read < buffer.len() {
            let Ok(n) = stream.read(&mut buffer[read..]).await else {
                break;
            };
            read += n;
            if n == 0 || buffer[..read].windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8_lossy(&buffer[..read]);
        let target = request
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("GET "))
            .and_then(|rest| rest.split(' ').next())
            .unwrap_or("");

        if !target.starts_with("/auth/callback") {
            let _ = stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                )
                .await;
            continue;
        }

        let url = Url::parse(&format!("http://127.0.0.1{target}"))
            .map_err(|err| AuthError::Other(err.to_string()))?;
        let result = code_from_redirect(&url);
        let body = match &result {
            Ok(_) => DONE_PAGE.to_owned(),
            Err(err) => format!(
                "<!doctype html><meta charset=utf-8><p>Sign-in failed: {}</p>",
                html_escape(&err.to_string())
            ),
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        return result;
    }
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_is_the_hashed_verifier() {
        let pkce = Pkce::new();
        assert_eq!(pkce.verifier.len(), 43);
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(pkce.verifier.as_bytes()));
        assert_eq!(pkce.challenge, expected);
        assert_ne!(Pkce::new().verifier, pkce.verifier);
    }

    #[test]
    fn builds_the_authorize_url() {
        let config = ClientConfig {
            supabase_url: "https://abc.supabase.co".into(),
            anon_key: "anon".into(),
            public_url: None,
        };
        let pkce = Pkce::new();
        let url = Url::parse(&authorize_url(
            &config,
            &loopback_redirect(LOOPBACK_PORT),
            &pkce,
        ))
        .unwrap();
        assert_eq!(url.path(), "/auth/v1/authorize");
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["provider"], "github");
        assert_eq!(query["redirect_to"], "http://127.0.0.1:53682/auth/callback");
        assert_eq!(query["code_challenge"], pkce.challenge);
        assert_eq!(query["code_challenge_method"], "s256");
    }

    #[test]
    fn reads_the_code_or_the_error_from_a_redirect() {
        let ok = Url::parse("cpzmusic://auth/callback?code=abc").unwrap();
        assert_eq!(code_from_redirect(&ok).unwrap(), "abc");
        let denied =
            Url::parse("cpzmusic://auth/callback?error=access_denied&error_description=Nope")
                .unwrap();
        assert_eq!(code_from_redirect(&denied).unwrap_err().to_string(), "Nope");
        let empty = Url::parse("cpzmusic://auth/callback").unwrap();
        assert!(code_from_redirect(&empty).is_err());
    }

    #[test]
    fn fills_in_expiry_from_expires_in() {
        let session = Session::from_response(&json!({
            "access_token": "a", "refresh_token": "r", "expires_in": 3600,
        }))
        .unwrap();
        assert!(session.expires_at > now_ms() / 1000 + 3500);
        assert!(!session.is_stale());
        assert!(Session::from_response(&json!({ "access_token": "a" })).is_none());
    }

    #[tokio::test]
    async fn catches_the_loopback_redirect_and_ignores_strays() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let waiting = tokio::spawn(receive_code(listener));

        let http = crate::http::client();
        let stray = http
            .get(format!("http://127.0.0.1:{port}/favicon.ico"))
            .send()
            .await
            .unwrap();
        assert_eq!(stray.status(), 404);
        let page = http
            .get(format!("{}?code=the-code", loopback_redirect(port)))
            .send()
            .await
            .unwrap();
        assert!(page.text().await.unwrap().contains("Signed in"));

        assert_eq!(waiting.await.unwrap().unwrap(), "the-code");
    }
}

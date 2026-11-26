use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{Value, json};

use super::User;
use super::session::Session;
use crate::config::Config;

/// The realtime topic every device listens on.
pub const PLAYER_CHANNEL: &str = "player";

/// The slice of the Supabase HTTP API this server uses. One client for the whole
/// process, so requests reuse pooled TLS connections.
pub struct Supabase {
    http: Client,
    url: String,
    anon_key: String,
    service_role_key: Option<String>,
}

impl Supabase {
    pub fn new(config: &Config) -> Result<Self> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            http,
            url: config.supabase_url.clone(),
            anon_key: config.anon_key.clone(),
            service_role_key: config.service_role_key.clone(),
        })
    }

    fn auth(&self, request: RequestBuilder) -> RequestBuilder {
        request
            .header("apikey", &self.anon_key)
            .header("X-Supabase-Api-Version", "2024-01-01")
    }

    fn service_key(&self) -> Result<&str> {
        self.service_role_key
            .as_deref()
            .context("SUPABASE_SERVICE_ROLE_KEY is not set")
    }

    async fn session_from(request: RequestBuilder) -> Result<Session> {
        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            // The body can echo credentials, so only the status goes in the error.
            bail!("auth server answered {status}");
        }
        Session::from_value(response.json().await?)
            .ok_or_else(|| anyhow!("response had no session"))
    }

    /// Asks the auth server who a token belongs to. `Ok(None)` is a definitive no;
    /// `Err` means it could not be reached, and must not be cached as a no.
    pub async fn get_user(&self, access_token: &str) -> Result<Option<User>> {
        let response = self
            .auth(self.http.get(format!("{}/auth/v1/user", self.url)))
            .bearer_auth(access_token)
            .send()
            .await?;
        match response.status() {
            status if status.is_success() => Ok(Some(response.json().await?)),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Ok(None),
            status => bail!("auth server answered {status}"),
        }
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<Session> {
        let url = format!("{}/auth/v1/token?grant_type=refresh_token", self.url);
        let body = json!({ "refresh_token": refresh_token });
        Self::session_from(self.auth(self.http.post(url)).json(&body)).await
    }

    pub async fn exchange_code(&self, code: &str, verifier: &str) -> Result<Session> {
        let url = format!("{}/auth/v1/token?grant_type=pkce", self.url);
        let body = json!({ "auth_code": code, "code_verifier": verifier });
        Self::session_from(self.auth(self.http.post(url)).json(&body)).await
    }

    pub async fn sign_in_anonymously(&self) -> Result<Session> {
        let url = format!("{}/auth/v1/signup", self.url);
        let body = json!({ "data": {}, "gotrue_meta_security": {} });
        Self::session_from(self.auth(self.http.post(url)).json(&body)).await
    }

    pub async fn sign_out(&self, access_token: &str) -> Result<()> {
        let url = format!("{}/auth/v1/logout?scope=global", self.url);
        self.auth(self.http.post(url))
            .bearer_auth(access_token)
            .send()
            .await?;
        Ok(())
    }

    /// Merges into `app_metadata`, which only the service role can write. That is
    /// what makes jam claims and the owner flag unforgeable from a browser.
    pub async fn update_app_metadata(&self, user_id: &str, metadata: Value) -> Result<()> {
        let key = self.service_key()?;
        let url = format!("{}/auth/v1/admin/users/{user_id}", self.url);
        let response = self
            .http
            .put(url)
            .header("apikey", key)
            .bearer_auth(key)
            .json(&json!({ "app_metadata": metadata }))
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("auth server answered {}", response.status());
        }
        Ok(())
    }

    /// Pushes an event onto the player channel from the server.
    ///
    /// Guests are barred from broadcasting by the Realtime policy, deliberately, so
    /// a guest's queue addition reaches the host's active device through here. A
    /// failure is ignored: the host polls the jam panel as well, so a dropped
    /// broadcast is a delay rather than a lost track.
    pub async fn broadcast(&self, event: &str, payload: Value) {
        let Ok(key) = self.service_key() else { return };
        let url = format!("{}/realtime/v1/api/broadcast", self.url);
        let body = json!({
            "messages": [{ "topic": PLAYER_CHANNEL, "event": event, "payload": payload, "private": true }],
        });
        let request = self
            .http
            .post(url)
            .header("apikey", key)
            .bearer_auth(key)
            .json(&body);
        if let Err(err) = request.send().await {
            tracing::warn!("broadcast failed: {err}");
        }
    }
}

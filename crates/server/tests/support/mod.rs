//! A real server, a real Postgres, and a stand-in for Supabase's HTTP API.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post, put};
use chrono::Utc;
use cpz_server::auth::cookies;
use cpz_server::auth::session::Session;
use cpz_server::config::Config;
use cpz_server::{AppState, Shared, routes};
use serde_json::{Value, json};
use sqlx::{Connection, Executor, PgConnection};
use tokio::net::TcpListener;
use uuid::Uuid;

pub const OWNER: &str = "4242";
pub const SECRET: &str = "0123456789abcdef0123456789abcdef";

#[derive(Default)]
pub struct MockSupabase {
    users: Mutex<HashMap<String, Value>>,
    /// access token -> user id
    access: Mutex<HashMap<String, String>>,
    /// refresh token -> user id
    refresh: Mutex<HashMap<String, String>>,
    pub broadcasts: Mutex<Vec<Value>>,
    pub user_lookups: AtomicUsize,
    pub refreshes: AtomicUsize,
    pub signups: AtomicUsize,
}

impl MockSupabase {
    pub fn add_user(&self, user: Value) -> String {
        let id = user["id"].as_str().unwrap().to_owned();
        self.users.lock().unwrap().insert(id.clone(), user);
        id
    }

    pub fn user(&self, id: &str) -> Value {
        self.users.lock().unwrap()[id].clone()
    }

    pub fn session(&self, user_id: &str, expires_in: i64) -> Value {
        let access = Uuid::new_v4().to_string();
        let refresh = Uuid::new_v4().to_string();
        self.access
            .lock()
            .unwrap()
            .insert(access.clone(), user_id.to_owned());
        self.refresh
            .lock()
            .unwrap()
            .insert(refresh.clone(), user_id.to_owned());
        json!({
            "access_token": access,
            "refresh_token": refresh,
            "token_type": "bearer",
            "expires_in": expires_in,
            "expires_at": Utc::now().timestamp() + expires_in,
            "user": self.user(user_id),
        })
    }
}

type Mock = Arc<MockSupabase>;

async fn get_user(State(mock): State<Mock>, headers: HeaderMap) -> Response {
    mock.user_lookups.fetch_add(1, Ordering::SeqCst);
    let token = headers["authorization"]
        .to_str()
        .unwrap()
        .trim_start_matches("Bearer ");
    let id = mock.access.lock().unwrap().get(token).cloned();
    match id {
        Some(id) => Json(mock.user(&id)).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

async fn token(
    State(mock): State<Mock>,
    Query(params): Query<HashMap<String, String>>,
    Json(body): Json<Value>,
) -> Response {
    match params["grant_type"].as_str() {
        "refresh_token" => {
            mock.refreshes.fetch_add(1, Ordering::SeqCst);
            let token = body["refresh_token"].as_str().unwrap();
            // Single use, like the real thing.
            let id = mock.refresh.lock().unwrap().remove(token);
            match id {
                Some(id) => Json(mock.session(&id, 3600)).into_response(),
                None => StatusCode::BAD_REQUEST.into_response(),
            }
        }
        "pkce" => {
            if body["code_verifier"] != "the-verifier" {
                return StatusCode::BAD_REQUEST.into_response();
            }
            let id = body["auth_code"].as_str().unwrap();
            Json(mock.session(id, 3600)).into_response()
        }
        _ => StatusCode::BAD_REQUEST.into_response(),
    }
}

async fn signup(State(mock): State<Mock>) -> Json<Value> {
    mock.signups.fetch_add(1, Ordering::SeqCst);
    let id = mock.add_user(json!({ "id": Uuid::new_v4().to_string(), "app_metadata": {} }));
    Json(mock.session(&id, 3600))
}

async fn update_user(
    State(mock): State<Mock>,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> StatusCode {
    let mut users = mock.users.lock().unwrap();
    let user = users.get_mut(&id).unwrap();
    for (key, value) in body["app_metadata"].as_object().unwrap() {
        if value.is_null() {
            user["app_metadata"].as_object_mut().unwrap().remove(key);
        } else {
            user["app_metadata"][key] = value.clone();
        }
    }
    StatusCode::OK
}

async fn broadcast(State(mock): State<Mock>, Json(body): Json<Value>) -> StatusCode {
    mock.broadcasts
        .lock()
        .unwrap()
        .push(body["messages"][0].clone());
    StatusCode::ACCEPTED
}

async fn serve(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{address}")
}

pub struct Harness {
    pub url: String,
    pub state: Shared,
    pub mock: Mock,
    pub http: reqwest::Client,
    pub music: PathBuf,
    pub owner_id: String,
}

/// A minimal valid WAV: a second of silence and no tags.
pub fn write_wav(path: &Path) {
    let rate: u32 = 8000;
    let data = vec![0u8; rate as usize];
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, wav).unwrap();
}

impl Harness {
    /// None when no test database is configured, so `cargo test` still works on a
    /// machine without Postgres. Each harness gets a database of its own.
    pub async fn start() -> Option<Self> {
        let Ok(admin_url) = std::env::var("TEST_DATABASE_URL") else {
            eprintln!("TEST_DATABASE_URL is not set, skipping");
            return None;
        };

        let name = format!("t_{}", Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(&admin_url).await.unwrap();
        admin
            .execute(sqlx::AssertSqlSafe(format!(r#"CREATE DATABASE "{name}""#)))
            .await
            .unwrap();

        let (base, _) = admin_url.rsplit_once('/').unwrap();
        let database_url = format!("{base}/{name}");
        let mut connection = PgConnection::connect(&database_url).await.unwrap();
        connection
            .execute(include_str!("../schema.sql"))
            .await
            .unwrap();
        connection.close().await.unwrap();

        let mock = Mock::default();
        let supabase = Router::new()
            .route("/auth/v1/user", get(get_user))
            .route("/auth/v1/token", post(token))
            .route("/auth/v1/signup", post(signup))
            .route("/auth/v1/logout", post(|| async { StatusCode::NO_CONTENT }))
            .route("/auth/v1/admin/users/{id}", put(update_user))
            .route("/realtime/v1/api/broadcast", post(broadcast))
            .with_state(mock.clone());
        let supabase_url = serve(supabase).await;

        let root = std::env::temp_dir().join(&name);
        let music = root.join("music");
        std::fs::create_dir_all(&music).unwrap();

        let state = AppState::new(Config {
            bind_addr: String::new(),
            database_url,
            supabase_url,
            anon_key: "anon".into(),
            service_role_key: Some("service".into()),
            owner_github_id: Some(OWNER.into()),
            music_dir: Some(music.clone()),
            artwork_dir: root.join("artwork"),
            releases_dir: root.join("releases"),
            app_secret: Some(SECRET.into()),
            public_url: Some("https://music.example.com".into()),
        })
        .unwrap();

        let url = serve(routes::router(state.clone())).await;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        let owner_id = mock.add_user(json!({
            "id": "owner",
            "app_metadata": { "owner": true },
            "identities": [{ "provider": "github", "identity_data": { "provider_id": OWNER } }],
        }));

        Some(Self {
            url,
            state,
            mock,
            http,
            music,
            owner_id,
        })
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.url)
    }

    pub fn cookie_for(&self, session: Value) -> String {
        let session = Session::from_value(session).unwrap();
        let headers = cookies::write_session(&[], self.state.auth.storage_key(), &session);
        headers
            .iter()
            .map(|value| value.to_str().unwrap().split(';').next().unwrap())
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn owner_cookie(&self) -> String {
        self.cookie_for(self.mock.session(&self.owner_id, 3600))
    }

    pub async fn scan(&self) {
        self.state.scanner().unwrap().scan_library().await.unwrap();
    }

    pub async fn get(&self, path: &str, cookie: &str) -> reqwest::Response {
        self.http
            .get(self.url(path))
            .header("cookie", cookie)
            .send()
            .await
            .unwrap()
    }

    pub async fn post(&self, path: &str, cookie: &str, body: Value) -> reqwest::Response {
        self.http
            .post(self.url(path))
            .header("cookie", cookie)
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    pub async fn json(&self, path: &str, cookie: &str) -> Value {
        let response = self.get(path, cookie).await;
        assert_eq!(response.status(), 200, "{path}");
        response.json().await.unwrap()
    }
}

/// `Set-Cookie` headers as the `Cookie` header a browser would send back.
pub fn returned_cookies(response: &reqwest::Response) -> String {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().unwrap().split(';').next().unwrap())
        .filter(|pair| !pair.ends_with('='))
        .collect::<Vec<_>>()
        .join("; ")
}

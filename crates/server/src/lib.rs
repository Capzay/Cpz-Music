pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod jam;
pub mod listen_day;
pub mod media;
pub mod metadata;
pub mod origin;
pub mod paths;
pub mod playlist_sync;
pub mod range;
pub mod rate_limit;
pub mod routes;
pub mod scanner;
pub mod tokens;

use std::sync::{Arc, RwLock};

use anyhow::Result;
use sqlx::PgPool;

use auth::Auth;
use config::Config;
use media::MediaCache;
use rate_limit::RateLimiter;
use routes::NowPlaying;
use scanner::Scanner;
use tokens::Tokens;

pub struct AppState {
    pub config: Config,
    pub db: PgPool,
    pub auth: Auth,
    pub tokens: Tokens,
    pub limiter: RateLimiter,
    pub media: Arc<MediaCache>,
    pub now_playing: RwLock<NowPlaying>,
}

pub type Shared = Arc<AppState>;

impl AppState {
    pub fn new(config: Config) -> Result<Shared> {
        // Both TLS users (Postgres and the Supabase client) share this provider. An
        // error only means it was installed already.
        let _ = rustls::crypto::ring::default_provider().install_default();

        Ok(Arc::new(Self {
            db: db::pool(&config.database_url)?,
            auth: Auth::new(&config)?,
            tokens: Tokens::new(config.app_secret.as_deref()),
            limiter: RateLimiter::default(),
            media: Arc::default(),
            now_playing: RwLock::default(),
            config,
        }))
    }

    /// None when MUSIC_DIR is unset, which leaves the API up with nothing to scan.
    pub fn scanner(&self) -> Option<Scanner> {
        let root = self.config.music_dir.clone()?;
        let artwork_dir = self.config.artwork_dir.clone();
        Some(Scanner::new(
            self.db.clone(),
            root,
            artwork_dir,
            self.media.clone(),
        ))
    }
}

use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::paths::resolve;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub supabase_url: String,
    pub anon_key: String,
    pub service_role_key: Option<String>,
    /// Unset refuses everyone rather than letting everyone in.
    pub owner_github_id: Option<String>,
    /// Absolute root of the library. Only ever read from.
    pub music_dir: Option<PathBuf>,
    /// Where extracted album art is cached. Written to, unlike the music directory.
    pub artwork_dir: PathBuf,
    pub app_secret: Option<String>,
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn required(name: &str) -> Result<String> {
    optional(name).with_context(|| format!("Missing required environment variable: {name}"))
}

impl Config {
    /// Takes the same variable names as the Next.js app, so one `.env.local`
    /// serves both while they run side by side.
    pub fn from_env() -> Result<Self> {
        let supabase_url = match optional("SUPABASE_URL") {
            Some(url) => url,
            None => required("NEXT_PUBLIC_SUPABASE_URL")?,
        };
        let anon_key = match optional("SUPABASE_ANON_KEY") {
            Some(key) => key,
            None => required("NEXT_PUBLIC_SUPABASE_ANON_KEY")?,
        };

        Ok(Self {
            bind_addr: optional("BIND_ADDR").unwrap_or_else(|| "127.0.0.1:3006".into()),
            database_url: required("DATABASE_URL")?,
            supabase_url: supabase_url.trim_end_matches('/').to_owned(),
            anon_key,
            service_role_key: optional("SUPABASE_SERVICE_ROLE_KEY"),
            owner_github_id: optional("OWNER_GITHUB_ID"),
            music_dir: optional("MUSIC_DIR").map(|dir| resolve(dir.as_ref())),
            artwork_dir: resolve(
                optional("ARTWORK_DIR")
                    .as_deref()
                    .unwrap_or(".artwork")
                    .as_ref(),
            ),
            app_secret: optional("APP_SECRET"),
        })
    }
}

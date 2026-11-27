use anyhow::{Context, Result};
use cpz_server::config::Config;
use cpz_server::{AppState, routes};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::EnvFilter;

async fn shutdown() {
    let Ok(mut terminate) = signal(SignalKind::terminate()) else {
        return;
    };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::from_filename(".env.local");
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = Config::from_env()?;
    if config.owner_github_id.is_none() {
        tracing::warn!("OWNER_GITHUB_ID is not set, so nobody can sign in as the owner");
    }
    let bind_addr = config.bind_addr.clone();
    let state = AppState::new(config)?;

    // Not awaited: a large library would otherwise delay accepting requests, and
    // clients cope with an empty library while this runs.
    match state.scanner() {
        Some(scanner) => drop(tokio::spawn(scanner.boot())),
        None => tracing::warn!("MUSIC_DIR is not set, skipping library scan"),
    }

    let listener = TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("could not bind {bind_addr}"))?;
    tracing::info!("listening on http://{}", listener.local_addr()?);

    axum::serve(listener, routes::router(state))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

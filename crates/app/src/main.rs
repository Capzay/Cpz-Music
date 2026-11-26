//! Cpz Music for Linux and Windows.

mod app;
mod art;
mod update;
mod view;

use std::sync::Arc;

use cpz_core::store::Store;
use slint::ComponentHandle;

slint::include_modules!();

fn paths() -> app::Paths {
    let data = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("cpz-music");
    let cache = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("cpz-music");
    app::Paths { data, cache }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,cpz_core=info".into()),
        )
        .init();

    update::remember_exe();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    // UI callbacks run on this thread and start tasks with `tokio::spawn`.
    let _entered = runtime.enter();

    let paths = paths();
    let store = Arc::new(Store::open(&paths.data.join("cpz-music.db"))?);
    let ui = AppWindow::new()?;
    view::install(&ui);
    let ctx = app::Ctx::new(&ui, store, paths);
    ctx.wire(&ui);
    ctx.boot();

    ui.run()?;

    // Stops playback and saves the queue before the process goes.
    ctx.shutdown();
    drop(_entered);
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    if ctx.restart_requested() {
        update::relaunch()?;
    }
    Ok(())
}

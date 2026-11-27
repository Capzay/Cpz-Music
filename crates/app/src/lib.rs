//! Cpz Music for Linux, Windows and Android. `main.rs` starts it on the
//! desktop, `android::android_main` on Android; both end up in `run`.

mod app;
mod art;
mod update;
mod view;

#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(not(target_os = "android"))]
mod discord;
#[cfg(not(target_os = "android"))]
mod install;

use std::sync::Arc;

use cpz_core::store::Store;
use slint::ComponentHandle;

slint::include_modules!();

pub use app::Paths;
#[cfg(not(target_os = "android"))]
pub use install::{Action, requested as install_requested, run as install};

/// Runs the app until its window closes. Returns whether an update was
/// installed and the caller should start the new executable.
pub fn run(paths: Paths) -> anyhow::Result<bool> {
    update::remember_exe();
    #[cfg(target_os = "windows")]
    install::refresh_version();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    // UI callbacks run on this thread and start tasks with `tokio::spawn`.
    let _entered = runtime.enter();

    let store = Arc::new(Store::open(&paths.data.join("cpz-music.db"))?);
    let ui = AppWindow::new()?;
    // Matches the menu entry `--install` writes, so the window gets its icon.
    // Needs Slint up (so after the window exists) and the window not yet shown.
    #[cfg(not(target_os = "android"))]
    let _ = slint::set_xdg_app_id(install::APP_ID);
    view::install(&ui);
    let ctx = app::Ctx::new(&ui, store, paths);
    ctx.wire(&ui);
    ctx.boot();

    ui.run()?;

    // Stops playback and saves the queue before the process goes.
    ctx.shutdown();
    drop(_entered);
    runtime.shutdown_timeout(std::time::Duration::from_secs(2));
    Ok(ctx.restart_requested())
}

/// Same as `update::relaunch`, for `main.rs`.
pub fn relaunch() -> anyhow::Result<()> {
    update::relaunch()
}

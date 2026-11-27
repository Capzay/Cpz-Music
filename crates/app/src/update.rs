//! Keeps the app current. The server holds the newest build for each
//! platform (see the server's `routes/client.rs`); when it is newer than this
//! one, the app offers to swap its own executable for it and restart.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, bail};
use cpz_core::api::{Api, ClientRelease};
use sha2::{Digest, Sha256};

/// This build's version, from `crates/app/Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Recorded at start-up: once the file is replaced, the running process can
/// no longer find its own path (on Linux it reads as "(deleted)").
static EXE: OnceLock<PathBuf> = OnceLock::new();

pub fn remember_exe() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = EXE.set(exe);
    }
}

/// The folder the server keeps this platform's builds in.
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Debug builds come from `cargo run`, which would rebuild over an update
/// anyway, so they only check when asked to with `CPZ_UPDATE_CHECK=1`.
pub fn enabled() -> bool {
    !cfg!(debug_assertions) || std::env::var_os("CPZ_UPDATE_CHECK").is_some()
}

/// Whether `latest` is a later version than this build. Anything that does not
/// parse is never newer, so a bad manifest cannot nag forever.
pub fn is_newer(latest: &str) -> bool {
    match (
        semver::Version::parse(latest),
        semver::Version::parse(VERSION),
    ) {
        (Ok(latest), Ok(current)) => latest > current,
        _ => false,
    }
}

/// The server's newest build, if it is newer than this one.
pub async fn check(api: &Api) -> anyhow::Result<Option<ClientRelease>> {
    let release = api.latest_client(&platform()).await?;
    Ok(release.filter(|r| is_newer(&r.version)))
}

/// Downloads the build, checks it against its hash and puts it in place of
/// this executable (takes effect on the next start), or on Android opens the
/// installer with it.
pub async fn install(api: &Api, release: &ClientRelease, scratch: &Path) -> anyhow::Result<()> {
    let file = scratch.join(format!("cpz-music-{}.download", release.version));
    api.download_client(&platform(), &file)
        .await
        .context("could not download the update")?;
    let bytes = tokio::fs::read(&file).await?;
    let digest = hex::encode(Sha256::digest(&bytes));
    if !digest.eq_ignore_ascii_case(release.sha256.trim()) {
        let _ = tokio::fs::remove_file(&file).await;
        bail!("the update was damaged on the way (checksum mismatch)");
    }
    // Android installs through its package installer, which asks first and
    // then restarts the app itself.
    #[cfg(target_os = "android")]
    let installed = {
        let apk = file.with_extension("apk");
        tokio::fs::rename(&file, &apk).await?;
        crate::android::install_apk(&apk)
    };
    #[cfg(not(target_os = "android"))]
    let installed = {
        let replaced = tokio::task::spawn_blocking({
            let file = file.clone();
            move || self_replace::self_replace(&file)
        })
        .await?;
        let _ = tokio::fs::remove_file(&file).await;
        replaced.context("could not replace the app")
    };
    installed
}

/// Starts the (new) executable again with the same arguments.
pub fn relaunch() -> anyhow::Result<()> {
    let exe = EXE.get().context("unknown executable path")?;
    std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .spawn()
        .context("could not start the updated app")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("999.0.0"));
        assert!(!is_newer(VERSION));
        assert!(!is_newer("0.0.1"));
        assert!(!is_newer("not a version"));
        assert!(!is_newer(""));
    }
}

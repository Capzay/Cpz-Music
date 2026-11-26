//! Cpz Music for Linux and Windows. Android starts in `android.rs` instead.

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,cpz_core=info".into()),
        )
        .init();

    let data = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("cpz-music");
    let cache = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("cpz-music");
    if cpz_app::run(cpz_app::Paths { data, cache })? {
        cpz_app::relaunch()?;
    }
    Ok(())
}

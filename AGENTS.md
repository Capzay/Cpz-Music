# Working on Cpz Music

A Cargo workspace; there is no JavaScript build any more.

- `crates/server`: the axum server (API, audio, jam guest page and OBS overlay in `pages/`). SQL schema in `migrations/`.
- `crates/core`: everything the apps share that isn't UI: API client, sign-in, queue, player, device handover.
- `crates/app`: the Slint app for Linux, Windows and Android. UI in `ui/*.slint`.
- `crates/xtask`: `cargo android`, which builds the APK from `android/`.
- `vendor/`: a patched Slint Android backend (see its PATCHED.md before bumping Slint).

Before finishing: `cargo fmt --all`, `cargo clippy --workspace --all-targets`, and `cargo test --workspace` (server tests need `TEST_DATABASE_URL`, see README). The Slint UI is checked by building `cpz-app`.

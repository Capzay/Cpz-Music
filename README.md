# Cpz Music

A self-hosted music streamer for a personal library. Your files stay on your own
disk; Supabase holds the database and handles sign-in. A small Rust server
streams the audio, and a native app plays it on Linux, Windows and Android.
Any device can act as a remote for whichever one is actually playing.

## Features

- **Library** scanned from disk, watched for changes, with album art extracted
  from tags
- **Player** with queue, shuffle, repeat, and the system's media controls
  (media keys and MPRIS on Linux, the media overlay on Windows, the
  notification and lock screen on Android)
- **Playlists**, stored on the device and synced when online
- **Multi-device control**: one device plays, the others act as remotes
- **Offline**: download songs and play them with no connection. Listens
  recorded offline are sent later, on the day they happened
- **Listening stats** by week, month, year, or all time
- **Jam mode**: send someone a link and they can queue songs from their phone's
  browser without seeing anything else
- **OBS overlay** showing the current song over a transparent background
- **Discord status** from the desktop app
- **Self-updates**: the apps fetch new builds from your own server

## Layout

```
crates/server   axum server: API, audio, jam guest page and OBS overlay (pages/),
                SQL schema (migrations/)
crates/core     shared client code: API, sign-in, queue, player, device handover
crates/app      the Slint app for Linux, Windows and Android
crates/xtask    `cargo android`: builds the APK from android/ without Gradle
android/        Android manifest, Java glue and resources
packaging/      app icon
scripts/        publish-client.sh (self-update builds), package-client.sh (installers)
vendor/         patched Slint Android backend (see its PATCHED.md)
```

## Server setup

You need a [Supabase](https://supabase.com) project (the free tier is plenty;
only rows are stored there, never audio) and a music folder.

1. Copy `.env.example` to `.env.local` and fill it in. Every value is explained
   there. `DATABASE_URL` must be the session pooler string (port 5432);
   `OWNER_GITHUB_ID` is your numeric GitHub id, and leaving it unset locks
   everyone out.
2. Apply the schema to a new database, in order:

   ```bash
   for f in crates/server/migrations/*/migration.sql; do psql "$DATABASE_URL" -f "$f"; done
   ```

   One of these enables row level security and the Realtime policies. Skipping
   it exposes your data, since the anon key is public.
   `crates/server/migrations/player-channel-policy.live.sql` checks the policies
   against the live database and rolls everything back.
3. Set up GitHub sign-in: create an OAuth app at
   <https://github.com/settings/developers>, put its id and secret in Supabase
   under **Authentication → Providers → GitHub**, and enable **Anonymous**
   sign-ins (jam guests use them). Under **URL Configuration → Redirect URLs**
   add `http://127.0.0.1:53682/auth/callback` (desktop) and
   `uk.capzay.music://auth/callback` (Android).
4. Build and run from the folder holding `.env.local`:

   ```bash
   cargo build --release -p cpz-server
   BIND_ADDR=0.0.0.0:3006 target/release/cpz-server
   ```

   For a Raspberry Pi, cross-build on a faster machine with
   `cargo zigbuild --release -p cpz-server --target aarch64-unknown-linux-gnu.2.36`.
   Under systemd, order it after `network-online.target`.

Serve it over HTTPS for anything beyond your own network, for example with a
Cloudflare Tunnel pointing at the server's port.
Set `PUBLIC_URL` to that https address: apps connected over a private one
(Tailscale, LAN) use it for cover art in Discord, which only shows public
https images.

## Apps

```bash
cargo run -p cpz-app                 # desktop, in development
scripts/package-client.sh            # installers for Linux and Windows → dist/
cargo android                        # signed Android APK → dist/
scripts/publish-client.sh --windows --android   # put builds on the server for self-update
```

On first start the app asks for the server's address, then signs in with GitHub.

- **Linux:** `sh dist/cpz-music-<version>-linux-x86_64-setup.sh` installs to
  `~/.local` and adds a menu entry; add `--uninstall` to remove it.
- **Windows:** `Cpz-Music-Setup-<version>.exe` installs per user with a Start
  menu shortcut; remove it from Installed apps. Cross-built from Linux with
  cargo-zigbuild; the exe icon needs `llvm-windres`.
- **Android:** needs the Android SDK with build-tools, a platform and the NDK
  (`ANDROID_HOME`, default `~/Android/Sdk`), a JDK, and
  `rustup target add aarch64-linux-android`. See `crates/xtask`.

Bump `version` in `crates/app/Cargo.toml` before publishing: apps only update to
a higher version.

## Security

The repository is public, so it is written on the assumption that a stranger is
reading it. See also [SECURITY.md](SECURITY.md).

- **Access** is a single GitHub account. An unset `OWNER_GITHUB_ID` refuses
  everyone rather than falling open.
- **Guests** get a Supabase anonymous session whose jam scope lives in
  `app_metadata`, which only the service role can write, so it cannot be forged.
  Guest permissions are one deny-by-default allow-list, so a new endpoint is
  host-only until somebody deliberately opens it.
- **Row level security** is on for every table with no permissive policy, and
  the default `anon` and `authenticated` grants are revoked.
- **Realtime** policies let any signed-in user listen on the player channel but
  only the owner broadcast, so a guest cannot forge playback state.
- **File serving** refuses any path outside its root.
- **The two web pages** carry a strict Content Security Policy with a fresh
  nonce per response; every other response allows no script at all.
- **Rate limits** on jam joins and queue additions.
- **CI** runs rustfmt, clippy, the tests and a gitleaks secret scan.

## Development

```bash
cargo fmt --all && cargo clippy --workspace --all-targets
docker run -d --rm --name cpz-test-pg -e POSTGRES_PASSWORD=pg -p 55432:5432 postgres:17-alpine
TEST_DATABASE_URL=postgres://postgres:pg@localhost:55432/postgres cargo test --workspace
```

Without `TEST_DATABASE_URL` the server's database tests skip themselves.

## Licence

MIT. See [LICENCE](LICENCE).

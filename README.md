# Cpz Music

A self-hosted music streamer for a personal library. Your files stay on your own
disk; Supabase holds the database and handles sign-in. One Next.js process
serves the web app, the API, and the audio.

Runs as a PWA, a desktop app with Discord Rich Presence, and an Android app with
lock-screen controls. Any of them can act as a remote for whichever device is
actually playing.

> This is a rewrite. The previous version was a Fastify API, a Vite SPA, an nginx
> container and a separate authentication gateway, orchestrated by Docker
> Compose. It is now a single application with no Docker anywhere.

## Features

- **Library** scanned from disk, watched for changes, with album art extracted
  from tags
- **Player** with queue, shuffle, repeat, and OS media-key integration
- **Playlists** with reordering
- **Multi-device control**: one device plays, the others act as remotes
- **Offline**: download albums to a device and play them with no connection,
  including seeking. Listens recorded offline are replayed later on the day they
  actually happened
- **Listening stats** by week, month, year, or all time
- **Jam mode**: send someone a link and they can queue tracks from their phone
  without seeing anything else
- **OBS overlay** showing the current track over a transparent background

## Requirements

- Node.js 20.9 or newer
- A [Supabase](https://supabase.com) project (the free tier is plenty; only rows
  are stored there, never audio)
- A music folder

## Setup

```bash
git clone https://github.com/<you>/cpz-music.git
cd cpz-music
npm install
cp .env.example .env.local
```

Fill in `.env.local`. Every value is explained in `.env.example`; the ones worth
calling out:

| Variable | Notes |
| --- | --- |
| `DATABASE_URL` | The **session pooler** string (port 5432, host `*.pooler.supabase.com`). Direct `db.*.supabase.co` is IPv6-only; the pooler still answers on IPv4, which a home network usually needs. Do not use transaction mode (port 6543). |
| `SUPABASE_SERVICE_ROLE_KEY` | Bypasses row level security. Server-side only, and never given a `NEXT_PUBLIC_` prefix. |
| `OWNER_GITHUB_ID` | Your numeric GitHub id, from `https://api.github.com/users/<username>`. Only this account can reach the library, and leaving it unset locks everyone out rather than letting everyone in. |
| `APP_SECRET` | Signs jam invites and overlay links. `openssl rand -hex 32`. Changing it revokes every outstanding link. |
| `MUSIC_DIR` | Absolute path to your music. Only ever read from. |

Then set up GitHub sign-in:

1. Create an OAuth app at <https://github.com/settings/developers>.
2. Set the callback URL to `https://<your-domain>/auth/callback`.
3. Paste the client id and secret into Supabase under
   **Authentication → Providers → GitHub**.
4. Under **Authentication → Providers → Anonymous**, enable anonymous sign-ins.
   Jam guests use them.

Apply the schema and start:

```bash
npx prisma migrate deploy
npm run build
npm start
```

The first launch scans `MUSIC_DIR`. Large libraries take a few minutes; the app
is usable while it works. If this runs under systemd, wait for DNS before
starting the process (`After=network-online.target` and
`Wants=network-online.target`); otherwise the first scan can fail with
`EAI_AGAIN` at boot.

> **Build with your real environment.** `NEXT_PUBLIC_*` values are compiled into
> the bundle and into the Content Security Policy, so `npm run build` must run
> with the same `.env.local` that production uses. Building with placeholders
> produces a policy that blocks your own Supabase project.

### Exposing it

Any HTTPS reverse proxy works. A Cloudflare Tunnel avoids opening a port:

```bash
cloudflared tunnel --url http://localhost:3000
```

Plain HTTP is not an option beyond localhost: session cookies and the service
worker both require a secure context.

## Desktop and Android

Both are thin shells around the hosted site, so a web change reaches them
without a rebuild. Each is a separate workspace with its own lockfile, and
neither is installed by the web app.

```bash
cd electron && npm install && npm start          # dev
cd electron && npm run dist:linux                # AppImage

cd mobile && npm install && npx cap sync android
cd mobile && npm run build:debug                 # needs the Android SDK
```

Point them at your own deployment with `CPZ_SERVER_URL` (Electron) or
`server.url` in `mobile/capacitor.config.ts` (Android).

## Architecture

```
Browser / Electron / Android
        |
        |  HTTPS
        v
   Next.js (one process)
   |         |         |
   |         |         +-- reads audio + artwork from MUSIC_DIR
   |         +-- Prisma ------> Supabase Postgres
   +-- Supabase Auth + Realtime
```

**Everything in one app.** Pages are server components that query Prisma
directly, so there is no REST layer or client-side data fetching to maintain.
Audio and artwork are the exception: Next.js will not serve arbitrary
filesystem paths, so those are route handlers implementing HTTP Range.

**The scanner** needs a long-lived watcher, which a request-driven framework has
nowhere obvious to put. It runs from `instrumentation.ts`, the once-per-boot
hook, and shares the web server's process.

**Multi-device sync** carries no server state. Devices announce themselves over
Supabase Realtime presence and talk to each other over broadcast. The active
device is a pure function of presence (highest claim timestamp, device id
breaking ties), so every client independently agrees, no election message can be
lost, and handover is automatic when a device disconnects.

**Authorisation** lives in `src/lib/auth.ts` and nowhere else. The previous
design put identity in an nginx gateway and permissions in the app, so neither
could see the other and every change meant editing both.

### Layout

```
src/
├── app/              routes: pages, API handlers, jam, OBS overlay
├── lib/              auth, db, scanner, queue maths, tokens, realtime
├── components/
├── store/            zustand: player and downloads
└── instrumentation.ts
prisma/               schema and migrations, including RLS policies
electron/             desktop wrapper
mobile/               Capacitor Android wrapper
```

## Security

The repository is public, so it is written on the assumption that a stranger is
reading it.

- **Access** is a single GitHub account. An unset `OWNER_GITHUB_ID` refuses
  everyone rather than falling open.
- **Guests** get a Supabase anonymous session whose jam scope lives in
  `app_metadata`, which only the service role can write. It arrives as a signed
  JWT claim, so it cannot be forged the way the old design's proxy headers
  could. Guest permissions are one deny-by-default allow-list, so a new endpoint
  is host-only until somebody deliberately opens it.
- **Row level security** is on for every table with no permissive policy, and
  the default `anon` and `authenticated` grants are revoked. The anon key ships
  in the browser bundle and is public by design; this is what makes that safe.
- **Realtime** policies let any signed-in user listen on the player channel but
  only the owner broadcast, so a guest cannot forge playback state or drive
  someone else's device.
- **File serving** resolves every path and refuses anything outside its root,
  and the client's track type carries no `filePath`.
- **Rate limits** on jam joins and queue additions, keyed on the participant as
  well as the address.
- **CI** runs typecheck, lint, tests, build, `npm audit`, and a gitleaks secret
  scan on every push. Dependabot is on.
- No audio, artwork, database dumps, or signing keys are tracked. Git history
  starts clean, so there is no historical secret to scrub.

Found something? Open an issue, or email the address on my GitHub profile.

## Development

```bash
npm run dev         # dev server
npm test            # vitest
npm run typecheck
npm run lint
```

Tests cover the parts that can be silently wrong rather than everything: HTTP
Range parsing, path traversal, shuffle and queue navigation, playlist
reordering, invite token signing, the owner gate, the guest allow-list, and
listen-timestamp clamping.

## Licence

MIT. See [LICENCE](LICENCE).

# Cpz-Music rewrite design

Date: 2026-07-30
Status: approved

## Goal

Rebuild Cpz-Music, a self-hosted personal music streamer, on a stack that runs
without Docker and uses Supabase for Postgres and auth. The repository is
public on GitHub as a portfolio piece, so it must be safe to read: no secrets,
no exposed library contents, no obvious attack surface.

Feature parity with the old build is required. Nothing is being dropped.

## Constraints

- Audio files stay on local disk. Supabase stores database rows only.
- No Docker anywhere, including development.
- The previous auth design (an external nginx gateway injecting trusted
  headers) is rejected. Configuration split across two systems was the
  specific complaint. Auth must be configurable in one place.
- Public repository. Assume every committed byte is read by a stranger.

## Architecture

One Next.js 15 application, App Router, TypeScript, Tailwind 4. Two thin
wrapper workspaces (Electron, Android) that load the hosted site.

```
cpz-music/
├── prisma/schema.prisma        # ported from the old server, Supabase Postgres
├── src/
│   ├── instrumentation.ts      # starts the chokidar watcher once on boot
│   ├── middleware.ts           # Supabase session refresh + route gating
│   ├── app/
│   │   ├── (library)/          # home, album, artist, playlist, search, stats, downloads
│   │   ├── jam/[token]/        # guest entry point
│   │   ├── obs/                # now-playing overlay
│   │   └── api/                # stream, artwork, scan, playlists, stats, jam
│   ├── lib/                    # db, supabase clients, auth, scanner, metadata, realtime
│   ├── components/
│   └── store/                  # zustand player store
├── electron/
├── android/
└── public/sw.js
```

### Why one app rather than API plus SPA

The old split (Fastify API, Vite SPA, nginx) existed because Docker made three
containers cheap. Without Docker it is three processes to supervise. Merging
them removes the CORS configuration, the hand written API client layer, the
static file server, and the duplicated type definitions. Page code queries
Prisma directly.

### Long lived work in a request driven framework

The library scanner needs a persistent chokidar watcher. `next start` is a long
lived Node process, and Next.js provides `instrumentation.ts` as the official
once-per-boot hook. The watcher lives there. `scanner.ts` and `metadata.ts`
port over nearly unchanged.

### Streaming

Next.js will not serve arbitrary filesystem paths, so audio and artwork remain
route handlers using `fs.createReadStream` with HTTP Range support, equivalent
in size to the old `streamer.ts`.

### Multi-device sync

The old design was a 392 line in-memory hub over a raw WebSocket, with state
lost on every restart. It is replaced by Supabase Realtime:

- One channel carries presence and broadcast.
- Devices announce themselves through presence: `{deviceId, name, platform, isActive}`.
- The active device broadcasts `state` events (track, position, playing, queue, volume).
- Remotes broadcast `cmd` events, which the active device applies.
- The OBS overlay and jam guests subscribe read-only.

The server no longer holds playback state at all. `routes/sync.ts`,
`routes/player.ts`, and `services/sync.ts` are deleted rather than ported.

### Deployment

`next start` behind the existing Cloudflare Tunnel on music.capzay.uk.
Supabase hosts Postgres and Auth.

## Auth

Two roles, host and guest.

**Host** is a Supabase Auth user signing in with GitHub OAuth. `OWNER_GITHUB_ID`
decides who qualifies; anyone else who signs in is refused. No password is
stored and no shared secret exists to leak.

**Guest** gets a Supabase anonymous session. Opening a valid invite link causes
the server to verify the HMAC signed token, create the `JamParticipant` row,
and write `{jamId, pid, role: "guest"}` into that user's `app_metadata` using
the service role key. `app_metadata` is server writable only, so the value
arrives in the JWT as a claim the client cannot forge.

All permission logic lives in `lib/auth.ts`: one function returning the
identity, one allow-list of what a guest may reach. Changing a permission is a
single file edit. This is the property the old two-system design lacked.

## Security

- `.env.local` is gitignored. `.env.example` holds placeholders only.
- The service role key is server side only and never prefixed `NEXT_PUBLIC_`.
- RLS is enabled on every table. Prisma connects as the owner role and bypasses
  it, but the anon key is public by design and RLS is what makes that safe.
- Realtime Authorization policies restrict guests to subscribing, not sending.
- The stream and artwork handlers resolve the requested path and assert it sits
  under `MUSIC_DIR`, rejecting anything else. Covered by a test.
- Jam join and invite creation are rate limited.
- Invite tokens keep the old design: HMAC signed, 30 day TTL, per-jam epoch
  counter to revoke all outstanding links at once.
- No audio, artwork, database dumps, or private hostnames in the repository.
- CI on every push: typecheck, lint, build, `npm audit`, gitleaks secret scan.
  Dependabot enabled.
- Security headers and a CSP in `next.config.ts`.

Git history starts empty, so there is no historical secret to scrub.

## Testing

Vitest, applied only to logic that can be silently wrong:

- path traversal guard
- Range header parsing
- invite token sign and verify
- shuffle order generation
- playlist reordering

No component test suite, no end to end tests. A bug earns a test when it
appears.

## Known simplifications

- The scanner shares a process with the web server, so a full rescan of a large
  library competes with request handling. Acceptable at personal scale. Marked
  with a `ponytail:` comment naming the upgrade path (a separate worker
  process) rather than building that now.

## Milestones

Each milestone is independently runnable and gets its own implementation plan.

| # | Milestone | Outcome |
|---|---|---|
| 1 | Scaffold, Prisma on Supabase, GitHub OAuth owner gate, CI, security baseline | log in, see an empty library |
| 2 | Scanner, streaming and artwork handlers, browse and search | find and play a track |
| 3 | Player store, queue, shuffle, repeat, volume, MediaSession, playlists | daily driver on desktop |
| 4 | Realtime presence, remote control, OBS overlay | control playback from a phone |
| 5 | Service worker, downloads, offline listen queue, stats page | works offline |
| 6 | Jam invites, guest sessions, approval, guest queue adds | a friend can queue a song |
| 7 | Electron wrapper with Discord Rich Presence | music shows in Discord |
| 8 | Android Capacitor wrapper, MediaSession plugin, foreground service | lock screen controls |
| 9 | README with screenshots, final security pass, release | presentable to a stranger |

Milestones 1 to 3 are the substantial work. 7 and 8 are largely ports of code
that already works.

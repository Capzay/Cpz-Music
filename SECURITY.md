# Security

## Reporting

Please report vulnerabilities privately through
[GitHub's advisory form](https://github.com/capzay/cpz-music/security/advisories/new)
rather than a public issue. I will acknowledge within a few days.

This is a personal project maintained in spare time, so there is no bounty and
no formal response window.

## Threat model

Each deployment serves one owner. The realistic attackers are:

- **The open internet**, which can reach the deployment. Everything except the
  health check, the sign-in config, cover art, the jam entry point, and the
  token-gated overlay requires the owner's session.
- **A jam guest**, who is deliberately given a session. Guests are treated as
  hostile: they get a deny-by-default allow-list, cannot broadcast on the
  Realtime channel, cannot write listening history, and never receive host UI.
- **Anyone reading this repository**, which is public. It contains no secrets
  and no library contents.

Not in scope: the machine running the server being compromised, and the
Supabase project itself being compromised.

## Notes for anyone deploying this

- `OWNER_GITHUB_ID` unset denies everyone. Do not "fix" that by defaulting it.
- `SUPABASE_SERVICE_ROLE_KEY` bypasses row level security. It stays on the
  server and must not be committed.
- Apply every file in `crates/server/migrations/` before first use. One of them
  is what enables row level security and revokes the default `anon` grants; the
  Supabase anon key is public, so skipping it exposes your data.
- Serve over HTTPS. Jam guests' session cookies require a secure context.
- The OBS overlay link and jam invite links are bearer credentials. Anyone
  holding one can use it. Rotate `APP_SECRET` to invalidate all of them.

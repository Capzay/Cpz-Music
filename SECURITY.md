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
  login page, the jam entry point, and the token-gated overlay requires the
  owner's session.
- **A jam guest**, who is deliberately given a session. Guests are treated as
  hostile: they get a deny-by-default allow-list, cannot broadcast on the
  Realtime channel, cannot write listening history, and never receive host UI.
- **Anyone reading this repository**, which is public. It contains no secrets
  and no library contents.

Not in scope: the machine running the server being compromised, and the
Supabase project itself being compromised.

## Notes for anyone deploying this

- `OWNER_GITHUB_ID` unset denies everyone. Do not "fix" that by defaulting it.
- `SUPABASE_SERVICE_ROLE_KEY` bypasses row level security. It must never be
  given a `NEXT_PUBLIC_` prefix, and it must not be committed.
- Run `npx prisma migrate deploy` before first use. One of the migrations is
  what enables row level security and revokes the default `anon` grants; the
  Supabase anon key is public, so skipping it exposes your data.
- Serve over HTTPS. Session cookies and the service worker both require a
  secure context, and `capacitor.config.ts` refuses cleartext.
- The OBS overlay link and jam invite links are bearer credentials. Anyone
  holding one can use it. Rotate `APP_SECRET` to invalidate all of them.

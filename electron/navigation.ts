/**
 * Sign-in leaves our own origin before it comes back: Supabase issues the OAuth
 * redirect, GitHub shows the consent screen. Those hops have to stay in the app
 * window. Handing them to the system browser signs the browser in instead, since
 * the two have separate cookie jars and the PKCE verifier is in the window's.
 */
const AUTH_HOSTS = ["supabase.co", "github.com"];

/** Whether a URL may load in the app window rather than the system browser. */
export function isInternalUrl(url: string, serverUrl: string): boolean {
  let target: URL;
  let server: URL;
  try {
    target = new URL(url);
    server = new URL(serverUrl);
  } catch {
    return false;
  }

  // Compared as origins, not as a prefix: "https://music.capzay.uk" is a prefix
  // of "https://music.capzay.uk.example.com", which is somebody else's site.
  if (target.origin === server.origin) return true;

  // The auth hops carry the session, so plaintext is not good enough for them
  // even though a self-hosted server on plain http is allowed above.
  if (target.protocol !== "https:") return false;

  return AUTH_HOSTS.some((host) => target.hostname === host || target.hostname.endsWith(`.${host}`));
}

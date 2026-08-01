/**
 * The origin the browser actually used.
 *
 * Behind the tunnel the server binds to 0.0.0.0:3000, so `nextUrl.origin` is an
 * address the browser cannot reach: redirecting there sends the user somewhere
 * their cookies do not exist. The Host header carries the name they typed, and
 * a proxy in front moves it to X-Forwarded-Host.
 */
export function requestOrigin(headers: Headers): string {
  const host = headers.get("x-forwarded-host") ?? headers.get("host") ?? "localhost:3000";
  const protocol = headers.get("x-forwarded-proto") ?? "http";
  return `${protocol}://${host}`;
}

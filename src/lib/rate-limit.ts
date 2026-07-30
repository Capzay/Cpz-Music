/**
 * Fixed-window rate limiter.
 *
 * ponytail: in-process, so the counters reset on restart and are not shared
 * across instances. This app runs as a single process by design. If it ever
 * runs more than one, move the counters into Postgres or Redis.
 */
const windows = new Map<string, { count: number; resetAt: number }>();

export function rateLimit(key: string, limit: number, windowMs: number): boolean {
  const now = Date.now();
  const entry = windows.get(key);

  if (!entry || now >= entry.resetAt) {
    windows.set(key, { count: 1, resetAt: now + windowMs });
    // Opportunistic sweep so an attacker cannot grow this map without bound by
    // varying the key.
    if (windows.size > 10_000) {
      for (const [k, v] of windows) if (now >= v.resetAt) windows.delete(k);
    }
    return true;
  }

  if (entry.count >= limit) return false;
  entry.count += 1;
  return true;
}

/** Best-effort client address for rate-limit keys behind Cloudflare Tunnel. */
export function clientKey(request: Request): string {
  const headers = request.headers;
  return (
    headers.get("cf-connecting-ip") ??
    headers.get("x-forwarded-for")?.split(",")[0].trim() ??
    "unknown"
  );
}

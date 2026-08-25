/**
 * Runs once when the server boots. This is the only place in a request-driven
 * framework where a long-lived watcher can legitimately live.
 */

const TRANSIENT_DB_CODES = new Set([
  "EAI_AGAIN",
  "ENETUNREACH",
  "ECONNREFUSED",
  "ETIMEDOUT",
  "ENOTFOUND",
  "ECONNRESET",
  "P1001",
]);

function isTransientDbError(err: unknown): boolean {
  return typeof err === "object" && err !== null && "code" in err && TRANSIENT_DB_CODES.has(String(err.code));
}

export async function register() {
  // Only the Node.js server runtime can touch the filesystem.
  if (process.env.NEXT_RUNTIME !== "nodejs") return;
  if (!process.env.MUSIC_DIR) {
    console.warn("[startup] MUSIC_DIR is not set, skipping library scan");
    return;
  }

  // Not awaited: a large library would otherwise delay the server accepting
  // requests, and the UI copes with an empty library while this runs.
  void bootLibrary();
}

async function bootLibrary() {
  const { scanLibrary, watchLibrary } = await import("@/lib/scanner");
  const delaysMs = [2_000, 5_000, 15_000, 30_000];

  for (let attempt = 0; ; attempt++) {
    try {
      await scanLibrary();
      watchLibrary();
      return;
    } catch (err) {
      const wait = delaysMs[attempt];
      if (wait === undefined || !isTransientDbError(err)) {
        console.error("[startup] library scan failed:", err);
        return;
      }
      console.warn(`[startup] database unreachable, retrying scan in ${wait}ms:`, err);
      await new Promise((resolve) => setTimeout(resolve, wait));
    }
  }
}

/**
 * Runs once when the server boots. This is the only place in a request-driven
 * framework where a long-lived watcher can legitimately live.
 */
export async function register() {
  // Only the Node.js server runtime can touch the filesystem.
  if (process.env.NEXT_RUNTIME !== "nodejs") return;
  if (!process.env.MUSIC_DIR) {
    console.warn("[startup] MUSIC_DIR is not set, skipping library scan");
    return;
  }

  const { scanLibrary, watchLibrary } = await import("@/lib/scanner");

  // Not awaited: a large library would otherwise delay the server accepting
  // requests, and the UI copes with an empty library while this runs.
  void scanLibrary()
    .then(() => watchLibrary())
    .catch((err) => console.error("[startup] library scan failed:", err));
}

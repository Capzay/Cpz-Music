/**
 * In-app offline routing for pages that read local state (playlists, downloads).
 *
 * Next soft navigations need a network round-trip for RSC. A full reload depends
 * on a separately cached document and wipes the in-memory Zustand stores. While
 * the shell is already running offline, pushState + a client outlet is enough.
 */

const listeners = new Set<() => void>();

let offlinePath: string | null = null;

function emit() {
  for (const listener of listeners) listener();
}

export function getOfflinePath(): string | null {
  return offlinePath;
}

export function subscribeOfflinePath(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function clearOfflinePath() {
  if (offlinePath === null) return;
  offlinePath = null;
  emit();
}

function hrefToPath(href: string): string {
  const url = new URL(href, location.origin);
  return `${url.pathname}${url.search}${url.hash}`;
}

/** Paths the client outlet can render without RSC. */
export function isOfflineClientPath(path: string): boolean {
  const pathname = path.split(/[?#]/)[0] ?? path;
  return (
    pathname === "/playlists" ||
    pathname.startsWith("/playlists/") ||
    pathname === "/downloads"
  );
}

/** Record the current URL as the offline outlet path (cold start / popstate). */
export function syncOfflinePathFromLocation() {
  if (typeof navigator === "undefined" || navigator.onLine) {
    clearOfflinePath();
    return;
  }
  const next = `${location.pathname}${location.search}${location.hash}`;
  if (!isOfflineClientPath(next)) {
    clearOfflinePath();
    return;
  }
  if (offlinePath === next) return;
  offlinePath = next;
  emit();
}

export function navigateOffline(href: string) {
  // Library HTML comes from the server; only a real document load can restore it.
  const url = new URL(href, location.origin);
  if (url.pathname === "/") {
    clearOfflinePath();
    location.assign(url.pathname + url.search + url.hash);
    return;
  }

  const next = hrefToPath(href);
  offlinePath = next;
  history.pushState({ cpzOffline: true }, "", next);
  emit();
}

export function replaceOffline(href: string) {
  const next = hrefToPath(href);
  offlinePath = next;
  history.replaceState({ cpzOffline: true }, "", next);
  emit();
}

/**
 * Soft-nav while online; client outlet while offline (no document reload).
 * Used by playlist flows that previously called location.assign.
 */
export function goOfflineAware(href: string, push: (href: string) => void) {
  if (typeof navigator !== "undefined" && !navigator.onLine) {
    navigateOffline(href);
    return;
  }
  clearOfflinePath();
  push(href);
}

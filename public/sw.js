/* Cpz Music service worker.
 *
 * Caches:
 *  - cpz-static-v1  : /_next/static/* build output, content-hashed so cache-first
 *  - cpz-pages-v1   : server-rendered HTML, network-first with a cache fallback
 *  - cpz-audio-v1   : explicitly downloaded tracks, served with Range support
 *                     so seeking works offline
 *  - cpz-artwork-v2 : album art, stale-while-revalidate. The URL is keyed by
 *                     album id, not by content, so a rescan changes the bytes
 *                     behind a URL we already hold. Cache-first would serve the
 *                     old cover until someone cleared the cache by hand.
 *
 * Nothing authenticated or live is cached: no stats, no jam, no now-playing.
 */

const STATIC_CACHE = "cpz-static-v1";
const PAGE_CACHE = "cpz-pages-v1";
const AUDIO_CACHE = "cpz-audio-v1";
const ARTWORK_CACHE = "cpz-artwork-v2";

const KNOWN = [STATIC_CACHE, PAGE_CACHE, AUDIO_CACHE, ARTWORK_CACHE];

const STREAM_RE = /^\/api\/tracks\/\d+\/stream$/;
const ARTWORK_RE = /^\/api\/artwork\/\d+$/;

self.addEventListener("install", () => self.skipWaiting());

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      const names = await caches.keys();
      await Promise.all(
        names.filter((n) => n.startsWith("cpz-") && !KNOWN.includes(n)).map((n) => caches.delete(n)),
      );
      await self.clients.claim();
    })(),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;

  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;

  if (request.mode === "navigate") {
    event.respondWith(handleNavigation(request));
    return;
  }
  if (STREAM_RE.test(url.pathname)) {
    event.respondWith(handleAudio(request, url));
    return;
  }
  if (ARTWORK_RE.test(url.pathname)) {
    event.respondWith(staleWhileRevalidate(event, ARTWORK_CACHE));
    return;
  }
  if (url.pathname.startsWith("/_next/static/")) {
    event.respondWith(cacheFirst(request, STATIC_CACHE));
  }
});

/** True for anything that must never end up in a cache. */
function isSensitive(url) {
  return (
    url.pathname === "/login" ||
    url.pathname.startsWith("/auth/") ||
    url.pathname.startsWith("/jam") ||
    url.pathname === "/obs" ||
    url.pathname.startsWith("/api/now-playing") ||
    url.pathname.startsWith("/api/stats")
  );
}

async function handleNavigation(request) {
  const url = new URL(request.url);
  try {
    const response = await fetch(request);

    // Only cache a real page. A redirect means the auth gate turned us away, and
    // caching that would serve the login page offline forever after.
    if (response.ok && !response.redirected && !isSensitive(url)) {
      const cache = await caches.open(PAGE_CACHE);
      cache.put(url.pathname, response.clone()).catch(() => {});
    }
    return response;
  } catch {
    const cached = await caches.match(url.pathname, { cacheName: PAGE_CACHE });
    if (cached) return cached;

    const downloads = await caches.match("/downloads", { cacheName: PAGE_CACHE });
    if (downloads) return downloads;

    return new Response(
      "<h1>Offline</h1><p>Open this page once while online to make it available offline.</p>",
      { status: 503, headers: { "Content-Type": "text/html" } },
    );
  }
}

async function cacheFirst(request, cacheName) {
  const cache = await caches.open(cacheName);
  const cached = await cache.match(request.url);
  if (cached) return cached;
  const response = await fetch(request);
  if (response.ok) cache.put(request.url, response.clone()).catch(() => {});
  return response;
}

/**
 * Answers from cache immediately, then refreshes the entry in the background so
 * the next load gets the new bytes. One stale paint after a rescan, rather than
 * a stale cover forever.
 */
async function staleWhileRevalidate(event, cacheName) {
  const request = event.request;
  const cache = await caches.open(cacheName);
  const cached = await cache.match(request.url);

  const fetched = fetch(request).then((response) => {
    if (response.ok) cache.put(request.url, response.clone()).catch(() => {});
    return response;
  });

  if (!cached) return fetched;

  // respondWith settles straight away with the cached copy, so without
  // waitUntil the worker can be shut down before the refresh lands.
  event.waitUntil(fetched.catch(() => {}));
  return cached;
}

/**
 * Serves a downloaded track from cache, honouring Range so the audio element
 * can seek. Cache API stores whole responses, so a partial request is answered
 * by slicing the stored blob ourselves.
 */
async function handleAudio(request, url) {
  const cache = await caches.open(AUDIO_CACHE);
  const cached = await cache.match(url.origin + url.pathname);
  if (!cached) return fetch(request);

  const range = request.headers.get("range");
  if (!range) return cached;

  const blob = await cached.blob();
  const size = blob.size;
  const match = /^bytes=(\d*)-(\d*)$/.exec(range.trim());
  if (!match) return cached;

  let start;
  let end;
  if (match[1] === "") {
    // Suffix form: the last N bytes.
    const suffix = Number(match[2]);
    if (!suffix) return rangeNotSatisfiable(size);
    start = Math.max(0, size - suffix);
    end = size - 1;
  } else {
    start = Number(match[1]);
    end = match[2] === "" ? size - 1 : Math.min(Number(match[2]), size - 1);
  }

  if (start >= size || end < start) return rangeNotSatisfiable(size);

  return new Response(blob.slice(start, end + 1), {
    status: 206,
    headers: {
      "Content-Range": `bytes ${start}-${end}/${size}`,
      "Accept-Ranges": "bytes",
      "Content-Length": String(end - start + 1),
      "Content-Type": cached.headers.get("Content-Type") || "audio/mpeg",
    },
  });
}

function rangeNotSatisfiable(size) {
  return new Response(null, {
    status: 416,
    headers: { "Content-Range": `bytes */${size}`, "Accept-Ranges": "bytes" },
  });
}

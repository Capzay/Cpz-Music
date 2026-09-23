import { streamUrl } from "@/lib/types";

/**
 * Same bucket `public/sw.js` reads. The page writes whole-file responses here;
 * playback reads them back as blob URLs rather than asking the service worker
 * to satisfy the media element's Range request.
 *
 * A media element issues a no-cors range request. The worker can only answer
 * that from Cache Storage by synthesizing a 206, and browsers reject that
 * response, so a downloaded track never starts while offline.
 */
export const AUDIO_CACHE = "cpz-audio-v1";

const objectUrls = new Map<number, string>();
const pending = new Map<number, Promise<string | null>>();

export function audioCacheKey(trackId: number): string {
  return new URL(streamUrl(trackId), location.origin).href;
}

/** True when this track should be read from Cache Storage instead of the network. */
export function preferCachedAudio(downloaded: boolean): boolean {
  if (downloaded) return true;
  return typeof navigator !== "undefined" && navigator.onLine === false;
}

export function peekOfflineUrl(trackId: number): string | null {
  return objectUrls.get(trackId) ?? null;
}

export function releaseOfflineUrl(trackId: number) {
  const url = objectUrls.get(trackId);
  if (!url) return;
  URL.revokeObjectURL(url);
  objectUrls.delete(trackId);
}

/** Drop every blob URL except the tracks still queued to play. */
export function retainOfflineUrls(keep: number[]) {
  const held = new Set(keep);
  for (const id of [...objectUrls.keys()]) {
    if (!held.has(id)) releaseOfflineUrl(id);
  }
}

export function clearOfflineUrls() {
  for (const id of [...objectUrls.keys()]) releaseOfflineUrl(id);
  pending.clear();
}

async function readCached(trackId: number): Promise<string | null> {
  if (typeof caches === "undefined") return null;
  try {
    const cache = await caches.open(AUDIO_CACHE);
    const cached = await cache.match(audioCacheKey(trackId));
    if (!cached) return null;
    const raw = await cached.blob();
    if (raw.size === 0) return null;
    const type = raw.type || cached.headers.get("Content-Type") || "audio/mpeg";
    const blob = raw.type === type ? raw : new Blob([raw], { type });
    const url = URL.createObjectURL(blob);
    objectUrls.set(trackId, url);
    return url;
  } catch {
    return null;
  }
}

/** Object URL for a cached track, or null when those bytes are not on the device. */
export function offlineObjectUrl(trackId: number): Promise<string | null> {
  const existing = objectUrls.get(trackId);
  if (existing) return Promise.resolve(existing);
  const inflight = pending.get(trackId);
  if (inflight) return inflight;

  const promise = readCached(trackId).finally(() => {
    pending.delete(trackId);
  });
  pending.set(trackId, promise);
  return promise;
}

/**
 * Blob URL when the track is on device, otherwise the streaming endpoint.
 * `preferCache` is false for an ordinary online play so we do not touch
 * Cache Storage on every skip.
 */
export async function playbackSrc(trackId: number, preferCache: boolean): Promise<string> {
  if (preferCache) {
    const local = await offlineObjectUrl(trackId);
    if (local) return local;
  }
  return streamUrl(trackId);
}

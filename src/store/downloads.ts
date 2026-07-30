"use client";

import { create } from "zustand";
import { streamUrl, type PlayerTrack } from "@/lib/types";

/**
 * Explicit offline downloads. Audio bytes live in the Cache Storage bucket the
 * service worker reads from; this store only tracks what is in there, because
 * enumerating a cache of whole albums on every render is far too slow.
 */

const AUDIO_CACHE = "cpz-audio-v1";
const REGISTRY_KEY = "cpz-downloads-v1";
const CONCURRENCY = 2;

export interface DownloadedTrack {
  track: PlayerTrack;
  bytes: number;
  at: number;
}

type Registry = Record<string, DownloadedTrack>;

function loadRegistry(): Registry {
  try {
    const raw = JSON.parse(localStorage.getItem(REGISTRY_KEY) ?? "{}");
    return raw && typeof raw === "object" ? (raw as Registry) : {};
  } catch {
    return {};
  }
}

function saveRegistry(registry: Registry) {
  try {
    localStorage.setItem(REGISTRY_KEY, JSON.stringify(registry));
  } catch {
    // Quota. The audio is still cached; only the index is lost.
  }
}

interface DownloadState {
  registry: Registry;
  /** Track ids queued or downloading right now. */
  pending: number[];
  active: number[];
  failed: number[];
  hydrate: () => void;
  download: (tracks: PlayerTrack[]) => void;
  remove: (trackId: number) => Promise<void>;
  clear: () => Promise<void>;
}

export const useDownloads = create<DownloadState>((set, get) => {
  const queue: PlayerTrack[] = [];
  let running = 0;

  async function fetchOne(track: PlayerTrack) {
    const url = new URL(streamUrl(track.id), location.origin).toString();
    const response = await fetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);

    const body = await response.blob();
    const cache = await caches.open(AUDIO_CACHE);
    // Store a complete 200 response; the service worker slices it for Range.
    await cache.put(
      url,
      new Response(body, {
        status: 200,
        headers: {
          "Content-Type": response.headers.get("Content-Type") ?? "audio/mpeg",
          "Content-Length": String(body.size),
        },
      }),
    );

    const registry = { ...get().registry, [track.id]: { track, bytes: body.size, at: Date.now() } };
    saveRegistry(registry);
    set({ registry });
  }

  function pump() {
    while (running < CONCURRENCY && queue.length > 0) {
      const track = queue.shift()!;
      running += 1;
      set((s) => ({
        active: [...s.active, track.id],
        pending: s.pending.filter((id) => id !== track.id),
      }));

      void fetchOne(track)
        .catch(() => set((s) => ({ failed: [...s.failed, track.id] })))
        .finally(() => {
          running -= 1;
          set((s) => ({ active: s.active.filter((id) => id !== track.id) }));
          pump();
        });
    }
  }

  return {
    registry: {},
    pending: [],
    active: [],
    failed: [],

    // localStorage is not readable during SSR, so this runs from an effect.
    hydrate: () => set({ registry: loadRegistry() }),

    download: (tracks) => {
      const { registry, pending, active } = get();
      const wanted = tracks.filter(
        (t) => !registry[t.id] && !pending.includes(t.id) && !active.includes(t.id),
      );
      if (wanted.length === 0) return;
      queue.push(...wanted);
      set((s) => ({
        pending: [...s.pending, ...wanted.map((t) => t.id)],
        failed: s.failed.filter((id) => !wanted.some((t) => t.id === id)),
      }));
      pump();
    },

    remove: async (trackId) => {
      const cache = await caches.open(AUDIO_CACHE);
      await cache.delete(new URL(streamUrl(trackId), location.origin).toString());
      const registry = { ...get().registry };
      delete registry[trackId];
      saveRegistry(registry);
      set({ registry });
    },

    clear: async () => {
      await caches.delete(AUDIO_CACHE);
      saveRegistry({});
      set({ registry: {}, failed: [] });
    },
  };
});

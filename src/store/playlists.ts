"use client";

import { create } from "zustand";
import { moveItem } from "@/lib/reorder";
import type { PlayerTrack } from "@/lib/types";
import {
  PLAYLIST_SYNC_PATH,
  cleanName,
  loadLocalPlaylists,
  mergeSnapshot,
  parseSnapshots,
  saveLocalPlaylists,
  toDirty,
  type LocalPlaylist,
} from "@/lib/playlist-sync";

interface PlaylistState {
  playlists: LocalPlaylist[];
  hydrated: boolean;
  syncing: boolean;
  hydrate: () => void;
  sync: () => Promise<void>;
  create: (name: string) => string | null;
  rename: (uuid: string, name: string) => void;
  remove: (uuid: string) => void;
  addTracks: (uuid: string, tracks: PlayerTrack[]) => void;
  removeTrack: (uuid: string, index: number) => void;
  moveTrack: (uuid: string, from: number, to: number) => void;
}

function persist(playlists: LocalPlaylist[]) {
  saveLocalPlaylists(playlists);
  return playlists;
}

function touch(
  playlists: LocalPlaylist[],
  uuid: string,
  patch: (current: LocalPlaylist) => LocalPlaylist,
): LocalPlaylist[] {
  return persist(
    playlists.map((playlist) => {
      if (playlist.uuid !== uuid || playlist.deleted) return playlist;
      return patch({
        ...playlist,
        dirty: true,
        updatedAt: Math.max(Date.now(), playlist.updatedAt + 1),
      });
    }),
  );
}

let syncStarted = false;

export const usePlaylists = create<PlaylistState>((set, get) => ({
  playlists: [],
  hydrated: false,
  syncing: false,

  // localStorage is not readable during SSR, so this runs from an effect.
  hydrate: () => set({ playlists: loadLocalPlaylists(), hydrated: true }),

  sync: async () => {
    if (get().syncing) return;
    if (typeof navigator !== "undefined" && !navigator.onLine) return;
    set({ syncing: true });

    const posted = get()
      .playlists.filter((p) => p.dirty)
      .map((p) => ({ uuid: p.uuid, updatedAt: p.updatedAt }));
    const postedAt = new Map(posted.map((p) => [p.uuid, p.updatedAt]));

    try {
      const dirty = get().playlists.filter((p) => p.dirty);
      const response =
        dirty.length > 0
          ? await fetch(PLAYLIST_SYNC_PATH, {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              body: JSON.stringify({ playlists: dirty.map(toDirty) }),
            })
          : await fetch(PLAYLIST_SYNC_PATH);
      if (!response.ok) return;
      const remote = parseSnapshots(await response.json());
      if (!remote) return;

      set((state) => {
        const stillDirty = state.playlists.filter((p) => {
          if (!p.dirty) return false;
          const sent = postedAt.get(p.uuid);
          return sent === undefined || p.updatedAt !== sent;
        });
        return { playlists: persist(mergeSnapshot(stillDirty, remote)) };
      });
    } catch {
      // Unreachable. Keep the local docs and retry on the next online event.
    } finally {
      set({ syncing: false });
    }
  },

  create: (name) => {
    const cleaned = cleanName(name);
    if (!cleaned) return null;
    const uuid = crypto.randomUUID();
    const playlist: LocalPlaylist = {
      uuid,
      id: null,
      name: cleaned,
      updatedAt: Date.now(),
      tracks: [],
      dirty: true,
    };
    set((s) => ({ playlists: persist([playlist, ...s.playlists]) }));
    void get().sync();
    return uuid;
  },

  rename: (uuid, name) => {
    const cleaned = cleanName(name);
    if (!cleaned) return;
    set((s) => ({
      playlists: touch(s.playlists, uuid, (p) => ({ ...p, name: cleaned })),
    }));
    void get().sync();
  },

  remove: (uuid) => {
    set((s) => ({
      playlists: persist(
        s.playlists.map((p) =>
          p.uuid === uuid
            ? { ...p, deleted: true, dirty: true, updatedAt: Math.max(Date.now(), p.updatedAt + 1) }
            : p,
        ),
      ),
    }));
    void get().sync();
  },

  addTracks: (uuid, tracks) => {
    if (tracks.length === 0) return;
    set((s) => ({
      playlists: touch(s.playlists, uuid, (p) => ({ ...p, tracks: [...p.tracks, ...tracks] })),
    }));
    void get().sync();
  },

  removeTrack: (uuid, index) => {
    set((s) => ({
      playlists: touch(s.playlists, uuid, (p) => ({
        ...p,
        tracks: p.tracks.filter((_, i) => i !== index),
      })),
    }));
    void get().sync();
  },

  moveTrack: (uuid, from, to) => {
    set((s) => ({
      playlists: touch(s.playlists, uuid, (p) => ({
        ...p,
        tracks: moveItem(p.tracks, from, to),
      })),
    }));
    void get().sync();
  },
}));

export function startPlaylistSync() {
  if (typeof window === "undefined" || syncStarted) return;
  syncStarted = true;
  usePlaylists.getState().hydrate();
  void usePlaylists.getState().sync();
  window.addEventListener("online", () => void usePlaylists.getState().sync());
}

import type { PlayerTrack } from "@/lib/types";

/**
 * Local-first playlists, one document per playlist.
 *
 * One host, several devices: the whole playlist is the conflict unit. Concurrent
 * edits on two devices — later `updatedAt` wins. There is no CRDT and no
 * per-track op log.
 */

export const PLAYLISTS_KEY = "cpz-playlists-v1";
export const PLAYLIST_SYNC_PATH = "/api/playlists/sync";

export const MAX_NAME = 120;
export const MAX_TRACKS = 2000;
export const MAX_SYNC_DOCS = 100;

const UUID_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export interface PlaylistSnapshot {
  uuid: string;
  id: number;
  name: string;
  updatedAt: number;
  tracks: PlayerTrack[];
}

export interface DirtyPlaylist {
  uuid: string;
  name: string;
  updatedAt: number;
  trackIds: number[];
  deleted?: boolean;
}

export interface LocalPlaylist {
  uuid: string;
  id: number | null;
  name: string;
  updatedAt: number;
  tracks: PlayerTrack[];
  deleted?: boolean;
  dirty: boolean;
}

export type WriteDecision = "create" | "replace" | "delete" | "skip";

export function isUuid(value: string): boolean {
  return UUID_RE.test(value);
}

export function cleanName(raw: unknown): string {
  return String(raw ?? "")
    .trim()
    .slice(0, MAX_NAME);
}

export function decideWrite(
  incomingUpdatedAt: number,
  incomingDeleted: boolean,
  existingUpdatedAt: number | null,
): WriteDecision {
  if (existingUpdatedAt === null) {
    return incomingDeleted ? "skip" : "create";
  }
  if (incomingUpdatedAt < existingUpdatedAt) return "skip";
  return incomingDeleted ? "delete" : "replace";
}

/** Drops ids the library no longer has, keeping the incoming order. */
export function filterKnownTrackIds(trackIds: number[], known: ReadonlySet<number>): number[] {
  return trackIds.filter((id) => known.has(id)).slice(0, MAX_TRACKS);
}

/**
 * Reconcile a server snapshot with local docs that have not been acknowledged
 * yet. Dirty local docs win; everything else is replaced by the snapshot.
 * A non-dirty local playlist missing from the snapshot was deleted elsewhere.
 */
export function mergeSnapshot(
  dirtyLocal: LocalPlaylist[],
  remote: PlaylistSnapshot[],
): LocalPlaylist[] {
  const dirtyByUuid = new Map(dirtyLocal.filter((p) => p.dirty).map((p) => [p.uuid, p]));
  const result: LocalPlaylist[] = [];
  const seen = new Set<string>();

  for (const r of remote) {
    seen.add(r.uuid);
    const local = dirtyByUuid.get(r.uuid);
    if (local) {
      result.push(local);
      continue;
    }
    result.push({
      uuid: r.uuid,
      id: r.id,
      name: r.name,
      updatedAt: r.updatedAt,
      tracks: r.tracks,
      dirty: false,
    });
  }

  for (const local of dirtyByUuid.values()) {
    if (!seen.has(local.uuid)) result.push(local);
  }

  return result.sort((a, b) => b.updatedAt - a.updatedAt);
}

export function toDirty(playlist: LocalPlaylist): DirtyPlaylist {
  return {
    uuid: playlist.uuid,
    name: playlist.name,
    updatedAt: playlist.updatedAt,
    trackIds: playlist.deleted ? [] : playlist.tracks.map((t) => t.id),
    ...(playlist.deleted ? { deleted: true } : {}),
  };
}

export function parseDirtyPlaylists(body: unknown): DirtyPlaylist[] | null {
  if (!body || typeof body !== "object") return null;
  const playlists = (body as { playlists?: unknown }).playlists;
  if (!Array.isArray(playlists) || playlists.length > MAX_SYNC_DOCS) return null;

  const parsed: DirtyPlaylist[] = [];
  for (const item of playlists) {
    const doc = parseDirtyPlaylist(item);
    if (!doc) return null;
    parsed.push(doc);
  }
  return parsed;
}

function parseDirtyPlaylist(value: unknown): DirtyPlaylist | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  if (typeof raw.uuid !== "string" || !isUuid(raw.uuid)) return null;
  if (typeof raw.updatedAt !== "number" || !Number.isFinite(raw.updatedAt) || raw.updatedAt < 0) {
    return null;
  }
  if (raw.deleted === true) {
    return {
      uuid: raw.uuid,
      name: cleanName(raw.name),
      updatedAt: raw.updatedAt,
      trackIds: [],
      deleted: true,
    };
  }
  const name = cleanName(raw.name);
  if (!name) return null;
  if (!Array.isArray(raw.trackIds) || raw.trackIds.length > MAX_TRACKS) return null;
  if (!raw.trackIds.every((id) => Number.isInteger(id) && (id as number) > 0)) return null;
  return {
    uuid: raw.uuid,
    name,
    updatedAt: raw.updatedAt,
    trackIds: raw.trackIds as number[],
  };
}

export function parseSnapshots(body: unknown): PlaylistSnapshot[] | null {
  if (!body || typeof body !== "object") return null;
  const playlists = (body as { playlists?: unknown }).playlists;
  if (!Array.isArray(playlists)) return null;
  const parsed: PlaylistSnapshot[] = [];
  for (const item of playlists) {
    const doc = parseSnapshot(item);
    if (!doc) return null;
    parsed.push(doc);
  }
  return parsed;
}

function parseSnapshot(value: unknown): PlaylistSnapshot | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  if (typeof raw.uuid !== "string" || !isUuid(raw.uuid)) return null;
  if (!Number.isInteger(raw.id) || (raw.id as number) <= 0) return null;
  if (typeof raw.name !== "string") return null;
  if (typeof raw.updatedAt !== "number" || !Number.isFinite(raw.updatedAt)) return null;
  if (!Array.isArray(raw.tracks) || !raw.tracks.every(isPlayerTrack)) return null;
  return {
    uuid: raw.uuid,
    id: raw.id as number,
    name: raw.name,
    updatedAt: raw.updatedAt,
    tracks: raw.tracks,
  };
}

export function isPlayerTrack(value: unknown): value is PlayerTrack {
  if (!value || typeof value !== "object") return false;
  const track = value as Record<string, unknown>;
  if (!Number.isInteger(track.id) || typeof track.title !== "string") return false;
  if (track.duration !== null && typeof track.duration !== "number") return false;
  if (track.trackNumber !== null && !Number.isInteger(track.trackNumber)) return false;
  if (!Number.isInteger(track.discNumber)) return false;
  const artist = track.artist;
  const album = track.album;
  if (!artist || typeof artist !== "object" || !album || typeof album !== "object") return false;
  const a = artist as Record<string, unknown>;
  const b = album as Record<string, unknown>;
  return (
    Number.isInteger(a.id) &&
    typeof a.name === "string" &&
    Number.isInteger(b.id) &&
    typeof b.title === "string" &&
    typeof b.hasArtwork === "boolean"
  );
}

export function loadLocalPlaylists(): LocalPlaylist[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(PLAYLISTS_KEY) ?? "[]");
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(isLocalPlaylist);
  } catch {
    return [];
  }
}

export function saveLocalPlaylists(playlists: LocalPlaylist[]) {
  try {
    localStorage.setItem(PLAYLISTS_KEY, JSON.stringify(playlists));
  } catch {
    // Quota. The in-memory store still has the latest; the next hydrate may
    // roll back to whatever fit last time.
  }
}

function isLocalPlaylist(value: unknown): value is LocalPlaylist {
  if (!value || typeof value !== "object") return false;
  const raw = value as Record<string, unknown>;
  if (typeof raw.uuid !== "string" || !isUuid(raw.uuid)) return false;
  if (raw.id !== null && (!Number.isInteger(raw.id) || (raw.id as number) <= 0)) return false;
  if (typeof raw.name !== "string") return false;
  if (typeof raw.updatedAt !== "number" || !Number.isFinite(raw.updatedAt)) return false;
  if (!Array.isArray(raw.tracks) || !raw.tracks.every(isPlayerTrack)) return false;
  if (raw.deleted !== undefined && raw.deleted !== true) return false;
  if (typeof raw.dirty !== "boolean") return false;
  return true;
}

export function visiblePlaylists(playlists: LocalPlaylist[]): LocalPlaylist[] {
  return playlists.filter((p) => !p.deleted);
}

export function findPlaylist(playlists: LocalPlaylist[], id: string): LocalPlaylist | undefined {
  return visiblePlaylists(playlists).find((p) => p.uuid === id || String(p.id) === id);
}

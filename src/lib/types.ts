/**
 * What the browser is allowed to know about a track.
 *
 * Deliberately excludes `filePath`: the old client type carried it, which told
 * anyone with dev tools open how the server's disk is laid out. Audio is
 * addressed by id through /api/tracks/[id]/stream instead.
 */
export interface PlayerTrack {
  id: number;
  title: string;
  duration: number | null;
  trackNumber: number | null;
  discNumber: number;
  artist: { id: number; name: string };
  album: { id: number; title: string; hasArtwork: boolean };
}

export type RepeatMode = "off" | "all" | "one";

export const trackSelect = {
  id: true,
  title: true,
  duration: true,
  trackNumber: true,
  discNumber: true,
  artist: { select: { id: true, name: true } },
  album: { select: { id: true, title: true, artworkPath: true } },
} as const;

type TrackRow = {
  id: number;
  title: string;
  duration: number | null;
  trackNumber: number | null;
  discNumber: number;
  artist: { id: number; name: string };
  album: { id: number; title: string; artworkPath: string | null };
};

/** Strips server-side fields before a track crosses into a client component. */
export function toPlayerTrack(row: TrackRow): PlayerTrack {
  return {
    id: row.id,
    title: row.title,
    duration: row.duration,
    trackNumber: row.trackNumber,
    discNumber: row.discNumber,
    artist: row.artist,
    album: {
      id: row.album.id,
      title: row.album.title,
      hasArtwork: row.album.artworkPath !== null,
    },
  };
}

export const streamUrl = (trackId: number) => `/api/tracks/${trackId}/stream`;
export const artworkUrl = (albumId: number) => `/api/artwork/${albumId}`;

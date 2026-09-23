import "server-only";

import { prisma } from "@/lib/db";
import { toPlayerTrack, trackSelect, type PlayerTrack } from "@/lib/types";

export type SearchAlbumHit = {
  id: number;
  title: string;
  year: number | null;
  artist: string;
  hasArtwork: boolean;
};

export type SearchArtistHit = {
  id: number;
  name: string;
};

export type SearchResults = {
  tracks: PlayerTrack[];
  albums: SearchAlbumHit[];
  artists: SearchArtistHit[];
};

export async function searchLibrary(query: string): Promise<SearchResults> {
  const q = query.trim();
  if (!q) return { tracks: [], albums: [], artists: [] };

  const contains = { contains: q, mode: "insensitive" as const };

  const [tracks, albums, artists] = await Promise.all([
    prisma.track.findMany({
      where: { title: contains },
      take: 40,
      orderBy: { title: "asc" },
      select: trackSelect,
    }),
    prisma.album.findMany({
      where: { title: contains },
      take: 20,
      orderBy: { title: "asc" },
      select: {
        id: true,
        title: true,
        year: true,
        artworkPath: true,
        artist: { select: { name: true } },
      },
    }),
    prisma.artist.findMany({
      where: { name: contains, tracks: { some: {} } },
      take: 20,
      orderBy: { name: "asc" },
      select: { id: true, name: true },
    }),
  ]);

  return {
    tracks: tracks.map(toPlayerTrack),
    albums: albums.map((album) => ({
      id: album.id,
      title: album.title,
      year: album.year,
      artist: album.artist.name,
      hasArtwork: Boolean(album.artworkPath),
    })),
    artists: artists.map((artist) => ({ id: artist.id, name: artist.name })),
  };
}

export async function tracksForAlbum(albumId: number): Promise<PlayerTrack[]> {
  const album = await prisma.album.findUnique({
    where: { id: albumId },
    select: {
      tracks: {
        orderBy: [{ discNumber: "asc" }, { trackNumber: "asc" }, { title: "asc" }],
        select: trackSelect,
      },
    },
  });
  return album?.tracks.map(toPlayerTrack) ?? [];
}

export async function tracksForArtist(artistId: number): Promise<PlayerTrack[]> {
  const tracks = await prisma.track.findMany({
    where: { artistId },
    orderBy: [
      { album: { year: "asc" } },
      { album: { title: "asc" } },
      { discNumber: "asc" },
      { trackNumber: "asc" },
    ],
    select: trackSelect,
  });
  return tracks.map(toPlayerTrack);
}

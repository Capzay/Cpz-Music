import { Search as SearchIcon } from "lucide-react";
import { prisma } from "@/lib/db";
import { toPlayerTrack, trackSelect } from "@/lib/types";
import { AlbumCard } from "@/components/AlbumCard";
import { ArtistCard } from "@/components/ArtistCard";
import { TrackList } from "@/components/TrackList";

export const metadata = { title: "Search" };

export default async function SearchPage(props: PageProps<"/search">) {
  const { q } = await props.searchParams;
  const query = (Array.isArray(q) ? q[0] : q)?.trim() ?? "";

  return (
    <>
      <h1 className="text-xl font-bold mb-4 md:text-2xl md:mb-6">Search</h1>

      <form action="/search" className="relative mb-6">
        <SearchIcon size={18} className="absolute left-3 top-1/2 -translate-y-1/2 text-zinc-500" />
        <input
          type="search"
          name="q"
          defaultValue={query}
          placeholder="Search tracks, albums, artists..."
          autoFocus
          className="w-full bg-zinc-800 border border-zinc-700 rounded-lg pl-9 pr-4 py-2.5 text-sm focus:outline-none focus:border-violet-500 text-white placeholder-zinc-500"
        />
      </form>

      {query ? <Results query={query} /> : null}
    </>
  );
}

async function Results({ query }: { query: string }) {
  const contains = { contains: query, mode: "insensitive" as const };

  const [tracks, albums, artists, playlists] = await Promise.all([
    prisma.track.findMany({
      where: { title: contains },
      take: 25,
      orderBy: { title: "asc" },
      select: trackSelect,
    }),
    prisma.album.findMany({
      where: { title: contains },
      take: 12,
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
      take: 12,
      orderBy: { name: "asc" },
      select: { id: true, name: true },
    }),
    prisma.playlist.findMany({ orderBy: { name: "asc" }, select: { id: true, name: true } }),
  ]);

  if (tracks.length === 0 && albums.length === 0 && artists.length === 0) {
    return <p className="text-zinc-400">No results for “{query}”</p>;
  }

  return (
    <>
      {tracks.length > 0 && (
        <div className="mb-8">
          <h2 className="text-lg font-semibold mb-3">Tracks</h2>
          <TrackList tracks={tracks.map(toPlayerTrack)} playlists={playlists} numbered={false} />
        </div>
      )}

      {albums.length > 0 && (
        <div className="mb-8">
          <h2 className="text-lg font-semibold mb-3">Albums</h2>
          <div className="grid grid-cols-3 sm:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
            {albums.map((album) => (
              <AlbumCard
                key={album.id}
                id={album.id}
                title={album.title}
                artist={album.artist.name}
                hasArtwork={Boolean(album.artworkPath)}
              />
            ))}
          </div>
        </div>
      )}

      {artists.length > 0 && (
        <div className="mb-8">
          <h2 className="text-lg font-semibold mb-3">Artists</h2>
          <div className="grid grid-cols-3 sm:grid-cols-4 lg:grid-cols-5 gap-4">
            {artists.map((artist) => (
              <ArtistCard key={artist.id} id={artist.id} name={artist.name} />
            ))}
          </div>
        </div>
      )}
    </>
  );
}

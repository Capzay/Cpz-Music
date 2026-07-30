import Link from "next/link";
import { prisma } from "@/lib/db";
import { toPlayerTrack, trackSelect } from "@/lib/types";
import { AlbumCard } from "@/components/AlbumCard";
import { TrackList } from "@/components/TrackList";

export const metadata = { title: "Search" };

export default async function SearchPage(props: PageProps<"/search">) {
  const { q } = await props.searchParams;
  const query = (Array.isArray(q) ? q[0] : q)?.trim() ?? "";

  return (
    <>
      <h1 className="mb-5 text-xl font-semibold tracking-tight">Search</h1>

      <form action="/search" className="mb-6">
        <input
          type="search"
          name="q"
          defaultValue={query}
          placeholder="Tracks, albums, artists"
          autoFocus
          className="w-full max-w-md rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm outline-none placeholder:text-neutral-600 focus:border-neutral-600"
        />
      </form>

      {query ? <Results query={query} /> : null}
    </>
  );
}

async function Results({ query }: { query: string }) {
  const contains = { contains: query, mode: "insensitive" as const };

  const [tracks, albums, artists] = await Promise.all([
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
  ]);

  if (tracks.length === 0 && albums.length === 0 && artists.length === 0) {
    return <p className="text-sm text-neutral-500">Nothing matches “{query}”.</p>;
  }

  return (
    <div className="space-y-8">
      {artists.length > 0 ? (
        <section>
          <h2 className="mb-2 text-sm font-medium text-neutral-400">Artists</h2>
          <ul className="flex flex-wrap gap-2">
            {artists.map((artist) => (
              <li key={artist.id}>
                <Link
                  href={`/artists/${artist.id}`}
                  className="rounded-full border border-neutral-800 px-3 py-1.5 text-sm hover:border-neutral-600"
                >
                  {artist.name}
                </Link>
              </li>
            ))}
          </ul>
        </section>
      ) : null}

      {albums.length > 0 ? (
        <section>
          <h2 className="mb-2 text-sm font-medium text-neutral-400">Albums</h2>
          <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-6">
            {albums.map((album) => (
              <AlbumCard
                key={album.id}
                id={album.id}
                title={album.title}
                artist={album.artist.name}
                year={album.year}
                hasArtwork={Boolean(album.artworkPath)}
              />
            ))}
          </div>
        </section>
      ) : null}

      {tracks.length > 0 ? (
        <section>
          <h2 className="mb-2 text-sm font-medium text-neutral-400">Tracks</h2>
          <TrackList tracks={tracks.map(toPlayerTrack)} showArtist numbered={false} />
        </section>
      ) : null}
    </div>
  );
}

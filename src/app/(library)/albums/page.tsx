import { prisma } from "@/lib/db";
import { AlbumCard } from "@/components/AlbumCard";

export const metadata = { title: "Albums" };

export default async function AlbumsPage() {
  const albums = await prisma.album.findMany({
    orderBy: [{ artist: { name: "asc" } }, { year: "asc" }, { title: "asc" }],
    select: {
      id: true,
      title: true,
      year: true,
      artworkPath: true,
      artist: { select: { name: true } },
    },
  });

  if (albums.length === 0) {
    return (
      <div className="py-16 text-center">
        <h1 className="text-lg font-medium">No music yet</h1>
        <p className="mt-2 text-sm text-zinc-400">
          Point <code className="text-zinc-300">MUSIC_DIR</code> at your library and restart the
          server. Scanning starts automatically.
        </p>
      </div>
    );
  }

  return (
    <>
      <h1 className="text-xl font-bold mb-4 md:text-2xl md:mb-6">Albums</h1>
      <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
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
    </>
  );
}

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
        <p className="mt-2 text-sm text-neutral-500">
          Point <code className="text-neutral-400">MUSIC_DIR</code> at your library and restart the
          server. Scanning starts automatically.
        </p>
      </div>
    );
  }

  return (
    <>
      <h1 className="mb-5 text-xl font-semibold tracking-tight">Albums</h1>
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
    </>
  );
}

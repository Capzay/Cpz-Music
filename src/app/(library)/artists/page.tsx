import { prisma } from "@/lib/db";
import { ArtistCard } from "@/components/ArtistCard";

export const metadata = { title: "Artists" };

export default async function ArtistsPage() {
  const artists = await prisma.artist.findMany({
    where: { tracks: { some: {} } },
    orderBy: { name: "asc" },
    select: {
      id: true,
      name: true,
      _count: { select: { tracks: true, albums: true } },
    },
  });

  return (
    <>
      <h1 className="text-xl font-bold mb-4 md:text-2xl md:mb-6">Artists</h1>
      <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-4">
        {artists.map((artist) => (
          <ArtistCard
            key={artist.id}
            id={artist.id}
            name={artist.name}
            albums={artist._count.albums}
            tracks={artist._count.tracks}
          />
        ))}
      </div>
    </>
  );
}

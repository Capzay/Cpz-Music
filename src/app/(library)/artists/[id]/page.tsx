import { notFound } from "next/navigation";
import { prisma } from "@/lib/db";
import { AlbumCard } from "@/components/AlbumCard";

export default async function ArtistPage(props: PageProps<"/artists/[id]">) {
  const { id } = await props.params;
  const artistId = Number(id);
  if (!Number.isInteger(artistId)) notFound();

  const artist = await prisma.artist.findUnique({
    where: { id: artistId },
    select: { id: true, name: true },
  });
  if (!artist) notFound();

  // Albums they are credited on as album artist, plus albums holding a track of
  // theirs, so features on other people's records still show up here.
  const albums = await prisma.album.findMany({
    where: {
      OR: [{ artistId: artist.id }, { tracks: { some: { artistId: artist.id } } }],
    },
    orderBy: [{ year: "asc" }, { title: "asc" }],
    select: {
      id: true,
      title: true,
      year: true,
      artworkPath: true,
      artist: { select: { name: true } },
    },
  });

  return (
    <>
      <h1 className="mb-5 text-xl font-semibold tracking-tight">{artist.name}</h1>
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

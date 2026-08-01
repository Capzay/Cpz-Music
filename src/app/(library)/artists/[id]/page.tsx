import { notFound } from "next/navigation";
import { prisma } from "@/lib/db";
import { toPlayerTrack, trackSelect } from "@/lib/types";
import { AlbumCard } from "@/components/AlbumCard";
import { PlayAlbumButton } from "@/components/PlayAlbumButton";
import { TrackList } from "@/components/TrackList";

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
  const [albums, tracks, playlists] = await Promise.all([
    prisma.album.findMany({
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
    }),
    prisma.track.findMany({
      where: { artistId: artist.id },
      orderBy: [
        { album: { year: "asc" } },
        { album: { title: "asc" } },
        { discNumber: "asc" },
        { trackNumber: "asc" },
      ],
      select: trackSelect,
    }),
    prisma.playlist.findMany({ orderBy: { name: "asc" }, select: { id: true, name: true } }),
  ]);

  const playerTracks = tracks.map(toPlayerTrack);

  return (
    <>
      <div className="mb-6">
        <div className="w-20 h-20 md:w-28 md:h-28 bg-zinc-700 rounded-full flex items-center justify-center text-4xl font-bold text-zinc-300 mb-3">
          {artist.name.charAt(0).toUpperCase()}
        </div>
        <h1 className="text-2xl md:text-3xl font-bold mb-1">{artist.name}</h1>
        <p className="text-sm text-zinc-500">
          {albums.length} album{albums.length !== 1 ? "s" : ""} · {playerTracks.length} track
          {playerTracks.length !== 1 ? "s" : ""}
        </p>
        <div className="flex gap-2 mt-4">
          <PlayAlbumButton tracks={playerTracks} />
        </div>
      </div>

      {albums.length > 0 && (
        <div className="mb-8">
          <h2 className="text-lg font-semibold mb-3">Albums</h2>
          <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-4">
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

      <div>
        <h2 className="text-lg font-semibold mb-3">All Tracks</h2>
        <TrackList tracks={playerTracks} playlists={playlists} numbered={false} />
      </div>
    </>
  );
}

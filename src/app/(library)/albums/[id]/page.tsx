import Link from "next/link";
import { notFound } from "next/navigation";
import { prisma } from "@/lib/db";
import { toPlayerTrack, trackSelect } from "@/lib/types";
import { TrackList } from "@/components/TrackList";
import { PlayAlbumButton } from "@/components/PlayAlbumButton";
import { DownloadButton } from "@/components/DownloadButton";

export default async function AlbumPage(props: PageProps<"/albums/[id]">) {
  const { id } = await props.params;
  const albumId = Number(id);
  if (!Number.isInteger(albumId)) notFound();

  const [album, playlists] = await Promise.all([
    prisma.album.findUnique({
      where: { id: albumId },
      select: {
        id: true,
        title: true,
        year: true,
        artworkPath: true,
        artist: { select: { id: true, name: true } },
        tracks: {
          orderBy: [{ discNumber: "asc" }, { trackNumber: "asc" }, { title: "asc" }],
          select: trackSelect,
        },
      },
    }),
    prisma.playlist.findMany({ orderBy: { name: "asc" }, select: { id: true, name: true } }),
  ]);
  if (!album) notFound();

  const tracks = album.tracks.map(toPlayerTrack);
  // Per-track artists are only worth showing on compilations, where they differ.
  const isCompilation = tracks.some((t) => t.artist.id !== album.artist.id);

  return (
    <>
      <header className="mb-6 flex flex-col gap-4 sm:flex-row sm:items-end">
        <div className="h-40 w-40 shrink-0 overflow-hidden rounded-md bg-neutral-900">
          {album.artworkPath ? (
            // eslint-disable-next-line @next/next/no-img-element
            <img src={`/api/artwork/${album.id}`} alt="" className="h-full w-full object-cover" />
          ) : null}
        </div>
        <div className="min-w-0">
          <h1 className="text-2xl font-semibold tracking-tight">{album.title}</h1>
          <p className="mt-1 text-sm text-neutral-400">
            <Link href={`/artists/${album.artist.id}`} className="hover:underline">
              {album.artist.name}
            </Link>
            {album.year ? ` · ${album.year}` : ""} · {tracks.length} tracks
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            <PlayAlbumButton tracks={tracks} />
            <DownloadButton tracks={tracks} />
          </div>
        </div>
      </header>

      <TrackList tracks={tracks} playlists={playlists} showArtist={isCompilation} />
    </>
  );
}

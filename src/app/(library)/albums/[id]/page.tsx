import Link from "next/link";
import { notFound } from "next/navigation";
import { Disc3 } from "lucide-react";
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

  return (
    <>
      <div className="flex flex-col md:flex-row gap-4 mb-6 items-center md:items-end">
        <div className="w-40 md:w-56 aspect-square bg-zinc-800 rounded-lg overflow-hidden flex-shrink-0 flex items-center justify-center">
          {album.artworkPath ? (
            // eslint-disable-next-line @next/next/no-img-element
            <img
              src={`/api/artwork/${album.id}`}
              alt=""
              className="w-full h-full object-cover"
            />
          ) : (
            <Disc3 size={64} className="text-zinc-600" />
          )}
        </div>
        <div className="text-center md:text-left">
          <p className="text-xs text-zinc-500 uppercase tracking-wide mb-1">Album</p>
          <h1 className="text-2xl md:text-3xl font-bold mb-1">{album.title}</h1>
          <p className="text-zinc-400 mb-1">
            <Link href={`/artists/${album.artist.id}`} className="hover:text-white">
              {album.artist.name}
            </Link>
          </p>
          <p className="text-sm text-zinc-500">
            {album.year && `${album.year} · `}
            {tracks.length} track{tracks.length !== 1 ? "s" : ""}
          </p>
          <div className="flex gap-2 mt-4 justify-center md:justify-start">
            <PlayAlbumButton tracks={tracks} />
            <DownloadButton tracks={tracks} />
          </div>
        </div>
      </div>

      <TrackList tracks={tracks} playlists={playlists} showAlbum={false} />
    </>
  );
}

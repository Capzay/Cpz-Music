import Link from "next/link";
import { notFound } from "next/navigation";
import { prisma } from "@/lib/db";
import { formatDuration } from "@/lib/format";

export default async function AlbumPage(props: PageProps<"/albums/[id]">) {
  const { id } = await props.params;
  const albumId = Number(id);
  if (!Number.isInteger(albumId)) notFound();

  const album = await prisma.album.findUnique({
    where: { id: albumId },
    select: {
      id: true,
      title: true,
      year: true,
      artworkPath: true,
      artist: { select: { id: true, name: true } },
      tracks: {
        orderBy: [{ discNumber: "asc" }, { trackNumber: "asc" }, { title: "asc" }],
        select: {
          id: true,
          title: true,
          duration: true,
          trackNumber: true,
          artist: { select: { id: true, name: true } },
        },
      },
    },
  });
  if (!album) notFound();

  // Only worth showing per track on compilations, where they differ.
  const isCompilation = album.tracks.some((t) => t.artist.id !== album.artist.id);

  return (
    <>
      <header className="mb-6 flex flex-col gap-4 sm:flex-row sm:items-end">
        <div className="h-40 w-40 shrink-0 overflow-hidden rounded-md bg-neutral-900">
          {album.artworkPath ? (
            // eslint-disable-next-line @next/next/no-img-element
            <img
              src={`/api/artwork/${album.id}`}
              alt=""
              className="h-full w-full object-cover"
            />
          ) : null}
        </div>
        <div className="min-w-0">
          <h1 className="text-2xl font-semibold tracking-tight">{album.title}</h1>
          <p className="mt-1 text-sm text-neutral-400">
            <Link href={`/artists/${album.artist.id}`} className="hover:underline">
              {album.artist.name}
            </Link>
            {album.year ? ` · ${album.year}` : ""} · {album.tracks.length} tracks
          </p>
        </div>
      </header>

      <ol className="divide-y divide-neutral-900">
        {album.tracks.map((track, i) => (
          <li key={track.id} className="flex items-center gap-4 py-2.5 text-sm">
            <span className="w-6 shrink-0 text-right tabular-nums text-neutral-600">
              {track.trackNumber ?? i + 1}
            </span>
            <span className="min-w-0 flex-1">
              <span className="block truncate">{track.title}</span>
              {isCompilation ? (
                <span className="block truncate text-xs text-neutral-500">
                  {track.artist.name}
                </span>
              ) : null}
            </span>
            <span className="shrink-0 tabular-nums text-neutral-500">
              {formatDuration(track.duration)}
            </span>
          </li>
        ))}
      </ol>
    </>
  );
}

import { notFound } from "next/navigation";
import { prisma } from "@/lib/db";
import { toPlayerTrack, trackSelect } from "@/lib/types";
import { PlayAlbumButton } from "@/components/PlayAlbumButton";
import { PlaylistTracks } from "@/components/PlaylistTracks";
import { PlaylistHeader } from "@/components/PlaylistHeader";

export default async function PlaylistPage(props: PageProps<"/playlists/[id]">) {
  const { id } = await props.params;
  const playlistId = Number(id);
  if (!Number.isInteger(playlistId)) notFound();

  const playlist = await prisma.playlist.findUnique({
    where: { id: playlistId },
    select: {
      id: true,
      name: true,
      tracks: {
        orderBy: { position: "asc" },
        select: { id: true, track: { select: trackSelect } },
      },
    },
  });
  if (!playlist) notFound();

  const entries = playlist.tracks.map((row) => ({
    playlistTrackId: row.id,
    track: toPlayerTrack(row.track),
  }));

  return (
    <>
      <PlaylistHeader id={playlist.id} name={playlist.name} count={entries.length} />

      {entries.length > 0 ? (
        <div className="mb-5">
          <PlayAlbumButton tracks={entries.map((e) => e.track)} />
        </div>
      ) : (
        <p className="text-sm text-neutral-500">
          Empty. Add tracks from an album or from search.
        </p>
      )}

      <PlaylistTracks playlistId={playlist.id} entries={entries} />
    </>
  );
}

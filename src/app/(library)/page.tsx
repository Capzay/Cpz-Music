import { prisma } from "@/lib/db";
import { toPlayerTrack, trackSelect } from "@/lib/types";
import { TrackList } from "@/components/TrackList";

export const metadata = { title: "Library" };

export default async function LibraryPage() {
  const tracks = await prisma.track.findMany({
    orderBy: [
      { artist: { name: "asc" } },
      { album: { year: "asc" } },
      { album: { title: "asc" } },
      { discNumber: "asc" },
      { trackNumber: "asc" },
    ],
    select: trackSelect,
  });

  return (
    <>
      <h1 className="text-xl font-bold mb-4 md:text-2xl md:mb-6">Library</h1>
      {tracks.length === 0 ? (
        <p className="text-zinc-400">No tracks found. Check your music directory.</p>
      ) : (
        <TrackList tracks={tracks.map(toPlayerTrack)} numbered={false} />
      )}
    </>
  );
}

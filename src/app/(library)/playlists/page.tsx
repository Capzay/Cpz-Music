import Link from "next/link";
import { ListMusic, Plus } from "lucide-react";
import { prisma } from "@/lib/db";
import { createPlaylist } from "./actions";

export const metadata = { title: "Playlists" };

export default async function PlaylistsPage() {
  const playlists = await prisma.playlist.findMany({
    orderBy: { updatedAt: "desc" },
    select: { id: true, name: true, _count: { select: { tracks: true } } },
  });

  return (
    <>
      <h1 className="text-xl font-bold mb-4 md:text-2xl md:mb-6">Playlists</h1>

      <form action={createPlaylist} className="flex gap-2 mb-6">
        <input
          name="name"
          required
          maxLength={120}
          placeholder="New playlist name..."
          className="flex-1 bg-zinc-800 border border-zinc-700 rounded-md px-3 py-2 text-sm focus:outline-none focus:border-violet-500 text-white placeholder-zinc-500"
        />
        <button className="flex items-center gap-1 bg-violet-600 hover:bg-violet-500 text-white px-4 py-2 rounded-md text-sm font-medium transition-colors">
          <Plus size={16} />
          Create
        </button>
      </form>

      {playlists.length === 0 ? (
        <p className="text-zinc-400">No playlists yet. Create one above.</p>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
          {playlists.map((playlist) => (
            <Link
              key={playlist.id}
              href={`/playlists/${playlist.id}`}
              className="block cursor-pointer bg-zinc-900 border border-zinc-800 rounded-lg p-4 hover:border-zinc-600 transition-colors"
            >
              <div className="w-12 h-12 bg-zinc-800 rounded-md flex items-center justify-center mb-3">
                <ListMusic size={24} className="text-violet-400" />
              </div>
              <p className="font-medium truncate">{playlist.name}</p>
              <p className="text-sm text-zinc-500 mt-0.5">
                {playlist._count.tracks} track{playlist._count.tracks !== 1 ? "s" : ""}
              </p>
            </Link>
          ))}
        </div>
      )}
    </>
  );
}

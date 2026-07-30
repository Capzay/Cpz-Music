import Link from "next/link";
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
      <h1 className="mb-5 text-xl font-semibold tracking-tight">Playlists</h1>

      <form action={createPlaylist} className="mb-6 flex max-w-md gap-2">
        <input
          name="name"
          required
          maxLength={120}
          placeholder="New playlist"
          className="flex-1 rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm outline-none placeholder:text-neutral-600 focus:border-neutral-600"
        />
        <button className="rounded-md bg-neutral-100 px-4 py-2 text-sm font-medium text-neutral-900 transition hover:bg-white">
          Create
        </button>
      </form>

      {playlists.length === 0 ? (
        <p className="text-sm text-neutral-500">No playlists yet.</p>
      ) : (
        <ul className="divide-y divide-neutral-900">
          {playlists.map((playlist) => (
            <li key={playlist.id}>
              <Link
                href={`/playlists/${playlist.id}`}
                className="flex items-center justify-between py-3 text-sm hover:text-white"
              >
                <span className="truncate">{playlist.name}</span>
                <span className="shrink-0 tabular-nums text-neutral-600">
                  {playlist._count.tracks}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

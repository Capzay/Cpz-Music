"use client";

import { useState } from "react";
import { artworkUrl, type PlayerTrack } from "@/lib/types";
import { formatDuration } from "@/lib/format";

interface GuestAlbum {
  id: number;
  title: string;
  artist: string;
  hasArtwork: boolean;
  tracks: PlayerTrack[];
}

/**
 * The guest view. Deliberately not the host app: a guest browses and adds to the
 * queue, and has no player, no playlists, no stats and no settings. Keeping it a
 * separate component means a guest cannot reach host UI by changing a flag.
 */
export function GuestApp({
  name,
  hostName,
  albums,
}: {
  name: string;
  hostName: string | null;
  albums: GuestAlbum[];
}) {
  const [open, setOpen] = useState<number | null>(null);
  const [added, setAdded] = useState<Record<number, "adding" | "done" | "error">>({});
  const [query, setQuery] = useState("");

  async function add(track: PlayerTrack) {
    setAdded((prev) => ({ ...prev, [track.id]: "adding" }));
    try {
      const response = await fetch("/api/jam/add", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ trackId: track.id }),
      });
      setAdded((prev) => ({ ...prev, [track.id]: response.ok ? "done" : "error" }));
    } catch {
      setAdded((prev) => ({ ...prev, [track.id]: "error" }));
    }
  }

  const term = query.trim().toLowerCase();
  const visible = term
    ? albums.filter(
        (a) =>
          a.title.toLowerCase().includes(term) ||
          a.artist.toLowerCase().includes(term) ||
          a.tracks.some((t) => t.title.toLowerCase().includes(term)),
      )
    : albums;

  return (
    <main className="mx-auto max-w-2xl p-4">
      <header className="mb-5">
        <h1 className="text-xl font-semibold tracking-tight">
          {hostName ? `${hostName}'s jam` : "Jam"}
        </h1>
        <p className="mt-1 text-sm text-neutral-500">
          You are in as {name}. Pick something to add to the queue.
        </p>
      </header>

      <input
        type="search"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder="Search"
        className="mb-5 w-full rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm outline-none placeholder:text-neutral-600 focus:border-neutral-600"
      />

      <ul className="space-y-2">
        {visible.map((album) => (
          <li key={album.id} className="rounded-lg border border-neutral-900">
            <button
              onClick={() => setOpen(open === album.id ? null : album.id)}
              aria-expanded={open === album.id}
              className="flex w-full items-center gap-3 p-3 text-left"
            >
              <span className="h-12 w-12 shrink-0 overflow-hidden rounded bg-neutral-900">
                {album.hasArtwork ? (
                  // eslint-disable-next-line @next/next/no-img-element
                  <img src={artworkUrl(album.id)} alt="" className="h-full w-full object-cover" />
                ) : null}
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium">{album.title}</span>
                <span className="block truncate text-xs text-neutral-500">{album.artist}</span>
              </span>
            </button>

            {open === album.id ? (
              <ol className="border-t border-neutral-900 px-3 pb-2">
                {album.tracks.map((track) => {
                  const status = added[track.id];
                  return (
                    <li key={track.id} className="flex items-center gap-3 py-2 text-sm">
                      <span className="min-w-0 flex-1 truncate">{track.title}</span>
                      <span className="shrink-0 text-xs tabular-nums text-neutral-600">
                        {formatDuration(track.duration)}
                      </span>
                      <button
                        onClick={() => void add(track)}
                        disabled={status === "adding" || status === "done"}
                        className="shrink-0 rounded-full border border-neutral-700 px-3 py-1 text-xs transition hover:border-neutral-500 disabled:opacity-50"
                      >
                        {status === "done"
                          ? "Added"
                          : status === "adding"
                            ? "Adding"
                            : status === "error"
                              ? "Retry"
                              : "Add"}
                      </button>
                    </li>
                  );
                })}
              </ol>
            ) : null}
          </li>
        ))}
      </ul>

      <form
        className="mt-8"
        action={async () => {
          await fetch("/api/jam/leave", { method: "POST" });
          window.location.href = "/jam";
        }}
      >
        <button className="text-xs text-neutral-600 hover:text-neutral-300">Leave jam</button>
      </form>
    </main>
  );
}

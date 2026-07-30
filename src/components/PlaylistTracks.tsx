"use client";

import { useTransition } from "react";
import { usePlayerStore } from "@/store/player";
import { formatDuration } from "@/lib/format";
import type { PlayerTrack } from "@/lib/types";
import { movePlaylistTrack, removeFromPlaylist } from "@/app/(library)/playlists/actions";
import { CloseIcon, PlayIcon } from "./Player/icons";

export function PlaylistTracks({
  playlistId,
  entries,
}: {
  playlistId: number;
  entries: { playlistTrackId: number; track: PlayerTrack }[];
}) {
  const dispatch = usePlayerStore((s) => s.dispatch);
  const currentId = usePlayerStore((s) => s.queue[s.index]?.id ?? null);
  const [pending, startTransition] = useTransition();

  const tracks = entries.map((e) => e.track);

  return (
    <ol className={`divide-y divide-neutral-900 ${pending ? "opacity-60" : ""}`}>
      {entries.map(({ playlistTrackId, track }, i) => (
        <li key={playlistTrackId} className="group flex items-center gap-3 py-2.5 text-sm">
          <button
            onClick={() => dispatch({ type: "setQueue", tracks, startIndex: i })}
            className="flex min-w-0 flex-1 items-center gap-4 text-left"
            aria-label={`Play ${track.title}`}
          >
            <span className="w-6 shrink-0 text-right tabular-nums text-neutral-600">
              <span className="group-hover:hidden">{i + 1}</span>
              <PlayIcon className="ml-auto hidden h-3 w-3 text-neutral-100 group-hover:block" />
            </span>
            <span className="min-w-0 flex-1">
              <span
                className={`block truncate ${
                  track.id === currentId ? "text-neutral-100" : "text-neutral-200"
                }`}
              >
                {track.title}
              </span>
              <span className="block truncate text-xs text-neutral-500">{track.artist.name}</span>
            </span>
          </button>

          <div className="flex shrink-0 items-center gap-1 opacity-0 transition group-hover:opacity-100">
            <button
              disabled={i === 0 || pending}
              onClick={() =>
                startTransition(() => movePlaylistTrack(playlistId, i, i - 1).catch(() => {}))
              }
              aria-label="Move up"
              className="px-1 text-neutral-500 hover:text-neutral-200 disabled:opacity-20"
            >
              ↑
            </button>
            <button
              disabled={i === entries.length - 1 || pending}
              onClick={() =>
                startTransition(() => movePlaylistTrack(playlistId, i, i + 1).catch(() => {}))
              }
              aria-label="Move down"
              className="px-1 text-neutral-500 hover:text-neutral-200 disabled:opacity-20"
            >
              ↓
            </button>
            <button
              disabled={pending}
              onClick={() =>
                startTransition(() =>
                  removeFromPlaylist(playlistId, playlistTrackId).catch(() => {}),
                )
              }
              aria-label={`Remove ${track.title}`}
              className="px-1 text-neutral-600 hover:text-red-400 disabled:opacity-20"
            >
              <CloseIcon className="h-4 w-4" />
            </button>
          </div>

          <span className="w-10 shrink-0 text-right tabular-nums text-neutral-500">
            {formatDuration(track.duration)}
          </span>
        </li>
      ))}
    </ol>
  );
}

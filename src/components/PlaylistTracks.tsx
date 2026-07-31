"use client";

import { useTransition } from "react";
import { ChevronDown, ChevronUp, Play, X } from "lucide-react";
import { usePlayerStore } from "@/store/player";
import { formatDuration } from "@/lib/format";
import { artworkUrl, type PlayerTrack } from "@/lib/types";
import { movePlaylistTrack, removeFromPlaylist } from "@/app/(library)/playlists/actions";

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
    <div className={`flex flex-col ${pending ? "opacity-60" : ""}`}>
      {entries.map(({ playlistTrackId, track }, i) => (
        <div
          key={playlistTrackId}
          className="group flex items-center gap-3 px-2 py-2 hover:bg-zinc-800/50 cursor-pointer"
          onClick={() => dispatch({ type: "setQueue", tracks, startIndex: i })}
        >
          <span className="w-6 shrink-0 text-right text-sm tabular-nums text-zinc-400">
            <span className="group-hover:hidden">{i + 1}</span>
            <Play size={14} fill="white" className="ml-auto hidden text-white group-hover:block" />
          </span>

          <div className="w-8 h-8 bg-zinc-800 rounded overflow-hidden flex-shrink-0">
            {track.album.hasArtwork ? (
              // eslint-disable-next-line @next/next/no-img-element
              <img src={artworkUrl(track.album.id)} alt="" className="w-full h-full object-cover" />
            ) : null}
          </div>

          <div className="min-w-0 flex-1">
            <p
              className={`text-sm font-medium truncate ${
                track.id === currentId ? "text-violet-400" : ""
              }`}
            >
              {track.title}
            </p>
            <p className="text-xs text-zinc-400 truncate">{track.artist.name}</p>
          </div>

          <div
            className="flex shrink-0 items-center gap-1 opacity-0 transition group-hover:opacity-100"
            onClick={(e) => e.stopPropagation()}
          >
            <button
              disabled={i === 0 || pending}
              onClick={() =>
                startTransition(() => movePlaylistTrack(playlistId, i, i - 1).catch(() => {}))
              }
              aria-label="Move up"
              className="px-1 text-zinc-500 hover:text-white disabled:opacity-20"
            >
              <ChevronUp size={16} />
            </button>
            <button
              disabled={i === entries.length - 1 || pending}
              onClick={() =>
                startTransition(() => movePlaylistTrack(playlistId, i, i + 1).catch(() => {}))
              }
              aria-label="Move down"
              className="px-1 text-zinc-500 hover:text-white disabled:opacity-20"
            >
              <ChevronDown size={16} />
            </button>
            <button
              disabled={pending}
              onClick={() =>
                startTransition(() =>
                  removeFromPlaylist(playlistId, playlistTrackId).catch(() => {}),
                )
              }
              aria-label={`Remove ${track.title}`}
              className="px-1 text-zinc-500 hover:text-red-400 disabled:opacity-20"
            >
              <X size={16} />
            </button>
          </div>

          <span className="w-10 shrink-0 text-right text-xs tabular-nums text-zinc-500">
            {formatDuration(track.duration)}
          </span>
        </div>
      ))}
    </div>
  );
}

"use client";

import { ChevronDown, ChevronUp, Play, X } from "lucide-react";
import { useEffect } from "react";
import { usePlayerStore } from "@/store/player";
import { usePlaylists } from "@/store/playlists";
import { useDownloads } from "@/store/downloads";
import { queueForPlayback } from "@/lib/offline-play";
import { formatDuration } from "@/lib/format";
import { artworkUrl, type PlayerTrack } from "@/lib/types";

export function PlaylistTracks({
  uuid,
  tracks,
}: {
  uuid: string;
  tracks: PlayerTrack[];
}) {
  const dispatch = usePlayerStore((s) => s.dispatch);
  const currentId = usePlayerStore((s) => s.queue[s.index]?.id ?? null);
  const removeTrack = usePlaylists((s) => s.removeTrack);
  const moveTrack = usePlaylists((s) => s.moveTrack);
  const hydrateDownloads = useDownloads((s) => s.hydrate);

  useEffect(() => hydrateDownloads(), [hydrateDownloads]);

  function playFrom(i: number) {
    const next = queueForPlayback(tracks, i);
    dispatch({ type: "setQueue", tracks: next.tracks, startIndex: next.startIndex });
  }

  return (
    <div className="flex flex-col">
      {tracks.map((track, i) => (
        <div
          key={`${track.id}-${i}`}
          className="group flex items-center gap-3 px-2 py-2 hover:bg-zinc-800/50"
        >
          <button
            type="button"
            onClick={() => playFrom(i)}
            aria-label={`Play ${track.title}`}
            className="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left"
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
          </button>

          {/*
            Invisible until hover on pointer devices. pointer-events-none while
            hidden so a tap meant to play cannot hit Remove / reorder.
            Always visible on coarse pointers (touch) so edits stay reachable.
          */}
          <div className="flex shrink-0 items-center gap-1 opacity-100 pointer-events-auto [@media(hover:hover)_and_(pointer:fine)]:pointer-events-none [@media(hover:hover)_and_(pointer:fine)]:opacity-0 [@media(hover:hover)_and_(pointer:fine)]:group-hover:pointer-events-auto [@media(hover:hover)_and_(pointer:fine)]:group-hover:opacity-100 transition">
            <button
              type="button"
              disabled={i === 0}
              onClick={() => moveTrack(uuid, i, i - 1)}
              aria-label="Move up"
              className="px-1 text-zinc-500 hover:text-white disabled:opacity-20"
            >
              <ChevronUp size={16} />
            </button>
            <button
              type="button"
              disabled={i === tracks.length - 1}
              onClick={() => moveTrack(uuid, i, i + 1)}
              aria-label="Move down"
              className="px-1 text-zinc-500 hover:text-white disabled:opacity-20"
            >
              <ChevronDown size={16} />
            </button>
            <button
              type="button"
              onClick={() => removeTrack(uuid, i)}
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

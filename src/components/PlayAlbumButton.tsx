"use client";

import { usePlayerStore } from "@/store/player";
import type { PlayerTrack } from "@/lib/types";
import { PlayIcon, ShuffleIcon } from "./Player/icons";

export function PlayAlbumButton({ tracks }: { tracks: PlayerTrack[] }) {
  const dispatch = usePlayerStore((s) => s.dispatch);
  const shuffle = usePlayerStore((s) => s.shuffle);
  if (tracks.length === 0) return null;

  return (
    <div className="flex gap-2">
      <button
        onClick={() => dispatch({ type: "setQueue", tracks, startIndex: 0 })}
        className="flex items-center gap-2 rounded-full bg-neutral-100 px-4 py-2 text-sm font-medium text-neutral-900 transition hover:bg-white"
      >
        <PlayIcon className="h-3.5 w-3.5" />
        Play
      </button>
      <button
        onClick={() => {
          // Shuffle on first, so the queue is built already shuffled. The start
          // index has to be random too: the shuffle order always begins at the
          // current track, so starting at 0 would play track one every time.
          if (!shuffle) dispatch({ type: "toggleShuffle" });
          dispatch({
            type: "setQueue",
            tracks,
            startIndex: Math.floor(Math.random() * tracks.length),
          });
        }}
        className="flex items-center gap-2 rounded-full border border-neutral-700 px-4 py-2 text-sm font-medium transition hover:border-neutral-500"
      >
        <ShuffleIcon className="h-3.5 w-3.5" />
        Shuffle
      </button>
    </div>
  );
}

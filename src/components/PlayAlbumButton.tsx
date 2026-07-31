"use client";

import { Play, Shuffle } from "lucide-react";
import { usePlayerStore } from "@/store/player";
import type { PlayerTrack } from "@/lib/types";

export function PlayAlbumButton({ tracks }: { tracks: PlayerTrack[] }) {
  const dispatch = usePlayerStore((s) => s.dispatch);
  const shuffle = usePlayerStore((s) => s.shuffle);
  if (tracks.length === 0) return null;

  return (
    <div className="flex gap-2">
      <button
        onClick={() => dispatch({ type: "setQueue", tracks, startIndex: 0 })}
        className="flex items-center gap-2 bg-violet-600 hover:bg-violet-500 text-white px-4 py-2 rounded-full text-sm font-medium transition-colors"
      >
        <Play size={16} fill="white" />
        Play All
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
        className="flex items-center gap-2 border border-zinc-600 hover:border-zinc-400 text-zinc-300 hover:text-white px-4 py-2 rounded-full text-sm font-medium transition-colors"
      >
        <Shuffle size={16} />
        Shuffle
      </button>
    </div>
  );
}

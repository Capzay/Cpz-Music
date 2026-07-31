"use client";

import { X } from "lucide-react";
import { usePlayerStore, useCurrentTrack } from "@/store/player";
import { artworkUrl, type PlayerTrack } from "@/lib/types";

/** What is still to come, in the order it will actually play. */
export function useUpcoming(): { track: PlayerTrack; queueIdx: number }[] {
  const queue = usePlayerStore((s) => s.queue);
  const index = usePlayerStore((s) => s.index);
  const shuffle = usePlayerStore((s) => s.shuffle);
  const shuffleOrder = usePlayerStore((s) => s.shuffleOrder);
  const shufflePos = usePlayerStore((s) => s.shufflePos);

  if (shuffle && shuffleOrder.length > 0) {
    return shuffleOrder
      .slice(shufflePos + 1)
      .map((i) => ({ track: queue[i], queueIdx: i }))
      .filter((x) => x.track);
  }
  return queue.slice(index + 1).map((track, i) => ({ track, queueIdx: index + 1 + i }));
}

export function Thumb({ track, size = "w-8 h-8" }: { track: PlayerTrack; size?: string }) {
  return (
    <div className={`${size} bg-zinc-800 rounded overflow-hidden flex-shrink-0`}>
      {track.album.hasArtwork ? (
        // eslint-disable-next-line @next/next/no-img-element
        <img src={artworkUrl(track.album.id)} alt="" className="w-full h-full object-cover" />
      ) : null}
    </div>
  );
}

export function QueuePanel({ onClose }: { onClose: () => void }) {
  const dispatch = usePlayerStore((s) => s.dispatch);
  const currentTrack = useCurrentTrack();
  const upcoming = useUpcoming();

  return (
    <div
      className="fixed right-0 bottom-20 w-72 bg-zinc-900 border border-zinc-700 rounded-t-lg shadow-2xl z-50 max-h-[60vh] flex flex-col"
      aria-label="Play queue"
    >
      <div className="flex items-center justify-between px-4 py-3 border-b border-zinc-700 flex-shrink-0">
        <span className="text-sm font-semibold">Queue</span>
        <button
          onClick={onClose}
          aria-label="Close queue"
          className="text-zinc-400 hover:text-white transition-colors"
        >
          <X size={16} />
        </button>
      </div>

      {currentTrack && (
        <div className="px-4 py-2 border-b border-zinc-800 flex-shrink-0">
          <p className="text-xs text-zinc-500 mb-1 uppercase tracking-wide">Now playing</p>
          <div className="flex items-center gap-2">
            <Thumb track={currentTrack} />
            <div className="min-w-0">
              <p className="text-sm font-medium truncate text-violet-400">{currentTrack.title}</p>
              <p className="text-xs text-zinc-400 truncate">{currentTrack.artist.name}</p>
            </div>
          </div>
        </div>
      )}

      <div className="overflow-y-auto flex-1">
        {upcoming.length === 0 ? (
          <p className="text-zinc-500 text-sm px-4 py-4">Nothing up next</p>
        ) : (
          <>
            <p className="text-xs text-zinc-500 px-4 pt-2 pb-1 uppercase tracking-wide">
              Next up · {upcoming.length}
            </p>
            {upcoming.map(({ track, queueIdx }) => (
              <div
                key={`${track.id}-${queueIdx}`}
                className="flex items-center gap-2 px-4 py-1.5 hover:bg-zinc-800/50 group"
              >
                <button
                  onClick={() => dispatch({ type: "jumpTo", index: queueIdx })}
                  className="flex items-center gap-2 min-w-0 flex-1 text-left"
                  aria-label={`Play ${track.title}`}
                >
                  <Thumb track={track} size="w-7 h-7" />
                  <span className="min-w-0">
                    <span className="block text-sm truncate">{track.title}</span>
                    <span className="block text-xs text-zinc-400 truncate">{track.artist.name}</span>
                  </span>
                </button>
                <button
                  onClick={() => dispatch({ type: "removeFromQueue", index: queueIdx })}
                  title="Remove from queue"
                  aria-label={`Remove ${track.title} from queue`}
                  className="opacity-0 group-hover:opacity-100 text-zinc-500 hover:text-red-400 transition-all p-1"
                >
                  <X size={12} />
                </button>
              </div>
            ))}
          </>
        )}
      </div>
    </div>
  );
}

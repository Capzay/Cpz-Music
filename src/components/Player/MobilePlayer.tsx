"use client";

import { useState } from "react";
import {
  ChevronDown,
  ListMusic,
  Pause,
  Play,
  Repeat,
  Repeat1,
  Shuffle,
  SkipBack,
  SkipForward,
  X,
} from "lucide-react";
import { usePlayerStore, useCurrentTrack } from "@/store/player";
import { artworkUrl } from "@/lib/types";
import { DevicePicker } from "./DevicePicker";
import { ProgressBar } from "./ProgressBar";
import { Thumb, useUpcoming } from "./QueuePanel";
import { VolumeControl } from "./VolumeControl";

export function MobilePlayer({ onClose }: { onClose: () => void }) {
  const track = useCurrentTrack();
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const shuffle = usePlayerStore((s) => s.shuffle);
  const repeat = usePlayerStore((s) => s.repeat);
  const isActiveDevice = usePlayerStore((s) => s.isActiveDevice);
  const dispatch = usePlayerStore((s) => s.dispatch);

  const [showQueue, setShowQueue] = useState(false);
  const upcoming = useUpcoming();

  if (!track) return null;

  return (
    <div className="fixed inset-0 z-[60] bg-zinc-950 flex flex-col px-6 pt-safe">
      <div className="flex justify-center pt-4 pb-6">
        <button
          onClick={onClose}
          aria-label="Close player"
          className="text-zinc-400 hover:text-white transition-colors min-w-[44px] min-h-[44px] flex items-center justify-center"
        >
          <ChevronDown size={28} />
        </button>
      </div>

      <div className="flex-1 flex flex-col items-center justify-center gap-6 overflow-hidden">
        <div className="w-full max-w-xs aspect-square bg-zinc-800 rounded-xl overflow-hidden shadow-2xl">
          {track.album.hasArtwork ? (
            // eslint-disable-next-line @next/next/no-img-element
            <img
              src={artworkUrl(track.album.id)}
              alt={track.album.title}
              className="w-full h-full object-cover"
            />
          ) : null}
        </div>

        <div className="w-full text-center">
          <p className="text-xl font-bold truncate">{track.title}</p>
          <p className="text-zinc-400 truncate">{track.artist.name}</p>
          {!isActiveDevice && (
            <p className="text-xs text-violet-400 mt-1">Listening on another device</p>
          )}
        </div>

        <div className="w-full">
          <ProgressBar />
        </div>

        <div className="flex items-center justify-center gap-8">
          <button
            onClick={() => dispatch({ type: "prev" })}
            aria-label="Previous track"
            className="text-zinc-300 hover:text-white transition-colors min-w-[48px] min-h-[48px] flex items-center justify-center"
          >
            <SkipBack size={28} />
          </button>
          <button
            onClick={() => dispatch({ type: "toggle" })}
            aria-label={isPlaying ? "Pause" : "Play"}
            className="bg-white text-black rounded-full w-14 h-14 flex items-center justify-center hover:bg-zinc-200 transition-colors"
          >
            {isPlaying ? <Pause size={28} /> : <Play size={28} fill="black" />}
          </button>
          <button
            onClick={() => dispatch({ type: "next" })}
            aria-label="Next track"
            className="text-zinc-300 hover:text-white transition-colors min-w-[48px] min-h-[48px] flex items-center justify-center"
          >
            <SkipForward size={28} />
          </button>
        </div>

        <div className="flex items-center justify-between w-full">
          <button
            onClick={() => dispatch({ type: "toggleShuffle" })}
            aria-label="Shuffle"
            aria-pressed={shuffle}
            className={`min-w-[44px] min-h-[44px] flex items-center justify-center transition-colors ${
              shuffle ? "text-violet-400" : "text-zinc-400"
            }`}
          >
            <Shuffle size={20} />
          </button>

          <VolumeControl className="flex" />

          <button
            onClick={() => dispatch({ type: "cycleRepeat" })}
            aria-label={`Repeat: ${repeat}`}
            aria-pressed={repeat !== "off"}
            className={`min-w-[44px] min-h-[44px] flex items-center justify-center transition-colors ${
              repeat !== "off" ? "text-violet-400" : "text-zinc-400"
            }`}
          >
            {repeat === "one" ? <Repeat1 size={20} /> : <Repeat size={20} />}
          </button>
        </div>

        <div className="flex items-center justify-between w-full mb-4">
          <button
            onClick={() => setShowQueue(true)}
            aria-label="Open queue"
            className="min-w-[44px] min-h-[44px] flex items-center justify-center text-zinc-400 hover:text-white transition-colors"
          >
            <ListMusic size={20} />
          </button>
          <DevicePicker />
        </div>
      </div>

      {showQueue && (
        <div className="absolute inset-0 z-10 bg-zinc-950 flex flex-col px-6 pt-safe">
          <div className="flex items-center justify-between pt-4 pb-4">
            <span className="text-lg font-semibold">Queue</span>
            <div className="flex items-center">
              {/* The overlay hides the player controls, so these live here too. */}
              <button
                onClick={() => dispatch({ type: "toggleShuffle" })}
                aria-label="Shuffle"
                aria-pressed={shuffle}
                className={`min-w-[44px] min-h-[44px] flex items-center justify-center transition-colors ${
                  shuffle ? "text-violet-400" : "text-zinc-400"
                }`}
              >
                <Shuffle size={20} />
              </button>
              <button
                onClick={() => dispatch({ type: "cycleRepeat" })}
                aria-label={`Repeat: ${repeat}`}
                aria-pressed={repeat !== "off"}
                className={`min-w-[44px] min-h-[44px] flex items-center justify-center transition-colors ${
                  repeat !== "off" ? "text-violet-400" : "text-zinc-400"
                }`}
              >
                {repeat === "one" ? <Repeat1 size={20} /> : <Repeat size={20} />}
              </button>
              <button
                onClick={() => setShowQueue(false)}
                aria-label="Close queue"
                className="min-w-[44px] min-h-[44px] flex items-center justify-center text-zinc-400 hover:text-white transition-colors"
              >
                <X size={24} />
              </button>
            </div>
          </div>

          <div className="pb-3 border-b border-zinc-800 mb-3">
            <p className="text-xs text-zinc-500 mb-2 uppercase tracking-wide">Now playing</p>
            <div className="flex items-center gap-3">
              <Thumb track={track} size="w-10 h-10" />
              <div className="min-w-0">
                <p className="text-sm font-medium truncate text-violet-400">{track.title}</p>
                <p className="text-xs text-zinc-400 truncate">{track.artist.name}</p>
              </div>
            </div>
          </div>

          <div className="overflow-y-auto flex-1 pb-safe">
            {upcoming.length === 0 ? (
              <p className="text-zinc-500 text-sm py-4">Nothing up next</p>
            ) : (
              <>
                <p className="text-xs text-zinc-500 pb-2 uppercase tracking-wide">
                  Next up · {upcoming.length}
                </p>
                {upcoming.map(({ track: queued, queueIdx }) => (
                  <div key={`${queued.id}-${queueIdx}`} className="flex items-center gap-3 py-2">
                    <button
                      onClick={() => dispatch({ type: "jumpTo", index: queueIdx })}
                      className="flex items-center gap-3 min-w-0 flex-1 text-left"
                      aria-label={`Play ${queued.title}`}
                    >
                      <Thumb track={queued} size="w-10 h-10" />
                      <span className="min-w-0 flex-1">
                        <span className="block text-sm truncate">{queued.title}</span>
                        <span className="block text-xs text-zinc-400 truncate">
                          {queued.artist.name}
                        </span>
                      </span>
                    </button>
                    <button
                      onClick={() => dispatch({ type: "removeFromQueue", index: queueIdx })}
                      aria-label={`Remove ${queued.title} from queue`}
                      className="text-zinc-500 hover:text-red-400 transition-colors min-w-[44px] min-h-[44px] flex items-center justify-center"
                    >
                      <X size={16} />
                    </button>
                  </div>
                ))}
              </>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

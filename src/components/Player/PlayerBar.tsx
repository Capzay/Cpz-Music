"use client";

import { useState } from "react";
import Link from "next/link";
import { ListMusic, Pause, Play, Repeat, Repeat1, Shuffle, SkipBack, SkipForward } from "lucide-react";
import { useAudio } from "@/hooks/useAudio";
import { useSync } from "@/hooks/useSync";
import { usePlayerStore, useCurrentTrack } from "@/store/player";
import { artworkUrl, type PlayerTrack } from "@/lib/types";
import { DevicePicker } from "./DevicePicker";
import { MobilePlayer } from "./MobilePlayer";
import { ProgressBar } from "./ProgressBar";
import { QueuePanel } from "./QueuePanel";
import { VolumeControl } from "./VolumeControl";

export function PlayerBar() {
  // Mounted once in the library layout, so this is where the audio element and
  // the Realtime channel live.
  useAudio();
  useSync();

  const track = useCurrentTrack();
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const shuffle = usePlayerStore((s) => s.shuffle);
  const repeat = usePlayerStore((s) => s.repeat);
  const dispatch = usePlayerStore((s) => s.dispatch);

  const [mobileOpen, setMobileOpen] = useState(false);
  const [queueOpen, setQueueOpen] = useState(false);

  if (!track) {
    return (
      <div className="bg-zinc-900 border-t border-zinc-800 h-16 flex items-center px-4 text-zinc-500 text-sm">
        No track playing
      </div>
    );
  }

  return (
    <>
      {/* Mobile compact player */}
      <div
        className="md:hidden flex items-center gap-3 bg-zinc-900 border-t border-zinc-800 px-3 h-16 cursor-pointer"
        onClick={() => setMobileOpen(true)}
      >
        <Artwork track={track} />
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium truncate">{track.title}</p>
          <p className="text-xs text-zinc-400 truncate">{track.artist.name}</p>
        </div>
        <button
          onClick={(e) => {
            e.stopPropagation();
            dispatch({ type: "toggle" });
          }}
          aria-label={isPlaying ? "Pause" : "Play"}
          className="text-white min-w-[44px] min-h-[44px] flex items-center justify-center"
        >
          {isPlaying ? <Pause size={22} /> : <Play size={22} fill="white" />}
        </button>
      </div>

      {/* Desktop full player */}
      <div className="hidden md:flex items-center bg-zinc-900 border-t border-zinc-800 px-4 h-20 gap-4">
        <div className="flex items-center gap-3 w-56 flex-shrink-0">
          <Artwork track={track} />
          <div className="min-w-0">
            <p className="text-sm font-medium truncate">{track.title}</p>
            <p className="text-xs text-zinc-400 truncate">
              <Link href={`/artists/${track.artist.id}`} className="hover:text-white">
                {track.artist.name}
              </Link>
            </p>
          </div>
        </div>

        <div className="flex-1 flex flex-col items-center gap-1">
          <div className="flex items-center gap-4">
            <button
              onClick={() => dispatch({ type: "prev" })}
              aria-label="Previous track"
              className="text-zinc-400 hover:text-white transition-colors min-w-[44px] min-h-[44px] flex items-center justify-center"
            >
              <SkipBack size={20} />
            </button>
            <button
              onClick={() => dispatch({ type: "toggle" })}
              aria-label={isPlaying ? "Pause" : "Play"}
              className="bg-white text-black rounded-full w-9 h-9 flex items-center justify-center hover:bg-zinc-200 transition-colors"
            >
              {isPlaying ? <Pause size={18} /> : <Play size={18} fill="black" />}
            </button>
            <button
              onClick={() => dispatch({ type: "next" })}
              aria-label="Next track"
              className="text-zinc-400 hover:text-white transition-colors min-w-[44px] min-h-[44px] flex items-center justify-center"
            >
              <SkipForward size={20} />
            </button>
          </div>
          <ProgressBar />
        </div>

        <div className="flex items-center gap-1 w-56 justify-end flex-shrink-0">
          <button
            onClick={() => dispatch({ type: "toggleShuffle" })}
            aria-label="Shuffle"
            aria-pressed={shuffle}
            className={`min-w-[44px] min-h-[44px] flex items-center justify-center transition-colors ${
              shuffle ? "text-violet-400" : "text-zinc-400 hover:text-white"
            }`}
          >
            <Shuffle size={18} />
          </button>
          <button
            onClick={() => dispatch({ type: "cycleRepeat" })}
            aria-label={`Repeat: ${repeat}`}
            aria-pressed={repeat !== "off"}
            className={`min-w-[44px] min-h-[44px] flex items-center justify-center transition-colors ${
              repeat !== "off" ? "text-violet-400" : "text-zinc-400 hover:text-white"
            }`}
          >
            {repeat === "one" ? <Repeat1 size={18} /> : <Repeat size={18} />}
          </button>
          <button
            onClick={() => setQueueOpen((open) => !open)}
            title="Queue"
            aria-label="Toggle queue"
            aria-pressed={queueOpen}
            className={`min-w-[44px] min-h-[44px] flex items-center justify-center transition-colors ${
              queueOpen ? "text-violet-400" : "text-zinc-400 hover:text-white"
            }`}
          >
            <ListMusic size={18} />
          </button>
          <DevicePicker />
          <VolumeControl />
        </div>
      </div>

      {queueOpen && <QueuePanel onClose={() => setQueueOpen(false)} />}

      {mobileOpen && <MobilePlayer onClose={() => setMobileOpen(false)} />}
    </>
  );
}

function Artwork({ track }: { track: PlayerTrack }) {
  return (
    <div className="w-10 h-10 bg-zinc-800 rounded overflow-hidden flex-shrink-0">
      {track.album.hasArtwork ? (
        // eslint-disable-next-line @next/next/no-img-element
        <img src={artworkUrl(track.album.id)} alt="" className="w-full h-full object-cover" />
      ) : null}
    </div>
  );
}

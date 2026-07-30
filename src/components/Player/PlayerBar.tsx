"use client";

import { useState } from "react";
import Link from "next/link";
import { useAudio } from "@/hooks/useAudio";
import { usePlayerStore, useCurrentTrack } from "@/store/player";
import { artworkUrl } from "@/lib/types";
import { Controls } from "./Controls";
import { ProgressBar } from "./ProgressBar";
import { QueuePanel } from "./QueuePanel";
import { QueueIcon, VolumeIcon } from "./icons";

export function PlayerBar() {
  // Mounted once in the library layout, so this is where the audio element lives.
  useAudio();

  const track = useCurrentTrack();
  const volume = usePlayerStore((s) => s.volume);
  const setVolume = usePlayerStore((s) => s.setVolume);
  const [queueOpen, setQueueOpen] = useState(false);

  if (!track) return null;

  return (
    <>
      {queueOpen ? <QueuePanel onClose={() => setQueueOpen(false)} /> : null}

      <div className="border-t border-neutral-900 bg-neutral-950">
        {/* Mobile: art, title, controls stacked. */}
        <div className="flex flex-col gap-2 p-3 md:hidden">
          <div className="flex items-center gap-3">
            <Artwork track={track} />
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium">{track.title}</p>
              <p className="truncate text-xs text-neutral-500">{track.artist.name}</p>
            </div>
            <button
              onClick={() => setQueueOpen(true)}
              aria-label="Open queue"
              className="text-neutral-500 hover:text-neutral-200"
            >
              <QueueIcon className="h-5 w-5" />
            </button>
          </div>
          <ProgressBar compact />
          <div className="flex justify-center">
            <Controls />
          </div>
        </div>

        {/* Desktop: three columns, controls centred. */}
        <div className="hidden items-center gap-4 p-3 md:grid md:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]">
          <div className="flex min-w-0 items-center gap-3">
            <Artwork track={track} />
            <div className="min-w-0">
              <p className="truncate text-sm font-medium">{track.title}</p>
              <p className="truncate text-xs text-neutral-500">
                <Link href={`/artists/${track.artist.id}`} className="hover:underline">
                  {track.artist.name}
                </Link>
              </p>
            </div>
          </div>

          <div className="flex flex-col items-center gap-1.5">
            <Controls />
            <div className="w-[26rem] max-w-full">
              <ProgressBar />
            </div>
          </div>

          <div className="flex items-center justify-end gap-3">
            <VolumeIcon className="h-4 w-4 text-neutral-500" muted={volume === 0} />
            <input
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={volume}
              aria-label="Volume"
              onChange={(e) => setVolume(Number(e.target.value))}
              className="h-1 w-24 cursor-pointer appearance-none rounded-full bg-neutral-700 accent-neutral-100"
            />
            <button
              onClick={() => setQueueOpen((open) => !open)}
              aria-label="Toggle queue"
              aria-pressed={queueOpen}
              className={queueOpen ? "text-neutral-100" : "text-neutral-500 hover:text-neutral-200"}
            >
              <QueueIcon className="h-5 w-5" />
            </button>
          </div>
        </div>
      </div>
    </>
  );
}

function Artwork({ track }: { track: { album: { id: number; title: string; hasArtwork: boolean } } }) {
  return (
    <Link
      href={`/albums/${track.album.id}`}
      className="block h-12 w-12 shrink-0 overflow-hidden rounded bg-neutral-900"
    >
      {track.album.hasArtwork ? (
        // eslint-disable-next-line @next/next/no-img-element
        <img src={artworkUrl(track.album.id)} alt="" className="h-full w-full object-cover" />
      ) : null}
    </Link>
  );
}

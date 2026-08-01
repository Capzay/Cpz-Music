"use client";

import { useEffect, useRef, useState } from "react";
import { artworkUrl, type PlayerTrack } from "@/lib/types";

interface NowPlaying {
  track: PlayerTrack | null;
  isPlaying: boolean;
  currentTime: number;
}

/** Smoothing between polls. The bar is 3px tall; quarter seconds are enough. */
const TICK_MS = 250;

function formatTime(seconds: number) {
  const whole = Math.max(0, Math.floor(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}

export function Overlay({ token }: { token: string }) {
  const [state, setState] = useState<NowPlaying>({
    track: null,
    isPlaying: false,
    currentTime: 0,
  });
  const [position, setPosition] = useState(0);
  /** Album whose art 404s, held by id so a track change clears it by itself. */
  const [artFailed, setArtFailed] = useState<number | null>(null);
  /** When the poll that produced `state` landed, so the tick can extrapolate. */
  const polledAt = useRef(0);

  useEffect(() => {
    const url = `/api/now-playing${token ? `?token=${encodeURIComponent(token)}` : ""}`;
    let cancelled = false;

    async function poll() {
      try {
        const res = await fetch(url, { cache: "no-store" });
        if (!res.ok) return;
        const data = (await res.json()) as NowPlaying;
        if (cancelled) return;
        polledAt.current = Date.now();
        setState(data);
      } catch {
        // OBS keeps the source alive across restarts of this server; just retry.
      }
    }

    void poll();
    const timer = setInterval(poll, 2000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [token]);

  const { track, isPlaying } = state;
  const duration = track?.duration ?? 0;

  // Polling every two seconds would make the bar jump in two-second steps, so
  // run the clock locally between polls and let each poll correct the drift.
  useEffect(() => {
    const update = () => {
      const elapsed = isPlaying ? (Date.now() - polledAt.current) / 1000 : 0;
      setPosition(duration ? Math.min(state.currentTime + elapsed, duration) : 0);
    };
    update();
    const timer = setInterval(update, TICK_MS);
    return () => clearInterval(timer);
  }, [state, isPlaying, duration]);

  const showArt = Boolean(track?.album.hasArtwork) && artFailed !== track?.album.id;

  return (
    // Transparent background so OBS composites this over the scene.
    <main className="flex min-h-dvh items-end bg-transparent p-4">
      <div
        className={`flex w-[420px] flex-col gap-2.5 rounded-[14px] border border-white/10 bg-[rgba(10,10,10,0.82)] px-3.5 pt-3 pb-2.5 shadow-lg backdrop-blur-xl backdrop-saturate-150 transition-all duration-[450ms] ${
          track ? "translate-y-0 opacity-100" : "translate-y-2 opacity-0"
        }`}
      >
        <div className="flex items-center gap-3">
          <div className="size-[62px] shrink-0 overflow-hidden rounded-lg bg-white/5">
            {showArt && track ? (
              // eslint-disable-next-line @next/next/no-img-element
              <img
                src={artworkUrl(track.album.id)}
                alt=""
                onError={() => setArtFailed(track.album.id)}
                className="h-full w-full object-cover"
              />
            ) : null}
          </div>

          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-semibold text-white">
              {track?.title ?? ""}
              {track && !isPlaying ? (
                <span className="ml-1.5 align-middle text-[10px] text-white/55">
                  {"⏸ PAUSED"}
                </span>
              ) : null}
            </p>
            <p className="mt-0.5 truncate text-xs text-white/55">
              {track ? `${track.artist.name}  ·  ${track.album.title}` : ""}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          <span className="min-w-8 text-[11px] tabular-nums text-white/55">
            {formatTime(position)}
          </span>
          <div className="h-[3px] flex-1 overflow-hidden rounded-full bg-white/20">
            <div
              className="h-full rounded-full bg-white"
              style={{ width: duration ? `${(position / duration) * 100}%` : "0%" }}
            />
          </div>
          <span className="min-w-8 text-right text-[11px] tabular-nums text-white/55">
            {formatTime(duration)}
          </span>
        </div>
      </div>
    </main>
  );
}

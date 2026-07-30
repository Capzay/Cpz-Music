"use client";

import { useEffect, useState } from "react";
import { artworkUrl, type PlayerTrack } from "@/lib/types";

interface NowPlaying {
  track: PlayerTrack | null;
  isPlaying: boolean;
}

export function Overlay({ token }: { token: string }) {
  const [state, setState] = useState<NowPlaying>({ track: null, isPlaying: false });

  useEffect(() => {
    const url = `/api/now-playing${token ? `?token=${encodeURIComponent(token)}` : ""}`;
    let cancelled = false;

    async function poll() {
      try {
        const res = await fetch(url, { cache: "no-store" });
        if (!res.ok) return;
        const data = (await res.json()) as NowPlaying;
        if (!cancelled) setState(data);
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

  return (
    // Transparent background so OBS composites this over the scene.
    <main className="flex min-h-dvh items-end bg-transparent p-4">
      <div
        className={`flex items-center gap-3 rounded-lg bg-black/70 p-3 text-white shadow-lg backdrop-blur transition-opacity duration-500 ${
          track && isPlaying ? "opacity-100" : "opacity-0"
        }`}
      >
        <div className="h-16 w-16 shrink-0 overflow-hidden rounded bg-neutral-800">
          {track?.album.hasArtwork ? (
            // eslint-disable-next-line @next/next/no-img-element
            <img src={artworkUrl(track.album.id)} alt="" className="h-full w-full object-cover" />
          ) : null}
        </div>
        <div className="min-w-0 max-w-xs">
          <p className="truncate text-base font-semibold">{track?.title ?? ""}</p>
          <p className="truncate text-sm text-neutral-300">{track?.artist.name ?? ""}</p>
        </div>
      </div>
    </main>
  );
}

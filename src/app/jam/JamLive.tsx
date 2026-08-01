"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { supabaseBrowser } from "@/lib/supabase/client";
import { PLAYER_CHANNEL, type SharedState } from "@/lib/realtime";
import { artworkUrl } from "@/lib/types";
import { formatDuration } from "@/lib/format";

/**
 * The guest half of the jam channel.
 *
 * Guests may read the player channel but never write to it, which the Realtime
 * policy enforces. That read is enough for both things this does: react to the
 * host admitting or removing someone, and show what is playing right now.
 *
 * Mounted in every guest state, including "waiting to be let in", because that
 * is the screen that most needs to move on its own.
 */
export function JamLive({ showNowPlaying = false }: { showNowPlaying?: boolean }) {
  const router = useRouter();
  const [state, setState] = useState<SharedState | null>(null);

  useEffect(() => {
    const supabase = supabaseBrowser();
    const channel = supabase.channel(PLAYER_CHANNEL, { config: { private: true } });

    channel
      // Admitted, removed, or the jam ended. The page re-reads the server rather
      // than trusting the payload, so one event covers all three.
      .on("broadcast", { event: "jam" }, () => router.refresh())
      .on("broadcast", { event: "state" }, ({ payload }) => setState(payload as SharedState))
      .subscribe();

    return () => {
      void supabase.removeChannel(channel);
    };
  }, [router]);

  const track = showNowPlaying ? state?.track : null;
  if (!track) return null;

  const duration = track.duration ?? 0;
  const progress = duration > 0 ? Math.min(100, (state!.currentTime / duration) * 100) : 0;

  return (
    <div className="fixed inset-x-0 bottom-0 border-t border-zinc-800 bg-zinc-900/95 backdrop-blur">
      <div className="mx-auto flex max-w-2xl items-center gap-3 p-3">
        <span className="h-10 w-10 shrink-0 overflow-hidden rounded bg-zinc-800">
          {track.album.hasArtwork ? (
            // eslint-disable-next-line @next/next/no-img-element
            <img src={artworkUrl(track.album.id)} alt="" className="h-full w-full object-cover" />
          ) : null}
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-medium">{track.title}</span>
          <span className="block truncate text-xs text-zinc-500">{track.artist.name}</span>
        </span>
        <span className="shrink-0 text-xs tabular-nums text-zinc-600">
          {formatDuration(state!.currentTime)} / {formatDuration(duration)}
        </span>
      </div>
      <div className="h-0.5 bg-zinc-800">
        <div className="h-full bg-violet-500" style={{ width: `${progress}%` }} />
      </div>
    </div>
  );
}

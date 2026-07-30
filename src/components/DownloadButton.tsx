"use client";

import { useEffect } from "react";
import { useDownloads } from "@/store/downloads";
import type { PlayerTrack } from "@/lib/types";

export function DownloadButton({ tracks, label }: { tracks: PlayerTrack[]; label?: string }) {
  const registry = useDownloads((s) => s.registry);
  const pending = useDownloads((s) => s.pending);
  const active = useDownloads((s) => s.active);
  const download = useDownloads((s) => s.download);
  const hydrate = useDownloads((s) => s.hydrate);

  useEffect(() => hydrate(), [hydrate]);

  if (tracks.length === 0) return null;

  const done = tracks.filter((t) => registry[t.id]).length;
  const busy = tracks.some((t) => pending.includes(t.id) || active.includes(t.id));
  const complete = done === tracks.length;

  return (
    <button
      onClick={() => download(tracks)}
      disabled={complete || busy}
      className="flex items-center gap-2 rounded-full border border-neutral-700 px-4 py-2 text-sm font-medium transition hover:border-neutral-500 disabled:opacity-50"
    >
      {complete
        ? "Downloaded"
        : busy
          ? `Downloading ${done}/${tracks.length}`
          : (label ?? "Download")}
    </button>
  );
}

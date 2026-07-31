"use client";

import { useEffect } from "react";
import { Check, Download, Loader2 } from "lucide-react";
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

  if (busy) {
    return (
      <button
        disabled
        className="flex items-center gap-2 border border-violet-500/50 text-violet-300 px-4 py-2 rounded-full text-sm font-medium"
      >
        <Loader2 size={16} className="animate-spin" />
        {done}/{tracks.length}
      </button>
    );
  }

  if (complete) {
    return (
      <button
        disabled
        className="flex items-center gap-2 border border-violet-500 text-violet-400 px-4 py-2 rounded-full text-sm font-medium"
      >
        <Check size={16} />
        Downloaded
      </button>
    );
  }

  return (
    <button
      onClick={() => download(tracks)}
      className="flex items-center gap-2 border border-zinc-600 hover:border-violet-500 text-zinc-300 hover:text-violet-400 px-4 py-2 rounded-full text-sm font-medium transition-colors"
    >
      <Download size={16} />
      {label ?? "Download"}
    </button>
  );
}

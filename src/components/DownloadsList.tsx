"use client";

import { useEffect } from "react";
import { useDownloads } from "@/store/downloads";
import { usePlayerStore } from "@/store/player";
import { formatDuration } from "@/lib/format";
import { Play, X } from "lucide-react";

function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.round(bytes / 1024)} KB`;
}

export function DownloadsList() {
  const registry = useDownloads((s) => s.registry);
  const active = useDownloads((s) => s.active);
  const pending = useDownloads((s) => s.pending);
  const failed = useDownloads((s) => s.failed);
  const hydrate = useDownloads((s) => s.hydrate);
  const remove = useDownloads((s) => s.remove);
  const clear = useDownloads((s) => s.clear);
  const dispatch = usePlayerStore((s) => s.dispatch);

  useEffect(() => hydrate(), [hydrate]);

  const entries = Object.values(registry).sort((a, b) => b.at - a.at);
  const totalBytes = entries.reduce((sum, e) => sum + e.bytes, 0);
  const inFlight = active.length + pending.length;

  if (entries.length === 0 && inFlight === 0) {
    return (
      <p className="text-sm text-zinc-500">
        Nothing downloaded. Use the Download button on an album to keep it available offline.
      </p>
    );
  }

  return (
    <>
      <div className="mb-4 flex items-center gap-4 text-sm text-zinc-500">
        <span>
          {entries.length} tracks · {formatBytes(totalBytes)}
        </span>
        {inFlight > 0 ? <span>{inFlight} downloading</span> : null}
        {failed.length > 0 ? (
          <span className="text-red-400">{failed.length} failed</span>
        ) : null}
        {entries.length > 0 ? (
          <button
            onClick={() => {
              if (confirm("Remove all downloaded tracks from this device?")) void clear();
            }}
            className="ml-auto text-zinc-600 hover:text-red-400"
          >
            Remove all
          </button>
        ) : null}
      </div>

      <ol className="divide-y divide-zinc-900">
        {entries.map(({ track, bytes }) => (
          <li key={track.id} className="group flex items-center gap-3 py-2.5 text-sm">
            <button
              onClick={() =>
                dispatch({ type: "setQueue", tracks: entries.map((e) => e.track), startIndex: entries.findIndex((e) => e.track.id === track.id) })
              }
              className="flex min-w-0 flex-1 items-center gap-3 text-left"
              aria-label={`Play ${track.title}`}
            >
              <Play size={14} className="shrink-0 text-zinc-500 group-hover:text-white" />
              <span className="min-w-0 flex-1">
                <span className="block truncate">{track.title}</span>
                <span className="block truncate text-xs text-zinc-500">
                  {track.artist.name} · {formatBytes(bytes)}
                </span>
              </span>
            </button>
            <span className="shrink-0 tabular-nums text-zinc-500">
              {formatDuration(track.duration)}
            </span>
            <button
              onClick={() => void remove(track.id)}
              aria-label={`Remove ${track.title}`}
              className="shrink-0 text-zinc-700 opacity-0 transition group-hover:opacity-100 hover:text-red-400"
            >
              <X size={16} />
            </button>
          </li>
        ))}
      </ol>
    </>
  );
}

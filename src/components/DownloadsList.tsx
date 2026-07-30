"use client";

import { useEffect } from "react";
import { useDownloads } from "@/store/downloads";
import { usePlayerStore } from "@/store/player";
import { formatDuration } from "@/lib/format";
import { CloseIcon, PlayIcon } from "./Player/icons";

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
      <p className="text-sm text-neutral-500">
        Nothing downloaded. Use the Download button on an album to keep it available offline.
      </p>
    );
  }

  return (
    <>
      <div className="mb-4 flex items-center gap-4 text-sm text-neutral-500">
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
            className="ml-auto text-neutral-600 hover:text-red-400"
          >
            Remove all
          </button>
        ) : null}
      </div>

      <ol className="divide-y divide-neutral-900">
        {entries.map(({ track, bytes }) => (
          <li key={track.id} className="group flex items-center gap-3 py-2.5 text-sm">
            <button
              onClick={() =>
                dispatch({ type: "setQueue", tracks: entries.map((e) => e.track), startIndex: entries.findIndex((e) => e.track.id === track.id) })
              }
              className="flex min-w-0 flex-1 items-center gap-3 text-left"
              aria-label={`Play ${track.title}`}
            >
              <PlayIcon className="h-3 w-3 shrink-0 text-neutral-600 group-hover:text-neutral-100" />
              <span className="min-w-0 flex-1">
                <span className="block truncate">{track.title}</span>
                <span className="block truncate text-xs text-neutral-500">
                  {track.artist.name} · {formatBytes(bytes)}
                </span>
              </span>
            </button>
            <span className="shrink-0 tabular-nums text-neutral-500">
              {formatDuration(track.duration)}
            </span>
            <button
              onClick={() => void remove(track.id)}
              aria-label={`Remove ${track.title}`}
              className="shrink-0 text-neutral-700 opacity-0 transition group-hover:opacity-100 hover:text-red-400"
            >
              <CloseIcon className="h-4 w-4" />
            </button>
          </li>
        ))}
      </ol>
    </>
  );
}

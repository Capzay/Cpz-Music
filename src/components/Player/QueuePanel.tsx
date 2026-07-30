"use client";

import { usePlayerStore } from "@/store/player";
import { formatDuration } from "@/lib/format";
import { CloseIcon } from "./icons";

export function QueuePanel({ onClose }: { onClose: () => void }) {
  const queue = usePlayerStore((s) => s.queue);
  const index = usePlayerStore((s) => s.index);
  const dispatch = usePlayerStore((s) => s.dispatch);

  return (
    <aside
      className="fixed inset-y-0 right-0 z-40 flex w-full max-w-sm flex-col border-l border-neutral-900 bg-neutral-950 shadow-2xl"
      aria-label="Play queue"
    >
      <header className="flex items-center justify-between border-b border-neutral-900 p-4">
        <h2 className="text-sm font-medium">Queue</h2>
        <button onClick={onClose} aria-label="Close queue" className="text-neutral-500 hover:text-neutral-200">
          <CloseIcon className="h-5 w-5" />
        </button>
      </header>

      <ol className="flex-1 overflow-y-auto p-2">
        {queue.length === 0 ? (
          <li className="p-4 text-sm text-neutral-500">Nothing queued.</li>
        ) : (
          queue.map((track, i) => (
            <li key={`${track.id}-${i}`} className="group flex items-center gap-3 rounded-md px-2 py-2">
              <button
                onClick={() => dispatch({ type: "jumpTo", index: i })}
                className="min-w-0 flex-1 text-left"
              >
                <span
                  className={`block truncate text-sm ${
                    i === index ? "text-neutral-100" : "text-neutral-300"
                  }`}
                >
                  {track.title}
                </span>
                <span className="block truncate text-xs text-neutral-500">{track.artist.name}</span>
              </button>
              <span className="shrink-0 text-xs tabular-nums text-neutral-600">
                {formatDuration(track.duration)}
              </span>
              {i !== index ? (
                <button
                  onClick={() => dispatch({ type: "removeFromQueue", index: i })}
                  aria-label={`Remove ${track.title} from queue`}
                  className="shrink-0 text-neutral-700 opacity-0 transition group-hover:opacity-100 hover:text-neutral-300"
                >
                  <CloseIcon className="h-4 w-4" />
                </button>
              ) : null}
            </li>
          ))
        )}
      </ol>
    </aside>
  );
}

"use client";

import { usePlayerStore } from "@/store/player";
import { formatDuration } from "@/lib/format";

export function ProgressBar({ compact = false }: { compact?: boolean }) {
  const currentTime = usePlayerStore((s) => s.currentTime);
  const duration = usePlayerStore((s) => s.duration);
  const dispatch = usePlayerStore((s) => s.dispatch);

  const max = duration > 0 ? duration : 0;

  return (
    <div className="flex w-full items-center gap-2">
      {!compact ? (
        <span className="w-10 shrink-0 text-right text-[11px] tabular-nums text-neutral-500">
          {formatDuration(currentTime)}
        </span>
      ) : null}
      <input
        type="range"
        min={0}
        max={max || 1}
        step={0.5}
        value={Math.min(currentTime, max || 1)}
        disabled={max === 0}
        aria-label="Seek"
        onChange={(e) => dispatch({ type: "seek", time: Number(e.target.value) })}
        className="h-1 w-full cursor-pointer appearance-none rounded-full bg-neutral-700 accent-neutral-100 disabled:cursor-default"
      />
      {!compact ? (
        <span className="w-10 shrink-0 text-[11px] tabular-nums text-neutral-500">
          {formatDuration(max)}
        </span>
      ) : null}
    </div>
  );
}

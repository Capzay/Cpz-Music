"use client";

import { usePlayerStore } from "@/store/player";
import { formatDuration } from "@/lib/format";

export function ProgressBar() {
  const currentTime = usePlayerStore((s) => s.currentTime);
  const duration = usePlayerStore((s) => s.duration);
  const dispatch = usePlayerStore((s) => s.dispatch);

  const max = duration > 0 ? duration : 0;

  return (
    <div className="flex items-center gap-2 w-full">
      <span className="text-xs text-zinc-400 w-8 text-right tabular-nums">
        {formatDuration(currentTime)}
      </span>
      <input
        type="range"
        min={0}
        max={max || 1}
        step={0.5}
        value={Math.min(currentTime, max || 1)}
        disabled={max === 0}
        aria-label="Seek"
        onChange={(e) => dispatch({ type: "seek", time: Number(e.target.value) })}
        className="flex-1 h-1 cursor-pointer disabled:cursor-default"
      />
      <span className="text-xs text-zinc-400 w-8 tabular-nums">{formatDuration(max)}</span>
    </div>
  );
}

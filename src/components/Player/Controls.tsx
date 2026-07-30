"use client";

import { usePlayerStore } from "@/store/player";
import {
  NextIcon,
  PauseIcon,
  PlayIcon,
  PrevIcon,
  RepeatIcon,
  RepeatOneIcon,
  ShuffleIcon,
} from "./icons";

export function Controls({ size = "sm" }: { size?: "sm" | "lg" }) {
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const shuffle = usePlayerStore((s) => s.shuffle);
  const repeat = usePlayerStore((s) => s.repeat);
  const hasQueue = usePlayerStore((s) => s.queue.length > 0);
  const dispatch = usePlayerStore((s) => s.dispatch);

  const icon = size === "lg" ? "h-7 w-7" : "h-4 w-4";
  const playIcon = size === "lg" ? "h-8 w-8" : "h-4 w-4";
  const playButton =
    size === "lg"
      ? "h-16 w-16 rounded-full bg-neutral-100 text-neutral-900"
      : "h-9 w-9 rounded-full bg-neutral-100 text-neutral-900";

  const toggleClass = (on: boolean) =>
    `transition ${on ? "text-neutral-100" : "text-neutral-500 hover:text-neutral-300"}`;

  return (
    <div className={`flex items-center ${size === "lg" ? "gap-6" : "gap-3"}`}>
      <button
        onClick={() => dispatch({ type: "toggleShuffle" })}
        aria-label="Shuffle"
        aria-pressed={shuffle}
        className={toggleClass(shuffle)}
      >
        <ShuffleIcon className={icon} />
      </button>

      <button
        onClick={() => dispatch({ type: "prev" })}
        disabled={!hasQueue}
        aria-label="Previous track"
        className="text-neutral-300 transition hover:text-white disabled:opacity-30"
      >
        <PrevIcon className={icon} />
      </button>

      <button
        onClick={() => dispatch({ type: "toggle" })}
        disabled={!hasQueue}
        aria-label={isPlaying ? "Pause" : "Play"}
        className={`${playButton} flex items-center justify-center transition hover:bg-white disabled:opacity-30`}
      >
        {isPlaying ? (
          <PauseIcon className={playIcon} />
        ) : (
          <PlayIcon className={`${playIcon} ml-0.5`} />
        )}
      </button>

      <button
        onClick={() => dispatch({ type: "next" })}
        disabled={!hasQueue}
        aria-label="Next track"
        className="text-neutral-300 transition hover:text-white disabled:opacity-30"
      >
        <NextIcon className={icon} />
      </button>

      <button
        onClick={() => dispatch({ type: "cycleRepeat" })}
        aria-label={`Repeat: ${repeat}`}
        aria-pressed={repeat !== "off"}
        className={toggleClass(repeat !== "off")}
      >
        {repeat === "one" ? <RepeatOneIcon className={icon} /> : <RepeatIcon className={icon} />}
      </button>
    </div>
  );
}

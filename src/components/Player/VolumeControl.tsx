"use client";

import { useState } from "react";
import { Volume2, VolumeX } from "lucide-react";
import { usePlayerStore } from "@/store/player";

export function VolumeControl({ className = "hidden md:flex" }: { className?: string }) {
  const volume = usePlayerStore((s) => s.volume);
  const setVolume = usePlayerStore((s) => s.setVolume);
  const [prevVolume, setPrevVolume] = useState(1);

  const toggleMute = () => {
    if (volume > 0) {
      setPrevVolume(volume);
      setVolume(0);
    } else {
      setVolume(prevVolume);
    }
  };

  return (
    <div className={`${className} items-center gap-2`}>
      <button
        onClick={toggleMute}
        aria-label={volume === 0 ? "Unmute" : "Mute"}
        className="text-zinc-400 hover:text-white transition-colors min-w-[44px] min-h-[44px] flex items-center justify-center"
      >
        {volume === 0 ? <VolumeX size={18} /> : <Volume2 size={18} />}
      </button>
      <input
        type="range"
        min={0}
        max={1}
        step={0.01}
        value={volume}
        aria-label="Volume"
        onChange={(e) => setVolume(Number(e.target.value))}
        className="w-24 h-1 cursor-pointer"
      />
    </div>
  );
}

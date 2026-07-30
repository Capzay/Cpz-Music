"use client";

import { create } from "zustand";
import {
  buildShuffleOrder,
  insertIntoShuffleOrder,
  nextPosition,
  previousPosition,
  removeFromShuffleOrder,
  type QueuePosition,
} from "@/lib/queue";
import type { PlayerTrack, RepeatMode } from "@/lib/types";
import type { DevicePresence, SharedState } from "@/lib/realtime";

/**
 * Everything the player can be asked to do.
 *
 * The old store spelled each of these out twice: a public method that decided
 * whether to act locally or forward to the active device, and a `_local`
 * twin that did the work. One command type and one dispatch replaces both, so
 * adding remote control later touches one function instead of twelve.
 */
export type Command =
  | { type: "play"; track?: PlayerTrack }
  | { type: "pause" }
  | { type: "toggle" }
  | { type: "next" }
  | { type: "prev" }
  | { type: "setQueue"; tracks: PlayerTrack[]; startIndex?: number }
  | { type: "playNext"; track: PlayerTrack }
  | { type: "addToQueue"; track: PlayerTrack }
  | { type: "removeFromQueue"; index: number }
  | { type: "jumpTo"; index: number }
  | { type: "seek"; time: number }
  | { type: "toggleShuffle" }
  | { type: "cycleRepeat" };

interface PlayerState {
  queue: PlayerTrack[];
  index: number;
  isPlaying: boolean;
  volume: number;
  currentTime: number;
  duration: number;
  shuffle: boolean;
  shuffleOrder: number[];
  shufflePos: number;
  repeat: RepeatMode;
  /** Set to ask the audio element to seek; cleared once it has. */
  seekTarget: number | null;

  // ── Multi-device ────────────────────────────────────────────────────────────
  devices: DevicePresence[];
  activeDeviceId: string | null;
  isActiveDevice: boolean;
  /**
   * Set by the sync hook once the channel is joined. Until then commands apply
   * locally, so a single device works with Realtime unreachable or disabled.
   */
  forward: ((command: Command) => void) | null;

  dispatch: (command: Command) => void;
  apply: (command: Command) => void;
  applySharedState: (state: SharedState) => void;
  setDevices: (devices: DevicePresence[], activeDeviceId: string | null, selfId: string) => void;
  setVolume: (volume: number) => void;
  setCurrentTime: (time: number) => void;
  setDuration: (duration: number) => void;
  clearSeekTarget: () => void;
}

const NEXT_REPEAT: Record<RepeatMode, RepeatMode> = { off: "all", all: "one", one: "off" };

export const usePlayerStore = create<PlayerState>((set, get) => {
  function position(): QueuePosition {
    const s = get();
    return {
      length: s.queue.length,
      index: s.index,
      shuffle: s.shuffle,
      shuffleOrder: s.shuffleOrder,
      shufflePos: s.shufflePos,
      repeat: s.repeat,
    };
  }

  /** Applies a queue navigation result, reshuffling when the order runs out. */
  function moveTo(advance: ReturnType<typeof nextPosition>) {
    if (advance.kind === "stop") return set({ isPlaying: false });
    if (advance.kind === "restart") return set({ seekTarget: 0, isPlaying: true });

    if (advance.index === -1) {
      const s = get();
      const order = buildShuffleOrder(s.queue.length, s.shuffleOrder[0] ?? 0);
      return set({ shuffleOrder: order, shufflePos: 0, index: order[0] ?? 0, isPlaying: true });
    }
    set({ index: advance.index, shufflePos: advance.shufflePos, isPlaying: true });
  }

  return {
    queue: [],
    index: 0,
    isPlaying: false,
    volume: 1,
    currentTime: 0,
    duration: 0,
    shuffle: false,
    shuffleOrder: [],
    shufflePos: 0,
    repeat: "off",
    seekTarget: null,

    devices: [],
    activeDeviceId: null,
    isActiveDevice: true,
    forward: null,

    /**
     * The one place that knows whether this device plays audio or only acts as a
     * remote. Everything in the UI calls dispatch and stays unaware.
     */
    dispatch: (command) => {
      const { isActiveDevice, forward } = get();
      if (!isActiveDevice && forward) return forward(command);
      get().apply(command);
    },

    apply: (command) => {
      const s = get();
      switch (command.type) {
        case "play":
          if (command.track) {
            const at = s.queue.findIndex((t) => t.id === command.track!.id);
            if (at >= 0) set({ index: at, isPlaying: true });
            else set({ queue: [command.track], index: 0, isPlaying: true, shuffleOrder: [] });
          } else {
            set({ isPlaying: true });
          }
          return;

        case "pause":
          return set({ isPlaying: false });

        case "toggle":
          return set({ isPlaying: !s.isPlaying });

        case "next":
          return moveTo(nextPosition(position()));

        case "prev":
          return moveTo(previousPosition(position(), s.currentTime));

        case "setQueue": {
          const startIndex = command.startIndex ?? 0;
          return set({
            queue: command.tracks,
            index: startIndex,
            isPlaying: command.tracks.length > 0,
            shuffleOrder: s.shuffle
              ? buildShuffleOrder(command.tracks.length, startIndex)
              : [],
            shufflePos: 0,
            currentTime: 0,
          });
        }

        case "playNext": {
          const at = s.index + 1;
          return set({
            queue: [...s.queue.slice(0, at), command.track, ...s.queue.slice(at)],
            shuffleOrder:
              s.shuffle && s.shuffleOrder.length > 0
                ? insertIntoShuffleOrder(s.shuffleOrder, at, s.shufflePos)
                : s.shuffleOrder,
          });
        }

        case "addToQueue": {
          const queue = [...s.queue, command.track];
          return set({
            queue,
            shuffleOrder:
              s.shuffle && s.shuffleOrder.length > 0
                ? [...s.shuffleOrder, queue.length - 1]
                : s.shuffleOrder,
          });
        }

        case "removeFromQueue": {
          // Removing what is playing would leave the audio element pointed at a
          // track no longer in the queue.
          if (command.index === s.index) return;
          const queue = s.queue.filter((_, i) => i !== command.index);
          const index = command.index < s.index ? s.index - 1 : s.index;
          const shuffleOrder =
            s.shuffle && s.shuffleOrder.length > 0
              ? removeFromShuffleOrder(s.shuffleOrder, command.index)
              : s.shuffleOrder;
          return set({
            queue,
            index,
            shuffleOrder,
            shufflePos: Math.max(0, shuffleOrder.indexOf(index)),
          });
        }

        case "jumpTo": {
          if (command.index < 0 || command.index >= s.queue.length) return;
          // Keep the shuffle cursor pointing at the track actually playing, or
          // the next track would be picked relative to the wrong position.
          const shufflePos = s.shuffle ? s.shuffleOrder.indexOf(command.index) : 0;
          return set({
            index: command.index,
            isPlaying: true,
            shufflePos: Math.max(0, shufflePos),
          });
        }

        case "seek":
          return set({ seekTarget: command.time, currentTime: command.time });

        case "toggleShuffle":
          return set(
            s.shuffle
              ? { shuffle: false, shuffleOrder: [], shufflePos: 0 }
              : {
                  shuffle: true,
                  shuffleOrder: buildShuffleOrder(s.queue.length, s.index),
                  shufflePos: 0,
                },
          );

        case "cycleRepeat":
          return set({ repeat: NEXT_REPEAT[s.repeat] });
      }
    },

    /** Mirrors what the active device is doing onto a remote's screen. */
    applySharedState: (state) => {
      set({
        queue: state.queue,
        index: state.index,
        isPlaying: state.isPlaying,
        currentTime: state.currentTime,
        shuffle: state.shuffle,
        repeat: state.repeat,
        // Shuffle bookkeeping belongs to whichever device is actually playing.
        shuffleOrder: [],
        shufflePos: 0,
      });
    },

    setDevices: (devices, activeDeviceId, selfId) => {
      const isActiveDevice = activeDeviceId === null || activeDeviceId === selfId;
      // Stop making noise the moment this device stops being the active one.
      if (!isActiveDevice && get().isActiveDevice) set({ isPlaying: false });
      set({ devices, activeDeviceId, isActiveDevice });
    },

    setVolume: (volume) => {
      set({ volume });
      try {
        localStorage.setItem("cpz-volume", String(volume));
      } catch {
        // Private browsing, or storage full. Volume just will not persist.
      }
    },
    setCurrentTime: (currentTime) => set({ currentTime }),
    setDuration: (duration) => set({ duration }),
    clearSeekTarget: () => set({ seekTarget: null }),
  };
});

/** Single source of truth: the current track is always whatever the index points at. */
export const useCurrentTrack = () =>
  usePlayerStore((s) => s.queue[s.index] ?? null);

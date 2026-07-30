import type { RepeatMode } from "@/lib/types";

/**
 * Pure queue navigation. Kept separate from the store because this is the only
 * part with real logic in it, and off-by-one errors here show up as "shuffle
 * repeats a song" rather than as a crash.
 */

export interface QueuePosition {
  length: number;
  index: number;
  shuffle: boolean;
  shuffleOrder: number[];
  shufflePos: number;
  repeat: RepeatMode;
}

export type Advance =
  | { kind: "move"; index: number; shufflePos: number }
  | { kind: "restart" }
  | { kind: "stop" };

function shuffled(items: number[], rng: () => number): number[] {
  const a = [...items];
  for (let i = a.length - 1; i > 0; i--) {
    const j = Math.floor(rng() * (i + 1));
    [a[i], a[j]] = [a[j], a[i]];
  }
  return a;
}

/**
 * A permutation of queue indices that always starts at the track playing now,
 * so turning shuffle on never interrupts the current song.
 */
export function buildShuffleOrder(
  length: number,
  currentIndex: number,
  rng: () => number = Math.random,
): number[] {
  if (length <= 0) return [];
  const rest = Array.from({ length }, (_, i) => i).filter((i) => i !== currentIndex);
  const head = currentIndex >= 0 && currentIndex < length ? [currentIndex] : [];
  return [...head, ...shuffled(rest, rng)];
}

export function nextPosition(state: QueuePosition): Advance {
  const { length, index, shuffle, shuffleOrder, shufflePos, repeat } = state;
  if (length === 0) return { kind: "stop" };

  // Repeat-one wins over everything, including the end of the queue.
  if (repeat === "one") return { kind: "restart" };

  if (shuffle && shuffleOrder.length > 0) {
    const pos = shufflePos + 1;
    if (pos < shuffleOrder.length) {
      return { kind: "move", index: shuffleOrder[pos], shufflePos: pos };
    }
    // Ran out of shuffled tracks. Repeat-all reshuffles; otherwise stop.
    return repeat === "all" ? { kind: "move", index: -1, shufflePos: 0 } : { kind: "stop" };
  }

  const next = index + 1;
  if (next < length) return { kind: "move", index: next, shufflePos: 0 };
  return repeat === "all" ? { kind: "move", index: 0, shufflePos: 0 } : { kind: "stop" };
}

/**
 * Restarts the current track when more than `restartAfter` seconds have played,
 * matching what every other music player does with a back button.
 */
export function previousPosition(
  state: QueuePosition,
  currentTime: number,
  restartAfter = 3,
): Advance {
  const { length, index, shuffle, shuffleOrder, shufflePos } = state;
  if (length === 0) return { kind: "stop" };
  if (currentTime > restartAfter) return { kind: "restart" };

  if (shuffle && shuffleOrder.length > 0) {
    const pos = Math.max(0, shufflePos - 1);
    return { kind: "move", index: shuffleOrder[pos], shufflePos: pos };
  }

  return { kind: "move", index: Math.max(0, index - 1), shufflePos: 0 };
}

/** Reindexes a shuffle order after a track is spliced out of the queue. */
export function removeFromShuffleOrder(order: number[], removed: number): number[] {
  return order.filter((i) => i !== removed).map((i) => (i > removed ? i - 1 : i));
}

/** Reindexes a shuffle order after a track is spliced into the queue. */
export function insertIntoShuffleOrder(
  order: number[],
  insertedAt: number,
  playAfterPos: number,
): number[] {
  const shifted = order.map((i) => (i >= insertedAt ? i + 1 : i));
  return [
    ...shifted.slice(0, playAfterPos + 1),
    insertedAt,
    ...shifted.slice(playAfterPos + 1),
  ];
}

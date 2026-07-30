import { describe, expect, it } from "vitest";
import {
  buildShuffleOrder,
  insertIntoShuffleOrder,
  nextPosition,
  previousPosition,
  removeFromShuffleOrder,
  type QueuePosition,
} from "./queue";

function pos(overrides: Partial<QueuePosition> = {}): QueuePosition {
  return {
    length: 5,
    index: 0,
    shuffle: false,
    shuffleOrder: [],
    shufflePos: 0,
    repeat: "off",
    ...overrides,
  };
}

describe("buildShuffleOrder", () => {
  it("keeps the current track first so enabling shuffle does not cut it off", () => {
    expect(buildShuffleOrder(5, 3)[0]).toBe(3);
  });

  it("is a permutation of every index, losing and duplicating nothing", () => {
    const order = buildShuffleOrder(50, 10);
    expect([...order].sort((a, b) => a - b)).toEqual(Array.from({ length: 50 }, (_, i) => i));
  });

  it("handles an empty queue", () => {
    expect(buildShuffleOrder(0, 0)).toEqual([]);
  });

  it("handles a current index outside the queue without duplicating it", () => {
    const order = buildShuffleOrder(3, 99);
    expect([...order].sort((a, b) => a - b)).toEqual([0, 1, 2]);
  });

  it("actually shuffles the tail", () => {
    // Reversing rng makes the result deterministic without stubbing globals.
    const order = buildShuffleOrder(6, 0, () => 0);
    expect(order[0]).toBe(0);
    expect(order).not.toEqual([0, 1, 2, 3, 4, 5]);
  });
});

describe("nextPosition", () => {
  it("advances through the queue", () => {
    expect(nextPosition(pos({ index: 1 }))).toEqual({ kind: "move", index: 2, shufflePos: 0 });
  });

  it("stops at the end when repeat is off", () => {
    expect(nextPosition(pos({ index: 4 }))).toEqual({ kind: "stop" });
  });

  it("wraps to the start when repeat is all", () => {
    expect(nextPosition(pos({ index: 4, repeat: "all" }))).toEqual({
      kind: "move",
      index: 0,
      shufflePos: 0,
    });
  });

  it("restarts the same track when repeat is one, even mid-queue", () => {
    expect(nextPosition(pos({ index: 2, repeat: "one" }))).toEqual({ kind: "restart" });
  });

  it("restarts when repeat is one at the end of the queue", () => {
    expect(nextPosition(pos({ index: 4, repeat: "one" }))).toEqual({ kind: "restart" });
  });

  it("follows the shuffle order rather than queue order", () => {
    const state = pos({ shuffle: true, shuffleOrder: [2, 0, 4, 1, 3], shufflePos: 1, index: 0 });
    expect(nextPosition(state)).toEqual({ kind: "move", index: 4, shufflePos: 2 });
  });

  it("stops after the last shuffled track when repeat is off", () => {
    const state = pos({ shuffle: true, shuffleOrder: [2, 0, 4, 1, 3], shufflePos: 4 });
    expect(nextPosition(state)).toEqual({ kind: "stop" });
  });

  it("signals a reshuffle after the last shuffled track when repeat is all", () => {
    const state = pos({
      shuffle: true,
      shuffleOrder: [2, 0, 4, 1, 3],
      shufflePos: 4,
      repeat: "all",
    });
    expect(nextPosition(state)).toEqual({ kind: "move", index: -1, shufflePos: 0 });
  });

  it("stops on an empty queue", () => {
    expect(nextPosition(pos({ length: 0 }))).toEqual({ kind: "stop" });
  });
});

describe("previousPosition", () => {
  it("restarts the track when it is already past three seconds", () => {
    expect(previousPosition(pos({ index: 2 }), 10)).toEqual({ kind: "restart" });
  });

  it("steps back when barely into the track", () => {
    expect(previousPosition(pos({ index: 2 }), 1)).toEqual({
      kind: "move",
      index: 1,
      shufflePos: 0,
    });
  });

  it("does not step before the first track", () => {
    expect(previousPosition(pos({ index: 0 }), 0)).toEqual({
      kind: "move",
      index: 0,
      shufflePos: 0,
    });
  });

  it("walks the shuffle order backwards", () => {
    const state = pos({ shuffle: true, shuffleOrder: [2, 0, 4, 1, 3], shufflePos: 2 });
    expect(previousPosition(state, 0)).toEqual({ kind: "move", index: 0, shufflePos: 1 });
  });
});

describe("shuffle order maintenance", () => {
  it("drops a removed index and closes the gap", () => {
    expect(removeFromShuffleOrder([2, 0, 4, 1, 3], 2)).toEqual([0, 3, 1, 2]);
  });

  it("stays a valid permutation after a removal", () => {
    const order = removeFromShuffleOrder(buildShuffleOrder(10, 0), 5);
    expect([...order].sort((a, b) => a - b)).toEqual(Array.from({ length: 9 }, (_, i) => i));
  });

  it("queues an inserted track to play next", () => {
    const order = insertIntoShuffleOrder([2, 0, 4, 1, 3], 1, 0);
    expect(order[1]).toBe(1);
  });

  it("stays a valid permutation after an insertion", () => {
    const order = insertIntoShuffleOrder(buildShuffleOrder(10, 0), 3, 0);
    expect([...order].sort((a, b) => a - b)).toEqual(Array.from({ length: 11 }, (_, i) => i));
  });
});

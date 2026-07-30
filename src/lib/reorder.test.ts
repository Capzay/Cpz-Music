import { describe, expect, it } from "vitest";
import { moveItem } from "./reorder";

describe("moveItem", () => {
  const items = ["a", "b", "c", "d"];

  it("moves an item later", () => {
    expect(moveItem(items, 0, 2)).toEqual(["b", "c", "a", "d"]);
  });

  it("moves an item earlier", () => {
    expect(moveItem(items, 3, 1)).toEqual(["a", "d", "b", "c"]);
  });

  it("is a no-op when nothing moves", () => {
    expect(moveItem(items, 1, 1)).toEqual(items);
  });

  it("clamps a target past the end instead of dropping the item", () => {
    expect(moveItem(items, 0, 99)).toEqual(["b", "c", "d", "a"]);
  });

  it("clamps a negative target", () => {
    expect(moveItem(items, 2, -5)).toEqual(["c", "a", "b", "d"]);
  });

  it("ignores an out-of-range source", () => {
    expect(moveItem(items, 9, 0)).toEqual(items);
  });

  it("never loses or duplicates an item", () => {
    const moved = moveItem(items, 1, 3);
    expect([...moved].sort()).toEqual([...items].sort());
    expect(moved).toHaveLength(items.length);
  });

  it("does not mutate the input", () => {
    const original = [...items];
    moveItem(items, 0, 3);
    expect(items).toEqual(original);
  });
});

import { describe, expect, it } from "vitest";
import { parseRange } from "./range";
import { isInside } from "./paths";

describe("parseRange", () => {
  const size = 1000;

  it("serves the whole file when no header is sent", () => {
    expect(parseRange(null, size)).toEqual({ kind: "whole" });
    expect(parseRange(undefined, size)).toEqual({ kind: "whole" });
  });

  it("parses an explicit window", () => {
    expect(parseRange("bytes=100-199", size)).toEqual({ kind: "range", start: 100, end: 199 });
  });

  it("treats an open end as the rest of the file", () => {
    expect(parseRange("bytes=500-", size)).toEqual({ kind: "range", start: 500, end: 999 });
  });

  it("clamps an end past the file to the last byte", () => {
    expect(parseRange("bytes=0-99999", size)).toEqual({ kind: "range", start: 0, end: 999 });
  });

  it("handles the suffix form", () => {
    expect(parseRange("bytes=-200", size)).toEqual({ kind: "range", start: 800, end: 999 });
  });

  it("clamps a suffix longer than the file", () => {
    expect(parseRange("bytes=-99999", size)).toEqual({ kind: "range", start: 0, end: 999 });
  });

  it("rejects a start past the end of the file", () => {
    expect(parseRange("bytes=1000-", size)).toEqual({ kind: "unsatisfiable" });
    expect(parseRange("bytes=5000-6000", size)).toEqual({ kind: "unsatisfiable" });
  });

  it("rejects a backwards window", () => {
    expect(parseRange("bytes=500-100", size)).toEqual({ kind: "unsatisfiable" });
  });

  it("rejects a zero-length suffix", () => {
    expect(parseRange("bytes=-0", size)).toEqual({ kind: "unsatisfiable" });
  });

  it("never produces a negative start", () => {
    const parsed = parseRange("bytes=-99999", size);
    expect(parsed).toMatchObject({ start: 0 });
  });

  it("ignores garbage rather than producing NaN offsets", () => {
    expect(parseRange("bytes=abc", size)).toEqual({ kind: "whole" });
    expect(parseRange("bytes=--", size)).toEqual({ kind: "whole" });
    expect(parseRange("items=0-10", size)).toEqual({ kind: "whole" });
    expect(parseRange("bytes=", size)).toEqual({ kind: "whole" });
  });

  it("falls back to the whole file for multi-range requests", () => {
    expect(parseRange("bytes=0-10,20-30", size)).toEqual({ kind: "whole" });
  });

  it("cannot satisfy any range on an empty file", () => {
    expect(parseRange("bytes=0-", 0)).toEqual({ kind: "unsatisfiable" });
  });
});

describe("isInside", () => {
  it("accepts a file within the library", () => {
    expect(isInside("/music", "/music/artist/album/song.mp3")).toBe(true);
  });

  it("rejects traversal out of the library", () => {
    expect(isInside("/music", "/music/../etc/passwd")).toBe(false);
    expect(isInside("/music", "/etc/passwd")).toBe(false);
  });

  it("rejects a sibling directory with a shared prefix", () => {
    expect(isInside("/music", "/music-private/secret.mp3")).toBe(false);
  });

  it("rejects the library root itself", () => {
    expect(isInside("/music", "/music")).toBe(false);
  });
});

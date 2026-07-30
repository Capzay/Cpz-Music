import { describe, expect, it } from "vitest";
import { listenDay } from "./listen-day";

const NOW = Date.UTC(2026, 6, 30, 14, 30, 0);

describe("listenDay", () => {
  it("truncates to the UTC day", () => {
    expect(listenDay(NOW, NOW).toISOString()).toBe("2026-07-30T00:00:00.000Z");
  });

  it("keeps an offline listen on the day it happened", () => {
    const threeDaysAgo = NOW - 3 * 24 * 3600_000;
    expect(listenDay(threeDaysAgo, NOW).toISOString()).toBe("2026-07-27T00:00:00.000Z");
  });

  it("falls back to now when no timestamp is given", () => {
    expect(listenDay(undefined, NOW).toISOString()).toBe("2026-07-30T00:00:00.000Z");
  });

  it("rejects a timestamp from the future, which would skew every chart", () => {
    const nextYear = NOW + 365 * 24 * 3600_000;
    expect(listenDay(nextYear, NOW).toISOString()).toBe("2026-07-30T00:00:00.000Z");
  });

  it("rejects a timestamp older than a year", () => {
    const longAgo = NOW - 400 * 24 * 3600_000;
    expect(listenDay(longAgo, NOW).toISOString()).toBe("2026-07-30T00:00:00.000Z");
  });

  it("allows small clock skew rather than discarding it", () => {
    const slightlyAhead = NOW + 60_000;
    expect(listenDay(slightlyAhead, NOW).toISOString()).toBe("2026-07-30T00:00:00.000Z");
  });

  it("ignores non-numeric input rather than producing an invalid date", () => {
    for (const bad of ["yesterday", null, NaN, Infinity, {}]) {
      expect(listenDay(bad, NOW).toISOString()).toBe("2026-07-30T00:00:00.000Z");
    }
  });
});

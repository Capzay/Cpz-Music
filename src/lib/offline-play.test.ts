import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PlayerTrack } from "@/lib/types";

vi.mock("@/store/downloads", () => ({
  useDownloads: {
    getState: vi.fn(),
  },
}));

import { useDownloads } from "@/store/downloads";
import { canPlayTrack, queueForPlayback } from "@/lib/offline-play";

function track(id: number): PlayerTrack {
  return {
    id,
    title: `t${id}`,
    duration: 1,
    trackNumber: id,
    discNumber: 1,
    artist: { id: 1, name: "a" },
    album: { id: 1, title: "al", hasArtwork: false },
  };
}

describe("canPlayTrack", () => {
  it("streams anything while online", () => {
    expect(canPlayTrack(true, false)).toBe(true);
    expect(canPlayTrack(true, true)).toBe(true);
  });

  it("requires a download while offline", () => {
    expect(canPlayTrack(false, true)).toBe(true);
    expect(canPlayTrack(false, false)).toBe(false);
  });
});

describe("queueForPlayback", () => {
  beforeEach(() => {
    vi.mocked(useDownloads.getState).mockReturnValue({
      registry: { 2: {}, 4: {} },
    } as unknown as ReturnType<typeof useDownloads.getState>);
  });

  it("passes the queue through while online", () => {
    vi.stubGlobal("navigator", { onLine: true });
    const tracks = [track(1), track(2), track(3)];
    expect(queueForPlayback(tracks, 1)).toEqual({ tracks, startIndex: 1 });
  });

  it("keeps only downloaded tracks while offline", () => {
    vi.stubGlobal("navigator", { onLine: false });
    const tracks = [track(1), track(2), track(3), track(4)];
    expect(queueForPlayback(tracks, 0)).toEqual({
      tracks: [tracks[1], tracks[3]],
      startIndex: 0,
    });
  });

  it("maps the clicked track into the filtered queue", () => {
    vi.stubGlobal("navigator", { onLine: false });
    const tracks = [track(1), track(2), track(3), track(4)];
    expect(queueForPlayback(tracks, 3)).toEqual({
      tracks: [tracks[1], tracks[3]],
      startIndex: 1,
    });
  });

  it("starts at the next downloaded track when the click was not cached", () => {
    vi.stubGlobal("navigator", { onLine: false });
    const tracks = [track(1), track(2), track(3), track(4)];
    expect(queueForPlayback(tracks, 0).startIndex).toBe(0);
    expect(queueForPlayback(tracks, 2)).toEqual({
      tracks: [tracks[1], tracks[3]],
      startIndex: 1,
    });
  });

  it("returns an empty queue when nothing is downloaded", () => {
    vi.stubGlobal("navigator", { onLine: false });
    vi.mocked(useDownloads.getState).mockReturnValue({
      registry: {},
    } as unknown as ReturnType<typeof useDownloads.getState>);
    expect(queueForPlayback([track(1), track(2)], 0)).toEqual({
      tracks: [],
      startIndex: 0,
    });
  });
});

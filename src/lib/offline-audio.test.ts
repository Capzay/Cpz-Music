import { afterEach, describe, expect, it, vi } from "vitest";
import { streamUrl } from "@/lib/types";
import {
  audioCacheKey,
  clearOfflineUrls,
  playbackSrc,
  preferCachedAudio,
} from "@/lib/offline-audio";

const origin = "https://music.example";

afterEach(() => {
  clearOfflineUrls();
  vi.unstubAllGlobals();
});

describe("preferCachedAudio", () => {
  it("uses the cache for a download even while online", () => {
    vi.stubGlobal("navigator", { onLine: true });
    expect(preferCachedAudio(true)).toBe(true);
  });

  it("streams a track that was never downloaded while online", () => {
    vi.stubGlobal("navigator", { onLine: true });
    expect(preferCachedAudio(false)).toBe(false);
  });

  it("checks the cache while offline", () => {
    vi.stubGlobal("navigator", { onLine: false });
    expect(preferCachedAudio(false)).toBe(true);
  });
});

describe("playbackSrc", () => {
  afterEach(() => {
    clearOfflineUrls();
    delete (URL as { createObjectURL?: unknown }).createObjectURL;
    delete (URL as { revokeObjectURL?: unknown }).revokeObjectURL;
  });

  function installCache(body: string | null) {
    vi.stubGlobal("location", { origin });
    const created: string[] = [];
    URL.createObjectURL = () => {
      const url = `blob:mock-${created.length + 1}`;
      created.push(url);
      return url;
    };
    URL.revokeObjectURL = () => {};

    vi.stubGlobal("caches", {
      open: vi.fn(async () => ({
        match: vi.fn(async (key: string) => {
          if (body == null) return undefined;
          expect(key).toBe(audioCacheKey(7));
          return new Response(new Blob([body], { type: "audio/flac" }));
        }),
      })),
    });
    return created;
  }

  it("returns a blob URL for a cached track and reuses it", async () => {
    const created = installCache("audio-bytes");
    await expect(playbackSrc(7, true)).resolves.toBe("blob:mock-1");
    await expect(playbackSrc(7, true)).resolves.toBe("blob:mock-1");
    expect(created).toEqual(["blob:mock-1"]);
  });

  it("falls back to the stream when the cache has no bytes", async () => {
    installCache(null);
    await expect(playbackSrc(7, true)).resolves.toBe(streamUrl(7));
  });

  it("does not open the cache for an online stream", async () => {
    const open = vi.fn();
    vi.stubGlobal("caches", { open });
    await expect(playbackSrc(7, false)).resolves.toBe(streamUrl(7));
    expect(open).not.toHaveBeenCalled();
  });
});

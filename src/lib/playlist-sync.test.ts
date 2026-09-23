import { describe, expect, it } from "vitest";
import type { PlayerTrack } from "@/lib/types";
import {
  cleanName,
  decideWrite,
  filterKnownTrackIds,
  findPlaylist,
  isUuid,
  mergeSnapshot,
  parseDirtyPlaylists,
  parseSnapshots,
  toDirty,
  visiblePlaylists,
  type LocalPlaylist,
  type PlaylistSnapshot,
} from "./playlist-sync";

const TRACK: PlayerTrack = {
  id: 1,
  title: "One",
  duration: 120,
  trackNumber: 1,
  discNumber: 1,
  artist: { id: 2, name: "Artist" },
  album: { id: 3, title: "Album", hasArtwork: true },
};

function local(overrides: Partial<LocalPlaylist> = {}): LocalPlaylist {
  return {
    uuid: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
    id: 1,
    name: "Mine",
    updatedAt: 100,
    tracks: [TRACK],
    dirty: false,
    ...overrides,
  };
}

function remote(overrides: Partial<PlaylistSnapshot> = {}): PlaylistSnapshot {
  return {
    uuid: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
    id: 1,
    name: "Server",
    updatedAt: 200,
    tracks: [TRACK],
    ...overrides,
  };
}

describe("isUuid", () => {
  it("accepts a randomUUID", () => {
    expect(isUuid("1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed")).toBe(true);
  });

  it("rejects a numeric playlist id and junk", () => {
    expect(isUuid("12")).toBe(false);
    expect(isUuid("not-a-uuid")).toBe(false);
    expect(isUuid("")).toBe(false);
  });
});

describe("cleanName", () => {
  it("trims and caps length", () => {
    expect(cleanName("  Road trip  ")).toBe("Road trip");
    expect(cleanName("x".repeat(200))).toHaveLength(120);
  });

  it("returns empty for blank input", () => {
    expect(cleanName("   ")).toBe("");
    expect(cleanName(null)).toBe("");
  });
});

describe("decideWrite", () => {
  it("creates when the server has never seen the uuid", () => {
    expect(decideWrite(10, false, null)).toBe("create");
  });

  it("ignores a tombstone for a playlist that does not exist", () => {
    expect(decideWrite(10, true, null)).toBe("skip");
  });

  it("skips a write that is older than the stored row", () => {
    expect(decideWrite(5, false, 10)).toBe("skip");
    expect(decideWrite(5, true, 10)).toBe("skip");
  });

  it("applies a write at the same timestamp, so a retry is idempotent", () => {
    expect(decideWrite(10, false, 10)).toBe("replace");
    expect(decideWrite(10, true, 10)).toBe("delete");
  });

  it("applies a newer write", () => {
    expect(decideWrite(20, false, 10)).toBe("replace");
    expect(decideWrite(20, true, 10)).toBe("delete");
  });
});

describe("filterKnownTrackIds", () => {
  it("drops unknown ids and keeps order", () => {
    expect(filterKnownTrackIds([3, 99, 1, 3], new Set([1, 3]))).toEqual([3, 1, 3]);
  });
});

describe("mergeSnapshot", () => {
  it("takes the server copy when local is clean", () => {
    const merged = mergeSnapshot([], [remote()]);
    expect(merged).toHaveLength(1);
    expect(merged[0].name).toBe("Server");
    expect(merged[0].dirty).toBe(false);
  });

  it("keeps a dirty local edit over the server copy of the same uuid", () => {
    const merged = mergeSnapshot([local({ dirty: true, name: "Local" })], [remote()]);
    expect(merged).toHaveLength(1);
    expect(merged[0].name).toBe("Local");
    expect(merged[0].dirty).toBe(true);
  });

  it("keeps a dirty local create that the server has not seen", () => {
    const created = local({
      uuid: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
      id: null,
      dirty: true,
      name: "New",
    });
    const merged = mergeSnapshot([created], [remote()]);
    expect(merged.map((p) => p.uuid).sort()).toEqual([
      "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
      "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
    ]);
  });

  it("drops a clean local playlist that the server no longer has", () => {
    const merged = mergeSnapshot([local()], []);
    expect(merged).toEqual([]);
  });

  it("keeps a tombstone until the server acknowledges the delete", () => {
    const tombstone = local({ dirty: true, deleted: true });
    const merged = mergeSnapshot([tombstone], []);
    expect(merged).toEqual([tombstone]);
  });
});

describe("parseDirtyPlaylists", () => {
  it("accepts a well-formed upsert and a tombstone", () => {
    expect(
      parseDirtyPlaylists({
        playlists: [
          {
            uuid: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            name: "Road",
            updatedAt: 1,
            trackIds: [1, 2],
          },
          {
            uuid: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
            name: "Gone",
            updatedAt: 2,
            deleted: true,
          },
        ],
      }),
    ).toEqual([
      {
        uuid: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        name: "Road",
        updatedAt: 1,
        trackIds: [1, 2],
      },
      {
        uuid: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
        name: "Gone",
        updatedAt: 2,
        trackIds: [],
        deleted: true,
      },
    ]);
  });

  it("rejects a missing name on a live playlist and a bad uuid", () => {
    expect(
      parseDirtyPlaylists({
        playlists: [
          { uuid: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", name: "  ", updatedAt: 1, trackIds: [] },
        ],
      }),
    ).toBeNull();
    expect(
      parseDirtyPlaylists({
        playlists: [{ uuid: "nope", name: "A", updatedAt: 1, trackIds: [] }],
      }),
    ).toBeNull();
  });
});

describe("parseSnapshots", () => {
  it("accepts a snapshot and rejects a malformed track", () => {
    expect(
      parseSnapshots({
        playlists: [
          {
            uuid: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            id: 4,
            name: "A",
            updatedAt: 1,
            tracks: [TRACK],
          },
        ],
      }),
    ).toHaveLength(1);
    expect(
      parseSnapshots({
        playlists: [
          {
            uuid: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
            id: 4,
            name: "A",
            updatedAt: 1,
            tracks: [{ id: 1, title: "x" }],
          },
        ],
      }),
    ).toBeNull();
  });
});

describe("toDirty / visible / find", () => {
  it("serialises a tombstone without its tracks", () => {
    expect(toDirty(local({ deleted: true, dirty: true }))).toEqual({
      uuid: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
      name: "Mine",
      updatedAt: 100,
      trackIds: [],
      deleted: true,
    });
  });

  it("hides tombstones from the UI and finds by uuid or numeric id", () => {
    const docs = [local(), local({ uuid: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", deleted: true })];
    expect(visiblePlaylists(docs)).toHaveLength(1);
    expect(findPlaylist(docs, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa")?.name).toBe("Mine");
    expect(findPlaylist(docs, "1")?.name).toBe("Mine");
    expect(findPlaylist(docs, "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb")).toBeUndefined();
  });
});

import { describe, expect, it } from "vitest";
import { albumFromPath, artistFromPath } from "./metadata";

describe("albumFromPath", () => {
  it("uses the containing folder", () => {
    expect(albumFromPath("/music/Radiohead/In Rainbows/01 15 Step.flac")).toBe("In Rainbows");
  });

  it("climbs out of a disc subfolder so both discs land in one album", () => {
    const disc1 = albumFromPath("/music/Pink Floyd/The Wall/CD1/01 In the Flesh.flac");
    const disc2 = albumFromPath("/music/Pink Floyd/The Wall/CD 2/01 Hey You.flac");
    expect(disc1).toBe("The Wall");
    expect(disc2).toBe(disc1);
  });

  it.each(["Disc 2", "disk_3", "cd-10", "CD2"])("treats %s as a disc folder", (folder) => {
    expect(albumFromPath(`/music/Artist/Album/${folder}/track.mp3`)).toBe("Album");
  });

  it("keeps a folder that merely starts with a disc-like word", () => {
    expect(albumFromPath("/music/Artist/Discovery/01 One More Time.mp3")).toBe("Discovery");
  });
});

describe("artistFromPath", () => {
  const root = "/music";

  it("takes the folder above the album", () => {
    expect(artistFromPath("/music/System Of A Down/Hypnotize/01 Attack.mp3", root)).toBe(
      "System Of A Down",
    );
  });

  it("agrees for a partly tagged album, so it cannot split in two", () => {
    const tagged = "/music/System Of A Down/Mezmerize/01 Soldier Side.mp3";
    const untagged = "/music/System Of A Down/Mezmerize/02 B.Y.O.B.mp3";
    expect(artistFromPath(untagged, root)).toBe(artistFromPath(tagged, root));
  });

  it("climbs past a disc subfolder", () => {
    expect(artistFromPath("/music/Pink Floyd/The Wall/CD2/01 Hey You.flac", root)).toBe(
      "Pink Floyd",
    );
  });

  it("gives up at the library root rather than naming an artist after it", () => {
    expect(artistFromPath("/music/Some Album/01 Track.mp3", root)).toBeUndefined();
    expect(artistFromPath("/music/01 Loose Track.mp3", root)).toBeUndefined();
  });

  it("ignores a root that only looks like a prefix of the path", () => {
    expect(artistFromPath("/music-backup/Artist/Album/01.mp3", root)).toBeUndefined();
  });
});

import { describe, expect, it } from "vitest";
import { albumFromPath } from "./metadata";

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

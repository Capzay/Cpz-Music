import { describe, expect, it } from "vitest";
import { requestOrigin } from "./origin";

const origin = (h: Record<string, string>) => requestOrigin(new Headers(h));

describe("requestOrigin", () => {
  it("prefers the forwarded host over the one the server bound to", () => {
    expect(
      origin({ "x-forwarded-host": "music.example.com", "x-forwarded-proto": "https", host: "0.0.0.0:3000" }),
    ).toBe("https://music.example.com");
  });

  it("falls back to the Host header when nothing is proxying", () => {
    expect(origin({ host: "192.168.1.5:3000" })).toBe("http://192.168.1.5:3000");
  });

  it("does not invent an origin when both headers are missing", () => {
    expect(origin({})).toBe("http://localhost:3000");
  });
});

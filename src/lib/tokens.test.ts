import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { signToken, verifyToken } from "./tokens";

beforeEach(() => {
  process.env.APP_SECRET = "0".repeat(64);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("signToken / verifyToken", () => {
  it("round-trips claims", () => {
    const token = signToken("obs", { scope: "overlay" }, 60);
    expect(verifyToken(token, "obs")).toMatchObject({ scope: "overlay", purpose: "obs" });
  });

  it("rejects a token signed for a different purpose", () => {
    const token = signToken("obs", {}, 60);
    expect(verifyToken(token, "jam-invite")).toBeNull();
  });

  it("rejects a tampered payload", () => {
    const token = signToken("jam-invite", { jamId: "jam-1" }, 60);
    const [body, sig] = token.split(".");
    const forged = Buffer.from(
      JSON.stringify({ jamId: "jam-2", purpose: "jam-invite", exp: 9e9 }),
      "utf8",
    ).toString("base64url");
    expect(verifyToken(`${forged}.${sig}`, "jam-invite")).toBeNull();
    expect(verifyToken(`${body}.${sig}`, "jam-invite")).not.toBeNull();
  });

  it("rejects a token signed with a different secret", () => {
    const token = signToken("obs", {}, 60);
    process.env.APP_SECRET = "1".repeat(64);
    expect(verifyToken(token, "obs")).toBeNull();
  });

  it("rejects an expired token", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
    const token = signToken("obs", {}, 60);
    vi.setSystemTime(new Date("2026-01-01T00:01:01Z"));
    expect(verifyToken(token, "obs")).toBeNull();
  });

  it("rejects malformed input rather than throwing", () => {
    expect(verifyToken(null, "obs")).toBeNull();
    expect(verifyToken("", "obs")).toBeNull();
    expect(verifyToken("nodot", "obs")).toBeNull();
    expect(verifyToken(".sig", "obs")).toBeNull();
    expect(verifyToken("not-base64.short", "obs")).toBeNull();
  });

  it("refuses to sign with a weak secret", () => {
    process.env.APP_SECRET = "tooshort";
    expect(() => signToken("obs", {}, 60)).toThrow(/at least 32/);
  });
});

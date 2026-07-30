import { afterEach, describe, expect, it } from "vitest";
import type { User } from "@supabase/supabase-js";
import { identityFromUser, isGuestAllowed } from "./auth";

function user(overrides: Partial<User> = {}): User {
  return {
    id: "user-uuid",
    app_metadata: {},
    user_metadata: {},
    aud: "authenticated",
    created_at: "",
    ...overrides,
  } as User;
}

function githubUser(providerId: string) {
  return user({
    identities: [
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      { provider: "github", identity_data: { provider_id: providerId } } as any,
    ],
  });
}

const originalOwner = process.env.OWNER_GITHUB_ID;
afterEach(() => {
  process.env.OWNER_GITHUB_ID = originalOwner;
});

describe("identityFromUser", () => {
  it("recognises the owner by GitHub id", () => {
    process.env.OWNER_GITHUB_ID = "12345";
    expect(identityFromUser(githubUser("12345"))).toEqual({
      role: "host",
      userId: "user-uuid",
    });
  });

  it("refuses a different GitHub account", () => {
    process.env.OWNER_GITHUB_ID = "12345";
    expect(identityFromUser(githubUser("99999")).role).toBe("none");
  });

  it("refuses everyone when no owner is configured, rather than falling open", () => {
    delete process.env.OWNER_GITHUB_ID;
    expect(identityFromUser(githubUser("12345")).role).toBe("none");
  });

  it("ignores a forged owner id in user_metadata when identities disagree", () => {
    process.env.OWNER_GITHUB_ID = "12345";
    const forged = user({
      identities: [
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        { provider: "github", identity_data: { provider_id: "99999" } } as any,
      ],
      user_metadata: { provider_id: "12345" },
    });
    expect(identityFromUser(forged).role).toBe("none");
  });

  it("reads guest claims from app_metadata", () => {
    const guest = user({
      app_metadata: { jam: { jamId: "jam-1", pid: "p-1", name: "Alex" } },
    });
    expect(identityFromUser(guest)).toEqual({
      role: "guest",
      userId: "user-uuid",
      jamId: "jam-1",
      pid: "p-1",
      name: "Alex",
    });
  });

  it("ignores a jam claim that is missing required fields", () => {
    const guest = user({ app_metadata: { jam: { jamId: "jam-1" } } });
    expect(identityFromUser(guest).role).toBe("none");
  });

  it("treats a signed-out caller as nobody", () => {
    expect(identityFromUser(null).role).toBe("none");
  });
});

describe("isGuestAllowed", () => {
  it("permits reading the library", () => {
    expect(isGuestAllowed("GET", "/api/tracks")).toBe(true);
    expect(isGuestAllowed("GET", "/api/tracks/12/stream")).toBe(true);
    expect(isGuestAllowed("GET", "/api/artwork/5")).toBe(true);
  });

  it("permits only the three guest write actions", () => {
    expect(isGuestAllowed("POST", "/api/jam/add")).toBe(true);
    expect(isGuestAllowed("POST", "/api/jam/leave")).toBe(true);
    expect(isGuestAllowed("POST", "/api/playlists")).toBe(false);
  });

  it("denies anything not named, including new endpoints", () => {
    expect(isGuestAllowed("GET", "/api/stats")).toBe(false);
    expect(isGuestAllowed("GET", "/api/playlists")).toBe(false);
    expect(isGuestAllowed("DELETE", "/api/tracks/1")).toBe(false);
    expect(isGuestAllowed("PATCH", "/api/jam/add")).toBe(false);
    expect(isGuestAllowed("GET", "/api/some-future-endpoint")).toBe(false);
  });
});

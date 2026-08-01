import { NextResponse, type NextRequest } from "next/server";
import { getIdentity } from "@/lib/auth-server";
import { verifyToken } from "@/lib/tokens";
import type { PlayerTrack } from "@/lib/types";

/**
 * A read-only mirror of what the active device is playing, for the OBS overlay.
 *
 * The overlay cannot join the Realtime channel: an OBS browser source has its
 * own cookie jar and no way to complete an OAuth flow, and handing it the anon
 * key plus a session would give a scene file more access than it needs. So the
 * active device posts here and the overlay polls with a signed token that grants
 * exactly this one endpoint.
 *
 * ponytail: module-level, so it resets when the server restarts and does not
 * survive multiple instances. The overlay repopulates on the next state change
 * a second later. Move to Postgres only if this ever runs more than one process.
 */
let nowPlaying: {
  track: PlayerTrack | null;
  isPlaying: boolean;
  currentTime: number;
  updatedAt: number;
} = {
  track: null,
  isPlaying: false,
  currentTime: 0,
  updatedAt: 0,
};

export async function POST(request: NextRequest) {
  const identity = await getIdentity();
  if (identity.role !== "host") {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const body = await request.json().catch(() => null);
  if (!body || typeof body !== "object") {
    return NextResponse.json({ error: "Bad request" }, { status: 400 });
  }

  const { track, isPlaying, currentTime } = body as {
    track: PlayerTrack | null;
    isPlaying: boolean;
    currentTime?: number;
  };
  nowPlaying = {
    track: track ?? null,
    isPlaying: Boolean(isPlaying),
    currentTime: Number.isFinite(currentTime) ? Number(currentTime) : 0,
    updatedAt: Date.now(),
  };
  return new NextResponse(null, { status: 204 });
}

export async function GET(request: NextRequest) {
  const token = request.nextUrl.searchParams.get("token");
  const identity = await getIdentity();

  // Either the host reading it directly, or an overlay holding a valid token.
  if (identity.role !== "host" && !verifyToken(token, "obs")) {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  // The device posts only when something changes, so run the clock forward here
  // rather than making it post a position every second. The overlay then only
  // has to smooth between polls, and never has to trust its own clock against
  // the server's.
  const elapsed = nowPlaying.isPlaying ? (Date.now() - nowPlaying.updatedAt) / 1000 : 0;
  const duration = nowPlaying.track?.duration ?? 0;
  const currentTime = Math.min(nowPlaying.currentTime + elapsed, duration || Infinity);

  return NextResponse.json(
    { track: nowPlaying.track, isPlaying: nowPlaying.isPlaying, currentTime },
    { headers: { "Cache-Control": "no-store" } },
  );
}

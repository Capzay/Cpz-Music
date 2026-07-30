import { NextResponse, type NextRequest } from "next/server";
import { prisma } from "@/lib/db";
import { getIdentity } from "@/lib/auth-server";
import { listenDay } from "@/lib/listen-day";

/** A single track cannot plausibly contribute more than this in one report. */
const MAX_LISTEN_SECS = 6 * 60 * 60;

export async function POST(request: NextRequest) {
  const identity = await getIdentity();
  // Guests listen along but do not write to the owner's listening history.
  if (identity.role !== "host") {
    return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
  }

  const body = await request.json().catch(() => null);
  if (!body || typeof body !== "object") {
    return NextResponse.json({ error: "Bad request" }, { status: 400 });
  }

  const { trackId, durationSecs, at } = body as Record<string, unknown>;
  if (!Number.isInteger(trackId) || typeof durationSecs !== "number" || !(durationSecs >= 0)) {
    return NextResponse.json({ error: "Bad request" }, { status: 400 });
  }

  const seconds = Math.min(Math.round(durationSecs), MAX_LISTEN_SECS);
  const date = listenDay(at);

  try {
    await prisma.dailyListenStat.upsert({
      where: { trackId_date: { trackId: trackId as number, date } },
      create: { trackId: trackId as number, date, playCount: 1, listenSecs: seconds },
      update: { playCount: { increment: 1 }, listenSecs: { increment: seconds } },
    });
  } catch {
    // Track deleted since the listen was queued. Nothing to record, and the
    // client must not retry forever, so this is a permanent rejection.
    return NextResponse.json({ error: "Unknown track" }, { status: 409 });
  }

  return new NextResponse(null, { status: 204 });
}

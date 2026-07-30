import { NextResponse, type NextRequest } from "next/server";
import { prisma } from "@/lib/db";
import { getIdentity } from "@/lib/auth-server";
import { broadcastToPlayer, getParticipant } from "@/lib/jam";
import { clientKey, rateLimit } from "@/lib/rate-limit";
import { toPlayerTrack, trackSelect } from "@/lib/types";

/** How many tracks one guest may add per minute. */
const GUEST_ADD_LIMIT = 5;

export async function POST(request: NextRequest) {
  const identity = await getIdentity();
  if (identity.role !== "guest") {
    return NextResponse.json({ error: "Forbidden" }, { status: 403 });
  }

  // Keyed on the participant, not the address, so one guest cannot flood the
  // queue by switching networks, and guests behind one NAT do not share a quota.
  if (!rateLimit(`jam-add:${identity.pid}`, GUEST_ADD_LIMIT, 60_000)) {
    return NextResponse.json({ error: "Slow down" }, { status: 429 });
  }
  if (!rateLimit(`jam-add-ip:${clientKey(request)}`, 30, 60_000)) {
    return NextResponse.json({ error: "Slow down" }, { status: 429 });
  }

  // Being in a jam is not the same as being let into it.
  const participant = await getParticipant(identity.pid);
  if (!participant || participant.status !== "admitted") {
    return NextResponse.json({ error: "Not admitted" }, { status: 403 });
  }

  const body = await request.json().catch(() => null);
  const trackId = (body as { trackId?: unknown } | null)?.trackId;
  if (!Number.isInteger(trackId)) {
    return NextResponse.json({ error: "Bad request" }, { status: 400 });
  }

  const track = await prisma.track.findUnique({
    where: { id: trackId as number },
    select: trackSelect,
  });
  if (!track) return NextResponse.json({ error: "Unknown track" }, { status: 404 });

  await prisma.jamQueueAdd.create({
    data: { jamId: identity.jamId, pid: identity.pid, trackId: track.id },
  });

  await broadcastToPlayer("jam-add", {
    track: toPlayerTrack(track),
    addedBy: identity.name,
  });

  return new NextResponse(null, { status: 204 });
}

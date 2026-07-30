import "server-only";
import { prisma } from "@/lib/db";
import { signToken, verifyToken } from "@/lib/tokens";
import { SUPABASE_URL } from "@/lib/supabase/server";
import { PLAYER_CHANNEL } from "@/lib/realtime";

const INVITE_TTL_SECONDS = 30 * 24 * 60 * 60;

export interface InviteClaims {
  purpose: string;
  exp: number;
  jamId: string;
  epoch: number;
  [key: string]: unknown;
}

export function signInvite(jamId: string, epoch: number): string {
  return signToken("jam-invite", { jamId, epoch }, INVITE_TTL_SECONDS);
}

export function readInvite(token: string | null | undefined): InviteClaims | null {
  const claims = verifyToken<InviteClaims>(token, "jam-invite");
  if (!claims || typeof claims.jamId !== "string" || typeof claims.epoch !== "number") return null;
  return claims;
}

export function getActiveJam() {
  return prisma.jam.findFirst({ where: { active: true }, orderBy: { createdAt: "desc" } });
}

export async function createJam(hostName?: string) {
  // One active jam at a time; starting a new one ends the old.
  await prisma.jam.updateMany({
    where: { active: true },
    data: { active: false, endedAt: new Date() },
  });
  return prisma.jam.create({ data: { hostName: hostName || null } });
}

export function endJam(jamId: string) {
  return prisma.jam.update({
    where: { id: jamId },
    data: { active: false, endedAt: new Date() },
  });
}

/** Bumps the epoch, which invalidates every outstanding invite link at once. */
export function rotateInvite(jamId: string) {
  return prisma.jam.update({ where: { id: jamId }, data: { inviteEpoch: { increment: 1 } } });
}

/**
 * Attaches a Supabase user to a jam, creating the participant row on first use.
 *
 * Returns null when the jam is over or the link has been rotated. Existing
 * participants keep working after a rotation; only new joins are refused, which
 * is what makes "revoke the link" usable mid-party.
 */
export async function joinJam(claims: InviteClaims, userId: string, name: string) {
  const jam = await prisma.jam.findFirst({ where: { id: claims.jamId, active: true } });
  if (!jam) return null;

  const existing = await prisma.jamParticipant.findUnique({ where: { userId } });
  if (existing) {
    if (existing.jamId !== jam.id) return null;
    if (existing.status === "kicked") return existing;
    return prisma.jamParticipant.update({ where: { id: existing.id }, data: { name } });
  }

  if (claims.epoch !== jam.inviteEpoch) return null;

  const admitted = !jam.requireApproval;
  return prisma.jamParticipant.create({
    data: {
      jamId: jam.id,
      userId,
      name,
      status: admitted ? "admitted" : "pending",
      admittedAt: admitted ? new Date() : null,
    },
  });
}

export function getParticipant(pid: string) {
  return prisma.jamParticipant.findUnique({ where: { id: pid } });
}

export function listParticipants(jamId: string) {
  return prisma.jamParticipant.findMany({
    where: { jamId, status: { in: ["pending", "admitted"] } },
    orderBy: { joinedAt: "asc" },
  });
}

export async function setParticipantStatus(jamId: string, pid: string, status: string) {
  const participant = await prisma.jamParticipant.findUnique({ where: { id: pid } });
  if (!participant || participant.jamId !== jamId) return null;
  return prisma.jamParticipant.update({
    where: { id: pid },
    data: { status, admittedAt: status === "admitted" ? new Date() : participant.admittedAt },
  });
}

/**
 * Pushes an event onto the player channel from the server.
 *
 * Guests are barred from broadcasting by the Realtime policy, deliberately, so
 * a guest's queue addition reaches the host's active device through here using
 * the service role rather than from the guest's own browser.
 */
export async function broadcastToPlayer(event: string, payload: unknown) {
  const key = process.env.SUPABASE_SERVICE_ROLE_KEY;
  if (!key) return;

  await fetch(`${SUPABASE_URL}/realtime/v1/api/broadcast`, {
    method: "POST",
    headers: { "Content-Type": "application/json", apikey: key, Authorization: `Bearer ${key}` },
    body: JSON.stringify({
      messages: [{ topic: PLAYER_CHANNEL, event, payload, private: true }],
    }),
  }).catch(() => {
    // The host device polls the jam panel as well, so a dropped broadcast is
    // a delay rather than a lost track.
  });
}

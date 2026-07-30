"use server";

import { revalidatePath } from "next/cache";
import { prisma } from "@/lib/db";
import { requireHost } from "@/lib/auth-server";
import { createJam, endJam, getActiveJam, rotateInvite, setParticipantStatus } from "@/lib/jam";
import { supabaseAdmin } from "@/lib/supabase/server";

export async function startJam(formData: FormData) {
  await requireHost();
  await createJam(String(formData.get("hostName") ?? "").trim().slice(0, 40));
  revalidatePath("/settings");
}

export async function stopJam() {
  await requireHost();
  const jam = await getActiveJam();
  if (!jam) return;

  await endJam(jam.id);
  // Revoke every guest session's claims, so ending a jam actually locks guests
  // out rather than leaving valid JWTs floating around until they expire.
  await revokeGuests(jam.id);
  revalidatePath("/settings");
}

export async function newInviteLink() {
  await requireHost();
  const jam = await getActiveJam();
  if (jam) await rotateInvite(jam.id);
  revalidatePath("/settings");
}

export async function admitGuest(pid: string) {
  await requireHost();
  const jam = await getActiveJam();
  if (jam) await setParticipantStatus(jam.id, pid, "admitted");
  revalidatePath("/settings");
}

export async function kickGuest(pid: string) {
  await requireHost();
  const jam = await getActiveJam();
  if (!jam) return;

  await setParticipantStatus(jam.id, pid, "kicked");
  const participant = await prisma.jamParticipant.findUnique({ where: { id: pid } });
  if (participant?.userId) await clearClaims(participant.userId);
  revalidatePath("/settings");
}

async function revokeGuests(jamId: string) {
  const participants = await prisma.jamParticipant.findMany({
    where: { jamId, userId: { not: null } },
    select: { userId: true },
  });
  await Promise.all(
    participants.map((p) => (p.userId ? clearClaims(p.userId) : Promise.resolve())),
  );
}

async function clearClaims(userId: string) {
  const admin = supabaseAdmin();
  await admin.auth.admin
    .updateUserById(userId, { app_metadata: { jam: null } })
    .catch(() => {});
}

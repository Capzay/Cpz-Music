"use server";

import { revalidatePath } from "next/cache";
import { redirect } from "next/navigation";
import { prisma } from "@/lib/db";
import { requireHost } from "@/lib/auth-server";
import { moveItem } from "@/lib/reorder";

/**
 * Server Actions are reachable by anyone who can guess the action id, so each
 * one re-establishes that the caller is the host. The proxy's check is an
 * optimisation, not the authorisation.
 */

function cleanName(raw: FormDataEntryValue | null): string {
  const name = String(raw ?? "").trim().slice(0, 120);
  if (!name) throw new Error("Playlist name is required");
  return name;
}

export async function createPlaylist(formData: FormData) {
  await requireHost();
  const playlist = await prisma.playlist.create({ data: { name: cleanName(formData.get("name")) } });
  revalidatePath("/playlists");
  redirect(`/playlists/${playlist.id}`);
}

export async function renamePlaylist(id: number, formData: FormData) {
  await requireHost();
  await prisma.playlist.update({ where: { id }, data: { name: cleanName(formData.get("name")) } });
  revalidatePath(`/playlists/${id}`);
  revalidatePath("/playlists");
}

export async function deletePlaylist(id: number) {
  await requireHost();
  await prisma.playlist.delete({ where: { id } });
  revalidatePath("/playlists");
  redirect("/playlists");
}

export async function addTracksToPlaylist(playlistId: number, trackIds: number[]) {
  await requireHost();
  if (trackIds.length === 0) return;

  await prisma.$transaction(async (tx) => {
    const last = await tx.playlistTrack.findFirst({
      where: { playlistId },
      orderBy: { position: "desc" },
      select: { position: true },
    });
    let position = last ? last.position + 1 : 0;
    await tx.playlistTrack.createMany({
      data: trackIds.map((trackId) => ({ playlistId, trackId, position: position++ })),
    });
  });

  revalidatePath(`/playlists/${playlistId}`);
  revalidatePath("/playlists");
}

/**
 * Writes a contiguous 0..n-1 ordering.
 *
 * Two phases, because (playlistId, position) is unique: moving a track upwards
 * would otherwise collide with a row that has not been moved out of the way
 * yet. The first pass parks every row at a negative position, which nothing
 * else can occupy, and the second pass lands them.
 */
async function renumber(playlistId: number, orderedIds: number[]) {
  await prisma.$transaction(async (tx) => {
    for (let i = 0; i < orderedIds.length; i++) {
      await tx.playlistTrack.update({
        where: { id: orderedIds[i] },
        data: { position: -(i + 1) },
      });
    }
    for (let i = 0; i < orderedIds.length; i++) {
      await tx.playlistTrack.update({ where: { id: orderedIds[i] }, data: { position: i } });
    }
  });
}

export async function removeFromPlaylist(playlistId: number, playlistTrackId: number) {
  await requireHost();

  await prisma.playlistTrack.delete({ where: { id: playlistTrackId } });
  const remaining = await prisma.playlistTrack.findMany({
    where: { playlistId },
    orderBy: { position: "asc" },
    select: { id: true },
  });
  await renumber(playlistId, remaining.map((r) => r.id));

  revalidatePath(`/playlists/${playlistId}`);
  revalidatePath("/playlists");
}

export async function movePlaylistTrack(playlistId: number, from: number, to: number) {
  await requireHost();

  const rows = await prisma.playlistTrack.findMany({
    where: { playlistId },
    orderBy: { position: "asc" },
    select: { id: true },
  });
  await renumber(playlistId, moveItem(rows.map((r) => r.id), from, to));

  revalidatePath(`/playlists/${playlistId}`);
}

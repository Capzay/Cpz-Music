import { NextResponse, type NextRequest } from "next/server";
import { prisma } from "@/lib/db";
import { getIdentity } from "@/lib/auth-server";
import { toPlayerTrack, trackSelect } from "@/lib/types";
import {
  decideWrite,
  filterKnownTrackIds,
  parseDirtyPlaylists,
  type DirtyPlaylist,
} from "@/lib/playlist-sync";

/**
 * Host-only snapshot of every playlist, plus a last-write-wins upsert of docs
 * the device edited while offline (or while the previous POST was in flight).
 */

export const dynamic = "force-dynamic";

const playlistSelect = {
  id: true,
  uuid: true,
  name: true,
  updatedAt: true,
  tracks: {
    orderBy: { position: "asc" as const },
    select: { track: { select: trackSelect } },
  },
};

async function snapshot() {
  const rows = await prisma.playlist.findMany({
    orderBy: { updatedAt: "desc" },
    select: playlistSelect,
  });
  return {
    playlists: rows.map((row) => ({
      uuid: row.uuid,
      id: row.id,
      name: row.name,
      updatedAt: row.updatedAt.getTime(),
      tracks: row.tracks.map((entry) => toPlayerTrack(entry.track)),
    })),
  };
}

function unauthorized() {
  return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
}

export async function GET() {
  const identity = await getIdentity();
  if (identity.role !== "host") return unauthorized();
  return NextResponse.json(await snapshot(), { headers: { "Cache-Control": "no-store" } });
}

export async function POST(request: NextRequest) {
  const identity = await getIdentity();
  if (identity.role !== "host") return unauthorized();

  const dirty = parseDirtyPlaylists(await request.json().catch(() => null));
  if (!dirty) {
    return NextResponse.json({ error: "Bad request" }, { status: 400 });
  }

  for (const doc of dirty) {
    await applyDoc(doc);
  }

  return NextResponse.json(await snapshot(), { headers: { "Cache-Control": "no-store" } });
}

async function applyDoc(doc: DirtyPlaylist) {
  const existing = await prisma.playlist.findUnique({
    where: { uuid: doc.uuid },
    select: { id: true, updatedAt: true },
  });
  const decision = decideWrite(
    doc.updatedAt,
    doc.deleted === true,
    existing ? existing.updatedAt.getTime() : null,
  );

  if (decision === "skip") return;

  if (decision === "delete" && existing) {
    await prisma.playlist.delete({ where: { id: existing.id } });
    return;
  }

  const uniqueIds = [...new Set(doc.trackIds)];
  const known = await prisma.track.findMany({
    where: { id: { in: uniqueIds } },
    select: { id: true },
  });
  const trackIds = filterKnownTrackIds(doc.trackIds, new Set(known.map((t) => t.id)));
  const updatedAt = new Date(doc.updatedAt);

  if (decision === "create") {
    await prisma.playlist.create({
      data: {
        uuid: doc.uuid,
        name: doc.name,
        updatedAt,
        tracks: {
          create: trackIds.map((trackId, position) => ({ trackId, position })),
        },
      },
    });
    return;
  }

  if (!existing) return;

  await prisma.$transaction(async (tx) => {
    await tx.playlist.update({
      where: { id: existing.id },
      data: { name: doc.name, updatedAt },
    });
    await tx.playlistTrack.deleteMany({ where: { playlistId: existing.id } });
    if (trackIds.length > 0) {
      await tx.playlistTrack.createMany({
        data: trackIds.map((trackId, position) => ({
          playlistId: existing.id,
          trackId,
          position,
        })),
      });
    }
  });
}

import { type NextRequest } from "next/server";
import { prisma } from "@/lib/db";
import { musicDir } from "@/lib/config";
import { fileResponse } from "@/lib/file-response";

// The proxy has already established that the caller is the host or an admitted
// jam guest, both of which may stream.
export async function GET(request: NextRequest, ctx: RouteContext<"/api/tracks/[id]/stream">) {
  const { id } = await ctx.params;
  const trackId = Number(id);
  if (!Number.isInteger(trackId)) return new Response("Not found", { status: 404 });

  const track = await prisma.track.findUnique({
    where: { id: trackId },
    select: { filePath: true, mimeType: true },
  });
  if (!track) return new Response("Not found", { status: 404 });

  return fileResponse(
    track.filePath,
    musicDir(),
    track.mimeType ?? "application/octet-stream",
    request.headers.get("range"),
    // Immutable: a changed file gets a new mtime and is rescanned, and the audio
    // element re-requests by track id anyway.
    { "Cache-Control": "private, max-age=31536000, immutable" },
  );
}

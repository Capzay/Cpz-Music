import { type NextRequest } from "next/server";
import { prisma } from "@/lib/db";
import { artworkDir } from "@/lib/config";
import { fileResponse } from "@/lib/file-response";

export async function GET(request: NextRequest, ctx: RouteContext<"/api/artwork/[albumId]">) {
  const { albumId } = await ctx.params;
  const id = Number(albumId);
  if (!Number.isInteger(id)) return new Response("Not found", { status: 404 });

  const album = await prisma.album.findUnique({
    where: { id },
    select: { artworkPath: true },
  });
  if (!album?.artworkPath) return new Response("Not found", { status: 404 });

  const contentType = album.artworkPath.endsWith(".png") ? "image/png" : "image/jpeg";

  return fileResponse(album.artworkPath, artworkDir(), contentType, request.headers.get("range"), {
    // Short: the URL is keyed by album id, so a rescan replaces the image
    // behind it. A week of browser cache outlived several rescans. Public
    // because the route is: Discord's proxy caches it on the way to the card.
    "Cache-Control": "public, max-age=86400",
  });
}

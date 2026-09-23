import { NextResponse, type NextRequest } from "next/server";
import { getIdentity } from "@/lib/auth-server";
import { searchLibrary, tracksForAlbum, tracksForArtist } from "@/lib/search";

export const dynamic = "force-dynamic";

function unauthorized() {
  return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
}

/**
 * Library search for the add-to-playlist picker.
 *
 *   GET /api/search?q=foo
 *   GET /api/search?album=12
 *   GET /api/search?artist=34
 */
export async function GET(request: NextRequest) {
  const identity = await getIdentity();
  if (identity.role !== "host") return unauthorized();

  const { searchParams } = request.nextUrl;
  const albumId = Number(searchParams.get("album"));
  if (searchParams.has("album")) {
    if (!Number.isInteger(albumId) || albumId <= 0) {
      return NextResponse.json({ error: "Bad request" }, { status: 400 });
    }
    return NextResponse.json(
      { tracks: await tracksForAlbum(albumId) },
      { headers: { "Cache-Control": "no-store" } },
    );
  }

  const artistId = Number(searchParams.get("artist"));
  if (searchParams.has("artist")) {
    if (!Number.isInteger(artistId) || artistId <= 0) {
      return NextResponse.json({ error: "Bad request" }, { status: 400 });
    }
    return NextResponse.json(
      { tracks: await tracksForArtist(artistId) },
      { headers: { "Cache-Control": "no-store" } },
    );
  }

  const q = (searchParams.get("q") ?? "").trim();
  if (!q) {
    return NextResponse.json(
      { tracks: [], albums: [], artists: [] },
      { headers: { "Cache-Control": "no-store" } },
    );
  }

  return NextResponse.json(await searchLibrary(q), {
    headers: { "Cache-Control": "no-store" },
  });
}

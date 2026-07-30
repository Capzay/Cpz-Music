import { NextResponse, type NextRequest } from "next/server";
import { supabaseServer, supabaseAdmin } from "@/lib/supabase/server";
import { joinJam, readInvite } from "@/lib/jam";
import { clientKey, rateLimit } from "@/lib/rate-limit";

/**
 * Redeems an invite link.
 *
 * The old build had an nginx gateway mint a guest cookie from this token. Here
 * the app does it itself: verify the signature, create a Supabase anonymous
 * session, then write the jam claims into `app_metadata`, which only the
 * service role can set. The guest ends up holding a real session whose scope is
 * baked into a signed JWT rather than a header a proxy promised to strip.
 */
export async function GET(request: NextRequest) {
  if (!rateLimit(`jam-join:${clientKey(request)}`, 10, 60_000)) {
    return NextResponse.redirect(new URL("/jam?error=rate_limited", request.nextUrl.origin));
  }

  const token = request.nextUrl.searchParams.get("token");
  const rawName = request.nextUrl.searchParams.get("name") ?? "";
  const name = rawName.trim().slice(0, 40) || "Guest";

  const claims = readInvite(token);
  if (!claims) {
    return NextResponse.redirect(new URL("/jam?error=invalid_invite", request.nextUrl.origin));
  }

  const supabase = await supabaseServer();

  // Reuse an existing session where there is one, so refreshing the invite link
  // does not strand the guest with a second anonymous account.
  const { data: current } = await supabase.auth.getUser();
  let userId = current.user?.id ?? null;

  if (!userId) {
    const { data, error } = await supabase.auth.signInAnonymously();
    if (error || !data.user) {
      return NextResponse.redirect(new URL("/jam?error=signin_failed", request.nextUrl.origin));
    }
    userId = data.user.id;
  }

  const participant = await joinJam(claims, userId, name);
  if (!participant) {
    return NextResponse.redirect(new URL("/jam?error=jam_closed", request.nextUrl.origin));
  }
  if (participant.status === "kicked") {
    return NextResponse.redirect(new URL("/jam?error=removed", request.nextUrl.origin));
  }

  const admin = supabaseAdmin();
  await admin.auth.admin.updateUserById(userId, {
    app_metadata: { jam: { jamId: participant.jamId, pid: participant.id, name } },
  });

  // The JWT in hand predates the metadata write, so mint a fresh one or the
  // guest's very next request would look like an ordinary anonymous user.
  await supabase.auth.refreshSession();

  return NextResponse.redirect(new URL("/jam", request.nextUrl.origin));
}

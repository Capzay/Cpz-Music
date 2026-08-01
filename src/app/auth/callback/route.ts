import { NextResponse, type NextRequest } from "next/server";
import type { User } from "@supabase/supabase-js";
import { supabaseServer, supabaseAdmin } from "@/lib/supabase/server";
import { identityFromUser } from "@/lib/auth";
import { requestOrigin } from "@/lib/origin";

export async function GET(request: NextRequest) {
  const { searchParams } = request.nextUrl;
  const code = searchParams.get("code");
  const next = searchParams.get("next");

  const origin = requestOrigin(request.headers);

  // Only same-origin relative paths, so ?next= cannot be used as an open redirect.
  const destination = next && next.startsWith("/") && !next.startsWith("//") ? next : "/";

  if (!code) {
    return NextResponse.redirect(`${origin}/login?error=missing_code`);
  }

  const supabase = await supabaseServer();
  const { data, error } = await supabase.auth.exchangeCodeForSession(code);
  if (error) {
    // The URL only ever carries a generic code; the reason goes to the server log,
    // where it is readable without handing it to whoever is at the browser.
    console.error("[auth] code exchange failed:", error.message);
    return NextResponse.redirect(`${origin}/login?error=exchange_failed`);
  }

  await stampOwner(supabase, data.user);

  return NextResponse.redirect(`${origin}${destination}`);
}

/**
 * Mirrors "this account is the owner" into `app_metadata`, where the Realtime
 * policies on the player channel can see it.
 *
 * Those policies run inside Postgres, which cannot read OWNER_GITHUB_ID, and
 * Supabase does not let the `postgres` role pin it as a database parameter. So
 * the comparison stays here, in the code that already owns every other
 * authorisation decision, and only its result crosses over. `app_metadata` is
 * writable by the service role alone, so the flag cannot be forged from a
 * browser the way a `user_metadata` field could.
 *
 * A non-owner is stamped false rather than skipped: an account that loses owner
 * status when OWNER_GITHUB_ID changes must not keep a stale true.
 */
async function stampOwner(
  supabase: Awaited<ReturnType<typeof supabaseServer>>,
  user: User | null,
) {
  if (!user) return;

  const isOwner = identityFromUser(user).role === "host";
  if (user.app_metadata?.owner === isOwner) return;

  const { error } = await supabaseAdmin()
    .auth.admin.updateUserById(user.id, { app_metadata: { owner: isOwner } });
  if (error) {
    console.error("[auth] could not stamp owner flag:", error.message);
    return;
  }

  // The JWT in hand predates the metadata write, so mint a fresh one or the
  // owner's devices would be refused the player channel until it expired.
  await supabase.auth.refreshSession();
}

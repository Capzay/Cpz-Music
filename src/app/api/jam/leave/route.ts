import { NextResponse } from "next/server";
import { getIdentity } from "@/lib/auth-server";
import { supabaseServer, supabaseAdmin } from "@/lib/supabase/server";

export async function POST() {
  const identity = await getIdentity();
  if (identity.role !== "guest") {
    return NextResponse.json({ error: "Forbidden" }, { status: 403 });
  }

  // Strip the claims as well as the session, so a stale JWT cached anywhere
  // stops carrying jam access the moment it is refreshed.
  const admin = supabaseAdmin();
  await admin.auth.admin.updateUserById(identity.userId, { app_metadata: { jam: null } });

  const supabase = await supabaseServer();
  await supabase.auth.signOut();

  return new NextResponse(null, { status: 204 });
}

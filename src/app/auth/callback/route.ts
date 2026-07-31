import { NextResponse, type NextRequest } from "next/server";
import { supabaseServer } from "@/lib/supabase/server";

export async function GET(request: NextRequest) {
  const { searchParams } = request.nextUrl;
  const code = searchParams.get("code");
  const next = searchParams.get("next");

  // Behind the tunnel, nextUrl.origin is the address the server bound to
  // (0.0.0.0:3000), not the address the browser used. Redirecting there sends
  // the user somewhere their cookies do not exist.
  const forwardedHost = request.headers.get("x-forwarded-host") ?? request.headers.get("host");
  const forwardedProto = request.headers.get("x-forwarded-proto");
  const origin = forwardedHost
    ? `${forwardedProto ?? request.nextUrl.protocol.replace(":", "")}://${forwardedHost}`
    : request.nextUrl.origin;

  // Only same-origin relative paths, so ?next= cannot be used as an open redirect.
  const destination = next && next.startsWith("/") && !next.startsWith("//") ? next : "/";

  if (!code) {
    return NextResponse.redirect(`${origin}/login?error=missing_code`);
  }

  const supabase = await supabaseServer();
  const { error } = await supabase.auth.exchangeCodeForSession(code);
  if (error) {
    // The URL only ever carries a generic code; the reason goes to the server log,
    // where it is readable without handing it to whoever is at the browser.
    console.error("[auth] code exchange failed:", error.message);
    return NextResponse.redirect(`${origin}/login?error=exchange_failed`);
  }

  return NextResponse.redirect(`${origin}${destination}`);
}

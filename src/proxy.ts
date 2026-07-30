import { createServerClient } from "@supabase/ssr";
import { NextResponse, type NextRequest } from "next/server";
import { identityFromUser, isGuestAllowed } from "@/lib/auth";

/** Reachable without a session. Everything else needs one. */
const PUBLIC_PATHS = ["/login", "/auth/callback", "/api/health", "/jam/join"];

function isPublic(pathname: string) {
  return PUBLIC_PATHS.some((p) => pathname === p || pathname.startsWith(p + "/"));
}

export async function proxy(request: NextRequest) {
  // Must be the object we ultimately return: Supabase writes refreshed auth
  // cookies onto it, and dropping them logs the user out on the next request.
  let response = NextResponse.next({ request });

  const supabase = createServerClient(
    process.env.NEXT_PUBLIC_SUPABASE_URL!,
    process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY!,
    {
      cookies: {
        getAll: () => request.cookies.getAll(),
        setAll: (items) => {
          for (const { name, value } of items) request.cookies.set(name, value);
          response = NextResponse.next({ request });
          for (const { name, value, options } of items) {
            response.cookies.set(name, value, options);
          }
        },
      },
    },
  );

  const { data } = await supabase.auth.getUser();
  const identity = identityFromUser(data?.user);
  const { pathname } = request.nextUrl;

  if (isPublic(pathname)) return response;

  if (identity.role === "none") {
    if (pathname.startsWith("/api/")) {
      return NextResponse.json({ error: "Unauthorized" }, { status: 401 });
    }
    const login = request.nextUrl.clone();
    login.pathname = "/login";
    login.search = "";
    return NextResponse.redirect(login);
  }

  if (identity.role === "guest") {
    const allowed =
      pathname.startsWith("/jam") || isGuestAllowed(request.method, pathname);
    if (!allowed) {
      if (pathname.startsWith("/api/")) {
        return NextResponse.json({ error: "Forbidden in jam guest mode" }, { status: 403 });
      }
      const jam = request.nextUrl.clone();
      jam.pathname = "/jam";
      jam.search = "";
      return NextResponse.redirect(jam);
    }
  }

  return response;
}

export const config = {
  // Skip Next's own asset routes; everything else is checked.
  matcher: ["/((?!_next/static|_next/image|favicon.ico|manifest.webmanifest|sw.js|icons/).*)"],
};

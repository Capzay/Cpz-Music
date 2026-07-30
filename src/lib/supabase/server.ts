// Importing this from a client component is a build error rather than a leaked
// service role key. next/headers would catch most cases; this catches the rest.
import "server-only";
import { createServerClient } from "@supabase/ssr";
import { createClient } from "@supabase/supabase-js";
import { cookies } from "next/headers";

export const SUPABASE_URL = process.env.NEXT_PUBLIC_SUPABASE_URL!;
const ANON_KEY = process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY!;

/** Request-scoped client that reads and refreshes the caller's session cookies. */
export async function supabaseServer() {
  const cookieStore = await cookies();
  return createServerClient(SUPABASE_URL, ANON_KEY, {
    cookies: {
      getAll: () => cookieStore.getAll(),
      setAll: (items) => {
        try {
          for (const { name, value, options } of items) {
            cookieStore.set(name, value, options);
          }
        } catch {
          // Called from a Server Component, where cookies are read-only. The proxy
          // refreshes them on the next request, so this is safe to swallow.
        }
      },
    },
  });
}

/**
 * Service-role client. Bypasses RLS and can write `app_metadata`, so it must never
 * be constructed in code that ships to the browser.
 */
export function supabaseAdmin() {
  const key = process.env.SUPABASE_SERVICE_ROLE_KEY;
  if (!key) throw new Error("SUPABASE_SERVICE_ROLE_KEY is not set");
  return createClient(SUPABASE_URL, key, {
    auth: { autoRefreshToken: false, persistSession: false },
  });
}

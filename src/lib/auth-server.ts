import "server-only";
import { identityFromUser, UnauthorizedError, type Identity } from "@/lib/auth";
import { supabaseServer } from "@/lib/supabase/server";

export async function getIdentity(): Promise<Identity> {
  const supabase = await supabaseServer();
  // getUser revalidates the token against the auth server. getSession only
  // decodes the cookie and is not safe to authorise on.
  const { data, error } = await supabase.auth.getUser();
  if (error) return { role: "none" };
  return identityFromUser(data?.user);
}

export async function requireHost(): Promise<{ userId: string }> {
  const identity = await getIdentity();
  if (identity.role !== "host") throw new UnauthorizedError();
  return { userId: identity.userId };
}

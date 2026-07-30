import type { User } from "@supabase/supabase-js";

/**
 * Every authorisation decision in the app is made here. There is no gateway
 * injecting identity headers, so nothing upstream can be spoofed and nothing
 * downstream has to be kept in sync with a second system.
 *
 * This module is deliberately pure: the proxy, route handlers and tests all
 * import it, and none of them should drag in request-scoped Next.js APIs.
 */

export type Identity =
  | { role: "host"; userId: string }
  | { role: "guest"; userId: string; jamId: string; pid: string; name: string }
  | { role: "none" };

/** GitHub's immutable numeric account id, however Supabase happens to expose it. */
function githubId(user: User): string | null {
  const gh = user.identities?.find((i) => i.provider === "github");
  const raw =
    gh?.identity_data?.provider_id ??
    gh?.id ??
    user.user_metadata?.provider_id ??
    user.user_metadata?.sub;
  return raw == null ? null : String(raw);
}

function isOwner(user: User): boolean {
  const owner = process.env.OWNER_GITHUB_ID;
  // Refuse rather than fall open. An unset owner id must not grant access.
  if (!owner) return false;
  return githubId(user) === owner;
}

/**
 * Guest claims live in `app_metadata`, which only the service role can write.
 * A guest cannot edit their own JWT to promote themselves or hop jams.
 */
function jamClaim(user: User) {
  const jam = user.app_metadata?.jam as
    | { jamId?: unknown; pid?: unknown; name?: unknown }
    | undefined;
  if (!jam) return null;
  const { jamId, pid, name } = jam;
  if (typeof jamId !== "string" || typeof pid !== "string") return null;
  return { jamId, pid, name: typeof name === "string" ? name : "Guest" };
}

export function identityFromUser(user: User | null | undefined): Identity {
  if (!user) return { role: "none" };
  if (isOwner(user)) return { role: "host", userId: user.id };
  const jam = jamClaim(user);
  if (jam) return { role: "guest", userId: user.id, ...jam };
  return { role: "none" };
}

export class UnauthorizedError extends Error {
  constructor() {
    super("Unauthorized");
    this.name = "UnauthorizedError";
  }
}

/**
 * Paths a jam guest may reach. Anything absent is denied, so a new endpoint is
 * host-only until somebody deliberately adds it here.
 */
export function isGuestAllowed(method: string, pathname: string): boolean {
  if (method === "GET" || method === "HEAD") {
    return pathname.startsWith("/api/artwork/") || pathname.startsWith("/api/tracks/");
  }
  if (method === "POST") {
    return pathname === "/api/jam/add" || pathname === "/api/jam/leave";
  }
  return false;
}

import dns from "node:dns";
import { PrismaPg } from "@prisma/adapter-pg";
import { PrismaClient } from "@/generated/prisma/client";

// Dual-stack hosts (including the Supabase pooler) often return IPv6 first.
// `pg` does not fall back, so a machine with no IPv6 route fails with
// ENETUNREACH instead of using the A record.
dns.setDefaultResultOrder("ipv4first");

function ipv4Lookup(
  hostname: string,
  options: dns.LookupOneOptions,
  callback: (err: NodeJS.ErrnoException | null, address: string, family: number) => void,
) {
  dns.lookup(hostname, { ...options, family: 4 }, callback);
}

function databaseHost(connectionString: string): string | undefined {
  try {
    return new URL(connectionString).hostname;
  } catch {
    return undefined;
  }
}

// Cached on globalThis so Next.js dev hot-reloads reuse one pool instead of
// opening a new one per reload.
const globalForPrisma = globalThis as unknown as { prisma?: PrismaClient };

function getClient(): PrismaClient {
  if (globalForPrisma.prisma) return globalForPrisma.prisma;
  const connectionString = process.env.DATABASE_URL;
  if (!connectionString) throw new Error("DATABASE_URL is not set");
  const host = databaseHost(connectionString);
  if (host?.startsWith("db.") && host.endsWith(".supabase.co")) {
    console.error(
      `[db] ${host} is IPv6-only. Set DATABASE_URL to the session pooler (Supabase → Connect → Session pooler, port 5432, user postgres.<project-ref>).`,
    );
  }
  globalForPrisma.prisma = new PrismaClient({
    // Prisma's PoolConfig typings omit `lookup`; node-pg still honours it.
    adapter: new PrismaPg({ connectionString, lookup: ipv4Lookup } as { connectionString: string }),
  });
  return globalForPrisma.prisma;
}

/**
 * Connects on first use, not on import. `next build` loads every route module to
 * collect page data, and it must not need database credentials to do that.
 */
export const prisma = new Proxy({} as PrismaClient, {
  get(_target, prop) {
    const client = getClient();
    return Reflect.get(client, prop, client);
  },
});

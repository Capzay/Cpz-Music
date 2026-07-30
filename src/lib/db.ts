import { PrismaPg } from "@prisma/adapter-pg";
import { PrismaClient } from "@/generated/prisma/client";

// Cached on globalThis so Next.js dev hot-reloads reuse one pool instead of
// opening a new one per reload.
const globalForPrisma = globalThis as unknown as { prisma?: PrismaClient };

function getClient(): PrismaClient {
  if (globalForPrisma.prisma) return globalForPrisma.prisma;
  const connectionString = process.env.DATABASE_URL;
  if (!connectionString) throw new Error("DATABASE_URL is not set");
  globalForPrisma.prisma = new PrismaClient({
    adapter: new PrismaPg({ connectionString }),
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

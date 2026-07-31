import { redirect } from "next/navigation";
import { getIdentity } from "@/lib/auth-server";
import { prisma } from "@/lib/db";
import { MobileNav, Sidebar } from "@/components/Nav";
import { PlayerBar } from "@/components/Player/PlayerBar";
import { ServiceWorker } from "@/components/ServiceWorker";

export default async function LibraryLayout({ children }: { children: React.ReactNode }) {
  // The proxy already turned anonymous callers away. Repeating the check here
  // means a proxy matcher mistake cannot silently expose the library.
  const identity = await getIdentity();
  if (identity.role !== "host") redirect("/login");

  const playlists = await prisma.playlist.findMany({
    orderBy: { updatedAt: "desc" },
    select: { id: true, name: true },
  });

  return (
    <div className="flex h-dvh flex-col overflow-hidden">
      <ServiceWorker />
      <div className="flex flex-1 overflow-hidden">
        <Sidebar playlists={playlists} />
        <main className="flex-1 overflow-y-auto px-3 py-4 pb-32 md:px-6 md:py-6 md:pb-24">
          {children}
        </main>
      </div>
      {/* Player sits above the mobile nav, both pinned to the bottom. */}
      <div className="fixed bottom-0 left-0 right-0 z-50">
        <PlayerBar />
        <MobileNav />
      </div>
    </div>
  );
}

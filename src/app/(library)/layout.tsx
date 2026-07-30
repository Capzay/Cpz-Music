import { redirect } from "next/navigation";
import { getIdentity } from "@/lib/auth-server";
import { MobileNav, Sidebar } from "@/components/Nav";
import { PlayerBar } from "@/components/Player/PlayerBar";
import { ServiceWorker } from "@/components/ServiceWorker";

export default async function LibraryLayout({ children }: { children: React.ReactNode }) {
  // The proxy already turned anonymous callers away. Repeating the check here
  // means a proxy matcher mistake cannot silently expose the library.
  const identity = await getIdentity();
  if (identity.role !== "host") redirect("/login");

  return (
    <div className="flex min-h-dvh flex-col">
      <ServiceWorker />
      <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <main className="flex-1 overflow-y-auto p-4 md:p-6">{children}</main>
      </div>
      <PlayerBar />
      <MobileNav />
    </div>
  );
}

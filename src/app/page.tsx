import { prisma } from "@/lib/db";
import { requireHost } from "@/lib/auth-server";

export default async function HomePage() {
  await requireHost();
  const trackCount = await prisma.track.count();

  return (
    <main className="p-6">
      <h1 className="text-2xl font-semibold tracking-tight">Cpz Music</h1>
      <p className="mt-2 text-sm text-neutral-400">
        {trackCount === 0
          ? "Library is empty. Point MUSIC_DIR at your music and restart."
          : `${trackCount.toLocaleString()} tracks in the library.`}
      </p>
    </main>
  );
}

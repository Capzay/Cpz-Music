import Link from "next/link";
import { prisma } from "@/lib/db";

export const metadata = { title: "Artists" };

export default async function ArtistsPage() {
  const artists = await prisma.artist.findMany({
    where: { tracks: { some: {} } },
    orderBy: { name: "asc" },
    select: { id: true, name: true, _count: { select: { tracks: true } } },
  });

  return (
    <>
      <h1 className="mb-5 text-xl font-semibold tracking-tight">Artists</h1>
      <ul className="divide-y divide-neutral-900">
        {artists.map((artist) => (
          <li key={artist.id}>
            <Link
              href={`/artists/${artist.id}`}
              className="flex items-center justify-between py-3 text-sm hover:text-white"
            >
              <span className="truncate">{artist.name}</span>
              <span className="shrink-0 tabular-nums text-neutral-600">
                {artist._count.tracks}
              </span>
            </Link>
          </li>
        ))}
      </ul>
    </>
  );
}

import Link from "next/link";
import { prisma } from "@/lib/db";
import { requireHost } from "@/lib/auth-server";

export const metadata = { title: "Stats" };

type Period = "week" | "month" | "year" | "all";
const PERIODS: Period[] = ["week", "month", "year", "all"];

function rangeFor(period: Period): { start: Date; end: Date } {
  const now = new Date();
  const endOfToday = new Date(
    Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() + 1),
  );

  switch (period) {
    case "week": {
      const start = new Date(endOfToday);
      start.setUTCDate(start.getUTCDate() - 7);
      return { start, end: endOfToday };
    }
    case "month":
      return {
        start: new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 1)),
        end: endOfToday,
      };
    case "year":
      return { start: new Date(Date.UTC(now.getUTCFullYear(), 0, 1)), end: endOfToday };
    case "all":
      return { start: new Date(0), end: endOfToday };
  }
}

function formatHours(seconds: number): string {
  const hours = seconds / 3600;
  if (hours >= 10) return `${Math.round(hours)} h`;
  if (hours >= 1) return `${hours.toFixed(1)} h`;
  return `${Math.round(seconds / 60)} min`;
}

export default async function StatsPage(props: PageProps<"/stats">) {
  await requireHost();

  const { period: raw } = await props.searchParams;
  const value = Array.isArray(raw) ? raw[0] : raw;
  const period: Period = PERIODS.includes(value as Period) ? (value as Period) : "year";
  const { start, end } = rangeFor(period);
  const where = { date: { gte: start, lt: end } };

  const [totals, uniqueTracks, topTrackStats, topArtists] = await Promise.all([
    prisma.dailyListenStat.aggregate({ where, _sum: { listenSecs: true, playCount: true } }),
    prisma.dailyListenStat.groupBy({ by: ["trackId"], where }),
    prisma.dailyListenStat.groupBy({
      by: ["trackId"],
      where,
      _sum: { listenSecs: true, playCount: true },
      orderBy: { _sum: { listenSecs: "desc" } },
      take: 10,
    }),
    // groupBy cannot join, and artist totals need one. The old build reused this
    // query's LIMIT 10 as the unique-artist count, which was simply wrong.
    prisma.$queryRaw<{ artistId: number; artistName: string; listenSecs: number }[]>`
      SELECT a.id AS "artistId", a.name AS "artistName",
             CAST(SUM(s."listenSecs") AS INTEGER) AS "listenSecs"
      FROM "DailyListenStat" s
      JOIN "Track" t ON s."trackId" = t.id
      JOIN "Artist" a ON t."artistId" = a.id
      WHERE s.date >= ${start} AND s.date < ${end}
      GROUP BY a.id, a.name
      ORDER BY SUM(s."listenSecs") DESC
      LIMIT 10
    `,
  ]);

  const [{ count: uniqueArtists }] = await prisma.$queryRaw<{ count: number }[]>`
    SELECT CAST(COUNT(DISTINCT t."artistId") AS INTEGER) AS count
    FROM "DailyListenStat" s
    JOIN "Track" t ON s."trackId" = t.id
    WHERE s.date >= ${start} AND s.date < ${end}
  `;

  const tracks = await prisma.track.findMany({
    where: { id: { in: topTrackStats.map((s) => s.trackId) } },
    select: {
      id: true,
      title: true,
      album: { select: { id: true } },
      artist: { select: { name: true } },
    },
  });
  const byId = new Map(tracks.map((t) => [t.id, t]));

  const totalSecs = totals._sum.listenSecs ?? 0;
  const maxSecs = topTrackStats[0]?._sum.listenSecs ?? 0;

  return (
    <>
      <div className="mb-5 flex flex-wrap items-center gap-3">
        <h1 className="text-xl font-semibold tracking-tight">Stats</h1>
        <nav className="flex gap-1">
          {PERIODS.map((p) => (
            <Link
              key={p}
              href={`/stats?period=${p}`}
              className={`rounded-full px-3 py-1 text-xs capitalize transition ${
                p === period ? "bg-neutral-800 text-neutral-100" : "text-neutral-500 hover:text-neutral-300"
              }`}
            >
              {p}
            </Link>
          ))}
        </nav>
      </div>

      <dl className="mb-8 grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Stat label="Listening time" value={formatHours(totalSecs)} />
        <Stat label="Plays" value={(totals._sum.playCount ?? 0).toLocaleString()} />
        <Stat label="Tracks" value={uniqueTracks.length.toLocaleString()} />
        <Stat label="Artists" value={(uniqueArtists ?? 0).toLocaleString()} />
      </dl>

      {totalSecs === 0 ? (
        <p className="text-sm text-neutral-500">Nothing listened to in this period yet.</p>
      ) : (
        <div className="grid gap-8 lg:grid-cols-2">
          <section>
            <h2 className="mb-3 text-sm font-medium text-neutral-400">Top tracks</h2>
            <ol className="space-y-2">
              {topTrackStats.map((stat) => {
                const track = byId.get(stat.trackId);
                if (!track) return null;
                const secs = stat._sum.listenSecs ?? 0;
                return (
                  <li key={stat.trackId} className="text-sm">
                    <div className="flex items-baseline justify-between gap-3">
                      <Link href={`/albums/${track.album.id}`} className="min-w-0 truncate hover:underline">
                        {track.title}
                        <span className="text-neutral-500"> · {track.artist.name}</span>
                      </Link>
                      <span className="shrink-0 text-xs tabular-nums text-neutral-500">
                        {formatHours(secs)}
                      </span>
                    </div>
                    <Bar value={secs} max={maxSecs} />
                  </li>
                );
              })}
            </ol>
          </section>

          <section>
            <h2 className="mb-3 text-sm font-medium text-neutral-400">Top artists</h2>
            <ol className="space-y-2">
              {topArtists.map((artist) => (
                <li key={artist.artistId} className="text-sm">
                  <div className="flex items-baseline justify-between gap-3">
                    <Link
                      href={`/artists/${artist.artistId}`}
                      className="min-w-0 truncate hover:underline"
                    >
                      {artist.artistName}
                    </Link>
                    <span className="shrink-0 text-xs tabular-nums text-neutral-500">
                      {formatHours(artist.listenSecs)}
                    </span>
                  </div>
                  <Bar value={artist.listenSecs} max={topArtists[0]?.listenSecs ?? 0} />
                </li>
              ))}
            </ol>
          </section>
        </div>
      )}
    </>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded-lg border border-neutral-900 p-3">
      <dt className="text-xs text-neutral-500">{label}</dt>
      <dd className="mt-1 text-lg font-semibold tabular-nums">{value}</dd>
    </div>
  );
}

function Bar({ value, max }: { value: number; max: number }) {
  const pct = max > 0 ? Math.max(2, Math.round((value / max) * 100)) : 0;
  return (
    <div className="mt-1 h-1 w-full rounded-full bg-neutral-900">
      <div className="h-full rounded-full bg-neutral-600" style={{ width: `${pct}%` }} />
    </div>
  );
}

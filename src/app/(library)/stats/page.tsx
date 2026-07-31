import { Clock, Mic2, Music, Play } from "lucide-react";
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
      album: { select: { id: true, artworkPath: true } },
      artist: { select: { name: true } },
    },
  });
  const byId = new Map(tracks.map((t) => [t.id, t]));

  const totalSecs = totals._sum.listenSecs ?? 0;

  return (
    <>
      <h1 className="text-xl font-bold mb-4 md:text-2xl md:mb-6">Stats</h1>

      <div className="flex gap-2 mb-5">
        {PERIODS.map((p) => (
          <Link
            key={p}
            href={`/stats?period=${p}`}
            className={`px-4 py-1.5 rounded-full text-sm font-medium capitalize transition-colors ${
              p === period
                ? "bg-violet-600 text-white"
                : "bg-zinc-800 text-zinc-400 hover:text-white"
            }`}
          >
            {p}
          </Link>
        ))}
      </div>

      <div className="grid grid-cols-2 md:grid-cols-4 gap-3 mb-6">
        <StatCard icon={<Clock size={16} />} label="Listening Time" value={formatHours(totalSecs)} />
        <StatCard
          icon={<Play size={16} />}
          label="Total Plays"
          value={(totals._sum.playCount ?? 0).toLocaleString()}
        />
        <StatCard
          icon={<Music size={16} />}
          label="Unique Tracks"
          value={uniqueTracks.length.toLocaleString()}
        />
        <StatCard
          icon={<Mic2 size={16} />}
          label="Artists"
          value={(uniqueArtists ?? 0).toLocaleString()}
        />
      </div>

      {totalSecs === 0 ? (
        <div className="text-center py-16 text-zinc-500">
          <Music size={40} className="mx-auto mb-3 opacity-30" />
          <p>No listening data for this period yet.</p>
          <p className="text-sm mt-1">Start playing music to track your stats.</p>
        </div>
      ) : (
        <>
          {topTrackStats.length > 0 && (
            <section className="mb-6">
              <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wider mb-3">
                Top Tracks
              </h2>
              <div className="bg-zinc-900 rounded-xl overflow-hidden">
                {topTrackStats.map((stat, i) => {
                  const track = byId.get(stat.trackId);
                  if (!track) return null;
                  return (
                    <Link
                      key={stat.trackId}
                      href={`/albums/${track.album.id}`}
                      className="flex items-center gap-3 px-4 py-3 border-b border-zinc-800 last:border-0 hover:bg-zinc-800/50 transition-colors"
                    >
                      <span className="text-zinc-600 font-mono text-sm w-5 shrink-0 text-right">
                        {i + 1}
                      </span>
                      <div className="w-9 h-9 rounded bg-zinc-800 shrink-0 overflow-hidden">
                        {track.album.artworkPath ? (
                          // eslint-disable-next-line @next/next/no-img-element
                          <img
                            src={`/api/artwork/${track.album.id}`}
                            alt=""
                            className="w-full h-full object-cover"
                          />
                        ) : null}
                      </div>
                      <div className="flex-1 min-w-0">
                        <p className="text-sm font-medium truncate">{track.title}</p>
                        <p className="text-xs text-zinc-500 truncate">{track.artist.name}</p>
                      </div>
                      <div className="text-right shrink-0">
                        <p className="text-sm text-violet-400 font-medium">
                          {formatHours(stat._sum.listenSecs ?? 0)}
                        </p>
                        <p className="text-xs text-zinc-600">{stat._sum.playCount ?? 0} plays</p>
                      </div>
                    </Link>
                  );
                })}
              </div>
            </section>
          )}

          {topArtists.length > 0 && (
            <section>
              <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wider mb-3">
                Top Artists
              </h2>
              <div className="bg-zinc-900 rounded-xl overflow-hidden">
                {topArtists.map((artist, i) => (
                  <Link
                    key={artist.artistId}
                    href={`/artists/${artist.artistId}`}
                    className="flex items-center gap-3 px-4 py-3 border-b border-zinc-800 last:border-0 hover:bg-zinc-800/50 transition-colors"
                  >
                    <span className="text-zinc-600 font-mono text-sm w-5 shrink-0 text-right">
                      {i + 1}
                    </span>
                    <div className="w-9 h-9 rounded-full bg-violet-900/40 flex items-center justify-center shrink-0">
                      <Mic2 size={16} className="text-violet-400" />
                    </div>
                    <p className="flex-1 text-sm font-medium truncate">{artist.artistName}</p>
                    <p className="text-sm text-violet-400 font-medium shrink-0">
                      {formatHours(artist.listenSecs)}
                    </p>
                  </Link>
                ))}
              </div>
            </section>
          )}
        </>
      )}
    </>
  );
}

function StatCard({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
}) {
  return (
    <div className="bg-zinc-900 rounded-xl p-4">
      <div className="flex items-center gap-1.5 text-violet-400 mb-2">
        {icon}
        <span className="text-xs font-medium">{label}</span>
      </div>
      <p className="text-lg font-bold leading-tight">{value}</p>
    </div>
  );
}

/**
 * Resolves which UTC day a listen belongs to.
 *
 * Listens recorded offline are replayed later carrying their original
 * timestamp, so the client controls this value and it cannot be trusted: a
 * wrong clock (or a hostile caller) would otherwise write rows years into the
 * future and skew every chart.
 */
export function listenDay(at: unknown, now = Date.now()): Date {
  const FIVE_MINUTES = 5 * 60_000;
  const ONE_YEAR = 365 * 24 * 3_600_000;

  const claimed = typeof at === "number" && Number.isFinite(at) ? at : now;
  const when = claimed > now + FIVE_MINUTES || claimed < now - ONE_YEAR ? now : claimed;

  const day = new Date(when);
  day.setUTCHours(0, 0, 0, 0);
  return day;
}

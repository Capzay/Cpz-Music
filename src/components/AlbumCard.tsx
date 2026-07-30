import Link from "next/link";

export function AlbumCard({
  id,
  title,
  artist,
  year,
  hasArtwork,
}: {
  id: number;
  title: string;
  artist: string;
  year?: number | null;
  hasArtwork: boolean;
}) {
  return (
    <Link href={`/albums/${id}`} className="group block">
      <div className="aspect-square overflow-hidden rounded-md bg-neutral-900">
        {hasArtwork ? (
          // Plain img: artwork is served from local disk by a route handler, so
          // next/image's optimiser would only add a round trip.
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={`/api/artwork/${id}`}
            alt=""
            loading="lazy"
            className="h-full w-full object-cover transition group-hover:opacity-85"
          />
        ) : (
          <div className="flex h-full w-full items-center justify-center text-neutral-700">
            <svg viewBox="0 0 24 24" className="h-1/3 w-1/3" fill="currentColor" aria-hidden>
              <path d="M12 3v10.55A4 4 0 1 0 14 17V7h4V3h-6Z" />
            </svg>
          </div>
        )}
      </div>
      <p className="mt-2 truncate text-sm font-medium">{title}</p>
      <p className="truncate text-xs text-neutral-500">
        {artist}
        {year ? ` · ${year}` : ""}
      </p>
    </Link>
  );
}

import Link from "next/link";
import { Disc3 } from "lucide-react";

export function AlbumCard({
  id,
  title,
  artist,
  hasArtwork,
}: {
  id: number;
  title: string;
  artist: string;
  hasArtwork: boolean;
}) {
  return (
    <Link
      href={`/albums/${id}`}
      className="group block cursor-pointer hover:scale-105 transition-transform duration-200"
    >
      <div className="aspect-square bg-zinc-800 rounded-md overflow-hidden mb-2 flex items-center justify-center">
        {hasArtwork ? (
          // Plain img: artwork is served from local disk by a route handler, so
          // next/image's optimiser would only add a round trip.
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={`/api/artwork/${id}`}
            alt=""
            loading="lazy"
            className="w-full h-full object-cover"
          />
        ) : (
          <Disc3
            size={48}
            className="text-zinc-600 group-hover:text-zinc-500 transition-colors"
          />
        )}
      </div>
      <p className="font-medium text-sm truncate">{title}</p>
      <p className="text-xs text-zinc-400 truncate">{artist}</p>
    </Link>
  );
}

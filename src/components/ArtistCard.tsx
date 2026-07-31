import Link from "next/link";

export function ArtistCard({
  id,
  name,
  albums,
  tracks,
}: {
  id: number;
  name: string;
  albums?: number;
  tracks?: number;
}) {
  return (
    <Link
      href={`/artists/${id}`}
      className="block cursor-pointer hover:scale-105 transition-transform duration-200"
    >
      <div className="aspect-square bg-zinc-700 rounded-full flex items-center justify-center mb-2 text-3xl font-bold text-zinc-300">
        {name.charAt(0).toUpperCase()}
      </div>
      <p className="font-medium text-sm truncate text-center">{name}</p>
      {albums !== undefined && tracks !== undefined && (
        <p className="text-xs text-zinc-400 text-center">
          {albums} album{albums !== 1 ? "s" : ""} · {tracks} track{tracks !== 1 ? "s" : ""}
        </p>
      )}
    </Link>
  );
}

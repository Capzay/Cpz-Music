import path from "node:path";

function required(name: string): string {
  const value = process.env[name];
  if (!value) throw new Error(`Missing required environment variable: ${name}`);
  return value;
}

/** Absolute, symlink-free root of the music library. Read-only. */
export function musicDir(): string {
  return path.resolve(required("MUSIC_DIR"));
}

/** Where extracted album art is cached. Written to, unlike the music directory. */
export function artworkDir(): string {
  return path.resolve(process.env.ARTWORK_DIR ?? ".artwork");
}

import fsp from "node:fs/promises";
import path from "node:path";
import { parseFile } from "music-metadata";
import { isInside } from "@/lib/paths";

export interface TrackMetadata {
  title: string;
  artist: string;
  albumArtist: string;
  album: string;
  year?: number;
  trackNumber?: number;
  discNumber: number;
  duration?: number;
  bitrate?: number;
  sampleRate?: number;
  artwork?: Uint8Array;
  artworkMimeType?: string;
}

/**
 * The folder standing in for the album: the file's own, or its parent when the
 * file sits in a CD2-style disc subfolder, so both discs resolve alike.
 */
function albumDirOf(filePath: string): string {
  const dir = path.dirname(filePath);
  if (/^(cd|disc|disk)[\s._-]*\d+$/i.test(path.basename(dir))) {
    const parent = path.dirname(dir);
    if (path.basename(parent)) return parent;
  }
  return dir;
}

/**
 * Album name for a file with no ALBUM tag.
 *
 * "Unknown Album" is a terrible fallback here: albums are keyed by title and
 * album artist, so every untagged file by the same artist collapses into one
 * row holding tracks from unrelated records, wearing whichever cover happened
 * to be read first. The containing folder is nearly always the album.
 */
export function albumFromPath(filePath: string): string {
  return path.basename(albumDirOf(filePath)) || "Unknown Album";
}

/**
 * Artist for a file with no ARTIST tag, from the folder above the album.
 *
 * Same reasoning as the album fallback, and it matters more: an album whose
 * tracks are only partly tagged splits into two rows, one of them filed under
 * "Unknown Artist". Returns undefined once the walk reaches the library root,
 * whose name is a music folder rather than an artist.
 */
export function artistFromPath(filePath: string, root: string): string | undefined {
  const artistDir = path.dirname(albumDirOf(filePath));
  if (!isInside(root, artistDir)) return undefined;
  return path.basename(artistDir) || undefined;
}

const COVER_NAMES = new Set(["cover", "folder", "front", "album", "albumart", "artwork"]);
const COVER_TYPES: Record<string, string> = {
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".png": "image/png",
};

/**
 * Cover art sitting next to the tracks, for rips that carry no embedded
 * picture. Candidates are sorted so a folder holding more than one always
 * yields the same answer, however the filesystem happens to list it.
 */
async function folderCover(dir: string) {
  let entries;
  try {
    entries = await fsp.readdir(dir, { withFileTypes: true });
  } catch {
    return undefined;
  }

  const candidates = entries
    .filter((entry) => entry.isFile())
    .map((entry) => ({ name: entry.name, ext: path.extname(entry.name).toLowerCase() }))
    .filter(({ name, ext }) => COVER_TYPES[ext] && COVER_NAMES.has(path.basename(name, ext).toLowerCase()))
    .sort((a, b) => a.name.localeCompare(b.name));

  for (const { name, ext } of candidates) {
    try {
      return { data: await fsp.readFile(path.join(dir, name)), mimeType: COVER_TYPES[ext] };
    } catch {
      // Unreadable candidate; a later one may still work.
    }
  }
  return undefined;
}

export async function extractMetadata(filePath: string, root: string): Promise<TrackMetadata> {
  const { common, format } = await parseFile(filePath, { skipCovers: false });

  const embedded = common.picture?.[0];
  const artwork = embedded
    ? { data: embedded.data, mimeType: embedded.format }
    : await folderCover(albumDirOf(filePath));

  const folderArtist = artistFromPath(filePath, root);

  return {
    title: common.title || path.basename(filePath, path.extname(filePath)),
    artist: common.artist || folderArtist || "Unknown Artist",
    // Album artist matters for compilations, where every track has a different
    // artist but they all belong to one album.
    albumArtist: common.albumartist || common.artist || folderArtist || "Unknown Artist",
    album: common.album || albumFromPath(filePath),
    year: common.year,
    trackNumber: common.track?.no ?? undefined,
    discNumber: common.disk?.no ?? 1,
    duration: format.duration,
    bitrate: format.bitrate ? Math.round(format.bitrate / 1000) : undefined,
    sampleRate: format.sampleRate,
    artwork: artwork?.data,
    artworkMimeType: artwork?.mimeType,
  };
}

const MIME_BY_EXT: Record<string, string> = {
  ".mp3": "audio/mpeg",
  ".flac": "audio/flac",
  ".m4a": "audio/mp4",
  ".aac": "audio/aac",
  ".ogg": "audio/ogg",
  ".opus": "audio/ogg",
  ".wav": "audio/wav",
  ".wma": "audio/x-ms-wma",
  ".aiff": "audio/aiff",
};

export const SUPPORTED_EXTENSIONS = new Set(Object.keys(MIME_BY_EXT));

export function mimeForFile(filePath: string): string {
  return MIME_BY_EXT[path.extname(filePath).toLowerCase()] ?? "application/octet-stream";
}

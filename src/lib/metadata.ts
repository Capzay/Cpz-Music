import path from "node:path";
import { parseFile } from "music-metadata";

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
 * Album name for a file with no ALBUM tag.
 *
 * "Unknown Album" is a terrible fallback here: albums are keyed by title and
 * album artist, so every untagged file by the same artist collapses into one
 * row holding tracks from unrelated records, wearing whichever cover happened
 * to be read first. The containing folder is nearly always the album, and a
 * disc subfolder resolves to its parent so CD1 and CD2 stay together.
 */
export function albumFromPath(filePath: string): string {
  const dir = path.dirname(filePath);
  const name = path.basename(dir);
  if (/^(cd|disc|disk)[\s._-]*\d+$/i.test(name)) {
    const parent = path.basename(path.dirname(dir));
    if (parent) return parent;
  }
  return name || "Unknown Album";
}

export async function extractMetadata(filePath: string): Promise<TrackMetadata> {
  const { common, format } = await parseFile(filePath, { skipCovers: false });
  const artwork = common.picture?.[0];

  return {
    title: common.title || path.basename(filePath, path.extname(filePath)),
    artist: common.artist || "Unknown Artist",
    // Album artist matters for compilations, where every track has a different
    // artist but they all belong to one album.
    albumArtist: common.albumartist || common.artist || "Unknown Artist",
    album: common.album || albumFromPath(filePath),
    year: common.year,
    trackNumber: common.track?.no ?? undefined,
    discNumber: common.disk?.no ?? 1,
    duration: format.duration,
    bitrate: format.bitrate ? Math.round(format.bitrate / 1000) : undefined,
    sampleRate: format.sampleRate,
    artwork: artwork?.data,
    artworkMimeType: artwork?.format,
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

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

export async function extractMetadata(filePath: string): Promise<TrackMetadata> {
  const { common, format } = await parseFile(filePath, { skipCovers: false });
  const artwork = common.picture?.[0];

  return {
    title: common.title || path.basename(filePath, path.extname(filePath)),
    artist: common.artist || "Unknown Artist",
    // Album artist matters for compilations, where every track has a different
    // artist but they all belong to one album.
    albumArtist: common.albumartist || common.artist || "Unknown Artist",
    album: common.album || "Unknown Album",
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

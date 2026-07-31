import "server-only";
import fs from "node:fs";
import fsp from "node:fs/promises";
import path from "node:path";
import chokidar from "chokidar";
import { prisma } from "@/lib/db";
import { artworkDir, musicDir } from "@/lib/config";
import { SUPPORTED_EXTENSIONS, extractMetadata, mimeForFile } from "@/lib/metadata";

// ponytail: the scan runs in the web server's process, so a full rescan of a
// large library competes with request handling. Fine at personal scale. If it
// starts costing noticeable latency, move this module behind a worker_thread
// and have the route handlers trigger it over a message port.

async function saveArtwork(albumId: number, data: Uint8Array, mimeType: string) {
  const dir = artworkDir();
  const ext = mimeType.includes("png") ? "png" : "jpg";
  const dest = path.join(dir, `${albumId}.${ext}`);
  await fsp.mkdir(dir, { recursive: true });
  await fsp.writeFile(dest, data);
  return dest;
}

export async function processFile(filePath: string) {
  let stat: fs.Stats;
  try {
    stat = await fsp.stat(filePath);
  } catch {
    return; // Removed between discovery and processing.
  }

  const existing = await prisma.track.findUnique({
    where: { filePath },
    select: { mtime: true },
  });
  if (existing && existing.mtime === stat.mtimeMs) return;

  try {
    const meta = await extractMetadata(filePath, musicDir());

    const artist = await prisma.artist.upsert({
      where: { name: meta.artist },
      create: { name: meta.artist },
      update: {},
    });
    const albumArtist =
      meta.albumArtist === meta.artist
        ? artist
        : await prisma.artist.upsert({
            where: { name: meta.albumArtist },
            create: { name: meta.albumArtist },
            update: {},
          });

    const album = await prisma.album.upsert({
      where: { title_artistId: { title: meta.album, artistId: albumArtist.id } },
      create: { title: meta.album, artistId: albumArtist.id, year: meta.year },
      update: {},
    });

    // Also re-saves when the row points at a file that is no longer there, so a
    // cover lost to a cleared artwork directory comes back on the next scan.
    if (meta.artwork && (!album.artworkPath || !fs.existsSync(album.artworkPath))) {
      const artworkPath = await saveArtwork(
        album.id,
        meta.artwork,
        meta.artworkMimeType ?? "image/jpeg",
      );
      await prisma.album.update({ where: { id: album.id }, data: { artworkPath } });
    }

    const fields = {
      title: meta.title,
      artistId: artist.id,
      albumId: album.id,
      trackNumber: meta.trackNumber,
      discNumber: meta.discNumber,
      duration: meta.duration,
      fileSize: stat.size,
      mimeType: mimeForFile(filePath),
      bitrate: meta.bitrate,
      sampleRate: meta.sampleRate,
      mtime: stat.mtimeMs,
    };

    await prisma.track.upsert({
      where: { filePath },
      create: { filePath, ...fields },
      update: fields,
    });
  } catch (err) {
    console.error(`[scanner] failed to process ${filePath}:`, err);
  }
}

/**
 * `complete` is false when any directory could not be read. A partial listing
 * must never be treated as the truth about what is on disk: the caller deletes
 * everything it does not see, and that cascades into playlists and stats.
 */
async function walk(dir: string): Promise<{ files: string[]; complete: boolean }> {
  const files: string[] = [];
  let complete = true;
  let entries;
  try {
    entries = await fsp.readdir(dir, { withFileTypes: true });
  } catch (err) {
    console.error(`[scanner] cannot read ${dir}:`, err);
    return { files, complete: false };
  }
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      const sub = await walk(full);
      files.push(...sub.files);
      if (!sub.complete) complete = false;
    } else if (SUPPORTED_EXTENSIONS.has(path.extname(entry.name).toLowerCase())) {
      files.push(full);
    }
  }
  return { files, complete };
}

/**
 * Removes albums and artists left with nothing after tracks disappear, along
 * with the cover files of the albums going away. Leaving those behind fills the
 * artwork directory with images no album will ever claim again.
 */
async function pruneEmpty() {
  const orphans = await prisma.album.findMany({
    where: { tracks: { none: {} } },
    select: { id: true, artworkPath: true },
  });
  if (orphans.length > 0) {
    await prisma.album.deleteMany({ where: { id: { in: orphans.map((o) => o.id) } } });
    for (const { artworkPath } of orphans) {
      if (artworkPath) await fsp.rm(artworkPath, { force: true }).catch(() => {});
    }
  }
  await prisma.artist.deleteMany({
    where: { tracks: { none: {} }, albums: { none: {} } },
  });
}

export async function scanLibrary() {
  const root = musicDir();
  console.log(`[scanner] scanning ${root}`);
  const { files, complete } = await walk(root);
  console.log(`[scanner] found ${files.length} audio files`);

  for (let i = 0; i < files.length; i++) {
    await processFile(files[i]);
    if ((i + 1) % 100 === 0) console.log(`[scanner] ${i + 1}/${files.length}`);
  }

  const known = await prisma.track.findMany({ select: { id: true, filePath: true } });
  const onDisk = new Set(files);
  const stale = known.filter((t) => !onDisk.has(t.filePath)).map((t) => t.id);

  // Deleting a track takes its playlist entries and listening history with it,
  // so the two ways a scan can be wrong about the disk both stop here: a
  // directory that would not open, and a library that came back empty when the
  // database says it should not be. Both mean a broken mount or a bad
  // MUSIC_DIR far more often than they mean somebody deleted their music.
  const emptyButShouldNotBe = files.length === 0 && known.length > 0;
  if (!complete || emptyButShouldNotBe) {
    console.error(
      `[scanner] skipping cleanup: ${
        complete ? "the library scanned empty but the database holds tracks" : "part of the library could not be read"
      }. ${stale.length} tracks left untouched.`,
    );
    console.log("[scanner] scan complete");
    return;
  }

  if (stale.length > 0) {
    await prisma.track.deleteMany({ where: { id: { in: stale } } });
    console.log(`[scanner] removed ${stale.length} tracks no longer on disk`);
  }

  await pruneEmpty();
  console.log("[scanner] scan complete");
}

export function watchLibrary() {
  const root = musicDir();
  const timers = new Map<string, NodeJS.Timeout>();

  // Writes arrive in bursts (tag editors, file copies). Wait for quiet before
  // reading, or metadata gets parsed from a half-written file.
  const debounce = (key: string, fn: () => void) => {
    clearTimeout(timers.get(key));
    timers.set(
      key,
      setTimeout(() => {
        timers.delete(key);
        fn();
      }, 2000),
    );
  };

  const watcher = chokidar.watch(root, { ignoreInitial: true, persistent: true });

  watcher
    .on("add", (p) => debounce(p, () => void processFile(p)))
    .on("change", (p) => debounce(p, () => void processFile(p)))
    .on("unlink", (p) =>
      debounce(p, async () => {
        await prisma.track.deleteMany({ where: { filePath: p } });
        await pruneEmpty();
      }),
    )
    .on("error", (err) => console.error("[scanner] watch error:", err));

  console.log(`[scanner] watching ${root}`);
  return watcher;
}

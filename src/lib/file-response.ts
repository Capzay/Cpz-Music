import "server-only";
import fs from "node:fs";
import fsp from "node:fs/promises";
import { Readable } from "node:stream";
import { parseRange } from "@/lib/range";
import { isInside } from "@/lib/paths";

/**
 * Serves a file from disk with Range support.
 *
 * `root` is the only directory this will ever read from. Paths come from the
 * database rather than the URL, but the guard means a corrupted row or a future
 * caller still cannot turn this into an arbitrary file reader.
 */
export async function fileResponse(
  absolutePath: string,
  root: string,
  contentType: string,
  rangeHeader: string | null,
  extraHeaders: Record<string, string> = {},
): Promise<Response> {
  if (!isInside(root, absolutePath)) {
    console.error(`[files] refused path outside ${root}: ${absolutePath}`);
    return new Response("Not found", { status: 404 });
  }

  let stat;
  try {
    stat = await fsp.stat(absolutePath);
  } catch {
    return new Response("Not found", { status: 404 });
  }
  if (!stat.isFile()) return new Response("Not found", { status: 404 });

  const size = stat.size;
  const parsed = parseRange(rangeHeader, size);

  if (parsed.kind === "unsatisfiable") {
    return new Response(null, {
      status: 416,
      headers: { "Content-Range": `bytes */${size}`, "Accept-Ranges": "bytes" },
    });
  }

  const headers = new Headers({
    "Content-Type": contentType,
    "Accept-Ranges": "bytes",
    ...extraHeaders,
  });

  const options = parsed.kind === "range" ? { start: parsed.start, end: parsed.end } : {};
  const length = parsed.kind === "range" ? parsed.end - parsed.start + 1 : size;

  headers.set("Content-Length", String(length));
  if (parsed.kind === "range") {
    headers.set("Content-Range", `bytes ${parsed.start}-${parsed.end}/${size}`);
  }

  const stream = Readable.toWeb(
    fs.createReadStream(absolutePath, options),
  ) as ReadableStream<Uint8Array>;

  return new Response(stream, { status: parsed.kind === "range" ? 206 : 200, headers });
}

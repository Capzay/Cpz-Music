import path from "node:path";

/**
 * True when `child` resolves to something strictly inside `parent`.
 *
 * File paths reaching the stream and artwork handlers come from the database
 * rather than the URL, but a bad scan or a future caller must not be able to
 * turn this app into an arbitrary file reader.
 */
export function isInside(parent: string, child: string): boolean {
  const rel = path.relative(path.resolve(parent), path.resolve(child));
  return rel !== "" && !rel.startsWith("..") && !path.isAbsolute(rel);
}

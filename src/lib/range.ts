/**
 * Single-range HTTP Range parsing (RFC 9110 s14).
 *
 * Seeking in a long track and the offline downloader both depend on this, and a
 * bad byte window is the kind of thing that shows up as silent corruption
 * rather than an error, so malformed input is rejected explicitly.
 */
export type ParsedRange =
  | { kind: "range"; start: number; end: number }
  | { kind: "whole" }
  | { kind: "unsatisfiable" };

export function parseRange(header: string | null | undefined, size: number): ParsedRange {
  if (!header) return { kind: "whole" };

  const match = /^bytes=(.+)$/.exec(header.trim());
  if (!match) return { kind: "whole" };

  const specs = match[1].split(",");
  // Multi-range responses need multipart/byteranges. Nothing here asks for it,
  // and serving the whole file instead is a valid response.
  if (specs.length !== 1) return { kind: "whole" };

  const spec = specs[0].trim();
  const parts = /^(\d*)-(\d*)$/.exec(spec);
  if (!parts) return { kind: "whole" };

  const [, startStr, endStr] = parts;

  // An empty file can satisfy no range at all.
  if (size === 0) return { kind: "unsatisfiable" };

  if (startStr === "") {
    // Suffix form: "bytes=-500" means the final 500 bytes.
    if (endStr === "") return { kind: "whole" };
    const suffix = Number(endStr);
    if (suffix === 0) return { kind: "unsatisfiable" };
    const start = Math.max(0, size - suffix);
    return { kind: "range", start, end: size - 1 };
  }

  const start = Number(startStr);
  if (start >= size) return { kind: "unsatisfiable" };

  const end = endStr === "" ? size - 1 : Math.min(Number(endStr), size - 1);
  if (end < start) return { kind: "unsatisfiable" };

  return { kind: "range", start, end };
}

import crypto from "node:crypto";

/**
 * Small signed tokens for the two places a browser has to prove something
 * without a Supabase session: the OBS overlay (a browser source cannot complete
 * an OAuth flow) and jam invite links (handed to people with no account).
 *
 * Not a JWT library, because the entire requirement is "HMAC this object and
 * check it later". The `purpose` field is part of the signed payload, so an OBS
 * token can never be replayed as a jam invite.
 */

function secret(): Buffer {
  const value = process.env.APP_SECRET;
  if (!value || value.length < 32) {
    throw new Error("APP_SECRET must be set to at least 32 characters");
  }
  return Buffer.from(value, "utf8");
}

const b64url = (buf: Buffer) => buf.toString("base64url");

function hmac(body: string): string {
  return b64url(crypto.createHmac("sha256", secret()).update(body).digest());
}

export interface TokenPayload {
  purpose: string;
  /** Unix seconds. */
  exp: number;
  [key: string]: unknown;
}

export function signToken(
  purpose: string,
  claims: Record<string, unknown>,
  ttlSeconds: number,
): string {
  const payload: TokenPayload = {
    ...claims,
    purpose,
    exp: Math.floor(Date.now() / 1000) + ttlSeconds,
  };
  const body = b64url(Buffer.from(JSON.stringify(payload), "utf8"));
  return `${body}.${hmac(body)}`;
}

export function verifyToken<T extends TokenPayload = TokenPayload>(
  token: string | null | undefined,
  purpose: string,
): T | null {
  if (!token) return null;

  const dot = token.lastIndexOf(".");
  if (dot <= 0) return null;

  const body = token.slice(0, dot);
  const provided = token.slice(dot + 1);
  const expected = hmac(body);

  // Both are fixed-length base64url of a SHA-256 digest, but a caller can send
  // any length, and timingSafeEqual throws on a mismatch.
  const a = Buffer.from(provided);
  const b = Buffer.from(expected);
  if (a.length !== b.length || !crypto.timingSafeEqual(a, b)) return null;

  let payload: TokenPayload;
  try {
    payload = JSON.parse(Buffer.from(body, "base64url").toString("utf8"));
  } catch {
    return null;
  }

  if (payload.purpose !== purpose) return null;
  if (typeof payload.exp !== "number" || payload.exp < Date.now() / 1000) return null;

  return payload as T;
}

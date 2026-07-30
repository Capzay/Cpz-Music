import net from "node:net";
import path from "node:path";

/**
 * Minimal Discord Rich Presence client.
 *
 * Discord's local IPC is a length-prefixed JSON protocol over a named pipe, so
 * this is a socket and two integers. A dependency for it would be more code to
 * audit than the protocol itself.
 */

const OP_HANDSHAKE = 0;
const OP_FRAME = 1;
const OP_CLOSE = 2;
const OP_PING = 3;
const OP_PONG = 4;

export interface Presence {
  title: string;
  artist: string;
  album: string;
  artwork: string;
  isPlaying: boolean;
}

export interface Frame {
  op: number;
  payload: string;
}

/**
 * Splits a buffer into whole frames, returning the undrained remainder.
 *
 * Frames arrive coalesced or split across TCP reads, so this must never assume
 * one read equals one frame. Getting it wrong desynchronises the stream
 * permanently rather than failing loudly.
 */
export function decodeFrames(buffer: Buffer): { frames: Frame[]; rest: Buffer } {
  const frames: Frame[] = [];
  let rest: Buffer = buffer;

  while (rest.length >= 8) {
    const op = rest.readUInt32LE(0);
    const length = rest.readUInt32LE(4);
    if (rest.length < 8 + length) break;

    frames.push({ op, payload: rest.subarray(8, 8 + length).toString("utf8") });
    rest = rest.subarray(8 + length);
  }

  return { frames, rest };
}

export function encodeFrame(op: number, payload: unknown): Buffer {
  const data = Buffer.from(JSON.stringify(payload), "utf8");
  const header = Buffer.alloc(8);
  header.writeUInt32LE(op, 0);
  header.writeUInt32LE(data.length, 4);
  return Buffer.concat([header, data]);
}

function pipePath(id: number): string {
  if (process.platform === "win32") return `\\\\?\\pipe\\discord-ipc-${id}`;
  const base =
    process.env.XDG_RUNTIME_DIR ||
    process.env.TMPDIR ||
    process.env.TMP ||
    process.env.TEMP ||
    "/tmp";
  return path.join(base, `discord-ipc-${id}`);
}

class DiscordIPC {
  private socket: net.Socket | null = null;
  // Annotated: Buffer.alloc infers the narrower Buffer<ArrayBuffer>, which
  // subarray results do not satisfy.
  private buffer: Buffer = Buffer.alloc(0);
  ready = false;
  onReady: (() => void) | null = null;
  onClose: (() => void) | null = null;

  constructor(private clientId: string) {}

  async connect(): Promise<void> {
    let lastError: unknown = null;
    // Discord numbers its pipes when several clients are installed side by side.
    for (let i = 0; i < 10; i++) {
      try {
        await this.tryConnect(pipePath(i));
        return;
      } catch (err) {
        lastError = err;
      }
    }
    throw lastError ?? new Error("No Discord IPC pipe found. Is Discord running?");
  }

  private tryConnect(target: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const socket = net.createConnection(target);
      const onError = (err: Error) => {
        socket.removeAllListeners();
        socket.destroy();
        reject(err);
      };

      socket.once("error", onError);
      socket.once("connect", () => {
        socket.removeListener("error", onError);
        this.socket = socket;
        socket.on("data", (chunk: Buffer) => this.onData(chunk));
        socket.on("error", () => {
          this.ready = false;
        });
        socket.on("close", () => {
          this.ready = false;
          this.socket = null;
          this.onClose?.();
        });
        this.send(OP_HANDSHAKE, { v: 1, client_id: this.clientId });
        resolve();
      });
    });
  }

  private send(op: number, payload: unknown): void {
    this.socket?.write(encodeFrame(op, payload));
  }

  private onData(chunk: Buffer): void {
    const { frames, rest } = decodeFrames(Buffer.concat([this.buffer, chunk]));
    this.buffer = rest;

    for (const { op, payload } of frames) {
      let message: { cmd?: string; evt?: string } = {};
      try {
        message = JSON.parse(payload);
      } catch {
        continue;
      }

      if (op === OP_FRAME && message.cmd === "DISPATCH" && message.evt === "READY") {
        this.ready = true;
        this.onReady?.();
      } else if (op === OP_CLOSE) {
        this.destroy();
      } else if (op === OP_PING) {
        this.send(OP_PONG, message);
      }
    }
  }

  setActivity(activity: Record<string, unknown> | null): void {
    if (!this.ready) return;
    this.send(OP_FRAME, {
      cmd: "SET_ACTIVITY",
      args: { pid: process.pid, activity },
      nonce: `${Date.now()}-${Math.random()}`,
    });
  }

  destroy(): void {
    this.ready = false;
    try {
      this.socket?.destroy();
    } catch {
      // Already gone.
    }
    this.socket = null;
  }
}

/** Owns the connection, retries, and replaying presence after a reconnect. */
export class DiscordPresence {
  private rpc: DiscordIPC | null = null;
  private last: Presence | null = null;
  private retry: NodeJS.Timeout | null = null;

  constructor(private clientId: string) {}

  connect(): void {
    const rpc = new DiscordIPC(this.clientId);
    this.rpc = rpc;

    rpc.onReady = () => {
      if (this.last) this.update(this.last);
    };
    rpc.onClose = () => this.scheduleRetry();

    rpc.connect().catch(() => this.scheduleRetry());
  }

  private scheduleRetry(): void {
    if (this.retry) return;
    // Discord may simply not be running. Keep trying quietly.
    this.retry = setTimeout(() => {
      this.retry = null;
      this.connect();
    }, 15_000);
  }

  update(presence: Presence | null): void {
    this.last = presence;
    if (!this.rpc?.ready) return;

    if (!presence?.isPlaying || !presence.title) {
      this.rpc.setActivity(null);
      return;
    }

    const cap = (value: string) => value.slice(0, 128);
    this.rpc.setActivity({
      type: 2, // Listening
      name: cap(presence.artist || "Cpz Music"),
      details: cap(presence.title),
      state: presence.artist ? cap(`by ${presence.artist}`) : undefined,
      assets: {
        // Discord only fetches remote artwork over https.
        large_image: /^https:\/\//.test(presence.artwork) ? presence.artwork : "icon",
        large_text: cap(presence.album || presence.title),
        small_image: "play",
        small_text: "Playing",
      },
      instance: false,
    });
  }

  destroy(): void {
    if (this.retry) clearTimeout(this.retry);
    this.retry = null;
    this.rpc?.destroy();
  }
}

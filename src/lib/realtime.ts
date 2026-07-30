"use client";

import type { PlayerTrack, RepeatMode } from "@/lib/types";
import type { Command } from "@/store/player";

/**
 * Multi-device sync over Supabase Realtime.
 *
 * The old build kept playback state in a 392 line in-memory hub on the server,
 * which meant state died on every restart and every device had to be routed
 * through code the server owned. Here the server holds no playback state at
 * all: devices announce themselves through presence and talk to each other over
 * broadcast.
 *
 * The channel is private, so joining requires a policy on `realtime.messages`.
 * Without that, anyone holding the anon key (which ships in the browser bundle
 * and is public by design) could listen in.
 */
export const PLAYER_CHANNEL = "player";

export interface DevicePresence {
  deviceId: string;
  name: string;
  platform: string;
  /**
   * When this device last claimed playback. The device with the highest value
   * is the active one, which makes election a pure function of presence: no
   * election message to lose, and handover is automatic when the active device
   * disconnects and drops out of presence.
   */
  claimedAt: number;
}

export interface SharedState {
  track: PlayerTrack | null;
  queue: PlayerTrack[];
  index: number;
  isPlaying: boolean;
  currentTime: number;
  shuffle: boolean;
  repeat: RepeatMode;
  /** Lets a late-joining remote estimate elapsed time since this was sent. */
  sentAt: number;
}

export type Broadcast =
  | { event: "state"; payload: SharedState }
  | { event: "cmd"; payload: { target: string; command: Command } };

/** Highest claim wins; device id breaks ties so every client agrees. */
export function electActive(devices: DevicePresence[]): string | null {
  if (devices.length === 0) return null;
  return [...devices].sort(
    (a, b) => b.claimedAt - a.claimedAt || a.deviceId.localeCompare(b.deviceId),
  )[0].deviceId;
}

export function detectPlatform(): string {
  if (typeof navigator === "undefined") return "unknown";
  const ua = navigator.userAgent;
  if (/Android/i.test(ua)) return "Android";
  if (/iPhone|iPad|iPod/i.test(ua)) return "iOS";
  if (/Electron/i.test(ua)) return "Desktop app";
  return "Browser";
}

export function loadDeviceId(): string {
  const existing = localStorage.getItem("cpz-device-id");
  if (existing) return existing;
  const id = crypto.randomUUID();
  localStorage.setItem("cpz-device-id", id);
  return id;
}

export function loadDeviceName(): string {
  const existing = localStorage.getItem("cpz-device-name");
  if (existing) return existing;
  const name = detectPlatform();
  localStorage.setItem("cpz-device-name", name);
  return name;
}

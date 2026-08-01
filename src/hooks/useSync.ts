"use client";

import { useEffect, useRef } from "react";
import type { RealtimeChannel } from "@supabase/supabase-js";
import { supabaseBrowser } from "@/lib/supabase/client";
import { usePlayerStore, type Command } from "@/store/player";
import type { PlayerTrack } from "@/lib/types";
import {
  PLAYER_CHANNEL,
  detectPlatform,
  electActive,
  loadDeviceId,
  loadDeviceName,
  type DevicePresence,
  type SharedState,
} from "@/lib/realtime";

/** How often the active device republishes its position while playing. */
const STATE_INTERVAL_MS = 1000;

/** How often it corrects the overlay's position. Rarer: that one is an HTTP post. */
const OVERLAY_INTERVAL_MS = 15_000;

/** One channel per tab, like the audio element. The device picker reaches it here. */
let playerChannel: RealtimeChannel | null = null;

export function useSync() {
  const channelRef = useRef<RealtimeChannel | null>(null);
  const deviceIdRef = useRef<string>("");
  const claimedAtRef = useRef(0);

  useEffect(() => {
    const deviceId = loadDeviceId();
    deviceIdRef.current = deviceId;
    claimedAtRef.current = Number(sessionStorage.getItem("cpz-claimed-at") ?? 0);

    const supabase = supabaseBrowser();
    const channel = supabase.channel(PLAYER_CHANNEL, {
      config: {
        private: true,
        presence: { key: deviceId },
        // This device already knows what it just did.
        broadcast: { self: false },
      },
    });
    channelRef.current = channel;
    playerChannel = channel;

    function presenceDevices(): DevicePresence[] {
      const state = channel.presenceState<DevicePresence>();
      return Object.values(state)
        .map((entries) => entries[0])
        // Presence entries carry a presence_ref alongside the tracked payload.
        .filter((entry) => Boolean(entry?.deviceId))
        .map(({ deviceId, name, platform, claimedAt }) => ({
          deviceId,
          name,
          platform,
          claimedAt,
        }));
    }

    function syncDevices() {
      const devices = presenceDevices();
      usePlayerStore
        .getState()
        .setDevices(devices, electActive(devices), deviceIdRef.current);
    }

    channel
      .on("presence", { event: "sync" }, syncDevices)
      .on("presence", { event: "join" }, syncDevices)
      .on("presence", { event: "leave" }, syncDevices)
      .on("broadcast", { event: "state" }, ({ payload }) => {
        // Only remotes mirror state; the active device is the one producing it.
        if (usePlayerStore.getState().isActiveDevice) return;
        usePlayerStore.getState().applySharedState(payload as SharedState);
      })
      .on("broadcast", { event: "jam-add" }, ({ payload }) => {
        // Sent by the server on a guest's behalf; guests cannot broadcast here.
        if (!usePlayerStore.getState().isActiveDevice) return;
        const { track } = payload as { track: PlayerTrack };
        if (track) usePlayerStore.getState().apply({ type: "addToQueue", track });
      })
      .on("broadcast", { event: "cmd" }, ({ payload }) => {
        const { target, command } = payload as { target: string; command: Command };
        if (target !== deviceIdRef.current) return;
        if (!usePlayerStore.getState().isActiveDevice) return;
        usePlayerStore.getState().apply(command);
      });

    // A private channel is authorised by RLS policies on realtime.messages, which
    // need the caller's access token on the socket rather than just the anon key.
    void supabase.auth.getSession().then(({ data }) => {
      void supabase.realtime.setAuth(data.session?.access_token);
    });

    channel.subscribe(async (status) => {
      if (status !== "SUBSCRIBED") return;
      await channel.track({
        deviceId,
        name: loadDeviceName(),
        platform: detectPlatform(),
        claimedAt: claimedAtRef.current,
      } satisfies DevicePresence);
    });

    // Remotes send commands to whichever device currently holds playback.
    usePlayerStore.setState({
      forward: (command: Command) => {
        const target = usePlayerStore.getState().activeDeviceId;
        if (!target) return;
        void channel.send({ type: "broadcast", event: "cmd", payload: { target, command } });
      },
    });

    return () => {
      usePlayerStore.setState({ forward: null });
      void supabase.removeChannel(channel);
      channelRef.current = null;
      playerChannel = null;
    };
  }, []);

  // The active device publishes what it is doing, on a timer while playing and
  // immediately whenever the track or transport state changes.
  const isActiveDevice = usePlayerStore((s) => s.isActiveDevice);
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const index = usePlayerStore((s) => s.index);
  const queueLength = usePlayerStore((s) => s.queue.length);

  useEffect(() => {
    const channel = channelRef.current;
    if (!channel || !isActiveDevice) return;

    const publish = () => {
      const s = usePlayerStore.getState();
      const payload: SharedState = {
        track: s.queue[s.index] ?? null,
        queue: s.queue,
        index: s.index,
        isPlaying: s.isPlaying,
        currentTime: s.currentTime,
        shuffle: s.shuffle,
        repeat: s.repeat,
        sentAt: Date.now(),
      };
      void channel.send({ type: "broadcast", event: "state", payload });
    };

    // The OBS overlay cannot join the channel, so mirror the essentials to the
    // server for it to poll. Only on track or transport changes, not per second:
    // the endpoint runs the position forward on its own between posts.
    const publishOverlay = () => {
      const s = usePlayerStore.getState();
      void fetch("/api/now-playing", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          track: s.queue[s.index] ?? null,
          isPlaying: s.isPlaying,
          currentTime: s.currentTime,
        }),
      }).catch(() => {});
    };

    publish();
    publishOverlay();
    if (!isPlaying) return;
    const timer = setInterval(publish, STATE_INTERVAL_MS);
    // A seek moves the position without changing the track or transport state,
    // so nothing above would re-post it and the overlay's bar would sit wrong
    // for the rest of the track. Slow enough not to be the per-second posting
    // the comment above rules out.
    const overlayTimer = setInterval(publishOverlay, OVERLAY_INTERVAL_MS);
    return () => {
      clearInterval(timer);
      clearInterval(overlayTimer);
    };
  }, [isActiveDevice, isPlaying, index, queueLength]);
}

/**
 * Takes over playback on this device. Claiming is just a presence update with a
 * newer timestamp, which every client independently resolves the same way, so
 * there is no election message that can be lost or arrive out of order.
 */
export async function claimPlayback() {
  const claimedAt = Date.now();
  sessionStorage.setItem("cpz-claimed-at", String(claimedAt));
  await playerChannel?.track({
    deviceId: loadDeviceId(),
    name: loadDeviceName(),
    platform: detectPlatform(),
    claimedAt,
  } satisfies DevicePresence);
}

/** Renames this device everywhere, including in other devices' pickers. */
export async function renameDevice(name: string) {
  const trimmed = name.trim().slice(0, 40);
  if (!trimmed) return;
  localStorage.setItem("cpz-device-name", trimmed);
  await playerChannel?.track({
    deviceId: loadDeviceId(),
    name: trimmed,
    platform: detectPlatform(),
    claimedAt: Number(sessionStorage.getItem("cpz-claimed-at") ?? 0),
  } satisfies DevicePresence);
}

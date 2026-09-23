"use client";

import { useCallback, useLayoutEffect, useSyncExternalStore } from "react";
import { canPlayTrack } from "@/lib/offline-play";
import { useDownloads } from "@/store/downloads";

function subscribeOnline(onChange: () => void) {
  window.addEventListener("online", onChange);
  window.addEventListener("offline", onChange);
  return () => {
    window.removeEventListener("online", onChange);
    window.removeEventListener("offline", onChange);
  };
}

/** True when this track can start playback on this device right now. */
export function usePlayableTrack() {
  const registry = useDownloads((s) => s.registry);
  const hydrate = useDownloads((s) => s.hydrate);
  // Server render assumes online so the HTML matches; the client corrects it.
  const online = useSyncExternalStore(subscribeOnline, () => navigator.onLine, () => true);

  useLayoutEffect(() => {
    hydrate();
  }, [hydrate]);

  return useCallback(
    (trackId: number) => canPlayTrack(online, !!registry[trackId]),
    [online, registry],
  );
}

"use client";

import { useEffect, useSyncExternalStore, type ReactNode } from "react";
import { PlaylistsApp } from "@/components/PlaylistsApp";
import { DownloadsList } from "@/components/DownloadsList";
import { startPlaylistSync } from "@/store/playlists";
import {
  clearOfflinePath,
  getOfflinePath,
  isOfflineClientPath,
  subscribeOfflinePath,
  syncOfflinePathFromLocation,
} from "@/lib/offline-nav";

/**
 * While offline, swap the main outlet to local-state pages without asking Next
 * for an RSC payload. Online, the server children render as usual.
 */
export function OfflineShell({ children }: { children: ReactNode }) {
  const offlinePath = useSyncExternalStore(
    subscribeOfflinePath,
    getOfflinePath,
    () => null,
  );

  useEffect(() => {
    startPlaylistSync();
    syncOfflinePathFromLocation();

    const onPop = () => syncOfflinePathFromLocation();
    const onOnline = () => clearOfflinePath();
    const onOffline = () => syncOfflinePathFromLocation();

    window.addEventListener("popstate", onPop);
    window.addEventListener("online", onOnline);
    window.addEventListener("offline", onOffline);
    return () => {
      window.removeEventListener("popstate", onPop);
      window.removeEventListener("online", onOnline);
      window.removeEventListener("offline", onOffline);
    };
  }, []);

  const pathname = (offlinePath ?? "").split(/[?#]/)[0] ?? "";

  if (offlinePath && isOfflineClientPath(pathname)) {
    if (pathname === "/downloads") {
      return (
        <>
          <h1 className="text-xl font-bold mb-4 md:text-2xl md:mb-6">Downloads</h1>
          <DownloadsList />
        </>
      );
    }

    const routeId = pathname.startsWith("/playlists/")
      ? decodeURIComponent(pathname.slice("/playlists/".length).replace(/\/$/, ""))
      : undefined;
    return <PlaylistsApp routeId={routeId || undefined} />;
  }

  return children;
}

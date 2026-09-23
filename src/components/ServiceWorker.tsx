"use client";

import { useEffect } from "react";

/**
 * Registered from inside the authenticated layout, never from the login page.
 * A service worker registered before sign-in would cache the login redirect as
 * the app shell and serve it offline forever after.
 */
export function ServiceWorker() {
  useEffect(() => {
    if (!("serviceWorker" in navigator)) return;

    let cancelled = false;

    function requestPrecache(worker: ServiceWorker | null | undefined) {
      worker?.postMessage({ type: "precache" });
    }

    async function register() {
      try {
        const registration = await navigator.serviceWorker.register("/sw.js");
        if (cancelled) return;

        // Soft Next navigations never populate the page cache. Ask the worker
        // to pull the offline shell (/, /playlists, /downloads) while online.
        const ready = await navigator.serviceWorker.ready;
        if (cancelled) return;
        requestPrecache(ready.active);

        // A brand-new worker may still be installing; message it once it takes
        // control so the first offline visit after sign-in is not empty.
        registration.addEventListener("updatefound", () => {
          const worker = registration.installing;
          if (!worker) return;
          worker.addEventListener("statechange", () => {
            if (worker.state === "activated") requestPrecache(worker);
          });
        });
      } catch {
        // Unsupported browser, or served over plain HTTP. Offline is optional.
      }
    }

    function onOnline() {
      void navigator.serviceWorker.ready.then((ready) => requestPrecache(ready.active));
    }

    void register();
    window.addEventListener("online", onOnline);
    return () => {
      cancelled = true;
      window.removeEventListener("online", onOnline);
    };
  }, []);

  return null;
}

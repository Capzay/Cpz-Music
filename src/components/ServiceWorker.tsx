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
    navigator.serviceWorker.register("/sw.js").catch(() => {
      // Unsupported browser, or served over plain HTTP. Offline is optional.
    });
  }, []);

  return null;
}

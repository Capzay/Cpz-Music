"use client";

import Link from "next/link";
import type { MouseEvent, ReactNode } from "react";
import { clearOfflinePath, goOfflineAware, navigateOffline } from "@/lib/offline-nav";

export function playlistHref(uuid: string) {
  return `/playlists/${uuid}`;
}

export { goOfflineAware };

/**
 * Soft navigations need a network round-trip for RSC. Offline, stay inside the
 * already-loaded shell and swap the outlet instead of reloading the document.
 */
export function OfflineAwareLink({
  href,
  className,
  "aria-current": ariaCurrent,
  children,
}: {
  href: string;
  className?: string;
  "aria-current"?: "page" | undefined;
  children: ReactNode;
}) {
  function onClick(event: MouseEvent<HTMLAnchorElement>) {
    if (typeof navigator !== "undefined" && !navigator.onLine) {
      event.preventDefault();
      navigateOffline(href);
      return;
    }
    clearOfflinePath();
  }

  return (
    <Link href={href} className={className} aria-current={ariaCurrent} onClick={onClick}>
      {children}
    </Link>
  );
}

export function PlaylistLink({
  uuid,
  className,
  children,
}: {
  uuid: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <OfflineAwareLink href={playlistHref(uuid)} className={className}>
      {children}
    </OfflineAwareLink>
  );
}

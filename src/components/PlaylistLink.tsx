"use client";

import Link from "next/link";
import type { MouseEvent, ReactNode } from "react";

export function playlistHref(uuid: string) {
  return `/playlists/${uuid}`;
}

/** Next client navigations fetch RSC, which fails offline for unseen playlist URLs. */
export function goOfflineAware(href: string, push: (href: string) => void) {
  if (typeof navigator !== "undefined" && !navigator.onLine) {
    location.assign(href);
    return;
  }
  push(href);
}

/**
 * Soft navigations need a network round-trip for RSC. Offline-capable pages
 * (playlists, downloads) must fall back to a full load so the service worker
 * can serve the cached document instead.
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
      location.assign(href);
    }
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

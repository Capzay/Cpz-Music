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

export function PlaylistLink({
  uuid,
  className,
  children,
}: {
  uuid: string;
  className?: string;
  children: ReactNode;
}) {
  const href = playlistHref(uuid);

  function onClick(event: MouseEvent<HTMLAnchorElement>) {
    if (typeof navigator !== "undefined" && !navigator.onLine) {
      event.preventDefault();
      location.assign(href);
    }
  }

  return (
    <Link href={href} className={className} onClick={onClick}>
      {children}
    </Link>
  );
}

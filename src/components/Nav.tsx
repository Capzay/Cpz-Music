"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const PRIMARY = [
  { href: "/", label: "Albums" },
  { href: "/artists", label: "Artists" },
  { href: "/playlists", label: "Playlists" },
  { href: "/search", label: "Search" },
];

const SECONDARY = [
  { href: "/downloads", label: "Downloads" },
  { href: "/stats", label: "Stats" },
  { href: "/settings", label: "Settings" },
];

function isActive(pathname: string, href: string) {
  return href === "/" ? pathname === "/" : pathname.startsWith(href);
}

export function Sidebar() {
  const pathname = usePathname();
  const link = (href: string, label: string) => (
    <Link
      key={href}
      href={href}
      aria-current={isActive(pathname, href) ? "page" : undefined}
      className={`rounded-md px-3 py-2 text-sm transition ${
        isActive(pathname, href)
          ? "bg-neutral-800 text-neutral-100"
          : "text-neutral-400 hover:bg-neutral-900 hover:text-neutral-200"
      }`}
    >
      {label}
    </Link>
  );

  return (
    <nav className="hidden w-52 shrink-0 flex-col gap-1 border-r border-neutral-900 p-4 md:flex">
      <p className="mb-4 px-3 text-sm font-semibold tracking-tight">Cpz Music</p>
      {PRIMARY.map(({ href, label }) => link(href, label))}
      <hr className="my-2 border-neutral-900" />
      {SECONDARY.map(({ href, label }) => link(href, label))}
    </nav>
  );
}

export function MobileNav() {
  const pathname = usePathname();
  // Only the primary destinations fit across a phone; the rest live in Settings.
  return (
    <nav className="flex border-t border-neutral-900 bg-neutral-950 md:hidden">
      {PRIMARY.map(({ href, label }) => (
        <Link
          key={href}
          href={href}
          aria-current={isActive(pathname, href) ? "page" : undefined}
          className={`flex-1 py-3 text-center text-xs transition ${
            isActive(pathname, href) ? "text-neutral-100" : "text-neutral-500"
          }`}
        >
          {label}
        </Link>
      ))}
    </nav>
  );
}

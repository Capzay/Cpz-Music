"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const LINKS = [
  { href: "/", label: "Albums" },
  { href: "/artists", label: "Artists" },
  { href: "/search", label: "Search" },
];

function isActive(pathname: string, href: string) {
  return href === "/" ? pathname === "/" : pathname.startsWith(href);
}

export function Sidebar() {
  const pathname = usePathname();
  return (
    <nav className="hidden w-52 shrink-0 flex-col gap-1 border-r border-neutral-900 p-4 md:flex">
      <p className="mb-4 px-3 text-sm font-semibold tracking-tight">Cpz Music</p>
      {LINKS.map(({ href, label }) => (
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
      ))}
    </nav>
  );
}

export function MobileNav() {
  const pathname = usePathname();
  return (
    <nav className="flex border-t border-neutral-900 bg-neutral-950 md:hidden">
      {LINKS.map(({ href, label }) => (
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

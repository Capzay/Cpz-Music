"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import {
  BarChart2,
  Disc3,
  Download,
  Library as LibraryIcon,
  ListMusic,
  Mic2,
  Music2,
  Radio,
  Search,
} from "lucide-react";

const LINKS = [
  { href: "/", icon: LibraryIcon, label: "Library" },
  { href: "/albums", icon: Disc3, label: "Albums" },
  { href: "/artists", icon: Mic2, label: "Artists" },
  { href: "/playlists", icon: ListMusic, label: "Playlists" },
  { href: "/search", icon: Search, label: "Search" },
  { href: "/stats", icon: BarChart2, label: "Stats" },
  { href: "/downloads", icon: Download, label: "Offline" },
];

function isActive(pathname: string, href: string) {
  return href === "/" ? pathname === "/" : pathname.startsWith(href);
}

const linkClass = (active: boolean) =>
  `flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors ${
    active ? "bg-zinc-800 text-white" : "text-zinc-400 hover:text-white hover:bg-zinc-800/50"
  }`;

export function Sidebar({ playlists }: { playlists: { id: number; name: string }[] }) {
  const pathname = usePathname();

  return (
    <nav className="hidden md:flex flex-col w-60 flex-shrink-0 h-full bg-zinc-950 border-r border-zinc-800 py-4 overflow-y-auto">
      <div className="flex items-center gap-2 px-4 mb-6">
        <Music2 size={22} className="text-violet-400" />
        <span className="font-bold text-base tracking-tight">Cpz Music</span>
      </div>

      <div className="flex flex-col gap-0.5 px-2">
        {LINKS.map(({ href, icon: Icon, label }) => (
          <Link
            key={href}
            href={href}
            aria-current={isActive(pathname, href) ? "page" : undefined}
            className={linkClass(isActive(pathname, href))}
          >
            <Icon size={18} />
            <span>{label}</span>
          </Link>
        ))}
        <Link
          href="/settings"
          aria-current={isActive(pathname, "/settings") ? "page" : undefined}
          className={linkClass(isActive(pathname, "/settings"))}
        >
          <Radio size={18} />
          <span>Jam &amp; settings</span>
        </Link>
      </div>

      {playlists.length > 0 && (
        <div className="mt-6 px-2">
          <p className="px-3 text-xs font-semibold text-zinc-500 uppercase tracking-wider mb-1">
            Playlists
          </p>
          <div className="flex flex-col gap-0.5">
            {playlists.map((playlist) => (
              <Link
                key={playlist.id}
                href={`/playlists/${playlist.id}`}
                className={linkClass(pathname === `/playlists/${playlist.id}`)}
              >
                <ListMusic size={16} />
                <span className="truncate">{playlist.name}</span>
              </Link>
            ))}
          </div>
        </div>
      )}
    </nav>
  );
}

export function MobileNav() {
  const pathname = usePathname();

  return (
    <nav className="md:hidden flex items-center justify-around bg-zinc-900 border-t border-zinc-800 pb-safe">
      {LINKS.map(({ href, icon: Icon, label }) => (
        <Link
          key={href}
          href={href}
          aria-current={isActive(pathname, href) ? "page" : undefined}
          className={`flex flex-col items-center gap-0.5 py-2 px-2 text-xs transition-colors ${
            isActive(pathname, href) ? "text-violet-400" : "text-zinc-400"
          }`}
        >
          <Icon size={20} />
          <span>{label}</span>
        </Link>
      ))}
    </nav>
  );
}

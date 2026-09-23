import { PlaylistsApp } from "@/components/PlaylistsApp";

export const metadata = { title: "Playlists" };

/**
 * Entirely client-rendered: playlists live in this device's local store and
 * sync when the server is reachable. Same reason /downloads cannot query Prisma.
 */
export default function PlaylistsPage() {
  return <PlaylistsApp />;
}

"use client";

import { useEffect, useState } from "react";
import { ListMusic, Plus } from "lucide-react";
import { useRouter } from "next/navigation";
import { usePlaylists } from "@/store/playlists";
import { PlayAlbumButton } from "@/components/PlayAlbumButton";
import { DownloadButton } from "@/components/DownloadButton";
import { AddToPlaylistButton } from "@/components/AddToPlaylistPicker";
import { PlaylistTracks } from "@/components/PlaylistTracks";
import { PlaylistHeader } from "@/components/PlaylistHeader";
import { PlaylistLink, goOfflineAware, playlistHref } from "@/components/PlaylistLink";
import { findPlaylist, visiblePlaylists } from "@/lib/playlist-sync";
import { replaceOffline } from "@/lib/offline-nav";

export function PlaylistsApp({ routeId }: { routeId?: string }) {
  const hydrate = usePlaylists((s) => s.hydrate);
  const [hashId, setHashId] = useState<string | null>(null);
  const router = useRouter();

  useEffect(() => hydrate(), [hydrate]);

  useEffect(() => {
    const read = () => {
      try {
        const hash = decodeURIComponent(location.hash.replace(/^#/, ""));
        setHashId(hash || null);
      } catch {
        setHashId(null);
      }
    };
    read();
    window.addEventListener("hashchange", read);
    return () => window.removeEventListener("hashchange", read);
  }, []);

  function closeDetail() {
    if (hashId) {
      if (typeof navigator !== "undefined" && !navigator.onLine) {
        replaceOffline("/playlists");
      } else {
        history.replaceState(null, "", "/playlists");
      }
      setHashId(null);
      return;
    }
    if (typeof navigator !== "undefined" && !navigator.onLine) {
      replaceOffline("/playlists");
      return;
    }
    goOfflineAware("/playlists", (href) => router.push(href));
  }

  const selected = routeId || hashId;
  if (selected) return <PlaylistDetail selected={selected} onClose={closeDetail} />;
  return <PlaylistsList />;
}

function PlaylistsList() {
  const playlists = visiblePlaylists(usePlaylists((s) => s.playlists));
  const hydrated = usePlaylists((s) => s.hydrated);
  const create = usePlaylists((s) => s.create);
  const router = useRouter();

  return (
    <>
      <h1 className="text-xl font-bold mb-4 md:text-2xl md:mb-6">Playlists</h1>

      <form
        className="flex gap-2 mb-6"
        onSubmit={(event) => {
          event.preventDefault();
          const form = event.currentTarget;
          const name = String(new FormData(form).get("name") ?? "");
          const uuid = create(name);
          if (!uuid) return;
          form.reset();
          goOfflineAware(playlistHref(uuid), (href) => router.push(href));
        }}
      >
        <input
          name="name"
          required
          maxLength={120}
          placeholder="New playlist name..."
          className="flex-1 bg-zinc-800 border border-zinc-700 rounded-md px-3 py-2 text-sm focus:outline-none focus:border-violet-500 text-white placeholder-zinc-500"
        />
        <button className="flex items-center gap-1 bg-violet-600 hover:bg-violet-500 text-white px-4 py-2 rounded-md text-sm font-medium transition-colors">
          <Plus size={16} />
          Create
        </button>
      </form>

      {!hydrated ? null : playlists.length === 0 ? (
        <p className="text-zinc-400">No playlists yet. Create one above.</p>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4">
          {playlists.map((playlist) => (
            <PlaylistLink
              key={playlist.uuid}
              uuid={playlist.uuid}
              className="block cursor-pointer bg-zinc-900 border border-zinc-800 rounded-lg p-4 hover:border-zinc-600 transition-colors"
            >
              <div className="w-12 h-12 bg-zinc-800 rounded-md flex items-center justify-center mb-3">
                <ListMusic size={24} className="text-violet-400" />
              </div>
              <p className="font-medium truncate">{playlist.name}</p>
              <p className="text-sm text-zinc-500 mt-0.5">
                {playlist.tracks.length} track{playlist.tracks.length !== 1 ? "s" : ""}
              </p>
            </PlaylistLink>
          ))}
        </div>
      )}
    </>
  );
}

function PlaylistDetail({ selected, onClose }: { selected: string; onClose: () => void }) {
  const playlists = usePlaylists((s) => s.playlists);
  const hydrated = usePlaylists((s) => s.hydrated);
  const playlist = findPlaylist(playlists, selected);
  const router = useRouter();

  useEffect(() => {
    if (!playlist || playlist.uuid === selected) return;
    if (typeof navigator !== "undefined" && !navigator.onLine) return;
    router.replace(playlistHref(playlist.uuid));
  }, [playlist, selected, router]);

  if (!hydrated) return null;

  if (!playlist) {
    return (
      <p className="text-sm text-zinc-500">
        Playlist not found.{" "}
        <button className="text-violet-400 hover:text-violet-300" onClick={onClose}>
          Back to playlists
        </button>
      </p>
    );
  }

  return (
    <>
      <PlaylistHeader
        uuid={playlist.uuid}
        name={playlist.name}
        count={playlist.tracks.length}
        onDeleted={onClose}
      />

      <div className="mb-5 flex flex-wrap gap-2">
        {playlist.tracks.length > 0 ? (
          <>
            <PlayAlbumButton tracks={playlist.tracks} />
            <DownloadButton tracks={playlist.tracks} />
          </>
        ) : null}
        <AddToPlaylistButton
          uuid={playlist.uuid}
          existingIds={new Set(playlist.tracks.map((t) => t.id))}
          trackCount={playlist.tracks.length}
        />
      </div>

      {playlist.tracks.length === 0 ? (
        <p className="mb-4 text-sm text-zinc-500">Empty. Use Add songs to search the library.</p>
      ) : null}

      <PlaylistTracks uuid={playlist.uuid} tracks={playlist.tracks} />
    </>
  );
}

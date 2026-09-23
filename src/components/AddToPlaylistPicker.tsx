"use client";

import { useEffect, useEffectEvent, useRef, useState, useTransition } from "react";
import { Check, ChevronDown, ChevronRight, Plus, Search as SearchIcon } from "lucide-react";
import { usePlaylists } from "@/store/playlists";
import { artworkUrl, type PlayerTrack } from "@/lib/types";
import { formatDuration } from "@/lib/format";
import { MAX_TRACKS } from "@/lib/playlist-sync";

type SearchAlbumHit = {
  id: number;
  title: string;
  year: number | null;
  artist: string;
  hasArtwork: boolean;
};

type SearchArtistHit = {
  id: number;
  name: string;
};

type SearchResults = {
  tracks: PlayerTrack[];
  albums: SearchAlbumHit[];
  artists: SearchArtistHit[];
};

type Selected = Map<number, PlayerTrack>;

function emptyResults(): SearchResults {
  return { tracks: [], albums: [], artists: [] };
}

export function AddToPlaylistButton({
  uuid,
  existingIds,
  trackCount,
}: {
  uuid: string;
  existingIds: ReadonlySet<number>;
  trackCount: number;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const addTracks = usePlaylists((s) => s.addTracks);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResults>(emptyResults);
  const [selected, setSelected] = useState<Selected>(() => new Map());
  const [expandedAlbums, setExpandedAlbums] = useState<Record<number, PlayerTrack[] | "loading">>(
    {},
  );
  const [expandedArtists, setExpandedArtists] = useState<
    Record<number, PlayerTrack[] | "loading">
  >({});
  const [error, setError] = useState<string | null>(null);
  const [pending, startTransition] = useTransition();
  const [offline, setOffline] = useState(false);

  const room = Math.max(0, MAX_TRACKS - trackCount);

  useEffect(() => {
    const sync = () => setOffline(typeof navigator !== "undefined" && !navigator.onLine);
    sync();
    window.addEventListener("online", sync);
    window.addEventListener("offline", sync);
    return () => {
      window.removeEventListener("online", sync);
      window.removeEventListener("offline", sync);
    };
  }, []);

  const runSearch = useEffectEvent(async (term: string) => {
    if (typeof navigator !== "undefined" && !navigator.onLine) {
      setError("Search needs a network connection.");
      setResults(emptyResults());
      setExpandedAlbums({});
      setExpandedArtists({});
      return;
    }
    try {
      const response = await fetch(`/api/search?q=${encodeURIComponent(term)}`);
      if (!response.ok) throw new Error("Search failed");
      const data = (await response.json()) as SearchResults;
      setResults({
        tracks: data.tracks ?? [],
        albums: data.albums ?? [],
        artists: data.artists ?? [],
      });
      setExpandedAlbums({});
      setExpandedArtists({});
      setError(null);
    } catch {
      setError("Search failed. Try again.");
      setResults(emptyResults());
      setExpandedAlbums({});
      setExpandedArtists({});
    }
  });

  const term = query.trim();

  useEffect(() => {
    if (!term) return;
    const handle = window.setTimeout(() => {
      startTransition(() => {
        void runSearch(term);
      });
    }, 250);
    return () => window.clearTimeout(handle);
  }, [term]);

  const shown = term ? results : emptyResults();

  function open() {
    setQuery("");
    setResults(emptyResults());
    setSelected(new Map());
    setExpandedAlbums({});
    setExpandedArtists({});
    setError(null);
    dialogRef.current?.showModal();
  }

  function close() {
    dialogRef.current?.close();
  }

  function toggleTrack(track: PlayerTrack) {
    setSelected((prev) => {
      const next = new Map(prev);
      if (next.has(track.id)) next.delete(track.id);
      else next.set(track.id, track);
      return next;
    });
  }

  function selectMany(tracks: PlayerTrack[], on: boolean) {
    setSelected((prev) => {
      const next = new Map(prev);
      for (const track of tracks) {
        if (on) next.set(track.id, track);
        else next.delete(track.id);
      }
      return next;
    });
  }

  function visibleTracks(): PlayerTrack[] {
    const byId = new Map<number, PlayerTrack>();
    for (const track of shown.tracks) byId.set(track.id, track);
    for (const tracks of Object.values(expandedAlbums)) {
      if (tracks === "loading") continue;
      for (const track of tracks) byId.set(track.id, track);
    }
    for (const tracks of Object.values(expandedArtists)) {
      if (tracks === "loading") continue;
      for (const track of tracks) byId.set(track.id, track);
    }
    return [...byId.values()];
  }

  function selectAllVisible() {
    selectMany(visibleTracks(), true);
  }

  function clearSelection() {
    setSelected(new Map());
  }

  async function expandAlbum(album: SearchAlbumHit) {
    if (expandedAlbums[album.id]) {
      setExpandedAlbums((prev) => {
        const next = { ...prev };
        delete next[album.id];
        return next;
      });
      return;
    }
    setExpandedAlbums((prev) => ({ ...prev, [album.id]: "loading" }));
    try {
      const response = await fetch(`/api/search?album=${album.id}`);
      if (!response.ok) throw new Error("failed");
      const data = (await response.json()) as { tracks: PlayerTrack[] };
      setExpandedAlbums((prev) => ({ ...prev, [album.id]: data.tracks ?? [] }));
    } catch {
      setExpandedAlbums((prev) => {
        const next = { ...prev };
        delete next[album.id];
        return next;
      });
      setError("Could not load album tracks.");
    }
  }

  async function expandArtist(artist: SearchArtistHit) {
    if (expandedArtists[artist.id]) {
      setExpandedArtists((prev) => {
        const next = { ...prev };
        delete next[artist.id];
        return next;
      });
      return;
    }
    setExpandedArtists((prev) => ({ ...prev, [artist.id]: "loading" }));
    try {
      const response = await fetch(`/api/search?artist=${artist.id}`);
      if (!response.ok) throw new Error("failed");
      const data = (await response.json()) as { tracks: PlayerTrack[] };
      setExpandedArtists((prev) => ({ ...prev, [artist.id]: data.tracks ?? [] }));
    } catch {
      setExpandedArtists((prev) => {
        const next = { ...prev };
        delete next[artist.id];
        return next;
      });
      setError("Could not load artist tracks.");
    }
  }

  function commit() {
    const picks = [...selected.values()];
    if (picks.length === 0) return;
    const capped = room > 0 ? picks.slice(0, room) : [];
    if (capped.length === 0) {
      setError(`Playlist is full (${MAX_TRACKS} tracks).`);
      return;
    }
    addTracks(uuid, capped);
    close();
  }

  const hasResults =
    shown.tracks.length > 0 || shown.albums.length > 0 || shown.artists.length > 0;
  const selectedCount = selected.size;

  return (
    <>
      <button
        type="button"
        onClick={open}
        className="flex items-center gap-2 border border-zinc-600 hover:border-zinc-400 text-zinc-300 hover:text-white px-4 py-2 rounded-full text-sm font-medium transition-colors"
      >
        <Plus size={16} />
        Add songs
      </button>

      <dialog
        ref={dialogRef}
        onClose={close}
        className="m-auto flex h-[min(40rem,90vh)] w-full max-w-lg flex-col rounded-lg border border-zinc-700 bg-zinc-900 p-0 text-white backdrop:bg-black/60"
      >
        <div className="border-b border-zinc-800 px-4 py-3">
          <p className="text-sm font-medium">Add songs</p>
          <p className="text-xs text-zinc-500">Search tracks, albums, or artists</p>
        </div>

        <div className="border-b border-zinc-800 px-4 py-3">
          <div className="relative">
            <SearchIcon
              size={16}
              className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-zinc-500"
            />
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search…"
              autoFocus
              disabled={offline}
              className="w-full rounded-md border border-zinc-700 bg-zinc-800 py-2 pl-9 pr-3 text-sm text-white placeholder-zinc-500 outline-none focus:border-violet-500 disabled:opacity-50"
            />
          </div>
          {offline ? (
            <p className="mt-2 text-xs text-amber-400">Go online to search the library.</p>
          ) : null}
          {error ? <p className="mt-2 text-xs text-red-400">{error}</p> : null}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-2 py-2">
          {!term ? (
            <p className="px-2 py-6 text-center text-sm text-zinc-500">
              Type to find songs, albums, or artists.
            </p>
          ) : pending && !hasResults ? (
            <p className="px-2 py-6 text-center text-sm text-zinc-500">Searching…</p>
          ) : !hasResults ? (
            <p className="px-2 py-6 text-center text-sm text-zinc-500">No results for “{term}”</p>
          ) : (
            <div className="space-y-4">
              {shown.tracks.length > 0 ? (
                <section>
                  <SectionHeader
                    title="Tracks"
                    count={shown.tracks.length}
                    onSelectAll={() => selectMany(shown.tracks, true)}
                    onClear={() => selectMany(shown.tracks, false)}
                  />
                  <ul>
                    {shown.tracks.map((track) => (
                      <TrackRow
                        key={track.id}
                        track={track}
                        checked={selected.has(track.id)}
                        alreadyIn={existingIds.has(track.id)}
                        onToggle={() => toggleTrack(track)}
                      />
                    ))}
                  </ul>
                </section>
              ) : null}

              {shown.albums.length > 0 ? (
                <section>
                  <h3 className="px-2 pb-1 text-xs font-semibold uppercase tracking-wide text-zinc-500">
                    Albums
                  </h3>
                  <ul className="space-y-1">
                    {shown.albums.map((album) => {
                      const expanded = expandedAlbums[album.id];
                      const openAlbum = expanded !== undefined;
                      const tracks = Array.isArray(expanded) ? expanded : [];
                      return (
                        <li key={album.id} className="rounded-md">
                          <div className="flex items-center gap-1">
                            <button
                              type="button"
                              onClick={() => void expandAlbum(album)}
                              className="flex min-w-0 flex-1 items-center gap-3 rounded-md px-2 py-2 text-left hover:bg-zinc-800/80"
                            >
                              {openAlbum ? (
                                <ChevronDown size={16} className="shrink-0 text-zinc-500" />
                              ) : (
                                <ChevronRight size={16} className="shrink-0 text-zinc-500" />
                              )}
                              <span className="h-9 w-9 shrink-0 overflow-hidden rounded bg-zinc-800">
                                {album.hasArtwork ? (
                                  // eslint-disable-next-line @next/next/no-img-element
                                  <img
                                    src={artworkUrl(album.id)}
                                    alt=""
                                    className="h-full w-full object-cover"
                                  />
                                ) : null}
                              </span>
                              <span className="min-w-0 flex-1">
                                <span className="block truncate text-sm font-medium">
                                  {album.title}
                                </span>
                                <span className="block truncate text-xs text-zinc-500">
                                  {album.artist}
                                  {album.year ? ` · ${album.year}` : ""}
                                </span>
                              </span>
                            </button>
                            {Array.isArray(expanded) && expanded.length > 0 ? (
                              <button
                                type="button"
                                onClick={() => selectMany(expanded, true)}
                                className="shrink-0 px-2 text-xs text-violet-400 hover:text-violet-300"
                              >
                                All
                              </button>
                            ) : null}
                          </div>
                          {expanded === "loading" ? (
                            <p className="px-8 pb-2 text-xs text-zinc-500">Loading…</p>
                          ) : null}
                          {tracks.length > 0 ? (
                            <ul className="ml-4 border-l border-zinc-800 pb-1 pl-1">
                              {tracks.map((track) => (
                                <TrackRow
                                  key={track.id}
                                  track={track}
                                  checked={selected.has(track.id)}
                                  alreadyIn={existingIds.has(track.id)}
                                  onToggle={() => toggleTrack(track)}
                                  compact
                                />
                              ))}
                            </ul>
                          ) : null}
                        </li>
                      );
                    })}
                  </ul>
                </section>
              ) : null}

              {shown.artists.length > 0 ? (
                <section>
                  <h3 className="px-2 pb-1 text-xs font-semibold uppercase tracking-wide text-zinc-500">
                    Artists
                  </h3>
                  <ul className="space-y-1">
                    {shown.artists.map((artist) => {
                      const expanded = expandedArtists[artist.id];
                      const openArtist = expanded !== undefined;
                      const tracks = Array.isArray(expanded) ? expanded : [];
                      return (
                        <li key={artist.id} className="rounded-md">
                          <div className="flex items-center gap-1">
                            <button
                              type="button"
                              onClick={() => void expandArtist(artist)}
                              className="flex min-w-0 flex-1 items-center gap-3 rounded-md px-2 py-2 text-left hover:bg-zinc-800/80"
                            >
                              {openArtist ? (
                                <ChevronDown size={16} className="shrink-0 text-zinc-500" />
                              ) : (
                                <ChevronRight size={16} className="shrink-0 text-zinc-500" />
                              )}
                              <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-zinc-800 text-sm font-semibold text-zinc-300">
                                {artist.name.charAt(0).toUpperCase()}
                              </span>
                              <span className="min-w-0 flex-1 truncate text-sm font-medium">
                                {artist.name}
                              </span>
                            </button>
                            {Array.isArray(expanded) && expanded.length > 0 ? (
                              <button
                                type="button"
                                onClick={() => selectMany(expanded, true)}
                                className="shrink-0 px-2 text-xs text-violet-400 hover:text-violet-300"
                              >
                                All
                              </button>
                            ) : null}
                          </div>
                          {expanded === "loading" ? (
                            <p className="px-8 pb-2 text-xs text-zinc-500">Loading…</p>
                          ) : null}
                          {tracks.length > 0 ? (
                            <ul className="ml-4 max-h-64 overflow-y-auto border-l border-zinc-800 pb-1 pl-1">
                              {tracks.map((track) => (
                                <TrackRow
                                  key={track.id}
                                  track={track}
                                  checked={selected.has(track.id)}
                                  alreadyIn={existingIds.has(track.id)}
                                  onToggle={() => toggleTrack(track)}
                                  compact
                                />
                              ))}
                            </ul>
                          ) : null}
                        </li>
                      );
                    })}
                  </ul>
                </section>
              ) : null}
            </div>
          )}
        </div>

        <div className="flex flex-wrap items-center gap-2 border-t border-zinc-800 px-4 py-3">
          <button
            type="button"
            onClick={selectAllVisible}
            disabled={visibleTracks().length === 0}
            className="text-xs text-zinc-400 hover:text-white disabled:opacity-40"
          >
            Select all
          </button>
          <button
            type="button"
            onClick={clearSelection}
            disabled={selectedCount === 0}
            className="text-xs text-zinc-400 hover:text-white disabled:opacity-40"
          >
            Clear
          </button>
          <div className="ml-auto flex items-center gap-2">
            <button
              type="button"
              onClick={close}
              className="rounded-md border border-zinc-600 px-3 py-1.5 text-sm hover:border-zinc-400"
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={commit}
              disabled={selectedCount === 0}
              className="rounded-md bg-violet-600 px-3 py-1.5 text-sm font-medium hover:bg-violet-500 disabled:opacity-40"
            >
              Add{selectedCount > 0 ? ` ${selectedCount}` : ""}
            </button>
          </div>
        </div>
      </dialog>
    </>
  );
}

function SectionHeader({
  title,
  count,
  onSelectAll,
  onClear,
}: {
  title: string;
  count: number;
  onSelectAll: () => void;
  onClear: () => void;
}) {
  return (
    <div className="mb-1 flex items-center gap-2 px-2">
      <h3 className="text-xs font-semibold uppercase tracking-wide text-zinc-500">
        {title}
        <span className="ml-1 font-normal text-zinc-600">({count})</span>
      </h3>
      <button
        type="button"
        onClick={onSelectAll}
        className="ml-auto text-xs text-violet-400 hover:text-violet-300"
      >
        All
      </button>
      <button type="button" onClick={onClear} className="text-xs text-zinc-500 hover:text-zinc-300">
        None
      </button>
    </div>
  );
}

function TrackRow({
  track,
  checked,
  alreadyIn,
  onToggle,
  compact = false,
}: {
  track: PlayerTrack;
  checked: boolean;
  alreadyIn: boolean;
  onToggle: () => void;
  compact?: boolean;
}) {
  return (
    <li>
      <button
        type="button"
        onClick={onToggle}
        className={`flex w-full items-center gap-3 rounded-md px-2 text-left hover:bg-zinc-800/80 ${
          compact ? "py-1.5" : "py-2"
        }`}
      >
        <span
          className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border ${
            checked ? "border-violet-500 bg-violet-600" : "border-zinc-600"
          }`}
        >
          {checked ? <Check size={12} strokeWidth={3} /> : null}
        </span>
        {!compact ? (
          <span className="h-8 w-8 shrink-0 overflow-hidden rounded bg-zinc-800">
            {track.album.hasArtwork ? (
              // eslint-disable-next-line @next/next/no-img-element
              <img
                src={artworkUrl(track.album.id)}
                alt=""
                className="h-full w-full object-cover"
              />
            ) : null}
          </span>
        ) : null}
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm">{track.title}</span>
          <span className="block truncate text-xs text-zinc-500">
            {track.artist.name}
            {!compact ? ` · ${track.album.title}` : ""}
            {alreadyIn ? " · in playlist" : ""}
          </span>
        </span>
        <span className="shrink-0 text-xs tabular-nums text-zinc-600">
          {formatDuration(track.duration)}
        </span>
      </button>
    </li>
  );
}

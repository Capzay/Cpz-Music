"use client";

import { useRef, useState, useTransition } from "react";
import { ListEnd, ListStart, Music, Play, Plus } from "lucide-react";
import { usePlayerStore } from "@/store/player";
import { formatDuration } from "@/lib/format";
import { artworkUrl, type PlayerTrack } from "@/lib/types";
import { addTracksToPlaylist } from "@/app/(library)/playlists/actions";

export interface PlaylistOption {
  id: number;
  name: string;
}

function Thumb({ track }: { track: PlayerTrack }) {
  return (
    <div className="w-8 h-8 bg-zinc-800 rounded overflow-hidden flex-shrink-0">
      {track.album.hasArtwork ? (
        // eslint-disable-next-line @next/next/no-img-element
        <img src={artworkUrl(track.album.id)} alt="" className="w-full h-full object-cover" />
      ) : null}
    </div>
  );
}

/**
 * `tracks` is the whole context the user clicked into (an album, a playlist, a
 * set of search results). Clicking one queues all of them starting there, which
 * is what people expect from a track list.
 */
export function TrackList({
  tracks,
  playlists = [],
  showAlbum = true,
  numbered = true,
}: {
  tracks: PlayerTrack[];
  playlists?: PlaylistOption[];
  showAlbum?: boolean;
  numbered?: boolean;
}) {
  const dispatch = usePlayerStore((s) => s.dispatch);
  const currentId = usePlayerStore((s) => s.queue[s.index]?.id ?? null);
  const isPlaying = usePlayerStore((s) => s.isPlaying);

  const dialogRef = useRef<HTMLDialogElement>(null);
  const [target, setTarget] = useState<PlayerTrack | null>(null);
  const [pending, startTransition] = useTransition();

  function openPlaylistPicker(track: PlayerTrack) {
    setTarget(track);
    dialogRef.current?.showModal();
  }

  const rowActions = (track: PlayerTrack) => (
    <div className="flex items-center gap-1 opacity-0 transition group-hover:opacity-100">
      <button
        onClick={() => dispatch({ type: "playNext", track })}
        title="Play next"
        aria-label={`Play ${track.title} next`}
        className="p-1 text-zinc-500 hover:text-white transition-colors"
      >
        <ListStart size={14} />
      </button>
      <button
        onClick={() => dispatch({ type: "addToQueue", track })}
        title="Add to queue"
        aria-label={`Add ${track.title} to queue`}
        className="p-1 text-zinc-500 hover:text-white transition-colors"
      >
        <ListEnd size={14} />
      </button>
      {playlists.length > 0 && (
        <button
          onClick={() => openPlaylistPicker(track)}
          title="Add to playlist"
          aria-label={`Add ${track.title} to a playlist`}
          className="p-1 text-zinc-500 hover:text-white transition-colors"
        >
          <Plus size={14} />
        </button>
      )}
    </div>
  );

  return (
    <>
      {/* Desktop table */}
      <table className="hidden md:table w-full">
        <thead>
          <tr className="text-xs text-zinc-500 border-b border-zinc-800">
            {numbered && <th className="py-2 pl-4 pr-2 text-left w-10">#</th>}
            <th className="py-2 pr-4 text-left">Title</th>
            <th className="py-2 pr-4 text-left">Artist</th>
            {showAlbum && <th className="py-2 pr-4 text-left">Album</th>}
            <th className="py-2 pr-2" />
            <th className="py-2 pr-4 text-right">Duration</th>
          </tr>
        </thead>
        <tbody>
          {tracks.map((track, i) => {
            const isCurrent = track.id === currentId;
            return (
              <tr
                key={track.id}
                className={`group cursor-pointer hover:bg-zinc-800/50 ${
                  isCurrent ? "text-violet-400" : ""
                }`}
                onClick={() => dispatch({ type: "setQueue", tracks, startIndex: i })}
              >
                {numbered && (
                  <td className="py-2 pl-4 pr-2 w-10 text-zinc-400 text-sm">
                    <span className="group-hover:hidden">
                      {isCurrent && isPlaying ? (
                        <Music size={14} className="text-violet-400" />
                      ) : (
                        (track.trackNumber ?? i + 1)
                      )}
                    </span>
                    <Play size={14} fill="white" className="hidden text-white group-hover:block" />
                  </td>
                )}
                <td className="py-2 pr-4 text-sm font-medium max-w-xs">
                  <div className="flex items-center gap-3 min-w-0">
                    <Thumb track={track} />
                    <span className="truncate">{track.title}</span>
                  </div>
                </td>
                <td className="py-2 pr-4 text-sm text-zinc-400 truncate max-w-xs">
                  {track.artist.name}
                </td>
                {showAlbum && (
                  <td className="py-2 pr-4 text-sm text-zinc-400 truncate max-w-xs">
                    {track.album.title}
                  </td>
                )}
                <td className="py-2 pr-2" onClick={(e) => e.stopPropagation()}>
                  {rowActions(track)}
                </td>
                <td className="py-2 pr-4 text-sm text-zinc-400 text-right tabular-nums">
                  {formatDuration(track.duration)}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>

      {/* Mobile list */}
      <div className="md:hidden flex flex-col">
        {tracks.map((track, i) => (
          <div
            key={track.id}
            className="flex items-center gap-3 px-3 py-2 hover:bg-zinc-800/50 cursor-pointer"
            onClick={() => dispatch({ type: "setQueue", tracks, startIndex: i })}
          >
            <Thumb track={track} />
            <div className="flex-1 min-w-0">
              <p
                className={`text-sm font-medium truncate ${
                  track.id === currentId ? "text-violet-400" : ""
                }`}
              >
                {track.title}
              </p>
              <p className="text-xs text-zinc-400 truncate">{track.artist.name}</p>
            </div>
            {playlists.length > 0 && (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  openPlaylistPicker(track);
                }}
                aria-label={`Add ${track.title} to a playlist`}
                className="px-1 text-zinc-500"
              >
                <Plus size={16} />
              </button>
            )}
            <span className="text-xs text-zinc-500 tabular-nums">
              {formatDuration(track.duration)}
            </span>
          </div>
        ))}
      </div>

      {/* Native dialog: modal semantics, focus trapping and Escape for free. */}
      <dialog
        ref={dialogRef}
        onClose={() => setTarget(null)}
        className="m-auto w-full max-w-xs rounded-lg border border-zinc-700 bg-zinc-800 p-4 text-white backdrop:bg-black/60"
      >
        <p className="mb-3 truncate text-sm font-medium">Add “{target?.title}” to</p>
        <ul className="max-h-64 space-y-1 overflow-y-auto">
          {playlists.map((playlist) => (
            <li key={playlist.id}>
              <button
                disabled={pending}
                onClick={() => {
                  const track = target;
                  if (!track) return;
                  startTransition(async () => {
                    await addTracksToPlaylist(playlist.id, [track.id]);
                    dialogRef.current?.close();
                  });
                }}
                className="w-full truncate rounded px-3 py-2 text-left text-sm text-zinc-200 hover:bg-zinc-700 disabled:opacity-50"
              >
                {playlist.name}
              </button>
            </li>
          ))}
        </ul>
        <button
          onClick={() => dialogRef.current?.close()}
          className="mt-3 w-full rounded border border-zinc-600 px-3 py-1.5 text-sm hover:border-zinc-400"
        >
          Cancel
        </button>
      </dialog>
    </>
  );
}

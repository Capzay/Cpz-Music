"use client";

import { useRef, useState, useTransition } from "react";
import { usePlayerStore } from "@/store/player";
import { formatDuration } from "@/lib/format";
import type { PlayerTrack } from "@/lib/types";
import { addTracksToPlaylist } from "@/app/(library)/playlists/actions";
import { PlayIcon } from "./Player/icons";

export interface PlaylistOption {
  id: number;
  name: string;
}

/**
 * `tracks` is the whole context the user clicked into (an album, a playlist, a
 * set of search results). Clicking one queues all of them starting there, which
 * is what people expect from a track list.
 */
export function TrackList({
  tracks,
  playlists = [],
  showArtist = false,
  numbered = true,
}: {
  tracks: PlayerTrack[];
  playlists?: PlaylistOption[];
  showArtist?: boolean;
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

  return (
    <>
      <ol className="divide-y divide-neutral-900">
        {tracks.map((track, i) => {
          const isCurrent = track.id === currentId;
          return (
            <li key={track.id}>
              <div className="group flex items-center gap-3 py-2.5 text-sm">
                <button
                  onClick={() => dispatch({ type: "setQueue", tracks, startIndex: i })}
                  className="flex min-w-0 flex-1 items-center gap-4 text-left"
                  aria-label={`Play ${track.title}`}
                >
                  {numbered ? (
                    <span className="w-6 shrink-0 text-right tabular-nums text-neutral-600">
                      <span className="group-hover:hidden">
                        {isCurrent && isPlaying ? (
                          <PlayIcon className="ml-auto h-3 w-3 text-neutral-100" />
                        ) : (
                          (track.trackNumber ?? i + 1)
                        )}
                      </span>
                      <PlayIcon className="ml-auto hidden h-3 w-3 text-neutral-100 group-hover:block" />
                    </span>
                  ) : null}

                  <span className="min-w-0 flex-1">
                    <span
                      className={`block truncate ${
                        isCurrent ? "text-neutral-100" : "text-neutral-200"
                      }`}
                    >
                      {track.title}
                    </span>
                    {showArtist ? (
                      <span className="block truncate text-xs text-neutral-500">
                        {track.artist.name}
                      </span>
                    ) : null}
                  </span>
                </button>

                <div className="flex shrink-0 items-center gap-2 text-xs text-neutral-600 opacity-0 transition group-hover:opacity-100">
                  <button
                    onClick={() => dispatch({ type: "playNext", track })}
                    className="hover:text-neutral-200"
                  >
                    Play next
                  </button>
                  <button
                    onClick={() => dispatch({ type: "addToQueue", track })}
                    className="hover:text-neutral-200"
                  >
                    Queue
                  </button>
                  {playlists.length > 0 ? (
                    <button
                      onClick={() => openPlaylistPicker(track)}
                      className="hover:text-neutral-200"
                    >
                      Add to
                    </button>
                  ) : null}
                </div>

                <span className="w-10 shrink-0 text-right tabular-nums text-neutral-500">
                  {formatDuration(track.duration)}
                </span>
              </div>
            </li>
          );
        })}
      </ol>

      {/* Native dialog: modal semantics, focus trapping and Escape for free. */}
      <dialog
        ref={dialogRef}
        onClose={() => setTarget(null)}
        className="m-auto w-full max-w-xs rounded-lg border border-neutral-800 bg-neutral-900 p-4 text-neutral-100 backdrop:bg-black/60"
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
                className="w-full truncate rounded px-3 py-2 text-left text-sm hover:bg-neutral-800 disabled:opacity-50"
              >
                {playlist.name}
              </button>
            </li>
          ))}
        </ul>
        <button
          onClick={() => dialogRef.current?.close()}
          className="mt-3 w-full rounded border border-neutral-700 px-3 py-1.5 text-sm"
        >
          Cancel
        </button>
      </dialog>
    </>
  );
}

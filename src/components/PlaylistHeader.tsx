"use client";

import { useState } from "react";
import { deletePlaylist, renamePlaylist } from "@/app/(library)/playlists/actions";

export function PlaylistHeader({
  id,
  name,
  count,
}: {
  id: number;
  name: string;
  count: number;
}) {
  const [editing, setEditing] = useState(false);

  return (
    <header className="mb-5">
      {editing ? (
        <form
          action={async (formData) => {
            await renamePlaylist(id, formData);
            setEditing(false);
          }}
          className="flex max-w-md gap-2"
        >
          <input
            name="name"
            defaultValue={name}
            required
            maxLength={120}
            autoFocus
            className="flex-1 rounded-md border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm outline-none focus:border-zinc-600"
          />
          <button className="rounded-md bg-violet-600 px-3 py-2 text-sm font-medium text-white transition-colors hover:bg-violet-500">
            Save
          </button>
          <button
            type="button"
            onClick={() => setEditing(false)}
            className="rounded-md border border-zinc-800 px-3 py-2 text-sm"
          >
            Cancel
          </button>
        </form>
      ) : (
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-semibold tracking-tight">{name}</h1>
          <button
            onClick={() => setEditing(true)}
            className="text-xs text-zinc-500 hover:text-zinc-200"
          >
            Rename
          </button>
          <form action={deletePlaylist.bind(null, id)}>
            <button
              className="text-xs text-zinc-600 hover:text-red-400"
              onClick={(e) => {
                // A playlist is not recoverable, and the button sits next to Rename.
                if (!confirm(`Delete "${name}"?`)) e.preventDefault();
              }}
            >
              Delete
            </button>
          </form>
        </div>
      )}
      <p className="mt-1 text-sm text-zinc-500">{count} tracks</p>
    </header>
  );
}

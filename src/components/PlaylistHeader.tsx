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
            className="flex-1 rounded-md border border-neutral-800 bg-neutral-900 px-3 py-2 text-sm outline-none focus:border-neutral-600"
          />
          <button className="rounded-md bg-neutral-100 px-3 py-2 text-sm font-medium text-neutral-900">
            Save
          </button>
          <button
            type="button"
            onClick={() => setEditing(false)}
            className="rounded-md border border-neutral-800 px-3 py-2 text-sm"
          >
            Cancel
          </button>
        </form>
      ) : (
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-semibold tracking-tight">{name}</h1>
          <button
            onClick={() => setEditing(true)}
            className="text-xs text-neutral-500 hover:text-neutral-200"
          >
            Rename
          </button>
          <form action={deletePlaylist.bind(null, id)}>
            <button
              className="text-xs text-neutral-600 hover:text-red-400"
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
      <p className="mt-1 text-sm text-neutral-500">{count} tracks</p>
    </header>
  );
}

"use client";

import { useState } from "react";
import { usePlaylists } from "@/store/playlists";

export function PlaylistHeader({
  uuid,
  name,
  count,
  onDeleted,
}: {
  uuid: string;
  name: string;
  count: number;
  onDeleted: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const rename = usePlaylists((s) => s.rename);
  const remove = usePlaylists((s) => s.remove);

  return (
    <header className="mb-5">
      {editing ? (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            const next = String(new FormData(event.currentTarget).get("name") ?? "");
            rename(uuid, next);
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
          <button
            className="text-xs text-zinc-600 hover:text-red-400"
            onClick={() => {
              // A playlist is not recoverable, and the button sits next to Rename.
              if (!confirm(`Delete "${name}"?`)) return;
              remove(uuid);
              onDeleted();
            }}
          >
            Delete
          </button>
        </div>
      )}
      <p className="mt-1 text-sm text-zinc-500">{count} tracks</p>
    </header>
  );
}

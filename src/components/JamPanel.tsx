"use client";

import { useRouter } from "next/navigation";
import { useEffect, useTransition } from "react";
import { CopyField } from "./CopyField";
import {
  admitGuest,
  kickGuest,
  newInviteLink,
  startJam,
  stopJam,
} from "@/app/(library)/settings/jam-actions";

interface Participant {
  id: string;
  name: string;
  status: string;
}

export function JamPanel({
  jam,
  inviteUrl,
  participants,
}: {
  jam: { id: string; hostName: string | null; requireApproval: boolean; createdAt: string } | null;
  inviteUrl: string | null;
  participants: Participant[];
}) {
  const [pending, startTransition] = useTransition();
  const router = useRouter();

  // Someone opening the invite link does not notify this page, so poll while a
  // jam is running. Cheap, and only while the panel is actually on screen.
  useEffect(() => {
    if (!jam) return;
    const timer = setInterval(() => router.refresh(), 5000);
    return () => clearInterval(timer);
  }, [jam, router]);

  if (!jam) {
    return (
      <section>
        <h2 className="text-sm font-medium">Jam</h2>
        <p className="mt-1 mb-3 text-sm text-zinc-500">
          Start a jam to let people add tracks to your queue from their own phones. They never see
          your playlists, stats or settings.
        </p>
        <form action={startJam} className="flex max-w-sm gap-2">
          <input
            name="hostName"
            maxLength={40}
            placeholder="Your name (optional)"
            className="min-w-0 flex-1 rounded-md border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm outline-none placeholder:text-zinc-600 focus:border-zinc-600"
          />
          <button className="shrink-0 rounded-md bg-violet-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-violet-500">
            Start jam
          </button>
        </form>
      </section>
    );
  }

  const waiting = participants.filter((p) => p.status === "pending");
  const admitted = participants.filter((p) => p.status === "admitted");

  return (
    <section className={pending ? "opacity-60" : ""}>
      <div className="flex items-center gap-3">
        <h2 className="text-sm font-medium">Jam running</h2>
        <form action={stopJam}>
          <button className="text-xs text-zinc-600 hover:text-red-400">End jam</button>
        </form>
      </div>

      {inviteUrl ? (
        <>
          <p className="mt-1 mb-2 text-sm text-zinc-500">
            Share this link. Anyone who opens it can ask to join.
          </p>
          <CopyField value={inviteUrl} />
          <form action={newInviteLink} className="mt-2">
            <button className="text-xs text-zinc-600 hover:text-zinc-300">
              Replace link (revokes the old one for new joiners)
            </button>
          </form>
        </>
      ) : null}

      {waiting.length > 0 ? (
        <div className="mt-5">
          <h3 className="mb-2 text-xs font-medium text-zinc-400">
            Waiting to be let in ({waiting.length})
          </h3>
          <ul className="space-y-1">
            {waiting.map((p) => (
              <li key={p.id} className="flex items-center gap-2 text-sm">
                <span className="min-w-0 flex-1 truncate">{p.name}</span>
                <button
                  onClick={() => startTransition(() => admitGuest(p.id).catch(() => {}))}
                  className="rounded-full border border-zinc-700 px-3 py-1 text-xs hover:border-zinc-500"
                >
                  Admit
                </button>
                <button
                  onClick={() => startTransition(() => kickGuest(p.id).catch(() => {}))}
                  className="text-xs text-zinc-600 hover:text-red-400"
                >
                  Deny
                </button>
              </li>
            ))}
          </ul>
        </div>
      ) : null}

      <div className="mt-5">
        <h3 className="mb-2 text-xs font-medium text-zinc-400">
          In the jam ({admitted.length})
        </h3>
        {admitted.length === 0 ? (
          <p className="text-sm text-zinc-600">Nobody yet.</p>
        ) : (
          <ul className="space-y-1">
            {admitted.map((p) => (
              <li key={p.id} className="flex items-center gap-2 text-sm">
                <span className="min-w-0 flex-1 truncate">{p.name}</span>
                <button
                  onClick={() => startTransition(() => kickGuest(p.id).catch(() => {}))}
                  className="text-xs text-zinc-600 hover:text-red-400"
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  );
}

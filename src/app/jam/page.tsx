import { prisma } from "@/lib/db";
import { getIdentity } from "@/lib/auth-server";
import { getActiveJam, getParticipant } from "@/lib/jam";
import { toPlayerTrack, trackSelect } from "@/lib/types";
import { GuestApp } from "./GuestApp";
import { JamLive } from "./JamLive";

export const metadata = { title: "Jam" };

const ERRORS: Record<string, string> = {
  invalid_invite: "That invite link is not valid. Ask for a new one.",
  jam_closed: "This jam has ended.",
  removed: "You were removed from this jam.",
  rate_limited: "Too many attempts. Wait a minute and try again.",
  signin_failed: "Could not start a guest session. Try again.",
};

export default async function JamPage(props: PageProps<"/jam">) {
  const { error: rawError } = await props.searchParams;
  const errorKey = Array.isArray(rawError) ? rawError[0] : rawError;
  const error = errorKey ? (ERRORS[errorKey] ?? "Something went wrong.") : null;

  const identity = await getIdentity();

  if (identity.role !== "guest") {
    return (
      <Shell>
        <h1 className="text-lg font-medium">Jam</h1>
        <p className="mt-2 text-sm text-zinc-400">
          {error ?? "Open the invite link you were sent to join."}
        </p>
      </Shell>
    );
  }

  const [jam, participant] = await Promise.all([
    getActiveJam(),
    getParticipant(identity.pid),
  ]);

  if (!jam || jam.id !== identity.jamId) {
    return (
      <Shell>
        <h1 className="text-lg font-medium">Jam ended</h1>
        <p className="mt-2 text-sm text-zinc-400">The host has closed this jam.</p>
      </Shell>
    );
  }

  if (participant?.status === "kicked") {
    return (
      <Shell>
        <h1 className="text-lg font-medium">Removed</h1>
        <p className="mt-2 text-sm text-zinc-400">The host removed you from this jam.</p>
      </Shell>
    );
  }

  if (participant?.status !== "admitted") {
    return (
      <Shell>
        <JamLive />
        <h1 className="text-lg font-medium">Waiting to be let in</h1>
        <p className="mt-2 text-sm text-zinc-400">
          {identity.name}, the host has to admit you before you can add tracks.
        </p>
      </Shell>
    );
  }

  // Guests browse a capped slice rather than the whole library: the page is
  // server-rendered per request, and someone else's party guest has no reason
  // to be handed a full inventory of the library.
  const albums = await prisma.album.findMany({
    orderBy: { title: "asc" },
    take: 60,
    select: {
      id: true,
      title: true,
      artworkPath: true,
      artist: { select: { name: true } },
      tracks: { orderBy: { trackNumber: "asc" }, take: 30, select: trackSelect },
    },
  });

  return (
    <>
      <JamLive showNowPlaying />
      <GuestApp
        name={identity.name}
        hostName={jam.hostName}
        albums={albums.map((album) => ({
          id: album.id,
          title: album.title,
          artist: album.artist.name,
          hasArtwork: Boolean(album.artworkPath),
          tracks: album.tracks.map(toPlayerTrack),
        }))}
      />
    </>
  );
}

function Shell({ children }: { children: React.ReactNode }) {
  return (
    <main className="flex min-h-dvh items-center justify-center p-6 text-center">
      <div className="max-w-sm">{children}</div>
    </main>
  );
}

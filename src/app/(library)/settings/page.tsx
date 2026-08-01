import { headers } from "next/headers";
import { requireHost } from "@/lib/auth-server";
import { signToken } from "@/lib/tokens";
import { requestOrigin } from "@/lib/origin";
import { getActiveJam, listParticipants, signInvite } from "@/lib/jam";
import { CopyField } from "@/components/CopyField";
import { JamPanel } from "@/components/JamPanel";

export const metadata = { title: "Settings" };

export default async function SettingsPage() {
  await requireHost();

  const origin = requestOrigin(await headers());

  // A year: an OBS scene is set up once and left alone. Rotating APP_SECRET
  // invalidates every outstanding overlay link at once.
  let overlayUrl: string | null = null;
  let secretMissing = false;
  try {
    overlayUrl = `${origin}/obs?token=${signToken("obs", {}, 365 * 24 * 60 * 60)}`;
  } catch {
    secretMissing = true;
  }

  const jam = await getActiveJam();
  const participants = jam ? await listParticipants(jam.id) : [];
  const inviteUrl =
    jam && !secretMissing
      ? `${origin}/jam/join?token=${signInvite(jam.id, jam.inviteEpoch)}`
      : null;

  return (
    <div className="max-w-2xl space-y-10">
      <h1 className="text-xl font-bold md:text-2xl">Settings</h1>

      {secretMissing ? (
        <p className="rounded-md border border-red-900 bg-red-950/40 p-3 text-sm text-red-300">
          APP_SECRET is not set, so invite and overlay links cannot be signed. Generate one with{" "}
          <code>openssl rand -hex 32</code> and restart.
        </p>
      ) : null}

      <JamPanel
        jam={
          jam
            ? {
                id: jam.id,
                hostName: jam.hostName,
                requireApproval: jam.requireApproval,
                createdAt: jam.createdAt.toISOString(),
              }
            : null
        }
        inviteUrl={inviteUrl}
        participants={participants.map((p) => ({
          id: p.id,
          name: p.name,
          status: p.status,
        }))}
      />

      <section>
        <h2 className="text-sm font-medium">OBS overlay</h2>
        <p className="mt-1 mb-3 text-sm text-zinc-500">
          Add this as a browser source in OBS. It shows the current track over a transparent
          background. The link carries its own signed token, because a browser source cannot sign
          in. Treat it as a password.
        </p>
        {overlayUrl ? <CopyField value={overlayUrl} /> : null}
      </section>
    </div>
  );
}

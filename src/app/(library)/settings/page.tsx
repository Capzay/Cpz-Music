import { headers } from "next/headers";
import { requireHost } from "@/lib/auth-server";
import { signToken } from "@/lib/tokens";
import { CopyField } from "@/components/CopyField";

export const metadata = { title: "Settings" };

export default async function SettingsPage() {
  await requireHost();

  const headerList = await headers();
  const host = headerList.get("host") ?? "localhost:3000";
  const protocol = headerList.get("x-forwarded-proto") ?? "http";

  // A year: an OBS scene is set up once and left alone. Rotating APP_SECRET
  // invalidates every outstanding overlay link at once.
  let overlayUrl: string | null = null;
  let error: string | null = null;
  try {
    const token = signToken("obs", {}, 365 * 24 * 60 * 60);
    overlayUrl = `${protocol}://${host}/obs?token=${token}`;
  } catch {
    error = "APP_SECRET is not set, so overlay links cannot be signed.";
  }

  return (
    <>
      <h1 className="mb-5 text-xl font-semibold tracking-tight">Settings</h1>

      <section className="max-w-2xl">
        <h2 className="text-sm font-medium">OBS overlay</h2>
        <p className="mt-1 mb-3 text-sm text-neutral-500">
          Add this as a browser source in OBS. It shows the current track over a transparent
          background. The link carries its own signed token, because a browser source cannot sign
          in. Treat it as a password.
        </p>
        {overlayUrl ? (
          <CopyField value={overlayUrl} />
        ) : (
          <p className="text-sm text-red-400">{error}</p>
        )}
      </section>
    </>
  );
}

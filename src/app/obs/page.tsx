import { verifyToken } from "@/lib/tokens";
import { getIdentity } from "@/lib/auth-server";
import { Overlay } from "./Overlay";

export const metadata = { title: "Now playing overlay" };

/**
 * Streaming overlay, added to OBS as a browser source. The token in the URL is
 * the only credential, so it is verified here before any markup is produced.
 */
export default async function ObsPage(props: PageProps<"/obs">) {
  const { token } = await props.searchParams;
  const value = Array.isArray(token) ? token[0] : token;

  const identity = await getIdentity();
  if (identity.role !== "host" && !verifyToken(value, "obs")) {
    return (
      <main className="p-6 text-sm text-neutral-500">
        This overlay link is missing or expired. Generate a new one from Settings.
      </main>
    );
  }

  return <Overlay token={value ?? ""} />;
}

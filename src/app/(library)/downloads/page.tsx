import { DownloadsList } from "@/components/DownloadsList";

export const metadata = { title: "Downloads" };

/**
 * Entirely client-rendered: what is downloaded lives in this device's Cache
 * Storage, and the page has to work with the server unreachable.
 */
export default function DownloadsPage() {
  return (
    <>
      <h1 className="mb-5 text-xl font-semibold tracking-tight">Downloads</h1>
      <DownloadsList />
    </>
  );
}

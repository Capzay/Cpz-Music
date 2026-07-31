import { DownloadsList } from "@/components/DownloadsList";

export const metadata = { title: "Downloads" };

/**
 * Entirely client-rendered: what is downloaded lives in this device's Cache
 * Storage, and the page has to work with the server unreachable.
 */
export default function DownloadsPage() {
  return (
    <>
      <h1 className="text-xl font-bold mb-4 md:text-2xl md:mb-6">Downloads</h1>
      <DownloadsList />
    </>
  );
}

import { useDownloads } from "@/store/downloads";
import type { PlayerTrack } from "@/lib/types";

/** Online, every track can stream. Offline, only a downloaded copy can play. */
export function canPlayTrack(online: boolean, downloaded: boolean): boolean {
  return online || downloaded;
}

/**
 * When offline, drop tracks that were never downloaded so the player does not
 * race through a queue of failing streams. Online, the list is unchanged.
 */
export function queueForPlayback(
  tracks: PlayerTrack[],
  startIndex = 0,
): { tracks: PlayerTrack[]; startIndex: number } {
  if (typeof navigator === "undefined" || navigator.onLine) {
    return { tracks, startIndex };
  }

  const registry = useDownloads.getState().registry;
  const playable = tracks.filter((t) => registry[t.id]);
  if (playable.length === 0) return { tracks: [], startIndex: 0 };

  const clickedId = tracks[startIndex]?.id;
  let index = clickedId != null ? playable.findIndex((t) => t.id === clickedId) : 0;
  if (index < 0) {
    index = 0;
    for (let i = startIndex + 1; i < tracks.length; i++) {
      const j = playable.findIndex((t) => t.id === tracks[i]!.id);
      if (j >= 0) {
        index = j;
        break;
      }
    }
  }

  return { tracks: playable, startIndex: index };
}

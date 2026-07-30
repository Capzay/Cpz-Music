import { artworkUrl, type PlayerTrack } from "@/lib/types";

declare global {
  interface Window {
    /** Injected by the Electron preload script. Absent in a normal browser. */
    cpzMusic?: { updatePresence: (presence: DesktopPresence | null) => void };
  }
}

export interface DesktopPresence {
  title: string;
  artist: string;
  album: string;
  artwork: string;
  isPlaying: boolean;
}

/**
 * Hands the Discord Rich Presence data to the Electron shell.
 *
 * The old build could not do this: the desktop wrapper loaded the site as a
 * black box, so it injected a script that patched the MediaSession prototype
 * setters to spy on metadata changes. Both sides are ours now, so the app just
 * says what it is playing and roughly eighty lines of injected JavaScript go
 * away with it.
 */
function notifyDesktop(track: PlayerTrack | null, isPlaying: boolean) {
  if (typeof window === "undefined" || !window.cpzMusic) return;

  if (!track) return window.cpzMusic.updatePresence(null);

  window.cpzMusic.updatePresence({
    title: track.title,
    artist: track.artist.name,
    album: track.album.title,
    artwork: track.album.hasArtwork
      ? new URL(artworkUrl(track.album.id), window.location.href).toString()
      : "",
    isPlaying,
  });
}

/**
 * Updates the OS-level now-playing card (lock screen, media keys, notification
 * shade). Must be callable synchronously from the audio `ended` handler: on
 * Android, deferring it to a React effect lets the lock screen show the
 * previous track until Chrome gets around to running the effect.
 */
export function updateMediaSession(track: PlayerTrack | null, isPlaying: boolean) {
  notifyDesktop(track, isPlaying);

  if (typeof navigator === "undefined" || !("mediaSession" in navigator)) return;

  navigator.mediaSession.playbackState = isPlaying ? "playing" : "paused";
  if (!track) {
    navigator.mediaSession.metadata = null;
    return;
  }

  navigator.mediaSession.metadata = new MediaMetadata({
    title: track.title,
    artist: track.artist.name,
    album: track.album.title,
    artwork: track.album.hasArtwork
      ? [{ src: artworkUrl(track.album.id), sizes: "512x512", type: "image/jpeg" }]
      : [],
  });
}

export function setMediaSessionHandlers(handlers: {
  play: () => void;
  pause: () => void;
  next: () => void;
  prev: () => void;
  seek: (time: number) => void;
}) {
  if (typeof navigator === "undefined" || !("mediaSession" in navigator)) return;
  const ms = navigator.mediaSession;
  ms.setActionHandler("play", handlers.play);
  ms.setActionHandler("pause", handlers.pause);
  ms.setActionHandler("nexttrack", handlers.next);
  ms.setActionHandler("previoustrack", handlers.prev);
  ms.setActionHandler("seekto", (details) => {
    if (details.seekTime != null) handlers.seek(details.seekTime);
  });
}

export function setPositionState(duration: number, position: number) {
  if (typeof navigator === "undefined" || !("mediaSession" in navigator)) return;
  if (!Number.isFinite(duration) || duration <= 0) return;
  try {
    navigator.mediaSession.setPositionState({
      duration,
      position: Math.min(position, duration),
      playbackRate: 1,
    });
  } catch {
    // Safari throws on out-of-range values it disagrees with. Not worth failing over.
  }
}

import { artworkUrl, type PlayerTrack } from "@/lib/types";

declare global {
  interface Window {
    /** Injected by the Electron preload script. Absent in a normal browser. */
    cpzMusic?: { updatePresence: (presence: DesktopPresence | null) => void };
    /**
     * Injected by Capacitor inside the Android WebView. Reached through the
     * global rather than by importing @capacitor/core, so the web app carries
     * no dependency that only one wrapper needs.
     */
    Capacitor?: {
      Plugins?: {
        NativeMediaSession?: {
          update: (state: NativeMediaState) => Promise<void>;
        };
      };
    };
    /** Called by the Android plugin when a lock-screen button is pressed. */
    __cpzMediaAction?: (action: string, seekTime?: number) => void;
  }
}

interface NativeMediaState {
  title: string;
  artist: string;
  album: string;
  artworkUrl: string;
  playing: boolean;
  position: number;
  duration: number;
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
export function updateMediaSession(
  track: PlayerTrack | null,
  isPlaying: boolean,
  position = 0,
) {
  notifyDesktop(track, isPlaying);
  notifyAndroid(track, isPlaying, position);

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

/**
 * Drives the Android lock screen through the native plugin.
 *
 * Chrome's own MediaSession is not enough inside a WebView: Android suspends
 * the WebView when the screen locks, the session goes away with it, and the
 * lock-screen controls vanish mid-track. A real MediaSessionCompat owned by a
 * foreground service survives that.
 */
function notifyAndroid(track: PlayerTrack | null, isPlaying: boolean, position: number) {
  const plugin = typeof window === "undefined" ? undefined : window.Capacitor?.Plugins?.NativeMediaSession;
  if (!plugin) return;

  void plugin
    .update({
      title: track?.title ?? "",
      artist: track?.artist.name ?? "",
      album: track?.album.title ?? "",
      artworkUrl:
        track && track.album.hasArtwork
          ? new URL(artworkUrl(track.album.id), window.location.href).toString()
          : "",
      playing: isPlaying,
      position,
      duration: track?.duration ?? 0,
    })
    .catch(() => {
      // Plugin missing or the activity is going away. The web MediaSession
      // below still runs, so this degrades rather than breaks.
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

  // The Android plugin calls this via evaluateJavascript, which keeps working
  // while the activity is stopped. Capacitor's own event bridge does not.
  if (typeof window !== "undefined") {
    window.__cpzMediaAction = (action, seekTime) => {
      switch (action) {
        case "play":
          return handlers.play();
        case "pause":
          return handlers.pause();
        case "next":
          return handlers.next();
        case "previous":
          return handlers.prev();
        case "seek":
          if (typeof seekTime === "number") handlers.seek(seekTime);
      }
    };
  }
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

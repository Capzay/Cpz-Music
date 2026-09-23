"use client";

import { useCallback, useEffect, useRef } from "react";
import { usePlayerStore } from "@/store/player";
import { useDownloads } from "@/store/downloads";
import { streamUrl } from "@/lib/types";
import { nextPosition } from "@/lib/queue";
import {
  offlineObjectUrl,
  peekOfflineUrl,
  playbackSrc,
  preferCachedAudio,
  retainOfflineUrls,
} from "@/lib/offline-audio";
import {
  setMediaSessionHandlers,
  setPositionState,
  updateMediaSession,
} from "@/lib/media-session";
import { reportListen, startListenQueue } from "@/lib/listen-queue";
import { startPlaylistSync } from "@/store/playlists";

/** One element for the whole app; a second one would play over the first. */
let element: HTMLAudioElement | null = null;

export function audioElement(): HTMLAudioElement | null {
  if (typeof window === "undefined") return null;
  if (!element) {
    element = new Audio();
    element.preload = "auto";
  }
  return element;
}

function isDownloaded(trackId: number): boolean {
  return !!useDownloads.getState().registry[trackId];
}

function upcomingTrackId(): number | null {
  const s = usePlayerStore.getState();
  const advance = nextPosition({
    length: s.queue.length,
    index: s.index,
    shuffle: s.shuffle,
    shuffleOrder: s.shuffleOrder,
    shufflePos: s.shufflePos,
    repeat: s.repeat,
  });
  if (advance.kind !== "move" || advance.index < 0) return null;
  return s.queue[advance.index]?.id ?? null;
}

function warmUpcoming() {
  const id = upcomingTrackId();
  if (id == null || !preferCachedAudio(isDownloaded(id))) return;
  void offlineObjectUrl(id);
}

export function useAudio() {
  const dispatch = usePlayerStore((s) => s.dispatch);
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const volume = usePlayerStore((s) => s.volume);
  const seekTarget = usePlayerStore((s) => s.seekTarget);
  const trackId = usePlayerStore((s) => s.queue[s.index]?.id ?? null);
  const isActiveDevice = usePlayerStore((s) => s.isActiveDevice);

  const loadedTrackId = useRef<number | null>(null);
  /** Track id whose bytes are actually assigned to the element. */
  const srcFor = useRef<number | null>(null);
  /** Track id whose cache read is in flight, so a second effect does not start another. */
  const loadingId = useRef<number | null>(null);
  /** Stream URL already failed for this id; the next error is the cached copy. */
  const blobAttempt = useRef<number | null>(null);
  const consecutiveErrors = useRef(0);
  const lastNativePush = useRef(0);

  const commitSrc = useCallback((audio: HTMLAudioElement, id: number, src: string) => {
    srcFor.current = id;
    loadingId.current = null;
    audio.src = src;
    audio.load();
    if (usePlayerStore.getState().isPlaying) void audio.play().catch(() => {});
    const next = upcomingTrackId();
    const keep = src.startsWith("blob:") ? [id] : [];
    if (next != null) keep.push(next);
    retainOfflineUrls(keep);
    warmUpcoming();
  }, []);

  const beginTrack = useCallback(
    (audio: HTMLAudioElement, id: number, isCancelled: () => boolean) => {
      loadedTrackId.current = id;
      loadingId.current = id;

      const apply = (src: string) => {
        if (isCancelled() || loadedTrackId.current !== id) return;
        commitSrc(audio, id, src);
      };

      const ready = peekOfflineUrl(id);
      if (ready) {
        apply(ready);
        return;
      }

      if (!preferCachedAudio(isDownloaded(id))) {
        apply(streamUrl(id));
        return;
      }

      void playbackSrc(id, true).then(apply);
    },
    [commitSrc],
  );

  // Restore the saved volume once, on the client, so SSR and hydration agree.
  useEffect(() => {
    // getItem returns null with nothing stored, and Number(null) is 0, which
    // would pass the range check below and mute every fresh session.
    const stored = localStorage.getItem("cpz-volume");
    const saved = Number(stored);
    if (stored && Number.isFinite(saved) && saved >= 0 && saved <= 1) {
      usePlayerStore.setState({ volume: saved });
    }
    startListenQueue();
    startPlaylistSync();
    useDownloads.getState().hydrate();
  }, []);

  // Load whenever the track changes. A remote mirrors state on screen but must
  // never fetch or play audio, or two devices would be making noise at once.
  useEffect(() => {
    const audio = audioElement();
    if (!audio || trackId == null || !isActiveDevice) return;
    if (srcFor.current === trackId || loadingId.current === trackId) return;

    // Bank whatever played of the outgoing track before currentTime resets.
    if (loadedTrackId.current !== null && loadedTrackId.current !== trackId) {
      reportListen(loadedTrackId.current, audio.currentTime, false);
    }

    let cancelled = false;
    beginTrack(audio, trackId, () => cancelled);
    return () => {
      cancelled = true;
      if (loadingId.current === trackId && srcFor.current !== trackId) {
        loadingId.current = null;
      }
    };
  }, [trackId, isActiveDevice, beginTrack]);

  useEffect(() => {
    const audio = audioElement();
    if (!audio) return;

    if (!isActiveDevice) {
      audio.pause();
      // Drop the source so handing playback away also stops buffering.
      audio.removeAttribute("src");
      loadedTrackId.current = null;
      srcFor.current = null;
      loadingId.current = null;
      updateMediaSession(null, false);
      return;
    }

    // A cached track assigns its source asynchronously. Playing before that
    // resumes whatever file was loaded last.
    if (isPlaying) {
      if (srcFor.current === trackId) void audio.play().catch(() => {});
    } else {
      audio.pause();
    }

    const s = usePlayerStore.getState();
    updateMediaSession(s.queue[s.index] ?? null, isPlaying);
  }, [isPlaying, trackId, isActiveDevice]);

  useEffect(() => {
    const audio = audioElement();
    if (audio) audio.volume = volume;
  }, [volume]);

  useEffect(() => {
    const audio = audioElement();
    if (!audio || seekTarget === null) return;
    audio.currentTime = seekTarget;
    usePlayerStore.getState().clearSeekTarget();
  }, [seekTarget]);

  useEffect(() => {
    setMediaSessionHandlers({
      play: () => dispatch({ type: "play" }),
      pause: () => dispatch({ type: "pause" }),
      next: () => dispatch({ type: "next" }),
      prev: () => dispatch({ type: "prev" }),
      seek: (time) => dispatch({ type: "seek", time }),
    });
  }, [dispatch]);

  useEffect(() => {
    const audio = audioElement();
    if (!audio) return;

    const onTimeUpdate = () => {
      usePlayerStore.getState().setCurrentTime(audio.currentTime);
      setPositionState(audio.duration, audio.currentTime);

      // The Android lock screen needs the position pushed to it, but timeupdate
      // fires several times a second and every call crosses the native bridge.
      const now = Date.now();
      if (now - lastNativePush.current > 5000) {
        lastNativePush.current = now;
        const s = usePlayerStore.getState();
        updateMediaSession(s.queue[s.index] ?? null, s.isPlaying, audio.currentTime);
      }
    };

    const onDurationChange = () => {
      usePlayerStore.getState().setDuration(Number.isFinite(audio.duration) ? audio.duration : 0);
    };

    const onEnded = () => {
      const finished = usePlayerStore.getState().queue[usePlayerStore.getState().index];
      if (finished) reportListen(finished.id, audio.duration || 0, true);

      dispatch({ type: "next" });

      // Load and play the next track right here rather than waiting for the
      // effect above. iOS and Android only keep the lock-screen session alive if
      // play() happens synchronously inside the ended handler.
      const s = usePlayerStore.getState();
      const next = s.queue[s.index] ?? null;
      if (next && s.isPlaying) {
        updateMediaSession(next, true);
        // play() stays inside this turn when the next file was already warmed,
        // which is what keeps the lock-screen session alive.
        beginTrack(audio, next.id, () => false);
      }
    };

    // Offline, a track that was never downloaded fails to load. Skip past it so a
    // partly-downloaded queue keeps playing, but give up after a full lap so an
    // entirely undownloaded queue does not spin. A downloaded track that still
    // errors after the cached bytes have been tried is a real failure.
    const onError = () => {
      if (!audio.src) return;
      const s = usePlayerStore.getState();
      const failed = s.queue[s.index];
      const downloaded = failed != null && isDownloaded(failed.id);
      if (
        failed &&
        downloaded &&
        !audio.src.startsWith("blob:") &&
        blobAttempt.current !== failed.id
      ) {
        blobAttempt.current = failed.id;
        void offlineObjectUrl(failed.id).then((local) => {
          if (loadedTrackId.current !== failed.id) return;
          if (!local) {
            usePlayerStore.setState({ isPlaying: false });
            return;
          }
          commitSrc(audio, failed.id, local);
        });
        return;
      }
      if (navigator.onLine || downloaded) {
        consecutiveErrors.current = 0;
        usePlayerStore.setState({ isPlaying: false });
        return;
      }
      consecutiveErrors.current += 1;
      const limit = Math.min(s.queue.length || 1, 30);
      if (consecutiveErrors.current >= limit) {
        consecutiveErrors.current = 0;
        usePlayerStore.setState({ isPlaying: false });
        return;
      }
      dispatch({ type: "next" });
    };

    const onPlaying = () => {
      consecutiveErrors.current = 0;
      blobAttempt.current = null;
    };

    audio.addEventListener("timeupdate", onTimeUpdate);
    audio.addEventListener("durationchange", onDurationChange);
    audio.addEventListener("ended", onEnded);
    audio.addEventListener("error", onError);
    audio.addEventListener("playing", onPlaying);

    return () => {
      audio.removeEventListener("timeupdate", onTimeUpdate);
      audio.removeEventListener("durationchange", onDurationChange);
      audio.removeEventListener("ended", onEnded);
      audio.removeEventListener("error", onError);
      audio.removeEventListener("playing", onPlaying);
    };
  }, [dispatch, beginTrack, commitSrc]);
}

"use client";

import { useEffect, useRef } from "react";
import { usePlayerStore } from "@/store/player";
import { streamUrl } from "@/lib/types";
import {
  setMediaSessionHandlers,
  setPositionState,
  updateMediaSession,
} from "@/lib/media-session";
import { reportListen, startListenQueue } from "@/lib/listen-queue";

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

export function useAudio() {
  const dispatch = usePlayerStore((s) => s.dispatch);
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const volume = usePlayerStore((s) => s.volume);
  const seekTarget = usePlayerStore((s) => s.seekTarget);
  const trackId = usePlayerStore((s) => s.queue[s.index]?.id ?? null);
  const isActiveDevice = usePlayerStore((s) => s.isActiveDevice);

  const loadedTrackId = useRef<number | null>(null);
  const consecutiveErrors = useRef(0);

  // Restore the saved volume once, on the client, so SSR and hydration agree.
  useEffect(() => {
    const saved = Number(localStorage.getItem("cpz-volume"));
    if (Number.isFinite(saved) && saved >= 0 && saved <= 1) {
      usePlayerStore.setState({ volume: saved });
    }
    startListenQueue();
  }, []);

  // Load whenever the track changes. A remote mirrors state on screen but must
  // never fetch or play audio, or two devices would be making noise at once.
  useEffect(() => {
    const audio = audioElement();
    if (!audio || trackId == null || !isActiveDevice) return;
    if (trackId === loadedTrackId.current) return;

    // Bank whatever played of the outgoing track before currentTime resets.
    if (loadedTrackId.current !== null) {
      reportListen(loadedTrackId.current, audio.currentTime, false);
    }

    loadedTrackId.current = trackId;
    audio.src = streamUrl(trackId);
    audio.load();
    if (usePlayerStore.getState().isPlaying) void audio.play().catch(() => {});
  }, [trackId, isActiveDevice]);

  useEffect(() => {
    const audio = audioElement();
    if (!audio) return;

    if (!isActiveDevice) {
      audio.pause();
      // Drop the source so handing playback away also stops buffering.
      audio.removeAttribute("src");
      loadedTrackId.current = null;
      updateMediaSession(null, false);
      return;
    }

    if (isPlaying) void audio.play().catch(() => {});
    else audio.pause();

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
        loadedTrackId.current = next.id;
        updateMediaSession(next, true);
        audio.src = streamUrl(next.id);
        void audio.play().catch(() => {});
      }
    };

    // Offline, a track that was never downloaded fails to load. Skip past it so a
    // partly-downloaded queue keeps playing, but give up after a full lap so an
    // entirely undownloaded queue does not spin.
    const onError = () => {
      if (!audio.src) return;
      consecutiveErrors.current += 1;
      const s = usePlayerStore.getState();
      const limit = Math.min(s.queue.length || 1, 30);
      if (navigator.onLine || consecutiveErrors.current >= limit) {
        consecutiveErrors.current = 0;
        usePlayerStore.setState({ isPlaying: false });
        return;
      }
      dispatch({ type: "next" });
    };

    const onPlaying = () => {
      consecutiveErrors.current = 0;
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
  }, [dispatch]);
}

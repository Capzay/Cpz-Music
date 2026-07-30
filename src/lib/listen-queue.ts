"use client";

/**
 * Listens that could not reach the server (offline playback of downloaded
 * tracks, flaky connections) are queued and replayed with their original
 * timestamp, so they land on the day they actually happened.
 */

interface QueuedListen {
  id: string;
  trackId: number;
  durationSecs: number;
  at: number;
}

const KEY = "cpz-listen-queue-v1";
const MAX_QUEUE = 500;
/** Below this, a play was a skip rather than a listen. */
const MIN_LISTEN_SECS = 30;

function load(): QueuedListen[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function save(queue: QueuedListen[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(queue.slice(-MAX_QUEUE)));
  } catch {
    // Quota. Stats are best-effort by design.
  }
}

async function send(listen: Omit<QueuedListen, "id">) {
  const response = await fetch("/api/stats/listen", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(listen),
  });
  // 4xx means the server will never accept this event; treat it as done rather
  // than letting one bad row block the whole queue forever.
  if (!response.ok && response.status >= 500) throw new Error(`HTTP ${response.status}`);
}

export function reportListen(trackId: number, durationSecs: number, completed: boolean) {
  if (durationSecs < MIN_LISTEN_SECS && !completed) return;

  const listen = { trackId, durationSecs: Math.round(durationSecs), at: Date.now() };

  if (!navigator.onLine) return enqueue(listen);
  void send(listen).catch(() => enqueue(listen));
}

function enqueue(listen: Omit<QueuedListen, "id">) {
  const queue = load();
  queue.push({ ...listen, id: `${Date.now()}-${Math.random().toString(36).slice(2)}` });
  save(queue);
}

let flushing = false;

export async function flushListenQueue() {
  if (flushing || !navigator.onLine) return;
  flushing = true;
  try {
    for (;;) {
      const queue = load();
      if (queue.length === 0) return;
      const next = queue[0];
      try {
        await send({ trackId: next.trackId, durationSecs: next.durationSecs, at: next.at });
      } catch {
        // Still unreachable. Keep the queue and retry on the next online event.
        return;
      }
      save(load().filter((item) => item.id !== next.id));
    }
  } finally {
    flushing = false;
  }
}

export function startListenQueue() {
  window.addEventListener("online", () => void flushListenQueue());
  void flushListenQueue();
}

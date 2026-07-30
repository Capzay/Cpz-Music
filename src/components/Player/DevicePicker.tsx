"use client";

import { useRef } from "react";
import { usePlayerStore } from "@/store/player";
import { claimPlayback, renameDevice } from "@/hooks/useSync";
import { loadDeviceId } from "@/lib/realtime";

export function DevicePicker() {
  const devices = usePlayerStore((s) => s.devices);
  const activeDeviceId = usePlayerStore((s) => s.activeDeviceId);
  const isActiveDevice = usePlayerStore((s) => s.isActiveDevice);
  const dialogRef = useRef<HTMLDialogElement>(null);

  // Nothing to choose between until a second device shows up.
  if (devices.length < 2) return null;

  const active = devices.find((d) => d.deviceId === activeDeviceId);

  return (
    <>
      <button
        onClick={() => dialogRef.current?.showModal()}
        className={`text-xs ${isActiveDevice ? "text-neutral-500" : "text-emerald-400"} hover:text-neutral-200`}
        aria-label="Choose playback device"
      >
        {isActiveDevice ? "This device" : (active?.name ?? "Remote")}
      </button>

      <dialog
        ref={dialogRef}
        className="m-auto w-full max-w-xs rounded-lg border border-neutral-800 bg-neutral-900 p-4 text-neutral-100 backdrop:bg-black/60"
      >
        <p className="mb-3 text-sm font-medium">Play on</p>
        <ul className="space-y-1">
          {devices.map((device) => {
            const isSelf = device.deviceId === loadDeviceId();
            return (
              <li key={device.deviceId}>
                <button
                  onClick={async () => {
                    // Only this device can claim for itself; a remote asks by
                    // sending commands, not by rewriting someone else's presence.
                    if (isSelf) await claimPlayback();
                    dialogRef.current?.close();
                  }}
                  disabled={!isSelf}
                  className={`w-full rounded px-3 py-2 text-left text-sm hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-40 ${
                    device.deviceId === activeDeviceId ? "text-emerald-400" : ""
                  }`}
                >
                  <span className="block truncate">
                    {device.name}
                    {isSelf ? " (this device)" : ""}
                  </span>
                  <span className="block text-xs text-neutral-500">{device.platform}</span>
                </button>
              </li>
            );
          })}
        </ul>

        <form
          className="mt-3 flex gap-2"
          action={async (formData) => {
            await renameDevice(String(formData.get("name") ?? ""));
          }}
        >
          <input
            name="name"
            placeholder="Rename this device"
            maxLength={40}
            className="min-w-0 flex-1 rounded border border-neutral-700 bg-neutral-950 px-2 py-1.5 text-xs outline-none focus:border-neutral-500"
          />
          <button className="rounded border border-neutral-700 px-2 py-1.5 text-xs">Save</button>
        </form>

        <button
          onClick={() => dialogRef.current?.close()}
          className="mt-2 w-full rounded border border-neutral-700 px-3 py-1.5 text-sm"
        >
          Close
        </button>
      </dialog>
    </>
  );
}

"use client";

import { useState } from "react";
import { Cast, Monitor, MonitorSpeaker, Smartphone } from "lucide-react";
import { usePlayerStore } from "@/store/player";
import { claimPlayback, renameDevice } from "@/hooks/useSync";
import { loadDeviceId } from "@/lib/realtime";

function DeviceIcon({ name, size = 16 }: { name: string; size?: number }) {
  if (/mobile|android|phone|ios/i.test(name)) return <Smartphone size={size} />;
  if (/desktop|pc|windows|mac|linux/i.test(name)) return <Monitor size={size} />;
  return <MonitorSpeaker size={size} />;
}

export function DevicePicker() {
  const devices = usePlayerStore((s) => s.devices);
  const activeDeviceId = usePlayerStore((s) => s.activeDeviceId);
  const isActiveDevice = usePlayerStore((s) => s.isActiveDevice);
  const [open, setOpen] = useState(false);

  // Nothing to choose between until a second device shows up.
  if (devices.length < 2) return null;

  return (
    <div className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        title="Devices"
        aria-label="Choose playback device"
        className={`min-w-[44px] min-h-[44px] flex items-center justify-center transition-colors ${
          isActiveDevice ? "text-zinc-400 hover:text-white" : "text-violet-400"
        }`}
      >
        <Cast size={18} />
      </button>

      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />

          <div className="absolute bottom-full right-0 mb-2 w-64 bg-zinc-800 border border-zinc-700 rounded-xl shadow-xl z-50 overflow-hidden">
            <p className="text-xs font-semibold text-zinc-400 uppercase tracking-wider px-4 pt-3 pb-1">
              Devices
            </p>

            {devices.map((device) => {
              const isActive = device.deviceId === activeDeviceId;
              const isSelf = device.deviceId === loadDeviceId();

              return (
                <div
                  key={device.deviceId}
                  className={`flex items-center gap-3 px-4 py-3 ${isActive ? "bg-zinc-700/50" : ""}`}
                >
                  <span className={`flex-shrink-0 ${isActive ? "text-white" : "text-zinc-400"}`}>
                    <DeviceIcon name={device.name} />
                  </span>

                  <div className="flex-1 min-w-0">
                    <p
                      className={`text-sm font-medium truncate ${
                        isActive ? "text-white" : "text-zinc-300"
                      }`}
                    >
                      {device.name}
                      {isSelf && <span className="ml-1 text-xs text-zinc-500">(this device)</span>}
                    </p>
                    {isActive && <p className="text-xs text-violet-400">Playing</p>}
                  </div>

                  {/* Only this device can claim for itself; a remote asks by
                      sending commands, not by rewriting someone else's presence. */}
                  {!isActive && isSelf && (
                    <button
                      onClick={async () => {
                        await claimPlayback();
                        setOpen(false);
                      }}
                      className="text-xs text-violet-400 hover:text-violet-300 flex-shrink-0 transition-colors"
                    >
                      Listen here
                    </button>
                  )}
                </div>
              );
            })}

            <form
              className="flex gap-2 border-t border-zinc-700 p-3"
              action={async (formData) => {
                await renameDevice(String(formData.get("name") ?? ""));
              }}
            >
              <input
                name="name"
                placeholder="Rename this device"
                maxLength={40}
                className="min-w-0 flex-1 rounded border border-zinc-600 bg-zinc-900 px-2 py-1.5 text-xs outline-none focus:border-violet-500"
              />
              <button className="rounded border border-zinc-600 px-2 py-1.5 text-xs hover:border-zinc-400">
                Save
              </button>
            </form>
          </div>
        </>
      )}
    </div>
  );
}

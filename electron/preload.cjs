const { contextBridge, ipcRenderer } = require("electron");

/**
 * The only thing the page is allowed to reach in the main process.
 *
 * Context isolation is on and node integration is off, so the site runs with no
 * more privilege than it has in a browser tab. This surface is one function
 * that forwards a plain object.
 */
contextBridge.exposeInMainWorld("cpzMusic", {
  updatePresence: (presence) => {
    if (presence === null) return ipcRenderer.send("discord-presence", null);
    if (typeof presence !== "object") return;

    // Rebuild rather than forward, so only these fields ever cross the bridge.
    ipcRenderer.send("discord-presence", {
      title: String(presence.title ?? "").slice(0, 200),
      artist: String(presence.artist ?? "").slice(0, 200),
      album: String(presence.album ?? "").slice(0, 200),
      artwork: String(presence.artwork ?? "").slice(0, 500),
      isPlaying: Boolean(presence.isPlaying),
    });
  },
});

import { app, BrowserWindow, ipcMain, shell } from "electron";
import path from "node:path";
import { DiscordPresence, type Presence } from "./discord";

// Point at your own deployment. No secret lives here; the site handles sign-in.
const SERVER_URL = process.env.CPZ_SERVER_URL ?? "https://music.capzay.uk";
const DISCORD_CLIENT_ID = process.env.CPZ_DISCORD_CLIENT_ID ?? "1493213988664774796";

let mainWindow: BrowserWindow | null = null;
const presence = new DiscordPresence(DISCORD_CLIENT_ID);

ipcMain.on("discord-presence", (_event, data: Presence | null) => {
  presence.update(data);
});

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1200,
    height: 800,
    minWidth: 400,
    minHeight: 600,
    title: "Cpz Music",
    backgroundColor: "#0a0a0a",
    autoHideMenuBar: true,
    show: false,
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  mainWindow.on("ready-to-show", () => mainWindow?.show());

  // Anything not on our own origin opens in the real browser, so a stray link
  // cannot navigate the app window somewhere hostile.
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (url.startsWith(SERVER_URL)) return { action: "allow" };
    void shell.openExternal(url);
    return { action: "deny" };
  });

  mainWindow.webContents.on("will-navigate", (event, url) => {
    if (!url.startsWith(SERVER_URL)) {
      event.preventDefault();
      void shell.openExternal(url);
    }
  });

  void mainWindow.loadURL(SERVER_URL);
  mainWindow.on("closed", () => {
    mainWindow = null;
  });
}

app.whenReady().then(() => {
  createWindow();
  presence.connect();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  presence.destroy();
  app.quit();
});

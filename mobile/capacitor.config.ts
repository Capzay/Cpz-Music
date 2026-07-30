import type { CapacitorConfig } from "@capacitor/cli";

/**
 * The app is a thin shell around the live site rather than a bundled build, so
 * shipping a new web version needs no new APK. Point `server.url` at your own
 * deployment before building.
 */
const config: CapacitorConfig = {
  appId: "uk.capzay.music",
  appName: "Cpz Music",
  webDir: "www",
  server: {
    url: process.env.CPZ_SERVER_URL ?? "https://music.capzay.uk",
    androidScheme: "https",
    // No plaintext HTTP: the session cookie would be readable on any shared
    // network, and Android's own default is to forbid it.
    cleartext: false,
  },
  android: {
    allowMixedContent: false,
    webContentsDebuggingEnabled: false,
  },
};

export default config;

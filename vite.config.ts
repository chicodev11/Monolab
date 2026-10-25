import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { langEditor } from "./scripts/vite-lang-editor";
import { readFileSync } from "node:fs";

const appVersion: string = JSON.parse(
  readFileSync(new URL("./src-tauri/tauri.conf.json", import.meta.url), "utf8"),
).version;

const host = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react(), langEditor()],
  define: { __APP_VERSION__: JSON.stringify(appVersion) },

  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));

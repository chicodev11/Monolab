import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { langEditor } from "./scripts/vite-lang-editor";
import { readFileSync } from "node:fs";

const appVersion: string = JSON.parse(
  readFileSync(new URL("./src-tauri/tauri.conf.json", import.meta.url), "utf8"),
).version;

/** WebView2 always takes woff2, so the woff fallbacks the font packages list would only grow the installer. */
function woff2Only(): Plugin {
  return {
    name: "woff2-only",
    apply: "build",
    enforce: "post",
    generateBundle(_options, bundle) {
      for (const [name, file] of Object.entries(bundle)) {
        if (name.endsWith(".woff")) delete bundle[name];
        else if (file.type === "asset" && name.endsWith(".css") && typeof file.source === "string")
          file.source = file.source.replace(/,\s*url\([^)]*\.woff\)\s*format\(["']?woff["']?\)/g, "");
      }
    },
  };
}

const host = (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react(), langEditor(), woff2Only()],
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

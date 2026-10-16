import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";

const appVersion: string = JSON.parse(
  readFileSync(new URL("./src-tauri/tauri.conf.json", import.meta.url), "utf8"),
).version;

export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify(appVersion) },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
    setupFiles: ["src/test/setup.ts"],
    isolate: true,
  },
});

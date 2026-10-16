import { execSync } from "node:child_process";
import { appRunning, installTestSettings, startApp } from "./app";

export default async function globalSetup() {
  if (appRunning()) {
    throw new Error(
      "Monolab is already running. Close it first (including npm run tauri dev); the tests start their own copy.",
    );
  }
  execSync("npm run build", { stdio: "inherit" });
  installTestSettings();
  await startApp();
}

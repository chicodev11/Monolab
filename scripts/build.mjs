
import { spawnSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import { homedir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
// The updater signing key lives outside the repository (so no tool reading this folder, and no
// `git add -A`, can take it): MONOLAB_UPDATER_KEY, else ~/.monolab/updater.key. The old place in
// src-tauri still works, with a warning, until it is moved.
const places = [process.env.MONOLAB_UPDATER_KEY, join(homedir(), ".monolab", "updater.key"), join(root, "src-tauri/updater.key")].filter(Boolean);
const keyPath = places.find((one) => existsSync(one));

if (!keyPath) {
  console.error(`
Signing key not found. Looked in:
  ${places.join("\n  ")}

Restore it from your backup (to ~/.monolab/updater.key, or set MONOLAB_UPDATER_KEY).
Without it, installed copies can't receive updates.
`);
  process.exit(1);
}
if (keyPath === join(root, "src-tauri/updater.key")) {
  console.warn(`
Warning: the signing key is inside the repository (${keyPath}).
Move it to ${join(homedir(), ".monolab", "updater.key")} so nothing that reads this folder can take it.
`);
}

// Windows code signing: with a certificate in the environment (see scripts/sign-windows.mjs) the exe and
// installer are Authenticode-signed; without one they are built unsigned as before.
const signing = Boolean(process.env.WINDOWS_CERTIFICATE || process.env.WINDOWS_CERTIFICATE_THUMBPRINT);
const extra = signing ? ["--", "--config", "src-tauri/tauri.sign.conf.json"] : [];
if (!signing) console.warn("No code signing certificate set (WINDOWS_CERTIFICATE_THUMBPRINT); building unsigned.");

const result = spawnSync("npm", ["run", "tauri", "build", ...extra], {
  cwd: root,
  stdio: "inherit",
  shell: true,
  env: {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: readFileSync(keyPath, "utf8"),
    // Set this when the key has a password (recommended).
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? "",
  },
});
process.exit(result.status ?? 1);

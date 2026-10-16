
import { spawnSync } from "node:child_process";
import { readFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const keyPath = join(root, "src-tauri/updater.key");

if (!existsSync(keyPath)) {
  console.error(`
Signing key not found:
  ${keyPath}

Restore it from your backup.
Without it, installed copies can't receive updates.
`);
  process.exit(1);
}

const result = spawnSync("npm", ["run", "tauri", "build"], {
  cwd: root,
  stdio: "inherit",
  shell: true,
  env: {
    ...process.env,
    TAURI_SIGNING_PRIVATE_KEY: readFileSync(keyPath, "utf8"),
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD: "",
  },
});
process.exit(result.status ?? 1);

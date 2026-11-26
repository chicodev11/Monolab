// Merges the optional Tauri config pieces for a build into src-tauri/tauri.release.conf.json and prints
// `args=--config …` (for $GITHUB_OUTPUT), or `args=` when no piece is wanted.
//   --forks  bundle the PumpkinMC and Nova forks (tauri.forks.conf.json)
//   --sign   Authenticode-sign the Windows files (tauri.sign.conf.json, needs WINDOWS_CERTIFICATE…)
// One merged file, because one --config is all the Tauri CLI reliably takes.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const tauri = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri");
const pieces = { "--forks": "tauri.forks.conf.json", "--sign": "tauri.sign.conf.json" };

function merge(into, from) {
  for (const [key, value] of Object.entries(from)) {
    if (value && typeof value === "object" && !Array.isArray(value) && into[key] && typeof into[key] === "object") {
      merge(into[key], value);
    } else {
      into[key] = value;
    }
  }
  return into;
}

const wanted = process.argv.slice(2).filter((arg) => arg in pieces);
if (wanted.length === 0) {
  console.log("args=");
} else {
  const config = wanted.reduce((all, arg) => merge(all, JSON.parse(readFileSync(join(tauri, pieces[arg]), "utf8"))), {});
  const out = join(tauri, "tauri.release.conf.json");
  writeFileSync(out, `${JSON.stringify(config, null, 2)}\n`);
  console.log("args=--config src-tauri/tauri.release.conf.json");
}

import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { addonVersion } from "./addon-gradle.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const crate = join(root, "addon", "pumpkin");
const out = join(root, "src-tauri", "resources");

const VERSION = addonVersion();

const { targets } = JSON.parse(readFileSync(join(crate, "targets.json"), "utf8"));
const cargoToml = readFileSync(join(crate, "Cargo.toml"), "utf8");
const source = readFileSync(join(crate, "src", "lib.rs"), "utf8");

const sourceVersion = source.match(/const VERSION: &str = "(\d+)";/)?.[1];
if (sourceVersion !== VERSION) {
  throw new Error(`VERSION in addon/pumpkin/src/lib.rs (${sourceVersion}) differs from ${VERSION}.`);
}

if (targets.length !== 1) {
  throw new Error("targets.json must list exactly one release.");
}
const [target] = targets;
if (!cargoToml.includes(`pumpkin-plugin-api = "${target.api}"`)) {
  throw new Error(
    `pumpkin-plugin-api in addon/pumpkin/Cargo.toml differs from api (${target.api}) in targets.json.`,
  );
}

console.log(`\n=== Pumpkin ${target.id} (API ${target.api}) ===`);
const done = spawnSync(
  "cargo",
  ["build", "--release", "--target", "wasm32-wasip2"],
  { cwd: crate, stdio: "inherit", shell: true },
);
if (done.status !== 0) {
  throw new Error(`cargo exited with ${done.status}. Is the wasm32-wasip2 target installed?`);
}

const built = join(crate, "target", "wasm32-wasip2", "release", "monolab_pumpkin.wasm");
if (!existsSync(built)) {
  throw new Error(`${built} is missing. The crate name may have changed.`);
}
const head = readFileSync(built).subarray(0, 8);
if (head.readUInt32LE(0) !== 0x6d736100 || head.readUInt16LE(6) !== 1) {
  throw new Error("The output is not a WASM component. Check that the target is wasm32-wasip2.");
}

mkdirSync(out, { recursive: true });
const name = `Monolab-pumpkin-${target.id}-${VERSION}.wasm`;
copyFileSync(built, join(out, name));
console.log(`Done: ${name} (${(statSync(built).size / 1024).toFixed(1)} KB)`);

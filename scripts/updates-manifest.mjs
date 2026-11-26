// Rewrites one section of updates.json (the signed list the app reads to update add-ons and the PumpkinMC fork
// without a new app release; design: docs/updates-and-stats.md §1) and publishes it to the `updates` release.
//
// Usage:
//   node scripts/updates-manifest.mjs addon <version> <dir> [--min-app X] [--notes "..."] [--min-fork N] [--dry-run]
//   node scripts/updates-manifest.mjs fork <build> <dir> [--min-app X] [--notes "..."] [--dry-run]
//
// addon: <dir> holds the add-on files of release addon-<version> (Monolab-<n>.jar, Monolab-fabric-<id>-<n>.jar,
//   Monolab-mod-<id>-<n>.jar, Monolab-pumpkin-<id>-<n>.wasm). Every file must end in -<version>.jar/.wasm.
//   --min-fork N sets "minFork": N on the Pumpkin .wasm files (the app skips them on an older fork build).
// fork: <dir> holds pumpkin-<os>-<arch>[.exe] from release fork-<build>; each becomes files["<os>-<arch>"].
//   Other files in <dir> (the licence) are ignored.
// --min-app defaults to "version" in package.json. --notes defaults to "".
// --dry-run: no gh, no signing; starts from {"schema":1} and prints the new updates.json.
//
// Without --dry-run it needs:
//   gh (authenticated: GH_TOKEN), GH_REPO (defaults to chicodev11/Monolab)
//   TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD (the app updater's key)
// and it: downloads updates.json from the `updates` release (or starts from {"schema":1} when there is none),
// replaces the one section, writes updates.json, signs it, creates the `updates` release when it is missing
// (--latest=false, so releases/latest keeps pointing at the app) and uploads both files with --clobber.
//
// What is uploaded, and what the app must expect:
//   updates.json      the JSON below, UTF-8, exactly the bytes that were signed.
//   updates.json.sig  exactly what `tauri signer sign updates.json` writes: ONE LINE of base64 whose decoded text
//                     is a minisign signature file ("untrusted comment: ...\n<sig>\ntrusted comment: ...\n<global
//                     sig>\n"). This is the same format as the "signature" field of the app updater's latest.json.
//   The app verifies it the way the Tauri updater does: base64-decode the .sig content to text and parse it with
//   minisign_verify::Signature::decode; base64-decode tauri.conf.json plugins.updater.pubkey to text and parse it
//   with minisign_verify::PublicKey::decode; then public_key.verify(updates_json_bytes, &signature, true).
//
// Shape (schema 1):
//   { "schema": 1,
//     "addon": { "version": 42, "minApp": "1.0.2", "notes": "", "base": ".../releases/download/addon-42/",
//                "files": { "Monolab-42.jar": { "sha256": "<hex>", "size": 1 },
//                           "Monolab-pumpkin-26.63-42.wasm": { "sha256": "<hex>", "size": 1, "minFork": 3 } } },
//     "fork":  { "build": 3, "minApp": "1.0.2", "notes": "", "base": ".../releases/download/fork-3/",
//                "files": { "windows-x86_64": { "name": "pumpkin-windows-x86_64.exe", "sha256": "<hex>", "size": 1 } } } }
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repo = process.env.GH_REPO || "chicodev11/Monolab";
const RELEASE = "updates";

const fail = (message) => {
  console.error(`updates-manifest: ${message}`);
  process.exit(1);
};

const argv = process.argv.slice(2);
const flags = {};
const positional = [];
for (let i = 0; i < argv.length; i++) {
  const arg = argv[i];
  if (arg === "--dry-run") flags.dryRun = true;
  else if (arg === "--min-app" || arg === "--notes" || arg === "--min-fork") {
    if (i + 1 >= argv.length) fail(`${arg} needs a value.`);
    flags[arg.slice(2)] = argv[++i];
  } else if (arg.startsWith("--")) fail(`unknown option ${arg}.`);
  else positional.push(arg);
}

const [kind, numberText, dirArg] = positional;
if (!["addon", "fork"].includes(kind) || !numberText || !dirArg || positional.length !== 3) {
  fail("usage: updates-manifest.mjs addon|fork <number> <dir> [--min-app X] [--notes \"...\"] [--min-fork N] [--dry-run]");
}
if (!/^\d+$/.test(numberText)) fail(`<number> must be a whole number, got "${numberText}".`);
const number = Number(numberText);
const dir = resolve(dirArg);
if (!existsSync(dir) || !statSync(dir).isDirectory()) fail(`${dir} is not a folder.`);
if (flags["min-fork"] !== undefined) {
  if (kind !== "addon") fail("--min-fork is only for addon.");
  if (!/^\d+$/.test(flags["min-fork"])) fail(`--min-fork must be a whole number, got "${flags["min-fork"]}".`);
}

const minApp = flags["min-app"] ?? JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(minApp)) fail(`--min-app must be a version like 1.0.2, got "${minApp}".`);
const notes = flags.notes ?? "";

const digest = (path) => {
  const bytes = readFileSync(path);
  return { sha256: createHash("sha256").update(bytes).digest("hex"), size: bytes.length };
};

const names = readdirSync(dir).filter((name) => statSync(join(dir, name)).isFile()).sort();
const files = {};
let section;

if (kind === "addon") {
  const addonName = /^Monolab-(?:(?:fabric|mod|pumpkin)-.+-)?(\d+)\.(jar|wasm)$/;
  for (const name of names) {
    const found = name.match(addonName);
    if (!found) continue;
    if (Number(found[1]) !== number) fail(`${name} is not add-on ${number}.`);
    const entry = digest(join(dir, name));
    if (flags["min-fork"] !== undefined && name.startsWith("Monolab-pumpkin-") && name.endsWith(".wasm")) {
      entry.minFork = Number(flags["min-fork"]);
    }
    files[name] = entry;
  }
  if (!Object.keys(files).length) fail(`no add-on files (Monolab-*.jar / .wasm) in ${dir}.`);
  section = {
    version: number,
    minApp,
    notes,
    base: `https://github.com/${repo}/releases/download/addon-${number}/`,
    files,
  };
} else {
  const forkName = /^pumpkin-(windows|linux|macos)-(x86_64|aarch64)(\.exe)?$/;
  for (const name of names) {
    const found = name.match(forkName);
    if (!found) continue;
    if ((found[1] === "windows") !== Boolean(found[3])) fail(`${name}: only the Windows build ends in .exe.`);
    files[`${found[1]}-${found[2]}`] = { name, ...digest(join(dir, name)) };
  }
  if (!Object.keys(files).length) fail(`no fork files (pumpkin-<os>-<arch>[.exe]) in ${dir}.`);
  section = {
    build: number,
    minApp,
    notes,
    base: `https://github.com/${repo}/releases/download/fork-${number}/`,
    files,
  };
}

const run = (command, args, options = {}) =>
  execFileSync(command, args, { stdio: ["ignore", "pipe", "inherit"], encoding: "utf8", ...options });

let current = { schema: 1 };
const work = flags.dryRun ? null : mkdtempSync(join(tmpdir(), "monolab-updates-"));

if (work) {
  try {
    run("gh", ["release", "download", RELEASE, "--repo", repo, "--pattern", "updates.json", "--dir", work, "--clobber"]);
    current = JSON.parse(readFileSync(join(work, "updates.json"), "utf8"));
    console.log(`Current list: addon ${current.addon?.version ?? "-"}, fork ${current.fork?.build ?? "-"}`);
  } catch {
    console.log("No updates.json in the updates release yet; starting a new list.");
    current = { schema: 1 };
  }
  if (current.schema !== 1) fail(`the current updates.json has schema ${current.schema}; this script writes schema 1.`);
  const previous = kind === "addon" ? current.addon?.version : current.fork?.build;
  if (typeof previous === "number" && number < previous) {
    fail(`${kind} ${number} is older than the listed ${previous}. Numbers only go up.`);
  }
}

const next = { ...current, schema: 1, [kind]: section };
const text = `${JSON.stringify(next, null, 2)}\n`;

if (flags.dryRun) {
  process.stdout.write(text);
  process.exit(0);
}

if (!process.env.TAURI_SIGNING_PRIVATE_KEY) fail("TAURI_SIGNING_PRIVATE_KEY is not set.");
const listPath = join(work, "updates.json");
const sigPath = `${listPath}.sig`;
rmSync(sigPath, { force: true });
writeFileSync(listPath, text);
// tauri signer reads the key and password from TAURI_SIGNING_PRIVATE_KEY / _PASSWORD and writes updates.json.sig.
execFileSync("npx", ["--no-install", "tauri", "signer", "sign", listPath], {
  cwd: root,
  stdio: "inherit",
  shell: process.platform === "win32",
});
if (!existsSync(sigPath)) fail(`tauri signer did not write ${sigPath}.`);

try {
  run("gh", ["release", "view", RELEASE, "--repo", repo, "--json", "tagName"]);
} catch {
  run("gh", [
    "release", "create", RELEASE, "--repo", repo, "--latest=false",
    "--title", "Update list",
    "--notes", "Holds updates.json (and its signature), which the app reads to update add-ons and the PumpkinMC fork.",
  ]);
}
run("gh", ["release", "upload", RELEASE, listPath, sigPath, "--repo", repo, "--clobber"], { stdio: "inherit" });
rmSync(work, { recursive: true, force: true });
console.log(`updates.json now lists ${kind} ${number} (${Object.keys(files).length} files).`);

// Builds the bundled Rust forks and lays them out in src-tauri/forks/, which
// `tauri build --config src-tauri/tauri.forks.conf.json` ships as resources:
//   forks/pumpkin(.exe)                 PumpkinMC fork (server), GPL-3.0
//   forks/nova/horizon-client(.exe)     Nova, the Horizon fork (client), MIT
//   forks/nova/horizon-prepare(.exe)    Nova's resource preparer
//   forks/nova/research-lock.json       the pins the preparer checks against
//   forks/nova/mods/horizon/src/test/java  exporter sources, compiled on the player's JDK
//   forks/fork.json                     {"build": N}, the PumpkinMC fork build number (docs/updates-and-stats.md §1);
//                                       the app compares it with fork releases (fork-<build>); 0 = the oldest
// Usage: node scripts/forks.mjs [--pumpkin DIR] [--nova DIR] [--skip-build] [--quick] [--build N]
// --build N (or env MONOLAB_FORK_BUILD) sets the number in fork.json; without either it is 0.
// --quick builds the PumpkinMC fork with its local `quick` profile (.cargo/config.toml in the fork, incremental) for
// play tests; releases use --release.
// Defaults: ../PumpkinMC (or ../../.pumpkinmc/PumpkinMC) and ../Horizon beside this checkout (either name case).
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const option = (name) => {
  const at = args.indexOf(name);
  return at >= 0 ? args[at + 1] : undefined;
};
const near = (...names) => names.map((one) => resolve(root, "..", one)).find(existsSync);
const pumpkin = option("--pumpkin") ?? near("PumpkinMC", "pumpkinmc", join("..", ".pumpkinmc", "PumpkinMC"));
const nova = option("--nova") ?? near("Horizon", "horizon");
const build = !args.includes("--skip-build");
const quick = args.includes("--quick");
const exe = process.platform === "win32" ? ".exe" : "";
const forkBuildText = option("--build") ?? process.env.MONOLAB_FORK_BUILD ?? "0";
if (!/^\d+$/.test(forkBuildText.trim())) {
  console.error(`The fork build number must be a whole number, got "${forkBuildText}".`);
  process.exit(1);
}
const forkBuild = Number(forkBuildText.trim());
const out = join(root, "src-tauri", "forks");

if (!pumpkin && !nova) {
  console.error("Neither fork checkout was found. Pass --pumpkin DIR and/or --nova DIR.");
  process.exit(1);
}
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const cargo = (cwd, cargoArgs) => execFileSync("cargo", cargoArgs, { cwd, stdio: "inherit" });

writeFileSync(join(out, "fork.json"), `${JSON.stringify({ build: forkBuild })}\n`);

if (pumpkin) {
  if (build) cargo(pumpkin, ["build", ...(quick ? ["--profile", "quick"] : ["--release"]), "--locked", "-p", "pumpkin"]);
  cpSync(join(pumpkin, "target", quick ? "quick" : "release", `pumpkin${exe}`), join(out, `pumpkin${exe}`));
  cpSync(join(pumpkin, "LICENSE"), join(out, "PumpkinMC-LICENSE.txt"));
  console.log(`PumpkinMC fork from ${pumpkin} (build ${forkBuild})`);
}

if (nova) {
  if (build) {
    cargo(nova, ["build", "--release", "--locked", "-p", "horizon-prepare"]);
    cargo(nova, ["build", "--release", "--locked", "-p", "horizon-client", "--no-default-features", "--features", "audio,controllers"]);
  }
  const dir = join(out, "nova");
  mkdirSync(dir, { recursive: true });
  for (const program of ["horizon-client", "horizon-prepare"]) {
    cpSync(join(nova, "target", "release", `${program}${exe}`), join(dir, `${program}${exe}`));
  }
  cpSync(join(nova, "research-lock.json"), join(dir, "research-lock.json"));
  cpSync(join(nova, "mods", "horizon", "src", "test", "java"), join(dir, "mods", "horizon", "src", "test", "java"), { recursive: true });
  // Horizon's MIT licence and its third-party notices go with every distributed build (NOVA.md).
  cpSync(join(nova, "LICENSE"), join(dir, "LICENSE.txt"));
  cpSync(join(nova, "THIRD_PARTY_NOTICES.md"), join(dir, "THIRD_PARTY_NOTICES.md"));
  console.log(`Nova from ${nova}`);
}
console.log(`Forks laid out in ${out}`);

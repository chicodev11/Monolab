import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const check = process.argv.includes("--check");
const stored = join(root, "src", "lib", "bindings.ts");
const exe = join(root, "src-tauri", "target", "debug", "monolab.exe");

execFileSync("cargo", ["build", "--manifest-path", join(root, "src-tauri", "Cargo.toml")], { stdio: "inherit" });
if (!existsSync(exe)) throw new Error(`${exe} not found.`);

const dir = mkdtempSync(join(tmpdir(), "se-bindings-"));
const fresh = join(dir, "bindings.ts");
try {
  execFileSync(exe, ["--export-bindings", fresh], { stdio: "inherit" });
  const text = readFileSync(fresh, "utf8").replace(/\r\n/g, "\n");
  const old = existsSync(stored) ? readFileSync(stored, "utf8").replace(/\r\n/g, "\n") : "";
  if (text === old) {
    console.log("bindings.ts is up to date.");
  } else if (check) {
    console.error("src/lib/bindings.ts is out of date. Run `npm run bindings` and commit it.");
    process.exit(1);
  } else {
    writeFileSync(stored, text, "utf8");
    console.log("Regenerated src/lib/bindings.ts.");
  }
} finally {
  rmSync(dir, { recursive: true, force: true });
}

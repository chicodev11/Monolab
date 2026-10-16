import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "src-tauri", "resources", "THIRD-PARTY-NOTICES.txt");

const FONTS = [
  ["Inter", "@fontsource-variable/inter"],
  ["JetBrains Mono", "@fontsource/jetbrains-mono"],
  ["Noto Sans KR", "@fontsource/noto-sans-kr"],
];

function run(command, args, where) {
  const done = spawnSync(command, args, {
    cwd: where ?? root,
    encoding: "utf8",
    shell: true,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (done.status !== 0) {
    throw new Error(`${command} exited with ${done.status}.\n${done.stderr}`);
  }
  return done.stdout;
}

function crates() {
  const data = JSON.parse(
    run("cargo", ["metadata", "--format-version", "1"], join(root, "src-tauri")),
  );
  return data.packages
    .filter((one) => one.name !== "monolab")
    .map((one) => ({
      name: one.name,
      version: one.version,
      license: one.license ?? "(not stated)",
    }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

function packages() {
  const data = JSON.parse(run("npm", ["ls", "--all", "--omit=dev", "--json"]));
  const found = new Map();
  const walk = (node) => {
    for (const [name, one] of Object.entries(node.dependencies ?? {})) {
      if (!found.has(name)) {
        found.set(name, one.version ?? "");
        walk(one);
      }
    }
  };
  walk(data);
  return [...found.entries()]
    .map(([name, version]) => ({ name, version, license: licenseOf(name) }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

function licenseOf(name) {
  const meta = join(root, "node_modules", name, "package.json");
  if (!existsSync(meta)) {
    return "(not found)";
  }
  const parsed = JSON.parse(readFileSync(meta, "utf8"));
  if (typeof parsed.license === "string") {
    return parsed.license;
  }
  return parsed.license?.type ?? "(not stated)";
}

function crateLicenseText(wanted) {
  const registry = join(homedir(), ".cargo", "registry", "src");
  if (!existsSync(registry)) {
    return null;
  }
  for (const index of readdirSync(registry)) {
    const folder = join(registry, index);
    for (const one of readdirSync(folder)) {
      if (!one.startsWith(`${wanted.crate}-`)) {
        continue;
      }
      const here = join(folder, one);
      for (const file of readdirSync(here)) {
        if (file.toUpperCase() === wanted.file) {
          return readFileSync(join(here, file), "utf8").trim();
        }
      }
    }
  }
  return null;
}

function block(title) {
  const bar = "─".repeat(72);
  return `\n${bar}\n${title}\n${bar}\n`;
}

const text = [];
text.push("Monolab — Third-party notices");
text.push("");
text.push("This program includes the components below. Their copyright notices and");
text.push("license texts are reproduced here as each license requires.");
text.push("");
text.push(`Generated: ${new Date().toISOString().slice(0, 10)} by \`npm run notices\`. Do not edit by hand.`);

text.push(block("Fonts (SIL Open Font License 1.1)"));
text.push("These fonts are bundled with the program. The OFL requires its text to be");
text.push("included when the fonts are redistributed.");
for (const [label, pkg] of FONTS) {
  const license = join(root, "node_modules", pkg, "LICENSE");
  text.push(block(`${label} — ${pkg}`));
  text.push(existsSync(license) ? readFileSync(license, "utf8").trim() : "(LICENSE file not found)");
}

const mit = crateLicenseText({ crate: "serde", file: "LICENSE-MIT" });
const apache = crateLicenseText({ crate: "serde", file: "LICENSE-APACHE" });
text.push(block("MIT License"));
text.push(mit ?? "(Not found in the cargo cache. Run `cargo fetch` and regenerate.)");
text.push(block("Apache License 2.0"));
text.push(apache ?? "(Not found in the cargo cache. Run `cargo fetch` and regenerate.)");

const rust = crates();
text.push(block(`Rust crates (${rust.length})`));
for (const one of rust) {
  text.push(`${one.name} ${one.version} — ${one.license}`);
}

const web = packages();
text.push(block(`npm packages (${web.length})`));
for (const one of web) {
  text.push(`${one.name} ${one.version} — ${one.license}`);
}

text.push(block("Minecraft"));
text.push("NOT AN OFFICIAL MINECRAFT PRODUCT.");
text.push("NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT.");
text.push("");
text.push("Server files are downloaded from the official sources (Mojang, PaperMC,");
text.push("NeoForged, FabricMC) when needed. No Minecraft files ship with this program.");
text.push("Translated names are fetched from Mojang's asset servers and stay on this");
text.push("computer; they belong to Mojang Studios.");
text.push("");

mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, text.join("\n"), "utf8");
console.log(`Done: ${out}`);
console.log(`${rust.length} crates · ${web.length} npm packages · ${FONTS.length} fonts`);

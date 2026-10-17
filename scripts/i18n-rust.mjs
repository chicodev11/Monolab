import fs from "node:fs";
import path from "node:path";
import { flat, languages, removeKeys, rustFile, save } from "./lang.mjs";

const root = path.resolve(import.meta.dirname, "..");
const files = [];
function walk(dir) {
  for (const one of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, one.name);
    if (one.isDirectory()) walk(full);
    else if (one.name.endsWith(".rs")) files.push(full);
  }
}
walk(path.join(root, "src-tauri/src"));

const literal = /r#*"((?:[^"\\]|\\.)*)"#*|"((?:[^"\\]|\\.)*)"/g;
const found = new Set();
for (const file of files) {
  const src = fs.readFileSync(file, "utf8");
  let skipping = false;
  let armed = false;
  for (const line of src.split(/\r?\n/)) {
    if (skipping) {
      if (line === "}") skipping = false;
      continue;
    }
    if (line.trim() === "#[cfg(test)]") {
      armed = true;
      continue;
    }
    if (armed) {
      armed = false;
      if (/^\s*(pub\s+)?mod\s+\w+\s*\{/.test(line)) {
        skipping = true;
        continue;
      }
    }
    const code = line.replace(/\/\/.*$/, "");
    for (const m of code.matchAll(literal)) {
      const text = (m[1] ?? m[2]).replace(/\\"/g, '"').replace(/\\n/g, "\n").replace(/\\\\/g, "\\");
      found.add(text);
      if (text.startsWith('{"')) {
        try {
          for (const value of Object.values(JSON.parse(text))) if (typeof value === "string") found.add(value);
        } catch {}
      }
    }
  }
}

let stale = 0;
for (const lang of languages()) {
  const one = rustFile(lang);
  if (!one.doc) continue;
  const table = flat(one.doc);
  const gone = Object.keys(table).filter((key) => !found.has(key));
  stale += gone.length;
  if (process.argv.includes("--prune")) {
    if (removeKeys(one.doc, gone)) save(one);
    console.log(`${one.name}: removed ${gone.length}`);
  } else {
    console.log(`${one.name}: ${Object.keys(table).length} strings · ${gone.length} no longer in the Rust source`);
    if (process.argv.includes("--list")) for (const key of gone) console.log(`  ${JSON.stringify(key)}`);
  }
}
process.exitCode = stale && !process.argv.includes("--prune") ? 1 : 0;

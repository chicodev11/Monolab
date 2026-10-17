import fs from "node:fs";
import path from "node:path";

export const langDir = path.resolve(import.meta.dirname, "..", "src", "lang");
export const NEW_SECTION = "new";

export function languages() {
  return fs
    .readdirSync(langDir)
    .filter((name) => /^[a-z]+(-[A-Za-z]+)?\.json$/.test(name))
    .map((name) => name.replace(/\.json$/, ""))
    .sort((a, b) => (a === "en" ? -1 : b === "en" ? 1 : a.localeCompare(b)));
}

function open(name) {
  const file = path.join(langDir, name);
  return { file, name, doc: fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, "utf8")) : null };
}

export const uiFile = (lang) => open(`${lang}.json`);
export const rustFile = (lang) => open(`rust.${lang}.json`);

export function flat(doc) {
  const out = {};
  for (const [key, value] of Object.entries(doc ?? {})) {
    if (typeof value === "string") out[key] = value;
    else Object.assign(out, value);
  }
  return out;
}

export function removeKeys(doc, keys) {
  let removed = 0;
  for (const value of Object.values(doc)) {
    if (typeof value !== "object") continue;
    for (const key of keys) if (key in value) { delete value[key]; removed++; }
  }
  for (const key of keys) if (typeof doc[key] === "string" && !key.startsWith("$")) { delete doc[key]; removed++; }
  return removed;
}

export function addKeys(doc, entries) {
  doc[NEW_SECTION] = { ...(doc[NEW_SECTION] ?? {}), ...entries };
}

export function save(one) {
  fs.writeFileSync(one.file, JSON.stringify(one.doc, null, 2) + "\n");
}

// Gets a translation file ready to work on: `npm run i18n:todo -- ko`.
// - Sentences the app uses that the file doesn't have yet go to its "todo" section with an empty translation.
//   Fill them in (by hand or with an AI); empty ones show in English meanwhile.
// - Sentences the app no longer uses move to an "unused" section (delete it whenever you like).
// - A new language file starts with "ready": false, which keeps it out of the Language setting until it's done.
import { codeStrings, flat, save, uiFile, langDir } from "./lang.mjs";
import fs from "node:fs";
import path from "node:path";

const lang = process.argv.slice(2).find((one) => !one.startsWith("-"));
if (!lang || !/^[a-z]+(-[A-Za-z]+)?$/.test(lang) || lang === "en") {
  console.error("Say which language, like: npm run i18n:todo -- ko");
  process.exit(2);
}
const one = uiFile(lang);
if (!one.doc) one.doc = { $language: lang, ready: false };
const doc = one.doc;
const { keys, literals } = codeStrings();
const isId = (key) => /^[a-z][A-Za-z]*(\.[A-Za-z0-9]+)+$/.test(key);
const table = flat(doc);

const todo = typeof doc.todo === "object" && doc.todo ? doc.todo : {};
let added = 0;
for (const key of [...keys].sort()) {
  if (key in table || key in todo) continue;
  todo[key] = "";
  added += 1;
}

const unused = typeof doc.unused === "object" && doc.unused ? doc.unused : {};
let moved = 0;
for (const [section, value] of Object.entries(doc)) {
  if (section === "unused" || section.startsWith("$") || typeof value !== "object" || !value) continue;
  for (const key of Object.keys(value)) {
    if (keys.has(key) || literals.has(key) || isId(key)) continue;
    unused[key] = value[key];
    delete value[key];
    moved += 1;
  }
}

delete doc.todo;
delete doc.unused;
if (Object.keys(todo).length) doc.todo = todo;
if (Object.keys(unused).length) doc.unused = unused;
for (const [section, value] of Object.entries(doc)) if (typeof value === "object" && value && !Object.keys(value).length) delete doc[section];
save(one);

const empty = Object.values(doc.todo ?? {}).filter((text) => !text).length;
const done = Object.entries(flat(doc)).filter(([key, text]) => keys.has(key) && text).length;
console.log(`${one.name}: ${done} of ${keys.size} sentences translated · ${added} added to "todo" · ${empty} still empty in "todo" · ${moved} moved to "unused"`);
const rust = path.join(langDir, `rust.${lang}.json`);
if (fs.existsSync(rust)) console.log(`rust.${lang}.json: messages from the app's Rust side (errors, notices); kept by hand.`);
if (doc.ready === false) console.log(`"ready": false — set it to true (or delete it) when the translation is done; the Language setting then shows it.`);

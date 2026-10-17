import ts from "typescript";
import fs from "node:fs";
import path from "node:path";
import { NEW_SECTION, addKeys, flat, languages as langList, save, uiFile } from "./lang.mjs";

const root = path.resolve(import.meta.dirname, "..");
const sync = process.argv.includes("--sync");
const files = [];
function walk(dir) {
  for (const one of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, one.name);
    if (one.isDirectory()) walk(full);
    else if (
      /\.(tsx?|mts)$/.test(one.name) &&
      !/\.test\.tsx?$/.test(one.name)
    )
      files.push(full);
  }
}
walk(path.join(root, "src"));

const keys = new Set();
for (const file of files) {
  const src = fs.readFileSync(file, "utf8");
  const sf = ts.createSourceFile(file, src, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const visit = (node) => {
    if (
      ts.isCallExpression(node) &&
      ts.isIdentifier(node.expression) &&
      (node.expression.text === "tr" || node.expression.text === "t") &&
      node.arguments[0] &&
      ts.isStringLiteral(node.arguments[0])
    ) {
      keys.add(node.arguments[0].text);
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
}

const languages = langList();

const isId = (key) => /^[a-z][A-Za-z]*(\.[A-Za-z0-9]+)+$/.test(key);
const report = {};
for (const lang of languages) {
  const one = uiFile(lang);
  const table = flat(one.doc);
  const missing = [...keys].filter((key) => !(key in table) && (lang !== "en" || isId(key))).sort();
  if (sync && lang !== "en" && missing.length) {
    addKeys(one.doc, Object.fromEntries(missing.map((key) => [key, key])));
    save(one);
    console.log(`Added ${missing.length} keys to the "${NEW_SECTION}" section of ${lang}.json (text = key). Move them to the right section.`);
    report[lang] = [];
    continue;
  }
  report[lang] = missing;
}

if (process.argv.includes("--dump")) {
  process.stdout.write(JSON.stringify(report, null, 1) + "\n");
} else {
  console.log(`${keys.size} tr/t keys · languages ${languages.join(", ")}`);
  for (const lang of languages) {
    const missing = report[lang];
    console.log(`  ${lang}.json — missing ${missing.length}`);
    for (const key of missing.slice(0, 10)) console.log("    -", key);
    if (missing.length > 10) console.log(`    … and ${missing.length - 10} more`);
  }
}
process.exitCode = Object.values(report).some((missing) => missing.length) ? 1 : 0;

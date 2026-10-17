import ts from "typescript";
import fs from "node:fs";
import path from "node:path";
import { flat, languages as langList, removeKeys, save, uiFile } from "./lang.mjs";

const root = path.resolve(import.meta.dirname, "..");
const prune = process.argv.includes("--prune");
const files = [];
function walk(dir) {
  for (const one of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, one.name);
    if (one.isDirectory()) walk(full);
    else if (/\.(tsx?|mts)$/.test(one.name) && !/\.test\.tsx?$/.test(one.name))
      files.push(full);
  }
}
walk(path.join(root, "src"));

const literals = new Set();
for (const file of files) {
  const src = fs.readFileSync(file, "utf8");
  const sf = ts.createSourceFile(file, src, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const visit = (node) => {
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) literals.add(node.text);
    else if (ts.isJsxText(node)) literals.add(node.text.trim());
    else if (ts.isTemplateExpression(node)) {
      literals.add(node.head.text);
      for (const span of node.templateSpans) literals.add(span.literal.text);
    }
    ts.forEachChild(node, visit);
  };
  visit(sf);
}

const languages = langList();
const unused = new Set();
for (const lang of languages) {
  for (const key of Object.keys(flat(uiFile(lang).doc))) {
    if (key.startsWith("$")) continue;
    if (!literals.has(key)) unused.add(key);
  }
}
const list = [...unused].sort();
console.log(`${list.length} unused keys`);
for (const key of list) console.log("  -", key);
if (prune && list.length) {
  for (const lang of languages) {
    const one = uiFile(lang);
    const removed = removeKeys(one.doc, list);
    if (removed) {
      save(one);
      console.log(`Removed ${removed} from ${one.name}`);
    }
  }
}

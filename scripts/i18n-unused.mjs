import ts from "typescript";
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(import.meta.dirname, "..");
const langDir = path.join(root, "src", "lang");
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

const languages = fs
  .readdirSync(langDir)
  .filter((name) => /^[a-z]+(-[A-Za-z]+)?\.json$/.test(name))
  .map((name) => name.replace(/\.json$/, ""));
const unused = new Set();
for (const lang of languages) {
  const table = JSON.parse(fs.readFileSync(path.join(langDir, `${lang}.json`), "utf8"));
  for (const key of Object.keys(table)) {
    if (key.startsWith("$")) continue;
    if (!literals.has(key)) unused.add(key);
  }
}
const list = [...unused].sort();
console.log(`${list.length} unused keys`);
for (const key of list) console.log("  -", key);
if (prune && list.length) {
  for (const lang of languages) {
    const file = path.join(langDir, `${lang}.json`);
    const table = JSON.parse(fs.readFileSync(file, "utf8"));
    let removed = 0;
    for (const key of list) if (key in table) { delete table[key]; removed++; }
    fs.writeFileSync(file, JSON.stringify(table, null, 2) + "\n");
    console.log(`Removed ${removed} from ${lang}.json`);
  }
}

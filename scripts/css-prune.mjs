import fs from "node:fs";
import path from "node:path";
import postcss from "postcss";

const root = path.resolve(import.meta.dirname, "..");
const prune = process.argv.includes("--prune");
const cssFiles = ["src/App.css", "src/components/primitives/primitives.css"].map((f) => path.join(root, f));

const walk = (dir) =>
  fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? walk(path.join(dir, e.name)) : /\.(tsx?|mts|html)$/.test(e.name) ? [path.join(dir, e.name)] : [],
  );
const stripComments = (text) => text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:"'`])\/\/[^\n]*/g, "$1");
const source = [...walk(path.join(root, "src")), path.join(root, "index.html")]
  .map((f) => stripComments(fs.readFileSync(f, "utf8")))
  .join("\n");

const roots = cssFiles.map((f) => ({ file: f, ast: postcss.parse(fs.readFileSync(f, "utf8"), { from: f }) }));
const allCss = roots.map((r) => r.ast.toString()).join("\n");
const classes = new Set([...allCss.matchAll(/\.(-?[_a-zA-Z][_a-zA-Z0-9-]*)/g)].map((m) => m[1]));
const LIBRARY = /^(cm-|xterm)/;
const used = (name) => {
  if (LIBRARY.test(name) || source.includes(name)) return true;
  for (let i = name.length - 1; i > 0; i--) {
    if (name[i] !== "-") continue;
    const prefix = name.slice(0, i + 1);
    if (source.includes(prefix + "${") || source.includes(`"${prefix}" +`) || source.includes(`'${prefix}' +`)) return true;
  }
  return false;
};
const dead = new Set([...classes].filter((c) => !used(c)));
const hasDead = (selector) => [...selector.matchAll(/\.(-?[_a-zA-Z][_a-zA-Z0-9-]*)/g)].some((m) => dead.has(m[1]));

let removedRules = 0;
let trimmedSelectors = 0;
let removedKeyframes = 0;
const report = [];
for (const { file, ast } of roots) {
  ast.walkRules((rule) => {
    if (rule.parent?.type === "atrule" && /keyframes/.test(rule.parent.name)) return;
    const selectors = rule.selectors;
    const alive = selectors.filter((s) => !hasDead(s));
    if (alive.length === selectors.length) return;
    if (alive.length === 0) {
      report.push(`${path.basename(file)}:${rule.source.start.line} ${rule.selector}`);
      removedRules++;
      if (prune) rule.remove();
    } else {
      trimmedSelectors += selectors.length - alive.length;
      report.push(`${path.basename(file)}:${rule.source.start.line} partly → ${selectors.filter((s) => hasDead(s)).join(", ")}`);
      if (prune) rule.selectors = alive;
    }
  });
  if (prune) {
    ast.walkAtRules((at) => {
      if (/media|supports|layer/.test(at.name) && at.nodes && at.nodes.length === 0) at.remove();
    });
  }
}
const remaining = roots.map((r) => r.ast.toString()).join("\n");
for (const { file, ast } of roots) {
  ast.walkAtRules(/keyframes/, (at) => {
    const name = at.params.trim();
    const refs = remaining.replace(/@(-webkit-)?keyframes\s+[^{]+\{/g, "").split(new RegExp(`\\b${name}\\b`)).length - 1;
    if (refs > 0 || source.includes(name)) return;
    report.push(`${path.basename(file)}:${at.source.start.line} @keyframes ${name}`);
    removedKeyframes++;
    if (prune) at.remove();
  });
}
for (const line of report) console.log("  -", line);
console.log(`Dead classes ${dead.size} · rules ${removedRules} · selectors ${trimmedSelectors} · keyframes ${removedKeyframes}${prune ? " removed" : " (candidates)"}`);
if (prune) {
  for (const { file, ast } of roots) fs.writeFileSync(file, ast.toString());
  for (const file of cssFiles) fs.writeFileSync(file, fs.readFileSync(file, "utf8").replace(/\n{4,}/g, "\n\n\n"));
}

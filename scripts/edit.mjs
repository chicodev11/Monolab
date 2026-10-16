import { readFileSync, writeFileSync } from "node:fs";

export function edit(file, pairs) {
  const raw = readFileSync(file, "utf8");
  const had = raw.includes("\r\n");
  let text = raw.split("\r\n").join("\n");
  for (const [from, to] of pairs) {
    const at = text.indexOf(from);
    if (at < 0) throw new Error("Not found: " + from.slice(0, 70));
    if (text.indexOf(from, at + 1) >= 0) throw new Error("Matched more than once: " + from.slice(0, 70));
    text = text.slice(0, at) + to + text.slice(at + from.length);
  }
  writeFileSync(file, had ? text.split("\n").join("\r\n") : text, "utf8");
}

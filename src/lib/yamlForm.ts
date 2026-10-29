/** Reads plugin config.yml files into editable fields and writes single values back, keeping comments and order. */

export type YamlKind = "bool" | "number" | "text" | "list" | "block";

export type YamlField = {
  /** Dotted path, e.g. "economy.start". */
  path: string;
  key: string;
  /** Top-level section this field sits in ("" for top-level values). */
  section: string;
  value: string;
  kind: YamlKind;
  /** Line index in the file. */
  line: number;
  /** Comment lines right above the key. */
  hint: string;
  /** Item count for lists. */
  items: number;
  /** The value was quoted in the file. */
  quoted: "'" | '"' | "";
};

const NUMBER = /^-?\d+(\.\d+)?$/;

function stripComment(text: string) {
  let quote = "";
  for (let at = 0; at < text.length; at += 1) {
    const c = text[at];
    if (quote) {
      if (c === quote) quote = "";
    } else if (c === "'" || c === '"') {
      quote = c;
    } else if (c === "#" && (at === 0 || text[at - 1] === " ")) {
      return text.slice(0, at).trimEnd();
    }
  }
  return text.trimEnd();
}

function splitKey(body: string): [string, string] | null {
  const quoted = body.match(/^(['"])(.*?)\1\s*:(?:\s+(.*)|$)/);
  if (quoted) return [quoted[2], quoted[3] ?? ""];
  const plain = body.match(/^([^:#'"\s][^:#]*?)\s*:(?:\s+(.*)|$)/);
  return plain ? [plain[1].trim(), plain[2] ?? ""] : null;
}

export function readYamlFields(text: string): YamlField[] {
  const lines = text.split(/\r?\n/);
  const fields: YamlField[] = [];
  const stack: { indent: number; key: string }[] = [];
  let comments: string[] = [];
  for (let at = 0; at < lines.length; at += 1) {
    const raw = lines[at];
    const trimmed = raw.trim();
    if (!trimmed) {
      comments = [];
      continue;
    }
    if (trimmed.startsWith("#")) {
      comments.push(trimmed.replace(/^#+\s?/, ""));
      continue;
    }
    if (trimmed.startsWith("-")) continue;
    const indent = raw.length - raw.trimStart().length;
    const split = splitKey(trimmed);
    if (!split) {
      comments = [];
      continue;
    }
    const [key, rest] = split;
    while (stack.length && stack[stack.length - 1].indent >= indent) stack.pop();
    const parents = stack.map((one) => one.key);
    const path = [...parents, key].join(".");
    const hint = comments.join(" ").trim();
    comments = [];
    const value = stripComment(rest).trim();
    const base = { path, key, section: parents[0] ?? "", line: at, hint, items: 0, quoted: "" as const };
    if (!value) {
      let next = at + 1;
      while (next < lines.length && (!lines[next].trim() || lines[next].trim().startsWith("#"))) next += 1;
      const nextLine = lines[next] ?? "";
      const nextIndent = nextLine.length - nextLine.trimStart().length;
      if (nextLine.trim().startsWith("-") && nextIndent >= indent) {
        let count = 0;
        for (let scan = next; scan < lines.length; scan += 1) {
          const one = lines[scan];
          if (!one.trim() || one.trim().startsWith("#")) continue;
          const own = one.length - one.trimStart().length;
          if (own < nextIndent || (own === nextIndent && !one.trim().startsWith("-"))) break;
          if (own === nextIndent) count += 1;
        }
        fields.push({ ...base, value: "", kind: "list", items: count });
      } else {
        stack.push({ indent, key });
      }
      continue;
    }
    if (value === "|" || value === ">" || /^[|>][+-]?$/.test(value)) {
      fields.push({ ...base, value: "", kind: "block" });
      continue;
    }
    if (value.startsWith("[")) {
      const inner = value.replace(/^\[|\]$/g, "").trim();
      fields.push({ ...base, value: "", kind: "list", items: inner ? inner.split(",").length : 0 });
      continue;
    }
    if (value.startsWith("{")) continue;
    const quote = value[0] === "'" || value[0] === '"' ? (value[0] as "'" | '"') : "";
    const plain = quote && value.endsWith(quote) ? value.slice(1, -1).replace(quote === "'" ? /''/g : /\\"/g, quote) : value;
    const kind: YamlKind = quote ? "text" : plain === "true" || plain === "false" ? "bool" : NUMBER.test(plain) ? "number" : "text";
    fields.push({ ...base, value: plain, kind, quoted: quote });
  }
  return fields;
}

function literal(field: YamlField, value: string) {
  if (field.kind === "bool" || field.kind === "number") return value;
  const needsQuote = field.quoted || !value || /^[\s'"&*!|>%@`{[\]-]|[:#]\s|\s$|^(true|false|null|yes|no|on|off|~)$/i.test(value) || NUMBER.test(value);
  if (!needsQuote) return value;
  if (field.quoted === '"') return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
  return `'${value.replace(/'/g, "''")}'`;
}

/** Puts a new value on the field's line; the key, indentation and any trailing comment stay as they were. */
export function setYamlField(text: string, field: YamlField, value: string): string {
  const newline = text.includes("\r\n") ? "\r\n" : "\n";
  const lines = text.split(/\r?\n/);
  const raw = lines[field.line];
  if (raw === undefined) return text;
  const colon = raw.indexOf(":", raw.indexOf(field.key));
  if (colon < 0) return text;
  const after = raw.slice(colon + 1);
  const comment = (() => {
    const bare = stripComment(after);
    return after.slice(bare.length).match(/\s+#.*$/)?.[0] ?? "";
  })();
  lines[field.line] = `${raw.slice(0, colon + 1)} ${literal(field, value)}${comment}`;
  return lines.join(newline);
}

/** TOML configs (most mod configs): `[section]` headers and `key = value` lines, read the same way as YAML fields. */
export function readTomlFields(text: string): YamlField[] {
  const lines = text.split(/\r?\n/);
  const fields: YamlField[] = [];
  let section = "";
  let comments: string[] = [];
  for (let at = 0; at < lines.length; at += 1) {
    const trimmed = lines[at].trim();
    if (!trimmed) {
      comments = [];
      continue;
    }
    if (trimmed.startsWith("#")) {
      comments.push(trimmed.replace(/^#+\s?/, ""));
      continue;
    }
    const header = trimmed.match(/^\[\[?\s*([^\]]+?)\s*\]\]?/);
    if (header) {
      section = trimmed.startsWith("[[") ? "" : header[1].replace(/"/g, "");
      comments = [];
      continue;
    }
    const pair = trimmed.match(/^("[^"]+"|[A-Za-z0-9_.-]+)\s*=\s*(.*)$/);
    if (!pair) {
      comments = [];
      continue;
    }
    const key = pair[1].replace(/^"|"$/g, "");
    const value = stripComment(pair[2]).trim();
    const base = { path: section ? `${section}.${key}` : key, key, section, line: at, hint: comments.join(" ").trim(), items: 0, quoted: "" as const };
    comments = [];
    if (value.startsWith('"""') || value.startsWith("'''")) {
      fields.push({ ...base, value: "", kind: "block" });
    } else if (value.startsWith("[")) {
      const inner = value.replace(/^\[|\]$/g, "").trim();
      fields.push({ ...base, value: "", kind: "list", items: inner ? inner.split(",").filter((one) => one.trim()).length : 0 });
    } else if (value.startsWith("{")) {
      continue;
    } else if (value.startsWith('"') || value.startsWith("'")) {
      const quote = value[0] as "'" | '"';
      fields.push({ ...base, value: value.slice(1, -1).replace(/\\"/g, '"'), kind: "text", quoted: quote });
    } else {
      fields.push({ ...base, value, kind: value === "true" || value === "false" ? "bool" : NUMBER.test(value) ? "number" : "text" });
    }
  }
  return fields;
}

/** Writes one TOML value back on its line, keeping the key and any trailing comment. */
export function setTomlField(text: string, field: YamlField, value: string): string {
  const newline = text.includes("\r\n") ? "\r\n" : "\n";
  const lines = text.split(/\r?\n/);
  const raw = lines[field.line];
  if (raw === undefined) return text;
  const equals = raw.indexOf("=");
  if (equals < 0) return text;
  const after = raw.slice(equals + 1);
  const comment = after.slice(stripComment(after).length).match(/\s+#.*$/)?.[0] ?? "";
  const literal =
    field.kind === "bool" || field.kind === "number"
      ? value
      : field.quoted === "'" && !value.includes("'")
        ? `'${value}'`
        : `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
  lines[field.line] = `${raw.slice(0, equals + 1)} ${literal}${comment}`;
  return lines.join(newline);
}

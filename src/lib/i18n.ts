export type Language = string;

type Sectioned = Record<string, string | Record<string, string>>;
const files = import.meta.glob<Sectioned>("../lang/*.json", { import: "default" });

export function flatten(table: Sectioned): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, value] of Object.entries(table)) {
    if (typeof value === "string") out[key] = value;
    else Object.assign(out, value);
  }
  return out;
}

async function load(file: string | undefined): Promise<Record<string, string>> {
  return file ? flatten(await files[file]()) : {};
}

function codeOf(file: string): string | null {
  const name = file.split("/").pop() ?? "";
  const code = name.replace(/\.json$/, "");
  return code.includes(".") ? null : code;
}

export function languageCodes(): string[] {
  return Object.keys(files)
    .map(codeOf)
    .filter((code): code is string => code !== null)
    .sort((a, b) => (a === "ko" ? -1 : b === "ko" ? 1 : a.localeCompare(b)));
}

export async function languageNames(): Promise<Record<string, string>> {
  const out: Record<string, string> = {};
  for (const [file, read] of Object.entries(files)) {
    const code = codeOf(file);
    if (!code) continue;
    try {
      const name = (await read())["$language"];
      out[code] = typeof name === "string" ? name : code;
    } catch {
      out[code] = code;
    }
  }
  return out;
}

type Rust = {
  rustExact: Record<string, string>;
  rustPatterns: [RegExp, string][];
};
function rustTables(table: Record<string, string>): Rust {
  const rustExact: Record<string, string> = {};
  const rustPatterns: [RegExp, string][] = [];
  const slot = /\{[^{}]*\}|\{\{|\}\}/g;
  const keys = Object.keys(table).sort((x, y) => y.length - x.length);
  for (const key of keys) {
    const text = table[key];
    if (!/\{[^{}]*\}/.test(key.replace(/\{\{|\}\}/g, ""))) {
      rustExact[key.replace(/\{\{/g, "{").replace(/\}\}/g, "}")] = text;
      continue;
    }
    let source = "^";
    let last = 0;
    for (const found of key.matchAll(slot)) {
      const at = found.index ?? 0;
      source += escapeRegex(key.slice(last, at));
      source += found[0] === "{{" ? "\\{" : found[0] === "}}" ? "\\}" : "([\\s\\S]*?)";
      last = at + found[0].length;
    }
    source += escapeRegex(key.slice(last)) + "$";
    rustPatterns.push([new RegExp(source), text]);
  }
  return { rustExact, rustPatterns };
}

function escapeRegex(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

let words: Record<string, string> | null = null;
let rust: Rust | null = null;
let loadedFor: string | null = null;
let loading: Promise<void> | null = null;

export function loadLanguage(lang: Language = current): Promise<void> {
  if (loadedFor === lang) return Promise.resolve();
  const table = load(Object.keys(files).find((one) => codeOf(one) === lang));
  const rustFile = Object.keys(files).find((one) => one.endsWith(`/rust.${lang}.json`));
  const rustTable = rustFile ? load(rustFile) : Promise.resolve(null);
  loading = Promise.all([table, rustTable])
    .then(([found, foundRust]) => {
      words = found;
      rust = foundRust ? rustTables(foundRust) : null;
      loadedFor = lang;
    })
    .catch(() => {
      words = null;
      rust = null;
      loadedFor = null;
    })
    .finally(() => {
      loading = null;
    });
  return loading;
}

const LANGUAGE_KEY = "monolab-language";
function systemLanguage(): Language {
  const codes = languageCodes();
  const wanted = typeof navigator === "undefined" ? [] : [...(navigator.languages ?? []), navigator.language];
  for (const one of wanted) {
    const code = (one ?? "").toLowerCase().split("-")[0];
    if (codes.includes(code)) return code;
  }
  return "en";
}

function rememberedLanguage(): Language {
  try {
    const saved = localStorage.getItem(LANGUAGE_KEY);
    return saved && languageCodes().includes(saved) ? saved : systemLanguage();
  } catch {
    return systemLanguage();
  }
}
let current: Language = rememberedLanguage();
document.documentElement.lang = current;

export function language(): Language {
  return current;
}

export function setLanguage(next: Language): Promise<void> {
  current = next;
  if (loadedFor !== next) {
    words = null;
    rust = null;
    loadedFor = null;
  }
  try {
    localStorage.setItem(LANGUAGE_KEY, next);
  } catch {}
  document.documentElement.lang = next;
  return loadLanguage(next);
}

export function tr(ko: string, args?: unknown[]): string {
  const text = words?.[ko] ?? ko;
  return args ? particles(format(text, Object.fromEntries(args.map((value, index) => [String(index), value])), current)) : text;
}

const PAIRED = /(을\(를\)|를\(을\)|이\(가\)|가\(이\)|은\(는\)|는\(은\)|과\(와\)|와\(과\)|\(으\)로|\(이\))/g;
const CLOSERS = new Set(["'", '"', "’", "”", ")", "]", "》", "」", "』", "`"]);

/** Whether a word ends in a final consonant, and whether that is ㄹ. Null when it can't be told (symbols). */
function finalSound(word: string): { coda: boolean; rieul: boolean } | null {
  const ch = word.at(-1);
  if (!ch) return null;
  const code = ch.charCodeAt(0);
  if (code >= 0xac00 && code <= 0xd7a3) {
    const jong = (code - 0xac00) % 28;
    return { coda: jong !== 0, rieul: jong === 8 };
  }
  if (/[0-9]/.test(ch)) return { coda: !"2459".includes(ch), rieul: "178".includes(ch) };
  const tail = word.match(/[A-Za-z]+$/)?.[0];
  if (!tail) return null;
  if (tail === tail.toUpperCase() && tail.length <= 4) {
    const last = tail.slice(-1);
    return { coda: "LMNR".includes(last), rieul: "LR".includes(last) };
  }
  const low = tail.toLowerCase();
  if (low.endsWith("l")) return { coda: true, rieul: true };
  return { coda: /(m|n|ng)$/.test(low), rieul: false };
}

/** Turns paired particles like 을(를) into the one that fits the word in front of them. */
export function particles(text: string): string {
  if (!text.includes("(")) return text;
  return text.replace(PAIRED, (pair: string, _group: string, at: number) => {
    let end = at;
    while (end > 0 && CLOSERS.has(text[end - 1])) end -= 1;
    const before = text.slice(0, end);
    if (!before || /\s$/.test(before)) return pair;
    const sound = finalSound(before);
    if (!sound) return pair;
    switch (pair) {
      case "을(를)":
      case "를(을)":
        return sound.coda ? "을" : "를";
      case "이(가)":
      case "가(이)":
        return sound.coda ? "이" : "가";
      case "은(는)":
      case "는(은)":
        return sound.coda ? "은" : "는";
      case "과(와)":
      case "와(과)":
        return sound.coda ? "과" : "와";
      case "(으)로":
        return sound.coda && !sound.rieul ? "으로" : "로";
      default:
        return sound.coda ? "이" : "";
    }
  });
}

export function format(text: string, args: Record<string, unknown>, locale: string = current): string {
  if (!text.includes("{")) return text;
  try {
    return render(text, args, locale, null);
  } catch {
    return text;
  }
}

function asNumber(value: unknown): number | null {
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  if (typeof value === "string" && value.trim()) {
    const parsed = Number(value.replace(/,/g, ""));
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

let pluralRules: { locale: string; rules: Intl.PluralRules } | null = null;
function pluralCategory(count: number, locale: string): string {
  try {
    if (!pluralRules || pluralRules.locale !== locale)
      pluralRules = { locale, rules: new Intl.PluralRules(locale) };
    return pluralRules.rules.select(count);
  } catch {
    return count === 1 ? "one" : "other";
  }
}

function render(text: string, args: Record<string, unknown>, locale: string, hash: string | null): string {
  let out = "";
  let i = 0;
  while (i < text.length) {
    const ch = text[i];
    if (ch === "'") {
      if (text[i + 1] === "'") {
        out += "'";
        i += 2;
        continue;
      }
      if (text[i + 1] === "{" || text[i + 1] === "}" || (text[i + 1] === "#" && hash !== null)) {
        const close = text.indexOf("'", i + 1);
        const stop = close < 0 ? text.length : close;
        out += text.slice(i + 1, stop);
        i = stop + 1;
        continue;
      }
    }
    if (ch === "#" && hash !== null) {
      out += hash;
      i += 1;
      continue;
    }
    if (ch !== "{") {
      out += ch;
      i += 1;
      continue;
    }
    const end = matchingBrace(text, i);
    const inner = text.slice(i + 1, end);
    out += placeholder(inner, args, locale);
    i = end + 1;
  }
  return out;
}

function matchingBrace(text: string, open: number): number {
  let depth = 0;
  for (let i = open; i < text.length; i += 1) {
    if (text[i] === "'" && (text[i + 1] === "{" || text[i + 1] === "}" || text[i + 1] === "#")) {
      const close = text.indexOf("'", i + 1);
      i = close < 0 ? text.length : close;
      continue;
    }
    if (text[i] === "{") depth += 1;
    else if (text[i] === "}") {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  throw new Error("unbalanced braces");
}

function placeholder(inner: string, args: Record<string, unknown>, locale: string): string {
  const comma = inner.indexOf(",");
  const name = (comma < 0 ? inner : inner.slice(0, comma)).trim();
  const value = args[name];
  if (comma < 0) {
    return name in args ? String(value) : `{${inner}}`;
  }
  const rest = inner.slice(comma + 1);
  const comma2 = rest.indexOf(",");
  const kind = (comma2 < 0 ? rest : rest.slice(0, comma2)).trim();
  const body = comma2 < 0 ? "" : rest.slice(comma2 + 1);
  if (kind === "number") {
    const number = asNumber(value);
    return number === null ? String(value) : number.toLocaleString(locale);
  }
  if (kind === "plural" || kind === "select") {
    const options = parseOptions(body);
    let key: string;
    let hash: string | null = null;
    if (kind === "plural") {
      const number = asNumber(value);
      if (number === null) return String(value);
      hash = typeof value === "string" ? value : number.toLocaleString(locale);
      const exact = `=${number}`;
      key = exact in options ? exact : pluralCategory(number, locale);
    } else {
      key = String(value);
    }
    const chosen = options[key] ?? options.other;
    if (chosen === undefined) throw new Error("missing other branch");
    return render(chosen, args, locale, hash);
  }
  throw new Error(`unknown format: ${kind}`);
}

function parseOptions(body: string): Record<string, string> {
  const out: Record<string, string> = {};
  let i = 0;
  while (i < body.length) {
    while (i < body.length && /\s/.test(body[i])) i += 1;
    if (i >= body.length) break;
    const open = body.indexOf("{", i);
    if (open < 0) throw new Error("branch without braces");
    const key = body.slice(i, open).trim();
    const close = matchingBrace(body, open);
    out[key] = body.slice(open + 1, close);
    i = close + 1;
  }
  return out;
}

export function trBack(message: string): string {
  if (!message || !words) return message;
  const exact = words[message] ?? rust?.rustExact[message];
  if (exact) return exact;
  for (const [pattern, text] of rust?.rustPatterns ?? []) {
    const found = message.match(pattern);
    if (found) return particles(text.replace(/\$(\d+)/g, (_, n) => found[Number(n)] ?? ""));
  }
  return message;
}

export function t(key: string, values?: Record<string, string | number>): string {
  const text = words?.[key] ?? key;
  return values ? particles(format(text, values, current)) : text;
}

export async function wordsFor(lang: Language): Promise<Record<string, string>> {
  try {
    return await load(Object.keys(files).find((one) => codeOf(one) === lang));
  } catch {
    return {};
  }
}

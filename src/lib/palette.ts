/** Command palette: entries, fuzzy matching and ranking. Pure so it can be tested without the app. */

export type PaletteGroup = "recent" | "server" | "tab" | "action" | "app";

export type PaletteEntry = {
  id: string;
  label: string;
  hint: string;
  icon: string;
  group: PaletteGroup;
  /** The server this entry acts on. Entries with one are shown when the palette is scoped to it. */
  serverId?: string;
  /** Extra words that should find this entry (not shown). */
  keywords?: string;
  /** Picking this entry scopes the palette to a server instead of running something. */
  scopes?: string;
  run?: () => void;
};

const WORD_START = /[\s\-_./:›@]/;

/**
 * Scores how well `needle` matches `text` as an in-order subsequence, or null when it doesn't.
 * Higher is better: matches at word starts, runs of consecutive letters and an early first match count more.
 */
export function fuzzyScore(needle: string, text: string): number | null {
  const want = needle.toLowerCase();
  const have = text.toLowerCase();
  if (!want) return 0;
  const exact = have.indexOf(want);
  if (exact >= 0) {
    const atWord = exact === 0 || WORD_START.test(have[exact - 1]);
    return 100 + want.length * 4 + (atWord ? 40 : 0) - Math.min(exact, 30);
  }
  let score = 0;
  let from = 0;
  let last = -2;
  let first = -1;
  for (const ch of want) {
    const at = have.indexOf(ch, from);
    if (at < 0) return null;
    if (first < 0) first = at;
    score += 1;
    if (at === 0 || WORD_START.test(have[at - 1])) score += 6;
    if (at === last + 1) score += 4;
    last = at;
    from = at + 1;
  }
  return score - Math.min(first, 20) / 2;
}

/** Each word of the query must match the label, hint or keywords; the label counts most. */
export function entryScore(query: string, entry: PaletteEntry): number | null {
  const words = query.trim().split(/\s+/).filter(Boolean);
  if (!words.length) return 0;
  let total = 0;
  for (const word of words) {
    const label = fuzzyScore(word, entry.label);
    const rest = fuzzyScore(word, `${entry.hint} ${entry.keywords ?? ""}`);
    const best = Math.max(label === null ? -Infinity : label * 2, rest === null ? -Infinity : rest);
    if (best === -Infinity) return null;
    total += best;
  }
  return total;
}

export type RankOptions = {
  /** Only entries for this server. */
  scope?: string | null;
  /** Ids of recently run entries, newest first. */
  recent?: string[];
  /** The server on screen; its entries rank a little higher. */
  current?: string | null;
  limit?: number;
};

export function rankPalette(entries: PaletteEntry[], query: string, options: RankOptions = {}): PaletteEntry[] {
  const { scope = null, recent = [], current = null, limit = 40 } = options;
  const pool = scope ? entries.filter((one) => one.serverId === scope && !one.scopes) : entries;
  const recentAt = new Map(recent.map((id, index) => [id, index]));
  const needle = query.trim();
  if (!needle) {
    const seen = recent.map((id) => pool.find((one) => one.id === id)).filter((one): one is PaletteEntry => !!one);
    const shown = new Set(seen.map((one) => one.id));
    // Without a query, unscoped shows recent entries, then servers and app actions; scoped shows that server's list.
    const rest = pool.filter((one) => !shown.has(one.id) && (scope || one.group === "server" || one.group === "app"));
    return [...seen.map((one) => ({ ...one, group: "recent" as const })), ...rest].slice(0, limit);
  }
  const scored: { entry: PaletteEntry; score: number }[] = [];
  for (const entry of pool) {
    const score = entryScore(needle, entry);
    if (score === null) continue;
    const recency = recentAt.has(entry.id) ? 30 - Math.min(recentAt.get(entry.id) ?? 0, 30) : 0;
    const here = current && entry.serverId === current ? 15 : 0;
    scored.push({ entry, score: score + recency + here });
  }
  scored.sort((a, b) => b.score - a.score || a.entry.label.localeCompare(b.entry.label));
  return scored.slice(0, limit).map((one) => one.entry);
}

const RECENT_KEY = "monolab-palette-recent";

export function readRecent(): string[] {
  try {
    const saved = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(saved) ? saved.filter((one): one is string => typeof one === "string").slice(0, 8) : [];
  } catch {
    return [];
  }
}

export function rememberRecent(id: string, list: string[]): string[] {
  const next = [id, ...list.filter((one) => one !== id)].slice(0, 8);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    /* private mode */
  }
  return next;
}

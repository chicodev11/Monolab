import type { ConsoleLevel } from "../components/console/ConsoleOutput";

/** A log search with everything it narrows by. Times are the log's own clock, "HH:MM". */
export type LogFilter = {
  name: string;
  query: string;
  /** The levels shown; empty for every line. */
  levels: LogLevel[];
  /** A plugin, mod or logger name; "" for every one. */
  source: string;
  /** "" for no limit. */
  from: string;
  to: string;
};

export type LogLevel = Exclude<ConsoleLevel, "all">;

export const EMPTY_FILTER: LogFilter = { name: "", query: "", levels: [], source: "", from: "", to: "" };

const KEY = "monolab.savedLogFilters";
const MOST = 30;
export const LOG_LEVELS: LogLevel[] = ["error", "warn", "info"];

const inOrder = (levels: LogLevel[]) => LOG_LEVELS.filter((level) => levels.includes(level));

/** Turns one level on or off; all three on is the same as none (every line). */
export function toggleLevel(levels: readonly LogLevel[], one: LogLevel): LogLevel[] {
  const next = levels.includes(one) ? levels.filter((level) => level !== one) : [...levels, one];
  return next.length === LOG_LEVELS.length ? [] : inOrder(next);
}


/** Minutes after midnight of a line's `[HH:MM:SS` stamp, or null when it has none. */
export function lineMinute(line: string): number | null {
  const found = line.match(/^(?:\S+\s)?\[?(\d{2}):(\d{2})(?::\d{2})?/);
  if (!found) return null;
  const hour = Number(found[1]);
  const minute = Number(found[2]);
  return hour < 24 && minute < 60 ? hour * 60 + minute : null;
}

function minuteOf(text: string): number | null {
  const found = text.match(/^(\d{1,2}):(\d{2})$/);
  return found ? Number(found[1]) * 60 + Number(found[2]) : null;
}

/**
 * Who wrote a line: the plugin in Paper's `[12:00:00 INFO]: [Essentials] …`, or the logger in
 * Fabric/Forge's `[12:00:00] [Server thread/INFO] [fabric/]: …`. Null when it says nothing.
 */
export function lineSource(line: string): string | null {
  const head = line.indexOf("]: ");
  if (head < 0) return null;
  const forge = line.slice(0, head + 1).match(/\]\s*\[([^\]/]+?)\/?(?:[A-Za-z.$]*)?\]$/);
  if (forge && !/thread|\/(INFO|WARN|ERROR)/i.test(forge[0]) && !/^(INFO|WARN|WARNING|ERROR|DEBUG|FATAL)$/.test(forge[1])) {
    return forge[1].trim() || null;
  }
  const plugin = line.slice(head + 3).match(/^\[([A-Za-z0-9_.\- ]{2,40})\]/);
  return plugin ? plugin[1] : null;
}

/** Whether a line passes the filter's text, source and time (the level is handled by the log view). */
export function lineMatches(line: string, filter: Pick<LogFilter, "query" | "source" | "from" | "to">): boolean {
  const needle = filter.query.trim().toLowerCase();
  if (needle && !line.toLowerCase().includes(needle)) return false;
  if (filter.source && lineSource(line) !== filter.source) return false;
  const from = minuteOf(filter.from);
  const to = minuteOf(filter.to);
  if (from !== null || to !== null) {
    const at = lineMinute(line);
    if (at === null) return false;
    // A range like 22:00–02:00 runs past midnight.
    if (from !== null && to !== null && from > to) return at >= from || at <= to;
    if (from !== null && at < from) return false;
    if (to !== null && at > to) return false;
  }
  return true;
}

/** The sources in `lines`, the busiest first. */
export function sourcesIn(lines: readonly string[]): string[] {
  const counts = new Map<string, number>();
  for (const line of lines) {
    const source = lineSource(line);
    if (source) counts.set(source, (counts.get(source) ?? 0) + 1);
  }
  return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map(([name]) => name);
}

export function sameFilter(a: LogFilter, b: LogFilter): boolean {
  return a.query.trim() === b.query.trim() && a.levels.join() === b.levels.join() && a.source === b.source && a.from === b.from && a.to === b.to;
}

export function isEmptyFilter(filter: LogFilter): boolean {
  return sameFilter(filter, EMPTY_FILTER);
}

function clean(raw: unknown): LogFilter | null {
  if (!raw || typeof raw !== "object") return null;
  const one = raw as Record<string, unknown>;
  const text = (key: string) => (typeof one[key] === "string" ? (one[key] as string) : "");
  // Older saved filters kept a single "level".
  const picked = Array.isArray(one.levels) ? one.levels : typeof one.level === "string" ? [one.level] : [];
  const levels = LOG_LEVELS.filter((level) => picked.includes(level));
  const filter = { name: text("name").trim(), query: text("query"), levels: levels.length === LOG_LEVELS.length ? [] : levels, source: text("source"), from: text("from"), to: text("to") };
  return filter.name ? filter : null;
}

/** Saved filters, kept in this PC's browser storage. */
export function readSavedFilters(): LogFilter[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(raw) ? raw.map(clean).filter((one): one is LogFilter => one !== null).slice(0, MOST) : [];
  } catch {
    return [];
  }
}

export function writeSavedFilters(list: LogFilter[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(list.slice(0, MOST)));
  } catch {
    // Storage can be off; the list just doesn't stay.
  }
}

/** Saves `filter` under `name`, replacing one with the same name. */
export function withSaved(list: LogFilter[], filter: LogFilter, name: string): LogFilter[] {
  const named = { ...filter, name: name.trim() };
  if (!named.name) return list;
  return [named, ...list.filter((one) => one.name !== named.name)].slice(0, MOST);
}

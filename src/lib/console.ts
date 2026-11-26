import { tr } from "./i18n";

export type ConsoleRow = { line: string; noise?: boolean };

function withoutPrompt(line: string): string {
  return line.replace(/^\s*>\s?/, "");
}

export function readLevel(line: string): "error" | "warn" | null {
  line = withoutPrompt(line);
  const head = pumpkinLine(line)
    ? line.slice(0, 40)
    : line.includes("]: ")
      ? line.slice(0, line.indexOf("]: "))
      : "";
  if (/\b(ERROR|SEVERE|FATAL)\b/.test(head)) return "error";
  if (/\bWARN(ING)?\b/.test(head)) return "warn";
  return null;
}

export function pumpkinLine(
  line: string,
): { time: string; level: string; body: string } | null {
  const old = line.match(
    /^\d{4}-\d{2}-\d{2} (\d{2}:\d{2}:\d{2})\s+([A-Z]+)\s+\S+\s+ThreadId\(\d+\)\s+\S+?:\s?(.*)$/s,
  );
  if (old) return { time: old[1], level: old[2], body: old[3] ?? "" };
  const now = line.match(
    /^(\d{2}:\d{2}:\d{2})\s+(TRACE|DEBUG|INFO|WARN|ERROR)\s+(?:[a-z_]+(?:::[a-z_]+)*:\s)?(.*)$/s,
  );
  if (!now) return null;
  return { time: now[1], level: now[2], body: now[3] ?? "" };
}

// eslint-disable-next-line no-control-regex
const BRACKET_STAMP = /^((?:\u001b\[[0-9;:]*m)*)\[(?:\d{1,2}(?:[A-Za-z]{3}|\d{1,2}\uC6D4)\d{4} )?\d{2}:\d{2}:\d{2}(?:[.,]\d{1,3})?\](?:\u001b\[[0-9;:]*m)* ?/;
export function withoutLogStamp(line: string): string {
  return line.replace(BRACKET_STAMP, "$1");
}

// eslint-disable-next-line no-control-regex
const NEOFORGE_HEAD = /^((?:\u001b\[[0-9;:]*m)*)\[(?:\d{1,2}(?:[A-Za-z]{3}|\d{1,2}\uC6D4)\d{4} )?(\d{2}:\d{2}:\d{2})(?:[.,]\d{1,3})?\] \[[^\]]*\/([A-Z]+)\](?: \[([^\]]*)\])?: /;

export function tidyLogLine(line: string): string {
  const found = line.match(NEOFORGE_HEAD);
  if (!found) return withoutLogStamp(line);
  const [whole, ansi, time, level, logger = ""] = found;
  const [name] = logger.split("/");
  const short = (name.split(".").pop() ?? "").trim();
  const vanilla = !short || /^(net\.)?minecraft\b/.test(name) || ["MinecraftServer", "DedicatedServer"].includes(short);
  return `${ansi}[${time} ${level}]: ${vanilla ? "" : `[${short}] `}${line.slice(whole.length)}`;
}

export function readPlayerCommand(line: string): { player: string; command: string } | null {
  line = withoutPrompt(line);
  const body = line.includes("]: ") ? line.slice(line.indexOf("]: ") + 3) : line;
  const found = body.match(/^([A-Za-z0-9_]{1,16}) issued server command: (\/.*)$/);
  return found ? { player: found[1], command: found[2] } : null;
}

export function splitConsole(line: string): {
  time?: string;
  level?: string;
  body: string;
} {
  line = withoutPrompt(line);
  const pumpkin = pumpkinLine(line);
  if (pumpkin) return pumpkin;
  const found = line.match(
    /^\[(?:\d{1,2}[A-Za-z]{3}\d{4} )?(\d{2}:\d{2}:\d{2})(?:\.\d+)?(?:\s+([A-Za-z]+))?\]\s*(?:\[[^\]]*?\/?([A-Za-z]+)\])?:?\s?(.*)$/s,
  );
  if (!found) return { body: line };
  return {
    time: found[1],
    level: (found[2] ?? found[3])?.toUpperCase(),
    body: found[4] ?? "",
  };
}

export function readNotice(
  body: string,
): { say: string; tone: "good" | "info" } | null {
  const done = body.match(/^Done \(([\d.]+)s\)!/);
  if (done) return { say: tr("Server started!"), tone: "good" };
  const joined = body.match(/^(.+) joined the game$/);
  if (joined) return { say: tr("{0} joined", [joined[1]]), tone: "good" };
  const left = body.match(/^(.+) left the game$/);
  if (left) return { say: tr("{0} left", [left[1]]), tone: "info" };
  return null;
}

export function isErrorTail(line: string): boolean {
  line = withoutPrompt(line);
  if (line.includes("]: ")) return false;
  const body = line.trim();
  return (
    /^at\s\S/.test(body) ||
    body.startsWith("Caused by:") ||
    body.startsWith("Suppressed:") ||
    /^\.\.\.\s\d+\smore$/.test(body) ||
    /^[\w.$]+(Exception|Error)\b/.test(body)
  );
}

export type ConsoleChunk<T extends ConsoleRow = ConsoleRow> =
  | { kind: "error"; rows: T[]; key: string }
  | { kind: "line"; row: T; key: string };

export function groupConsole<T extends ConsoleRow>(
  rows: readonly T[],
  keepNoise = false,
): ConsoleChunk<T>[] {
  const out: ConsoleChunk<T>[] = [];
  let hiding = false;
  for (let index = 0; index < rows.length; index += 1) {
    const row = rows[index];
    if (row.noise || (hiding && isErrorTail(row.line))) {
      hiding = true;
      if (keepNoise) out.push({ kind: "line", row, key: `${index}-${row.line}` });
      continue;
    }
    hiding = false;
    if (readLevel(row.line) !== "error") {
      out.push({ kind: "line", row, key: `${index}-${row.line}` });
      continue;
    }
    const group: T[] = [row];
    while (
      index + 1 < rows.length &&
      !rows[index + 1].noise &&
      (readLevel(rows[index + 1].line) === "error" ||
        isErrorTail(rows[index + 1].line))
    ) {
      index += 1;
      group.push(rows[index]);
    }
    out.push({ kind: "error", rows: group, key: `${index}-err-${group.length}` });
  }
  return out;
}

export function readChat(line: string): { player: string; text: string } | null {
  line = withoutPrompt(line);
  const body = line.includes("]: ") ? line.slice(line.lastIndexOf("]: ") + 3) : line;
  if (!body.startsWith("<")) return null;
  const close = body.indexOf("> ");
  if (close < 2) return null;
  const player = body.slice(1, close);
  if (!player || player.includes(" ")) return null;
  return { player, text: body.slice(close + 2) };
}

/** Whether a command argument with this name takes a player (Brigadier node names and usage lines). */
export function wantsPlayerName(name: string | null | undefined): boolean {
  return /^(targets?|players?|player_?names?|target_?players?)$/i.test(name ?? "");
}

/** Splits at `sep` outside brackets: `(a|b) <c>` at " " is `(a|b)`, `<c>`. */
function splitTop(text: string, sep: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let from = 0;
  for (let at = 0; at < text.length; at++) {
    const char = text[at];
    if (char === "(" || char === "[" || char === "<") depth++;
    else if (char === ")" || char === "]" || char === ">") depth = Math.max(0, depth - 1);
    else if (char === sep && depth === 0) {
      out.push(text.slice(from, at));
      from = at + 1;
    }
  }
  out.push(text.slice(from));
  return out.filter((one) => one.length > 0);
}

/** Takes off brackets around the whole text: `[<reason>]` is `<reason>`, `(a|b)` is `a|b`. */
function unwrap(text: string): string {
  let out = text.trim();
  for (;;) {
    const open = out[0];
    const close = open === "(" ? ")" : open === "[" ? "]" : "";
    if (!close || out[out.length - 1] !== close) return out;
    let depth = 0;
    for (let at = 0; at < out.length - 1; at++) {
      if (out[at] === "(" || out[at] === "[") depth++;
      else if (out[at] === ")" || out[at] === "]") depth--;
      if (depth === 0) return out;
    }
    out = out.slice(1, -1).trim();
  }
}

/**
 * What a command's usage line says goes at argument `index` (0 = the first word after the command): the plain
 * words it offers and whether a player name fits there. `kick <targets> [<reason>]`, `difficulty (peaceful|easy)`,
 * `tp (<targets>|<location>)`.
 */
export function usageSlot(usage: string | null | undefined, index: number): { words: string[]; players: boolean } {
  const parts = splitTop((usage ?? "").trim().replace(/^\//, ""), " ");
  const token = parts[index + 1];
  const words: string[] = [];
  let players = false;
  if (!token) return { words, players };
  for (const alternative of splitTop(unwrap(token), "|")) {
    // A longer form (`add <targets>`) offers its first word here.
    const first = unwrap(splitTop(unwrap(alternative), " ")[0] ?? "");
    const argument = /^<([^<>]+)>$/.exec(first);
    if (argument) {
      if (wantsPlayerName(argument[1])) players = true;
    } else if (/^[\w.:-]+$/.test(first)) {
      words.push(first);
    }
  }
  return { words, players };
}

/**
 * The faded rest of a completion shown after the cursor, as Pumpkin's console does.
 * `typed` is the command without its leading spaces or slash; `hint` is a full completion
 * of that same text. Returns "" unless the hint really continues what was typed.
 */
export function ghostSuffix(typed: string, hint: string | undefined): string {
  if (!typed || !hint || hint.length <= typed.length) return "";
  if (!hint.toLowerCase().startsWith(typed.toLowerCase())) return "";
  return hint.slice(typed.length);
}

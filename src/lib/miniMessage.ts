/**
 * A small MiniMessage reader for studio previews: colors, hex, gradients, rainbow, decorations, reset and
 * newlines. Placeholders are filled in first, escaped, like the add-on does. Unknown tags stay as text,
 * as in MiniMessage.
 */

export type Run = {
  text: string;
  color?: string;
  bold?: boolean;
  italic?: boolean;
  underlined?: boolean;
  strikethrough?: boolean;
  obfuscated?: boolean;
};

/** Minecraft's 16 named text colors. */
export const NAMED_COLORS: Record<string, string> = {
  black: "#000000",
  dark_blue: "#0000aa",
  dark_green: "#00aa00",
  dark_aqua: "#00aaaa",
  dark_red: "#aa0000",
  dark_purple: "#aa00aa",
  gold: "#ffaa00",
  gray: "#aaaaaa",
  grey: "#aaaaaa",
  dark_gray: "#555555",
  dark_grey: "#555555",
  blue: "#5555ff",
  green: "#55ff55",
  aqua: "#55ffff",
  red: "#ff5555",
  light_purple: "#ff55ff",
  yellow: "#ffff55",
  white: "#ffffff",
};

const DECORATIONS: Record<string, keyof Run> = {
  b: "bold",
  bold: "bold",
  i: "italic",
  italic: "italic",
  em: "italic",
  u: "underlined",
  underlined: "underlined",
  st: "strikethrough",
  strikethrough: "strikethrough",
  obf: "obfuscated",
  obfuscated: "obfuscated",
};

/** Sample values for the add-on's placeholders. */
export const SAMPLE_VALUES: Record<string, string> = {
  player: "Steve",
  online: "12",
  max: "20",
  tps: "20.0",
  world: "world",
  ping: "42",
  message: "Hello there!",
};

export function escapeTags(text: string): string {
  return text.replace(/\\/g, "\\\\").replace(/</g, "\\<");
}

export function fillPlaceholders(text: string, values: Record<string, string>): string {
  return text.replace(/<([a-z_]+)>/g, (whole, name: string) => (name in values ? escapeTags(values[name]) : whole));
}

function colorOf(name: string): string | undefined {
  const lower = name.toLowerCase();
  if (/^#[0-9a-f]{6}$/.test(lower)) return lower;
  return NAMED_COLORS[lower];
}

function hex(color: string): [number, number, number] {
  return [1, 3, 5].map((at) => parseInt(color.slice(at, at + 2), 16)) as [number, number, number];
}

function mix(stops: string[], at: number): string {
  if (stops.length === 1) return stops[0];
  const spot = Math.min(Math.max(at, 0), 1) * (stops.length - 1);
  const from = Math.min(Math.floor(spot), stops.length - 2);
  const part = spot - from;
  const [a, b] = [hex(stops[from]), hex(stops[from + 1])];
  return `#${a.map((one, index) => Math.round(one + (b[index] - one) * part).toString(16).padStart(2, "0")).join("")}`;
}

function rainbow(at: number): string {
  const hue = at * 360;
  const k = (n: number) => (n + hue / 60) % 6;
  const f = (n: number) => 1 - Math.max(0, Math.min(k(n), 4 - k(n), 1));
  return `#${[f(5), f(3), f(1)].map((one) => Math.round(one * 255).toString(16).padStart(2, "0")).join("")}`;
}

type Frame = {
  tag: string;
  style: Omit<Run, "text">;
  /** A gradient or rainbow colors each character of what it wraps; the runs are recolored when it closes. */
  paint?: { stops?: string[]; first: number };
};

/** Reads MiniMessage into lines of styled runs. */
export function parseMiniMessage(input: string): Run[][] {
  const lines: Run[][] = [[]];
  const all: { line: number; run: Run }[] = [];
  const stack: Frame[] = [{ tag: "", style: {} }];
  const style = () => stack[stack.length - 1].style;
  const push = (text: string) => {
    if (!text) return;
    const run = { ...style(), text };
    lines[lines.length - 1].push(run);
    all.push({ line: lines.length - 1, run });
  };
  const close = (at: number) => {
    const frames = stack.splice(at);
    for (const frame of frames.reverse()) {
      if (!frame.paint) continue;
      const painted = all.slice(frame.paint.first);
      const count = painted.reduce((sum, one) => sum + [...one.run.text].length, 0);
      let index = 0;
      const replaced: { line: number; run: Run }[] = [];
      for (const one of painted) {
        const row = lines[one.line];
        const letters = [...one.run.text].map((text) => {
          const spot = count <= 1 ? 0 : index++ / (count - 1);
          return { ...one.run, text, color: frame.paint?.stops ? mix(frame.paint.stops, spot) : rainbow(spot) };
        });
        row.splice(row.indexOf(one.run), 1, ...letters);
        replaced.push(...letters.map((run) => ({ line: one.line, run })));
      }
      // An outer gradient repaints these letters again, so it must see the new runs.
      all.splice(frame.paint.first, painted.length, ...replaced);
    }
  };
  let at = 0;
  let plain = "";
  while (at < input.length) {
    const letter = input[at];
    if (letter === "\\" && (input[at + 1] === "<" || input[at + 1] === "\\")) {
      plain += input[at + 1];
      at += 2;
      continue;
    }
    if (letter !== "<") {
      plain += letter;
      at += 1;
      continue;
    }
    const end = input.indexOf(">", at);
    if (end < 0) {
      plain += input.slice(at);
      break;
    }
    const body = input.slice(at + 1, end);
    const handled = (() => {
      const closing = body.startsWith("/");
      const raw = closing ? body.slice(1) : body;
      const negate = raw.startsWith("!");
      const [head, ...args] = (negate ? raw.slice(1) : raw).split(":");
      const name = head.toLowerCase();
      if (!/^#?[a-z0-9_]+$/.test(name)) return false;
      if (closing) {
        const found = stack.map((one) => one.tag).lastIndexOf(name);
        if (found <= 0) return true;
        push(plain);
        plain = "";
        close(found);
        return true;
      }
      if (name === "newline" || name === "br") {
        push(plain);
        plain = "";
        lines.push([]);
        return true;
      }
      if (name === "reset") {
        push(plain);
        plain = "";
        close(1);
        return true;
      }
      const color = name === "color" || name === "colour" || name === "c" ? colorOf(args[0] ?? "") : colorOf(name);
      const decoration = DECORATIONS[name];
      if (!color && !decoration && name !== "gradient" && name !== "rainbow") return false;
      push(plain);
      plain = "";
      const next: Frame = { tag: name, style: { ...style() } };
      if (color) next.style.color = color;
      if (decoration) (next.style as Record<string, unknown>)[decoration] = !negate;
      if (name === "gradient") {
        const stops = args.map(colorOf).filter((one): one is string => !!one);
        next.paint = { stops: stops.length ? stops : ["#ffffff", "#000000"], first: all.length };
      }
      if (name === "rainbow") next.paint = { first: all.length };
      stack.push(next);
      return true;
    })();
    if (handled) {
      at = end + 1;
    } else {
      plain += "<";
      at += 1;
    }
  }
  push(plain);
  close(1);
  return lines.map((line) => line.filter((run) => run.text.length));
}

/** Lines of styled runs, with the add-on's placeholders filled in. */
export function miniMessageLines(text: string, values: Record<string, string> = SAMPLE_VALUES): Run[][] {
  return parseMiniMessage(fillPlaceholders(text, values));
}

/** The plain text a player would read. */
export function miniMessagePlain(text: string, values: Record<string, string> = SAMPLE_VALUES): string {
  return miniMessageLines(text, values)
    .map((line) => line.map((run) => run.text).join(""))
    .join("\n");
}

import { useSyncExternalStore } from "react";
import { tr } from "../../lib/i18n";
import { miniMessageLines } from "../../lib/miniMessage";
import type { AppGlyph } from "./studioGlyphs";
import type { StudioData } from "./StudioPreview";
import type { StudioNames } from "./studioKinds";

/*
 * Studio maker M2: canvas dialogs (`"layout": "canvas"`), the rules without the screen. The editor and the previews
 * draw with these, and they follow the add-ons (`StudioCanvas.java` on Paper, `crates/studio-format/src/canvas.rs`
 * on Pumpkin), which came from the user's DialogUI: 9-pixel rows, pictures at any pixel, text moved down by whole
 * pixels unless it has unifont letters (Korean...), click areas that grow to whole rows, and on overlaps a lower row
 * drawing over the rows above it. Keep the three in step.
 */

export const LINE = 9;
export const BODY_PADDING = 4;
export const WRAP_SLACK = 32;
export const MOST_BODY = 1024;
export const MOST_ELEMENTS = 400;
export const MOST_WIDTH = MOST_BODY - BODY_PADDING * 2 - WRAP_SLACK;
/** The pack's see-through hit glyph moves the pen this far (it sits at both ends of a row with click areas). */
const HIT_ADVANCE = 2;
export const CANVAS_TYPES = ["image", "text", "button", "area"] as const;
export type CanvasType = (typeof CANVAS_TYPES)[number];
export const AFTERS = ["close", "stay", "refresh"] as const;
export type After = (typeof AFTERS)[number];
export const ALIGNS = ["left", "center", "right"] as const;
/** Actions that take the player somewhere else, so the canvas isn't drawn again after them. */
export const LEAVING = ["dialog", "menu", "close", "shop", "trade"];
export const MOST_VARIABLES = 64;

/* ---------- letter widths ---------- */

/** The game's widths for the first 128 letters (with the 1-pixel gap), used until the full table has loaded. */
const ASCII: [number, number, number][] = [
  [0x20, 0x20, 4], [0x21, 0x21, 2], [0x22, 0x22, 4], [0x23, 0x26, 6], [0x27, 0x27, 2], [0x28, 0x2a, 4], [0x2b, 0x2b, 6], [0x2c, 0x2c, 2],
  [0x2d, 0x2d, 6], [0x2e, 0x2e, 2], [0x2f, 0x39, 6], [0x3a, 0x3b, 2], [0x3c, 0x3c, 5], [0x3d, 0x3d, 6], [0x3e, 0x3e, 5], [0x3f, 0x3f, 6],
  [0x40, 0x40, 7], [0x41, 0x48, 6], [0x49, 0x49, 4], [0x4a, 0x5a, 6], [0x5b, 0x5b, 4], [0x5c, 0x5c, 6], [0x5d, 0x5d, 4], [0x5e, 0x5f, 6],
  [0x60, 0x60, 3], [0x61, 0x65, 6], [0x66, 0x66, 5], [0x67, 0x68, 6], [0x69, 0x69, 2], [0x6a, 0x6a, 6], [0x6b, 0x6b, 5], [0x6c, 0x6c, 3],
  [0x6d, 0x73, 6], [0x74, 0x74, 4], [0x75, 0x7a, 6], [0x7b, 0x7b, 4], [0x7c, 0x7c, 2], [0x7d, 0x7d, 4], [0x7e, 0x7e, 7],
];

/** The add-ons' own table (`vanilla_widths.txt`: start, end, width, b = bitmap / u = unifont), once loaded. */
type Table = { starts: number[]; ends: number[]; widths: number[]; bitmap: boolean[] };
let table: Table | null = null;
let asked = false;
const listeners = new Set<() => void>();
/** Letters the table doesn't know (emoji...), like the add-ons. */
const UNKNOWN_WIDTH = 8;

export function parseWidths(text: string): Table {
  const out: Table = { starts: [], ends: [], widths: [], bitmap: [] };
  for (const line of text.split(/\r?\n/)) {
    if (!line || line.startsWith("#")) continue;
    const parts = line.split(" ");
    if (parts.length < 4) continue;
    const start = parseInt(parts[0], 16);
    const end = parseInt(parts[1], 16);
    const width = parseInt(parts[2], 10);
    if (!Number.isFinite(start) || !Number.isFinite(end) || !Number.isFinite(width)) continue;
    out.starts.push(start);
    out.ends.push(end);
    out.widths.push(width);
    out.bitmap.push(parts[3] === "b");
  }
  return out;
}

/** Uses a table now (tests, or one read elsewhere). */
export function setWidths(next: Table | null) {
  table = next;
  for (const one of listeners) one();
}

/** Loads the add-ons' width table once, in its own chunk. */
export function loadWidths(): void {
  if (asked) return;
  asked = true;
  void import("../../../crates/studio-format/src/vanilla_widths.txt?raw")
    .then((module: { default: string }) => setWidths(parseWidths(module.default)))
    .catch(() => {
      asked = false;
    });
}

/** True once the full width table is in; components re-render when it lands. */
export function useVanillaWidths(): boolean {
  loadWidths();
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => table !== null,
  );
}

function find(code: number): number {
  if (!table) return -1;
  let low = 0;
  let high = table.starts.length - 1;
  let at = -1;
  while (low <= high) {
    const middle = (low + high) >> 1;
    if (table.starts[middle] <= code) {
      at = middle;
      low = middle + 1;
    } else high = middle - 1;
  }
  return at >= 0 && code <= table.ends[at] ? at : -1;
}

/** A letter's advance in GUI pixels (with the 1-pixel gap). */
export function letterWidth(code: number): number {
  if (table) {
    const at = find(code);
    return at < 0 ? UNKNOWN_WIDTH : table.widths[at];
  }
  for (const [start, end, width] of ASCII) if (code >= start && code <= end) return width;
  if (code >= 0xabfa && code <= 0xd7af) return 8;
  if ((code >= 0x3130 && code <= 0x318f) || (code >= 0x3200 && code <= 0x9fff)) return 9;
  if (code < 0x20) return 9;
  return code < 0x2000 ? 6 : 8;
}

/** Bitmap letters move by the pixel (the text_N fonts); unifont ones (Hangul, CJK...) snap to a row. */
export function isBitmap(code: number): boolean {
  if (table) {
    const at = find(code);
    return at >= 0 && table.bitmap[at];
  }
  return code >= 0x20 && code < 0x2000;
}

export function textWidth(text: string): number {
  let width = 0;
  for (const letter of text) width += letterWidth(letter.codePointAt(0) ?? 32);
  return width;
}

/* ---------- reading the file ---------- */

export function plainOf(text: string): string {
  return miniMessageLines(text)
    .map((line) => line.map((run) => run.text).join(""))
    .join(" ");
}

export function num(data: StudioData, key: string, fallback: number, least: number, most: number): number {
  const value = data[key];
  return Math.min(most, Math.max(least, typeof value === "number" && Number.isFinite(value) ? Math.round(value) : fallback));
}

export function str(data: StudioData, key: string): string {
  const value = data[key];
  return typeof value === "string" ? value : "";
}

export function elementsOf(canvas: StudioData): StudioData[] {
  return Array.isArray(canvas.elements) ? canvas.elements.filter((one): one is StudioData => !!one && typeof one === "object" && !Array.isArray(one)) : [];
}

export function canvasSize(canvas: StudioData): { width: number; height: number } {
  return { width: num(canvas, "width", 176, 1, MOST_BODY), height: num(canvas, "height", 166, 1, 2048) };
}

/** What a click does to the dialog: `after`, or the older `"close": false` (stay). */
export function afterOf(canvas: StudioData): After {
  const after = canvas.after;
  if (typeof after === "string" && (AFTERS as readonly string[]).includes(after)) return after as After;
  return canvas.close === false ? "stay" : "close";
}

/** The leaving action types anywhere in some actions (not inside conditions). */
export function leavingOf(steps: unknown): string[] {
  const out = new Set<string>();
  const walk = (value: unknown, depth: number) => {
    if (depth > 32) return;
    if (Array.isArray(value)) value.forEach((one) => walk(one, depth + 1));
    else if (value && typeof value === "object") {
      const map = value as StudioData;
      if (typeof map.type === "string" && LEAVING.includes(map.type)) out.add(map.type);
      for (const [key, inner] of Object.entries(map)) if (key !== "if") walk(inner, depth + 1);
    }
  };
  walk(steps, 0);
  return [...out];
}

export function conditionsOf(el: StudioData): StudioData[] {
  return Array.isArray(el.if) ? el.if.filter((one): one is StudioData => !!one && typeof one === "object" && !Array.isArray(one)) : [];
}

export function validGlyph(name: string): boolean {
  return /^[a-z0-9_]{1,32}$/.test(name);
}

/* ---------- layout ---------- */

/** Where one element lands in the game. */
export type Laid = {
  index: number;
  el: StudioData;
  type: CanvasType | "";
  x: number;
  y: number;
  w: number;
  h: number;
  /** The 9-pixel row it is drawn with (decides overlaps). */
  line: number;
  glyph: AppGlyph | null;
  /** A picture element whose glyph isn't known. */
  missing: boolean;
  /** Text with unifont letters, moved to the nearest row. */
  snapped: boolean;
  plain: string;
  /** How far it moves the pen (decides how wide the body gets). */
  advance: number;
  /** Drawn with its pressed glyph. */
  pressed: boolean;
};

/** Every element where the game draws it, in the game's drawing order (later draws on top). `pressed`: a button to
 * draw with its pressed picture. */
export function layoutCanvas(canvas: StudioData, glyphs: Record<string, AppGlyph>, pressed: number | null = null): Laid[] {
  const laid = elementsOf(canvas)
    .slice(0, MOST_ELEMENTS)
    .map((el, index): Laid => {
      const type = (CANVAS_TYPES as readonly string[]).includes(str(el, "type")) ? (str(el, "type") as CanvasType) : "";
      const one: Laid = { index, el, type, x: num(el, "x", 0, -2048, 2048), y: num(el, "y", 0, -2048, 2048), w: 0, h: 0, line: 0, glyph: null, missing: false, snapped: false, plain: "", advance: 0, pressed: false };
      if (type === "image" || type === "button") {
        const down = pressed === index && type === "button" && validGlyph(str(el, "pressed")) ? glyphs[str(el, "pressed")] : undefined;
        one.pressed = !!down;
        one.glyph = down ?? glyphs[str(el, "glyph")] ?? null;
        one.missing = !one.glyph;
        one.w = one.glyph?.width ?? 16;
        one.h = one.glyph?.height ?? 16;
        one.advance = one.glyph?.advance ?? 0;
        one.line = Math.floor(one.y / LINE);
      } else if (type === "text") {
        one.plain = plainOf(str(el, "text"));
        one.advance = textWidth(one.plain);
        const shown = Math.max(0, one.advance - 1);
        const align = str(el, "align");
        one.x = align === "center" ? one.x - Math.floor(shown / 2) : align === "right" ? one.x - shown : one.x;
        one.w = Math.max(1, shown);
        one.h = 8;
        const bitmap = [...one.plain].every((letter) => isBitmap(letter.codePointAt(0) ?? 32));
        one.line = bitmap ? Math.floor(one.y / LINE) : Math.round(one.y / LINE);
        if (!bitmap) {
          one.snapped = one.y % LINE !== 0;
          one.y = one.line * LINE;
        }
      } else if (type === "area") {
        one.w = num(el, "w", 0, 0, MOST_BODY);
        one.h = num(el, "h", 0, 0, 2048);
        one.advance = one.w;
        one.line = Math.floor(one.y / LINE);
      }
      return one;
    });
  return laid.slice().sort((a, b) => a.line - b.line || a.index - b.index);
}

/** An area's real click box: whole rows from the first it touches to the last. */
export function hitBox(one: Laid): { x: number; y: number; w: number; h: number } {
  const first = Math.floor(one.y / LINE);
  const last = Math.floor((one.y + Math.max(1, one.h) - 1) / LINE);
  return { x: one.x, y: first * LINE, w: one.w, h: (last - first + 1) * LINE };
}

/** The box a click lands in: an area's whole rows, a picture's own box. */
export function clickBox(one: Laid): { x: number; y: number; w: number; h: number } {
  return one.type === "area" ? hitBox(one) : { x: one.x, y: one.y, w: one.w, h: one.h };
}

type Box = { x: number; y: number; w: number; h: number };

export function boxesMeet(a: Box, b: Box): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/** How the body comes out: rows drawn, how wide the widest row's pen goes, and the body width the dialog gets. */
export function bodyOf(canvas: StudioData, laid: Laid[]): { rows: number; reach: number; body: number } {
  const { width, height } = canvasSize(canvas);
  let rows = Math.max(1, Math.ceil(height / LINE));
  const pens = new Map<number, number>();
  const reach = (line: number, right: number) => pens.set(line, Math.max(pens.get(line) ?? 0, right));
  for (const one of laid) {
    if (one.type === "image" || one.type === "button") {
      if (!one.glyph) continue;
      reach(one.line, one.x + one.advance);
      rows = Math.max(rows, one.line + 1);
    } else if (one.type === "text") {
      reach(one.line, one.x + one.advance);
      rows = Math.max(rows, one.line + 1);
    } else if (one.type === "area" && one.w > 0 && one.h > 0) {
      const last = Math.floor((one.y + one.h - 1) / LINE);
      for (let line = one.line; line <= last; line += 1) reach(line, one.x + one.w + HIT_ADVANCE - 1);
      rows = Math.max(rows, last + 1);
    }
  }
  const most = Math.max(width, ...pens.values());
  return { rows, reach: most, body: most + BODY_PADDING * 2 + WRAP_SLACK };
}

/* ---------- problems ---------- */

export type Problem = { index: number | null; level: "error" | "warn" | "info"; message: string };

function walkSteps(value: unknown, visit: (step: StudioData) => void, depth = 0) {
  if (depth > 32) return;
  if (Array.isArray(value)) value.forEach((one) => walkSteps(one, visit, depth + 1));
  else if (value && typeof value === "object") {
    const map = value as StudioData;
    if (typeof map.type === "string") visit(map);
    for (const [key, inner] of Object.entries(map)) if (key !== "if") walkSteps(inner, visit, depth + 1);
  }
}

/** Studio files actions name that don't exist (when the names are known). */
function missingRefs(steps: unknown, names: StudioNames | undefined): string[] {
  if (!names) return [];
  const out: string[] = [];
  const kinds: (keyof StudioNames)[] = ["dialog", "menu", "function", "shop", "trade", "loot"];
  walkSteps(steps, (step) => {
    const kind = kinds.find((one) => one === step.type);
    if (!kind) return;
    const wanted = typeof step[kind] === "string" ? (step[kind] as string).trim() : "";
    if (wanted && names[kind].length > 0 && !names[kind].includes(wanted)) out.push(`${kind} ${wanted}`);
  });
  return out;
}

/**
 * What may go wrong in the game, like DialogUI's editor: the add-ons' own checks (an error there and the add-on
 * won't load the dialog), and what will look or act differently than the editor suggests. `known` is false while
 * the glyphs are still loading.
 */
export function canvasProblems(dialog: StudioData, canvas: StudioData, laid: Laid[], known: boolean, names?: StudioNames): Problem[] {
  const out: Problem[] = [];
  const add = (index: number | null, level: Problem["level"], message: string) => out.push({ index, level, message });
  const { width, height } = canvasSize(canvas);
  const after = afterOf(canvas);
  if (width > MOST_WIDTH) add(null, "error", tr("The canvas is too wide (at most {0}).", [MOST_WIDTH]));
  else if (width > 320) add(null, "warn", tr("{0} wide: on a small screen at GUI scale 4 it can go off the screen (320 or less is safe).", [width]));
  if (height > 220) add(null, "warn", tr("{0} high: small screens get a scroll bar (220 or less is safe).", [height]));
  if (canvas.after !== undefined && !(AFTERS as readonly unknown[]).includes(canvas.after)) add(null, "error", tr("After a click must be close, stay or refresh."));
  if (elementsOf(canvas).length > MOST_ELEMENTS) add(null, "error", tr("A canvas can have at most {0} elements.", [MOST_ELEMENTS]));
  const vars = dialog.variables;
  if (vars !== undefined) {
    if (!vars || typeof vars !== "object" || Array.isArray(vars)) add(null, "error", tr("Starting variables must be names and values."));
    else {
      const entries = Object.entries(vars as StudioData);
      if (entries.length > MOST_VARIABLES) add(null, "error", tr("A dialog can start at most {0} variables.", [MOST_VARIABLES]));
      for (const [name, value] of entries) {
        if (!name.trim() || name.length > 64) add(null, "error", tr("A variable needs a name of 1 to 64 letters."));
        else if (!["string", "number", "boolean"].includes(typeof value)) add(null, "error", tr("The variable {0} needs a plain value (text or a number).", [name]));
      }
    }
  }
  for (const ref of missingRefs(dialog.onOpen, names)) add(null, "error", tr("On open: there is no {0}.", [ref]));
  if (JSON.stringify(dialog.onOpen ?? []).includes("<input:")) add(null, "warn", tr("On open there are no inputs yet, so <input:…> is empty."));

  const body = bodyOf(canvas, laid);
  if (body.body > MOST_BODY) add(null, "error", tr("The rows reach {0} pixels across, too wide for a dialog: the add-on won't draw it.", [body.reach]));
  else if (body.reach > width) add(null, "warn", tr("Something reaches {0} pixels across, past the canvas width {1}: the dialog gets wider.", [body.reach, width]));
  const rows = Math.ceil(height / LINE);
  if (body.rows > rows) add(null, "warn", tr("Something goes below the canvas: {0} more rows, so the dialog gets taller.", [body.rows - rows]));

  for (const one of laid) {
    const n = one.index;
    if (!one.type) {
      add(n, "error", tr("Pick what this element is (image, text, button or area)."));
      continue;
    }
    const clickable = one.type === "button" || one.type === "area";
    if (num(one.el, "y", 0, -2048, 2048) < 0) add(n, "error", tr("It is above the canvas (y below 0)."));
    if (one.el.if !== undefined && !Array.isArray(one.el.if)) add(n, "error", tr("Its conditions must be a list."));
    if (one.x < 0) add(n, "warn", tr("It starts left of the canvas; the game may cut it off."));
    else if (one.x + one.w > width || one.y + one.h > height) add(n, "info", tr("It goes past the canvas edge."));
    if (one.type === "image" || one.type === "button") {
      const name = str(one.el, "glyph");
      if (!validGlyph(name)) add(n, "error", tr("Pick a glyph picture."));
      else if (known && !one.glyph) add(n, "error", str(one.el, "item") ? tr("The icon for {0} isn't a glyph yet. Pick the item again to make it.", [str(one.el, "item")]) : tr("There is no glyph :{0}:.", [name]));
      else if (one.glyph && one.glyph.height < 8 && one.glyph.pictureHeight * Math.ceil(8 / one.glyph.height) > 255) add(n, "error", tr("This glyph is too small for a canvas. Make its height 8 or more."));
    }
    if (one.type === "button") {
      const pressed = str(one.el, "pressed");
      if (pressed) {
        // Whether the glyph exists is pressedProblems' part (it needs the table).
        if (!validGlyph(pressed)) add(n, "error", tr("The pressed picture must be a glyph name."));
        if (after === "close") add(n, "info", tr("The pressed picture only shows while the dialog stays open (after a click: stay or refresh)."));
        else if (leavingOf(one.el.actions).length) add(n, "info", tr("Its actions open or close something, so the pressed picture is skipped."));
      }
    } else if (str(one.el, "pressed")) add(n, "info", tr("Only buttons use a pressed picture."));
    if (one.type === "text") {
      if (one.snapped) add(n, "warn", tr("Text with letters like Korean can't move by the pixel, so y {0} shows at {1}. Use a multiple of 9.", [num(one.el, "y", 0, -2048, 2048), one.y]));
      const text = str(one.el, "text");
      if (/<(b|bold)>/i.test(text)) add(n, "info", tr("Bold letters are 1 pixel wider each, which the add-on doesn't count: centered or right-aligned bold text sits a little off."));
      if (/\n|<newline>|<br>/i.test(text)) add(n, "info", tr("A canvas text is one row: line breaks become spaces."));
      if (!one.plain.trim()) add(n, "warn", tr("The text is empty."));
    }
    if (one.type === "area") {
      if (one.w <= 0 || one.h <= 0) add(n, "error", tr("A click area needs a width and height."));
      else if (one.y % LINE !== 0 || (one.y + one.h) % LINE !== 0) add(n, "info", tr("Click areas grow to whole 9-pixel rows ({0} to {1}). Use a button for an exact box.", [hitBox(one).y, hitBox(one).y + hitBox(one).h]));
    }
    if (clickable) {
      const steps = one.el.actions;
      if (!(Array.isArray(steps) && steps.length)) add(n, "warn", tr("Clicking it does nothing yet (no actions)."));
      // A click on the canvas is a text click: the game sends only that, never what was typed into the inputs.
      if (JSON.stringify(steps ?? []).includes("<input:")) add(n, "warn", tr("Clicks on the canvas can't read the inputs, so <input:…> stays empty here. Read them from a button under the canvas."));
      for (const ref of missingRefs(steps, names)) add(n, "error", tr("Its actions name {0}, which doesn't exist.", [ref]));
      if (after === "refresh" && leavingOf(steps).some((type) => type !== "close")) add(n, "info", tr("It opens something else, so the canvas isn't drawn again after it."));
    }
  }
  // An element later in the list but drawn under an earlier one, because its row is higher up.
  const drawn = laid.filter((one) => one.type === "image" || one.type === "button" || one.type === "text");
  for (let a = 0; a < drawn.length; a += 1) {
    for (let b = a + 1; b < drawn.length; b += 1) {
      const under = drawn[a];
      const over = drawn[b];
      if (over.index < under.index && boxesMeet(under, over)) add(under.index, "warn", tr("It is after #{0} in the list but the game draws it under #{0}, because it starts on a higher row.", [over.index + 1]));
    }
  }
  // Clicks: where two clickables meet, the one drawn later gets the click.
  const clicks = laid.filter((one) => (one.type === "button" && one.glyph) || (one.type === "area" && one.w > 0 && one.h > 0));
  for (let a = 0; a < clicks.length; a += 1) {
    for (let b = a + 1; b < clicks.length; b += 1) {
      if (boxesMeet(clickBox(clicks[a]), clickBox(clicks[b]))) add(clicks[a].index, "info", tr("Where it meets #{0}, clicks go to #{0} (drawn later).", [clicks[b].index + 1]));
    }
  }
  return out;
}

/** Problems about a button's pressed picture that need the glyph table. */
export function pressedProblems(laid: Laid[], glyphs: Record<string, AppGlyph>, known: boolean): Problem[] {
  const out: Problem[] = [];
  if (!known) return out;
  for (const one of laid) {
    if (one.type !== "button") continue;
    const name = str(one.el, "pressed");
    if (!validGlyph(name)) continue;
    const down = glyphs[name];
    if (!down) out.push({ index: one.index, level: "error", message: tr("There is no glyph :{0}: for the pressed picture.", [name]) });
    else if (one.glyph && (down.width !== one.glyph.width || down.height !== one.glyph.height)) out.push({ index: one.index, level: "info", message: tr("The pressed picture is {0}×{1}, the button {2}×{3}: it will jump when pressed.", [down.width, down.height, one.glyph.width, one.glyph.height]) });
  }
  return out;
}

/** A label for an element in lists. */
export function elementLabel(el: StudioData): string {
  const name = str(el, "name").trim();
  if (name) return name;
  const type = str(el, "type");
  if (type === "text") return plainOf(str(el, "text")).trim() || tr("Empty text");
  if (str(el, "item")) return str(el, "item");
  if (type === "image" || type === "button") return str(el, "glyph") ? `:${str(el, "glyph")}:` : tr("No picture");
  if (type === "area") return `${num(el, "w", 0, 0, MOST_BODY)}×${num(el, "h", 0, 0, 2048)}`;
  return tr("Element");
}

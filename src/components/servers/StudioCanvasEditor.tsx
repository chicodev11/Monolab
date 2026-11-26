import { type CSSProperties, type DragEvent, type KeyboardEvent, type PointerEvent, type ReactNode, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { tr, trBack } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { onWeb, webSession } from "../../lib/bridge";
import { uploadRaw } from "../../lib/pickFile";
import { SegmentedControl, Switch } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { listOf, StepList } from "./StudioActions";
import { CanvasPiece } from "./StudioCanvas";
import { ItemField } from "./StudioItemPicker";
import { MiniText, type StudioData } from "./StudioPreview";
import { useArt } from "./gameArt";
import { type AppGlyph, byName, reloadGlyphs, usePicture, useStudioGlyphs } from "./studioGlyphs";
import { objectOf, type StudioNames } from "./studioKinds";
import {
  AFTERS,
  ALIGNS,
  type After,
  afterOf,
  bodyOf,
  canvasProblems,
  canvasSize,
  clickBox,
  conditionsOf,
  elementLabel,
  elementsOf,
  hitBox,
  type Laid,
  layoutCanvas,
  LINE,
  MOST_ELEMENTS,
  MOST_WIDTH,
  num,
  type Problem,
  pressedProblems,
  str,
  useVanillaWidths,
} from "./canvasModel";
import "./StudioCanvasEditor.css";

/*
 * The canvas dialog editor, built into the studio in place of DialogUI's web editor. A drafting table: rulers in GUI
 * pixels and 9-pixel rows around a stage that draws the canvas with the add-ons' rules (canvasModel.ts), layers and
 * pictures on the left, the inspector on the right, and the problems the game will have along the bottom.
 *
 * Keys (with the stage or the layers focused): arrows move 1 pixel, Shift+arrows 9 (a row); Ctrl+Z / Ctrl+Y (or
 * Ctrl+Shift+Z) undo and redo; Ctrl+D duplicates, Ctrl+C / Ctrl+V copy and paste, Delete removes; Alt+Up/Down moves
 * a layer; PageUp/PageDown pick the next element; Escape lets go; Ctrl+wheel, + and - zoom, 0 fits.
 * While dragging, Shift snaps y to a row and Alt turns off the edge guides.
 */

const MOST_ZOOM = 8;
const MOST_HISTORY = 200;
const GLYPH_DRAG = "application/x-monolab-glyph";
/** Where the guides catch an edge, in GUI pixels. */
const SNAP = 3;

type Tool = "image" | "button" | "text" | "area" | "item";
type Drag =
  | { mode: "move"; index: number; startX: number; startY: number; x: number; y: number; box: { x: number; y: number; w: number; h: number } }
  | { mode: "resize"; index: number; dir: string; startX: number; startY: number; x: number; y: number; w: number; h: number }
  | { mode: "pan"; startX: number; startY: number; left: number; top: number };
type Guides = { x: number[]; y: number[] };
type History = { past: StudioData[]; future: StudioData[]; key: string | null; at: number };

/** Copied elements outlive the editor, so one can go from one dialog to another. */
let clipboard: StudioData | null = null;

function copied(): StudioData | null {
  return clipboard;
}

function copy(one: StudioData) {
  clipboard = structuredClone(one);
}

function clamp(value: number, least: number, most: number): number {
  return Math.min(most, Math.max(least, value));
}

function typeLabel(type: string): string {
  switch (type) {
    case "image":
      return tr("Picture");
    case "text":
      return tr("Text");
    case "button":
      return tr("Button");
    case "area":
      return tr("Click area");
    default:
      return tr("Element");
  }
}

function typeIcon(el: StudioData): string {
  if (str(el, "item")) return "package";
  switch (str(el, "type")) {
    case "image":
      return "image";
    case "text":
      return "tag";
    case "button":
      return "play";
    case "area":
      return "select-all";
    default:
      return "info";
  }
}

function afterHint(after: After): string {
  switch (after) {
    case "stay":
      return tr("The dialog stays open; clicks run their actions and nothing redraws.");
    case "refresh":
      return tr("The dialog waits, then draws again with new variables and conditions (like a chest menu).");
    default:
      return tr("A click closes the dialog after its actions.");
  }
}

/** Indexes kept in a set (hidden, locked) after the list changed: `map` gives an index's new place, or null. */
function remap(set: Set<number>, map: (at: number) => number | null): Set<number> {
  const out = new Set<number>();
  for (const at of set) {
    const next = map(at);
    if (next !== null) out.add(next);
  }
  return out;
}

function moveMap(from: number, to: number) {
  return (at: number) => {
    if (at === from) return to;
    if (from < to && at > from && at <= to) return at - 1;
    if (from > to && at >= to && at < from) return at + 1;
    return at;
  };
}

/** A font file id from a name: a-z, 0-9 and _, at most 32. */
function glyphId(name: string): string {
  return (
    name
      .toLowerCase()
      .replace(/\.png$/, "")
      .replace(/[^a-z0-9_]+/g, "_")
      .replace(/^_+|_+$/g, "")
      .slice(0, 32) || "glyph"
  );
}

/** Makes a glyph image (a `font` file) for a studio texture; resolves its name. An existing one is used as it is. */
async function makeGlyph(serverId: string, image: string, height: number, taken: Set<string>): Promise<string> {
  const base = glyphId(image);
  for (let n = 1; n < 8; n += 1) {
    const id = n === 1 ? base : glyphId(base.slice(0, 29) + "_" + n);
    if (taken.has(id)) continue;
    try {
      await commands.studioSave(serverId, "font", id, JSON.stringify({ image, height: clamp(height, 1, 256), ascent: Math.min(7, height) }, null, 2) + "\n", "");
      try {
        await commands.studioPackBuild(serverId);
      } catch {
        // The glyph shows in the editor already; the pack builds again with the next change.
      }
      reloadGlyphs(serverId);
      return id;
    } catch (error) {
      if (!String(error).includes("already exists")) throw error;
    }
  }
  throw new Error(tr("Could not find a free name for the glyph."));
}

async function addTexture(serverId: string, file: File): Promise<void> {
  if (onWeb) await commands.studioAssetAddUpload(serverId, "textures", file.name, await uploadRaw(file, webSession()));
  else {
    const bytes = new Uint8Array(await file.arrayBuffer());
    let text = "";
    for (let at = 0; at < bytes.length; at += 0x8000) text += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
    await commands.studioAssetAdd(serverId, "textures", file.name, btoa(text));
  }
}

/** An item's game icon as a 16×16 studio texture and glyph (`item_diamond`); resolves the glyph name. */
async function makeItemGlyph(serverId: string, item: string, icon: string, taken: Set<string>): Promise<string> {
  const id = glyphId("item_" + item.replace(/^minecraft:/, ""));
  const picture = new Image();
  picture.src = icon;
  await picture.decode();
  const sheet = document.createElement("canvas");
  sheet.width = 16;
  sheet.height = 16;
  const ctx = sheet.getContext("2d");
  if (!ctx) throw new Error(tr("The icon couldn't be drawn."));
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(picture, 0, 0, 16, 16);
  const blob = await new Promise<Blob | null>((done) => sheet.toBlob(done, "image/png"));
  if (!blob) throw new Error(tr("The icon couldn't be drawn."));
  await addTexture(serverId, new File([blob], id + ".png", { type: "image/png" }));
  return makeGlyph(serverId, id, 16, taken);
}

/**
 * The canvas editor. `dialog` is the whole dialog file (the canvas is its `canvas`; title, variables and on-open
 * actions are the dialog's own); every change comes back through `onChange`.
 */
export function StudioCanvasEditor({ dialog, onChange, names, serverId, onBack }: { dialog: StudioData; onChange: (next: StudioData) => void; names: StudioNames; serverId?: string; onBack?: () => void }) {
  useVanillaWidths();
  const { glyphs, ready } = useStudioGlyphs(serverId);
  const named = useMemo(() => byName(glyphs), [glyphs]);
  const canvas = objectOf(dialog.canvas);
  const elements = elementsOf(canvas);
  const { width, height } = canvasSize(canvas);

  const [picked, setPicked] = useState<number | null>(null);
  const [zoom, setZoom] = useState(2);
  const [fitted, setFitted] = useState(false);
  const [rows, setRows] = useState(true);
  const [hits, setHits] = useState(true);
  const [pressing, setPressing] = useState(false);
  const [hidden, setHidden] = useState<Set<number>>(() => new Set());
  const [locked, setLocked] = useState<Set<number>>(() => new Set());
  const [tab, setTab] = useState<"layers" | "pictures">("layers");
  const [problemsOpen, setProblemsOpen] = useState(false);
  const [pointer, setPointer] = useState<{ x: number; y: number } | null>(null);
  const [guides, setGuides] = useState<Guides | null>(null);
  const [said, setSaid] = useState("");
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const history = useRef<History>({ past: [], future: [], key: null, at: 0 });
  const [steps, setSteps] = useState({ back: 0, ahead: 0 });
  const drag = useRef<Drag | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const surface = useRef<HTMLDivElement>(null);
  const space = useRef(false);

  const current = picked !== null && picked < elements.length ? elements[picked] : null;
  const pressedAt = pressing && current && str(current, "type") === "button" ? picked : null;
  const laid = layoutCanvas(canvas, named, pressedAt);
  const laidAt = new Map(laid.map((one) => [one.index, one]));
  const order = new Map(laid.map((one, at) => [one.index, at]));
  const problems: Problem[] = [...canvasProblems(dialog, canvas, laid, ready, names), ...pressedProblems(laid, named, ready)];
  const body = bodyOf(canvas, laid);
  const worst = problems.some((one) => one.level === "error") ? "error" : problems.some((one) => one.level === "warn") ? "warn" : problems.length ? "info" : "";

  /* ----- changes and history ----- */

  const commit = useCallback(
    (next: StudioData, key: string | null = null) => {
      const h = history.current;
      const now = Date.now();
      if (!(key && h.key === key && now - h.at < 900)) {
        h.past.push(dialog);
        if (h.past.length > MOST_HISTORY) h.past.shift();
      }
      h.future = [];
      h.key = key;
      h.at = now;
      setSteps({ back: h.past.length, ahead: 0 });
      onChange(next);
    },
    [dialog, onChange],
  );
  const setCanvas = (next: StudioData, key: string | null = null) => commit({ ...dialog, canvas: next }, key);
  const setElements = (next: StudioData[], key: string | null = null) => setCanvas({ ...canvas, elements: next }, key);
  const setElement = (at: number, next: StudioData, key: string | null = null) => setElements(elements.map((one, index) => (index === at ? next : one)), key);
  const patch = (at: number, changes: StudioData, key: string | null = null) => {
    const next = { ...elements[at], ...changes };
    for (const [name, value] of Object.entries(changes)) if (value === undefined) delete next[name];
    setElement(at, next, key);
  };
  const undo = () => {
    const h = history.current;
    const back = h.past.pop();
    if (!back) return;
    h.future.push(dialog);
    h.key = null;
    setSteps({ back: h.past.length, ahead: h.future.length });
    onChange(back);
    setSaid(tr("Undone"));
  };
  const redo = () => {
    const h = history.current;
    const next = h.future.pop();
    if (!next) return;
    h.past.push(dialog);
    h.key = null;
    setSteps({ back: h.past.length, ahead: h.future.length });
    onChange(next);
    setSaid(tr("Redone"));
  };

  const insert = (made: StudioData, at = elements.length) => {
    if (elements.length >= MOST_ELEMENTS) {
      setError(tr("A canvas can have at most {0} elements.", [MOST_ELEMENTS]));
      return;
    }
    const next = [...elements.slice(0, at), made, ...elements.slice(at)];
    const shift = (index: number) => (index >= at ? index + 1 : index);
    setHidden((now) => remap(now, shift));
    setLocked((now) => remap(now, shift));
    setElements(next);
    setPicked(at);
  };
  const remove = (at: number) => {
    const shift = (index: number) => (index === at ? null : index > at ? index - 1 : index);
    setHidden((now) => remap(now, shift));
    setLocked((now) => remap(now, shift));
    setElements(elements.filter((_, index) => index !== at));
    setPicked(elements.length > 1 ? Math.min(at, elements.length - 2) : null);
    setSaid(tr("Removed"));
  };
  const duplicate = (at: number) => {
    const copy = structuredClone(elements[at]);
    copy.x = num(copy, "x", 0, -2048, 2048) + 4;
    copy.y = num(copy, "y", 0, 0, 2048) + 4;
    insert(copy, at + 1);
  };
  const move = (from: number, to: number) => {
    if (from === to || to < 0 || to >= elements.length) return;
    const next = elements.slice();
    const [one] = next.splice(from, 1);
    next.splice(to, 0, one);
    const map = moveMap(from, to);
    setHidden((now) => remap(now, map));
    setLocked((now) => remap(now, map));
    setElements(next);
    setPicked(to);
  };
  const toggle = (set: Set<number>, at: number) => {
    const next = new Set(set);
    if (next.has(at)) next.delete(at);
    else next.add(at);
    return next;
  };

  /** A new element near the middle of what the stage shows, on a row. */
  const spot = (): { x: number; y: number } => {
    const box = scroller.current;
    const area = surface.current;
    if (box && area) {
      const at = area.getBoundingClientRect();
      const view = box.getBoundingClientRect();
      const x = Math.round((view.left + view.width / 2 - at.left) / zoom);
      const y = Math.round((view.top + view.height / 2 - at.top) / zoom);
      return { x: clamp(x - 8, 0, Math.max(0, width - 16)), y: clamp(Math.round(y / LINE) * LINE, 0, Math.max(0, height - LINE)) };
    }
    return { x: 8, y: LINE };
  };
  const add = (tool: Tool, at = spot()) => {
    const first = glyphs[0]?.name ?? "";
    const made: StudioData =
      tool === "text"
        ? { type: "text", text: tr("Text"), x: at.x, y: at.y }
        : tool === "area"
          ? { type: "area", x: at.x, y: at.y, w: 36, h: 18, tooltip: "", actions: [] }
          : tool === "button"
            ? { type: "button", glyph: first, x: at.x, y: at.y, tooltip: "", actions: [] }
            : tool === "item"
              ? { type: "image", item: "", glyph: "", x: at.x, y: at.y }
              : { type: "image", glyph: first, x: at.x, y: at.y };
    insert(made);
    setSaid(tr("Added {0}", [typeLabel(String(made.type))]));
  };
  const addGlyph = (name: string, at = spot()) => {
    if (current && picked !== null && (str(current, "type") === "image" || str(current, "type") === "button") && !str(current, "glyph")) {
      patch(picked, { glyph: name, item: undefined });
      return;
    }
    insert({ type: "image", glyph: name, x: at.x, y: at.y });
  };

  /* ----- zoom ----- */

  const fit = useCallback(() => {
    const box = scroller.current;
    if (!box) return;
    const room = Math.min((box.clientWidth - 72) / Math.max(1, width), (box.clientHeight - 72) / Math.max(1, height));
    setZoom(clamp(Math.floor(room), 1, MOST_ZOOM));
  }, [width, height]);
  useLayoutEffect(() => {
    if (fitted) return;
    setFitted(true);
    fit();
  }, [fitted, fit]);
  const zoomBy = useCallback(
    (by: number, around?: { clientX: number; clientY: number }) => {
      const box = scroller.current;
      const area = surface.current;
      setZoom((was) => {
        const next = clamp(was + by, 1, MOST_ZOOM);
        if (next !== was && box && area && around) {
          // Keep the pixel under the pointer where it is.
          const at = area.getBoundingClientRect();
          const gx = (around.clientX - at.left) / was;
          const gy = (around.clientY - at.top) / was;
          requestAnimationFrame(() => {
            const moved = area.getBoundingClientRect();
            box.scrollLeft += moved.left + gx * next - around.clientX;
            box.scrollTop += moved.top + gy * next - around.clientY;
          });
        }
        return next;
      });
    },
    [],
  );
  useEffect(() => {
    const box = scroller.current;
    if (!box) return;
    const wheel = (event: WheelEvent) => {
      if (!event.ctrlKey && !event.metaKey) return;
      event.preventDefault();
      zoomBy(event.deltaY < 0 ? 1 : -1, event);
    };
    box.addEventListener("wheel", wheel, { passive: false });
    return () => box.removeEventListener("wheel", wheel);
  }, [zoomBy]);

  /* ----- pointer ----- */

  const toGui = (clientX: number, clientY: number) => {
    const at = surface.current?.getBoundingClientRect();
    if (!at) return { x: 0, y: 0 };
    return { x: Math.floor((clientX - at.left) / zoom), y: Math.floor((clientY - at.top) / zoom) };
  };

  /** Edges the dragged box can catch: the canvas's edges and middle, and every other element's. */
  const snapTargets = (skip: number) => {
    const xs = [0, width, Math.round(width / 2)];
    const ys = [0, height, Math.round(height / 2)];
    for (const one of laid) {
      if (one.index === skip || hidden.has(one.index) || !one.type) continue;
      const box = clickBox(one);
      xs.push(box.x, box.x + box.w, Math.round(box.x + box.w / 2));
      ys.push(box.y, box.y + box.h);
    }
    return { xs, ys };
  };
  const catchEdge = (edges: number[], targets: number[]): { by: number; at: number[] } => {
    let best: { by: number; at: number[] } = { by: 0, at: [] };
    let gap = SNAP + 1;
    for (const edge of edges) {
      for (const target of targets) {
        const d = target - edge;
        if (Math.abs(d) < gap) {
          gap = Math.abs(d);
          best = { by: d, at: [target] };
        } else if (Math.abs(d) === gap && d === best.by && !best.at.includes(target)) best.at.push(target);
      }
    }
    return gap <= SNAP ? best : { by: 0, at: [] };
  };

  const onStageDown = (event: PointerEvent<HTMLDivElement>) => {
    const box = scroller.current;
    if (event.button === 1 || (event.button === 0 && space.current)) {
      if (!box) return;
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      drag.current = { mode: "pan", startX: event.clientX, startY: event.clientY, left: box.scrollLeft, top: box.scrollTop };
      return;
    }
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    const handle = target.closest<HTMLElement>("[data-handle]");
    const piece = target.closest<HTMLElement>("[data-index]");
    event.currentTarget.focus({ preventScroll: true });
    if (handle && picked !== null && current) {
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      drag.current = { mode: "resize", index: picked, dir: handle.dataset.handle ?? "se", startX: event.clientX, startY: event.clientY, x: num(current, "x", 0, -2048, 2048), y: num(current, "y", 0, 0, 2048), w: num(current, "w", 1, 1, 1024), h: num(current, "h", 1, 1, 2048) };
      return;
    }
    if (!piece) {
      setPicked(null);
      return;
    }
    const index = Number(piece.dataset.index);
    const one = laidAt.get(index);
    if (!one || locked.has(index)) return;
    event.preventDefault();
    setPicked(index);
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { mode: "move", index, startX: event.clientX, startY: event.clientY, x: num(one.el, "x", 0, -2048, 2048), y: num(one.el, "y", 0, 0, 2048), box: clickBox(one) };
  };
  const onStageMove = (event: PointerEvent<HTMLDivElement>) => {
    setPointer(toGui(event.clientX, event.clientY));
    const held = drag.current;
    if (!held) return;
    if (held.mode === "pan") {
      const box = scroller.current;
      if (!box) return;
      box.scrollLeft = held.left - (event.clientX - held.startX);
      box.scrollTop = held.top - (event.clientY - held.startY);
      return;
    }
    const dx = Math.round((event.clientX - held.startX) / zoom);
    const dy = Math.round((event.clientY - held.startY) / zoom);
    const el = elements[held.index];
    if (!el) return;
    if (held.mode === "move") {
      let x = held.x + dx;
      let y = Math.max(0, held.y + dy);
      const shown: Guides = { x: [], y: [] };
      if (!event.altKey) {
        const { xs, ys } = snapTargets(held.index);
        const left = held.box.x + (x - held.x);
        const caughtX = catchEdge([left, left + held.box.w, Math.round(left + held.box.w / 2)], xs);
        x += caughtX.by;
        shown.x = caughtX.at;
        if (!event.shiftKey) {
          const top = held.box.y + (y - held.y);
          const caughtY = catchEdge([top, top + held.box.h], ys);
          y = Math.max(0, y + caughtY.by);
          shown.y = caughtY.at;
        }
      }
      if (event.shiftKey) y = Math.max(0, Math.round(y / LINE) * LINE);
      setGuides(shown.x.length || shown.y.length ? shown : null);
      if (x !== num(el, "x", 0, -2048, 2048) || y !== num(el, "y", 0, 0, 2048)) setElement(held.index, { ...el, x, y }, "drag");
      return;
    }
    let { x, y, w, h } = held;
    const dir = held.dir;
    if (dir.includes("e")) w = Math.max(1, held.w + dx);
    if (dir.includes("s")) h = Math.max(1, held.h + dy);
    if (dir.includes("w")) {
      x = Math.min(held.x + held.w - 1, held.x + dx);
      w = held.w + (held.x - x);
    }
    if (dir.includes("n")) {
      y = clamp(held.y + dy, 0, held.y + held.h - 1);
      h = held.h + (held.y - y);
    }
    if (event.shiftKey) {
      // Rows: the click box grows to whole rows anyway, so line the box up with them.
      const top = Math.round(y / LINE) * LINE;
      const bottom = Math.max(top + LINE, Math.round((y + h) / LINE) * LINE);
      if (dir.includes("n")) y = top;
      h = bottom - y;
    }
    if (x !== num(el, "x", 0, -2048, 2048) || y !== num(el, "y", 0, 0, 2048) || w !== num(el, "w", 0, 0, 1024) || h !== num(el, "h", 0, 0, 2048)) setElement(held.index, { ...el, x, y, w, h }, "resize");
  };
  const onStageUp = () => {
    const held = drag.current;
    drag.current = null;
    setGuides(null);
    if (held && held.mode !== "pan") {
      history.current.key = null;
      const el = elements[held.index];
      if (el) setSaid(tr("{0} at {1}, {2}", [elementLabel(el), num(el, "x", 0, -2048, 2048), num(el, "y", 0, 0, 2048)]));
    }
  };

  const onDragOver = (event: DragEvent<HTMLDivElement>) => {
    if (event.dataTransfer.types.includes(GLYPH_DRAG) || event.dataTransfer.types.includes("Files")) {
      event.preventDefault();
      event.dataTransfer.dropEffect = "copy";
    }
  };
  const onDrop = (event: DragEvent<HTMLDivElement>) => {
    const at = toGui(event.clientX, event.clientY);
    const place = { x: at.x, y: Math.max(0, at.y) };
    const name = event.dataTransfer.getData(GLYPH_DRAG);
    if (name) {
      event.preventDefault();
      insert({ type: "image", glyph: name, x: place.x, y: place.y });
      return;
    }
    const files = [...event.dataTransfer.files].filter((one) => one.name.toLowerCase().endsWith(".png"));
    if (files.length && serverId) {
      event.preventDefault();
      void upload(files, place);
    }
  };

  /* ----- pictures ----- */

  const taken = useMemo(() => new Set(glyphs.map((one) => one.id)), [glyphs]);
  const upload = async (files: File[], place?: { x: number; y: number }) => {
    if (!serverId) return;
    setBusy(tr("Adding pictures"));
    setError("");
    try {
      let at = place;
      for (const file of files) {
        const name = glyphId(file.name) + ".png";
        await addTexture(serverId, new File([file], name, { type: "image/png" }));
        const picture = new Image();
        picture.src = URL.createObjectURL(file);
        await picture.decode().catch(() => undefined);
        const made = await makeGlyph(serverId, glyphId(file.name), picture.naturalHeight || 16, taken);
        URL.revokeObjectURL(picture.src);
        if (at) {
          insert({ type: "image", glyph: made, x: at.x, y: at.y });
          at = { x: at.x + 4, y: at.y + 4 };
        }
      }
    } catch (failed) {
      setError(trBack(String(failed instanceof Error ? failed.message : failed)));
    } finally {
      setBusy("");
    }
  };
  const fromTexture = async (image: string, pictureHeight: number) => {
    if (!serverId) return;
    setBusy(tr("Making the glyph"));
    setError("");
    try {
      const made = await makeGlyph(serverId, image, pictureHeight, taken);
      addGlyph(made);
    } catch (failed) {
      setError(trBack(String(failed instanceof Error ? failed.message : failed)));
    } finally {
      setBusy("");
    }
  };

  /* ----- keys ----- */

  const onKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement;
    if (target.closest("input, textarea, select, [contenteditable='true'], .scv-inspector")) return;
    const ctrl = event.ctrlKey || event.metaKey;
    const key = event.key;
    const done = () => {
      event.preventDefault();
      event.stopPropagation();
    };
    if (key === " ") {
      space.current = true;
      if (target.closest(".scv-scroller")) done();
      return;
    }
    if (ctrl && (key === "z" || key === "Z")) {
      done();
      if (event.shiftKey) redo();
      else undo();
      return;
    }
    if (ctrl && (key === "y" || key === "Y")) {
      done();
      redo();
      return;
    }
    if (ctrl && (key === "=" || key === "+")) {
      done();
      zoomBy(1);
      return;
    }
    if (ctrl && key === "-") {
      done();
      zoomBy(-1);
      return;
    }
    if (!ctrl && !event.altKey && (key === "+" || key === "=")) {
      done();
      zoomBy(1);
      return;
    }
    if (!ctrl && !event.altKey && key === "-") {
      done();
      zoomBy(-1);
      return;
    }
    if (!ctrl && key === "0") {
      done();
      fit();
      return;
    }
    const held = copied();
    if (ctrl && (key === "v" || key === "V") && held) {
      done();
      const pasted = structuredClone(held);
      pasted.x = num(pasted, "x", 0, -2048, 2048) + 4;
      pasted.y = num(pasted, "y", 0, 0, 2048) + 4;
      copy(pasted);
      insert(pasted, picked !== null ? picked + 1 : elements.length);
      return;
    }
    if (key === "PageDown" || key === "PageUp") {
      if (!elements.length) return;
      done();
      const by = key === "PageDown" ? 1 : -1;
      const next = picked === null ? (by > 0 ? 0 : elements.length - 1) : (picked + by + elements.length) % elements.length;
      setPicked(next);
      setSaid(tr("{0} of {1}: {2}", [next + 1, elements.length, elementLabel(elements[next])]));
      return;
    }
    if (picked === null || !current) return;
    if (key === "Escape") {
      done();
      setPicked(null);
      return;
    }
    if (ctrl && (key === "d" || key === "D")) {
      done();
      duplicate(picked);
      return;
    }
    if (ctrl && (key === "c" || key === "C")) {
      done();
      copy(current);
      void navigator.clipboard?.writeText(JSON.stringify(current, null, 2)).catch(() => undefined);
      setSaid(tr("Copied"));
      return;
    }
    if (key === "Delete" || key === "Backspace") {
      done();
      remove(picked);
      return;
    }
    if (event.altKey && (key === "ArrowUp" || key === "ArrowDown")) {
      done();
      // The layers list shows the top of the drawing first, so up is later in the file.
      move(picked, picked + (key === "ArrowUp" ? 1 : -1));
      return;
    }
    const step = event.shiftKey ? LINE : 1;
    const moves: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] };
    const by = moves[key];
    if (!by || locked.has(picked)) return;
    done();
    let y = num(current, "y", 0, 0, 2048) + by[1];
    if (event.shiftKey && by[1] !== 0) y = Math.round(y / LINE) * LINE;
    const x = num(current, "x", 0, -2048, 2048) + by[0];
    setElement(picked, { ...current, x, y: Math.max(0, y) }, "nudge");
    setSaid(tr("x {0}, y {1}", [x, Math.max(0, y)]));
  };
  const onKeyUp = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === " ") space.current = false;
  };

  /* ----- drawing ----- */

  const renderText = (text: string, shadow: boolean) => <MiniText text={text} shadow={shadow} />;
  const pickedLaid = picked !== null ? laidAt.get(picked) : undefined;
  const pickedBox = pickedLaid ? (pickedLaid.type === "area" ? { x: pickedLaid.x, y: pickedLaid.y, w: pickedLaid.w, h: pickedLaid.h } : clickBox(pickedLaid)) : null;
  const stageW = Math.max(width, body.reach) * zoom;
  const stageH = Math.max(height, body.rows * LINE) * zoom;
  const counts = { error: 0, warn: 0, info: 0 };
  for (const one of problems) counts[one.level] += 1;
  const problemAt = new Map<number, Problem["level"]>();
  for (const one of problems) {
    if (one.index === null) continue;
    const was = problemAt.get(one.index);
    if (!was || (was === "info" && one.level !== "info") || (was === "warn" && one.level === "error")) problemAt.set(one.index, one.level);
  }
  const pickProblem = (one: Problem) => {
    if (one.index === null) setPicked(null);
    else {
      setPicked(one.index);
      setHidden((now) => {
        if (!now.has(one.index as number)) return now;
        const next = new Set(now);
        next.delete(one.index as number);
        return next;
      });
    }
  };

  return (
    <div className="scv" onKeyDown={onKey} onKeyUp={onKeyUp}>
      <div className="scv-bar" role="toolbar" aria-label={tr("Canvas tools")}>
        {onBack && (
          <button type="button" className="scv-tool" onClick={onBack} data-tip={tr("Back to the dialog")} aria-label={tr("Back to the dialog")}>
            <Icon name="arrow-left" />
          </button>
        )}
        <span className="scv-bar-title">
          <strong>{tr("Canvas")}</strong>
          <small>
            {width}×{height}
          </small>
        </span>
        <span className="scv-sep" aria-hidden="true" />
        <span className="scv-tools" role="group" aria-label={tr("Add")}>
          {(
            [
              ["image", "image", tr("Picture")],
              ["button", "play", tr("Button")],
              ["text", "tag", tr("Text")],
              ["area", "select-all", tr("Click area")],
              ["item", "package", tr("Item icon")],
            ] as [Tool, string, string][]
          ).map(([tool, icon, label]) => (
            <button key={tool} type="button" className="scv-tool labeled" onClick={() => add(tool)} data-tip={tr("Add: {0}", [label])}>
              <Icon name={icon} />
              <span>{label}</span>
            </button>
          ))}
        </span>
        <span className="scv-sep" aria-hidden="true" />
        <button type="button" className="scv-tool" onClick={undo} disabled={!steps.back} aria-label={tr("Undo")} data-tip={tr("Undo (Ctrl+Z)")}>
          <Icon name="undo" />
        </button>
        <button type="button" className="scv-tool flip" onClick={redo} disabled={!steps.ahead} aria-label={tr("Redo")} data-tip={tr("Redo (Ctrl+Y)")}>
          <Icon name="undo" />
        </button>
        <span className="scv-gap" />
        <span className="scv-views" role="group" aria-label={tr("Show")}>
          <button type="button" className="scv-tool" aria-pressed={rows} onClick={() => setRows(!rows)} data-tip={tr("9-pixel rows")} aria-label={tr("9-pixel rows")}>
            <Icon name="list" />
          </button>
          <button type="button" className="scv-tool" aria-pressed={hits} onClick={() => setHits(!hits)} data-tip={tr("Click boxes as the game sees them")} aria-label={tr("Click boxes")}>
            <Icon name="grid" />
          </button>
          <button type="button" className="scv-tool" aria-pressed={pressing} onClick={() => setPressing(!pressing)} data-tip={tr("Show the picked button pressed")} aria-label={tr("Pressed look")}>
            <Icon name="check" />
          </button>
        </span>
        <span className="scv-sep" aria-hidden="true" />
        <span className="scv-zoom" role="group" aria-label={tr("Zoom")}>
          <button type="button" className="scv-tool" onClick={() => zoomBy(-1)} disabled={zoom <= 1} aria-label={tr("Zoom out")} data-tip={tr("Zoom out (-)")}>
            <Icon name="minus" />
          </button>
          <output className="scv-zoom-value" aria-live="polite">{zoom * 100}%</output>
          <button type="button" className="scv-tool" onClick={() => zoomBy(1)} disabled={zoom >= MOST_ZOOM} aria-label={tr("Zoom in")} data-tip={tr("Zoom in (+, Ctrl+wheel)")}>
            <Icon name="plus" />
          </button>
          <button type="button" className="scv-tool" onClick={fit} aria-label={tr("Fit")} data-tip={tr("Fit (0)")}>
            <Icon name="maximize" />
          </button>
        </span>
        <details className="scv-keys">
          <summary className="scv-tool" aria-label={tr("Keys")} data-tip={tr("Keys")}>
            <Icon name="info" />
          </summary>
          <dl>
            <dt>{tr("Arrows")}</dt>
            <dd>{tr("Move 1 pixel; with Shift a whole row (9)")}</dd>
            <dt>{tr("Drag")}</dt>
            <dd>{tr("Move; Shift snaps y to a row, Alt turns the guides off")}</dd>
            <dt>Ctrl+Z · Ctrl+Y</dt>
            <dd>{tr("Undo · redo")}</dd>
            <dt>Ctrl+D · Ctrl+C · Ctrl+V</dt>
            <dd>{tr("Duplicate · copy · paste")}</dd>
            <dt>Delete</dt>
            <dd>{tr("Remove")}</dd>
            <dt>Alt+↑ · Alt+↓</dt>
            <dd>{tr("Up or down a layer")}</dd>
            <dt>PageUp · PageDown</dt>
            <dd>{tr("Pick the previous or next element")}</dd>
            <dt>{tr("Ctrl+wheel · + · - · 0")}</dt>
            <dd>{tr("Zoom · fit")}</dd>
            <dt>{tr("Space+drag · middle drag")}</dt>
            <dd>{tr("Pan")}</dd>
          </dl>
        </details>
      </div>

      <div className="scv-body">
        <aside className="scv-side" aria-label={tr("Layers and pictures")}>
          <div className="scv-tabs" role="tablist">
            <button type="button" role="tab" aria-selected={tab === "layers"} className={tab === "layers" ? "on" : ""} onClick={() => setTab("layers")}>
              <Icon name="layers" />
              {tr("Layers")}
              <small>{elements.length}</small>
            </button>
            <button type="button" role="tab" aria-selected={tab === "pictures"} className={tab === "pictures" ? "on" : ""} onClick={() => setTab("pictures")}>
              <Icon name="image" />
              {tr("Pictures")}
              <small>{glyphs.length}</small>
            </button>
          </div>
          {tab === "layers" ? (
            <Layers
              elements={elements}
              laidAt={laidAt}
              order={order}
              picked={picked}
              hidden={hidden}
              locked={locked}
              problemAt={problemAt}
              onPick={setPicked}
              onMove={move}
              onHide={(at) => setHidden((now) => toggle(now, at))}
              onLock={(at) => setLocked((now) => toggle(now, at))}
            />
          ) : (
            <Pictures glyphs={glyphs} ready={ready} serverId={serverId} busy={busy} current={current} onPick={(name) => addGlyph(name)} onTexture={fromTexture} onUpload={(files) => void upload(files, spot())} />
          )}
        </aside>

        <section className="scv-stage" aria-label={tr("Stage")}>
          <div ref={scroller} className="scv-scroller">
            <div className="scv-board" style={{ width: stageW + 30 + 48, height: stageH + 18 + 48 }}>
              <Ruler axis="x" length={Math.max(width, body.reach)} zoom={zoom} mark={pointer?.x ?? null} span={pickedBox ? [pickedBox.x, pickedBox.x + pickedBox.w] : null} limit={width} />
              <Ruler axis="y" length={Math.max(height, body.rows * LINE)} zoom={zoom} mark={pointer?.y ?? null} span={pickedBox ? [pickedBox.y, pickedBox.y + pickedBox.h] : null} limit={height} />
              <span className="scv-corner" aria-hidden="true" />
              <div
                ref={surface}
                className={"scv-surface" + (rows ? " rows" : "") + (zoom >= 4 ? " pixels" : "")}
                style={{ width: stageW, height: stageH, ["--z" as string]: zoom + "px", ["--row" as string]: LINE * zoom + "px" } as CSSProperties}
                tabIndex={0}
                role="application"
                aria-roledescription={tr("canvas")}
                aria-label={tr("Canvas {0} by {1}. Arrows move the picked element (Shift: a row), PageUp and PageDown pick another, Escape lets go.", [width, height])}
                onPointerDown={onStageDown}
                onPointerMove={onStageMove}
                onPointerUp={onStageUp}
                onPointerCancel={onStageUp}
                onPointerLeave={() => setPointer(null)}
                onDragOver={onDragOver}
                onDrop={onDrop}
              >
                <span className="scv-bounds" style={{ width: width * zoom, height: height * zoom }} aria-hidden="true" />
                {body.reach > width && <span className="scv-spill" style={{ left: width * zoom, width: (body.reach - width) * zoom, height: stageH }} aria-hidden="true" />}
                {laid.map((one, at) =>
                  hidden.has(one.index) ? null : (
                    <CanvasPiece
                      key={one.index}
                      one={one}
                      order={at}
                      scale={zoom}
                      renderText={renderText}
                      className={(picked === one.index ? "picked " : "") + (locked.has(one.index) ? "locked " : "") + (conditionsOf(one.el).length ? "conditional " : "") + (one.pressed ? "pressed" : "")}
                />
                  ),
                )}
                <div className="scv-overlay" aria-hidden="true">
                  {hits &&
                    laid.map((one) => {
                      if (hidden.has(one.index)) return null;
                      if (one.type === "area" && one.w > 0 && one.h > 0) {
                        const hit = hitBox(one);
                        return (
                          <span key={"h" + one.index} className="scv-hit area" style={{ left: hit.x * zoom, top: hit.y * zoom, width: hit.w * zoom, height: hit.h * zoom }}>
                            <span className="scv-hit-asked" style={{ top: (one.y - hit.y) * zoom, height: one.h * zoom }} />
                          </span>
                        );
                      }
                      if (one.type === "button" && one.glyph) return <span key={"h" + one.index} className="scv-hit button" style={{ left: one.x * zoom, top: one.y * zoom, width: one.w * zoom, height: one.h * zoom }} />;
                      return null;
                    })}
                  {laid.map((one) => {
                    const level = problemAt.get(one.index);
                    if (!level || hidden.has(one.index)) return null;
                    const box = clickBox(one);
                    return <span key={"p" + one.index} className={"scv-flag " + level} style={{ left: (box.x + Math.max(1, box.w)) * zoom, top: box.y * zoom }} />;
                  })}
                  {pickedBox && pickedLaid && (
                    <span className={"scv-pick" + (locked.has(pickedLaid.index) ? " locked" : "")} style={{ left: pickedBox.x * zoom, top: pickedBox.y * zoom, width: Math.max(1, pickedBox.w) * zoom, height: Math.max(1, pickedBox.h) * zoom }}>
                      <span className="scv-pick-tag">
                        {typeLabel(pickedLaid.type)} · {num(pickedLaid.el, "x", 0, -2048, 2048)}, {num(pickedLaid.el, "y", 0, 0, 2048)}
                      </span>
                      {pickedLaid.type === "area" && !locked.has(pickedLaid.index) && ["n", "s", "e", "w", "ne", "nw", "se", "sw"].map((dir) => <span key={dir} className={"scv-handle " + dir} data-handle={dir} />)}
                    </span>
                  )}
                  {guides?.x.map((x) => <span key={"gx" + x} className="scv-guide x" style={{ left: x * zoom }} />)}
                  {guides?.y.map((y) => <span key={"gy" + y} className="scv-guide y" style={{ top: y * zoom }} />)}
                </div>
                {elements.length === 0 && (
                  <div className="scv-empty">
                    <strong>{tr("An empty canvas")}</strong>
                    <span>{tr("Add a picture, a button or text from the bar above, or drag a picture in from Pictures.")}</span>
                  </div>
                )}
              </div>
            </div>
          </div>

          {problemsOpen && (
            <div className="scv-problems" role="region" aria-label={tr("Problems")}>
              {problems.length === 0 ? (
                <p className="scv-quiet">{tr("Nothing looks wrong. The game should show it as drawn.")}</p>
              ) : (
                <ul>
                  {problems.map((one, at) => (
                    <li key={at}>
                      <button type="button" className={"scv-problem " + one.level} onClick={() => pickProblem(one)}>
                        <Icon name={one.level === "info" ? "info" : "alert"} />
                        <span className="scv-problem-where">{one.index === null ? tr("Dialog") : "#" + (one.index + 1)}</span>
                        <span>{one.message}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}

          <div className="scv-status">
            <button type="button" className={"scv-status-problems " + worst} aria-expanded={problemsOpen} onClick={() => setProblemsOpen(!problemsOpen)}>
              <Icon name={worst === "" ? "check" : worst === "info" ? "info" : "alert"} />
              {problems.length === 0 ? tr("No problems") : problems.length === 1 ? tr("1 problem") : tr("{0} problems", [problems.length])}
              {counts.error > 0 && <small>{tr("{0} stop it loading", [counts.error])}</small>}
            </button>
            <span className="scv-gap" />
            {busy && <span className="scv-status-busy">{busy}…</span>}
            {pointer && (
              <span className="scv-readout">
                x {pointer.x} · y {pointer.y} · {tr("row {0} +{1}", [Math.floor(Math.max(0, pointer.y) / LINE), ((pointer.y % LINE) + LINE) % LINE])}
              </span>
            )}
            <span className="scv-readout">{tr("body {0} / {1}", [body.body, 1024])}</span>
          </div>
          <span className="scv-live" aria-live="polite">
            {said}
          </span>
        </section>

        <aside className="scv-inspector" aria-label={tr("Inspector")}>
          {error && (
            <p className="scv-error" role="alert">
              <Icon name="alert" />
              <span>{error}</span>
              <button type="button" className="icon-button" aria-label={tr("Dismiss")} onClick={() => setError("")}>
                <Icon name="close" />
              </button>
            </p>
          )}
          {current && picked !== null ? (
            <ElementInspector
              key={picked}
              el={current}
              index={picked}
              count={elements.length}
              laid={laidAt.get(picked)}
              glyphs={glyphs}
              names={names}
              serverId={serverId}
              after={afterOf(canvas)}
              problems={problems.filter((one) => one.index === picked)}
              taken={taken}
              onPatch={(changes, key) => patch(picked, changes, key)}
              onReplace={(next) => setElement(picked, next)}
              onDuplicate={() => duplicate(picked)}
              onRemove={() => remove(picked)}
              onMove={(to) => move(picked, to)}
              onError={setError}
            />
          ) : (
            <ScreenInspector dialog={dialog} canvas={canvas} names={names} problems={problems.filter((one) => one.index === null)} onDialog={(next, key) => commit(next, key)} onCanvas={(next, key) => setCanvas(next, key)} />
          )}
        </aside>
      </div>
    </div>
  );
}

/* ---------- rulers ---------- */

/** A ruler along the stage, in GUI pixels (x) or 9-pixel rows (y), with the pointer and the picked element marked. */
function Ruler({ axis, length, zoom, mark, span, limit }: { axis: "x" | "y"; length: number; zoom: number; mark: number | null; span: [number, number] | null; limit: number }) {
  const labels: number[] = [];
  if (axis === "x") {
    const every = zoom >= 4 ? 10 : zoom >= 2 ? 20 : 50;
    for (let at = 0; at <= length; at += every) labels.push(at);
  } else {
    const every = LINE * zoom >= 18 ? 1 : LINE * zoom >= 9 ? 2 : 5;
    for (let row = 0; row * LINE <= length; row += every) labels.push(row);
  }
  const along = axis === "x" ? "left" : "top";
  const size = axis === "x" ? "width" : "height";
  return (
    <div className={"scv-ruler " + axis} style={{ [size]: length * zoom, ["--tick" as string]: (axis === "x" ? 10 : LINE) * zoom + "px" } as CSSProperties} aria-hidden="true">
      <span className="scv-ruler-end" style={{ [along]: limit * zoom }} />
      {span && <span className="scv-ruler-span" style={{ [along]: span[0] * zoom, [size]: Math.max(1, span[1] - span[0]) * zoom }} />}
      {labels.map((at) => (
        <span key={at} className="scv-ruler-label" style={{ [along]: (axis === "x" ? at : at * LINE) * zoom }}>
          {axis === "x" ? at : at}
        </span>
      ))}
      {mark !== null && mark >= 0 && mark <= length && <span className="scv-ruler-mark" style={{ [along]: mark * zoom }} />}
    </div>
  );
}

/* ---------- layers ---------- */

function Layers({
  elements,
  laidAt,
  order,
  picked,
  hidden,
  locked,
  problemAt,
  onPick,
  onMove,
  onHide,
  onLock,
}: {
  elements: StudioData[];
  laidAt: Map<number, Laid>;
  order: Map<number, number>;
  picked: number | null;
  hidden: Set<number>;
  locked: Set<number>;
  problemAt: Map<number, Problem["level"]>;
  onPick: (at: number | null) => void;
  onMove: (from: number, to: number) => void;
  onHide: (at: number) => void;
  onLock: (at: number) => void;
}) {
  const [dragging, setDragging] = useState<number | null>(null);
  const [over, setOver] = useState<{ at: number; above: boolean } | null>(null);
  if (elements.length === 0) return <p className="scv-quiet scv-side-empty">{tr("Nothing yet. Elements show here, the one drawn on top first.")}</p>;
  // Top of the list = drawn last (on top); the file keeps them the other way round.
  const shown = elements.map((_, at) => at).reverse();
  const land = (target: number, above: boolean) => {
    if (dragging === null) return;
    // Above a row in the list = after it in the file.
    let to = above ? target + 1 : target;
    if (dragging < to) to -= 1;
    onMove(dragging, to);
  };
  return (
    <div className="scv-layers" role="list" aria-label={tr("Layers, the one drawn on top first")}>
      {shown.map((at) => {
        const el = elements[at];
        const one = laidAt.get(at);
        const level = problemAt.get(at);
        const glyph = one?.glyph;
        // Drawn under something that comes before it in the file: its row is higher up.
        const sunk = one && [...laidAt.values()].some((other) => other.index < at && (order.get(other.index) ?? 0) > (order.get(at) ?? 0) && other.type !== "area" && one.type !== "area");
        const mark = over && over.at === at && dragging !== null ? (over.above ? " drop-above" : " drop-below") : "";
        return (
          <div
            key={at}
            role="listitem"
            className={"scv-layer" + (picked === at ? " picked" : "") + (hidden.has(at) ? " hidden" : "") + (dragging === at ? " dragging" : "") + mark}
            draggable
            onDragStart={(event) => {
              event.dataTransfer.effectAllowed = "move";
              event.dataTransfer.setData("text/plain", String(at));
              setDragging(at);
            }}
            onDragOver={(event) => {
              if (dragging === null) return;
              event.preventDefault();
              const box = event.currentTarget.getBoundingClientRect();
              setOver({ at, above: event.clientY < box.top + box.height / 2 });
            }}
            onDragLeave={() => setOver((now) => (now && now.at === at ? null : now))}
            onDrop={(event) => {
              event.preventDefault();
              if (over) land(over.at, over.above);
              setDragging(null);
              setOver(null);
            }}
            onDragEnd={() => {
              setDragging(null);
              setOver(null);
            }}
          >
            <button type="button" className="scv-layer-pick" aria-pressed={picked === at} aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown" onClick={() => onPick(picked === at ? null : at)}>
              <span className="scv-grip" aria-hidden="true" />
              <span className="scv-layer-thumb" aria-hidden="true">
                {glyph?.url ? <img src={glyph.url} alt="" /> : <Icon name={typeIcon(el)} />}
              </span>
              <span className="scv-layer-name">
                <span>{elementLabel(el)}</span>
                <small>
                  {str(el, "type") || "?"} · {tr("row {0}", [one?.line ?? 0])}
                </small>
              </span>
              {conditionsOf(el).length > 0 && (
                <span className="scv-chip" data-tip={tr("Shown only when its conditions hold")}>
                  if
                </span>
              )}
              {sunk && (
                <span className="scv-chip warn" data-tip={tr("The game draws it under something earlier in the list (a higher row).")}>
                  <Icon name="arrow-down" />
                </span>
              )}
              {level && <span className={"scv-dot " + level} aria-label={level === "error" ? tr("Has an error") : level === "warn" ? tr("Has a warning") : tr("Has a note")} />}
              <span className="scv-layer-index">#{at + 1}</span>
            </button>
            <span className="scv-layer-tools">
              <button type="button" className="scv-mini" aria-pressed={!hidden.has(at)} aria-label={hidden.has(at) ? tr("Show in the editor") : tr("Hide in the editor")} data-tip={tr("Hide in the editor (not in the game)")} onClick={() => onHide(at)}>
                <Icon name={hidden.has(at) ? "eye-off" : "eye"} />
              </button>
              <button type="button" className="scv-mini" aria-pressed={locked.has(at)} aria-label={locked.has(at) ? tr("Unlock") : tr("Lock")} data-tip={tr("Lock: clicks on the stage go through it")} onClick={() => onLock(at)}>
                <Icon name={locked.has(at) ? "shield" : "shield-check"} />
              </button>
            </span>
          </div>
        );
      })}
      <p className="scv-quiet scv-layers-note">{tr("Within a row, higher in this list draws on top; a lower row always draws over the rows above it.")}</p>
    </div>
  );
}

/* ---------- pictures ---------- */

function TextureTile({ name, serverId, onMake }: { name: string; serverId?: string; onMake: (name: string, height: number) => void }) {
  const picture = usePicture(name, serverId);
  return (
    <button type="button" className="scv-tile raw" onClick={() => onMake(name, picture?.height ?? 16)} data-tip={tr("Make it a glyph and add it")}>
      <span className="scv-tile-art">{picture ? <img src={picture.url} alt="" /> : <Icon name="image" />}</span>
      <span className="scv-tile-name">{name}</span>
      <small>{picture ? picture.width + "×" + picture.height : ""}</small>
    </button>
  );
}

function Pictures({ glyphs, ready, serverId, busy, current, onPick, onTexture, onUpload }: { glyphs: AppGlyph[]; ready: boolean; serverId?: string; busy: string; current: StudioData | null; onPick: (name: string) => void; onTexture: (image: string, height: number) => void; onUpload: (files: File[]) => void }) {
  const [query, setQuery] = useState("");
  const [textures, setTextures] = useState<string[]>([]);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (!serverId) return;
    let live = true;
    commands.studioAssetList(serverId).then(
      (list) => {
        if (live) setTextures(list.assets.filter((one) => one.folder === "textures" && one.file.endsWith(".png")).map((one) => one.name));
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [serverId, glyphs]);
  const needle = query.trim().toLowerCase();
  const used = new Set(glyphs.map((one) => one.image));
  const shown = glyphs.filter((one) => !needle || one.name.includes(needle));
  const raw = textures.filter((one) => !used.has(one) && (!needle || one.includes(needle)));
  const swapping = current && (str(current, "type") === "image" || str(current, "type") === "button");
  return (
    <div className="scv-pictures">
      <div className="scv-pictures-head">
        <label className="scv-search">
          <Icon name="search" />
          <input value={query} placeholder={tr("Find a picture")} aria-label={tr("Find a picture")} onChange={(event) => setQuery(event.target.value)} spellCheck={false} />
        </label>
        {serverId && (
          <>
            <button type="button" className="scv-tool" onClick={() => input.current?.click()} disabled={!!busy} aria-label={tr("Add PNG pictures")} data-tip={tr("Add PNG pictures (or drop them on the stage)")}>
              <Icon name="upload" />
            </button>
            <input
              ref={input}
              type="file"
              accept=".png"
              multiple
              hidden
              onChange={(event) => {
                const files = [...(event.target.files ?? [])];
                event.target.value = "";
                if (files.length) onUpload(files);
              }}
            />
          </>
        )}
      </div>
      <p className="scv-quiet">{swapping && !str(current, "glyph") ? tr("Click a picture to use it for the picked element.") : tr("Click to add, or drag onto the stage.")}</p>
      {!ready && <p className="scv-quiet">{tr("Reading the glyphs…")}</p>}
      {ready && glyphs.length === 0 && raw.length === 0 && <p className="scv-quiet">{tr("No pictures yet. Add a PNG: it becomes a studio texture and a glyph.")}</p>}
      <div className="scv-tiles">
        {shown.map((one) => (
          <button
            key={one.id}
            type="button"
            className="scv-tile"
            draggable
            onDragStart={(event) => {
              event.dataTransfer.effectAllowed = "copy";
              event.dataTransfer.setData(GLYPH_DRAG, one.name);
            }}
            onClick={() => onPick(one.name)}
            data-tip={":" + one.name + ":  " + one.width + "×" + one.height}
          >
            <span className="scv-tile-art">
              <img src={one.url} alt="" />
            </span>
            <span className="scv-tile-name">{one.name}</span>
            <small>
              {one.width}×{one.height}
            </small>
          </button>
        ))}
      </div>
      {raw.length > 0 && (
        <>
          <h4 className="scv-side-head">{tr("Textures without a glyph")}</h4>
          <div className="scv-tiles">
            {raw.map((name) => (
              <TextureTile key={name} name={name} serverId={serverId} onMake={onTexture} />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

/* ---------- inspector ---------- */

function Section({ title, children, aside }: { title: string; children: ReactNode; aside?: ReactNode }) {
  return (
    <section className="scv-section">
      <header>
        <h4>{title}</h4>
        {aside}
      </header>
      {children}
    </section>
  );
}

function Num({ label, value, onChange, least = -2048, most = 2048, hint }: { label: string; value: number; onChange: (next: number) => void; least?: number; most?: number; hint?: string }) {
  return (
    <label className="scv-num">
      <span>{label}</span>
      <input
        type="number"
        min={least}
        max={most}
        value={value}
        onChange={(event) => {
          const next = Number(event.target.value);
          if (event.target.value !== "" && Number.isFinite(next)) onChange(clamp(Math.round(next), least, most));
        }}
      />
      {hint && <small>{hint}</small>}
    </label>
  );
}

function Text({ label, value, onChange, placeholder, hint, mono = false }: { label: string; value: string; onChange: (next: string) => void; placeholder?: string; hint?: ReactNode; mono?: boolean }) {
  return (
    <label className={"field studio-field scv-text" + (mono ? " mono" : "")}>
      <span>{label}</span>
      <input value={value} placeholder={placeholder} onChange={(event) => onChange(event.target.value)} spellCheck={false} />
      {hint && <small>{hint}</small>}
    </label>
  );
}

function ProblemList({ problems }: { problems: Problem[] }) {
  if (!problems.length) return null;
  return (
    <ul className="scv-own-problems">
      {problems.map((one, at) => (
        <li key={at} className={one.level}>
          <Icon name={one.level === "info" ? "info" : "alert"} />
          <span>{one.message}</span>
        </li>
      ))}
    </ul>
  );
}

/** A glyph picker: the picture, its size, and a searchable list. */
function GlyphPick({ label, value, glyphs, onChange, optional = false, hint }: { label: string; value: string; glyphs: AppGlyph[]; onChange: (next: string) => void; optional?: boolean; hint?: string }) {
  const glyph = glyphs.find((one) => one.name === value);
  const options = [...(optional ? [{ value: "", label: tr("None") }] : []), ...glyphs.map((one) => ({ value: one.name, label: ":" + one.name + ":  " + one.width + "×" + one.height }))];
  if (value && !glyph) options.push({ value, label: ":" + value + ":  " + tr("(missing)") });
  return (
    <div className="field studio-field scv-glyph">
      <span>{label}</span>
      <div className="scv-glyph-row">
        <span className="scv-glyph-art" aria-hidden="true">
          {glyph?.url ? <img src={glyph.url} alt="" /> : <Icon name="image" />}
        </span>
        <Dropdown value={value} options={options} search={tr("Find a glyph")} placeholder={tr("Pick a glyph")} onChange={onChange} />
      </div>
      {hint && <small>{hint}</small>}
    </div>
  );
}

/** Makes the glyph for an item icon once the game's icon is in (the item picker loads it). */
function ItemIconMaker({ item, glyph, glyphs, serverId, taken, onGlyph, onError }: { item: string; glyph: string; glyphs: AppGlyph[]; serverId?: string; taken: Set<string>; onGlyph: (name: string) => void; onError: (message: string) => void }) {
  const art = useArt();
  const id = item.trim().toLowerCase().replace(/^minecraft:/, "");
  const icon = art.icons[id] ?? "";
  const wanted = glyphId("item_" + id);
  const have = glyphs.find((one) => one.name === wanted);
  const making = useRef("");
  useEffect(() => {
    if (!id || id.startsWith("studio:")) return;
    if (have) {
      if (glyph !== have.name) onGlyph(have.name);
      return;
    }
    if (!icon || !serverId || making.current === id) return;
    making.current = id;
    makeItemGlyph(serverId, id, icon, taken).then(onGlyph, (failed: unknown) => onError(trBack(String(failed instanceof Error ? failed.message : failed))));
    // Runs when the icon arrives or the item changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, icon, have?.name]);
  if (!id) return <p className="scv-quiet">{tr("Pick an item: its game icon becomes a 16×16 glyph (item_<name>).")}</p>;
  if (id.startsWith("studio:")) return <p className="scv-quiet">{tr("For a studio item, use its texture from Pictures.")}</p>;
  if (have) return <p className="scv-quiet">{tr("Drawn with :{0}:.", [have.name])}</p>;
  if (!Object.keys(art.files).length) return <p className="scv-quiet">{tr("Turn on the game's pictures (the studio's preview) to make item icons.")}</p>;
  return <p className="scv-quiet">{icon ? tr("Making :{0}:…", [wanted]) : tr("Looking for the icon…")}</p>;
}

function ElementInspector({
  el,
  index,
  count,
  laid,
  glyphs,
  names,
  serverId,
  after,
  problems,
  taken,
  onPatch,
  onReplace,
  onDuplicate,
  onRemove,
  onMove,
  onError,
}: {
  el: StudioData;
  index: number;
  count: number;
  laid: Laid | undefined;
  glyphs: AppGlyph[];
  names: StudioNames;
  serverId?: string;
  after: After;
  problems: Problem[];
  taken: Set<string>;
  onPatch: (changes: StudioData, key?: string) => void;
  onReplace: (next: StudioData) => void;
  onDuplicate: () => void;
  onRemove: () => void;
  onMove: (to: number) => void;
  onError: (message: string) => void;
}) {
  const type = str(el, "type");
  const clickable = type === "button" || type === "area";
  const x = num(el, "x", 0, -2048, 2048);
  const y = num(el, "y", 0, 0, 2048);
  const mini = tr("MiniMessage, like <gold>, <b> and :glyph:. Placeholders like <player> and <pvar:name> fill in for each player.");
  const changeType = (next: string) => {
    const made: StudioData = { ...el, type: next };
    if (next === "area") {
      made.w = laid?.w || 36;
      made.h = laid?.h || 18;
      delete made.glyph;
      delete made.pressed;
      delete made.item;
    }
    if (next === "image" || next === "button") {
      made.glyph = str(el, "glyph") || glyphs[0]?.name || "";
      delete made.w;
      delete made.h;
    }
    if (next === "text") made.text = str(el, "text") || tr("Text");
    if (next !== "text") {
      delete made.text;
      delete made.align;
      delete made.shadow;
    }
    if (next === "button" || next === "area") made.actions = listOf(el.actions);
    else {
      delete made.actions;
      delete made.tooltip;
      delete made.permission;
    }
    if (next !== "button") delete made.pressed;
    onReplace(made);
  };
  return (
    <>
      <div className="scv-inspector-head">
        <span className="scv-inspector-icon" aria-hidden="true">
          <Icon name={typeIcon(el)} />
        </span>
        <div className="scv-inspector-title">
          <input className="scv-name" value={str(el, "name")} placeholder={elementLabel({ ...el, name: "" })} aria-label={tr("Name in the editor")} onChange={(event) => onPatch({ name: event.target.value || undefined }, "name")} spellCheck={false} />
          <small>
            #{index + 1} / {count} · {type || "?"}
          </small>
        </div>
        <span className="scv-inspector-tools">
          <button type="button" className="scv-mini" onClick={() => onMove(index + 1)} disabled={index >= count - 1} aria-label={tr("Forward")} data-tip={tr("Forward (later in the list)")}>
            <Icon name="arrow-up" />
          </button>
          <button type="button" className="scv-mini" onClick={() => onMove(index - 1)} disabled={index === 0} aria-label={tr("Backward")} data-tip={tr("Backward (earlier in the list)")}>
            <Icon name="arrow-down" />
          </button>
          <button type="button" className="scv-mini" onClick={onDuplicate} aria-label={tr("Duplicate")} data-tip={tr("Duplicate (Ctrl+D)")}>
            <Icon name="copy" />
          </button>
          <button type="button" className="scv-mini danger" onClick={onRemove} aria-label={tr("Remove")} data-tip={tr("Remove (Delete)")}>
            <Icon name="trash" />
          </button>
        </span>
      </div>
      <ProblemList problems={problems} />
      <div className="scv-inspector-body">
        <Section title={tr("Kind")}>
          <SegmentedControl
            className="even scv-kinds"
            value={type || "image"}
            onChange={changeType}
            options={[
              { value: "image", label: "image" },
              { value: "button", label: "button" },
              { value: "text", label: "text" },
              { value: "area", label: "area" },
            ]}
          />
        </Section>

        <Section title={tr("Place")} aside={laid && <small className="scv-aside">{tr("row {0} +{1}", [Math.floor(y / LINE), y % LINE])}</small>}>
          <div className="scv-grid">
            <Num label="X" value={x} onChange={(next) => onPatch({ x: next }, "x")} />
            <Num label="Y" value={y} least={0} onChange={(next) => onPatch({ y: next }, "y")} />
            {type === "area" ? (
              <>
                <Num label="W" value={num(el, "w", 0, 0, 1024)} least={1} most={1024} onChange={(next) => onPatch({ w: next }, "w")} />
                <Num label="H" value={num(el, "h", 0, 0, 2048)} least={1} most={2048} onChange={(next) => onPatch({ h: next }, "h")} />
              </>
            ) : (
              laid && (
                <p className="scv-size">
                  {laid.w}×{laid.h}
                  <small>{type === "text" ? tr("advance {0}", [laid.advance]) : laid.glyph ? tr("advance {0}", [laid.advance]) : ""}</small>
                </p>
              )
            )}
          </div>
          {laid?.type === "text" && laid.snapped && <p className="scv-quiet">{tr("Shown at y {0}: letters like Korean stay on whole rows.", [laid.y])}</p>}
          {type === "area" && laid && laid.h > 0 && (
            <p className="scv-quiet">{tr("Clicks land on rows {0} to {1} (y {2} to {3}).", [Math.floor(laid.y / LINE), Math.floor((laid.y + laid.h - 1) / LINE), hitBox(laid).y, hitBox(laid).y + hitBox(laid).h])}</p>
          )}
        </Section>

        {(type === "image" || type === "button") && (
          <Section title={tr("Picture")}>
            {"item" in el ? (
              <>
                <ItemField label={tr("Item")} value={str(el, "item")} onChange={(item) => onPatch({ item, glyph: "" })} hint={tr("Its icon from the game, as a glyph.")} />
                <ItemIconMaker item={str(el, "item")} glyph={str(el, "glyph")} glyphs={glyphs} serverId={serverId} taken={taken} onGlyph={(glyph) => onPatch({ glyph })} onError={onError} />
                <button type="button" className="button ghost small" onClick={() => onPatch({ item: undefined })}>
                  {tr("Use a glyph instead")}
                </button>
              </>
            ) : (
              <>
                <GlyphPick label={tr("Glyph")} value={str(el, "glyph")} glyphs={glyphs} onChange={(glyph) => onPatch({ glyph })} />
                <button type="button" className="button ghost small" onClick={() => onPatch({ item: "", glyph: "" })}>
                  <Icon name="package" />
                  {tr("Use an item's icon")}
                </button>
              </>
            )}
            {type === "button" && (
              <GlyphPick
                label={tr("Pressed picture")}
                value={str(el, "pressed")}
                glyphs={glyphs}
                optional
                onChange={(pressed) => onPatch({ pressed: pressed || undefined })}
                hint={after === "close" ? tr("Shows only while the dialog stays open: set After a click to stay or refresh.") : tr("Shows for a moment after a click: Paper 3 ticks, Pumpkin half a second. A copy moved 1 pixel down feels like a press.")}
              />
            )}
          </Section>
        )}

        {type === "text" && (
          <Section title={tr("Text")}>
            <Text label={tr("Text")} value={str(el, "text")} onChange={(text) => onPatch({ text }, "text")} hint={mini} />
            <div className="field studio-field">
              <span>{tr("X is the text's")}</span>
              <SegmentedControl className="even" value={(ALIGNS as readonly string[]).includes(str(el, "align")) ? str(el, "align") : "left"} onChange={(align) => onPatch({ align: align === "left" ? undefined : align })} options={ALIGNS.map((one) => ({ value: one, label: one }))} />
            </div>
            <label className="scv-switch">
              <span>{tr("Text shadow")}</span>
              <Switch checked={el.shadow !== false} onChange={(event) => onPatch({ shadow: event.target.checked ? undefined : false })} />
            </label>
          </Section>
        )}

        {clickable && (
          <Section title={tr("Click")}>
            <Text label={tr("Tooltip")} value={str(el, "tooltip")} onChange={(tooltip) => onPatch({ tooltip }, "tooltip")} hint={tr("Shows while the pointer is over it. The only hover the game allows.")} />
            <Text label={tr("Permission")} value={str(el, "permission")} placeholder={tr("Everyone")} mono onChange={(permission) => onPatch({ permission: permission || undefined }, "permission")} hint={type === "area" ? tr("Without it the area isn't there at all.") : tr("Without it the button shows but does nothing.")} />
            <StepList label={tr("When clicked")} empty={tr("Nothing yet. Add an action.")} value={el.actions} names={names} onChange={(actions) => onPatch({ actions })} />
          </Section>
        )}

        <Section title={tr("Show only if")}>
          <StepList conditions label={tr("Conditions")} empty={tr("Always shown.")} value={el.if} names={names} onChange={(list) => onPatch({ if: list.length ? list : undefined })} />
          <p className="scv-quiet">{tr("Checked for each player whenever the dialog is drawn. Hidden buttons and areas can't be clicked.")}</p>
        </Section>
      </div>
    </>
  );
}

/** Nothing picked: the dialog and canvas settings, starting variables and on-open actions. */
function ScreenInspector({ dialog, canvas, names, problems, onDialog, onCanvas }: { dialog: StudioData; canvas: StudioData; names: StudioNames; problems: Problem[]; onDialog: (next: StudioData, key?: string) => void; onCanvas: (next: StudioData, key?: string) => void }) {
  const { width, height } = canvasSize(canvas);
  const after = afterOf(canvas);
  const vars = objectOf(dialog.variables);
  const entries = Object.entries(vars);
  const setVars = (next: [string, unknown][]) => {
    const out = { ...dialog };
    if (next.length) out.variables = Object.fromEntries(next);
    else delete out.variables;
    onDialog(out, "vars");
  };
  const sizes: [string, number, number][] = [
    [tr("Chest"), 176, 166],
    [tr("DialogUI"), 256, 150],
    [tr("Safe most"), 320, 220],
  ];
  return (
    <>
      <div className="scv-inspector-head">
        <span className="scv-inspector-icon" aria-hidden="true">
          <Icon name="settings" />
        </span>
        <div className="scv-inspector-title">
          <strong>{tr("Dialog")}</strong>
          <small>{tr("Pick an element to edit it")}</small>
        </div>
      </div>
      <ProblemList problems={problems} />
      <div className="scv-inspector-body">
        <Section title={tr("Screen")}>
          <Text label={tr("Title")} value={typeof dialog.title === "string" ? dialog.title : ""} onChange={(title) => onDialog({ ...dialog, title }, "title")} hint={tr("MiniMessage. Over the canvas.")} />
          <div className="scv-grid">
            <Num label={tr("Width")} value={width} least={1} most={MOST_WIDTH} onChange={(next) => onCanvas({ ...canvas, width: next }, "width")} />
            <Num label={tr("Height")} value={height} least={1} most={2048} onChange={(next) => onCanvas({ ...canvas, height: next }, "height")} />
          </div>
          <div className="scv-presets" role="group" aria-label={tr("Sizes")}>
            {sizes.map(([label, w, h]) => (
              <button key={label} type="button" className={"scv-preset" + (w === width && h === height ? " on" : "")} onClick={() => onCanvas({ ...canvas, width: w, height: h })}>
                {label}
                <small>
                  {w}×{h}
                </small>
              </button>
            ))}
          </div>
        </Section>

        <Section title={tr("After a click")}>
          <SegmentedControl
            className="even"
            value={after}
            onChange={(next) => {
              const out: StudioData = { ...canvas, after: next };
              delete out.close;
              onCanvas(out);
            }}
            options={AFTERS.map((one) => ({ value: one, label: one }))}
          />
          <p className="scv-quiet">{afterHint(after)}</p>
        </Section>

        <Section title={tr("Opening")}>
          <label className="scv-switch">
            <span>{tr("Esc closes it")}</span>
            <Switch checked={dialog.canClose !== false} onChange={(event) => onDialog({ ...dialog, canClose: event.target.checked })} />
          </label>
          <Text label={tr("Permission")} value={typeof dialog.permission === "string" ? dialog.permission : ""} placeholder={tr("Everyone")} mono onChange={(permission) => onDialog({ ...dialog, permission }, "permission")} hint={tr("Only players with it can open the dialog.")} />
        </Section>

        <Section
          title={tr("Starting variables")}
          aside={
            <button type="button" className="scv-mini" aria-label={tr("Add variable")} data-tip={tr("Add variable")} onClick={() => setVars([...entries, ["var" + (entries.length + 1), "0"]])}>
              <Icon name="plus" />
            </button>
          }
        >
          {entries.length === 0 ? (
            <p className="scv-quiet">{tr("Player variables set when the dialog opens and the player has none yet, like a selected tab. Read them with <pvar:name>, change them with the Set variable action, test them in conditions.")}</p>
          ) : (
            <div className="scv-vars">
              {entries.map(([name, value], at) => (
                <div key={at} className="scv-var">
                  <input
                    value={name}
                    aria-label={tr("Name")}
                    spellCheck={false}
                    onChange={(event) => setVars(entries.map((one, index) => (index === at ? [event.target.value, one[1]] : one)))}
                  />
                  <input
                    value={typeof value === "string" || typeof value === "number" || typeof value === "boolean" ? String(value) : ""}
                    aria-label={tr("Starts as")}
                    spellCheck={false}
                    onChange={(event) => setVars(entries.map((one, index) => (index === at ? [one[0], event.target.value] : one)))}
                  />
                  <button type="button" className="scv-mini" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={() => setVars(entries.filter((_, index) => index !== at))}>
                    <Icon name="trash" />
                  </button>
                </div>
              ))}
            </div>
          )}
        </Section>

        <Section title={tr("When it opens")}>
          <StepList label={tr("Actions")} empty={tr("Nothing. Runs before the canvas draws, so it can set what the canvas shows.")} value={dialog.onOpen} names={names} onChange={(list) => {
            const out = { ...dialog };
            if (list.length) out.onOpen = list;
            else delete out.onOpen;
            onDialog(out);
          }} />
        </Section>
        <p className="scv-quiet">{tr("Players need the studio pack to see glyphs. A click on the canvas doesn't carry the dialog's inputs.")}</p>
      </div>
    </>
  );
}

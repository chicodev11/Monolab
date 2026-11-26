import { createContext, useContext, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { commands } from "../../lib/bindings";
import type { Run } from "../../lib/miniMessage";
import { FONT_SHEETS } from "./gameFont";

/**
 * The game's own pictures for the studio previews: its font, item icons and GUI textures, from the Minecraft client
 * the app downloads from Mojang once the user says yes. Without them the previews draw their plain look.
 */
export type GameArt = {
  /** Texture path → data URL. */
  files: Record<string, string>;
  /** Item id (`diamond`) → icon data URL. */
  icons: Record<string, string>;
  /** The font sheet and each character's width, once loaded. */
  font: Font | null;
};

/** One character's place in a font picture, its width and how far above the base line it starts, in game pixels. */
type Glyph = { sheet: HTMLImageElement; x: number; y: number; wide: number; high: number; ascent: number; width: number };
type Font = Map<string, Glyph>;

const NONE: GameArt = { files: {}, icons: {}, font: null };
const ArtContext = createContext<GameArt>(NONE);

export function useArt(): GameArt {
  return useContext(ArtContext);
}

/** What the previews draw with, under `assets/minecraft/textures/`. */
export const BOSSBAR_SPRITES = ["pink", "blue", "red", "green", "yellow", "purple", "white"].flatMap((color) => [
  `gui/sprites/boss_bar/${color}_background.png`,
  `gui/sprites/boss_bar/${color}_progress.png`,
]);
export const NOTCH_SPRITES = [6, 10, 12, 20].flatMap((count) => [`gui/sprites/boss_bar/notched_${count}_background.png`, `gui/sprites/boss_bar/notched_${count}_progress.png`]);
export const ART_FILES = [
  ...FONT_SHEETS.map((one) => one.file),
  "gui/sprites/container/slot.png",
  "gui/sprites/tooltip/background.png",
  "gui/sprites/tooltip/frame.png",
  "gui/container/generic_54.png",
  "gui/container/crafting_table.png",
  "gui/container/furnace.png",
  "gui/sprites/toast/advancement.png",
  "gui/sprites/widget/button.png",
  "gui/sprites/widget/checkbox.png",
  "gui/sprites/widget/checkbox_selected.png",
  "gui/sprites/widget/text_field.png",
  "gui/sprites/icon/ping_5.png",
  "gui/sprites/container/villager/trade_arrow.png",
  "misc/unknown_server.png",
  "entity/player/wide/steve.png",
  ...BOSSBAR_SPRITES,
  ...NOTCH_SPRITES,
];

/** Loaded once per client version for the whole app; icons are added as previews ask for them. */
const loaded = new Map<string, { version: string; files: Record<string, string>; font: Font | null }>();
const icons = new Map<string, Record<string, string>>();

function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((done, fail) => {
    const image = new Image();
    image.onload = () => done(image);
    image.onerror = () => fail(new Error("The picture didn't load."));
    image.src = url;
  });
}

/** The game's font pictures read with the client's own character tables (not by code: in the ascii picture cell 183
 * is a box line, not the middle dot). A glyph is as wide as its rightmost lit column; a space is 3. */
async function readFont(files: Record<string, string>): Promise<Font | null> {
  if (!files["font/ascii.png"]) return null;
  const font: Font = new Map();
  for (const { file, cell, ascent, chars } of FONT_SHEETS) {
    const sheet = files[file] ? await loadImage(files[file]).catch(() => null) : null;
    const ctx = sheet && document.createElement("canvas").getContext("2d", { willReadFrequently: true });
    if (!sheet || !ctx) continue;
    ctx.canvas.width = sheet.naturalWidth;
    ctx.canvas.height = sheet.naturalHeight;
    ctx.drawImage(sheet, 0, 0);
    const pixels = ctx.getImageData(0, 0, sheet.naturalWidth, sheet.naturalHeight).data;
    const [wide, high] = cell;
    chars.forEach((row, line) =>
      [...row].forEach((char, at) => {
        if (char === "\0") return;
        const x = at * wide;
        const y = line * high;
        let right = -1;
        for (let column = wide - 1; column >= 0 && right < 0; column -= 1)
          for (let down = 0; down < high && right < 0; down += 1) if (pixels[((y + down) * sheet.naturalWidth + x + column) * 4 + 3] > 0) right = column;
        font.set(char, { sheet, x, y, wide, high, ascent, width: char === " " ? 3 : right + 1 });
      }),
    );
  }
  return font.size ? font : null;
}

/** Wraps a preview: loads the textures for `version` (null = the user hasn't said yes) and the `items` it shows. */
export function GameArtProvider({ version, items, children }: { version: string | null; items: string[]; children: ReactNode }) {
  const [art, setArt] = useState<GameArt>(NONE);
  const wanted = [...new Set(items.map((one) => one.trim().toLowerCase().replace(/^minecraft:/, "")).filter((one) => one && !one.startsWith("studio:")))].sort();
  const key = wanted.join(",");
  useEffect(() => {
    if (!version) {
      setArt(NONE);
      return;
    }
    let alive = true;
    void (async () => {
      let base = loaded.get(version);
      if (!base) {
        const got = await commands.gameFiles(version, ART_FILES).catch(() => null);
        if (!got) return;
        base = { version: got.version, files: got.files, font: await readFont(got.files) };
        loaded.set(version, base);
      }
      const have = icons.get(base.version) ?? {};
      const missing = wanted.filter((one) => !(one in have));
      if (missing.length) {
        const found = await commands.itemIcons(base.version, missing).catch(() => ({}) as Record<string, string>);
        for (const one of missing) have[one] = found[one] ?? "";
        icons.set(base.version, have);
      }
      if (alive) setArt({ files: base.files, icons: { ...have }, font: base.font });
    })();
    return () => {
      alive = false;
    };
    // `key` stands for `wanted`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version, key]);
  return <ArtContext.Provider value={art}>{children}</ArtContext.Provider>;
}

/** Text pixels per game pixel in the previews. */
export const SCALE = 2;

function shadeOf(color: string): string {
  const hex = /^#([0-9a-f]{6})$/i.exec(color)?.[1];
  if (!hex) return "#3f3f3f";
  const value = parseInt(hex, 16);
  const quarter = (shift: number) => Math.floor(((value >> shift) & 255) / 4);
  return `rgb(${quarter(16)}, ${quarter(8)}, ${quarter(0)})`;
}

/** How far one character moves the pen, in game pixels (bold adds one). */
function advance(font: Font, text: string, bold: boolean, fallback: CanvasRenderingContext2D): number {
  const glyph = font.get(text);
  if (glyph) return glyph.width + 1 + (bold ? 1 : 0);
  return Math.ceil(fallback.measureText(text).width / SCALE) + 1;
}

/** Room above the line for accents (the accented letters start 3 pixels higher). */
const RISE = 3;

/** One line of game text drawn with the game's font on a canvas (shadow, bold, underline, strikethrough). */
function GameLine({ runs, base, shadow, font }: { runs: Run[]; base: string; shadow: boolean; font: Font }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const plain = runs.map((one) => one.text).join("");
  useLayoutEffect(() => {
    const target = canvas.current;
    const ctx = target?.getContext("2d");
    if (!target || !ctx) return;
    const ink = document.createElement("canvas").getContext("2d");
    if (!ink) return;
    ink.font = `${8 * SCALE}px monospace`;
    const chars = runs.flatMap((run) => [...run.text].map((text) => ({ text, run })));
    const width = chars.reduce((sum, one) => sum + advance(font, one.text, !!one.run.bold, ink), 0);
    const ratio = window.devicePixelRatio || 1;
    target.width = Math.max(1, (width + 1) * SCALE * ratio);
    target.height = (9 + RISE) * SCALE * ratio;
    target.style.width = `${Math.max(1, (width + 1) * SCALE)}px`;
    target.style.height = `${(9 + RISE) * SCALE}px`;
    target.style.marginTop = `${-RISE * SCALE}px`;
    ctx.setTransform(ratio, 0, 0, ratio, 0, RISE * SCALE * ratio);
    ctx.imageSmoothingEnabled = false;
    // Each glyph is drawn white into a scratch canvas, then tinted: first the shadow, then the glyph.
    const glyph = document.createElement("canvas");
    glyph.width = 12 * SCALE;
    glyph.height = 12 * SCALE;
    const paint = glyph.getContext("2d");
    if (!paint) return;
    paint.imageSmoothingEnabled = false;
    paint.font = `${8 * SCALE}px monospace`;
    paint.textBaseline = "top";
    let x = 0;
    for (const { text, run } of chars) {
      const step = advance(font, text, !!run.bold, ink);
      const known = font.get(text);
      const rise = known ? (7 - known.ascent) * SCALE : 0;
      const color = run.color ?? base;
      if (text !== " ") {
        for (const [tint, offset] of shadow ? ([[shadeOf(color), SCALE], [color, 0]] as const) : ([[color, 0]] as const)) {
          paint.globalCompositeOperation = "source-over";
          paint.clearRect(0, 0, glyph.width, glyph.height);
          if (known) {
            paint.drawImage(known.sheet, known.x, known.y, known.wide, known.high, 0, 0, known.wide * SCALE, known.high * SCALE);
          } else {
            paint.fillStyle = "#fff";
            paint.fillText(text, 0, 0);
          }
          paint.globalCompositeOperation = "source-in";
          paint.fillStyle = tint;
          paint.fillRect(0, 0, glyph.width, glyph.height);
          const left = x * SCALE + offset;
          const skew = run.italic ? 1 : 0;
          if (skew) {
            // Italic leans each row a little, like the game.
            for (let row = 0; row < (known?.high ?? 8); row += 1) {
              const lean = ((7 - row) / 7) * SCALE;
              ctx.drawImage(glyph, 0, row * SCALE, glyph.width, SCALE, left + lean, row * SCALE + offset + rise, glyph.width, SCALE);
            }
          } else {
            ctx.drawImage(glyph, left, offset + rise);
            if (run.bold) ctx.drawImage(glyph, left + SCALE, offset + rise);
          }
        }
      }
      ctx.fillStyle = color;
      if (run.underlined) ctx.fillRect(x * SCALE - SCALE, 8 * SCALE, step * SCALE + SCALE, SCALE);
      if (run.strikethrough) ctx.fillRect(x * SCALE - SCALE, 3.5 * SCALE, step * SCALE + SCALE, SCALE);
      x += step;
    }
  }, [runs, base, shadow, font]);
  return (
    <span className="mc-line mc-art-line">
      <canvas ref={canvas} aria-hidden="true" />
      <span className="sr-only">{plain || " "}</span>
    </span>
  );
}

/** MiniMessage lines drawn with the game font. */
export function GameText({ lines, base, shadow, font }: { lines: Run[][]; base: string; shadow: boolean; font: Font }) {
  return (
    <>
      {lines.map((runs, at) => (
        <GameLine key={at} runs={runs} base={base} shadow={shadow} font={font} />
      ))}
    </>
  );
}

/** A piece of a texture, scaled up without blurring. `at` is where the piece starts in the texture. */
export function Piece({ url, at = [0, 0], size, scale = SCALE, sheet = 256, className, children }: { url: string; at?: [number, number]; size: [number, number]; scale?: number; sheet?: number; className?: string; children?: ReactNode }) {
  return (
    <div
      className={`mc-piece ${className ?? ""}`}
      style={{
        width: size[0] * scale,
        height: size[1] * scale,
        backgroundImage: `url(${url})`,
        backgroundPosition: `${-at[0] * scale}px ${-at[1] * scale}px`,
        backgroundSize: `${sheet * scale}px auto`,
      }}
    >
      {children}
    </div>
  );
}

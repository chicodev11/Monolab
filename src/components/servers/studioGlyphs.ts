import { useEffect, useSyncExternalStore } from "react";
import { commands } from "../../lib/ipc";
import type { StudioData } from "./StudioPreview";
import type { Run } from "../../lib/miniMessage";

/**
 * Studio maker M2: the server's glyph images (`font` files) for the editors and previews: each `:name:`, its
 * picture and the size it draws at. Same rules as the pack builder (`src-tauri/src/studio_pack_fonts.rs`).
 */
export type AppGlyph = {
  /** The `font` file. */
  id: string;
  /** What `:name:` uses. */
  name: string;
  /** The texture asset (without `.png`). */
  image: string;
  /** The picture as a data URL, once loaded. */
  url: string;
  height: number;
  ascent: number;
  /** Drawn size in GUI pixels, and how far it moves the pen (its colored part + 1). */
  width: number;
  advance: number;
  /** The picture's own pixels. */
  pictureWidth: number;
  pictureHeight: number;
};

type Loaded = { glyphs: AppGlyph[]; ready: boolean };
const EMPTY: Loaded = { glyphs: [], ready: false };
const cache = new Map<string, Loaded>();
const loading = new Set<string>();
const listeners = new Set<() => void>();
/** The server the studio last showed, for previews that aren't told which one. */
let lastServer = "";

function tell() {
  for (const one of listeners) one();
}

export function nameOf(id: string, data: StudioData): string {
  const name = typeof data.name === "string" ? data.name.trim() : "";
  return name || id.replace(/-/g, "_");
}

export function validGlyphName(name: string): boolean {
  return /^[a-z0-9_]{1,32}$/.test(name);
}

/** The picture's size and its colored width (the rightmost column with any color, plus one). */
export async function measurePicture(url: string): Promise<{ width: number; height: number; drawn: number }> {
  const picture = new Image();
  picture.src = url;
  await picture.decode();
  const canvas = document.createElement("canvas");
  canvas.width = picture.naturalWidth;
  canvas.height = picture.naturalHeight;
  const ctx = canvas.getContext("2d");
  if (!ctx || !canvas.width || !canvas.height) return { width: canvas.width, height: canvas.height, drawn: canvas.width };
  ctx.drawImage(picture, 0, 0);
  const pixels = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
  let drawn = 0;
  for (let x = canvas.width - 1; x >= 0 && !drawn; x -= 1) {
    for (let y = 0; y < canvas.height; y += 1) {
      if (pixels[(y * canvas.width + x) * 4 + 3] !== 0) {
        drawn = x + 1;
        break;
      }
    }
  }
  return { width: canvas.width, height: canvas.height, drawn };
}

/** A font file's numbers the way the pack builder reads them. */
export function glyphSize(data: StudioData, picture: { width: number; height: number; drawn: number }) {
  const round = (value: unknown, fallback: number) => (typeof value === "number" && Number.isFinite(value) ? Math.round(value) : fallback);
  const height = Math.min(256, Math.max(1, round(data.height, picture.height || 8)));
  const ascent = Math.min(height, Math.max(-256, round(data.ascent, Math.min(7, height))));
  const scale = picture.height ? height / picture.height : 1;
  return { height, ascent, width: Math.round(picture.width * scale), advance: Math.floor(0.5 + picture.drawn * scale) + 1 };
}

async function load(serverId: string) {
  if (loading.has(serverId)) return;
  loading.add(serverId);
  try {
    const overview = await commands.studioOverview(serverId);
    const fonts = overview.resources.filter((one) => one.kind === "font" && one.enabled);
    const glyphs: AppGlyph[] = [];
    for (const font of fonts) {
      try {
        const file = await commands.studioRead(serverId, "font", font.id);
        const data = JSON.parse(file.text) as StudioData;
        const image = (typeof data.image === "string" ? data.image : "").trim().replace(/\.png$/, "");
        if (!image) continue;
        const url = await commands.studioAssetImage(serverId, `${image}.png`);
        const picture = await measurePicture(url);
        const size = glyphSize(data, picture);
        glyphs.push({ id: font.id, name: nameOf(font.id, data), image, url, ...size, pictureWidth: picture.width, pictureHeight: picture.height });
      } catch {
        // A broken font file or a missing picture is skipped here; the pack's table says why.
      }
    }
    cache.set(serverId, { glyphs, ready: true });
  } catch {
    cache.set(serverId, { glyphs: [], ready: true });
  } finally {
    loading.delete(serverId);
    tell();
  }
}

/** Reads the glyphs again (after a font file or picture changed). */
export function reloadGlyphs(serverId?: string) {
  const id = serverId || lastServer;
  if (!id) return;
  cache.delete(id);
  for (const key of [...pictures.keys()]) if (key.startsWith(`${id}/`)) pictures.delete(key);
  void load(id);
}

/** The server a hook was given, remembered for hooks called without one; returns the server to use. */
function rememberServer(serverId?: string): string {
  if (serverId) lastServer = serverId;
  return serverId || lastServer;
}

/** The server's glyph images, loaded once and shared. Without `serverId`, the server the studio last showed. */
export function useStudioGlyphs(serverId?: string): Loaded {
  const id = rememberServer(serverId);
  const loaded = useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => (id ? (cache.get(id) ?? EMPTY) : EMPTY),
  );
  useEffect(() => {
    if (id && !cache.has(id)) void load(id);
  }, [id]);
  return loaded;
}

type Picture = { url: string; width: number; height: number; drawn: number };
const pictures = new Map<string, Picture | null>();

/** One texture asset (`ruby` → `ruby.png`) as a data URL with its size, or null while loading or when missing. */
export function usePicture(image: string, serverId?: string): Picture | null {
  const id = rememberServer(serverId);
  const key = `${id}/${image}`;
  const picture = useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => pictures.get(key) ?? null,
  );
  useEffect(() => {
    if (!id || !image || pictures.has(key)) return;
    pictures.set(key, null);
    void commands
      .studioAssetImage(id, `${image}.png`)
      .then(async (url) => {
        pictures.set(key, { url, ...(await measurePicture(url)) });
      })
      .catch(() => pictures.delete(key))
      .finally(tell);
  }, [id, image, key]);
  return picture;
}

/** Glyphs by `:name:`. */
export function byName(glyphs: AppGlyph[]): Record<string, AppGlyph> {
  const out: Record<string, AppGlyph> = {};
  for (const one of glyphs) if (!out[one.name]) out[one.name] = one;
  return out;
}

export type TextPiece = { runs: Run[] } | { glyph: AppGlyph };

/** A line cut where a known `:name:` glyph is, so the glyph's picture can show there (the add-on swaps the same
 * names for the glyph's letter; unknown names stay as written). */
export function glyphPieces(line: Run[], known: Record<string, AppGlyph>): TextPiece[] {
  const out: TextPiece[] = [];
  let runs: Run[] = [];
  for (const run of line) {
    let rest = run.text;
    for (let found = /:([a-z0-9_]{1,32}):/.exec(rest); found; found = /:([a-z0-9_]{1,32}):/.exec(rest)) {
      const glyph = known[found[1]];
      if (!glyph) {
        // Not a glyph: keep the text up to the second colon, which may start a real one.
        const keep = found.index + found[0].length - 1;
        runs.push({ ...run, text: rest.slice(0, keep) });
        rest = rest.slice(keep);
        continue;
      }
      if (found.index > 0) runs.push({ ...run, text: rest.slice(0, found.index) });
      if (runs.length) out.push({ runs });
      runs = [];
      out.push({ glyph });
      rest = rest.slice(found.index + found[0].length);
    }
    if (rest) runs.push({ ...run, text: rest });
  }
  if (runs.length) out.push({ runs });
  return out;
}

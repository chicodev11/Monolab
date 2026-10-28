import { tr } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import { useEffect, useMemo, useRef, useState } from "react";
import type { MapPin } from "./pins";
import { useSmoothPins } from "./pins";
import { Dropdown } from "../ui/Dropdown";
import { METRICS, colorOf, formatValue, gradient, makeScale, metricHint, metricLabel, type Metric, type StatFetch, type StatGrid } from "./chunkStats";

const TILE = 128;
const MIN_ZOOM = -4;
const MAX_ZOOM = 5;
const MAX_VIEW = 7;
const WORKERS = 6;

export type MapArea = { minX: number; minZ: number; maxX: number; maxZ: number };

function snapArea(a: { x: number; z: number }, b: { x: number; z: number }): MapArea {
  return {
    minX: Math.floor(Math.min(a.x, b.x) / 16) * 16,
    minZ: Math.floor(Math.min(a.z, b.z) / 16) * 16,
    maxX: Math.floor(Math.max(a.x, b.x) / 16) * 16 + 15,
    maxZ: Math.floor(Math.max(a.z, b.z) / 16) * 16 + 15,
  };
}

type AreaStay = { chunks: number; visited: number; total: number; max: number; tooBig: boolean };

export type TileFetch = (z: number, tx: number, ty: number, night: boolean) => Promise<string>;
export type MapChanges = () => Promise<[number, number][]>;
export type MapLayerProgress = { done: number; total: number };
export type MapHeight = {
  y: number | null;
  range: { min: number; max: number; start: number };
  ready: boolean;
  onChange: (y: number | null) => void;
  ask: (x: number, z: number) => Promise<MapLayerProgress | null>;
};

const CHANGES_MS = 3000;
const ASK_MS = 4000;

/** Whether a tile shows any of a region, counting the one-block edge its shading reads. */
export function tileTouchesRegion(level: number, tx: number, ty: number, rx: number, rz: number) {
  const span = TILE * 2 ** level;
  const left = tx * span;
  const top = ty * span;
  const rleft = rx * 512 - 1;
  const rtop = rz * 512 - 1;
  return left < rleft + 514 && rleft < left + span && top < rtop + 514 && rtop < top + span;
}

type Want = { key: string; level: number; tx: number; ty: number };

const KEEP_MS = 3 * 60 * 1000;
const KEEP_TILES = 500;
const held = new Map<string, { at: number; tiles: Record<string, string> }>();

function tileCache(key: string) {
  const now = Date.now();
  for (const [name, one] of held) if (now - one.at > KEEP_MS) held.delete(name);
  let one = held.get(key);
  if (!one) {
    one = { at: now, tiles: {} };
    held.set(key, one);
    while (held.size > 6) held.delete(held.keys().next().value!);
  }
  return one;
}

function keepTile(key: string, tile: string, url: string) {
  const one = tileCache(key);
  one.tiles[tile] = url;
  const names = Object.keys(one.tiles);
  if (names.length > KEEP_TILES) for (const name of names.slice(0, names.length - KEEP_TILES)) delete one.tiles[name];
}

export function WorldMap({
  pins,
  onPick,
  fetchTile,
  changes,
  start,
  height,
  onExport,
  chunks,
  stats,
  focus,
  cacheKey,
  onPregenArea,
  onDeleteArea,
}: {
  pins: MapPin[];
  onPick?: (x: number, z: number) => void;
  fetchTile: TileFetch;
  changes?: MapChanges | null;
  start: { x: number; z: number; level: number };
  height?: MapHeight | null;
  onExport?: (png: string) => void;
  chunks?: { on: boolean; toggle: () => void; loaded: number[]; forced: number[]; known: boolean } | null;
  stats?: StatFetch | null;
  focus?: { x: number; z: number; n: number } | null;
  cacheKey?: string;
  onPregenArea?: ((area: MapArea) => void) | null;
  onDeleteArea?: ((area: MapArea) => void) | null;
}) {
  const drawn = useSmoothPins(pins, true);
  const box = useRef<HTMLDivElement>(null);
  const [view, setViewState] = useState({ zoom: start.level, x: start.x, z: start.z });
  const viewRef = useRef(view);
  const setView = (next: { zoom: number; x: number; z: number }) => {
    viewRef.current = next;
    setViewState(next);
  };
  const zoom = view.zoom;
  const corner = view;
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [where, setWhere] = useState<{ x: number; z: number } | null>(null);
  const [tiles, setTiles] = useState<Record<string, string>>(() => (cacheKey ? { ...tileCache(cacheKey).tiles } : {}));
  const [night, setNight] = useState(false);
  const [showPins, setShowPins] = useState(true);
  const [showGrid, setShowGrid] = useState(false);
  const gridCanvas = useRef<HTMLCanvasElement>(null);
  const [area, setArea] = useState<MapArea | null>(null);
  const picking = useRef<{ x: number; z: number } | null>(null);
  const [selecting, setSelecting] = useState(false);
  const [stay, setStay] = useState<AreaStay | "loading" | null>(null);
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setArea(null);
        setStay(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const drag = useRef<{ x: number; y: number; from: { x: number; z: number }; moved: boolean } | null>(null);
  const glide = useRef<{ to: number; atX: number; atY: number } | null>(null);
  const frame = useRef(0);

  const bpp = 2 ** zoom;
  const level = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, Math.floor(zoom + 1e-6)));
  const prefix = (night ? "n" : "d") + (height?.y != null ? `y${height.y}` : "");
  const heightRef = useRef(height);
  heightRef.current = height;
  const [progress, setProgress] = useState<MapLayerProgress | null>(null);
  const [heightDraft, setHeightDraft] = useState<string | null>(null);
  const wantY = height?.y ?? null;
  const heightReady = height?.ready ?? false;
  useEffect(() => {
    setProgress(null);
    if (wantY === null || !heightReady) return;
    let stopped = false;
    const ask = () => {
      const node = box.current;
      const one = heightRef.current;
      if (!node || !one || document.visibilityState !== "visible") return;
      const at = viewRef.current;
      const scale = 2 ** at.zoom;
      void one
        .ask(at.x + (node.clientWidth * scale) / 2, at.z + (node.clientHeight * scale) / 2)
        .then((found) => {
          if (!stopped) setProgress(found);
        })
        .catch(() => {});
    };
    ask();
    const timer = window.setInterval(ask, ASK_MS);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [wantY, heightReady]);

  useEffect(() => {
    const node = box.current;
    if (!node) return;
    const watch = new ResizeObserver(() => {
      const rect = node.getBoundingClientRect();
      setSize({ w: Math.round(rect.width), h: Math.round(rect.height) });
    });
    watch.observe(node);
    return () => watch.disconnect();
  }, []);

  const needed = useMemo<Want[]>(() => {
    if (size.w < 16 || size.h < 16) return [];
    const span = TILE * 2 ** level;
    const first = Math.floor(corner.x / span);
    const last = Math.floor((corner.x + size.w * bpp) / span);
    const top = Math.floor(corner.z / span);
    const bottom = Math.floor((corner.z + size.h * bpp) / span);
    const midX = corner.x + (size.w * bpp) / 2;
    const midZ = corner.z + (size.h * bpp) / 2;
    const list: Want[] = [];
    for (let ty = top; ty <= bottom; ty += 1) {
      for (let tx = first; tx <= last; tx += 1) {
        list.push({ key: `${prefix}/${level}/${tx}/${ty}`, level, tx, ty });
      }
    }
    list.sort(
      (a, b) =>
        Math.hypot((a.tx + 0.5) * span - midX, (a.ty + 0.5) * span - midZ) -
        Math.hypot((b.tx + 0.5) * span - midX, (b.ty + 0.5) * span - midZ),
    );
    const coarse = Math.min(MAX_ZOOM, level + 2);
    if (coarse > level) {
      const wide = TILE * 2 ** coarse;
      const under: Want[] = [];
      for (let ty = Math.floor(corner.z / wide); ty <= Math.floor((corner.z + size.h * bpp) / wide); ty += 1) {
        for (let tx = Math.floor(corner.x / wide); tx <= Math.floor((corner.x + size.w * bpp) / wide); tx += 1) {
          under.push({ key: `${prefix}/${coarse}/${tx}/${ty}`, level: coarse, tx, ty });
        }
      }
      return [...under, ...list];
    }
    return list;
  }, [corner.x, corner.z, level, size.w, size.h, bpp, prefix]);

  const wantRef = useRef<Want[]>([]);
  const have = useRef<Record<string, string>>(cacheKey ? { ...tileCache(cacheKey).tiles } : {});
  const busy = useRef(new Set<string>());
  const later = useRef(new Map<string, number>());
  const running = useRef(0);
  const nightRef = useRef(night);
  nightRef.current = night;
  const fetchRef = useRef(fetchTile);
  fetchRef.current = fetchTile;
  const pump = useRef<() => void>(() => {});
  pump.current = () => {
    while (running.current < WORKERS) {
      const now = Date.now();
      const next = wantRef.current.find(
        (one) => !have.current[one.key] && !busy.current.has(one.key) && (later.current.get(one.key) ?? 0) <= now,
      );
      if (!next) return;
      busy.current.add(next.key);
      running.current += 1;
      const asNight = next.key.startsWith("n");
      void fetchRef
        .current(next.level, next.tx, next.ty, asNight)
        .then(
          (url) => {
            const partial = url.startsWith("partial:");
            const clean = partial ? url.slice("partial:".length) : url;
            if (partial) later.current.set(next.key, Date.now() + 2500);
            else later.current.delete(next.key);
            if (!partial) {
              have.current[next.key] = clean;
              if (cacheKey) keepTile(cacheKey, next.key, clean);
            }
            setTiles((held) => ({ ...held, [next.key]: clean }));
          },
          () => later.current.set(next.key, Date.now() + 3000),
        )
        .finally(() => {
          busy.current.delete(next.key);
          running.current -= 1;
          pump.current();
        });
    }
  };
  useEffect(() => {
    wantRef.current = needed;
    pump.current();
  }, [needed]);
  useEffect(() => {
    const timer = window.setInterval(() => pump.current(), 1000);
    return () => window.clearInterval(timer);
  }, []);
  const changesRef = useRef(changes);
  changesRef.current = changes;
  useEffect(() => {
    let asking = false;
    const timer = window.setInterval(() => {
      const ask = changesRef.current;
      if (!ask || asking || document.visibilityState !== "visible") return;
      asking = true;
      void ask()
        .then((regions) => {
          if (!regions.length) return;
          const stale = (key: string) => {
            const [, level, tx, ty] = key.split("/").map(Number);
            return regions.some(([rx, rz]) => tileTouchesRegion(level, tx, ty, rx, rz));
          };
          for (const key of Object.keys(have.current)) if (stale(key)) delete have.current[key];
          if (cacheKey) {
            const kept = tileCache(cacheKey).tiles;
            for (const key of Object.keys(kept)) if (stale(key)) delete kept[key];
          }
          pump.current();
        })
        .catch(() => {})
        .finally(() => {
          asking = false;
        });
    }, CHANGES_MS);
    return () => window.clearInterval(timer);
  }, [cacheKey]);

  useEffect(() => {
    if (!focus || size.w < 16) return;
    const zoomTo = Math.min(viewRef.current.zoom, 0);
    const step = 2 ** zoomTo;
    setView({ zoom: zoomTo, x: focus.x - (size.w * step) / 2, z: focus.z - (size.h * step) / 2 });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus?.n]);
  const [metric, setMetric] = useState<Metric | null>(null);
  const grids = useRef(new Map<string, StatGrid | null>());
  const [gridTick, setGridTick] = useState(0);
  const statsRef = useRef(stats);
  statsRef.current = stats;
  const regions = useMemo(() => {
    if (!metric || size.w < 16) return [];
    const first = Math.floor(corner.x / 512);
    const last = Math.floor((corner.x + size.w * bpp) / 512);
    const top = Math.floor(corner.z / 512);
    const bottom = Math.floor((corner.z + size.h * bpp) / 512);
    const midX = (corner.x + (size.w * bpp) / 2) / 512;
    const midZ = (corner.z + (size.h * bpp) / 2) / 512;
    const list: { rx: number; rz: number }[] = [];
    for (let rz = top; rz <= bottom; rz += 1) for (let rx = first; rx <= last; rx += 1) list.push({ rx, rz });
    list.sort((a, b) => Math.hypot(a.rx + 0.5 - midX, a.rz + 0.5 - midZ) - Math.hypot(b.rx + 0.5 - midX, b.rz + 0.5 - midZ));
    return list.slice(0, 256);
  }, [metric, corner.x, corner.z, size.w, size.h, bpp]);
  useEffect(() => {
    if (!metric || !statsRef.current) return;
    let stop = false;
    const queue = regions.filter((one) => !grids.current.has(`${metric}/${one.rx}/${one.rz}`));
    const work = async () => {
      while (!stop && queue.length) {
        const next = queue.shift()!;
        const key = `${metric}/${next.rx}/${next.rz}`;
        if (grids.current.has(key)) continue;
        grids.current.set(key, null);
        try {
          grids.current.set(key, (await statsRef.current?.(metric, next.rx, next.rz)) ?? null);
        } catch {
          grids.current.delete(key);
        }
        if (!stop) setGridTick((was) => was + 1);
      }
    };
    void Promise.all([work(), work(), work(), work(), work(), work()]);
    return () => {
      stop = true;
    };
  }, [metric, regions]);
  const shown = useMemo(() => {
    if (!metric) return [];
    return regions
      .map((one) => ({ ...one, grid: grids.current.get(`${metric}/${one.rx}/${one.rz}`) ?? null }))
      .filter((one): one is { rx: number; rz: number; grid: StatGrid } => one.grid !== null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [metric, regions, gridTick]);
  const scale = useMemo(() => (metric ? makeScale(metric, shown.map((one) => one.grid)) : null), [metric, shown]);
  const hover = useMemo(() => {
    if (!metric || !where) return null;
    const cx = Math.floor(where.x / 16);
    const cz = Math.floor(where.z / 16);
    const grid = grids.current.get(`${metric}/${Math.floor(cx / 32)}/${Math.floor(cz / 32)}`);
    const value = grid?.[(((cz % 32) + 32) % 32) * 32 + (((cx % 32) + 32) % 32)];
    return value === undefined || value === null ? null : formatValue(metric, value);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [metric, where, gridTick]);
  useEffect(() => {
    const canvas = gridCanvas.current;
    if (!canvas) return;
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.round(size.w * ratio);
    canvas.height = Math.round(size.h * ratio);
    const pen = canvas.getContext("2d");
    if (!pen) return;
    pen.setTransform(ratio, 0, 0, ratio, 0, 0);
    pen.clearRect(0, 0, size.w, size.h);
    if (!showGrid || size.w < 16) return;
    const lines = (step: number, color: string, width: number) => {
      pen.strokeStyle = color;
      pen.lineWidth = width;
      pen.beginPath();
      const firstX = Math.ceil(corner.x / step) * step;
      for (let x = firstX; x <= corner.x + size.w * bpp; x += step) {
        const px = Math.round((x - corner.x) / bpp) + 0.5;
        pen.moveTo(px, 0);
        pen.lineTo(px, size.h);
      }
      const firstZ = Math.ceil(corner.z / step) * step;
      for (let z = firstZ; z <= corner.z + size.h * bpp; z += step) {
        const py = Math.round((z - corner.z) / bpp) + 0.5;
        pen.moveTo(0, py);
        pen.lineTo(size.w, py);
      }
      pen.stroke();
    };
    if (16 / bpp >= 6) lines(16, "rgba(255, 255, 255, 0.22)", 1);
    if (512 / bpp >= 12) lines(512, "rgba(255, 255, 255, 0.55)", 1.5);
  }, [showGrid, corner.x, corner.z, bpp, size.w, size.h]);
  const statCanvas = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = statCanvas.current;
    if (!canvas) return;
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.round(size.w * ratio);
    canvas.height = Math.round(size.h * ratio);
    const pen = canvas.getContext("2d");
    if (!pen) return;
    pen.setTransform(ratio, 0, 0, ratio, 0, 0);
    pen.clearRect(0, 0, size.w, size.h);
    if (!scale) return;
    const cell = 16 / bpp;
    for (const { rx, rz, grid } of shown) {
      for (let index = 0; index < 1024; index += 1) {
        const value = grid[index];
        if (value === null || value === undefined) continue;
        const left = ((rx * 32 + (index % 32)) * 16 - corner.x) / bpp;
        const top = ((rz * 32 + Math.floor(index / 32)) * 16 - corner.z) / bpp;
        if (left + cell < 0 || top + cell < 0 || left > size.w || top > size.h) continue;
        pen.fillStyle = colorOf(scale, value);
        pen.fillRect(left, top, cell + 0.5, cell + 0.5);
      }
    }
  }, [shown, scale, corner.x, corner.z, bpp, size.w, size.h]);

  const overlay = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = overlay.current;
    if (!canvas) return;
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.round(size.w * ratio);
    canvas.height = Math.round(size.h * ratio);
    const pen = canvas.getContext("2d");
    if (!pen) return;
    pen.setTransform(ratio, 0, 0, ratio, 0, 0);
    pen.clearRect(0, 0, size.w, size.h);
    if (!chunks?.on) return;
    const cell = 16 / bpp;
    const small = cell < 5;
    const dot = small ? 5 : cell;
    const shift = (dot - cell) / 2;
    const paint = (list: number[], fill: string, line: string) => {
      const spots: [number, number][] = [];
      for (let at = 0; at + 1 < list.length; at += 2) {
        const left = (list[at] * 16 - corner.x) / bpp - shift;
        const top = (list[at + 1] * 16 - corner.z) / bpp - shift;
        if (left + dot < 0 || top + dot < 0 || left > size.w || top > size.h) continue;
        spots.push([left, top]);
      }
      if (small) {
        pen.fillStyle = "rgba(0, 0, 0, 0.6)";
        for (const [left, top] of spots) pen.fillRect(left - 1, top - 1, dot + 2, dot + 2);
      }
      pen.fillStyle = fill;
      pen.strokeStyle = line;
      pen.lineWidth = 1;
      for (const [left, top] of spots) {
        pen.fillRect(left, top, dot, dot);
        if (!small && cell >= 6) pen.strokeRect(left + 0.5, top + 0.5, cell - 1, cell - 1);
      }
    };
    paint(chunks.loaded, small ? "rgba(80, 200, 255, 0.95)" : "rgba(80, 200, 255, 0.28)", "rgba(80, 200, 255, 0.55)");
    paint(chunks.forced, small ? "rgba(255, 170, 40, 1)" : "rgba(255, 170, 40, 0.45)", "rgba(255, 170, 40, 0.85)");
  }, [chunks, corner.x, corner.z, bpp, size.w, size.h]);

  const [exporting, setExporting] = useState(false);
  const exportArea = async (pick: MapArea) => {
    if (!onExport || exporting) return;
    setExporting(true);
    try {
      const blocksW = pick.maxX - pick.minX + 1;
      const blocksH = pick.maxZ - pick.minZ + 1;
      const level = Math.max(-2, Math.min(MAX_ZOOM, Math.ceil(Math.log2(Math.max(blocksW, blocksH) / 4096))));
      const scale = 2 ** level;
      const span = TILE * scale;
      const left = pick.minX;
      const top = pick.minZ;
      const width = Math.max(1, Math.round(blocksW / scale));
      const height = Math.max(1, Math.round(blocksH / scale));
      const canvas = document.createElement("canvas");
      canvas.width = width;
      canvas.height = height;
      const pen = canvas.getContext("2d");
      if (!pen) return;
      pen.imageSmoothingEnabled = false;
      const jobs: Promise<void>[] = [];
      for (let ty = Math.floor(top / span); ty <= Math.floor((top + height * scale) / span); ty += 1) {
        for (let tx = Math.floor(left / span); tx <= Math.floor((left + width * scale) / span); tx += 1) {
          jobs.push(
            fetchRef.current(level, tx, ty, night).then(
              (url) =>
                new Promise<void>((done) => {
                  const picture = new Image();
                  picture.onload = () => {
                    pen.drawImage(picture, (tx * span - left) / scale, (ty * span - top) / scale, TILE, TILE);
                    done();
                  };
                  picture.onerror = () => done();
                  picture.src = url.startsWith("partial:") ? url.slice("partial:".length) : url;
                }),
              () => undefined,
            ),
          );
        }
      }
      await Promise.all(jobs);
      const url = canvas.toDataURL("image/png");
      onExport(url.slice(url.indexOf(",") + 1));
    } finally {
      setExporting(false);
    }
  };

  const toWorld = (clientX: number, clientY: number) => {
    const rect = box.current?.getBoundingClientRect();
    if (!rect) return null;
    return {
      x: Math.floor(corner.x + (clientX - rect.left) * bpp),
      z: Math.floor(corner.z + (clientY - rect.top) * bpp),
    };
  };

  const toScreen = (x: number, z: number) => ({
    left: (x - corner.x) / bpp,
    top: (z - corner.z) / bpp,
  });

  const animate = () => {
    cancelAnimationFrame(frame.current);
    const step = () => {
      const g = glide.current;
      if (!g) return;
      const v = viewRef.current;
      const diff = g.to - v.zoom;
      const next = Math.abs(diff) < 0.004 ? g.to : v.zoom + diff * 0.22;
      const before = 2 ** v.zoom;
      const after = 2 ** next;
      setView({ zoom: next, x: v.x + g.atX * (before - after), z: v.z + g.atY * (before - after) });
      if (next === g.to) {
        glide.current = null;
        return;
      }
      frame.current = requestAnimationFrame(step);
    };
    frame.current = requestAnimationFrame(step);
  };
  const glideTo = (target: number, atX: number, atY: number) => {
    glide.current = { to: Math.max(MIN_ZOOM, Math.min(MAX_VIEW, target)), atX, atY };
    animate();
  };

  useEffect(() => {
    const node = box.current;
    if (!node) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const rect = node.getBoundingClientRect();
      const base = glide.current?.to ?? viewRef.current.zoom;
      const unit = event.deltaMode === 1 ? 33 : event.deltaMode === 2 ? 400 : 1;
      const delta = Math.max(-1.5, Math.min(1.5, (event.deltaY * unit) / 250));
      glideTo(base + delta, event.clientX - rect.left, event.clientY - rect.top);
    };
    node.addEventListener("wheel", onWheel, { passive: false });
    return () => node.removeEventListener("wheel", onWheel);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => () => cancelAnimationFrame(frame.current), []);

  const layers = useMemo(() => {
    const out: { key: string; url: string; left: number; top: number; px: number; depth: number }[] = [];
    const viewW = size.w * bpp;
    const viewH = size.h * bpp;
    for (const [key, url] of Object.entries(tiles)) {
      const [p, l, x, y] = key.split("/");
      if (p !== prefix) continue;
      const tl = Number(l);
      if (Math.abs(tl - level) > 3) continue;
      const span = TILE * 2 ** tl;
      const wx = Number(x) * span;
      const wz = Number(y) * span;
      if (wx + span < corner.x || wz + span < corner.z || wx > corner.x + viewW || wz > corner.z + viewH) continue;
      out.push({
        key,
        url,
        left: (wx - corner.x) / bpp,
        top: (wz - corner.z) / bpp,
        px: span / bpp,
        depth: tl === level ? 1000 : 500 - Math.abs(tl - level) * 10 - (tl > level ? 0 : 5),
      });
    }
    out.sort((a, b) => a.depth - b.depth);
    return out;
  }, [tiles, prefix, level, corner.x, corner.z, size.w, size.h, bpp]);

  return (
    <div className="map-wrap">
      <div
        className={`map-view ${drag.current ? "dragging" : ""}`}
        ref={box}
        onContextMenu={(event) => event.preventDefault()}
        onPointerDown={(event) => {
          if (event.button === 2) {
            const spot = toWorld(event.clientX, event.clientY);
            if (!spot) return;
            event.currentTarget.setPointerCapture(event.pointerId);
            picking.current = spot;
            setSelecting(true);
            setArea(snapArea(spot, spot));
            setStay(null);
            setCopied(false);
            return;
          }
          if (event.button !== 0) return;
          event.currentTarget.setPointerCapture(event.pointerId);
          drag.current = { x: event.clientX, y: event.clientY, from: { x: event.clientX, z: event.clientY }, moved: false };
        }}
        onPointerMove={(event) => {
          const spotNow = toWorld(event.clientX, event.clientY);
          setWhere(spotNow);
          if (picking.current) {
            if (spotNow) setArea(snapArea(picking.current, spotNow));
            return;
          }
          if (!drag.current) return;
          const step = 2 ** viewRef.current.zoom;
          const last = drag.current.from;
          if (Math.abs(event.clientX - drag.current.x) + Math.abs(event.clientY - drag.current.y) > 4) drag.current.moved = true;
          const v = viewRef.current;
          setView({ zoom: v.zoom, x: v.x - (event.clientX - last.x) * step, z: v.z - (event.clientY - last.z) * step });
          drag.current.from = { x: event.clientX, z: event.clientY };
        }}
        onPointerUp={(event) => {
          if (picking.current) {
            event.currentTarget.releasePointerCapture(event.pointerId);
            picking.current = null;
            setSelecting(false);
            return;
          }
          const held = drag.current;
          if (!held) return;
          event.currentTarget.releasePointerCapture(event.pointerId);
          drag.current = null;
          if (!held.moved && onPick) {
            const spot = toWorld(event.clientX, event.clientY);
            if (spot) onPick(spot.x, spot.z);
          }
        }}
        onPointerLeave={() => setWhere(null)}
      >
        {layers.map((one) => (
          <img
            key={one.key}
            className="map-tile"
            src={one.url}
            alt=""
            draggable={false}
            style={{
              left: `${one.left}px`,
              top: `${one.top}px`,
              width: `${one.px + 0.5}px`,
              height: `${one.px + 0.5}px`,
            }}
          />
        ))}
        <canvas ref={statCanvas} className="map-chunks" style={{ width: `${size.w}px`, height: `${size.h}px` }} />
        <canvas ref={gridCanvas} className="map-chunks" style={{ width: `${size.w}px`, height: `${size.h}px` }} />
        <canvas ref={overlay} className="map-chunks" style={{ width: `${size.w}px`, height: `${size.h}px` }} />
        {area && (() => {
          const a = toScreen(area.minX, area.minZ);
          const b = toScreen(area.maxX + 1, area.maxZ + 1);
          return (
            <div
              className="map-area"
              style={{ left: `${a.left}px`, top: `${a.top}px`, width: `${Math.max(2, b.left - a.left)}px`, height: `${Math.max(2, b.top - a.top)}px` }}
            />
          );
        })()}
        <div className="map-pins" hidden={!showPins}>
          {drawn.map((pin) => {
            const spot = toScreen(pin.x, pin.z);
            if (spot.left < -60 || spot.top < -60 || spot.left > size.w + 60 || spot.top > size.h + 60) {
              return null;
            }
            return (
              <span
                className={`map-pin ${pin.kind} ${pin.icon ? "has-face" : ""} ${
                  pin.kind === "player" && !pin.online ? "away" : ""
                }`}
                key={pin.key}
                title={
                  pin.kind === "player" && !pin.online
                    ? tr("{0} · offline. Last known location.", [pin.label])
                    : undefined
                }
                style={{
                  left: `${spot.left}px`,
                  top: `${spot.top}px`,
                  transform: "translate(-50%, -50%)",
                }}
              >
                {pin.kind === "mark" ? (
                  <span className="map-mark" aria-hidden />
                ) : pin.kind === "death" ? (
                  <span className="map-death" aria-hidden>
                    <svg
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      strokeWidth="2.4"
                      strokeLinecap="round"
                    >
                      <path d="M6 6l12 12M18 6L6 18" />
                    </svg>
                  </span>
                ) : pin.kind === "player" ? (
                  <span
                    className={`map-face ${pin.online ? "on" : "off"}`}
                    data-tip={pin.online ? tr("Online now") : tr("Offline")}
                  >
                    {pin.icon ? <img src={pin.icon} alt="" /> : <i />}
                    <em />
                  </span>
                ) : (
                  <i />
                )}
                <b>{pin.label}</b>
              </span>
            );
          })}
        </div>
      </div>

      <div className="map-float map-float-top">
        {stats && (
          <div className="map-float-box map-view-box">
            <Dropdown
              className="map-view-pick"
              value={metric ?? "map"}
              options={[
                { value: "map", label: tr("Terrain") },
                ...METRICS.map((one) => ({ value: one, label: metricLabel(one) })),
              ]}
              onChange={(value) => setMetric(value === "map" ? null : (value as Metric))}
            />
          </div>
        )}
        {metric && (
          <div className="map-float-box map-legend">
            <small>{metricHint(metric)}</small>
            {scale ? (
              <>
                <span className="map-legend-bar" style={{ background: gradient() }} />
                <span className="map-legend-ends">
                  <span>{metric === "version" ? String(scale.values[0] ?? scale.min) : formatValue(metric, scale.min)}</span>
                  <span>{metric === "version" ? String(scale.values[scale.values.length - 1] ?? scale.max) : formatValue(metric, scale.max)}</span>
                </span>
              </>
            ) : (
              <small className="muted">{tr("Reading region files…")}</small>
            )}
          </div>
        )}
      </div>

      <div className="map-float map-float-layers map-float-box">
        <button
          className={`map-tool ${night ? "on" : ""}`}
          aria-pressed={night}
          data-tip={night ? tr("Night") : tr("Day")}
          aria-label={night ? tr("Night") : tr("Day")}
          onClick={() => setNight((was) => !was)}
        >
          <Icon name={night ? "moon" : "sun"} />
        </button>
        {chunks && (
          <button
            className={`map-tool ${chunks.on ? "on" : ""}`}
            aria-pressed={chunks.on}
            data-tip={
              chunks.on
                ? tr("{0, plural, one {# loaded chunk} other {# loaded chunks}}", [(chunks.loaded.length + chunks.forced.length) / 2])
                : chunks.known
                  ? tr("Blue: loaded chunks · orange: force-loaded chunks")
                  : tr("Loaded chunks in this dimension. This server type can't tell which ones are force-loaded.")
            }
            aria-label={tr("Loaded chunks")}
            onClick={chunks.toggle}
          >
            <Icon name="grid" />
          </button>
        )}
        <button
          className={`map-tool ${showGrid ? "on" : ""}`}
          aria-pressed={showGrid}
          data-tip={tr("Chunk borders · thick lines are region files (512 blocks)")}
          aria-label={tr("Chunk borders")}
          onClick={() => setShowGrid((was) => !was)}
        >
          <Icon name="chunk-grid" />
        </button>
        <button
          className={`map-tool ${showPins ? "on" : ""}`}
          aria-pressed={showPins}
          data-tip={tr("Players and bookmarks")}
          aria-label={tr("Players and bookmarks")}
          onClick={() => setShowPins((was) => !was)}
        >
          <Icon name="users" />
        </button>
        {height && (() => {
          const { min, max } = height.range;
          const auto = max + 1;
          const shown = heightDraft !== null ? Number(heightDraft) : (height.y ?? auto);
          const commit = (value: string) => {
            setHeightDraft(null);
            const y = Math.round(Number(value));
            if (!Number.isFinite(y)) return;
            height.onChange(y > max ? null : Math.max(min, y));
          };
          const drawing = height.y !== null && progress && progress.total > 0 && progress.done < progress.total;
          return (
            <div className="map-height" data-tip={height.ready ? undefined : tr("Update the Monolab addon to pick a map height.")}>
              <button
                className={`map-tool ${height.y !== null ? "on" : ""}`}
                aria-label={tr("Map height")}
                disabled={!height.ready}
              >
                <Icon name="layers" />
              </button>
              {height.ready && (
                <div className="map-height-pop map-float-box">
                  <span className="map-height-name">Y</span>
                  <input
                    type="range"
                    className="map-height-slider"
                    min={min}
                    max={auto}
                    step={1}
                    value={shown}
                    aria-label={tr("Map height")}
                    aria-valuetext={shown > max ? tr("Auto") : String(shown)}
                    onChange={(event) => setHeightDraft(event.target.value)}
                    onPointerUp={(event) => commit(event.currentTarget.value)}
                    onKeyUp={(event) => commit(event.currentTarget.value)}
                  />
                  <span className="map-height-value">{shown > max ? tr("Auto") : shown}</span>
                  {drawing && <small className="map-height-progress">{tr("Drawing {0}/{1}", [progress.done, progress.total])}</small>}
                </div>
              )}
            </div>
          );
        })()}
      </div>

      {area && !selecting && (() => {
        const b = toScreen(area.maxX + 1, area.maxZ + 1);
        const a = toScreen(area.minX, area.minZ);
        const left = Math.min(Math.max(8, b.left + 10), Math.max(8, size.w - 290));
        const top = Math.min(Math.max(8, a.top), Math.max(8, size.h - 260));
        const width = area.maxX - area.minX + 1;
        const height = area.maxZ - area.minZ + 1;
        const count = (width / 16) * (height / 16);
        const measureStay = async () => {
          if (!statsRef.current) return;
          setMetric("inhabited");
          const rx0 = Math.floor(area.minX / 512);
          const rx1 = Math.floor(area.maxX / 512);
          const rz0 = Math.floor(area.minZ / 512);
          const rz1 = Math.floor(area.maxZ / 512);
          if ((rx1 - rx0 + 1) * (rz1 - rz0 + 1) > 100) {
            setStay({ chunks: count, visited: 0, total: 0, max: 0, tooBig: true });
            return;
          }
          setStay("loading");
          let visited = 0;
          let total = 0;
          let max = 0;
          for (let rz = rz0; rz <= rz1; rz += 1) {
            for (let rx = rx0; rx <= rx1; rx += 1) {
              const key = `inhabited/${rx}/${rz}`;
              let grid = grids.current.get(key) ?? null;
              if (!grid) {
                grid = (await statsRef.current?.("inhabited", rx, rz).catch(() => null)) ?? null;
                if (grid) grids.current.set(key, grid);
              }
              if (!grid) continue;
              for (let index = 0; index < 1024; index += 1) {
                const cx = (rx * 32 + (index % 32)) * 16;
                const cz = (rz * 32 + Math.floor(index / 32)) * 16;
                if (cx < area.minX || cx > area.maxX || cz < area.minZ || cz > area.maxZ) continue;
                const value = grid[index];
                if (value === null || value === undefined) continue;
                total += value;
                if (value > 0) visited += 1;
                if (value > max) max = value;
              }
            }
          }
          setGridTick((was) => was + 1);
          setStay({ chunks: count, visited, total, max, tooBig: false });
        };
        return (
          <div className="map-area-tools map-float-box" style={{ left: `${left}px`, top: `${top}px` }} onPointerDown={(event) => event.stopPropagation()}>
            <div className="map-area-head">
              <span>
                <strong>{tr("Selected area")}</strong>
                <small>
                  {tr("{0} × {1} blocks · {2} chunks", [width.toLocaleString(), height.toLocaleString(), count.toLocaleString()])}
                </small>
              </span>
              <button className="map-tool" aria-label={tr("Clear selection")} data-tip={tr("Clear selection")} onClick={() => { setArea(null); setStay(null); }}>
                <Icon name="close" />
              </button>
            </div>
            <code className="map-area-coords">
              X {area.minX} ~ {area.maxX} · Z {area.minZ} ~ {area.maxZ}
            </code>
            {onPregenArea && (
              <button className="map-area-action" onClick={() => onPregenArea(area)}>
                <Icon name="layers" />
                {tr("Pre-generate this area")}
              </button>
            )}
            {statsRef.current && (
              <button className="map-area-action" disabled={stay === "loading"} onClick={() => void measureStay()}>
                <Icon name="clock" />
                {stay === "loading" ? tr("Reading region files…") : tr("Time players stayed here")}
              </button>
            )}
            {stay && stay !== "loading" && (
              <div className="map-area-stay">
                {stay.tooBig ? (
                  <small className="muted">{tr("The area is too big. Select a smaller area.")}</small>
                ) : (
                  <>
                    <span>
                      <small>{tr("Total")}</small>
                      <strong>{formatValue("inhabited", stay.total)}</strong>
                    </span>
                    <span>
                      <small>{tr("Longest in one chunk")}</small>
                      <strong>{formatValue("inhabited", stay.max)}</strong>
                    </span>
                    <span>
                      <small>{tr("Chunks visited")}</small>
                      <strong>
                        {stay.visited.toLocaleString()} / {stay.chunks.toLocaleString()}
                      </strong>
                    </span>
                  </>
                )}
              </div>
            )}
            <button
              className="map-area-action"
              onClick={() => {
                void navigator.clipboard?.writeText(`${area.minX} ${area.minZ} ${area.maxX} ${area.maxZ}`).then(() => setCopied(true));
              }}
            >
              <Icon name={copied ? "check" : "copy"} />
              {copied ? tr("Copied") : tr("Copy corners")}
            </button>
            {onExport && (
              <button className="map-area-action" disabled={exporting} onClick={() => void exportArea(area)}>
                <Icon name="image" />
                {exporting ? tr("Saving…") : tr("Save this area as PNG")}
              </button>
            )}
            {onDeleteArea && (
              <button className="map-area-action danger" onClick={() => onDeleteArea(area)}>
                <Icon name="trash" />
                {tr("Delete chunks in this area")}
              </button>
            )}
          </div>
        );
      })()}

      <div className="map-float map-float-where map-float-box">
        <span className="map-where">
          {where ? `X ${where.x}   Z ${where.z}` : tr("Click to add a bookmark · right-drag to select an area")}
          {where && hover && <b className="map-where-value"> · {hover}</b>}
        </span>
        {where && showGrid && (
          <span className="map-note">
            {tr("Chunk {0}, {1} · region r.{2}.{3}", [Math.floor(where.x / 16), Math.floor(where.z / 16), Math.floor(where.x / 512), Math.floor(where.z / 512)])}
          </span>
        )}
      </div>

      <div className="map-float map-float-zoom map-float-box">
        <button className="map-tool" data-tip={tr("Zoom in")} aria-label={tr("Zoom in")} onClick={() => glideTo(Math.round(zoom) - 1, size.w / 2, size.h / 2)}>
          <Icon name="plus" />
        </button>
        <button className="map-tool" data-tip={tr("Zoom out")} aria-label={tr("Zoom out")} onClick={() => glideTo(Math.round(zoom) + 1, size.w / 2, size.h / 2)}>
          <Icon name="minus" />
        </button>
      </div>
    </div>
  );
}

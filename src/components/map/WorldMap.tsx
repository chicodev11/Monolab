import { tr } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import { useEffect, useMemo, useRef, useState } from "react";
import type { MapPin } from "./pins";
import { useSmoothPins } from "./pins";

const TILE = 128;
const MIN_ZOOM = -4;
const MAX_ZOOM = 5;
const WORKERS = 6;

export type TileFetch = (z: number, tx: number, ty: number, night: boolean) => Promise<string>;

type Want = { key: string; level: number; tx: number; ty: number };

export function WorldMap({
  pins,
  onPick,
  fetchTile,
  start,
  cave,
  onExport,
  chunks,
}: {
  pins: MapPin[];
  onPick?: (x: number, z: number) => void;
  fetchTile: TileFetch;
  start: { x: number; z: number; level: number };
  cave?: { on: boolean; toggle: () => void } | null;
  onExport?: (png: string) => void;
  chunks?: { on: boolean; toggle: () => void; loaded: number[]; forced: number[]; known: boolean } | null;
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
  const [tiles, setTiles] = useState<Record<string, string>>({});
  const [night, setNight] = useState(false);
  const drag = useRef<{ x: number; y: number; from: { x: number; z: number }; moved: boolean } | null>(null);
  const glide = useRef<{ to: number; atX: number; atY: number } | null>(null);
  const frame = useRef(0);

  const bpp = 2 ** zoom;
  const level = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, Math.floor(zoom + 1e-6)));
  const prefix = (night ? "n" : "d") + (cave?.on ? "c" : "");

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
    return list;
  }, [corner.x, corner.z, level, size.w, size.h, bpp, prefix]);

  const wantRef = useRef<Want[]>([]);
  const have = useRef<Record<string, string>>({});
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
      const asNight = next.key.startsWith("n/");
      void fetchRef
        .current(next.level, next.tx, next.ty, asNight)
        .then(
          (url) => {
            const partial = url.startsWith("partial:");
            const clean = partial ? url.slice("partial:".length) : url;
            if (partial) later.current.set(next.key, Date.now() + 2500);
            else later.current.delete(next.key);
            if (!partial) have.current[next.key] = clean;
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
    const paint = (list: number[], fill: string, line: string) => {
      pen.fillStyle = fill;
      pen.strokeStyle = line;
      pen.lineWidth = 1;
      for (let at = 0; at + 1 < list.length; at += 2) {
        const left = (list[at] * 16 - corner.x) / bpp;
        const top = (list[at + 1] * 16 - corner.z) / bpp;
        if (left + cell < 0 || top + cell < 0 || left > size.w || top > size.h) continue;
        pen.fillRect(left, top, cell, cell);
        if (cell >= 6) pen.strokeRect(left + 0.5, top + 0.5, cell - 1, cell - 1);
      }
    };
    paint(chunks.loaded, "rgba(80, 200, 255, 0.28)", "rgba(80, 200, 255, 0.55)");
    paint(chunks.forced, "rgba(255, 170, 40, 0.45)", "rgba(255, 170, 40, 0.85)");
  }, [chunks, corner.x, corner.z, bpp, size.w, size.h]);

  const [exporting, setExporting] = useState(false);
  const exportView = async () => {
    if (!onExport || exporting || size.w < 16) return;
    setExporting(true);
    try {
      const scale = 2 ** level;
      const span = TILE * scale;
      const left = corner.x;
      const top = corner.z;
      const width = Math.round((size.w * bpp) / scale);
      const height = Math.round((size.h * bpp) / scale);
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
    glide.current = { to: Math.max(MIN_ZOOM, Math.min(MAX_ZOOM, target)), atX, atY };
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
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          event.currentTarget.setPointerCapture(event.pointerId);
          drag.current = { x: event.clientX, y: event.clientY, from: { x: event.clientX, z: event.clientY }, moved: false };
        }}
        onPointerMove={(event) => {
          setWhere(toWorld(event.clientX, event.clientY));
          if (!drag.current) return;
          const step = 2 ** viewRef.current.zoom;
          const last = drag.current.from;
          if (Math.abs(event.clientX - drag.current.x) + Math.abs(event.clientY - drag.current.y) > 4) drag.current.moved = true;
          const v = viewRef.current;
          setView({ zoom: v.zoom, x: v.x - (event.clientX - last.x) * step, z: v.z - (event.clientY - last.z) * step });
          drag.current.from = { x: event.clientX, z: event.clientY };
        }}
        onPointerUp={(event) => {
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
        <canvas ref={overlay} className="map-chunks" style={{ width: `${size.w}px`, height: `${size.h}px` }} />
        <div className="map-pins">
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

      <div className="map-bar">
        <span className="map-where">
          {where ? `X ${where.x}   Z ${where.z}` : tr("Click the map to add a bookmark")}
        </span>
        <button
          className={`button ghost small map-night ${night ? "on" : ""}`}
          aria-pressed={night}
          onClick={() => setNight((was) => !was)}
        >
          <Icon name={night ? "moon" : "sun"} />
          {night ? tr("Night") : tr("Day")}
        </button>
        {cave && (
          <button
            className={`button ghost small map-night ${cave.on ? "on" : ""}`}
            aria-pressed={cave.on}
            data-tip={tr("Shows cave floors below Y 32")}
            onClick={cave.toggle}
          >
            <Icon name="layers" />
            {tr("Caves")}
          </button>
        )}
        {chunks && (
          <button
            className={`button ghost small map-night ${chunks.on ? "on" : ""}`}
            aria-pressed={chunks.on}
            data-tip={
              chunks.known
                ? tr("Blue: loaded chunks · orange: force-loaded chunks")
                : tr("Loaded chunks in this dimension. This server type can't tell which ones are force-loaded.")
            }
            onClick={chunks.toggle}
          >
            <Icon name="grid" />
            {chunks.on
              ? tr("{0, plural, one {# loaded chunk} other {# loaded chunks}}", [(chunks.loaded.length + chunks.forced.length) / 2])
              : tr("Loaded chunks")}
          </button>
        )}
        {onExport && (
          <button
            className="button ghost small map-night"
            data-tip={tr("Saves what you see as a PNG")}
            disabled={exporting}
            onClick={() => void exportView()}
          >
            <Icon name="image" />
            {exporting ? tr("Saving…") : tr("Save PNG")}
          </button>
        )}
        <span className="map-note">
          {bpp >= 1
            ? tr("{0, plural, one {# block} other {# blocks}} per px", [Math.round(bpp * 10) / 10])
            : tr("{0} px per block", [Math.round((1 / bpp) * 10) / 10])}
        </span>
        <button
          className="icon-button"
          data-tip={tr("Small")}
          onClick={() => glideTo(Math.round(zoom) + 1, size.w / 2, size.h / 2)}
        >
          −
        </button>
        <button
          className="icon-button"
          data-tip={tr("Large")}
          onClick={() => glideTo(Math.round(zoom) - 1, size.w / 2, size.h / 2)}
        >
          ＋
        </button>
      </div>
    </div>
  );
}

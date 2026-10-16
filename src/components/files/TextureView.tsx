import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { TextureAnimation } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Icon } from "../ui/Icon";

const TICK = 50;

function imageZoom(size: { w: number; h: number }) {
  if (size.w <= 0 || size.h <= 0) return 1;
  const fits = Math.floor(512 / Math.max(size.w, size.h));
  return Math.max(1, Math.min(8, fits));
}

type Frame = { index: number; ticks: number };
type Film = { w: number; h: number; count: number; frames: Frame[]; blend: boolean };

export function TextureView({
  serverId,
  path,
  src,
}: {
  serverId: string;
  path: string;
  src: string;
}) {
  const [sheet, setSheet] = useState<HTMLImageElement | null>(null);
  const [animation, setAnimation] = useState<TextureAnimation | null>(null);
  const [playing, setPlaying] = useState(true);
  const [showing, setShowing] = useState(0);
  const canvas = useRef<HTMLCanvasElement | null>(null);
  const at = useRef(0);

  useEffect(() => {
    setSheet(null);
    const image = new Image();
    image.onload = () => setSheet(image);
    image.src = src;
    return () => {
      image.onload = null;
    };
  }, [src]);

  useEffect(() => {
    let alive = true;
    setAnimation(null);
    setShowing(0);
    setPlaying(true);
    at.current = 0;
    void commands
      .readTextureAnimation(serverId, path)
      .then((found) => {
        if (alive) setAnimation(found);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [serverId, path]);

  const film: Film | null = useMemo(() => {
    if (!sheet || !animation) return null;
    const w = animation.width || sheet.naturalWidth;
    const h = animation.height || w;
    if (w <= 0 || h <= 0) return null;
    const count = Math.max(1, Math.floor(sheet.naturalHeight / h));
    const order: Frame[] = animation.frames.length
      ? animation.frames.map((one) => ({ index: one.index, ticks: one.time || animation.frametime }))
      : Array.from({ length: count }, (_, index) => ({ index, ticks: animation.frametime }));
    const frames = order.filter((one) => one.index < count && one.ticks > 0);
    if (frames.length < 2) return null;
    return { w, h, count, frames, blend: animation.interpolate };
  }, [sheet, animation]);

  const paint = useCallback(
    (index: number, mix: number) => {
      const node = canvas.current;
      if (!node || !sheet || !film) return;
      const ctx = node.getContext("2d");
      if (!ctx) return;
      ctx.imageSmoothingEnabled = false;
      ctx.clearRect(0, 0, film.w, film.h);
      const one = film.frames[index];
      if (!one) return;
      ctx.drawImage(sheet, 0, one.index * film.h, film.w, film.h, 0, 0, film.w, film.h);
      if (mix > 0) {
        const next = film.frames[(index + 1) % film.frames.length];
        ctx.globalAlpha = mix;
        ctx.drawImage(sheet, 0, next.index * film.h, film.w, film.h, 0, 0, film.w, film.h);
        ctx.globalAlpha = 1;
      }
    },
    [sheet, film],
  );

  useEffect(() => {
    if (film) paint(at.current, 0);
  }, [film, paint]);

  useEffect(() => {
    if (!film || !playing) return;
    const whole = film.frames.reduce((sum, one) => sum + one.ticks, 0) * TICK;
    if (whole <= 0) return;
    const from = film.frames.slice(0, at.current).reduce((sum, one) => sum + one.ticks, 0) * TICK;
    const began = performance.now();
    let raf = 0;
    const step = (now: number) => {
      const spot = (from + (now - began)) % whole;
      let seen = 0;
      for (let index = 0; index < film.frames.length; index += 1) {
        const span = film.frames[index].ticks * TICK;
        if (spot < seen + span) {
          paint(index, film.blend ? (spot - seen) / span : 0);
          if (at.current !== index) {
            at.current = index;
            setShowing(index);
          }
          break;
        }
        seen += span;
      }
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [film, playing, paint]);

  const size = film ?? (sheet ? { w: sheet.naturalWidth, h: sheet.naturalHeight } : null);
  const zoom = size ? imageZoom(size) : 1;
  const round = film ? (film.frames.reduce((sum, one) => sum + one.ticks, 0) * TICK) / 1000 : 0;

  return (
    <>
      <div className="image-box">
        {film ? (
          <canvas
            ref={canvas}
            width={film.w}
            height={film.h}
            style={{ width: film.w * zoom }}
            aria-label={path.split("/").pop() ?? ""}
          />
        ) : (
          <img
            src={src}
            alt={path.split("/").pop() ?? ""}
            style={size ? { width: size.w * zoom } : undefined}
          />
        )}
      </div>
      <div className="image-facts">
        {size && tr("{0} × {1}", [size.w, size.h])}
        {film && <em>{tr("{0} frames · {1}s per loop", [film.frames.length, round.toFixed(2)])}</em>}
        {size && zoom > 1 && <em>{tr("shown at {0}×", [zoom])}</em>}
        {film && (
          <span className="texture-play">
            <button
              type="button"
              className="icon-button"
              aria-label={playing ? tr("Pause") : tr("Play")}
              data-tip={playing ? tr("Pause") : tr("Play")}
              onClick={() => setPlaying((was) => !was)}
            >
              <Icon name={playing ? "pause" : "play"} />
            </button>
            <b>{tr("{0} / {1}", [showing + 1, film.frames.length])}</b>
          </span>
        )}
      </div>
    </>
  );
}

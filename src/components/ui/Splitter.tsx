import { useCallback, useRef, useState, type CSSProperties } from "react";
import { tr } from "../../lib/i18n";

const KEY = "monolab.width.";

function readWidth(key: string, fallback: number, min: number, max: number) {
  try {
    const saved = Number(window.localStorage.getItem(KEY + key));
    if (Number.isFinite(saved) && saved > 0) return Math.min(max, Math.max(min, saved));
  } catch {
    // No storage (web panel in a private window): the default width.
  }
  return fallback;
}

/** A pane width the user can drag, kept per place (`key`) across restarts. */
export function usePaneWidth(key: string, fallback: number, min: number, max: number) {
  const [width, setWidth] = useState(() => readWidth(key, fallback, min, max));
  const set = useCallback(
    (next: number) => {
      const kept = Math.round(Math.min(max, Math.max(min, next)));
      setWidth(kept);
      try {
        window.localStorage.setItem(KEY + key, String(kept));
      } catch {
        // Not kept; it still applies until the page closes.
      }
    },
    [key, max, min],
  );
  const reset = useCallback(() => {
    setWidth(fallback);
    try {
      window.localStorage.removeItem(KEY + key);
    } catch {
      // Nothing kept anyway.
    }
  }, [fallback, key]);
  return { width, set, reset, min, max };
}

/**
 * The handle between two panes. `side` is where the sized pane sits: "left" grows when dragged right, "right" grows
 * when dragged left. Double-click puts the default width back; arrow keys move it 16 px.
 */
export function Splitter({ pane, side, label, className }: { pane: ReturnType<typeof usePaneWidth>; side: "left" | "right"; label: string; className?: string }) {
  const start = useRef<{ x: number; width: number } | null>(null);
  const [dragging, setDragging] = useState(false);
  const sign = side === "left" ? 1 : -1;
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={pane.width}
      aria-valuemin={pane.min}
      aria-valuemax={pane.max}
      tabIndex={0}
      data-tip={tr("Drag to resize · double-click to reset")}
      className={`pane-splitter${dragging ? " dragging" : ""}${className ? ` ${className}` : ""}`}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        event.currentTarget.setPointerCapture(event.pointerId);
        start.current = { x: event.clientX, width: pane.width };
        setDragging(true);
      }}
      onPointerMove={(event) => {
        if (!start.current) return;
        pane.set(start.current.width + sign * (event.clientX - start.current.x));
      }}
      onPointerUp={(event) => {
        start.current = null;
        setDragging(false);
        event.currentTarget.releasePointerCapture(event.pointerId);
      }}
      onPointerCancel={() => {
        start.current = null;
        setDragging(false);
      }}
      onDoubleClick={pane.reset}
      onKeyDown={(event) => {
        if (event.key === "ArrowLeft") pane.set(pane.width - 16 * sign);
        else if (event.key === "ArrowRight") pane.set(pane.width + 16 * sign);
        else return;
        event.preventDefault();
      }}
    />
  );
}

/** The CSS variable a layout reads the pane width from. */
export function widthVar(name: string, width: number): CSSProperties {
  return { [name]: `${width}px` } as CSSProperties;
}

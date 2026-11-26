import type { CSSProperties, ReactNode } from "react";
import { canvasSize, hitBox, layoutCanvas, LINE, str, useVanillaWidths, type Laid } from "./canvasModel";
import { type AppGlyph, byName } from "./studioGlyphs";
import type { StudioData } from "./StudioPreview";
import "./StudioCanvas.css";

/*
 * Studio maker M2: canvas dialogs (`"layout": "canvas"`) as the game draws them, for the previews. The rules live in
 * `canvasModel.ts`; the editor is `StudioCanvasEditor.tsx`.
 */

export { LINE, CANVAS_TYPES, canvasProblems, canvasSize, elementsOf, hitBox, isBitmap, layoutCanvas, letterWidth, textWidth } from "./canvasModel";
export type { Laid, Problem } from "./canvasModel";

/** Preview pixels per GUI pixel. */
const SCALE = 2;

/** One element drawn at `scale` screen pixels per GUI pixel. `renderText` draws MiniMessage at 2×. */
export function CanvasPiece({ one, scale, renderText, order, className = "", style, children }: { one: Laid; scale: number; renderText: (text: string, shadow: boolean) => ReactNode; order: number; className?: string; style?: CSSProperties; children?: ReactNode }) {
  const box = one.type === "area" ? hitBox(one) : one;
  const place: CSSProperties = { left: box.x * scale, top: box.y * scale, width: Math.max(1, box.w) * scale, height: Math.max(1, box.h) * scale, zIndex: order + 1, ...style };
  const kind = `mc-canvas-el ${one.type || "unknown"} ${className}`;
  if (one.type === "text") {
    return (
      <div className={kind} style={{ ...place, width: undefined, height: LINE * scale }} data-index={one.index}>
        <span className="mc-canvas-text" style={scale === SCALE ? undefined : { transform: `scale(${scale / SCALE})` }}>
          {renderText(str(one.el, "text"), one.el.shadow !== false)}
        </span>
        {children}
      </div>
    );
  }
  if (one.type === "area") return <div className={kind} style={place} data-index={one.index}>{children}</div>;
  if (one.glyph?.url) {
    return (
      <div className={kind} style={place} data-index={one.index}>
        <img src={one.glyph.url} alt="" draggable={false} />
        {children}
      </div>
    );
  }
  return (
    <div className={`${kind} empty`} style={place} data-index={one.index}>
      <span>{str(one.el, "glyph") ? `:${str(one.el, "glyph")}:` : "?"}</span>
      {children}
    </div>
  );
}

/** The canvas as the game draws it (every element shown, none pressed). */
export function CanvasView({ canvas, glyphs, renderText }: { canvas: StudioData; glyphs: AppGlyph[]; renderText: (text: string, shadow: boolean) => ReactNode }) {
  useVanillaWidths();
  const { width, height } = canvasSize(canvas);
  const laid = layoutCanvas(canvas, byName(glyphs));
  return (
    <div className="mc-canvas" style={{ width: width * SCALE, height: height * SCALE }}>
      {laid.map((one, order) => (
        <CanvasPiece key={one.index} one={one} order={order} scale={SCALE} renderText={renderText} />
      ))}
    </div>
  );
}

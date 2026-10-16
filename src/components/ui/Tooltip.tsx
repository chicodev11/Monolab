import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

const EDGE = 8;
const GAP = 9;

export function Tooltip() {
  const [tip, setTip] = useState<{ text: string; box: DOMRect } | null>(null);
  const [spot, setSpot] = useState<{ left: number; top: number; below: boolean } | null>(null);
  const node = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let timer = 0;
    const show = (event: MouseEvent) => {
      const host = (event.target as HTMLElement | null)?.closest?.(
        "[data-tip]",
      ) as HTMLElement | null;
      window.clearTimeout(timer);
      if (!host || !host.dataset.tip) {
        setTip(null);
        return;
      }
      const text = host.dataset.tip;
      timer = window.setTimeout(() => {
        setSpot(null);
        setTip({ text, box: host.getBoundingClientRect() });
      }, 350);
    };
    const hide = () => {
      window.clearTimeout(timer);
      setTip(null);
    };
    document.addEventListener("mouseover", show);
    document.addEventListener("mousedown", hide);
    document.addEventListener("scroll", hide, true);
    window.addEventListener("blur", hide);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("mouseover", show);
      document.removeEventListener("mousedown", hide);
      document.removeEventListener("scroll", hide, true);
      window.removeEventListener("blur", hide);
    };
  }, []);

  useLayoutEffect(() => {
    if (!tip || !node.current) return;
    const size = { width: node.current.offsetWidth, height: node.current.offsetHeight };
    const center = tip.box.left + tip.box.width / 2;
    const left = Math.min(Math.max(center - size.width / 2, EDGE), window.innerWidth - size.width - EDGE);
    const aboveTop = tip.box.top - GAP - size.height;
    const below = aboveTop < EDGE;
    const top = below
      ? Math.min(tip.box.bottom + GAP, window.innerHeight - size.height - EDGE)
      : aboveTop;
    setSpot({ left, top, below });
  }, [tip]);

  if (!tip) return null;
  return createPortal(
    <div
      ref={node}
      className={`tip placed ${spot?.below ? "below" : ""}`}
      role="tooltip"
      style={{
        left: spot?.left ?? 0,
        top: spot?.top ?? 0,
        visibility: spot ? "visible" : "hidden",
      }}
    >
      {tip.text}
    </div>,
    document.body,
  );
}

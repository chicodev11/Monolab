import { useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";

export type ContextItem =
  | { label: string; icon?: string; hint?: string; danger?: boolean; disabled?: boolean; onSelect: () => void }
  | "-";

export function ContextMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: ContextItem[];
  onClose: () => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const [spot, setSpot] = useState({ left: x, top: y });

  useLayoutEffect(() => {
    const menu = box.current;
    if (!menu) return;
    const gap = 8;
    const left = Math.max(gap, Math.min(x, window.innerWidth - menu.offsetWidth - gap));
    const top = y + menu.offsetHeight + gap > window.innerHeight ? Math.max(gap, y - menu.offsetHeight) : y;
    if (left !== spot.left || top !== spot.top) setSpot({ left, top });
  }, [x, y, spot.left, spot.top]);

  useLayoutEffect(() => {
    const close = (event: Event) => {
      if (event instanceof MouseEvent && box.current?.contains(event.target as Node)) return;
      onClose();
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("mousedown", close, true);
    window.addEventListener("wheel", close, true);
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    window.addEventListener("keydown", key);
    box.current?.focus();
    return () => {
      window.removeEventListener("mousedown", close, true);
      window.removeEventListener("wheel", close, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("keydown", key);
    };
  }, [onClose]);

  const shown = items.filter((one, index) => one !== "-" || (index > 0 && items[index - 1] !== "-" && index < items.length - 1));

  return createPortal(
    <div
      ref={box}
      className="context-menu"
      role="menu"
      tabIndex={-1}
      style={{ left: spot.left, top: spot.top }}
      onContextMenu={(event) => event.preventDefault()}
    >
      {shown.map((one, index) =>
        one === "-" ? (
          <hr key={index} />
        ) : (
          <button
            key={index}
            type="button"
            role="menuitem"
            className={one.danger ? "danger" : ""}
            disabled={one.disabled}
            onClick={() => {
              onClose();
              one.onSelect();
            }}
          >
            <span className="context-icon">{one.icon && <Icon name={one.icon} />}</span>
            <span className="context-label">{one.label}</span>
            {one.hint && <kbd>{one.hint}</kbd>}
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}

import { tr } from "../../lib/i18n";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";

export type DropdownOption = { value: string; label: string; hint?: string };

type Spot = {
  left?: number;
  right?: number;
  width: number;
  maxHeight: number;
  up: boolean;
  top?: number;
  bottom?: number;
};

const GAP = 5;
const EDGE = 8;

export function Dropdown({
  value,
  options,
  onChange,
  disabled,
  placeholder = tr("Choose"),
  align = "start",
  search,
  className = "",
}: {
  value: string;
  options: DropdownOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  placeholder?: string;
  align?: "start" | "end";
  search?: string;
  className?: string;
}) {
  const [openList, setOpenList] = useState(false);
  const [query, setQuery] = useState("");
  const [spot, setSpot] = useState<Spot | null>(null);
  const holder = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const current = options.find((option) => option.value === value);
  const needle = query.trim().toLowerCase();
  const shown = needle
    ? options.filter((option) => option.label.toLowerCase().includes(needle))
    : options;

  useLayoutEffect(() => {
    if (!openList) {
      setSpot(null);
      return;
    }
    const place = () => {
      const box = holder.current?.getBoundingClientRect();
      if (!box) return;
      const below = window.innerHeight - box.bottom - GAP - EDGE;
      const above = box.top - GAP - EDGE;
      const wanted = Math.min(options.length * 34 + 12, 300) + (search ? 40 : 0);
      const cap = 300 + (search ? 40 : 0);
      const up = below < wanted && above > below;
      const room = up ? above : below;
      const end = align === "end";
      let left = Math.min(box.left, window.innerWidth - box.width - EDGE);
      left = Math.max(EDGE, left);
      let right = Math.min(window.innerWidth - box.right, window.innerWidth - box.width - EDGE);
      right = Math.max(EDGE, right);
      setSpot({
        left: end ? undefined : left,
        right: end ? right : undefined,
        width: box.width,
        maxHeight: Math.max(96, Math.min(cap, room)),
        up,
        top: up ? undefined : box.bottom + GAP,
        bottom: up ? window.innerHeight - box.top + GAP : undefined,
      });
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [openList, options.length, align, search]);

  useLayoutEffect(() => {
    const box = menu.current?.getBoundingClientRect();
    if (!spot || !box) return;
    const over = box.right - (window.innerWidth - EDGE);
    const under = EDGE - box.left;
    const move = (key: "left" | "right", value: number) => {
      if (Math.abs((spot[key] ?? 0) - value) > 0.5) setSpot({ ...spot, [key]: value });
    };
    if (over > 0.5 && spot.left !== undefined) move("left", Math.max(EDGE, spot.left - over));
    else if (over > 0.5 && spot.right !== undefined) move("right", Math.max(EDGE, spot.right + over));
    else if (under > 0.5 && spot.right !== undefined) move("right", Math.max(EDGE, spot.right - under));
    else if (under > 0.5 && spot.left !== undefined) move("left", EDGE);
  }, [spot]);

  useEffect(() => {
    if (!openList) return;
    const away = (event: MouseEvent) => {
      const target = event.target as Node;
      if (holder.current?.contains(target)) return;
      if (menu.current?.contains(target)) return;
      setOpenList(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpenList(false);
    };
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("keydown", escape);
    };
  }, [openList]);

  return (
    <div className={`dropdown ${className} ${openList ? "open" : ""}`} ref={holder}>
      <button
        type="button"
        className="dropdown-trigger"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={openList}
        onClick={() => {
          setQuery("");
          setOpenList((previous) => !previous);
        }}
      >
        <span className={current ? "" : "placeholder"}>
          {current?.label ?? placeholder}
        </span>
        <Icon name="chevron" />
      </button>
      {openList &&
        !disabled &&
        spot &&
        createPortal(
          <div
            ref={menu}
            className={`dropdown-menu ${className} ${align === "end" ? "to-end" : ""} ${spot.up ? "up" : ""} ${search ? "with-search" : ""}`}
            role="listbox"
            style={{
              left: spot.left,
              right: spot.right,
              top: spot.top,
              bottom: spot.bottom,
              minWidth: spot.width,
              maxHeight: spot.maxHeight,
            }}
          >
            {search && (
              <label className="search inline-search">
                <Icon name="search" />
                <input
                  autoFocus
                  value={query}
                  placeholder={search}
                  onChange={(event) => setQuery(event.target.value)}
                />
              </label>
            )}
            <div className="dropdown-list">
              {shown.map((option) => (
                <button
                  type="button"
                  key={option.value}
                  role="option"
                  aria-selected={option.value === value}
                  className={option.value === value ? "chosen" : ""}
                  onClick={() => {
                    onChange(option.value);
                    setOpenList(false);
                  }}
                >
                  <span>{option.label}</span>
                  {option.hint && <small>{option.hint}</small>}
                  {option.value === value && <Icon name="check" />}
                </button>
              ))}
              {!shown.length && <p className="muted">{tr("No matches.")}</p>}
            </div>
          </div>,
          document.body,
        )}
    </div>
  );
}

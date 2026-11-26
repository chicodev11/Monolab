import { useEffect } from "react";

function scrolls(node: HTMLElement, axis: "x" | "y"): boolean {
  const style = getComputedStyle(node);
  const flow = axis === "y" ? style.overflowY : style.overflowX;
  if (!/auto|scroll/.test(flow)) return false;
  return axis === "y"
    ? node.scrollHeight > node.clientHeight + 1
    : node.scrollWidth > node.clientWidth + 1;
}

function scrollerAt(start: HTMLElement | null): HTMLElement | null {
  for (let node = start; node; node = node.parentElement) {
    if (scrolls(node, "y") || scrolls(node, "x")) return node;
  }
  return null;
}

export function DragScroll() {
  useEffect(() => {
    let box: HTMLElement | null = null;
    let fromX = 0;
    let fromY = 0;
    let moved = false;

    const down = (event: MouseEvent) => {
      if (event.button !== 2) return;
      box = scrollerAt(event.target as HTMLElement | null);
      if (!box) return;
      fromX = event.clientX;
      fromY = event.clientY;
      moved = false;
    };
    const move = (event: MouseEvent) => {
      if (!box) return;
      const dx = fromX - event.clientX;
      const dy = fromY - event.clientY;
      if (!moved && Math.abs(dx) + Math.abs(dy) < 5) return;
      moved = true;
      box.scrollLeft += dx;
      box.scrollTop += dy;
      fromX = event.clientX;
      fromY = event.clientY;
      document.body.classList.add("drag-scrolling");
      event.preventDefault();
    };
    const up = () => {
      box = null;
      document.body.classList.remove("drag-scrolling");
    };
    const menu = (event: MouseEvent) => {
      if (!moved) return;
      moved = false;
      event.preventDefault();
    };

    document.addEventListener("mousedown", down);
    document.addEventListener("mousemove", move);
    document.addEventListener("mouseup", up);
    document.addEventListener("contextmenu", menu);
    return () => {
      document.removeEventListener("mousedown", down);
      document.removeEventListener("mousemove", move);
      document.removeEventListener("mouseup", up);
      document.removeEventListener("contextmenu", menu);
    };
  }, []);
  return null;
}

/**
 * Every pill tab row (.ui-tabs, .segmented) gets a sliding mark behind its chosen pill. The mark is
 * the row's ::before, placed with CSS variables, so no element is added inside React's markup. It
 * takes the chosen pill's own background, read once before the pill turns see-through.
 */
const ROWS = ".ui-tabs, .segmented";
const CHOSEN = ':scope > .on, :scope > [aria-selected="true"], :scope > [aria-checked="true"]';

function clear(row: HTMLElement) {
  row.classList.remove("pill-anim", "pill-ready");
}

function place(row: HTMLElement) {
  // A row that already draws its own sliding mark keeps it.
  if (row.querySelector(":scope > .place-mark")) return;
  const chosen = row.querySelector<HTMLElement>(CHOSEN);
  if (!chosen || chosen.offsetWidth === 0) {
    clear(row);
    return;
  }
  if (!row.classList.contains("pill-anim")) {
    const look = getComputedStyle(chosen);
    const fill = look.backgroundColor;
    if (!fill || fill === "transparent" || fill === "rgba(0, 0, 0, 0)") return;
    row.style.setProperty("--pill-bg", fill);
    row.style.setProperty("--pill-radius", look.borderRadius);
    row.classList.add("pill-anim");
  }
  row.style.setProperty("--pill-x", `${chosen.offsetLeft}px`);
  row.style.setProperty("--pill-y", `${chosen.offsetTop}px`);
  row.style.setProperty("--pill-w", `${chosen.offsetWidth}px`);
  row.style.setProperty("--pill-h", `${chosen.offsetHeight}px`);
  // The first placement jumps; only later moves slide.
  if (!row.classList.contains("pill-ready")) requestAnimationFrame(() => row.classList.add("pill-ready"));
}

export function watchPillMarks() {
  let queued = false;
  const pass = () => {
    queued = false;
    document.querySelectorAll<HTMLElement>(ROWS).forEach(place);
  };
  const queue = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(pass);
  };
  new MutationObserver(queue).observe(document.body, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ["class", "aria-selected", "aria-checked"],
  });
  window.addEventListener("resize", queue);
  void document.fonts?.ready.then(queue);
  queue();
}

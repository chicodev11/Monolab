/**
 * Keyboard reach for every modal dialog (`role="dialog"` with `aria-modal`, or `role="alertdialog"`):
 * focus moves into a dialog when it opens, Tab and Shift+Tab stay inside the topmost one, and focus
 * goes back where it was when it closes. Escape is handled by the app (it closes the topmost popup).
 */

const DIALOGS = '[role="dialog"][aria-modal="true"], [role="alertdialog"]';
const FOCUSABLE = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type='hidden'])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
  "[contenteditable='true']",
].join(",");

export function topDialog(root: ParentNode = document): HTMLElement | null {
  // A docked panel (Settings, Tasks…) shares the page with the navigation, so it isn't trapped.
  const all = [...root.querySelectorAll<HTMLElement>(DIALOGS)].filter((one) => !one.closest(".as-panel"));
  return all.at(-1) ?? null;
}

export function focusables(dialog: HTMLElement): HTMLElement[] {
  return [...dialog.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (one) => !one.closest("[inert], [aria-hidden='true']") && one.getAttribute("tabindex") !== "-1",
  );
}

/** Where Tab goes from `current` inside `dialog`: wraps at either end, or null to let the browser move. */
export function nextInside(dialog: HTMLElement, current: Element | null, back: boolean): HTMLElement | null {
  const list = focusables(dialog);
  if (!list.length) return dialog;
  const at = current ? list.indexOf(current as HTMLElement) : -1;
  if (at === -1) return back ? list[list.length - 1] : list[0];
  if (back && at === 0) return list[list.length - 1];
  if (!back && at === list.length - 1) return list[0];
  return null;
}

function enter(dialog: HTMLElement) {
  if (dialog.contains(document.activeElement)) return;
  const first = dialog.querySelector<HTMLElement>("[autofocus]") ?? focusables(dialog)[0];
  if (first) first.focus();
  else {
    if (!dialog.hasAttribute("tabindex")) dialog.setAttribute("tabindex", "-1");
    dialog.focus();
  }
}

/** Starts watching; returns a function that stops. */
export function watchDialogFocus(): () => void {
  const returns: { dialog: HTMLElement; back: Element | null }[] = [];
  let shown: HTMLElement | null = null;
  const check = () => {
    // Forget dialogs that are gone, sending focus back where it was before each opened.
    for (let index = returns.length - 1; index >= 0; index -= 1) {
      const one = returns[index];
      if (one.dialog.isConnected) continue;
      returns.splice(index, 1);
      const back = one.back;
      if (back instanceof HTMLElement && back.isConnected && !topDialog()?.contains(document.activeElement)) back.focus();
    }
    const top = topDialog();
    if (top && top !== shown && !returns.some((one) => one.dialog === top)) {
      returns.push({ dialog: top, back: document.activeElement });
      // After React has put the content in.
      requestAnimationFrame(() => top.isConnected && enter(top));
    }
    shown = top;
  };
  const tab = (event: KeyboardEvent) => {
    if (event.key !== "Tab" || event.defaultPrevented) return;
    const top = topDialog();
    if (!top) return;
    const next = nextInside(top, document.activeElement, event.shiftKey);
    if (next) {
      event.preventDefault();
      next.focus();
    }
  };
  const watcher = new MutationObserver(check);
  watcher.observe(document.body, { childList: true, subtree: true });
  document.addEventListener("keydown", tab, true);
  check();
  return () => {
    watcher.disconnect();
    document.removeEventListener("keydown", tab, true);
  };
}

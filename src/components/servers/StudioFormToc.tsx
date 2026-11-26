import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { tr } from "../../lib/i18n";

/** The parts a long form is made of: mob sections and item parts (each has its own heading or switch row). */
const PARTS = ".studio-mob-section, .studio-item-part";
const labelOf = (one: Element) =>
  (one.querySelector(":scope > .studio-mob-section-head strong, :scope > .studio-toggle > span")?.textContent ?? "").trim();
const still = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/**
 * The studio form with a table of contents on the left when it is long (four parts or more). The list is read from
 * the form itself, so every kind that uses sections or parts gets one without listing them here.
 */
export function StudioFormToc({ children }: { children: ReactNode }) {
  const box = useRef<HTMLDivElement>(null);
  const parts = useRef<HTMLElement[]>([]);
  const [labels, setLabels] = useState<string[]>([]);
  const [at, setAt] = useState(0);

  const scan = useCallback(() => {
    const form = box.current;
    if (!form) return;
    const found = [...form.querySelectorAll<HTMLElement>(PARTS)].filter((one) => !one.parentElement?.closest(PARTS) && labelOf(one));
    // Fields before the first part are the basics (name, base mob…).
    const first = found[0];
    const basics = first && first.getBoundingClientRect().top - form.getBoundingClientRect().top + form.scrollTop > 24;
    parts.current = basics ? [form, ...found] : found;
    const next = [...(basics ? [tr("Basics")] : []), ...found.map(labelOf)];
    setLabels((now) => (now.join("\n") === next.join("\n") ? now : next));
  }, []);

  const follow = useCallback(() => {
    const form = box.current;
    if (!form) return;
    const top = form.getBoundingClientRect().top;
    let current = 0;
    parts.current.forEach((one, index) => {
      if (one === form) return;
      if (one.getBoundingClientRect().top - top <= 32) current = index;
    });
    if (form.scrollTop + form.clientHeight >= form.scrollHeight - 4) current = parts.current.length - 1;
    setAt(current);
  }, []);

  useEffect(() => {
    const form = box.current;
    if (!form) return;
    scan();
    const watch = new MutationObserver(() => scan());
    watch.observe(form, { childList: true, subtree: true });
    return () => watch.disconnect();
  }, [scan]);

  const go = (index: number) => {
    const form = box.current;
    const one = parts.current[index];
    if (!form || !one) return;
    const top = one === form ? 0 : one.getBoundingClientRect().top - form.getBoundingClientRect().top + form.scrollTop - 12;
    form.scrollTo({ top, behavior: still() ? "auto" : "smooth" });
    setAt(index);
  };

  const shown = labels.length >= 4;
  return (
    <div className={`studio-form-wrap ${shown ? "with-toc" : ""}`}>
      {shown && (
        <nav className="studio-toc" aria-label={tr("Parts of this file")}>
          {labels.map((label, index) => (
            <button key={`${index}-${label}`} type="button" className={index === at ? "on" : ""} aria-current={index === at ? "true" : undefined} onClick={() => go(index)}>
              {label}
            </button>
          ))}
        </nav>
      )}
      <div className="studio-form" ref={box} onScroll={shown ? follow : undefined}>
        {children}
      </div>
    </div>
  );
}

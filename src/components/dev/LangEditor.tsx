import { useEffect, useMemo, useRef, useState } from "react";
import { language, setWord, tr, wordEntries } from "../../lib/i18n";
import { Icon } from "../ui/Icon";

type Entry = { key: string; value: string };

export const LANG_EDIT_EVENT = "monolab-lang-edit";

const PAIRS: [string, string, string][] = [
  ["을(를)", "을", "를"],
  ["를(을)", "를", "을"],
  ["이(가)", "이", "가"],
  ["가(이)", "가", "이"],
  ["은(는)", "은", "는"],
  ["는(은)", "는", "은"],
  ["과(와)", "과", "와"],
  ["와(과)", "와", "과"],
  ["(으)로", "으로", "로"],
  ["(이)", "이", ""],
];

function escape(text: string) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// A value with {slots} or 을(를) markers becomes a pattern that matches what the screen shows.
function patternOf(value: string): RegExp | null {
  if (!value.includes("{") && !PAIRS.some(([marker]) => value.includes(marker))) return null;
  let source = "";
  let i = 0;
  while (i < value.length) {
    if (value[i] === "{") {
      let depth = 0;
      let j = i;
      for (; j < value.length; j++) {
        if (value[j] === "{") depth++;
        else if (value[j] === "}" && --depth === 0) break;
      }
      source += "[\\s\\S]*?";
      i = j + 1;
      continue;
    }
    const pair = PAIRS.find(([marker]) => value.startsWith(marker, i));
    if (pair) {
      source += `(?:${escape(pair[0])}|${escape(pair[1])}${pair[2] ? "|" + escape(pair[2]) : "|"})`;
      i += pair[0].length;
      continue;
    }
    source += escape(value[i]);
    i++;
  }
  try {
    return new RegExp(`^${source}$`);
  } catch {
    return null;
  }
}

function textsAt(target: Element): string[] {
  const out = new Set<string>();
  let node: Element | null = target;
  for (let depth = 0; node && depth < 4; depth++, node = node.parentElement) {
    const own = [...node.childNodes]
      .filter((child) => child.nodeType === Node.TEXT_NODE)
      .map((child) => child.textContent ?? "")
      .join("")
      .trim();
    if (own) out.add(own);
    const all = (node.textContent ?? "").trim();
    if (all && all.length < 240) out.add(all);
    for (const name of ["placeholder", "aria-label", "data-tip", "title"]) {
      const value = node.getAttribute(name);
      if (value) out.add(value.trim());
    }
  }
  return [...out];
}

function findEntries(texts: string[], entries: Entry[]): Entry[] {
  const found: Entry[] = [];
  const seen = new Set<string>();
  const add = (entry: Entry) => {
    if (!seen.has(entry.key)) {
      seen.add(entry.key);
      found.push(entry);
    }
  };
  for (const text of texts) for (const entry of entries) if (entry.value && entry.value === text) add(entry);
  for (const text of texts) {
    for (const entry of entries) {
      if (found.length >= 12) return found;
      const pattern = patternOf(entry.value);
      if (pattern?.test(text)) add(entry);
    }
  }
  return found;
}

export function LangEditor() {
  const [on, setOn] = useState(false);
  const [box, setBox] = useState<DOMRect | null>(null);
  const [picked, setPicked] = useState<Entry[]>([]);
  const [current, setCurrent] = useState<Entry | null>(null);
  const [draft, setDraft] = useState("");
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState("");
  const panel = useRef<HTMLDivElement>(null);
  const original = useRef(new Map<string, string>());
  const [version, setVersion] = useState(0);

  const entries = useMemo<Entry[]>(
    () => wordEntries().filter(([key]) => !key.startsWith("$")).map(([key, value]) => ({ key, value })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [on, version],
  );

  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "l") {
        event.preventDefault();
        setOn((was) => !was);
      } else if (event.key === "Escape" && on) {
        setOn(false);
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [on]);

  useEffect(() => {
    if (!on) {
      setBox(null);
      return;
    }
    const inPanel = (target: EventTarget | null) =>
      target instanceof Node && (panel.current?.contains(target) || (target as Element).closest?.(".lang-edit-toggle"));
    const move = (event: MouseEvent) => {
      if (inPanel(event.target) || !(event.target instanceof Element)) return setBox(null);
      setBox(event.target.getBoundingClientRect());
    };
    const block = (event: Event) => {
      if (inPanel(event.target)) return;
      event.preventDefault();
      event.stopPropagation();
    };
    const pick = (event: MouseEvent) => {
      if (inPanel(event.target) || !(event.target instanceof Element)) return;
      event.preventDefault();
      event.stopPropagation();
      const found = findEntries(textsAt(event.target), entries);
      setPicked(found);
      setQuery("");
      choose(found[0] ?? null);
      setStatus(found.length ? "" : tr("No text from the language file here."));
    };
    window.addEventListener("mousemove", move, true);
    window.addEventListener("pointerdown", block, true);
    window.addEventListener("mousedown", block, true);
    window.addEventListener("click", pick, true);
    return () => {
      window.removeEventListener("mousemove", move, true);
      window.removeEventListener("pointerdown", block, true);
      window.removeEventListener("mousedown", block, true);
      window.removeEventListener("click", pick, true);
    };
  }, [on, entries]);

  function choose(entry: Entry | null) {
    setCurrent(entry);
    setDraft(entry?.value ?? "");
  }

  const searched = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return picked;
    return entries
      .filter((one) => one.value.toLowerCase().includes(needle) || one.key.toLowerCase().includes(needle))
      .slice(0, 30);
  }, [query, picked, entries]);

  async function save(value: string) {
    if (!current) return;
    try {
      const response = await fetch("/__lang/set", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ lang: language(), key: current.key, value }),
      });
      if (!response.ok) {
        const body = (await response.json().catch(() => ({}))) as { message?: string };
        throw new Error(body.message ?? String(response.status));
      }
      if (!original.current.has(current.key)) original.current.set(current.key, current.value);
      setWord(current.key, value);
      const next = { key: current.key, value };
      setCurrent(next);
      setDraft(value);
      setPicked((list) => list.map((one) => (one.key === next.key ? next : one)));
      setVersion((n) => n + 1);
      window.dispatchEvent(new Event(LANG_EDIT_EVENT));
      setStatus(value ? tr("Saved.") : tr("Hidden."));
    } catch (error) {
      setStatus(tr("Could not save: {0}", [String(error)]));
    }
  }

  const before = current ? original.current.get(current.key) : undefined;

  return (
    <>
      <button
        type="button"
        className={`lang-edit-toggle ${on ? "on" : ""}`}
        onClick={() => setOn((was) => !was)}
        data-tip={tr("Edit text (Ctrl+Shift+L)")}
        aria-label={tr("Edit text (Ctrl+Shift+L)")}
      >
        <Icon name="pencil" />
      </button>
      {on && box && (
        <div
          className="lang-edit-box"
          style={{ left: box.left - 2, top: box.top - 2, width: box.width + 4, height: box.height + 4 }}
        />
      )}
      {on && (
        <div className="lang-edit-panel" ref={panel}>
          <div className="lang-edit-head">
            <strong>{tr("Edit text")}</strong>
            <button type="button" className="icon-button" onClick={() => setOn(false)} aria-label={tr("Close")}>
              <Icon name="close" />
            </button>
          </div>
          <input
            className="input"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={tr("Click text on screen, or search")}
          />
          {searched.length > 0 && (
            <div className="lang-edit-list">
              {searched.map((one) => (
                <button
                  type="button"
                  key={one.key}
                  className={current?.key === one.key ? "on" : ""}
                  onClick={() => choose(one)}
                >
                  <span>{one.value || tr("(hidden)")}</span>
                  <small>{one.key}</small>
                </button>
              ))}
            </div>
          )}
          {current && (
            <div className="lang-edit-form">
              <small className="lang-edit-key">{current.key}</small>
              <textarea value={draft} onChange={(event) => setDraft(event.target.value)} rows={3} />
              <div className="lang-edit-actions">
                <button type="button" className="button ghost small" onClick={() => void save("")}>
                  {tr("Hide")}
                </button>
                {before !== undefined && before !== current.value && (
                  <button type="button" className="button ghost small" onClick={() => void save(before)}>
                    {tr("Undo")}
                  </button>
                )}
                <button
                  type="button"
                  className="button primary small"
                  disabled={draft === current.value}
                  onClick={() => void save(draft)}
                >
                  {tr("Save")}
                </button>
              </div>
            </div>
          )}
          {status && <p className="lang-edit-status">{status}</p>}
        </div>
      )}
    </>
  );
}

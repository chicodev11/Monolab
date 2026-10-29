import { useEffect, useMemo, useRef, useState } from "react";
import { language, setWord, tr, wordEntries } from "../../lib/i18n";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";

type Entry = { key: string; value: string };
type Doc = { sections: Record<string, string[]>; committed: Record<string, string> | null };
type View = "all" | "changed" | "warn";

export const LANG_EDIT_EVENT = "monolab-lang-edit";

const LIMIT = 200;

const PAIRS: [string, string, string][] = [
  ["\uC744(\uB97C)", "\uC744", "\uB97C"],
  ["\uB97C(\uC744)", "\uB97C", "\uC744"],
  ["\uC774(\uAC00)", "\uC774", "\uAC00"],
  ["\uAC00(\uC774)", "\uAC00", "\uC774"],
  ["\uC740(\uB294)", "\uC740", "\uB294"],
  ["\uB294(\uC740)", "\uB294", "\uC740"],
  ["\uACFC(\uC640)", "\uACFC", "\uC640"],
  ["\uC640(\uACFC)", "\uC640", "\uACFC"],
  ["(\uC73C)\uB85C", "\uC73C\uB85C", "\uB85C"],
  ["(\uC774)", "\uC774", ""],
];

// Words the Korean style guide replaces: [pattern, word to use, word found, allowed when this also matches].
const WORDS: [RegExp, string, string, RegExp?][] = [
  [/\uC9C0(?:\uC6B0(?!\uAE30)|\uC6CC|\uC6E0|\uC6B8|\uC6B4)/, "\uC0AD\uC81C", "\uC9C0\uC6B0\uB2E4", /\uAC80\uC0C9/],
  [/\uB2E4\uC6B4\s?\uBC1B/, "\uB2E4\uC6B4\uB85C\uB4DC", "\uB2E4\uC6B4\uBC1B\uB2E4"],
  [/\uC555\uCD95(?:\uC744)?\s?\uD480/, "\uC555\uCD95 \uD574\uC81C", "\uC555\uCD95 \uD480\uAE30"],
  [/\uC790\uB3D9\uC801\uC73C\uB85C/, "\uC790\uB3D9\uC73C\uB85C", "\uC790\uB3D9\uC801\uC73C\uB85C"],
  [/\uB2E8\uCD94/, "\uBC84\uD2BC", "\uB2E8\uCD94"],
  [/\uC2F1\uD06C/, "\uB3D9\uAE30\uD654", "\uC2F1\uD06C"],
  [/\uBCF5\uAD6C/, "\uBCF5\uC6D0", "\uBCF5\uAD6C"],
  [/\uD14D\uC2A4\uD2B8\uD30C\uC77C/, "\uD14D\uC2A4\uD2B8 \uD30C\uC77C", "\uD14D\uC2A4\uD2B8\uD30C\uC77C"],
];

/** Style problems in a value, following the Korean writing rules in CLAUDE.md. */
function issuesOf(key: string, value: string, lang: string): string[] {
  const out: string[] = [];
  if (!value) return out;
  const extra = (pattern: RegExp) => pattern.test(value) && !pattern.test(key);
  if (extra(/^\s/) || extra(/\s$/)) out.push(tr("Starts or ends with a space."));
  if (/\S {2,}\S/.test(value)) out.push(tr("Has two spaces in a row."));
  if (lang !== "ko") return out;
  if (/\uC2B5\uB2C8\uAE4C/.test(value)) out.push(tr("Ask with -kkayo, not -seumnikka."));
  if (!/[.?!\u2026)]$/.test(value) && /(?:\uC5D0\uB9CC|\uC5D0\uC11C|\uC73C\uB85C|\uC744|\uB97C|\uC5D0\uAC8C)$/.test(value) && !key.startsWith("particle.") && !key.startsWith("suffix."))
    out.push(tr("A label shouldn't end with a particle."));
  if (/\S\s+\uC8FC\uC138\uC694/.test(value)) out.push(tr("Write the request ending as one word (haejuseyo)."));
  if (/\uB2F9\uC2E0|\uC800\uD76C|\uC5EC\uB7EC\uBD84/.test(value)) out.push(tr("Leave out personal pronouns."));
  if (/\uC131\uACF5\uC801\uC73C\uB85C/.test(value)) out.push(tr("Drop 'successfully'."));
  if ((value.match(/\uC218 \uC788\uC5B4\uC694/g) ?? []).length > 1) out.push(tr("Repeats 'can do'; say it once or not at all."));
  if (/\}(?:\uC744|\uB97C|\uC774|\uAC00|\uC740|\uB294|\uACFC|\uC640)(?!\()|\}(?:\uC73C\uB85C|\uB85C)/.test(value) && !key.startsWith("particle.")) out.push(tr("After a placeholder, write both particle forms so the right one is picked."));
  for (const [pattern, use, found, unless] of WORDS) {
    if (pattern.test(value) && !unless?.test(value)) out.push(tr("Use '{0}', not '{1}'.", [use, found]));
  }
  return out;
}

function escape(text: string) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// A value with {slots} or \uC744(\uB97C) markers becomes a pattern that matches what the screen shows.
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
  const [section, setSection] = useState("");
  const [view, setView] = useState<View>("all");
  const [status, setStatus] = useState("");
  const [doc, setDoc] = useState<Doc | null>(null);
  const panel = useRef<HTMLDivElement>(null);
  const [version, setVersion] = useState(0);
  const lang = language();

  const entries = useMemo<Entry[]>(
    () => wordEntries().filter(([key]) => !key.startsWith("$")).map(([key, value]) => ({ key, value })),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [on, version],
  );

  useEffect(() => {
    if (!on) return;
    let gone = false;
    void fetch(`/__lang/doc?lang=${encodeURIComponent(lang)}`)
      .then((response) => (response.ok ? (response.json() as Promise<Doc>) : null))
      .then((found) => {
        if (!gone && found) setDoc(found);
      })
      .catch(() => {});
    return () => {
      gone = true;
    };
  }, [on, lang]);

  const sectionOf = useMemo(() => {
    const out = new Map<string, string>();
    for (const [name, keys] of Object.entries(doc?.sections ?? {})) for (const key of keys) out.set(key, name);
    return out;
  }, [doc]);
  const changed = (entry: Entry) => doc?.committed != null && doc.committed[entry.key] !== entry.value;
  const changedCount = useMemo(() => entries.filter(changed).length, [entries, doc]); // eslint-disable-line react-hooks/exhaustive-deps
  const warned = useMemo(() => new Set(entries.filter((one) => issuesOf(one.key, one.value, lang).length > 0).map((one) => one.key)), [entries, lang]);

  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "l") {
        event.preventDefault();
        setOn((was) => !was);
      } else if (event.key === "Escape" && on && !document.querySelector(".dropdown-menu.lang-edit-section")) {
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
      target instanceof Node &&
      (panel.current?.contains(target) || (target as Element).closest?.(".lang-edit-toggle, .lang-edit-section"));
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
      setSection("");
      setView("all");
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

  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (view === "all" && !section && !needle) return picked;
    return entries.filter(
      (one) =>
        (view !== "changed" || changed(one)) &&
        (view !== "warn" || warned.has(one.key)) &&
        (!section || sectionOf.get(one.key) === section) &&
        (!needle || one.value.toLowerCase().includes(needle) || one.key.toLowerCase().includes(needle)),
    );
  }, [query, view, section, picked, entries, warned, sectionOf, doc]); // eslint-disable-line react-hooks/exhaustive-deps

  const groups = useMemo(() => {
    const order = Object.keys(doc?.sections ?? {});
    const out = new Map<string, Entry[]>();
    for (const one of shown.slice(0, LIMIT)) {
      const name = sectionOf.get(one.key) ?? "";
      out.set(name, [...(out.get(name) ?? []), one]);
    }
    return [...out.entries()].sort(([a], [b]) => order.indexOf(a) - order.indexOf(b));
  }, [shown, sectionOf, doc]);

  async function save(value: string) {
    if (!current) return;
    try {
      const response = await fetch("/__lang/set", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ lang, key: current.key, value }),
      });
      if (!response.ok) {
        const body = (await response.json().catch(() => ({}))) as { message?: string };
        throw new Error(body.message ?? String(response.status));
      }
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

  const committed = current && doc?.committed ? doc.committed[current.key] : undefined;
  const draftIssues = current ? issuesOf(current.key, draft, lang) : [];
  const browsing = view !== "all" || section !== "" || query.trim() !== "";

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
          <label className="search lang-edit-search">
            <Icon name="search" />
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={tr("Click text on screen, or search")}
            />
            {query && (
              <button type="button" className="icon-button" aria-label={tr("Clear search")} onClick={() => setQuery("")}>
                <Icon name="close" />
              </button>
            )}
          </label>
          <div className="lang-edit-filters">
            <div className="wb-chips">
              <button type="button" className={`wb-chip ${view === "all" ? "on" : ""}`} onClick={() => setView("all")}>
                {tr("All")}
              </button>
              <button type="button" className={`wb-chip ${view === "changed" ? "on" : ""}`} onClick={() => setView("changed")}>
                {tr("Changed")} <b>{changedCount}</b>
              </button>
              <button type="button" className={`wb-chip ${view === "warn" ? "on" : ""}`} onClick={() => setView("warn")}>
                {tr("Warnings")} <b>{warned.size}</b>
              </button>
            </div>
            <Dropdown
              className="lang-edit-section"
              value={section}
              align="end"
              options={[
                { value: "", label: tr("All sections") },
                ...Object.entries(doc?.sections ?? {}).map(([name, keys]) => ({ value: name, label: name, hint: String(keys.length) })),
              ]}
              onChange={setSection}
            />
          </div>
          {groups.length > 0 ? (
            <div className="lang-edit-list">
              {groups.map(([name, list]) => (
                <section key={name}>
                  {name && <h4>{name}</h4>}
                  {list.map((one) => (
                    <button
                      type="button"
                      key={one.key}
                      className={current?.key === one.key ? "on" : ""}
                      onClick={() => choose(one)}
                    >
                      <span>{one.value || tr("(hidden)")}</span>
                      <small>
                        {changed(one) && <em>{tr("Changed")}</em>}
                        {warned.has(one.key) && <Icon name="alert" />}
                        {one.key}
                      </small>
                    </button>
                  ))}
                </section>
              ))}
              {shown.length > LIMIT && <p className="lang-edit-more">{tr("Showing {0} of {1}. Search to narrow it down.", [LIMIT, shown.length])}</p>}
            </div>
          ) : (
            <p className={browsing ? "lang-edit-status" : "lang-edit-hint"}>
              {browsing ? tr("Nothing matches.") : tr("Click text on screen to edit it.")}
            </p>
          )}
          {current && (
            <div className="lang-edit-form">
              <small className="lang-edit-key">
                {sectionOf.get(current.key) ? `${sectionOf.get(current.key)} \u00B7 ` : ""}
                {current.key}
              </small>
              <textarea
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && event.ctrlKey && draft !== current.value) {
                    event.preventDefault();
                    void save(draft);
                  }
                }}
                rows={3}
              />
              {draftIssues.length > 0 && (
                <ul className="lang-edit-issues">
                  {draftIssues.map((one) => (
                    <li key={one}>
                      <Icon name="alert" />
                      {one}
                    </li>
                  ))}
                </ul>
              )}
              {committed !== undefined && committed !== current.value && (
                <p className="lang-edit-was">
                  <span>{tr("Last commit")}</span>
                  {committed || tr("(hidden)")}
                </p>
              )}
              <div className="lang-edit-actions">
                <button type="button" className="button ghost small" onClick={() => void save("")}>
                  {tr("Hide")}
                </button>
                {committed !== undefined && committed !== current.value && (
                  <button type="button" className="button ghost small" onClick={() => void save(committed)}>
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

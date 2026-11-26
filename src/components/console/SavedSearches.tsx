import { useState } from "react";
import { tr } from "../../lib/i18n";
import { DropdownMenu } from "../primitives";
import { Icon } from "../ui/Icon";

const KEY = "monolab.savedLogSearches";
const MOST = 20;

/** Saved log searches, shared by the console and the log files. Kept in this PC's browser storage. */
export function readSaved(): string[] {
  try {
    const raw: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((one): one is string => typeof one === "string" && one.trim() !== "").slice(0, MOST) : [];
  } catch {
    return [];
  }
}

function writeSaved(list: string[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(list.slice(0, MOST)));
  } catch {
    // Storage can be off; the list just doesn't stay.
  }
}

/** Adds `query` to the front (once), or takes it out. */
export function toggleSaved(list: string[], query: string): string[] {
  const text = query.trim();
  if (!text) return list;
  return list.includes(text) ? list.filter((one) => one !== text) : [text, ...list].slice(0, MOST);
}

export function SavedSearches({ query, onPick }: { query: string; onPick: (query: string) => void }) {
  const [saved, setSaved] = useState(readSaved);
  const text = query.trim();
  const isSaved = text !== "" && saved.includes(text);
  const change = (next: string[]) => {
    setSaved(next);
    writeSaved(next);
  };
  const items: { label: string; icon?: string; heading?: string; checked?: boolean; disabled?: boolean; onSelect: () => void }[] = [
    ...(text ? [{ label: isSaved ? tr("Forget this search") : tr("Save this search"), icon: isSaved ? "trash" : "plus", onSelect: () => change(toggleSaved(saved, text)) }] : []),
    ...saved.map((one, at) => ({ heading: at === 0 ? tr("Saved searches") : undefined, label: one, checked: one === text, onSelect: () => onPick(one) })),
  ];
  if (!items.length) items.push({ label: tr("Type a search to save it"), icon: "info", disabled: true, onSelect: () => undefined });
  return (
    <DropdownMenu
      trigger={
        <button type="button" className={`search-saved${isSaved ? " chosen" : ""}`} aria-label={tr("Saved searches")} data-tip={tr("Saved searches")}>
          <Icon name="tag" />
        </button>
      }
      items={items}
    />
  );
}

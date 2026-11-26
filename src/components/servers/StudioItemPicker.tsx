import * as RadixDialog from "@radix-ui/react-dialog";
import { createContext, useContext, useEffect, useMemo, useState, type MouseEvent, type ReactNode } from "react";
import { commands } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import { GameArtProvider } from "./gameArt";
import { ItemCell, itemName } from "./StudioPreview";

/**
 * Item slots for the studio forms: a slot that shows the item's picture and opens a picker (search the game's items
 * or the studio's own) when clicked. Right click empties it, like taking an item out in the game.
 */

/** The client version the game's pictures come from (null when the user hasn't said yes), and the studio items. */
type PickerSource = { version: string | null; studio: string[] };
const PickerContext = createContext<PickerSource>({ version: null, studio: [] });

export function ItemPickerSource({ version, studio, items, children }: { version: string | null; studio: string[]; items: string[]; children: ReactNode }) {
  const value = useMemo(() => ({ version, studio }), [version, studio]);
  return (
    <PickerContext.Provider value={value}>
      <GameArtProvider version={version} items={items}>
        {children}
      </GameArtProvider>
    </PickerContext.Provider>
  );
}

/** Every item id per client version, asked for once. */
const idLists = new Map<string, Promise<string[]>>();

function useItemIds(version: string | null): string[] | null {
  const [ids, setIds] = useState<string[] | null>(null);
  useEffect(() => {
    if (!version) {
      setIds(null);
      return;
    }
    let alive = true;
    let asked = idLists.get(version);
    if (!asked) {
      asked = commands.itemIds(version).catch(() => {
        idLists.delete(version);
        return [];
      });
      idLists.set(version, asked);
    }
    void asked.then((found) => {
      if (alive) setIds(found);
    });
    return () => {
      alive = false;
    };
  }, [version]);
  return ids;
}

/** How many results the picker draws at once; the search narrows the rest. */
const SHOWN = 96;

/** `diamond sword` and `Diamond_Sword` both find `diamond_sword`. */
export function matchItems(ids: string[], query: string): string[] {
  const words = query.trim().toLowerCase().replace(/^minecraft:/, "").split(/[\s_]+/).filter(Boolean);
  if (!words.length) return ids;
  const hits = ids.filter((id) => words.every((word) => id.includes(word)));
  const whole = query.trim().toLowerCase().replace(/\s+/g, "_");
  // Exact and starting-with matches first, then the rest in order.
  return [...hits.filter((id) => id === whole), ...hits.filter((id) => id !== whole && id.startsWith(whole)), ...hits.filter((id) => id !== whole && !id.startsWith(whole))];
}

function PickerDialog({ open, value, empty, onPick, onClose }: { open: boolean; value: string; empty: boolean; onPick: (item: string) => void; onClose: () => void }) {
  const { version, studio } = useContext(PickerContext);
  const ids = useItemIds(open ? version : null);
  const [query, setQuery] = useState("");
  useEffect(() => {
    if (open) setQuery("");
  }, [open]);
  const found = useMemo(() => (ids ? matchItems(ids, query) : []), [ids, query]);
  const shown = found.slice(0, SHOWN);
  const studioShown = studio.map((name) => `studio:${name}`).filter((one) => !query.trim() || one.includes(query.trim().toLowerCase()));
  const typed = query.trim().toLowerCase().replace(/\s+/g, "_");
  const canUseTyped = typed && /^[a-z0-9_.:/-]+$/.test(typed) && !found.includes(typed.replace(/^minecraft:/, "")) && !studioShown.includes(typed);
  const pick = (item: string) => {
    onPick(item);
    onClose();
  };
  return (
    <RadixDialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="ask-backdrop modal-backdrop" />
        <RadixDialog.Content className="ask-dialog dialog studio-picker" aria-describedby={undefined}>
          <RadixDialog.Title asChild>
            <h2>{tr("Pick an item")}</h2>
          </RadixDialog.Title>
          <input
            className="studio-picker-search"
            value={query}
            placeholder={tr("Search items, like diamond sword")}
            autoFocus
            spellCheck={false}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key !== "Enter") return;
              event.preventDefault();
              const first = studioShown[0] ?? shown[0];
              if (first && query.trim()) pick(first);
              else if (canUseTyped) pick(typed);
            }}
          />
          <PickerIcons items={[...studioShown, ...shown]}>
            <div className="studio-picker-body">
              {(empty || canUseTyped) && (
                <div className="studio-picker-extra">
                  {empty && (
                    <button type="button" className="button ghost small" onClick={() => pick("")}>
                      <Icon name="close" />
                      {tr("Leave it empty")}
                    </button>
                  )}
                  {canUseTyped && (
                    <button type="button" className="button ghost small" onClick={() => pick(typed)}>
                      <Icon name="check" />
                      {tr("Use {0}", [typed])}
                    </button>
                  )}
                </div>
              )}
              {studioShown.length > 0 && (
                <>
                  <span className="studio-picker-label">{tr("Studio items")}</span>
                  <div className="studio-picker-grid">
                    {studioShown.map((one) => (
                      <PickerChoice key={one} item={one} chosen={one === value} onPick={pick} />
                    ))}
                  </div>
                </>
              )}
              {version ? (
                <>
                  <span className="studio-picker-label">{ids === null ? tr("Loading the game's items…") : tr("{0, plural, one {# item} other {# items}}", [found.length])}</span>
                  <div className="studio-picker-grid">
                    {shown.map((one) => (
                      <PickerChoice key={one} item={one} chosen={one === value.replace(/^minecraft:/, "")} onPick={pick} />
                    ))}
                  </div>
                  {found.length > SHOWN && <p className="studio-picker-more">{tr("{0} more. Type to narrow it down.", [found.length - SHOWN])}</p>}
                </>
              ) : (
                <p className="studio-picker-more">{tr("Turn on the game's pictures in the preview to browse every item. You can still type an item name above.")}</p>
              )}
            </div>
          </PickerIcons>
          <div className="modal-actions">
            <button type="button" className="button ghost" onClick={onClose}>
              {tr("Cancel")}
            </button>
          </div>
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}

/** Loads the pictures for what the picker shows (the dialog is in a portal, so it needs its own provider). */
function PickerIcons({ items, children }: { items: string[]; children: ReactNode }) {
  const { version } = useContext(PickerContext);
  return (
    <GameArtProvider version={version} items={items}>
      {children}
    </GameArtProvider>
  );
}

function PickerChoice({ item, chosen, onPick }: { item: string; chosen: boolean; onPick: (item: string) => void }) {
  return (
    <button type="button" className={`studio-picker-choice ${chosen ? "chosen" : ""}`} title={itemName(item)} aria-label={itemName(item)} onClick={() => onPick(item)}>
      <ItemCell name={item} bare />
    </button>
  );
}

/**
 * One slot. `value` is an item name (`diamond`, `studio:<name>`) or "" for empty. `amount` draws the count like the
 * game does. `empty` lets the picker clear it.
 */
export function ItemSlot({ value, onChange, amount, empty = true, big = false, label }: { value: string; onChange: (item: string) => void; amount?: number; empty?: boolean; big?: boolean; label?: string }) {
  const [open, setOpen] = useState(false);
  const name = itemName(value);
  const tip = value ? `${label ? `${label}: ` : ""}${name}` : label || tr("Empty slot");
  const clear = (event: MouseEvent) => {
    if (!empty || !value) return;
    event.preventDefault();
    onChange("");
  };
  return (
    <>
      <button type="button" className={`studio-slot ${big ? "big" : ""} ${value ? "full" : ""}`} data-tip={tip} aria-label={tip} onClick={() => setOpen(true)} onContextMenu={clear}>
        <ItemCell name={value} amount={amount} label={tip} bare />
      </button>
      <PickerDialog open={open} value={value} empty={empty} onPick={onChange} onClose={() => setOpen(false)} />
    </>
  );
}

/** A labelled slot with the name next to it, typed or picked: drop-in for an item text field. */
export function ItemField({ label, value, placeholder, hint, empty = true, onChange }: { label: string; value: string; placeholder?: string; hint?: string; empty?: boolean; onChange: (item: string) => void }) {
  return (
    <div className="field studio-field studio-item-field">
      <span>{label}</span>
      <div className="studio-item-field-row">
        <ItemSlot value={value} onChange={onChange} empty={empty} label={label} />
        <input value={value} placeholder={placeholder} aria-label={label} onChange={(event) => onChange(event.target.value)} spellCheck={false} />
      </div>
      {hint && <small>{hint}</small>}
    </div>
  );
}

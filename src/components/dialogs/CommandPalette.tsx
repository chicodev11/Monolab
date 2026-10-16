import { tr } from "../../lib/i18n";
import { Icon } from "../ui/Icon";

export type PaletteItem = {
  id: string;
  label: string;
  hint: string;
  run: () => void;
};

export function CommandPalette({
  query,
  activeIndex,
  matches,
  onQuery,
  onIndex,
  onPick,
  onClose,
}: {
  query: string;
  activeIndex: number;
  matches: PaletteItem[];
  onQuery: (value: string) => void;
  onIndex: (next: number) => void;
  onPick: (item: PaletteItem) => void;
  onClose: () => void;
}) {
  return (

<div
  className="modal-backdrop palette-backdrop"
  onMouseDown={(event) => {
    if (event.target === event.currentTarget) onClose();
  }}
>
  <div className="palette" role="dialog" aria-label={tr("Command palette")}>
    <label className="palette-input">
      <Icon name="search" />
      <input
        autoFocus
        value={query}
        placeholder={tr("What would you like to do? Enter a server name or action")}
        onChange={(event) => {
          onQuery(event.target.value);
          onIndex(0);
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            onClose();
            return;
          }
          if (event.key === "ArrowDown") {
            event.preventDefault();
            onIndex(Math.min(activeIndex + 1, matches.length - 1));
          }
          if (event.key === "ArrowUp") {
            event.preventDefault();
            onIndex(Math.max(activeIndex - 1, 0));
          }
          if (event.key === "Enter") {
            event.preventDefault();
            const picked = matches[activeIndex];
                    if (picked) onPick(picked);
          }
        }}
      />
      <kbd>Esc</kbd>
    </label>
    <div className="palette-list">
      {matches.length ? (
        matches.map((item, index) => (
          <button
            key={item.id}
            className={index === activeIndex ? "on" : ""}
            onMouseEnter={() => onIndex(index)}
            onClick={() => onPick(item)}
          >
            <span>{item.label}</span>
            <small>{item.hint}</small>
          </button>
        ))
      ) : (
        <p className="muted">{tr("No matching actions.")}</p>
      )}
    </div>
  </div>
</div>
  );
}

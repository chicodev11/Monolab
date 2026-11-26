import { useEffect, useRef } from "react";
import { tr } from "../../lib/i18n";
import type { PaletteEntry, PaletteGroup } from "../../lib/palette";
import { Icon } from "../ui/Icon";

export type { PaletteEntry };

const GROUP_LABEL: Record<PaletteGroup, string> = {
  recent: tr("Recent"),
  server: tr("Servers"),
  tab: tr("Go to"),
  action: tr("Actions"),
  app: tr("App"),
};

/**
 * Ctrl+Space / Ctrl+K. Type to search everything; type @ or press Tab on a server to scope the list to it,
 * Backspace on an empty box to leave the scope.
 */
export function CommandPalette({
  query,
  activeIndex,
  matches,
  scopeName,
  onQuery,
  onIndex,
  onPick,
  onScope,
  onClose,
}: {
  query: string;
  activeIndex: number;
  matches: PaletteEntry[];
  /** Name of the server the list is scoped to. */
  scopeName: string | null;
  onQuery: (value: string) => void;
  onIndex: (next: number) => void;
  onPick: (item: PaletteEntry) => void;
  onScope: (serverId: string | null) => void;
  onClose: () => void;
}) {
  const list = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>("button.on")?.scrollIntoView({ block: "nearest" });
  }, [activeIndex]);
  const headings = matches.map((item, index) => (index === 0 || matches[index - 1].group !== item.group ? GROUP_LABEL[item.group] : null));
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
          {scopeName && (
            <span className="palette-scope">
              {scopeName}
              <button type="button" aria-label={tr("Search everything")} onClick={() => onScope(null)}>
                <Icon name="close" />
              </button>
            </span>
          )}
          <input
            autoFocus
            value={query}
            placeholder={scopeName ? tr("Search this server's tabs and actions") : tr("Search servers, tabs and actions · @ picks a server · # a group")}
            onChange={(event) => onQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                if (scopeName || query) {
                  onQuery("");
                  onScope(null);
                } else onClose();
                return;
              }
              if (event.key === "Backspace" && !query && scopeName) {
                event.preventDefault();
                onScope(null);
                return;
              }
              if (event.key === "ArrowDown" || (event.key === "n" && event.ctrlKey)) {
                event.preventDefault();
                onIndex(Math.min(activeIndex + 1, matches.length - 1));
              }
              if (event.key === "ArrowUp" || (event.key === "p" && event.ctrlKey)) {
                event.preventDefault();
                onIndex(Math.max(activeIndex - 1, 0));
              }
              if (event.key === "Tab") {
                event.preventDefault();
                const picked = matches[activeIndex];
                const server = picked?.scopes ?? picked?.serverId;
                if (server) onScope(server);
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
        <div className="palette-list" ref={list}>
          {matches.length ? (
            matches.map((item, index) => {
              const heading = headings[index];
              return (
                <div key={item.id} className="palette-row">
                  {heading && <span className="palette-group">{heading}</span>}
                  <button
                    type="button"
                    className={index === activeIndex ? "on" : ""}
                    onMouseEnter={() => onIndex(index)}
                    onClick={() => onPick(item)}
                  >
                    <Icon name={item.icon} />
                    <span>{item.label}</span>
                    <small>{item.hint}</small>
                    {item.scopes && index === activeIndex && <kbd>Tab</kbd>}
                  </button>
                </div>
              );
            })
          ) : (
            <p className="muted">{tr("No matching actions.")}</p>
          )}
        </div>
        <div className="palette-foot">
          <span><kbd>↑</kbd><kbd>↓</kbd> {tr("move")}</span>
          <span><kbd>Enter</kbd> {tr("run")}</span>
          <span><kbd>Tab</kbd> {tr("scope to server")}</span>
          <span><kbd>@</kbd> {tr("pick a server")}</span>
          <span><kbd>#</kbd> {tr("a server group")}</span>
        </div>
      </div>
    </div>
  );
}

import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import type { BrowseEntry } from "../../lib/bindings";
import { date, fileSize, isImage, isViewable, relativeTime } from "../../lib/format";
import { tr } from "../../lib/i18n";
import type { Sent } from "../../lib/sent";
import { Icon } from "../ui/Icon";
import { List, ListRow } from "../primitives";

/** Size and Modified columns, shared with the header in App.tsx. */
export const FILE_COLUMNS = "132px 180px";
/** The Actions column: the header's word and the rows' menu button share it. */
export const FILE_ACTIONS_W = "76px";

export type Entry = Sent<BrowseEntry>;

function fileIcon(entry: Entry): string {
  const name = entry.name.toLowerCase();
  if (entry.isDir) {
    if (name === "logs" || name === "crash-reports") return "logs";
    if (name === "plugins" || name === "mods") return "puzzle";
    if (name === "config" || name === "configs") return "file-cog";
    if (name === "backups") return "backup";
    if (name === "libraries" || name === "versions" || name === "cache") return "library";
    if (name === "world" || name.startsWith("world_") || name === "saves") return "globe";
    return "folder";
  }
  if (name.endsWith(".jar")) return "package";
  if (name.endsWith(".zip") || name.endsWith(".mrpack")) return "archive";
  if (name.endsWith(".log") || name.endsWith(".log.gz")) return "logs";
  if (/\.(json|ya?ml|toml|properties|conf|cfg|ini)$/.test(name)) return "file-cog";
  if (isImage(entry.name)) return "image";
  if (entry.editable) return "file";
  return "list";
}

const ROW_ESTIMATE = 61;

const UNKNOWN_COUNT = 4294967295;

export function FileRows({ entries, activeFile, scroll, picked, onPick, onPickSet, onOpen }: {
  entries: readonly Entry[];
  activeFile: string | null;
  scroll: RefObject<HTMLDivElement | null>;
  picked: ReadonlySet<string>;
  onPick: (entry: Entry, on: boolean) => void;
  onPickSet: (next: Set<string>) => void;
  onOpen: (entry: Entry) => void;
}) {
  const [box, setBox] = useState<HTMLDivElement | null>(null);
  useEffect(() => {
    setBox(scroll.current);
  }, [scroll]);
  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => box,
    estimateSize: () => ROW_ESTIMATE,
    getItemKey: (index) => entries[index].path,
    overscan: 10,
  });
  const drag = useRef<{ index: number; base: Set<string>; on: boolean } | null>(null);
  const dragged = useRef(false);
  const rowAt = (x: number, y: number) =>
    document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-index]")?.dataset.index;
  function startDrag(event: React.PointerEvent<HTMLUListElement>) {
    if (event.button !== 0 || (event.target as HTMLElement).closest(".file-more-menu")) return;
    const index = Number(rowAt(event.clientX, event.clientY));
    if (!Number.isFinite(index) || !entries[index]) return;
    drag.current = { index, base: new Set(picked), on: !picked.has(entries[index].path) };
    dragged.current = false;
    const onMove = (move: PointerEvent) => moveDrag(move);
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      endDrag();
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp, { once: true });
    window.addEventListener("pointercancel", onUp, { once: true });
  }
  function moveDrag(event: PointerEvent) {
    const start = drag.current;
    if (!start) return;
    const here = Number(rowAt(event.clientX, event.clientY));
    if (!Number.isFinite(here)) return;
    if (here === start.index && !dragged.current) return;
    dragged.current = true;
    const [from, to] = here < start.index ? [here, start.index] : [start.index, here];
    const next = new Set(start.base);
    for (let at = from; at <= to; at += 1) {
      const path = entries[at]?.path;
      if (!path) continue;
      if (start.on) next.add(path);
      else next.delete(path);
    }
    onPickSet(next);
    if (box) {
      const edge = box.getBoundingClientRect();
      if (event.clientY > edge.bottom - 24) box.scrollTop += 12;
      else if (event.clientY < edge.top + 24) box.scrollTop -= 12;
    }
  }
  function endDrag() {
    drag.current = null;
    if (dragged.current) window.setTimeout(() => { dragged.current = false; }, 0);
  }
  return (
    <List
      variant="plain"
      columns={FILE_COLUMNS}
      actionsWidth={FILE_ACTIONS_W}
      className="file-virtual file-cols"
      style={{ height: virtualizer.getTotalSize() }}
      onPointerDown={startDrag}
      onClickCapture={(event) => {
        if (!dragged.current) return;
        event.preventDefault();
        event.stopPropagation();
      }}
    >
      {virtualizer.getVirtualItems().map((item) => {
        const entry = entries[item.index];
        const openable = entry.isDir || entry.editable || isImage(entry.name) || isViewable(entry.name);
        return (
          <ListRow
            className={`file-row ${entry.isDir ? "is-dir" : openable ? "" : "locked"}`}
            selected={activeFile === entry.path}
            key={item.key}
            data-index={item.index}
            data-name={entry.name}
            data-path={entry.path}
            ref={virtualizer.measureElement}
            style={{ transform: `translateY(${item.start}px)` } as CSSProperties}
            leading={
              <>
                <input
                  type="checkbox"
                  className="file-pick"
                  aria-label={tr("Select {0}", [entry.name])}
                  checked={picked.has(entry.path)}
                  onChange={(event) => onPick(entry, event.target.checked)}
                />
                <span className="file-mark">
                  <Icon name={fileIcon(entry)} />
                </span>
              </>
            }
            title={entry.name}
            onClick={() => onOpen(entry)}
            disabled={!openable}
            mainProps={{
              "data-tip":
                entry.isDir
                  ? tr("Open")
                  : `${
                      entry.editable
                        ? tr("Edit")
                        : isImage(entry.name) || isViewable(entry.name)
                          ? tr("View")
                          : tr("This isn't a text file, so it can't be opened here. You can still download it.")
                    } · ${fileSize(entry.sizeBytes)}`,
            }}
            meta={[
              entry.isDir
                ? entry.itemCount === UNKNOWN_COUNT
                  ? "—"
                  : tr("{0, plural, one {# item} other {# items}}", [entry.itemCount])
                : fileSize(entry.sizeBytes),
              <span key="when" title={entry.modifiedAt ? relativeTime(entry.modifiedAt) : ""}>
                {entry.modifiedAt ? date(entry.modifiedAt) : "—"}
              </span>,
            ]}
            actions={
              <button
                className="icon-button file-more-menu"
                aria-label={tr("{0} menu", [entry.name])}
                onClick={(event) => {
                  // Same menu as a right click on the row.
                  const box = event.currentTarget.getBoundingClientRect();
                  event.currentTarget.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: box.right - 4, clientY: box.bottom + 4 }));
                }}
              >
                <Icon name="dots" />
              </button>
            }
          />
        );
      })}
    </List>
  );
}

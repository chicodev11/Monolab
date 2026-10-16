import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useRef, useState, type CSSProperties, type RefObject } from "react";
import type { BrowseEntry } from "../../lib/bindings";
import { date, fileSize, isImage, relativeTime } from "../../lib/format";
import { tr } from "../../lib/i18n";
import type { Sent } from "../../lib/sent";
import { DropdownMenu } from "../primitives";
import { Icon } from "../ui/Icon";

export type Entry = Sent<BrowseEntry>;

function fileIcon(entry: Entry): string {
  if (entry.isDir) return "folder";
  const name = entry.name.toLowerCase();
  if (name.endsWith(".jar")) return "archive";
  if (name.endsWith(".zip") || name.endsWith(".mrpack")) return "archive";
  if (name.endsWith(".log")) return "console";
  if (isImage(entry.name)) return "image";
  if (entry.editable) return "file";
  return "list";
}

const ROW_ESTIMATE = 47;

const UNKNOWN_COUNT = 4294967295;

export function FileRows({ entries, activeFile, scroll, locked, picked, onPick, onPickSet, onOpen, onRename, onDownload, onTrash, onZip, onUnzip }: {
  entries: readonly Entry[];
  activeFile: string | null;
  scroll: RefObject<HTMLDivElement | null>;
  locked: boolean;
  picked: ReadonlySet<string>;
  onPick: (entry: Entry, on: boolean) => void;
  onPickSet: (next: Set<string>) => void;
  onZip: (entry: Entry) => void;
  onUnzip: (entry: Entry) => void;
  onOpen: (entry: Entry) => void;
  onRename: (entry: Entry) => void;
  onDownload: (entry: Entry) => void;
  onTrash: (entry: Entry) => void;
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
  function startDrag(event: React.PointerEvent<HTMLDivElement>) {
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
    <div
      className="file-virtual"
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
        const openable = entry.isDir || entry.editable || isImage(entry.name);
        return (
          <div
            className={`file-row ${activeFile === entry.path ? "selected" : ""} ${
              entry.isDir ? "is-dir" : entry.editable || isImage(entry.name) ? "" : "locked"
            }`}
            key={item.key}
            data-index={item.index}
            data-name={entry.name}
            ref={virtualizer.measureElement}
            style={{ transform: `translateY(${item.start}px)` } as CSSProperties}
          >
            <input
              type="checkbox"
              className="file-pick"
              aria-label={tr("Select {0}", [entry.name])}
              checked={picked.has(entry.path)}
              onChange={(event) => onPick(entry, event.target.checked)}
            />
            <button
              className="file-open"
              disabled={!openable}
              data-tip={
                entry.isDir
                  ? tr("Open")
                  : `${
                      entry.editable
                        ? tr("Edit")
                        : isImage(entry.name)
                          ? tr("View")
                          : tr("This isn't a text file, so it can't be opened here. You can still download it.")
                    } · ${fileSize(entry.sizeBytes)}`
              }
              onClick={() => onOpen(entry)}
            >
              <Icon name={fileIcon(entry)} />
              <span className="file-name">{entry.name}</span>
              <span className="file-size">
                {entry.isDir
                  ? entry.itemCount === UNKNOWN_COUNT
                    ? "—"
                    : tr("count.items", [entry.itemCount])
                  : fileSize(entry.sizeBytes)}
              </span>
              <span className="file-when" title={entry.modifiedAt ? date(entry.modifiedAt) : ""}>
                {entry.modifiedAt ? relativeTime(entry.modifiedAt) : "—"}
              </span>
            </button>
            <DropdownMenu
              trigger={
                <button className="icon-button file-more-menu" aria-label={tr("{0} menu", [entry.name])}>
                  <Icon name="dots" />
                </button>
              }
              items={[
                ...(locked
                  ? []
                  : [{ label: tr("Rename"), icon: "pencil", onSelect: () => onRename(entry) }]),
                ...(entry.isDir
                  ? []
                  : [{ label: tr("Download"), icon: "import", onSelect: () => onDownload(entry) }]),
                { label: tr("Compress"), icon: "archive", onSelect: () => onZip(entry) },
                ...(!entry.isDir && !locked && entry.name.toLowerCase().endsWith(".zip")
                  ? [{ label: tr("Extract"), icon: "folder", onSelect: () => onUnzip(entry) }]
                  : []),
                ...(locked
                  ? []
                  : [{ label: tr("Delete"), icon: "trash", danger: true, onSelect: () => onTrash(entry) }]),
              ]}
            />
          </div>
        );
      })}
    </div>
  );
}

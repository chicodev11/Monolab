import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { NbtAction, NbtStep, NbtChunk as RawChunk, NbtRow as RawRow, NbtView as RawView } from "../../lib/bindings";
import { commands, type Sent } from "../../lib/ipc";

type NbtView = Sent<RawView>;
type NbtRow = Sent<RawRow>;
type NbtChunk = Sent<RawChunk>;
import { tr, trBack } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import "./NbtEditor.css";

/** NBT type ids, their short badge and the name shown in menus. */
export const NBT_KINDS: Record<number, { badge: string; name: string }> = {
  1: { badge: "B", name: "Byte" },
  2: { badge: "S", name: "Short" },
  3: { badge: "I", name: "Int" },
  4: { badge: "L", name: "Long" },
  5: { badge: "F", name: "Float" },
  6: { badge: "D", name: "Double" },
  7: { badge: "[B]", name: "Byte array" },
  8: { badge: "“”", name: "String" },
  9: { badge: "[ ]", name: "List" },
  10: { badge: "{ }", name: "Compound" },
  11: { badge: "[I]", name: "Int array" },
  12: { badge: "[L]", name: "Long array" },
};
const ADD_ORDER = [10, 9, 8, 1, 2, 3, 4, 5, 6, 7, 11, 12];

/** Files the NBT editor opens: single documents and Anvil regions. */
export const isNbtEditable = (name: string) =>
  !name.includes("!/") && /\.(dat|dat_old|nbt|schem|schematic|litematic|mca|mcr)$/i.test(name);

const ROW_HEIGHT = 26;

/** A row's value as shown: quoted strings, counted compounds and lists. */
function summary(row: NbtRow): string {
  if (row.kind === 8) return `"${row.preview}"`;
  if (row.kind === 10) return tr("{0, plural, one {# entry} other {# entries}}", [row.children]);
  if (row.kind === 9) {
    if (!row.children) return tr("empty");
    const item = Object.values(NBT_KINDS).find((one) => row.preview.endsWith(` · ${one.name}`));
    const count = tr("{0, plural, one {# item} other {# items}}", [row.children]);
    return item ? `${count} · ${tr(item.name)}` : count;
  }
  return row.preview;
}
const key = (path: readonly NbtStep[]) => JSON.stringify(path);
const same = (a: readonly NbtStep[] | null, b: readonly NbtStep[]) => !!a && key(a) === key(b);
const parentOf = (path: readonly NbtStep[]) => path.slice(0, -1);

type Editing = { path: NbtStep[]; field: "value" | "name"; text: string };

export function NbtEditor({
  path,
  serverId,
  session,
  running,
  onDirty,
  onError,
  onNotice,
  confirm,
}: {
  path: string;
  serverId: string;
  session: string;
  running: boolean;
  onDirty: (dirty: boolean) => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
}) {
  const [view, setView] = useState<NbtView | null>(null);
  const [chunks, setChunks] = useState<NbtChunk[] | null>(null);
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<NbtStep[] | null>([]);
  const [editing, setEditing] = useState<Editing | null>(null);
  const [adding, setAdding] = useState(false);
  const [busy, setBusy] = useState(false);
  const scroll = useRef<HTMLDivElement | null>(null);
  const tree = useRef<HTMLDivElement | null>(null);
  const searchBox = useRef<HTMLInputElement | null>(null);
  const searchNow = useRef("");
  searchNow.current = search;
  const editRef = useRef<Editing | null>(null);
  editRef.current = editing;

  const take = useCallback(
    (next: NbtView) => {
      setView(next);
      if (next.chunks) setChunks(next.chunks);
      onDirty(next.dirty);
      if (next.focus) setSelected(next.focus);
    },
    [onDirty],
  );
  const fail = useCallback((cause: unknown) => onError(trBack(String(cause))), [onError]);

  const act = useCallback(
    async (action: NbtAction) => {
      try {
        const next = await commands.nbtAct(session, action, searchNow.current);
        take(next);
        return next;
      } catch (cause) {
        fail(cause);
        return null;
      }
    },
    [session, take, fail],
  );

  useEffect(() => {
    void act({ type: "refresh" });
  }, [act]);

  // Search runs on the backend over the whole tree; wait for a pause in typing.
  const firstSearch = useRef(true);
  useEffect(() => {
    if (firstSearch.current) {
      firstSearch.current = false;
      return;
    }
    const timer = window.setTimeout(() => {
      if (view?.chunk != null || !view?.isRegion) void act({ type: "refresh" });
    }, 160);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only a new search should refetch
  }, [search]);

  const rows = useMemo(() => view?.rows ?? [], [view]);
  const index = useMemo(() => {
    const map = new Map<string, number>();
    rows.forEach((row, at) => map.set(key(row.path), at));
    return map;
  }, [rows]);
  const selectedAt = selected ? (index.get(key(selected)) ?? -1) : -1;
  const current: NbtRow | null = selectedAt >= 0 ? rows[selectedAt] : null;

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroll.current,
    estimateSize: () => ROW_HEIGHT,
    getItemKey: (at) => key(rows[at].path),
    overscan: 16,
  });
  useEffect(() => {
    if (selectedAt >= 0) virtualizer.scrollToIndex(selectedAt, { align: "auto" });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- scroll when the selection moves, not on every render
  }, [selectedAt]);

  async function openChunk(chunk: NbtChunk) {
    if (view?.chunk === chunk.index) return;
    setBusy(true);
    try {
      take(await commands.nbtOpenChunk(session, chunk.index));
      setSelected([]);
      setEditing(null);
    } catch (cause) {
      fail(cause);
    } finally {
      setBusy(false);
    }
  }

  async function save(force = false): Promise<void> {
    if (!view?.dirty) return;
    if (running && !force) {
      const go = await confirm(
        tr("Save while the server is running?"),
        tr("The server rewrites world and player files while it runs, and can overwrite your edits. Stopping it first is safer."),
        tr("Save anyway"),
        true,
      );
      if (!go) return;
    }
    try {
      take(await commands.nbtSave(serverId, session, force || running));
      onNotice(tr("File saved!"));
    } catch (cause) {
      if (String(cause).includes("CHANGED_ON_DISK")) {
        const go = await confirm(
          tr("The file changed on disk"),
          tr("Something else wrote {0} after you opened it. Saving replaces those changes with yours.", [path.split("/").pop() ?? path]),
          tr("Overwrite"),
          true,
        );
        if (go) {
          try {
            take(await commands.nbtSave(serverId, session, true));
            onNotice(tr("File saved!"));
          } catch (again) {
            fail(again);
          }
        }
        return;
      }
      fail(cause);
    }
  }

  function startEdit(row: NbtRow, field: Editing["field"]) {
    if (field === "value" && row.text == null) {
      if (row.children > 0 || row.kind === 9 || row.kind === 10) void act({ type: "toggle", path: row.path });
      return;
    }
    if (field === "name" && typeof row.path.at(-1) !== "string") return;
    setSelected(row.path);
    setEditing({ path: row.path, field, text: field === "value" ? (row.text ?? "") : row.label });
  }

  async function commitEdit() {
    // Enter commits and the blur that follows must not commit again.
    const now = editRef.current;
    if (!now) return;
    editRef.current = null;
    const row = rows[index.get(key(now.path)) ?? -1];
    const unchanged = now.field === "value" ? row?.text === now.text : row?.label === now.text;
    if (unchanged) {
      setEditing(null);
      return;
    }
    const ok = await act(
      now.field === "value" ? { type: "set", path: now.path, text: now.text } : { type: "rename", path: now.path, name: now.text },
    );
    if (!ok) {
      editRef.current = now;
      return;
    }
    setEditing(null);
    if (now.field === "name") setSelected([...parentOf(now.path), now.text.trim()]);
    tree.current?.focus();
  }

  async function remove(row: NbtRow) {
    if (row.path.length === 0) return;
    const at = index.get(key(row.path)) ?? 0;
    const next = await act({ type: "remove", path: row.path });
    if (next) setSelected(next.rows[Math.min(at, next.rows.length - 1)]?.path ?? []);
  }

  async function copy(row: NbtRow) {
    try {
      const text = await commands.nbtCopy(session, row.path, false);
      await navigator.clipboard.writeText(text);
      onNotice(tr("Copied as SNBT."));
    } catch (cause) {
      fail(cause);
    }
  }

  async function paste(row: NbtRow) {
    let text: string;
    try {
      text = (await navigator.clipboard.readText()).trim();
    } catch (cause) {
      fail(cause);
      return;
    }
    if (!text) return;
    void act({ type: "paste", path: row.path, snbt: text, name: "" });
  }

  function onKey(event: React.KeyboardEvent<HTMLDivElement>) {
    if (editing || (event.target as HTMLElement).tagName === "INPUT") return;
    const mod = event.ctrlKey || event.metaKey;
    const move = (to: number) => {
      const row = rows[Math.max(0, Math.min(rows.length - 1, to))];
      if (row) setSelected(row.path);
    };
    if (mod && event.key.toLowerCase() === "z") {
      event.preventDefault();
      void act({ type: event.shiftKey ? "redo" : "undo" });
    } else if (mod && event.key.toLowerCase() === "y") {
      event.preventDefault();
      void act({ type: "redo" });
    } else if (mod && event.key.toLowerCase() === "f") {
      event.preventDefault();
      searchBox.current?.focus();
    } else if (mod && event.key.toLowerCase() === "c" && current) {
      event.preventDefault();
      void copy(current);
    } else if (mod && event.key.toLowerCase() === "v" && current) {
      event.preventDefault();
      void paste(current);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      move(selectedAt + 1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      move(selectedAt - 1);
    } else if (event.key === "PageDown" || event.key === "PageUp") {
      event.preventDefault();
      const page = Math.max(1, Math.floor((scroll.current?.clientHeight ?? 260) / ROW_HEIGHT) - 1);
      move(selectedAt + (event.key === "PageDown" ? page : -page));
    } else if (event.key === "Home" || event.key === "End") {
      event.preventDefault();
      move(event.key === "Home" ? 0 : rows.length - 1);
    } else if (event.key === "ArrowRight" && current) {
      event.preventDefault();
      if (current.children > 0 && !current.open) void act({ type: "toggle", path: current.path });
      else if (current.open) move(selectedAt + 1);
    } else if (event.key === "ArrowLeft" && current) {
      event.preventDefault();
      if (current.open && current.path.length > 0) void act({ type: "toggle", path: current.path });
      else if (current.path.length > 0) setSelected(parentOf(current.path));
    } else if (event.key === "Enter" && current) {
      event.preventDefault();
      startEdit(current, "value");
    } else if (event.key === "F2" && current) {
      event.preventDefault();
      startEdit(current, "name");
    } else if (event.key === "Delete" && current) {
      event.preventDefault();
      void remove(current);
    } else if (event.key === "*" && current) {
      event.preventDefault();
      void act({ type: "expandAll", path: current.path });
    }
  }

  const fileName = path.split("/").pop() ?? path;
  const target = current ?? rows[0] ?? null;
  const noChunk = !!view?.isRegion && view.chunk == null;

  return (
    <div className="nbt" onKeyDownCapture={(event) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        event.stopPropagation();
        void save();
      }
    }}>
      <div className="nbt-bar">
        <label className="nbt-search">
          <Icon name="search" />
          <input
            ref={searchBox}
            value={search}
            placeholder={tr("Search names and values")}
            onChange={(event) => setSearch(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") setSearch("");
              if (event.key === "Enter" || event.key === "ArrowDown") {
                event.preventDefault();
                const hit = rows.find((row) => row.hit);
                if (hit) setSelected(hit.path);
                tree.current?.focus();
              }
            }}
            disabled={noChunk}
          />
        </label>
        <div className="nbt-tools">
          <button className="nbt-tool" disabled={noChunk || !target} data-tip={tr("Expand all") + " (*)"} aria-label={tr("Expand all")} onClick={() => target && void act({ type: "expandAll", path: target.path })}>
            <Icon name="plus" />
          </button>
          <button className="nbt-tool" disabled={noChunk} data-tip={tr("Collapse all")} aria-label={tr("Collapse all")} onClick={() => void act({ type: "collapseAll" })}>
            <Icon name="minus" />
          </button>
          <span className="nbt-sep" />
          <button className="nbt-tool" disabled={!view?.canUndo} data-tip={tr("Undo") + " (Ctrl+Z)"} aria-label={tr("Undo")} onClick={() => void act({ type: "undo" })}>
            <Icon name="undo" />
          </button>
          <button className="nbt-tool flip" disabled={!view?.canRedo} data-tip={tr("Redo") + " (Ctrl+Y)"} aria-label={tr("Redo")} onClick={() => void act({ type: "redo" })}>
            <Icon name="undo" />
          </button>
          <span className="nbt-sep" />
          <div className="nbt-add">
            <button className="nbt-tool" disabled={noChunk || !target} data-tip={tr("Add a value")} aria-label={tr("Add a value")} onClick={() => setAdding((now) => !now)}>
              <Icon name="plus" />
              <span>{tr("Add")}</span>
            </button>
            {adding && target && (
              <div className="nbt-menu" onMouseLeave={() => setAdding(false)}>
                {ADD_ORDER.map((kind) => (
                  <button
                    key={kind}
                    onClick={() => {
                      setAdding(false);
                      void act({ type: "add", path: target.path, kind, name: "" }).then(() => tree.current?.focus());
                    }}
                  >
                    <b className={`nbt-badge k${kind}`}>{NBT_KINDS[kind].badge}</b>
                    {tr(NBT_KINDS[kind].name)}
                  </button>
                ))}
              </div>
            )}
          </div>
          <button className="nbt-tool" disabled={!current} data-tip={tr("Copy as SNBT") + " (Ctrl+C)"} aria-label={tr("Copy as SNBT")} onClick={() => current && void copy(current)}>
            <Icon name="copy" />
          </button>
          <button className="nbt-tool" disabled={!current} data-tip={tr("Paste SNBT") + " (Ctrl+V)"} aria-label={tr("Paste SNBT")} onClick={() => current && void paste(current)}>
            <Icon name="paste" />
          </button>
          <button className="nbt-tool danger" disabled={!current || current.path.length === 0} data-tip={tr("Delete") + " (Del)"} aria-label={tr("Delete")} onClick={() => current && void remove(current)}>
            <Icon name="trash" />
          </button>
        </div>
        <span className="nbt-gap" />
        {view?.compression && <span className="chip subtle">{view.compression}</span>}
        <button className="ide-action strong" disabled={!view?.dirty} onClick={() => void save()} data-tip="Ctrl+S">
          {tr("Save")}
        </button>
      </div>
      {running && (
        <div className="nbt-warn">
          <Icon name="alert" />
          {tr("The server is running. It may overwrite this file; stop it before saving.")}
        </div>
      )}
      <div className="nbt-main">
        {view?.isRegion && (
          <ChunkGrid chunks={chunks ?? []} current={view.chunk} regionAt={view.regionAt} busy={busy} onOpen={(chunk) => void openChunk(chunk)} />
        )}
        <div
          className="nbt-tree"
          ref={(element) => {
            scroll.current = element;
            tree.current = element;
          }}
          tabIndex={0}
          onKeyDown={onKey}
          role="tree"
          aria-label={fileName}
        >
          {noChunk ? (
            <div className="nbt-empty">
              <Icon name="chunk-grid" />
              <p>{chunks?.length ? tr("Pick a chunk on the left to see its data.") : tr("This region has no chunks.")}</p>
            </div>
          ) : view && rows.length === 0 ? (
            <div className="nbt-empty"><p>{tr("Nothing matches.")}</p></div>
          ) : (
            <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
              {virtualizer.getVirtualItems().map((item) => {
                const row = rows[item.index];
                const isSel = same(selected, row.path);
                const edit = editing && same(editing.path, row.path) ? editing : null;
                const named = typeof row.path.at(-1) === "string";
                return (
                  <div
                    key={item.key}
                    role="treeitem"
                    aria-selected={isSel}
                    aria-expanded={row.children > 0 ? row.open : undefined}
                    className={`nbt-row${row.children > 0 ? " branch" : ""}${isSel ? " sel" : ""}${search && !row.hit && row.path.length ? " dim" : ""}`}
                    style={{ transform: `translateY(${item.start}px)`, paddingLeft: 8 + row.depth * 16 }}
                    onMouseDown={() => {
                      if (!edit) setSelected(row.path);
                    }}
                    onClick={(event) => {
                      // One click opens or closes a compound or list; the second click of a double-click does not.
                      if (edit || event.detail !== 1 || row.children === 0) return;
                      if ((event.target as HTMLElement).closest("input")) return;
                      void act({ type: "toggle", path: row.path });
                    }}
                  >
                    <button
                      className={`nbt-twist${row.children > 0 ? "" : " leaf"}${row.open ? " open" : ""}`}
                      tabIndex={-1}
                      aria-hidden
                      onClick={(event) => {
                        event.stopPropagation();
                        if (row.children > 0) void act({ type: "toggle", path: row.path });
                      }}
                    >
                      {row.children > 0 && <Icon name="chevron" />}
                    </button>
                    <b className={`nbt-badge k${row.kind}`} title={tr(NBT_KINDS[row.kind]?.name ?? "")}>{NBT_KINDS[row.kind]?.badge}</b>
                    {edit?.field === "name" ? (
                      <EditField edit={edit} onChange={setEditing} onCommit={commitEdit} onCancel={() => { setEditing(null); tree.current?.focus(); }} />
                    ) : (
                      <span
                        className={`nbt-label${named ? "" : " index"}`}
                        onDoubleClick={() => (named ? startEdit(row, "name") : undefined)}
                      >
                        {row.path.length === 0 ? (view?.chunk != null ? tr("Chunk") : fileName) : row.label}
                      </span>
                    )}
                    {row.path.length > 0 && <span className="nbt-colon">:</span>}
                    {edit?.field === "value" ? (
                      <EditField edit={edit} onChange={setEditing} onCommit={commitEdit} onCancel={() => { setEditing(null); tree.current?.focus(); }} />
                    ) : (
                      <span
                        className={`nbt-value k${row.kind}${row.text == null ? " summary" : ""}`}
                        onDoubleClick={() => {
                          // Compounds and lists already opened on the first click.
                          if (row.text != null) startEdit(row, "value");
                        }}
                      >
                        {summary(row)}
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
      <div className="ide-status">
        <span>
          {view?.dirty ? (
            <span className="ide-status-dirty"><i className="ide-dot" aria-hidden />{tr("Unsaved changes")}</span>
          ) : (
            tr("Saved")
          )}
        </span>
        <span className="ide-status-gap" />
        {current && current.path.length > 0 && (
          <span className="nbt-crumb" title={current.path.join(" › ")}>
            {current.path.map((step) => (typeof step === "number" ? `[${step}]` : step)).join(" › ")}
          </span>
        )}
        <span>{tr("Click to open · Double-click to edit · F2 rename · Del delete")}</span>
      </div>
    </div>
  );
}

function EditField({ edit, onChange, onCommit, onCancel }: {
  edit: Editing;
  onChange: (next: Editing) => void;
  onCommit: () => void;
  onCancel: () => void;
}) {
  return (
    <input
      className="nbt-input"
      autoFocus
      value={edit.text}
      spellCheck={false}
      onFocus={(event) => event.currentTarget.select()}
      onChange={(event) => onChange({ ...edit, text: event.target.value })}
      onBlur={() => onCommit()}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === "Enter") {
          event.preventDefault();
          onCommit();
        } else if (event.key === "Escape") {
          event.preventDefault();
          onCancel();
        }
      }}
    />
  );
}

/** The 32×32 chunks of a region: filled cells hold data, click one to open it. */
function ChunkGrid({ chunks, current, regionAt, busy, onOpen }: {
  chunks: NbtChunk[];
  current: number | null;
  regionAt: number[] | null;
  busy: boolean;
  onOpen: (chunk: NbtChunk) => void;
}) {
  const byIndex = useMemo(() => new Map(chunks.map((chunk) => [chunk.index, chunk])), [chunks]);
  const [hover, setHover] = useState<NbtChunk | null>(null);
  const shown = hover ?? (current != null ? (byIndex.get(current) ?? null) : null);
  const world = (chunk: NbtChunk) =>
    regionAt ? `${regionAt[0] * 32 + chunk.x}, ${regionAt[1] * 32 + chunk.z}` : `${chunk.x}, ${chunk.z}`;
  return (
    <div className={`nbt-chunks${busy ? " busy" : ""}`}>
      <div className="nbt-grid" onMouseLeave={() => setHover(null)}>
        {Array.from({ length: 1024 }, (_, at) => {
          const chunk = byIndex.get(at);
          return (
            <button
              key={at}
              className={`nbt-cell${chunk ? " full" : ""}${current === at ? " on" : ""}`}
              disabled={!chunk || busy}
              tabIndex={-1}
              aria-label={chunk ? tr("Chunk {0}", [world(chunk)]) : undefined}
              onMouseEnter={() => setHover(chunk ?? null)}
              onClick={() => chunk && onOpen(chunk)}
            />
          );
        })}
      </div>
      <div className="nbt-chunk-info">
        <span>{tr("{0, plural, one {# chunk} other {# chunks}}", [chunks.length])}</span>
        {shown && (
          <span>
            {tr("Chunk {0}", [world(shown)])}
            {shown.timestamp > 0 && ` · ${new Date(shown.timestamp * 1000).toLocaleDateString()}`}
            {` · ${(shown.bytes / 1024).toFixed(1)} KB`}
          </span>
        )}
      </div>
    </div>
  );
}

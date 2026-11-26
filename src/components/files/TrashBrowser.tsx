import { useEffect, useState } from "react";
import { commands } from "../../lib/ipc";
import type { BrowseEntry, FileView, TrashEntry } from "../../lib/bindings";
import { tr, trBack } from "../../lib/i18n";
import { fileSize, isImage } from "../../lib/format";
import { Icon } from "../ui/Icon";
import { EmptyState, List, ListRow } from "../primitives";
import { CodeEditor } from "./CodeEditor";
import "./TrashBrowser.css";
import { Splitter, usePaneWidth, widthVar } from "../ui/Splitter";

const noop = () => undefined;

function viewNote(view: FileView) {
  const base =
    view.kind === "image"
      ? tr("Image preview")
      : view.kind === "archive"
        ? tr("{0, plural, one {# file} other {# files}}", [view.entries.length])
        : view.note === "decompiled"
          ? tr("Decompiled with Vineflower. It may differ from the original source.")
          : view.kind === "nbt"
            ? tr("NBT data, read-only")
            : view.kind === "hex"
              ? tr("Not a text file, so it's shown as bytes.")
              : tr("Read-only");
  return view.note === "truncated" ? `${base} · ${tr("Only the beginning is shown.")}` : base;
}

/** Read-only look into a trashed server's folder; nothing here writes to it. */
export function TrashBrowser({ entry, onBack }: { entry: TrashEntry; onBack: () => void }) {
  const explorerPane = usePaneWidth("trash-explorer", 248, 180, 480);
  const [folder, setFolder] = useState("");
  const [rows, setRows] = useState<BrowseEntry[] | null>(null);
  const [total, setTotal] = useState(0);
  const [listFailed, setListFailed] = useState<string | null>(null);
  const [more, setMore] = useState(false);
  const [active, setActive] = useState<string | null>(null);
  const [view, setView] = useState<FileView | null>(null);
  const [viewFailed, setViewFailed] = useState<string | null>(null);

  useEffect(() => {
    let gone = false;
    setRows(null);
    setListFailed(null);
    void commands.browseTrashFiles(entry.id, folder, null).then(
      (page) => {
        if (gone) return;
        setRows(page.entries);
        setTotal(Number(page.total));
      },
      (cause) => !gone && setListFailed(trBack(String(cause))),
    );
    return () => {
      gone = true;
    };
  }, [entry.id, folder]);

  useEffect(() => {
    let gone = false;
    setView(null);
    setViewFailed(null);
    if (active === null) return;
    void commands.viewTrashFile(entry.id, active).then(
      (next) => !gone && setView(next),
      (cause) => !gone && setViewFailed(trBack(String(cause))),
    );
    return () => {
      gone = true;
    };
  }, [entry.id, active]);

  async function showMore() {
    if (!rows) return;
    setMore(true);
    try {
      const page = await commands.browseTrashFiles(entry.id, folder, rows.length);
      setRows([...rows, ...page.entries]);
      setTotal(Number(page.total));
    } catch (cause) {
      setListFailed(trBack(String(cause)));
    } finally {
      setMore(false);
    }
  }

  const head = (
    <div className="trash-browser-head">
      <button className="icon-button" aria-label={tr("Back to trash")} data-tip={tr("Back to trash")} onClick={onBack}>
        <Icon name="arrow-left" />
      </button>
      <div className="trash-browser-title">
        <h2>{entry.name}</h2>
        <code title={entry.originalPath}>{entry.originalPath}</code>
      </div>
      <span className="chip subtle">{tr("Read-only")}</span>
    </div>
  );

  if (listFailed && !folder) {
    return (
      <div className="trash-browser">
        {head}
        <EmptyState icon="folder" title={tr("Can't show these files")} hint={listFailed} />
      </div>
    );
  }

  const fileName = active
    ? active.includes("!/")
      ? active.slice(active.lastIndexOf("/", active.indexOf("!/")) + 1)
      : active.split("/").pop()
    : "";
  const trail = (active ?? "").split("!/")[0].split("/").slice(0, -1);

  return (
    <div className="trash-browser">
      {head}
      <div className="ide with-split" style={widthVar("--side-w", explorerPane.width)}>
        <aside className="ide-explorer" aria-label={tr("Explorer")}>
          <div className="ide-explorer-path" title={`/${folder}`}>
            <Icon name="folder" />
            <span>{folder ? folder.split("/").pop() : entry.name}</span>
          </div>
          <div className="ide-explorer-list">
            {listFailed && <p className="muted ide-explorer-empty">{listFailed}</p>}
            {!rows && !listFailed && <p className="muted ide-explorer-empty">{tr("Loading…")}</p>}
            <List variant="plain" density="compact" className="side-rows ide-rows">
              {folder && <ListRow onClick={() => setFolder(folder.split("/").slice(0, -1).join("/"))} leading={<Icon name="undo" />} title=".." />}
              {rows?.map((one) => (
                <ListRow
                  key={one.path}
                  data-path={one.path}
                  selected={one.path === active}
                  mainProps={{ title: one.isDir ? one.name : `${one.name} · ${fileSize(one.sizeBytes)}` }}
                  onClick={() => (one.isDir ? setFolder(one.path) : setActive(one.path))}
                  leading={<Icon name={one.isDir ? "folder" : isImage(one.name) ? "image" : "file"} />}
                  title={one.name}
                  meta={!one.isDir ? <small className="ide-row-size">{fileSize(one.sizeBytes)}</small> : undefined}
                />
              ))}
              {rows && rows.length < total && (
                <ListRow disabled={more} onClick={() => void showMore()} leading={<Icon name="arrow-down" />} title={tr("Show more · {0} left", [total - rows.length])} />
              )}
            </List>
            {rows && !rows.length && <p className="muted ide-explorer-empty">{tr("This folder is empty.")}</p>}
          </div>
        </aside>
        <Splitter pane={explorerPane} side="left" label={tr("Resize the explorer")} />
        <section className="ide-main">
          {active ? (
            <>
              <div className="ide-crumbs">
                <button onClick={() => setFolder("")}>{entry.name}</button>
                {trail.map((piece, at) => (
                  <button key={`${piece}-${at}`} onClick={() => setFolder(trail.slice(0, at + 1).join("/"))}>
                    <Icon name="chevron" />
                    {piece}
                  </button>
                ))}
                <span>
                  <Icon name="chevron" />
                  {fileName}
                </span>
              </div>
              <div className="ide-body">
                {viewFailed ? (
                  <p className="muted tool-empty">{viewFailed}</p>
                ) : !view ? (
                  <p className="muted tool-empty">{tr("Loading…")}</p>
                ) : view.kind === "image" ? (
                  <div className="file-view-image">
                    <img src={view.text} alt={fileName} />
                  </div>
                ) : view.kind === "archive" ? (
                  <div className="file-view-archive">
                    <List variant="plain" density="compact">
                      {view.entries.map((one) => (
                        <ListRow
                          key={one.path}
                          onClick={() => setActive(`${active}!/${one.path}`)}
                          leading={<Icon name={isImage(one.path) ? "image" : /\.(jar|zip)$/i.test(one.path) ? "archive" : "file"} />}
                          title={one.path}
                          meta={fileSize(one.size)}
                        />
                      ))}
                    </List>
                  </div>
                ) : (
                  <CodeEditor
                    key={active}
                    path={view.kind === "text" ? active.replace(/\.gz$/i, "") : `${active}.view`}
                    value={view.text}
                    readOnly
                    onChange={noop}
                    onSave={noop}
                    onCursor={noop}
                  />
                )}
              </div>
              <div className="ide-status">{view && <span>{viewNote(view)}</span>}</div>
            </>
          ) : (
            <div className="ide-empty">
              <EmptyState icon="file" title={tr("Choose a file on the left to view it.")} />
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

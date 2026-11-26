import { useCallback, useEffect, useMemo, useState, Fragment } from "react";
import { createPortal } from "react-dom";
import { commands } from "../../lib/ipc";
import type { DiskReport } from "../../lib/bindings";
import { tr, trBack } from "../../lib/i18n";
import { fileSize } from "../../lib/format";
import { Icon } from "../ui/Icon";
import { List, ListRow } from "../primitives";

type Props = {
  server: { id: string; name: string };
  confirm: (title: string, body: string, label: string, danger: boolean) => Promise<boolean>;
  onClose: () => void;
  onDone: (message: string) => void;
};

const CLEAN_LABEL: Record<string, () => string> = {
  "old-logs": () => tr("Log archives older than 30 days"),
  "old-crash-reports": () => tr("Crash reports older than 30 days"),
  "heap-dumps": () => tr("Memory dumps (.hprof)"),
};

export function DiskDialog({ server, confirm, onClose, onDone }: Props) {
  const [report, setReport] = useState<DiskReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    setError(null);
    setReport(null);
    commands.diskUsage(server.id).then(setReport, (cause) => setError(trBack(String(cause))));
  }, [server.id]);

  useEffect(() => load(), [load]);

  const top = useMemo(
    () => (report ? report.entries.filter((one) => !one.path.includes("/")).sort((a, b) => b.bytes - a.bytes) : []),
    [report],
  );
  const childrenOf = (path: string) =>
    (report?.entries ?? []).filter((one) => one.path.startsWith(`${path}/`)).sort((a, b) => b.bytes - a.bytes);
  const biggest = top[0]?.bytes || 1;

  const clean = async (kind: string, files: number, bytes: number) => {
    const ok = await confirm(
      tr("Delete {0}?", [CLEAN_LABEL[kind]?.() ?? kind]),
      tr("{0} files, {1}. They are deleted for good, not moved to the trash.", [files, fileSize(bytes)]),
      tr("Delete"),
      true,
    );
    if (!ok) return;
    setBusy(true);
    try {
      await commands.cleanDisk(server.id, kind);
      onDone(tr("Freed {0}.", [fileSize(bytes)]));
      load();
    } catch (cause) {
      setError(trBack(String(cause)));
    } finally {
      setBusy(false);
    }
  };

  const emptyTrash = async (bytes: number) => {
    const ok = await confirm(tr("Empty the trash?"), tr("Files in the server's trash ({0}) are deleted for good.", [fileSize(bytes)]), tr("Empty"), true);
    if (!ok) return;
    setBusy(true);
    try {
      await commands.emptyServerTrash(server.id);
      onDone(tr("Freed {0}.", [fileSize(bytes)]));
      load();
    } catch (cause) {
      setError(trBack(String(cause)));
    } finally {
      setBusy(false);
    }
  };

  const cleanable = (report?.cleanable ?? []).filter((one) => one.files > 0);

  return createPortal(
    <div className="modal-backdrop roomy" onClick={() => !busy && onClose()}>
      <div className="add-modal disk-dialog" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
        <div className="tool-top">
          <p>
            <strong>{tr("Disk usage")}</strong> · {server.name}
          </p>
          <div className="modal-top-actions">
            <button className="icon-button" aria-label={tr("Refresh")} data-tip={tr("Refresh")} disabled={busy} onClick={load}>
              <Icon name="refresh" />
            </button>
            <button className="close in-flow" aria-label={tr("Close")} disabled={busy} onClick={onClose}>
              <Icon name="close" />
            </button>
          </div>
        </div>
        {error && <p className="modal-error">{error}</p>}
        {!report && !error && <p className="muted tool-empty">{tr("Measuring folders… This can take a while for big worlds.")}</p>}
        {report && (
          <div className="disk-body">
            <div className="disk-stats">
              <div>
                <span>{tr("Server folder")}</span>
                <strong>{fileSize(report.totalBytes)}</strong>
              </div>
              <div>
                <span>{tr("Backups")}</span>
                <strong>{fileSize(report.backupsBytes)}</strong>
              </div>
              <div>
                <span>{tr("Trash")}</span>
                <strong>{fileSize(report.trashBytes)}</strong>
              </div>
              <div>
                <span>{tr("Free on this drive")}</span>
                <strong>{report.freeBytes ? fileSize(report.freeBytes) : "—"}</strong>
              </div>
            </div>
            {(cleanable.length > 0 || report.trashBytes > 0) && (
              <section className="disk-clean">
                <h4>{tr("Can be cleaned up")}</h4>
                <List variant="box" density="compact">
                {cleanable.map((one) => (
                  <ListRow
                    key={one.kind}
                    title={CLEAN_LABEL[one.kind]?.() ?? one.kind}
                    meta={tr("{0} files · {1}", [one.files, fileSize(one.bytes)])}
                    actions={
                    <button className="button secondary small" disabled={busy} onClick={() => void clean(one.kind, one.files, one.bytes)}>
                      {tr("Delete")}
                    </button>
                    }
                  />
                ))}
                {report.trashBytes > 0 && (
                  <ListRow
                    title={tr("Server trash")}
                    meta={fileSize(report.trashBytes)}
                    actions={
                    <button className="button secondary small" disabled={busy} onClick={() => void emptyTrash(report.trashBytes)}>
                      {tr("Empty")}
                    </button>
                    }
                  />
                )}
                </List>
              </section>
            )}
            <section className="disk-list">
              <h4>{tr("What takes up space")}</h4>
              <List variant="plain" columns="minmax(80px, 160px) 84px" actionsWidth="20px" className="disk-rows">
              {top.map((one) => {
                const inner = one.directory ? childrenOf(one.path) : [];
                const expanded = open === one.path;
                return (
                  <Fragment key={one.path}>
                    <ListRow
                      className={`disk-entry ${expanded ? "open" : ""}`}
                      onClick={inner.length > 0 ? () => setOpen(expanded ? null : one.path) : undefined}
                      mainProps={inner.length > 0 ? { "aria-expanded": expanded } : undefined}
                      leading={<Icon name={one.directory ? "folder" : "file"} />}
                      title={one.path}
                      meta={[
                        <span key="bar" className="disk-bar">
                          <span style={{ width: `${Math.max(1, (one.bytes / biggest) * 100)}%` }} />
                        </span>,
                        <span key="size" className="disk-size">{fileSize(one.bytes)}</span>,
                      ]}
                      actions={inner.length > 0 ? <Icon name="chevron" /> : undefined}
                    />
                    {expanded &&
                      inner.slice(0, 40).map((child) => (
                        <ListRow
                          key={child.path}
                          className="disk-entry child"
                          leading={<Icon name={child.directory ? "folder" : "file"} />}
                          title={child.path.slice(one.path.length + 1)}
                          meta={[
                            <span key="bar" className="disk-bar">
                              <span style={{ width: `${Math.max(1, (child.bytes / Math.max(1, one.bytes)) * 100)}%` }} />
                            </span>,
                            <span key="size" className="disk-size">{fileSize(child.bytes)}</span>,
                          ]}
                        />
                      ))}
                  </Fragment>
                );
              })}
              </List>
            </section>
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}

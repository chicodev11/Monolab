import { lazy, Suspense, useEffect, useState } from "react";
import type { Side } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Icon } from "../ui/Icon";

const CodeEditor = lazy(() => import("../files/CodeEditor").then((module) => ({ default: module.CodeEditor })));

export type SftpFile = { side: Side; dir: string; name: string; place: string };

/** A text file from either SFTP pane, edited in place and saved back to the same side. */
export function SftpEditor({
  file,
  onClose,
  onError,
  onNotice,
  confirm,
}: {
  file: SftpFile;
  onClose: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
}) {
  const [saved, setSaved] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const sep = file.side.kind === "local" ? "\\" : "/";
  const path = file.dir ? `${file.dir.replace(/[\\/]+$/, "")}${sep}${file.name}` : file.name;
  const dirty = saved !== null && text !== saved;

  useEffect(() => {
    let gone = false;
    commands
      .sftpReadText(file.side, file.dir, file.name)
      .then((got) => {
        if (gone) return;
        setSaved(got);
        setText(got);
      })
      .catch((cause) => {
        if (gone) return;
        onError(String(cause));
        onClose();
      });
    return () => {
      gone = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file.dir, file.name]);

  const save = async () => {
    if (!dirty || busy) return;
    setBusy(true);
    try {
      await commands.sftpWriteText(file.side, file.dir, file.name, text);
      setSaved(text);
      onNotice(tr("Saved {0}.", [file.name]));
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy(false);
    }
  };
  const close = async () => {
    if (dirty && !(await confirm(tr("Close without saving?"), tr("Your changes to {0} will be lost.", [file.name]), tr("Close"), true))) return;
    onClose();
  };

  return (
    <section className="sftp-editor">
      <div className="sftp-editor-head">
        <button type="button" className="wb-icon" aria-label={tr("Back to list")} data-tip={tr("Back to list")} onClick={() => void close()}>
          <Icon name="arrow-left" />
        </button>
        <div className="sftp-editor-title">
          <strong>{file.name}</strong>
          <small>
            {file.place} · {path}
          </small>
        </div>
        <span className="live-spacer" />
        {dirty && (
          <>
            <button type="button" className="button ghost small" onClick={() => setText(saved ?? "")}>
              <Icon name="undo" />
              {tr("Roll back")}
            </button>
            <button type="button" className="button primary small" disabled={busy} onClick={() => void save()} data-tip="Ctrl+S">
              {tr("Save")}
            </button>
          </>
        )}
      </div>
      <div className="sftp-editor-body">
        {saved === null ? (
          <p className="muted tool-empty">{tr("Loading…")}</p>
        ) : (
          <Suspense fallback={null}>
            <CodeEditor path={file.name} value={text} readOnly={false} onChange={setText} onSave={() => void save()} onCursor={() => undefined} />
          </Suspense>
        )}
      </div>
      <div className="ide-status">
        {dirty ? (
          <span className="ide-status-dirty">
            <i className="ide-dot" aria-hidden />
            {tr("Unsaved changes")}
          </span>
        ) : (
          <span>{tr("Saved")}</span>
        )}
        <span className="ide-status-gap" />
        <span>{text.includes("\r\n") ? "CRLF" : "LF"}</span>
      </div>
    </section>
  );
}

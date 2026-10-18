import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { commands } from "../../lib/ipc";
import { listenTo } from "../../lib/bridge";
import type { TrimReport } from "../../lib/bindings";
import { tr, trBack } from "../../lib/i18n";
import { fileSize } from "../../lib/format";
import { Icon } from "../ui/Icon";
import { Dropdown } from "../ui/Dropdown";

type Props = {
  server: { id: string; name: string; status: string; remoteHost?: string };
  realms: string[];
  confirm: (title: string, body: string, label: string, danger: boolean) => Promise<boolean>;
  onClose: () => void;
  onDone: (message: string) => void;
};

const MINUTES = [0, 1, 5, 30];

export function TrimDialog({ server, realms, confirm, onClose, onDone }: Props) {
  const [realm, setRealm] = useState(realms[0] ?? "overworld");
  const [minutes, setMinutes] = useState(0);
  const [radius, setRadius] = useState(16);
  const [report, setReport] = useState<TrimReport | null>(null);
  const [busy, setBusy] = useState<"preview" | "apply" | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let stop: (() => void) | undefined;
    void listenTo<{ serverId: string; done: number; total: number }>("trim-progress", (one) => {
      if (one.serverId === server.id) setProgress({ done: one.done, total: one.total });
    }).then((off) => {
      stop = off;
    });
    return () => stop?.();
  }, [server.id]);

  useEffect(() => setReport(null), [realm, minutes, radius]);

  const run = async (apply: boolean) => {
    setError(null);
    setBusy(apply ? "apply" : "preview");
    setProgress(null);
    try {
      const made = await commands.trimChunks(server.id, realm, minutes, radius, apply);
      setReport(made);
      if (apply) onDone(tr("Removed {0} chunks. Backup: {1}", [made.removed.toLocaleString(), made.backup]));
    } catch (cause) {
      setError(trBack(String(cause)));
    } finally {
      setBusy(null);
    }
  };

  const apply = async () => {
    if (!report) return;
    const ok = await confirm(
      tr("Remove {0} chunks?", [report.removed.toLocaleString()]),
      server.remoteHost
        ? tr("A backup of the whole server folder is made on the SSH server first. For a big server this can take several minutes. Removed chunks are generated fresh when someone goes there again, so builds in them are lost.")
        : tr("A backup of the worlds is made first. Removed chunks are generated fresh when someone goes there again, so builds in them are lost."),
      tr("Remove chunks"),
      true,
    );
    if (ok) await run(true);
  };

  const label = (one: string) => (one === "nether" ? tr("Nether") : one === "end" ? tr("End") : tr("Overworld"));
  const running = server.status !== "stopped" && server.status !== "crashed";

  return createPortal(
    <div className="modal-backdrop roomy" onClick={() => !busy && onClose()}>
      <div className="add-modal trim-dialog" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
        <div className="tool-top">
          <p>
            <strong>{tr("Clean up chunks")}</strong> · {server.name}
          </p>
          <div className="modal-top-actions">
            <button className="close in-flow" disabled={!!busy} aria-label={tr("Close")} onClick={onClose}>
              <Icon name="close" />
            </button>
          </div>
        </div>
        <p className="muted trim-lead">
          {tr("Removes chunks that players barely spent time in, such as ones made by pre-generation, to make the world smaller. They are generated again when someone visits.")}
        </p>
        <div className="trim-fields">
          <label>
            <span>{tr("Dimension")}</span>
            <Dropdown value={realm} options={realms.map((one) => ({ value: one, label: label(one) }))} onChange={setRealm} />
          </label>
          <label>
            <span>{tr("Remove chunks players stayed in for")}</span>
            <Dropdown
              value={String(minutes)}
              options={MINUTES.map((one) => ({
                value: String(one),
                label: one === 0 ? tr("Never (0 minutes)") : tr("{0} minutes or less", [one]),
              }))}
              onChange={(value) => setMinutes(Number(value))}
            />
          </label>
          <label>
            <span>{tr("Always keep around spawn (chunks)")}</span>
            <input type="number" min={0} max={512} value={radius} onChange={(event) => setRadius(Math.max(0, Math.min(512, Number(event.target.value) || 0)))} />
          </label>
        </div>
        {error && <p className="trim-error">{error}</p>}
        {busy && (
          <div className="trim-progress">
            <span>{busy === "apply" ? tr("Backing up and removing chunks…") : tr("Reading region files…")}</span>
            {progress && progress.total > 0 && (
              <div className="trim-bar">
                <div className="trim-bar-fill" style={{ width: `${Math.round((progress.done / progress.total) * 100)}%` }} />
              </div>
            )}
          </div>
        )}
        {report && !busy && (
          <div className="trim-report">
            <div><span>{tr("Region files")}</span><strong>{report.regions.toLocaleString()}</strong></div>
            <div><span>{tr("Chunks")}</span><strong>{report.chunks.toLocaleString()}</strong></div>
            <div><span>{report.applied ? tr("Removed") : tr("Will be removed")}</span><strong>{report.removed.toLocaleString()}</strong></div>
            <div><span>{tr("Size")}</span><strong>{fileSize(report.bytesBefore)} → {fileSize(report.bytesAfter)}</strong></div>
          </div>
        )}
        <div className="trim-actions">
          {running && <span className="muted">{tr("Stop the server to remove chunks. You can preview while it runs.")}</span>}
          <button className="button secondary" disabled={!!busy} onClick={() => void run(false)}>
            <Icon name="search" />
            {tr("Preview")}
          </button>
          <button
            className="button danger"
            disabled={!!busy || running || !report || report.applied || report.removed === 0}
            onClick={() => void apply()}
          >
            <Icon name="trash" />
            {tr("Remove chunks")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

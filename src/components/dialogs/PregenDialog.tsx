import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { commands } from "../../lib/ipc";
import { tr, trBack } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import { Dropdown } from "../ui/Dropdown";
import { SegmentedControl } from "../primitives";

type Props = {
  server: { id: string; name: string; status: string };
  onClose: () => void;
  onNotice: (message: string) => void;
};

const CHUNKY_ID = "fALzjamp";

type ChunkyStatus = Awaited<ReturnType<typeof commands.chunkyStatus>>;

export function PregenDialog({ server, onClose, onNotice }: Props) {
  const [status, setStatus] = useState<ChunkyStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [realm, setRealm] = useState(0);
  const [center, setCenter] = useState<{ x: string; z: string } | null>(null);
  const [radius, setRadius] = useState("2000");
  const [shape, setShape] = useState<"square" | "circle">("square");
  const running = server.status === "running";

  const load = useCallback(() => {
    commands.chunkyStatus(server.id).then(
      (got) => {
        setStatus(got);
        setCenter((was) => was ?? { x: String(got.spawn?.[0] ?? 0), z: String(got.spawn?.[1] ?? 0) });
      },
      (cause) => setError(trBack(String(cause))),
    );
  }, [server.id]);

  useEffect(() => {
    load();
    const timer = window.setInterval(load, 2000);
    return () => window.clearInterval(timer);
  }, [load]);

  const act = async (work: () => Promise<unknown>, done?: string) => {
    setError(null);
    setBusy(true);
    try {
      await work();
      if (done) onNotice(done);
      load();
    } catch (cause) {
      setError(trBack(String(cause)));
    } finally {
      setBusy(false);
    }
  };

  const install = () =>
    act(
      () => commands.installModrinth(server.id, CHUNKY_ID, status?.folder === "plugins" ? "plugin" : "mod", null),
      running ? tr("Installed Chunky. Restart the server to use it.") : tr("Installed Chunky."),
    );

  const world = status?.worlds[realm] ?? "";
  const run = (action: string) =>
    act(() =>
      commands.chunkyRun(server.id, action, world, Math.round(Number(center?.x) || 0), Math.round(Number(center?.z) || 0), Math.round(Number(radius) || 0), shape),
    );

  const progress = status?.progress ? { ...status.progress, percent: status.progress.percent ?? 0, rate: status.progress.rate ?? 0 } : null;
  const side = Math.max(0, Math.round(Number(radius) || 0)) * 2;
  const chunks = shape === "circle" ? Math.round(Math.PI * (side / 32) ** 2) : Math.round((side / 16) ** 2);
  const stateLabel: Record<string, string> = {
    running: tr("Generating"),
    finished: tr("Finished"),
    paused: tr("Paused"),
    cancelled: tr("Cancelled"),
  };

  return createPortal(
    <div className="modal-backdrop roomy" onClick={() => !busy && onClose()}>
      <div className="add-modal pregen-dialog" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
        <div className="tool-top">
          <p>
            <strong>{tr("Pre-generate chunks")}</strong> · {server.name}
          </p>
          <div className="modal-top-actions">
            <button className="close in-flow" aria-label={tr("Close")} disabled={busy} onClick={onClose}>
              <Icon name="close" />
            </button>
          </div>
        </div>
        <p className="muted trim-lead">
          {tr("Makes the world ahead of time with Chunky, so players don't wait for new land and the server stays smooth while exploring.")}
        </p>
        {error && <p className="modal-error">{error}</p>}
        {!status ? (
          <p className="muted tool-empty">{tr("Loading…")}</p>
        ) : !status.installed ? (
          <div className="pregen-install">
            <p>{tr("Chunky isn't installed on this server.")}</p>
            <button className="button primary" disabled={busy} onClick={() => void install()}>
              <Icon name="import" />
              {tr("Install Chunky")}
            </button>
          </div>
        ) : (
          <>
            {progress && (
              <div className={`pregen-progress state-${progress.state}`}>
                <div className="pregen-progress-head">
                  <strong>{stateLabel[progress.state] ?? progress.state}</strong>
                  <code>{progress.world}</code>
                  <span className="muted">
                    {tr("{0} chunks", [progress.chunks.toLocaleString()])}
                    {progress.state === "running" && progress.eta ? ` · ${tr("{0} left", [progress.eta])}` : ""}
                    {progress.state === "running" && progress.rate ? ` · ${progress.rate.toFixed(0)} cps` : ""}
                  </span>
                  <strong className="pregen-percent">{progress.percent.toFixed(1)}%</strong>
                </div>
                <div className="progress-track">
                  <span style={{ width: `${Math.min(100, progress.percent)}%` }} />
                </div>
                <div className="pregen-progress-actions">
                  {progress.state === "running" && (
                    <button className="button secondary small" disabled={busy || !running} onClick={() => void run("pause")}>
                      {tr("Pause")}
                    </button>
                  )}
                  {progress.state === "paused" && (
                    <button className="button secondary small" disabled={busy || !running} onClick={() => void run("continue")}>
                      {tr("Continue")}
                    </button>
                  )}
                  {(progress.state === "running" || progress.state === "paused") && (
                    <button className="button ghost small" disabled={busy || !running} onClick={() => void run("cancel")}>
                      {tr("Cancel")}
                    </button>
                  )}
                </div>
              </div>
            )}
            <div className="pregen-fields">
              <label>
                <span>{tr("Dimension")}</span>
                <Dropdown
                  value={String(realm)}
                  options={[tr("Overworld"), tr("Nether"), tr("End")].map((label, index) => ({ value: String(index), label, hint: status.worlds[index] }))}
                  onChange={(value) => setRealm(Number(value))}
                />
              </label>
              <label>
                <span>{tr("Center X")}</span>
                <input type="number" value={center?.x ?? "0"} onChange={(event) => setCenter({ x: event.target.value, z: center?.z ?? "0" })} />
              </label>
              <label>
                <span>{tr("Center Z")}</span>
                <input type="number" value={center?.z ?? "0"} onChange={(event) => setCenter({ x: center?.x ?? "0", z: event.target.value })} />
              </label>
              <label>
                <span>{tr("Radius (blocks)")}</span>
                <input type="number" min={1} max={100000} value={radius} onChange={(event) => setRadius(event.target.value)} />
              </label>
              <div className="pregen-shape">
                <span>{tr("Shape")}</span>
                <SegmentedControl
                  className="page-tabs"
                  value={shape}
                  onChange={setShape}
                  options={[
                    { value: "square", label: tr("Square") },
                    { value: "circle", label: tr("Circle") },
                  ]}
                />
              </div>
            </div>
            <p className="muted pregen-estimate">
              {tr("About {0} chunks ({1} × {1} blocks). Existing chunks are skipped.", [chunks.toLocaleString(), side.toLocaleString()])}
            </p>
            <div className="trim-actions">
              {!running && <span className="muted">{tr("Start the server first. Chunky runs inside the server.")}</span>}
              <button
                className="button primary"
                disabled={busy || !running || progress?.state === "running"}
                onClick={() => void run("start")}
              >
                <Icon name="play" />
                {tr("Start generating")}
              </button>
            </div>
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}

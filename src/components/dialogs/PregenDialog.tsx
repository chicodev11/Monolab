import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { commands } from "../../lib/ipc";
import { tr, trBack } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import { Dropdown } from "../ui/Dropdown";
import { SegmentedControl } from "../primitives";

type Props = {
  server: { id: string; name: string; status: string; core: string };
  realm?: number;
  area?: { x: number; z: number; rx: number; rz: number } | null;
  onClose: () => void;
  onNotice: (message: string) => void;
};

const TOOLS = [
  { id: "chunky", project: "fALzjamp", name: "Chunky" },
  { id: "chunksmith", project: "4BeAEBIb", name: "Chunksmith" },
] as const;

type ChunkyStatus = Awaited<ReturnType<typeof commands.chunkyStatus>>;

export function PregenDialog({ server, realm: startRealm = 0, area = null, onClose, onNotice }: Props) {
  const [status, setStatus] = useState<ChunkyStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [realm, setRealm] = useState(startRealm);
  const [center, setCenter] = useState<{ x: string; z: string } | null>(area ? { x: String(area.x), z: String(area.z) } : null);
  const [radius, setRadius] = useState(area ? String(area.rx) : "2000");
  const [radiusZ, setRadiusZ] = useState(area ? String(area.rz) : "2000");
  const [shape, setShape] = useState<"square" | "circle" | "rectangle">(area ? (area.rx === area.rz ? "square" : "rectangle") : "square");
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

  const install = (tool: (typeof TOOLS)[number]) =>
    act(
      () => commands.installModrinth(server.id, tool.project, status?.folder === "plugins" ? "plugin" : "mod", null),
      running ? tr("Installed {0}. Restart the server to use it.", [tool.name]) : tr("Installed {0}.", [tool.name]),
    );
  const offered = TOOLS.filter((tool) => !(tool.id === "chunksmith" && server.core === "Folia"));
  const toolName = TOOLS.find((tool) => tool.id === status?.tool)?.name ?? "";

  const world = status?.worlds[realm] ?? "";
  const run = (action: string) =>
    act(() =>
      commands.chunkyRun(server.id, action, world, Math.round(Number(center?.x) || 0), Math.round(Number(center?.z) || 0), Math.round(Number(radius) || 0), shape === "rectangle" ? Math.round(Number(radiusZ) || 0) : 0, shape),
    );

  const progress = status?.progress ? { ...status.progress, percent: status.progress.percent ?? 0, rate: status.progress.rate ?? 0 } : null;
  const side = Math.max(0, Math.round(Number(radius) || 0)) * 2;
  const sideZ = shape === "rectangle" ? Math.max(0, Math.round(Number(radiusZ) || 0)) * 2 : side;
  const chunks = shape === "circle" ? Math.round(Math.PI * (side / 32) ** 2) : Math.round((side / 16) * (sideZ / 16));
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
          {tr("Makes the world ahead of time, so players don't wait for new land and the server stays smooth while exploring.")}
          {toolName && <> {tr("Using {0}.", [toolName])}</>}
        </p>
        {error && <p className="modal-error">{error}</p>}
        {!status ? (
          <p className="muted tool-empty">{tr("Loading…")}</p>
        ) : !status.installed ? (
          <div className="pregen-install">
            <p>{tr("Install a pre-generator on this server first.")}</p>
            <div className="pregen-tools">
              {offered.map((tool) => (
                <div key={tool.id} className="pregen-tool">
                  <strong>{tool.name}</strong>
                  <small className="muted">
                    {tool.id === "chunky"
                      ? tr("The well-known pre-generator. Simple and light.")
                      : tr("Based on Chunky. Slows itself down while players are online, and builds Distant Horizons / voxy distant terrain as it goes.")}
                  </small>
                  <button className="button secondary small" disabled={busy} onClick={() => void install(tool)}>
                    <Icon name="import" />
                    {tr("Install {0}", [tool.name])}
                  </button>
                </div>
              ))}
            </div>
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
                <span>{shape === "rectangle" ? tr("Radius X") : tr("Radius (blocks)")}</span>
                <input type="number" min={1} max={100000} value={radius} onChange={(event) => setRadius(event.target.value)} />
              </label>
              {shape === "rectangle" && (
                <label>
                  <span>{tr("Radius Z")}</span>
                  <input type="number" min={1} max={100000} value={radiusZ} onChange={(event) => setRadiusZ(event.target.value)} />
                </label>
              )}
              <div className="pregen-shape">
                <span>{tr("Shape")}</span>
                <SegmentedControl
                  className="page-tabs"
                  value={shape}
                  onChange={setShape}
                  options={[
                    { value: "square", label: tr("Square") },
                    { value: "circle", label: tr("Circle") },
                    { value: "rectangle", label: tr("Rectangle") },
                  ]}
                />
              </div>
            </div>
            <p className="muted pregen-estimate">
              {tr("About {0} chunks ({1} × {2} blocks). Existing chunks are skipped.", [chunks.toLocaleString(), side.toLocaleString(), sideZ.toLocaleString()])}
            </p>
            <div className="trim-actions">
              {!running && <span className="muted">{tr("Start the server first. The pre-generator runs inside the server.")}</span>}
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

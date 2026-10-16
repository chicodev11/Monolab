import { tr } from "../../lib/i18n";
import { useEffect } from "react";
import { Icon } from "../ui/Icon";

export type DependencyInfo = {
  projectId: string;
  title: string;
  iconUrl?: string | null;
  versionName: string;
  required: boolean;
  alreadyInstalled: boolean;
  fileName: string;
};

export function DependencyDialog({
  title,
  dependencies,
  chosen,
  busy,
  onToggle,
  onCancel,
  onConfirm,
}: {
  title: string;
  dependencies: DependencyInfo[];
  chosen: string[];
  busy: boolean;
  onToggle: (projectId: string) => void;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) onCancel();
    };
    document.addEventListener("keydown", escape);
    return () => document.removeEventListener("keydown", escape);
  }, [busy, onCancel]);

  const pending = dependencies.filter((one) => !one.alreadyInstalled);
  const required = pending.filter((one) => one.required);
  const optional = pending.filter((one) => !one.required);
  const already = dependencies.filter((one) => one.alreadyInstalled);
  const picked = required.length + optional.filter((one) => chosen.includes(one.projectId)).length;

  const icon = (one: DependencyInfo) =>
    one.iconUrl ? (
      <img src={one.iconUrl} alt="" className="dep-card-icon" />
    ) : (
      <span className="dep-card-icon empty" aria-hidden>
        <Icon name="archive" />
      </span>
    );

  return (
    <div
      className="modal-backdrop dep-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onCancel();
      }}
    >
      <div className="dep-dialog" role="dialog" aria-modal="true">
        <div className="dep-head">
          <h2>{tr("Dependencies required")}</h2>
          <button className="close in-flow" aria-label={tr("Close")} data-tip={tr("Close")} disabled={busy} onClick={onCancel}>
            <Icon name="close" />
          </button>
        </div>

        <p className="dep-lead">
          <b>{title}</b>{tr(" does not work on its own. The following will be installed too.")}</p>

        {required.length > 0 && (
          <section className="dep-group">
            <h4>{tr("Required")} <span>{required.length}</span></h4>
            <div className="dep-cards">
              {required.map((one) => (
                <div className="dep-card fixed" key={one.projectId}>
                  {icon(one)}
                  <span className="dep-card-name">
                    <strong>{one.title}</strong>
                    <small>{one.versionName || one.fileName}</small>
                  </span>
                  <span className="dep-card-mark" aria-hidden><Icon name="check" /></span>
                </div>
              ))}
            </div>
          </section>
        )}

        {optional.length > 0 && (
          <section className="dep-group">
            <h4>{tr("Optional")} <span>{optional.length}</span> <small>{tr("Select")}</small></h4>
            <div className="dep-cards">
              {optional.map((one) => {
                const on = chosen.includes(one.projectId);
                return (
                  <button
                    type="button"
                    className={`dep-card pick ${on ? "on" : ""}`}
                    key={one.projectId}
                    disabled={busy}
                    aria-pressed={on}
                    onClick={() => onToggle(one.projectId)}
                  >
                    {icon(one)}
                    <span className="dep-card-name">
                      <strong>{one.title}</strong>
                      <small>{one.versionName || one.fileName}</small>
                    </span>
                    <span className="dep-card-mark" aria-hidden>{on && <Icon name="check" />}</span>
                  </button>
                );
              })}
            </div>
          </section>
        )}

        {already.length > 0 && (
          <section className="dep-group">
            <h4>{tr("Already included")} <span>{already.length}</span></h4>
            <div className="dep-cards">
              {already.map((one) => (
                <div className="dep-card done" key={one.projectId}>
                  {icon(one)}
                  <span className="dep-card-name">
                    <strong>{one.title}</strong>
                    <small>{one.versionName || one.fileName}</small>
                  </span>
                  <span className="dep-card-mark" aria-hidden><Icon name="check" /></span>
                </div>
              ))}
            </div>
          </section>
        )}

        <div className="modal-actions">
          <button className="button primary" disabled={busy} onClick={onConfirm}>
            <Icon name="import" />
            {busy ? tr("Installing…") : tr("Install {0}", [picked + 1])}
          </button>
        </div>
      </div>
    </div>
  );
}

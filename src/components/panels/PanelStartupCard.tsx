import { useCallback, useEffect, useState } from "react";
import type { PanelStartup } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Icon } from "../ui/Icon";

export function PanelStartupCard({
  id,
  panelName,
  onError,
  onNotice,
}: {
  id: string;
  panelName: string;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}) {
  const [startup, setStartup] = useState<PanelStartup | null>(null);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState("");

  const load = useCallback(() => {
    commands
      .panelStartup(id)
      .then((got) => {
        setStartup(got);
        setDraft(Object.fromEntries(got.variables.map((one) => [one.env, one.value])));
      })
      .catch((cause) => onError(String(cause)));
  }, [id, onError]);
  useEffect(() => load(), [load]);

  async function save(env: string) {
    setBusy(env);
    try {
      await commands.setPanelVariable(id, env, draft[env] ?? "");
      onNotice(tr("Changed {0}. Applies on the next start.", [env]));
      load();
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy("");
    }
  }

  if (!startup) {
    return <div className="setting-card panel-startup"><div className="skeleton-row" /></div>;
  }
  return (
    <div className="setting-card panel-startup">
      <span className="setting-title">
        {tr("Startup")}
        <small className="panel-startup-from">
          <Icon name="cloud" />
          {tr("Set by {0}", [panelName])}
        </small>
      </span>
      <p className="panel-startup-note">
        {tr("The panel controls Java and memory through eggs and server limits. Edit the variables allowed by the egg here. Changes apply on the next start.")}
      </p>
      {startup.command && (
        <div className="panel-startup-command">
          <span>{tr("Command that will run")}</span>
          <code title={startup.command}>{startup.command}</code>
        </div>
      )}
      {startup.dockerImage && (
        <div className="panel-startup-command">
          <span>{tr("Docker image")}</span>
          <code>{startup.dockerImage}</code>
        </div>
      )}
      <div className="panel-startup-vars">
        {startup.variables.map((one) => {
          const changed = (draft[one.env] ?? "") !== one.value;
          return (
            <label key={one.env} className="panel-var" title={one.description || undefined}>
              <span>
                <strong>{one.name || one.env}</strong>
                <code>{one.env}</code>
              </span>
              <span className="panel-var-input">
                <input
                  value={draft[one.env] ?? ""}
                  readOnly={!one.editable}
                  placeholder={one.defaultValue}
                  spellCheck={false}
                  onChange={(event) => setDraft({ ...draft, [one.env]: event.target.value })}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && one.editable && changed) void save(one.env);
                  }}
                />
                {!one.editable ? (
                  <em>{tr("Locked")}</em>
                ) : changed ? (
                  <button
                    type="button"
                    className="button primary small"
                    disabled={busy === one.env}
                    onClick={() => void save(one.env)}
                  >
                    {tr("Save")}
                  </button>
                ) : null}
              </span>
            </label>
          );
        })}
        {!startup.variables.length && <p className="muted">{tr("This egg has no startup variables to change.")}</p>}
      </div>
    </div>
  );
}

import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { Icon } from "../ui/Icon";
import { commands, type AddonStatus } from "../../lib/bindings";
import { tr } from "../../lib/i18n";

type Switch = Awaited<ReturnType<typeof commands.addonConfig>>[number];

const LABELS: Record<string, [() => string, () => string]> = {
  map: [() => tr("World map"), () => tr("Tiles drawn from saved region files")],
  "live-map": [() => tr("Live map"), () => tr("Updates around players every tick (up to 0.6 ms)")],
  "web-map": [() => tr("Web map"), () => tr("Map page for friends on its own port")],
  inventories: [() => tr("Live inventories"), () => tr("What online players carry, every 5 seconds")],
  "loaded-chunks": [() => tr("Loaded chunks"), () => tr("Shown on the map, every 5 seconds")],
  commands: [() => tr("Command list"), () => tr("Console autocomplete")],
  "performance-log": [() => tr("Performance history"), () => tr("TPS and memory once a minute")],
  "jvm-stats": [() => tr("JVM details"), () => tr("Garbage collection, memory areas and tick times")],
};

/** Feature switches of the Monolab add-on, kept in its config.yml. */
export function AddonSettings({ serverId, serverName, status, onUpdate, onError, onClose }: {
  serverId: string;
  serverName: string;
  status: AddonStatus;
  onUpdate: () => Promise<void>;
  onError: (message: string) => void;
  onClose: () => void;
}) {
  const [updating, setUpdating] = useState(false);
  const [switches, setSwitches] = useState<Switch[] | null>(null);
  useEffect(() => {
    commands.addonConfig(serverId).then(setSwitches).catch((cause) => onError(String(cause)));
  }, [serverId, onError]);

  async function flip(key: string, on: boolean) {
    setSwitches((now) => now?.map((one) => (one.key === key ? { ...one, on } : one)) ?? now);
    try {
      await commands.setAddonConfig(serverId, key, on);
    } catch (cause) {
      setSwitches((now) => now?.map((one) => (one.key === key ? { ...one, on: !on } : one)) ?? now);
      onError(String(cause));
    }
  }

  return createPortal(
    <div className="modal-backdrop roomy" onClick={onClose}>
      <div className="add-modal addon-settings" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
        <div className="tool-top">
          <p>
            <strong>{tr("Monolab add-on")}</strong> · {serverName}
          </p>
          <div className="modal-top-actions">
            <button className="close in-flow" aria-label={tr("Close")} onClick={onClose}>
              <Icon name="close" />
            </button>
          </div>
        </div>
        <div className="addon-version">
          <div>
            <strong>{tr("Version {0}", [status.installedVersion ?? "—"])}</strong>
            <small>
              {status.needsUpdate
                ? status.running
                  ? tr("Version {0} is ready. Stop the server to update.", [status.latestVersion])
                  : tr("Version {0} is ready.", [status.latestVersion])
                : tr("Up to date")}
            </small>
          </div>
          {status.needsUpdate && (
            <button
              className="button primary small"
              disabled={status.running || updating}
              onClick={async () => {
                setUpdating(true);
                await onUpdate();
                setUpdating(false);
              }}
            >
              {updating ? tr("Updating…") : tr("Update")}
            </button>
          )}
        </div>
        <p className="muted adv-note">{tr("Saved to the add-on's config.yml. Most changes apply within a few seconds.")}</p>
      <div className="integ-settings">
        {switches === null && <p className="muted adv-note">{tr("Loading…")}</p>}
        {switches?.map((one) => {
          const [label, hint] = LABELS[one.key] ?? [() => one.key, () => ""];
          return (
            <div className="setting-row" key={one.key}>
              <div>
                <strong>{label()}</strong>
                <small>{one.restart ? `${hint()} · ${tr("applies after a restart")}` : hint()}</small>
              </div>
              <label className="switch">
                <input type="checkbox" checked={one.on} onChange={(event) => void flip(one.key, event.target.checked)} />
                <span />
              </label>
            </div>
          );
        })}
      </div>
      </div>
    </div>,
    document.body,
  );
}

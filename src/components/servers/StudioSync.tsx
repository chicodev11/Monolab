import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import type { StudioSyncTarget } from "../../lib/bindings";
import { tr, trBack } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { List, ListRow, Switch } from "../primitives";
import { Icon } from "../ui/Icon";
import "./StudioBundles.css";

/**
 * Copies this server's whole studio (files, assets, pack numbers) to other servers, such as the others behind the
 * same Velocity proxy, so a network has the same items, blocks and pack everywhere.
 */
export function StudioSync({ serverId, onClose, onNotice }: { serverId: string; onClose: () => void; onNotice: (message: string) => void }) {
  const [targets, setTargets] = useState<StudioSyncTarget[] | null>(null);
  const [failed, setFailed] = useState("");
  const [picked, setPicked] = useState<string[]>([]);
  const [mirror, setMirror] = useState(false);
  const [busy, setBusy] = useState(false);
  const [lines, setLines] = useState<string[]>([]);

  useEffect(() => {
    let live = true;
    commands.studioSyncTargets(serverId).then(
      (list) => {
        if (!live) return;
        setTargets(list);
        setPicked(list.filter((one) => one.network).map((one) => one.id));
      },
      (cause) => live && setFailed(trBack(String(cause))),
    );
    return () => {
      live = false;
    };
  }, [serverId]);

  async function run() {
    setBusy(true);
    setLines([]);
    try {
      const said = await commands.studioSync(serverId, picked, mirror);
      setLines(said);
      onNotice(tr("Copied the studio to {0, plural, one {# server} other {# servers}}.", [picked.length]));
    } catch (cause) {
      setLines([trBack(String(cause))]);
    } finally {
      setBusy(false);
    }
  }

  const close = () => !busy && onClose();
  const flip = (id: string, on: boolean) => setPicked((now) => (on ? [...now, id] : now.filter((one) => one !== id)));

  return createPortal(
    <div className="modal-backdrop roomy" onClick={close}>
      <div className="add-modal studio-bundle-dialog" role="dialog" aria-modal="true" aria-label={tr("Copy to other servers")} onClick={(event) => event.stopPropagation()}>
        <div className="tool-top">
          <p>
            <strong>{tr("Copy to other servers")}</strong>
          </p>
          <div className="modal-top-actions">
            <button type="button" className="close in-flow" aria-label={tr("Close")} disabled={busy} onClick={close}>
              <Icon name="close" />
            </button>
          </div>
        </div>
        <div className="studio-bundle-body">
          <p className="muted">{tr("Every studio file, asset and pack number from this server goes to the servers you pick, so players find the same items and blocks on each. Servers behind the same Velocity proxy are picked for you. Each server keeps its own variables and logs.")}</p>
          {failed && <p className="studio-held-problem">{failed}</p>}
          {targets === null && !failed && <p className="muted">{tr("Looking…")}</p>}
          {targets !== null && targets.length === 0 && <p className="muted">{tr("No other server here has a studio (Paper, Purpur, Folia, Leaf or Pumpkin).")}</p>}
          {targets !== null && targets.length > 0 && (
            <List variant="box" density="compact" className="check-list">
              {targets.map((one) => (
                <ListRow
                  key={one.id}
                  leading={<input type="checkbox" checked={picked.includes(one.id)} disabled={busy} aria-label={one.name} onChange={(event) => flip(one.id, event.target.checked)} />}
                  onClick={() => flip(one.id, !picked.includes(one.id))}
                  disabled={busy}
                  mainProps={{ tabIndex: -1 }}
                  title={one.name}
                  meta={<>{one.core}{one.network ? ` · ${tr("Same proxy")}` : ""}</>}
                />
              ))}
            </List>
          )}
          <label className="studio-toggle">
            <span>{tr("Also remove what this server doesn't have")}</span>
            <Switch checked={mirror} disabled={busy} onChange={(event) => setMirror(event.target.checked)} />
          </label>
          {lines.length > 0 && (
            <ul className="studio-sync-lines">
              {lines.map((line) => (
                <li key={line}>{trBack(line)}</li>
              ))}
            </ul>
          )}
          <div className="modal-actions">
            <button type="button" className="button secondary" disabled={busy} onClick={close}>
              {lines.length ? tr("Done") : tr("Cancel")}
            </button>
            <button type="button" className="button primary" disabled={busy || picked.length === 0} onClick={() => void run()}>
              {busy ? tr("Copying…") : tr("Copy")}
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}

import { useEffect, useState } from "react";
import { commands } from "../../lib/ipc";
import type { ProxyNetwork as Network } from "../../lib/bindings";
import { tr, trBack } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import { DropdownMenu } from "../primitives";

function stateText(state: string) {
  if (state === "Setup") return tr("Set up when applied");
  if (state === "NeedsMod") return tr("Needs FabricProxy-Lite");
  if (state === "StartFirst") return tr("Start it once first");
  return tr("Not supported");
}

/** The servers behind a Velocity proxy; adding or removing one sets up velocity.toml, the secret and that server's config. */
export function ProxyNetwork({ serverId, onNotice }: { serverId: string; onNotice: (message: string) => void }) {
  const [network, setNetwork] = useState<Network | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [notes, setNotes] = useState<string[]>([]);
  const [round, setRound] = useState(0);

  useEffect(() => {
    let gone = false;
    setFailed(null);
    void commands.proxyNetwork(serverId).then(
      (next) => !gone && setNetwork(next),
      (cause) => !gone && setFailed(trBack(String(cause))),
    );
    return () => {
      gone = true;
    };
  }, [serverId, round]);

  const linked = network?.backends.filter((one) => one.linked) ?? [];
  const others = network?.backends.filter((one) => !one.linked) ?? [];

  async function save(servers: string[], first: string) {
    setBusy(true);
    setNotes([]);
    try {
      const said = await commands.linkProxy(serverId, servers, first);
      setNotes(said.map(trBack));
      onNotice(tr("Saved the proxy network."));
    } catch (cause) {
      setNotes([trBack(String(cause))]);
    } finally {
      setBusy(false);
      setRound((at) => at + 1);
    }
  }

  const ids = linked.map((one) => one.id);
  const first = network?.first || ids[0] || "";

  return (
    <div className="setting-card proxy-net">
      <div className="proxy-net-head">
        <span className="setting-title">{tr("Servers behind this proxy")}</span>
        {network && (
          <DropdownMenu
            trigger={
              <button className="button secondary small" disabled={busy || others.length === 0}>
                <Icon name="plus" />
                {tr("Add")}
              </button>
            }
            items={others.map((one) => ({
              label: one.state === "Ready" || one.state === "Setup" ? one.name : `${one.name} · ${stateText(one.state)}`,
              icon: "server",
              onSelect: () => void save([...ids, one.id], first || one.id),
            }))}
          />
        )}
      </div>
      {failed ? (
        <p className="muted proxy-net-lead">{failed}</p>
      ) : !network ? (
        <p className="muted proxy-net-lead">{tr("Loading…")}</p>
      ) : linked.length === 0 ? (
        <p className="muted proxy-net-lead">{tr("No servers linked yet. Players join the proxy and move between the servers you link.")}</p>
      ) : (
        <div className="proxy-net-list">
          {linked.map((one) => (
            <div className="setting-row proxy-net-row" key={one.id}>
              <div>
                <strong>{one.name}</strong>
                <small>
                  {one.core} · <code>{one.address}</code>
                </small>
              </div>
              <span className="proxy-net-tags">
                {one.id === first && <span className="chip subtle">{tr("joins first")}</span>}
                <span className={`chip subtle proxy-state ${one.state === "Ready" && network.modern ? "ok" : ""}`}>
                  {one.state === "Ready" && network.modern ? tr("Linked") : stateText(one.state)}
                </span>
              </span>
              <span className="proxy-net-actions">
                {one.id !== first && (
                  <button className="button secondary small" disabled={busy} onClick={() => void save(ids, one.id)}>
                    {tr("Join here first")}
                  </button>
                )}
                <button
                  className="icon-button"
                  aria-label={tr("Unlink {0}", [one.name])}
                  data-tip={tr("Unlink")}
                  disabled={busy}
                  onClick={() => {
                    const rest = ids.filter((id) => id !== one.id);
                    void save(rest, one.id === first ? rest[0] ?? "" : first);
                  }}
                >
                  <Icon name="close" />
                </button>
              </span>
            </div>
          ))}
        </div>
      )}
      {notes.length > 0 && (
        <ul className="proxy-net-notes">
          {notes.map((one) => (
            <li key={one}>{one}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

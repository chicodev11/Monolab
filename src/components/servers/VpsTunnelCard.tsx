import { useEffect, useState } from "react";
import { commands, type VpsTunnel } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";

type Host = { id: string; name: string; address: string };

/**
 * Network tab: open the server to players through a port on one of your own SSH servers (a VPS), like `ssh -R`.
 * Players join `<VPS address>:<port>` and never see your home IP. Runs while the app is open.
 */
export function VpsTunnelCard({
  serverId,
  serverPort,
  hosts,
  onNotice,
  onError,
}: {
  serverId: string;
  serverPort: number;
  hosts: Host[];
  onNotice: (message: string) => void;
  onError: (message: string) => void;
}) {
  const [state, setState] = useState<VpsTunnel | null>(null);
  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let gone = false;
    let first = true;
    const read = () =>
      void commands.vpsTunnelState(serverId).then((now) => {
        if (gone) return;
        setState(now);
        if (first) {
          first = false;
          setHost(now.host || hosts[0]?.id || "");
          setPort(String(now.port || serverPort));
        }
      }, () => {});
    read();
    const timer = window.setInterval(read, 3000);
    return () => {
      gone = true;
      window.clearInterval(timer);
    };
  }, [serverId, serverPort, hosts]);

  async function start() {
    const wanted = Number(port);
    if (!host || !Number.isInteger(wanted) || wanted < 1024 || wanted > 65535) {
      onError(tr("Pick an SSH server and a port from 1024 to 65535."));
      return;
    }
    setBusy(true);
    try {
      setState(await commands.vpsTunnelStart(serverId, host, wanted));
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    setBusy(true);
    try {
      setState(await commands.vpsTunnelStop(serverId, false));
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy(false);
    }
  }

  const running = !!state?.running;
  return (
    <div className="setting-card vps-tunnel">
      <span className="setting-title reach-card-title">
        <span className="reach-icon">
          <Icon name="server" />
        </span>
        {tr("Through your own VPS")}
      </span>
      <p className="reach-note">
        {tr("Players join your VPS's address and the traffic comes here over SSH, so your home IP stays hidden. Works while this app is open.")}
      </p>
      {hosts.length === 0 ? (
        <small className="muted">{tr("Add an SSH server first (Servers → SSH servers). Any small Linux VPS works.")}</small>
      ) : running ? (
        <div className="vps-tunnel-row">
          <button
            type="button"
            className="reach-address"
            disabled={!state?.connected}
            data-tip={tr("Click to copy")}
            onClick={() => {
              if (!state) return;
              void navigator.clipboard.writeText(state.address);
              onNotice(tr("Address copied!"));
            }}
          >
            <code>{state?.address}</code>
            <Icon name="copy" />
          </button>
          <small className={state?.error ? "warn" : "muted"}>{state?.connected ? tr("Connected") : state?.error || tr("Connecting…")}</small>
          <button type="button" className="button secondary small" disabled={busy} onClick={() => void stop()}>
            <Icon name="stop" />
            {tr("Stop")}
          </button>
        </div>
      ) : (
        <div className="vps-tunnel-row">
          <Dropdown value={host} onChange={setHost} options={hosts.map((one) => ({ value: one.id, label: one.name, hint: one.address }))} />
          <label className="vps-tunnel-port">
            <span>{tr("Port on the VPS")}</span>
            <input type="number" min={1024} max={65535} value={port} onChange={(event) => setPort(event.target.value)} />
          </label>
          <button type="button" className="button secondary small" disabled={busy} onClick={() => void start()}>
            <Icon name="cloud" />
            {busy ? tr("One moment…") : tr("Start")}
          </button>
        </div>
      )}
      {!running && state?.error && <small className="warn">{state.error}</small>}
      <small className="muted">
        {tr("The VPS needs \"GatewayPorts yes\" (or clientspecified) in sshd_config and the port open in its firewall. Java players only (TCP).")}
      </small>
    </div>
  );
}

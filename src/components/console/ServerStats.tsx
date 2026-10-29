import { useEffect, useRef, useState } from "react";
import { commands } from "../../lib/ipc";
import type { LivePlayer, PanelStats } from "../../lib/bindings";
import { useHead } from "../../lib/heads";
import { fileSize } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { period, useAttention } from "../../lib/polling";
import { Icon } from "../ui/Icon";

type Stats = {
  cpu: number | null;
  memory: number;
  memoryLimit: number;
  disk: number | null;
  diskLimit: number | null;
  rx: number | null;
  tx: number | null;
  uptime: number;
  tps: number | null;
  mspt: number | null;
  players: number | null;
  entities: number | null;
  chunks: number | null;
  roster: LivePlayer[] | null;
};

type Server = {
  id: string;
  name: string;
  panelHost?: string;
  maxMemoryMib: number;
  port: number;
  status: string;
  uptimeSeconds?: number | null;
};

const KEEP = 48;

function uptimeText(ms: number) {
  const minutes = Math.floor(ms / 60000);
  if (minutes < 60) return tr("{0} min", [minutes]);
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return tr("{0} h {1} min", [hours, minutes % 60]);
  return tr("{0} d {1} h", [Math.floor(hours / 24), hours % 24]);
}

function fromPanel(stats: PanelStats): Partial<Stats> {
  return {
    cpu: stats.cpu,
    memory: stats.memoryBytes ?? 0,
    memoryLimit: stats.memoryLimitBytes ?? 0,
    disk: stats.diskBytes ?? 0,
    rx: stats.rxBytes ?? 0,
    tx: stats.txBytes ?? 0,
    uptime: stats.uptimeMs ?? 0,
  };
}

/** A small filled line of the last readings, drawn with plain SVG. */
function Trace({ values, zero, tone }: { values: number[]; zero?: boolean; tone?: string }) {
  if (values.length < 2) return <svg className="stat-trace" viewBox="0 0 100 28" preserveAspectRatio="none" aria-hidden="true" />;
  const high = Math.max(...values);
  const low = zero ? 0 : Math.min(...values);
  const span = Math.max(high - low, high * 0.08, 1);
  const step = 100 / (Math.min(values.length, KEEP) - 1);
  const points = values.map((value, index) => `${(index * step).toFixed(2)},${(27 - ((value - low) / span) * 24).toFixed(2)}`);
  const line = `M${points.join(" L")}`;
  return (
    <svg className={`stat-trace ${tone ?? ""}`} viewBox="0 0 100 28" preserveAspectRatio="none" aria-hidden="true">
      <path className="area" d={`${line} L100,28 L0,28 Z`} />
      <path className="line" d={line} vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

function Card({
  label,
  value,
  note,
  ratio,
  trace,
  zero,
  tone,
  plain,
  icon,
  delay,
  onOpen,
}: {
  label: string;
  value: string;
  note?: string;
  ratio?: number;
  trace?: number[];
  zero?: boolean;
  tone?: "warn" | "danger";
  plain?: boolean;
  icon: string;
  delay: number;
  onOpen?: () => void;
}) {
  const level = tone ?? (plain || ratio === undefined ? undefined : ratio >= 0.9 ? "danger" : ratio >= 0.75 ? "warn" : undefined);
  return (
    <div
      className={`stat-card${level ? ` ${level}` : ""}${onOpen ? " link" : ""}`}
      style={{ animationDelay: `${delay * 40}ms` }}
      role={onOpen ? "button" : undefined}
      tabIndex={onOpen ? 0 : undefined}
      onClick={onOpen}
      onKeyDown={onOpen ? (event) => (event.key === "Enter" || event.key === " ") && onOpen() : undefined}
    >
      <div className="stat-head">
        <Icon name={icon} />
        <span>{label}</span>
      </div>
      <strong className="stat-value">{value}</strong>
      {note && <small className="stat-note">{note}</small>}
      {ratio !== undefined && (
        <span className="stat-bar">
          <i style={{ width: `${Math.min(100, Math.max(0, ratio * 100))}%` }} />
        </span>
      )}
      {trace && <Trace values={trace} zero={zero} tone={level} />}
    </div>
  );
}

function statusText(status: string) {
  switch (status) {
    case "running":
      return tr("Running");
    case "starting":
      return tr("Starting");
    case "stopping":
      return tr("Stopping");
    case "crashed":
      return tr("Crashed");
    default:
      return tr("Stopped");
  }
}

function PlayerRow({ player, onOpen }: { player: LivePlayer; onOpen: (name: string) => void }) {
  const head = useHead(player.name);
  return (
    <button type="button" className="stat-player" onClick={() => onOpen(player.name)}>
      {head ? <img src={head} alt="" /> : <span className="stat-player-face">{player.name.slice(0, 1).toUpperCase()}</span>}
      <strong>{player.name}</strong>
      <small>{player.ping} ms</small>
    </button>
  );
}

export function ServerStats({ server, onOpenPlayer, onOpenTool, bare }: { server: Server; onOpenPlayer: (name: string) => void; onOpenTool?: (tool: "monitor" | "players" | "disk") => void; bare?: boolean }) {
  const toMonitor = onOpenTool ? () => onOpenTool("monitor") : undefined;
  const [stats, setStats] = useState<Stats | null>(null);
  const [cpuTrace, setCpuTrace] = useState<number[]>([]);
  const [copied, setCopied] = useState(false);
  const attention = useAttention();
  const running = server.status === "running";
  const panel = Boolean(server.panelHost);
  const last = useRef<string>("");

  useEffect(() => {
    if (last.current !== server.id) {
      last.current = server.id;
      setCpuTrace([]);
    }
    if (!running) {
      setStats(null);
      return;
    }
    let gone = false;
    let timer = 0;
    const read = async () => {
      try {
        let next: Stats;
        const blank = { cpu: null, memory: 0, memoryLimit: server.maxMemoryMib * 1048576, disk: null, diskLimit: null, rx: null, tx: null, uptime: 0, tps: null, mspt: null, players: null, entities: null, chunks: null, roster: null };
        if (panel) {
          const got = await commands.panelUsage(server.id);
          next = { ...blank, ...(got ? fromPanel(got) : {}) };
        } else {
          const [resource, tps, live] = await Promise.allSettled([
            commands.getResourceSnapshot(server.id),
            commands.getTpsSnapshot(server.id),
            commands.readLive(server.id),
          ]);
          const snap = resource.status === "fulfilled" ? resource.value : null;
          const speed = tps.status === "fulfilled" ? tps.value : null;
          const now = live.status === "fulfilled" ? live.value : null;
          next = {
            ...blank,
            cpu: snap?.cpuPercent ?? null,
            memory: (now?.memoryMib ?? snap?.memoryMib ?? 0) * 1048576,
            disk: snap ? (snap.diskTotalMib - snap.diskFreeMib) * 1048576 : null,
            diskLimit: snap ? snap.diskTotalMib * 1048576 : null,
            tps: now?.tps ?? speed?.tps ?? null,
            mspt: now?.mspt ?? speed?.mspt ?? null,
            players: now?.players?.length ?? null,
            entities: now?.entities ?? null,
            chunks: now?.chunks ?? null,
            roster: now?.players ?? null,
          };
        }
        if (!gone) {
          setStats(next);
          setCpuTrace((list) => [...list, Math.max(0, next.cpu ?? 0)].slice(-KEEP));
        }
      } catch {
        // Keep the last numbers; the next round tries again.
      }
      const wait = period(panel ? 2000 : 1500, attention);
      if (!gone && wait) timer = window.setTimeout(() => void read(), wait);
    };
    void read();
    return () => {
      gone = true;
      window.clearTimeout(timer);
    };
  }, [attention, panel, running, server.id, server.maxMemoryMib]);

  const address = `localhost:${server.port}`;
  const copy = async () => {
    await navigator.clipboard.writeText(address).catch(() => {});
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  };

  const memoryRatio = stats && stats.memoryLimit > 0 ? stats.memory / stats.memoryLimit : undefined;
  const cards: (Parameters<typeof Card>[0] | null)[] = stats
    ? [
        { icon: "cpu", label: tr("CPU load"), value: stats.cpu == null ? "—" : `${stats.cpu.toFixed(1)}%`, trace: cpuTrace, zero: true, delay: 0, onOpen: toMonitor },
        {
          icon: "memory",
          label: tr("Memory"),
          value: stats.memoryLimit > 0 ? `${fileSize(stats.memory)} / ${fileSize(stats.memoryLimit)}` : fileSize(stats.memory),
          ratio: memoryRatio,
          delay: 1,
          onOpen: toMonitor,
        },
        stats.tps != null
          ? { icon: "gauge", label: "TPS", value: stats.tps.toFixed(1), note: stats.mspt != null ? `${stats.mspt.toFixed(1)} ms/tick` : undefined, tone: stats.tps < 15 ? "danger" : stats.tps < 18 ? "warn" : undefined, delay: 2, onOpen: toMonitor }
          : null,
        stats.players != null ? { icon: "users", label: tr("Players"), value: String(stats.players), delay: 3, onOpen: onOpenTool ? () => onOpenTool("players") : undefined } : null,
        stats.entities != null
          ? { icon: "ghost", label: tr("Entities"), value: Math.round(stats.entities).toLocaleString(), note: stats.chunks != null ? tr("{0} chunks loaded", [Math.round(stats.chunks).toLocaleString()]) : undefined, delay: 4, onOpen: toMonitor }
          : null,
        stats.disk != null
          ? {
              icon: "disk",
              label: panel ? tr("Disk") : tr("Disk usage"),
              value: fileSize(stats.disk),
              note: !panel && stats.diskLimit ? tr("of {0}", [fileSize(stats.diskLimit)]) : undefined,
              ratio: !panel && stats.diskLimit ? stats.disk / stats.diskLimit : undefined,
              plain: !panel,
              tone: !panel && stats.diskLimit && stats.disk / stats.diskLimit >= 0.95 ? ("danger" as const) : undefined,
              delay: 5,
              onOpen: onOpenTool ? () => onOpenTool(panel ? "monitor" : "disk") : undefined,
            }
          : null,
        panel && stats.rx != null ? { icon: "network", label: tr("Network"), value: `↓ ${fileSize(stats.rx)}`, note: `↑ ${fileSize(stats.tx ?? 0)}`, delay: 6 } : null,
        stats.uptime > 0 || (server.uptimeSeconds ?? 0) > 0
          ? { icon: "clock", label: tr("Uptime"), value: uptimeText(stats.uptime > 0 ? stats.uptime : (server.uptimeSeconds ?? 0) * 1000), delay: 7 }
          : null,
      ]
    : [];

  return (
    <aside className={`server-stats${bare ? " bare" : ""}`} aria-label={tr("Usage")}>
      {bare ? null : (
      <div className={`stat-status ${server.status}`}>
        <span className="stat-dot" />
        <div>
          <strong>{statusText(server.status)}</strong>
          <small>{server.name}</small>
        </div>
        <button className={`stat-copy${copied ? " done" : ""}`} onClick={() => void copy()} data-tip={tr("Copy address")} aria-label={tr("Copy address")}>
          <code>{address}</code>
          <Icon name={copied ? "check" : "copy"} />
        </button>
      </div>
      )}
      {running && stats ? (
        <>
          <div className="stat-grid">
            {cards.map((one) => (one ? <Card key={one.label} {...one} /> : null))}
          </div>
          {stats.roster && (
            <section className="stat-players">
              <div className="stat-head">
                <Icon name="users" />
                <span>{tr("Online players")}</span>
              </div>
              {stats.roster.length ? (
                <div className="stat-player-list">
                  {stats.roster.map((one) => (
                    <PlayerRow key={one.name} player={one} onOpen={onOpenPlayer} />
                  ))}
                </div>
              ) : (
                <p className="stat-players-none">{tr("No one is online.")}</p>
              )}
            </section>
          )}
        </>
      ) : (
        <p className="stat-idle">{running ? tr("Loading…") : tr("Numbers appear while the server runs.")}</p>
      )}
    </aside>
  );
}

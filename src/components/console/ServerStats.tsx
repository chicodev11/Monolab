import { useEffect, useRef, useState, type ReactNode } from "react";
import { commands } from "../../lib/ipc";
import type { LivePlayer, PanelStats } from "../../lib/bindings";
import { useHead } from "../../lib/heads";
import { fileSize } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { period, useAttention } from "../../lib/polling";
import { Icon } from "../ui/Icon";
import { List, ListRow } from "../primitives";

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
    <ListRow
      onClick={() => onOpen(player.name)}
      leading={head ? <img className="stat-player-head" src={head} alt="" /> : <span className="stat-player-face">{player.name.slice(0, 1).toUpperCase()}</span>}
      title={player.name}
      meta={`${player.ping} ms`}
    />
  );
}

/** Short numbers for the side tiles: "3.1" GB, "488" GB, "900" MB. */
function splitSize(bytes: number) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = Math.max(0, bytes);
  let step = 0;
  while (value >= 1024 && step < units.length - 1) {
    value /= 1024;
    step += 1;
  }
  const num = value >= 100 || step < 2 ? Math.round(value).toString() : value.toFixed(1).replace(/\.0$/, "");
  return { num, unit: units[step] };
}

function memoryTile(used: number, limit: number) {
  const now = splitSize(used);
  if (limit <= 0) return { label: tr("Memory"), value: now.num, unit: now.unit };
  const top = splitSize(limit);
  return {
    label: tr("Memory"),
    value: now.num,
    unit: now.unit === top.unit ? `/ ${top.num} ${top.unit}` : `${now.unit} / ${top.num} ${top.unit}`,
    ratio: used / limit,
  };
}

function diskTile(used: number, limit: number | null) {
  const now = splitSize(used);
  if (!limit) return { label: tr("Disk"), value: now.num, unit: now.unit };
  const top = splitSize(limit);
  return {
    label: tr("Disk"),
    value: now.num,
    unit: now.unit === top.unit ? `/ ${top.num} ${top.unit}` : `${now.unit} / ${top.num} ${top.unit}`,
    ratio: used / limit,
    plain: true,
  };
}

function Tile({ label, value, unit, note, ratio, plain, trace, zero }: { label: string; value: string; unit?: string; note?: string; ratio?: number; plain?: boolean; trace?: number[]; zero?: boolean }) {
  const level = ratio === undefined ? undefined : ratio >= (plain ? 0.95 : 0.9) ? "danger" : !plain && ratio >= 0.75 ? "warn" : undefined;
  return (
    <div className={`side-tile${level ? ` ${level}` : ""}`}>
      <span className="side-tile-label">{label}</span>
      <span className="side-tile-value">
        <strong>{value}</strong>
        {unit && <small>{unit}</small>}
      </span>
      {note && <small className="side-tile-note">{note}</small>}
      {trace && trace.length > 1 && <Trace values={trace} zero={zero} tone={level} />}
      {ratio !== undefined && !trace && (
        <span className="side-tile-bar">
          <i style={{ width: `${Math.min(100, Math.max(0, ratio * 100))}%` }} />
        </span>
      )}
    </div>
  );
}

const BUDGET = 50;
const RING = 2 * Math.PI * 80;
const ARC = RING * 0.75;

/** A 270° ring: how much of the 50 ms tick the server uses. */
function TickGauge({ mspt, high, tps }: { mspt: number; high: number | null; tps: number | null }) {
  const ratio = Math.min(1, Math.max(0, mspt / BUDGET));
  const level = mspt >= BUDGET ? "danger" : mspt >= BUDGET * 0.8 ? "warn" : undefined;
  const byTps = tps == null ? undefined : tps < 15 ? "danger" : tps < 18 ? "warn" : undefined;
  const tpsLevel = byTps === "danger" || level === "danger" ? "danger" : byTps ?? level;
  return (
    <div className="tick-gauge">
      <svg viewBox="0 0 200 200" aria-hidden="true">
        <circle className="track" cx="100" cy="100" r="80" strokeDasharray={`${ARC} ${RING}`} transform="rotate(135 100 100)" />
        <circle className="fill" cx="100" cy="100" r="80" strokeDasharray={`${Math.max(ARC * ratio, 0.01)} ${RING}`} transform="rotate(135 100 100)" />
      </svg>
      <div className="tick-gauge-core">
        <span className="tick-gauge-value">
          <strong>{mspt.toFixed(1)}</strong>
          <small>ms</small>
        </span>
        <span className="tick-gauge-note">
          {high != null ? tr("of {0} ms · 5 min peak {1} ms", [BUDGET, high.toFixed(0)]) : tr("of {0} ms", [BUDGET])}
        </span>
        {(tps != null || tpsLevel) && <span className={`tick-gauge-tps${tpsLevel ? ` ${tpsLevel}` : ""}`}>{tps != null ? `TPS ${tps.toFixed(1)}` : tr("Over budget")}</span>}
      </div>
      <span className="tick-gauge-min">0</span>
      <span className="tick-gauge-max">{BUDGET}</span>
    </div>
  );
}

/** The last five minutes of tick times as thin bars under the ring, with the 50 ms line. */
function TickHistory({ log }: { log: { at: number; value: number }[] }) {
  const bars = log.slice(-60);
  if (bars.length < 2) return null;
  // Scale to the readings (at least 5 ms) so a quiet server still shows its shape; the 50 ms line only when it fits.
  const peak = Math.max(...bars.map((one) => one.value));
  const top = peak >= BUDGET * 0.6 ? Math.max(BUDGET * 1.2, peak * 1.1) : Math.max(5, Math.ceil(peak * 1.4));
  const width = 100 / 60;
  const budgetY = top >= BUDGET ? 40 - (BUDGET / top) * 40 : null;
  return (
    <div className="tick-history">
      <div className="side-stats-row">
        <span className="side-stats-label">{tr("Last 5 minutes")}</span>
        <span className="tick-history-legend">{tr("up to {0} ms", [Math.round(top)])}</span>
      </div>
      <svg viewBox="0 0 100 40" preserveAspectRatio="none" aria-hidden="true">
        {bars.map((one, index) => {
          const height = Math.max(2, (one.value / top) * 40);
          const level = one.value >= BUDGET ? "danger" : one.value >= BUDGET * 0.8 ? "warn" : "";
          return <rect key={one.at} className={level} x={(60 - bars.length + index) * width + width * 0.15} y={40 - height} width={width * 0.7} height={height} rx={0.4} />;
        })}
        <line className="base" x1="0" x2="100" y1="40" y2="40" vectorEffect="non-scaling-stroke" />
        {budgetY != null && <line className="budget" x1="0" x2="100" y1={budgetY} y2={budgetY} vectorEffect="non-scaling-stroke" />}
      </svg>
    </div>
  );
}

function SidePlayer({ player, onOpen }: { player: LivePlayer; onOpen: (name: string) => void }) {
  const head = useHead(player.name);
  return (
    <ListRow
      onClick={() => onOpen(player.name)}
      leading={head ? <img className="stat-player-head" src={head} alt="" /> : <span className="stat-player-face">{player.name.slice(0, 1).toUpperCase()}</span>}
      title={player.name}
      subtitle={player.world || undefined}
      meta={`${player.ping} ms`}
    />
  );
}

export function ServerStats({
  server,
  onOpenPlayer,
  onOpenTool,
  bare,
  onHide,
  extra,
}: {
  server: Server;
  onOpenPlayer: (name: string) => void;
  onOpenTool?: (tool: "monitor" | "players" | "disk") => void;
  bare?: boolean;
  onHide?: () => void;
  extra?: ReactNode;
}) {
  const toMonitor = onOpenTool ? () => onOpenTool("monitor") : undefined;
  const [stats, setStats] = useState<Stats | null>(null);
  const [cpuTrace, setCpuTrace] = useState<number[]>([]);
  const [memTrace, setMemTrace] = useState<number[]>([]);
  const [copied, setCopied] = useState(false);
  const [msptLog, setMsptLog] = useState<{ at: number; value: number }[]>([]);
  const [maxPlayers, setMaxPlayers] = useState<number | null>(null);
  const attention = useAttention();
  const running = server.status === "running";
  const panel = Boolean(server.panelHost);
  const last = useRef<string>("");

  useEffect(() => {
    if (last.current !== server.id) {
      last.current = server.id;
      setCpuTrace([]);
      setMemTrace([]);
      setMsptLog([]);
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
          setMemTrace((list) => [...list, Math.max(0, next.memory)].slice(-KEEP));
          const mspt = next.mspt;
          if (mspt != null) {
            const now = Date.now();
            setMsptLog((list) => [...list.filter((one) => now - one.at <= 300000), { at: now, value: mspt }]);
          }
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

  useEffect(() => {
    setMaxPlayers(null);
    if (!bare || !running || panel) return;
    let gone = false;
    commands
      .readServerProperties(server.id)
      .then((list) => {
        const found = list.find((one) => one.key === "max-players" || one.key.endsWith("max_players"));
        const value = found ? Number.parseInt(found.value, 10) : Number.NaN;
        if (!gone && Number.isFinite(value) && value > 0) setMaxPlayers(value);
      })
      .catch(() => {});
    return () => {
      gone = true;
    };
  }, [bare, panel, running, server.id]);

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


  if (bare) {
    const mspt = stats?.mspt ?? null;
    const high = msptLog.length ? Math.max(...msptLog.map((one) => one.value)) : null;
    const cells = maxPlayers != null && maxPlayers <= 100 ? maxPlayers : 0;
    const online = stats?.roster?.length ?? stats?.players ?? null;
    const tiles: (Parameters<typeof Tile>[0] | null)[] = stats
      ? [
          { ...memoryTile(stats.memory, stats.memoryLimit), trace: memTrace },
          { label: tr("CPU"), value: stats.cpu == null ? "—" : stats.cpu.toFixed(0), unit: stats.cpu == null ? undefined : "%", ratio: stats.cpu == null ? undefined : stats.cpu / 100, trace: cpuTrace, zero: true },
          stats.entities != null
            ? { label: tr("Entities"), value: Math.round(stats.entities).toLocaleString(), note: stats.chunks != null ? tr("{0} chunks", [Math.round(stats.chunks).toLocaleString()]) : undefined }
            : null,
          stats.disk != null ? diskTile(stats.disk, panel ? null : stats.diskLimit) : null,
          panel && stats.rx != null ? { label: tr("Network"), value: `↓ ${fileSize(stats.rx)}`, unit: `↑ ${fileSize(stats.tx ?? 0)}` } : null,
        ]
      : [];
    const uptime = stats && stats.uptime > 0 ? stats.uptime : (server.uptimeSeconds ?? 0) * 1000;
    return (
      <div className="server-stats bare side-stats" aria-label={tr("Usage")}>
        <div className="side-stats-head">
          <span className="side-stats-title">{mspt != null ? tr("Tick budget") : tr("Server status")}</span>
          {running && (
            <span className="side-stats-live">
              <span className="status-dot status-running" />
              {tr("Live")}
            </span>
          )}
          {onHide && (
            <button type="button" className="wb-icon ghost" aria-label={tr("Hide status panel")} data-tip={tr("Hide status panel")} onClick={onHide}>
              <Icon name="panel-right" />
            </button>
          )}
        </div>
        {!running || !stats ? (
          <>
            <div className="tick-gauge idle" aria-hidden="true">
              <svg viewBox="0 0 200 200">
                <circle className="track" cx="100" cy="100" r="80" strokeDasharray={`${ARC} ${RING}`} transform="rotate(135 100 100)" />
              </svg>
              <div className="tick-gauge-core">
                <span className="tick-gauge-value">
                  <strong>—</strong>
                  <small>ms</small>
                </span>
              </div>
              <span className="tick-gauge-min">0</span>
              <span className="tick-gauge-max">{BUDGET}</span>
            </div>
            <p className="stat-idle">{running ? tr("Loading…") : tr("Numbers appear while the server runs.")}</p>
          </>
        ) : (
          <>
            {mspt != null && <TickGauge mspt={mspt} high={high} tps={stats.tps} />}
            {mspt != null && <TickHistory log={msptLog} />}
            <section className="side-stats-section">
              <div className="side-stats-row">
                <span className="side-stats-label">{tr("Resources")}</span>
                {onOpenTool && (
                  <button type="button" className="side-stats-link" onClick={() => onOpenTool("monitor")}>
                    {tr("Monitor")}
                    <Icon name="arrow" />
                  </button>
                )}
              </div>
              <div className="side-tiles">{tiles.map((one) => (one ? <Tile key={one.label} {...one} /> : null))}</div>
            </section>
            {stats.roster && (
              <section className="side-stats-section">
                <div className="side-stats-row">
                  <span className="side-stats-label">{tr("Players")}</span>
                  <span className="side-stats-count">
                    <b>{online ?? 0}</b>
                    {maxPlayers != null && <small>{` / ${maxPlayers}`}</small>}
                  </span>
                </div>
                {cells > 0 && (
                  <div className="side-seats" aria-hidden="true">
                    {Array.from({ length: cells }, (_, index) => (
                      <i key={index} className={index < (online ?? 0) ? "on" : undefined} />
                    ))}
                  </div>
                )}
                {stats.roster.length ? (
                  <List variant="plain" density="compact" className="side-players">
                    {stats.roster.map((one) => (
                      <SidePlayer key={one.name} player={one} onOpen={onOpenPlayer} />
                    ))}
                  </List>
                ) : (
                  <p className="stat-players-none">{tr("No one is online.")}</p>
                )}
              </section>
            )}
          </>
        )}
        <div className="side-stats-foot">
          {uptime > 0 && running && (
            <div className="side-foot-row">
              <span>{tr("Uptime")}</span>
              <code>{uptimeText(uptime)}</code>
            </div>
          )}
          <div className="side-foot-row">
            <span>{tr("Address")}</span>
            <code>{address}</code>
            <button type="button" className={`wb-icon ghost side-copy${copied ? " done" : ""}`} onClick={() => void copy()} data-tip={tr("Copy address")} aria-label={tr("Copy address")}>
              <Icon name={copied ? "check" : "copy"} />
            </button>
          </div>
          {extra}
        </div>
      </div>
    );
  }

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
                <List variant="box" density="compact" className="stat-player-list">
                  {stats.roster.map((one) => (
                    <PlayerRow key={one.name} player={one} onOpen={onOpenPlayer} />
                  ))}
                </List>
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

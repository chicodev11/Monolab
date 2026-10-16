import { useEffect, useState } from "react";
import { commands } from "../../lib/ipc";
import type { PanelStats } from "../../lib/bindings";
import { fileSize } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { period, useAttention } from "../../lib/polling";

type Usage = { cpu: number | null; memory: number; memoryLimit: number; disk: number; rx: number; tx: number; uptime: number };

function fromPanel(stats: PanelStats): Usage {
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

function uptimeText(ms: number) {
  const minutes = Math.floor(ms / 60000);
  if (minutes < 60) return tr("{0} min", [minutes]);
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return tr("{0} h {1} min", [hours, minutes % 60]);
  return tr("{0} d {1} h", [Math.floor(hours / 24), hours % 24]);
}

function Meter({ label, value, ratio }: { label: string; value: string; ratio?: number }) {
  const level = ratio === undefined ? "" : ratio >= 0.9 ? " danger" : ratio >= 0.75 ? " warn" : "";
  return (
    <span className={`usage-item${level}`}>
      <span className="usage-label">{label}</span>
      <span className="usage-value">{value}</span>
      {ratio !== undefined && (
        <span className="usage-bar">
          <i style={{ width: `${Math.min(100, Math.max(0, ratio * 100))}%` }} />
        </span>
      )}
    </span>
  );
}

export function UsageStrip({ server }: { server: { id: string; panelHost?: string; maxMemoryMib: number; running: boolean } }) {
  const [usage, setUsage] = useState<Usage | null>(null);
  const panel = Boolean(server.panelHost);
  const attention = useAttention();

  useEffect(() => {
    if (!server.running) {
      setUsage(null);
      return;
    }
    let gone = false;
    let timer = 0;
    const read = async () => {
      try {
        if (panel) {
          const stats = await commands.panelUsage(server.id);
          if (!gone) setUsage(stats ? fromPanel(stats) : null);
        } else {
          const snap = await commands.getResourceSnapshot(server.id);
          if (!gone)
            setUsage({ cpu: snap.cpuPercent, memory: snap.memoryMib * 1048576, memoryLimit: server.maxMemoryMib * 1048576, disk: 0, rx: 0, tx: 0, uptime: 0 });
        }
      } catch {}
      const wait = period(panel ? 2000 : 2500, attention);
      if (!gone && wait) timer = window.setTimeout(() => void read(), wait);
    };
    void read();
    return () => {
      gone = true;
      window.clearTimeout(timer);
    };
  }, [attention, panel, server.id, server.maxMemoryMib, server.running]);

  if (!usage) return null;
  const memoryRatio = usage.memoryLimit > 0 ? usage.memory / usage.memoryLimit : undefined;
  return (
    <div className="usage-strip" aria-label={tr("Usage")}>
      <Meter label="CPU" value={usage.cpu == null ? "—" : `${usage.cpu.toFixed(0)}%`} />
      <Meter
        label={tr("Memory")}
        value={usage.memoryLimit > 0 ? `${fileSize(usage.memory)} / ${fileSize(usage.memoryLimit)}` : fileSize(usage.memory)}
        ratio={memoryRatio}
      />
      {panel && <Meter label={tr("Disk")} value={fileSize(usage.disk)} />}
      {panel && <Meter label={tr("Network")} value={`↓ ${fileSize(usage.rx)} · ↑ ${fileSize(usage.tx)}`} />}
      {panel && usage.uptime > 0 && <Meter label={tr("Uptime")} value={uptimeText(usage.uptime)} />}
    </div>
  );
}

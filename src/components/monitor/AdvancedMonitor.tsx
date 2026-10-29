import { useEffect, useRef, useState } from "react";
import { commands, type JvmLive } from "../../lib/bindings";
import { fileSize } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { Card } from "../primitives";

type Details = NonNullable<Awaited<ReturnType<typeof commands.processDetails>>>;
type GcSample = { at: number; gc: Record<string, { count: number; ms: number }> };

const mib = (value: number) => fileSize(value * 1048576);
const ms = (value: number) => `${value < 10 ? value.toFixed(1) : Math.round(value)} ms`;

function since(seconds: number) {
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  return hours ? tr("{0} h {1} min", [hours, minutes]) : tr("{0} min", [minutes]);
}

/** Extra monitor detail: tick spread, JVM, garbage collection, memory pools and this PC. */
export function AdvancedMonitor({ serverId, local, jvm, jvmCards }: { serverId: string; local: boolean; jvm: JvmLive | null | undefined; jvmCards: boolean }) {
  const [details, setDetails] = useState<Details | null>(null);
  const [samples, setSamples] = useState<GcSample[]>([]);
  const seen = useRef<string>(serverId);

  useEffect(() => {
    if (!local) return;
    let gone = false;
    const read = () =>
      commands
        .processDetails(serverId)
        .then((found) => !gone && setDetails(found))
        .catch(() => {});
    void read();
    const timer = window.setInterval(read, 2000);
    return () => {
      gone = true;
      window.clearInterval(timer);
    };
  }, [serverId, local]);

  useEffect(() => {
    if (!jvm) return;
    const fresh = seen.current !== serverId;
    seen.current = serverId;
    const now = Date.now();
    const sample = { at: now, gc: Object.fromEntries(jvm.gc.map((one) => [one.name, { count: one.count, ms: one.ms }])) };
    const timer = window.setTimeout(() =>
      setSamples((kept) => [...(fresh ? [] : kept).filter((one) => now - one.at <= 65_000), sample]),
    );
    return () => window.clearTimeout(timer);
  }, [jvm, serverId]);
  const oldest = samples[0];
  const newest = samples.at(-1);
  const span = oldest && newest ? (newest.at - oldest.at) / 1000 : 0;
  const ticks = jvm?.ticks;
  // A tick has 50 ms. Typical ticks near that mean lag; a single slow tick (a save, a GC pause) is normal.
  const typical = (value: number) => (value >= 50 ? "danger" : value >= 40 ? "warn" : "");
  const worst = (value: number) => (value >= 500 ? "danger" : value >= 100 ? "warn" : "");
  const heap = (jvm?.pools ?? []).filter((one) => one.heap).sort((a, b) => b.usedMib - a.usedMib);
  const other = (jvm?.pools ?? []).filter((one) => !one.heap).sort((a, b) => b.usedMib - a.usedMib);
  const pool = (one: { name: string; usedMib: number; maxMib: number }) => {
    const share = one.maxMib > 0 ? Math.min(100, (one.usedMib / one.maxMib) * 100) : null;
    return (
      <li key={one.name}>
        <div className="adv-pool-line">
          <span>{one.name}</span>
          <strong>
            {mib(one.usedMib)}
            {one.maxMib > 0 && <small> / {mib(one.maxMib)}</small>}
          </strong>
        </div>
        {share !== null && (
          <div className="adv-meter" aria-hidden>
            <i style={{ width: `${share}%` }} />
          </div>
        )}
      </li>
    );
  };

  return (
    <div className="advanced-monitor">
      {jvmCards && (
        <div className="adv-col">
            <Card className="adv-ticks" title={tr("Tick time")} aside={ticks ? <span className="muted">{tr("Last {0} ticks", [ticks.samples])}</span> : null}>
              {ticks ? (
                <>
                  <div className="adv-stats three">
                    <div className={typical(ticks.p50 ?? 0)}><span>{tr("Median")}</span><strong>{ms(ticks.p50 ?? 0)}</strong></div>
                    <div className={typical(ticks.p95 ?? 0)}><span>{tr("Slowest 5%")}</span><strong>{ms(ticks.p95 ?? 0)}</strong></div>
                    <div className={worst(ticks.max ?? 0)}><span>{tr("Slowest")}</span><strong>{ms(ticks.max ?? 0)}</strong></div>
                  </div>
                  <p className="adv-note">{tr("A tick has 50 ms. The median and slowest 5% matter; one slow tick now and then is normal.")}</p>
                </>
              ) : (
                <p className="adv-note">{jvm ? tr("Waiting for tick times.") : tr("Shown with add-on 34 or later.")}</p>
              )}
            </Card>
            {(heap.length > 0 || other.length > 0) && (
              <Card className="adv-memory" title={tr("Memory areas")}>
                <ul className="adv-pools">{heap.map(pool)}</ul>
                {other.length > 0 && (
                  <>
                    <h4 className="adv-sub">{tr("Outside the heap")}</h4>
                    <ul className="adv-pools">{other.map(pool)}</ul>
                  </>
                )}
              </Card>
            )}
        </div>
      )}
      <div className="adv-col">
        {jvmCards && (
          <>
            <Card className="adv-jvm" title="JVM" aside={jvm ? <span className="muted">{jvm.vm}</span> : null}>
              {jvm ? (
                <div className="adv-stats">
                  {jvm.processCpu != null && <div><span>{tr("JVM CPU")}</span><strong>{`${jvm.processCpu.toFixed(1)}%`}</strong></div>}
                  <div><span>{tr("Threads")}</span><strong>{jvm.threads.toLocaleString()}<small> / {jvm.peakThreads.toLocaleString()}</small></strong></div>
                  <div><span>{tr("Loaded classes")}</span><strong>{jvm.classes.toLocaleString()}</strong></div>
                  <div><span>{tr("Running for")}</span><strong>{since(jvm.uptimeMs / 1000)}</strong></div>
                </div>
              ) : (
                <p className="adv-note">{tr("Shown with add-on 34 or later.")}</p>
              )}
            </Card>
            {jvm && jvm.gc.length > 0 && (
              <Card className="adv-gc" title={tr("Garbage collection")} aside={span >= 5 ? <span className="muted">{tr("Change over the last {0} s", [Math.round(span)])}</span> : null}>
                <ul className="adv-rows">
                  {jvm.gc.map((one) => {
                    const before = oldest?.gc[one.name];
                    return (
                      <li key={one.name}>
                        <div>
                          <code>{one.name}</code>
                          <span>{tr("{0} times · {1} in total", [one.count.toLocaleString(), ms(one.ms)])}</span>
                        </div>
                        <strong>{before && span >= 5 && one.count >= before.count ? `+${one.count - before.count} · ${ms(one.ms - before.ms)}` : "—"}</strong>
                      </li>
                    );
                  })}
                </ul>
              </Card>
            )}
          </>
        )}
        {local && details && (
          <Card className="adv-pc" title={tr("This PC")} aside={<span className="muted">{tr("{0, plural, one {# process} other {# processes}}", [details.processes])}</span>}>
            <div className="adv-pc-body">
              <ul className="adv-rows">
                <li><span>{tr("Disk read")}</span><strong>{`${fileSize((details.diskReadKib ?? 0) * 1024)}/s`}</strong></li>
                <li><span>{tr("Disk write")}</span><strong>{`${fileSize((details.diskWriteKib ?? 0) * 1024)}/s`}</strong></li>
                <li><span>{tr("PC memory")}</span><strong>{`${mib(details.systemMemoryUsedMib)} / ${mib(details.systemMemoryTotalMib)}`}</strong></li>
                {details.swapTotalMib > 0 && <li><span>{tr("Page file")}</span><strong>{`${mib(details.swapUsedMib)} / ${mib(details.swapTotalMib)}`}</strong></li>}
              </ul>
              <div className="adv-cores-box">
                <span className="adv-sub">{tr("CPU by core")}</span>
                <div className="adv-cores" aria-label={tr("CPU by core")}>
                  {details.cores.map((one, at) => {
                    const usage = one ?? 0;
                    return (
                      <span key={at} data-tip={`${tr("Core {0}", [at + 1])} · ${usage.toFixed(0)}%`}>
                        <i style={{ height: `${Math.min(100, usage)}%` }} />
                      </span>
                    );
                  })}
                </div>
              </div>
            </div>
          </Card>
        )}
      </div>
    </div>
  );
}

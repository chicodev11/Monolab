import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { commands } from "../../lib/ipc";
import type { AddonEntry, PropertyInfo } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import { Dropdown } from "../ui/Dropdown";

type Side = { id: string; name: string; core: string; version: string; javaPath: string; maxMemoryMib: number; minMemoryMib?: number; jvmArgs: string; customCommand?: string };

type Loaded = { props: PropertyInfo[]; addons: AddonEntry[] };

type Row = { label: string; left: string; right: string };

function memory(side: Side) {
  const gb = (mib: number) => `${Math.round((mib / 1024) * 10) / 10} GB`;
  const low = side.minMemoryMib || side.maxMemoryMib;
  return low === side.maxMemoryMib ? gb(side.maxMemoryMib) : `${gb(low)} ~ ${gb(side.maxMemoryMib)}`;
}

async function load(id: string): Promise<Loaded> {
  const [props, addons] = await Promise.all([
    commands.readServerProperties(id).catch(() => [] as PropertyInfo[]),
    commands.scanAddons(id).catch(() => [] as AddonEntry[]),
  ]);
  return { props, addons };
}

export function CompareDialog({ current, others, onClose }: { current: Side; others: Side[]; onClose: () => void }) {
  const [otherId, setOtherId] = useState(others[0]?.id ?? "");
  const [mine, setMine] = useState<Loaded | null>(null);
  const [theirs, setTheirs] = useState<Loaded | null>(null);
  const [same, setSame] = useState(false);
  const other = others.find((one) => one.id === otherId) ?? null;

  useEffect(() => {
    void load(current.id).then(setMine);
  }, [current.id]);
  useEffect(() => {
    setTheirs(null);
    if (otherId) void load(otherId).then(setTheirs);
  }, [otherId]);

  const start = useMemo<Row[]>(() => {
    if (!other) return [];
    return [
      { label: tr("Core"), left: `${current.core} ${current.version}`, right: `${other.core} ${other.version}` },
      { label: tr("Memory"), left: memory(current), right: memory(other) },
      { label: tr("Java"), left: current.javaPath || tr("Auto"), right: other.javaPath || tr("Auto") },
      { label: tr("JVM options"), left: current.jvmArgs || "—", right: other.jvmArgs || "—" },
      { label: tr("Start command"), left: current.customCommand || tr("Auto"), right: other.customCommand || tr("Auto") },
    ];
  }, [current, other]);

  const settings = useMemo<Row[]>(() => {
    if (!mine || !theirs) return [];
    const left = new Map(mine.props.map((one) => [one.key, one.value]));
    const right = new Map(theirs.props.map((one) => [one.key, one.value]));
    const keys = [...new Set([...left.keys(), ...right.keys()])].sort();
    return keys.map((key) => ({ label: key, left: left.get(key) ?? "—", right: right.get(key) ?? "—" }));
  }, [mine, theirs]);

  const addons = useMemo<Row[]>(() => {
    if (!mine || !theirs) return [];
    const keyOf = (one: AddonEntry) => one.name.toLowerCase();
    const show = (one: AddonEntry | undefined) => (one ? `${one.version || "?"}${one.enabled ? "" : ` (${tr("Off")})`}` : "—");
    const left = new Map(mine.addons.map((one) => [keyOf(one), one]));
    const right = new Map(theirs.addons.map((one) => [keyOf(one), one]));
    const keys = [...new Set([...left.keys(), ...right.keys()])];
    return keys
      .map((key) => ({ label: (left.get(key) ?? right.get(key))!.name, left: show(left.get(key)), right: show(right.get(key)) }))
      .sort((a, b) => a.label.localeCompare(b.label));
  }, [mine, theirs]);

  const section = (title: string, rows: Row[]) => {
    const differ = rows.filter((one) => one.left !== one.right);
    const shown = same ? rows : differ;
    return (
      <section className="compare-section">
        <h4>
          {title}
          <span className="muted">{tr("{0} different", [differ.length])}</span>
        </h4>
        {shown.length === 0 ? (
          <p className="muted compare-none">{tr("No differences.")}</p>
        ) : (
          <div className="compare-table">
            {shown.map((one) => (
              <div key={one.label} className={one.left === one.right ? "same" : "differ"}>
                <span className="compare-key" title={one.label}>{one.label}</span>
                <code title={one.left}>{one.left}</code>
                <code title={one.right}>{one.right}</code>
              </div>
            ))}
          </div>
        )}
      </section>
    );
  };

  return createPortal(
    <div className="modal-backdrop roomy" onClick={onClose}>
      <div className="add-modal compare-dialog" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
        <div className="tool-top">
          <p>
            <strong>{tr("Compare servers")}</strong> · {current.name}
          </p>
          <div className="modal-top-actions">
            <button className="close in-flow" data-tip={tr("Close")} aria-label={tr("Close")} onClick={onClose}>
              <Icon name="close" />
            </button>
          </div>
        </div>
        {others.length === 0 ? (
          <p className="muted tool-empty">{tr("There is no other server to compare with.")}</p>
        ) : (
          <>
            <div className="compare-head">
              <span className="compare-key" />
              <strong>{current.name}</strong>
              <Dropdown
                value={otherId}
                options={others.map((one) => ({ value: one.id, label: one.name }))}
                onChange={setOtherId}
              />
            </div>
            <label className="compare-same">
              <input type="checkbox" checked={same} onChange={(event) => setSame(event.target.checked)} />
              {tr("Show settings that are the same")}
            </label>
            <div className="compare-body">
              {section(tr("Start"), start)}
              {!mine || !theirs ? (
                <p className="muted tool-empty">{tr("Loading…")}</p>
              ) : (
                <>
                  {section("server.properties", settings)}
                  {section(tr("Mods and plugins"), addons)}
                </>
              )}
            </div>
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}

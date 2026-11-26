import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { commands } from "../../lib/ipc";
import type { AddonEntry, BackupFile, PropertyInfo } from "../../lib/bindings";
import { date } from "../../lib/format";
import { tr, trBack } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import { Dropdown } from "../ui/Dropdown";
import { CoreIcon } from "../ui/CoreIcon";
import { FilterTabs, SearchField } from "../primitives";
import { foldDiff, lineDiff, type DiffRow } from "../../lib/lineDiff";

type Side = { id: string; name: string; core: string; version: string; javaPath: string; maxMemoryMib: number; minMemoryMib?: number; jvmArgs: string; customCommand?: string; remoteHost?: string; panelHost?: string };

/** A server that is off or out of reach shouldn't keep a side on "Loading…" forever. */
const WAIT_MS = 15_000;
function inTime<T>(work: Promise<T>, name: string): Promise<T> {
  return Promise.race([
    work,
    new Promise<T>((_, fail) => window.setTimeout(() => fail(new Error(tr("{0} didn't answer in time.", [name]))), WAIT_MS)),
  ]);
}
const far = (side: Side) => Boolean(side.remoteHost || side.panelHost);

/** Each part loads on its own: a slow mods folder shouldn't hold back server.properties. */
type Loaded = { props: PropertyInfo[] | null; addons: AddonEntry[] | null };
const NOTHING: Loaded = { props: null, addons: null };

/** A row's state: both sides equal, different, or present on one side only (mods and plugins). */
type RowState = "same" | "differ" | "left" | "right";
type Row = { label: string; left: string; right: string; state: RowState };

const MISSING = "—";
function row(label: string, left: string, right: string): Row {
  return { label, left, right, state: left === right ? "same" : left === MISSING ? "right" : right === MISSING ? "left" : "differ" };
}

type Tab = "start" | "props" | "addons" | "files";

function memory(side: Side) {
  const gb = (mib: number) => `${Math.round((mib / 1024) * 10) / 10} GB`;
  const low = side.minMemoryMib || side.maxMemoryMib;
  return low === side.maxMemoryMib ? gb(side.maxMemoryMib) : `${gb(low)} ~ ${gb(side.maxMemoryMib)}`;
}

/** Loads both parts of a server, calling `put` as each arrives; the returned function stops it. */
function load(side: Side, put: (change: Partial<Loaded>) => void): () => void {
  let gone = false;
  void inTime(commands.readServerProperties(side.id), side.name)
    .catch(() => [] as PropertyInfo[])
    .then((props) => !gone && put({ props }));
  void inTime(commands.scanAddons(side.id), side.name)
    .catch(() => [] as AddonEntry[])
    .then((addons) => !gone && put({ addons }));
  return () => {
    gone = true;
  };
}

/** One side of a file compare: a server's folder, or a backup of one. */
type Reader = { name: string; read: (path: string) => Promise<string | null> };

const BACKUP = "backup:";

function serverReader(side: Side): Reader {
  return {
    name: side.name,
    read: (path) => inTime(commands.readEditableFile(side.id, path), side.name).catch((error: unknown) => {
      const text = String(error);
      if (/not found|no such|cannot find|does not exist/i.test(text)) return null;
      return Promise.reject(new Error(`${side.name}: ${trBack(text)}`));
    }),
  };
}

function backupReader(server: Side, backup: BackupFile): Reader {
  const name = tr("Backup {0}", [date(backup.createdAt)]);
  return { name, read: (path) => commands.backupEntryText(server.id, backup.fileName, path) };
}

export function CompareDialog({ current, others: given, onClose }: { current: Side; others: Side[]; onClose: () => void }) {
  // Servers on this PC first (they answer at once), SSH and panel servers after them.
  const others = useMemo(() => [...given].sort((a, b) => Number(far(a)) - Number(far(b))), [given]);
  const [otherId, setOtherId] = useState(others[0]?.id ?? "");
  const [backups, setBackups] = useState<BackupFile[]>([]);
  useEffect(() => {
    let gone = false;
    void commands.listBackups(current.id).then((found) => {
      if (gone) return;
      setBackups(found);
      if (!others.length && found[0]) setOtherId(BACKUP + found[0].fileName);
    }).catch(() => {});
    return () => {
      gone = true;
    };
  }, [current.id, others.length]);
  const backup = otherId.startsWith(BACKUP) ? backups.find((one) => one.fileName === otherId.slice(BACKUP.length)) ?? null : null;
  const [mine, setMine] = useState<Loaded>(NOTHING);
  const [theirs, setTheirs] = useState<Loaded>(NOTHING);
  const [same, setSame] = useState(false);
  const [query, setQuery] = useState("");
  const [tab, setTab] = useState<Tab>("start");
  const other = others.find((one) => one.id === otherId) ?? null;
  // A backup only has files to compare.
  const shownTab: Tab = backup ? "files" : tab;

  useEffect(() => {
    setMine(NOTHING);
    return load(current, (change) => setMine((now) => ({ ...now, ...change })));
    // The id says which server this is.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current.id]);
  useEffect(() => {
    setTheirs(NOTHING);
    const side = others.find((one) => one.id === otherId);
    if (!side) return;
    return load(side, (change) => setTheirs((now) => ({ ...now, ...change })));
  }, [otherId, others]);

  const start = useMemo<Row[]>(() => {
    if (!other) return [];
    return [
      row(tr("Core"), `${current.core} ${current.version}`, `${other.core} ${other.version}`),
      row(tr("Memory"), memory(current), memory(other)),
      row(tr("Java"), current.javaPath || tr("Auto"), other.javaPath || tr("Auto")),
      row(tr("JVM options"), current.jvmArgs || MISSING, other.jvmArgs || MISSING),
      row(tr("Start command"), current.customCommand || tr("Auto"), other.customCommand || tr("Auto")),
    ].map((one) => (one.state === "left" || one.state === "right" ? { ...one, state: "differ" as const } : one));
  }, [current, other]);

  const settings = useMemo<Row[] | null>(() => {
    if (!mine.props || !theirs.props) return null;
    const left = new Map(mine.props.map((one) => [one.key, one.value]));
    const right = new Map(theirs.props.map((one) => [one.key, one.value]));
    const keys = [...new Set([...left.keys(), ...right.keys()])].sort();
    return keys.map((key) => row(key, left.get(key) ?? MISSING, right.get(key) ?? MISSING));
  }, [mine, theirs]);

  const addons = useMemo<Row[] | null>(() => {
    if (!mine.addons || !theirs.addons) return null;
    const keyOf = (one: AddonEntry) => one.name.toLowerCase();
    const show = (one: AddonEntry | undefined) => (one ? `${one.version || "?"}${one.enabled ? "" : ` · ${tr("Disabled")}`}` : MISSING);
    const left = new Map(mine.addons.map((one) => [keyOf(one), one]));
    const right = new Map(theirs.addons.map((one) => [keyOf(one), one]));
    const keys = [...new Set([...left.keys(), ...right.keys()])];
    return keys
      .map((key) => row((left.get(key) ?? right.get(key))!.name, show(left.get(key)), show(right.get(key))))
      .sort((a, b) => a.label.localeCompare(b.label));
  }, [mine, theirs]);

  const differing = (rows: Row[] | null) => (rows ? rows.filter((one) => one.state !== "same").length : null);
  const rowsOf: Record<Exclude<Tab, "files">, Row[] | null> = { start, props: settings, addons };
  const otherName = backup ? tr("Backup {0}", [date(backup.createdAt)]) : other?.name ?? "";

  const table = (which: Exclude<Tab, "files">) => {
    const rows = rowsOf[which];
    if (!rows) return <p className="cmp-empty muted">{tr("Loading…")}</p>;
    const needle = query.trim().toLowerCase();
    const shown = rows.filter(
      (one) => (same || one.state !== "same") && (!needle || `${one.label} ${one.left} ${one.right}`.toLowerCase().includes(needle)),
    );
    const onlyMine = rows.filter((one) => one.state === "left").length;
    const onlyTheirs = rows.filter((one) => one.state === "right").length;
    const versions = rows.filter((one) => one.state === "differ").length;
    return (
      <>
        {which === "addons" && (onlyMine > 0 || onlyTheirs > 0 || versions > 0) && (
          <div className="cmp-summary">
            {onlyMine > 0 && <span>{tr("{0} only in {1}", [onlyMine, current.name])}</span>}
            {onlyTheirs > 0 && <span>{tr("{0} only in {1}", [onlyTheirs, otherName])}</span>}
            {versions > 0 && <span>{tr("{0, plural, one {# different version} other {# different versions}}", [versions])}</span>}
          </div>
        )}
        {shown.length === 0 ? (
          <p className="cmp-empty muted">{needle ? tr("Nothing matches your search.") : tr("No differences.")}</p>
        ) : (
          <div className="cmp-table" role="table" aria-label={otherName}>
            <div className="cmp-row cmp-head" role="row">
              <span role="columnheader">{which === "addons" ? tr("Name") : tr("Setting")}</span>
              <span role="columnheader">{current.name}</span>
              <span role="columnheader">{otherName}</span>
            </div>
            {shown.map((one) => (
              <div key={one.label} className={`cmp-row ${one.state}`} role="row">
                <span className="cmp-key" role="cell" title={one.label}>
                  {one.label}
                </span>
                <code role="cell" className={one.left === MISSING ? "none" : ""} title={one.left}>
                  {one.left}
                </code>
                <code role="cell" className={one.right === MISSING ? "none" : ""} title={one.right}>
                  {one.right}
                </code>
              </div>
            ))}
          </div>
        )}
      </>
    );
  };

  const tabs: { value: Tab; label: string; count?: number | null }[] = backup
    ? [{ value: "files", label: tr("Config files") }]
    : [
        { value: "start", label: tr("Start"), count: differing(start) },
        { value: "props", label: "server.properties", count: differing(settings) },
        { value: "addons", label: tr("Mods and plugins"), count: differing(addons) },
        { value: "files", label: tr("Config files") },
      ];

  return createPortal(
    <div className="modal-backdrop roomy" onClick={onClose}>
      <div className="add-modal compare-dialog" role="dialog" aria-modal="true" aria-label={tr("Compare servers")} onClick={(event) => event.stopPropagation()}>
        <div className="cmp-top">
          <h2>{tr("Compare servers")}</h2>
          <button className="close in-flow" data-tip={tr("Close")} aria-label={tr("Close")} onClick={onClose}>
            <Icon name="close" />
          </button>
        </div>
        {others.length === 0 && backups.length === 0 ? (
          <p className="muted tool-empty">{tr("There is no other server or backup to compare with.")}</p>
        ) : (
          <>
            <div className="cmp-sides">
              <div className="cmp-side">
                <span className="cmp-side-icon">
                  <CoreIcon core={current.core} />
                </span>
                <span className="cmp-side-text">
                  <strong>{current.name}</strong>
                  <small>{`${current.core} ${current.version}`}</small>
                </span>
              </div>
              <span className="cmp-versus" aria-hidden>
                <Icon name="arrow" />
              </span>
              <div className="cmp-side pick">
                <Dropdown
                  className="cmp-pick"
                  value={otherId}
                  search={others.length + backups.length > 8 ? tr("Search") : undefined}
                  options={[
                    ...others.map((one) => ({ value: one.id, label: one.name, hint: one.panelHost ? tr("Panel") : one.remoteHost ? "SSH" : `${one.core} ${one.version}` })),
                    ...backups.map((one) => ({ value: BACKUP + one.fileName, label: tr("Backup {0}", [date(one.createdAt)]), hint: one.note || (one.worldsOnly ? tr("Worlds only") : tr("Backup")) })),
                  ]}
                  onChange={setOtherId}
                />
              </div>
            </div>
            <div className="cmp-bar">
              <FilterTabs
                value={shownTab}
                onChange={setTab}
                label={tr("Compare")}
                options={tabs.map((one) => ({ value: one.value, label: one.label, count: one.count ?? undefined }))}
              />
              <span className="live-spacer" />
              {shownTab !== "files" && (
                <>
                  <SearchField className="cmp-search" value={query} onChange={setQuery} placeholder={tr("Filter")} />
                  <label className="cmp-same">
                    <span className="switch">
                      <input type="checkbox" checked={same} onChange={(event) => setSame(event.target.checked)} />
                      <span />
                    </span>
                    {tr("Show the same too")}
                  </label>
                </>
              )}
            </div>
            <div className={`cmp-body ${shownTab}`}>
              {shownTab !== "files" && table(shownTab)}
              {shownTab === "files" && backup && <FileCompare key={otherId} left={serverReader(current)} right={backupReader(current, backup)} />}
              {shownTab === "files" && !backup && other && <FileCompare key={otherId} left={serverReader(current)} right={serverReader(other)} />}
            </div>
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}

const COMMON_FILES = ["server.properties", "bukkit.yml", "spigot.yml", "config/paper-global.yml", "config/paper-world-defaults.yml", "ops.json", "whitelist.json"];

type Overview = { path: string; state: "same" | "differ" | "left" | "right" }[];

/** The usual config files on both sides, and which of them differ. */
function useOverview(left: Reader, right: Reader): Overview | null {
  const [found, setFound] = useState<Overview | null>(null);
  useEffect(() => {
    let gone = false;
    setFound(null);
    void Promise.all(
      COMMON_FILES.map(async (path) => {
        const [one, two] = await Promise.all([left.read(path).catch(() => null), right.read(path).catch(() => null)]);
        if (one === null && two === null) return null;
        const state: Overview[number]["state"] = one === null ? "right" : two === null ? "left" : one === two ? "same" : "differ";
        return { path, state };
      }),
    ).then((rows) => !gone && setFound(rows.filter((one): one is Overview[number] => one !== null)));
    return () => {
      gone = true;
    };
    // The readers are remade each render; the names say which sides these are.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [left.name, right.name]);
  return found;
}

/** One text file, line by line, between two servers or a server and a backup. */
function FileCompare({ left, right }: { left: Reader; right: Reader }) {
  const [path, setPath] = useState("server.properties");
  const [rows, setRows] = useState<DiffRow[] | "big" | "same" | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState("");
  const overview = useOverview(left, right);
  const compare = async (wanted = path) => {
    const file = wanted.trim().replace(/^\/+/, "");
    if (!file) return;
    setPath(file);
    setBusy(true);
    setFailed("");
    setRows(null);
    const read = async (side: Reader) => {
      const text = await side.read(file);
      if (text === null) throw new Error(tr("{0} has no {1}.", [side.name, file]));
      return text;
    };
    try {
      const [before, after] = await Promise.all([read(left), read(right)]);
      const diff = lineDiff(before, after);
      setRows(!diff ? "big" : diff.every((one) => one.kind === "same") ? "same" : foldDiff(diff));
    } catch (error) {
      setFailed(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  };
  // Open on server.properties so there's something to read right away.
  useEffect(() => {
    void compare("server.properties");
    // Once per pair of sides (the component is keyed by the other side).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const stateText = (state: Overview[number]["state"]) =>
    state === "same" ? tr("Same") : state === "differ" ? tr("Different") : state === "left" ? tr("Only in {0}", [left.name]) : tr("Only in {0}", [right.name]);
  return (
    <div className="cmp-files">
      <aside className="cmp-file-list">
        <form
          className="cmp-file-form"
          onSubmit={(event) => {
            event.preventDefault();
            void compare();
          }}
        >
          <input list="compare-files" value={path} onChange={(event) => setPath(event.target.value)} placeholder="plugins/Essentials/config.yml" spellCheck={false} aria-label={tr("File in the server folder")} />
          <datalist id="compare-files">
            {COMMON_FILES.map((one) => (
              <option key={one} value={one} />
            ))}
          </datalist>
          <button type="submit" className="wb-icon" disabled={busy || !path.trim()} aria-label={tr("Compare")} data-tip={tr("Compare")}>
            <Icon name="search" />
          </button>
        </form>
        {overview === null ? (
          <p className="cmp-empty muted">{tr("Loading…")}</p>
        ) : (
          <ul aria-label={tr("Config files")}>
            {overview.map((one) => (
              <li key={one.path}>
                <button
                  type="button"
                  className={`cmp-file ${one.state}${one.path === path ? " chosen" : ""}`}
                  disabled={one.state === "left" || one.state === "right"}
                  onClick={() => void compare(one.path)}
                >
                  <code title={one.path}>{one.path}</code>
                  <small>{stateText(one.state)}</small>
                </button>
              </li>
            ))}
          </ul>
        )}
      </aside>
      <section className="cmp-file-view" aria-live="polite">
        <div className="cmp-file-head">
          <code>{path}</code>
          {Array.isArray(rows) && (
            <span className="cmp-legend">
              <span className="bk-line remove">− {left.name}</span>
              <span className="bk-line add">+ {right.name}</span>
            </span>
          )}
        </div>
        {busy && <p className="cmp-empty muted">{tr("Comparing…")}</p>}
        {failed && <p className="cmp-empty muted">{failed}</p>}
        {rows === "same" && <p className="cmp-empty muted">{tr("No differences.")}</p>}
        {rows === "big" && <p className="cmp-empty muted">{tr("Too many changed lines to compare.")}</p>}
        {Array.isArray(rows) && (
          <pre className="cmp-diff">
            {rows.map((row, at) =>
              row.kind === "gap" ? (
                <span key={at} className="bk-line gap">{tr("{0, plural, one {# unchanged line} other {# unchanged lines}}", [row.count])}</span>
              ) : (
                <span key={at} className={`bk-line ${row.kind}`}>
                  {row.kind === "add" ? "+ " : row.kind === "remove" ? "− " : "  "}
                  {row.text}
                </span>
              ),
            )}
          </pre>
        )}
      </section>
    </div>
  );
}

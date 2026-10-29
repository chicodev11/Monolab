import { useEffect, useMemo, useState } from "react";
import { commands } from "../../lib/ipc";
import type { BackupContents, BackupEntry, BackupFile } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { fileSize, relativeTime } from "../../lib/format";
import { foldDiff, lineDiff, type DiffRow } from "../../lib/lineDiff";
import { Icon } from "../ui/Icon";
import { SegmentedControl } from "../primitives";

type Filter = "diff" | "Changed" | "Removed" | "Added" | "all";
type Group = { name: string; folder: boolean; entries: BackupEntry[] };

const SHOWN = 300;
const TEXT_LIMIT = 1024 * 1024;

function stateLabel(state: string) {
  if (state === "Changed") return tr("Changed");
  if (state === "Removed") return tr("Only in backup");
  if (state === "Added") return tr("Only now");
  return tr("Same");
}

function groupOf(entries: BackupEntry[]): Group[] {
  const groups = new Map<string, Group>();
  for (const one of entries) {
    const cut = one.path.indexOf("/");
    const name = cut < 0 ? one.path : one.path.slice(0, cut);
    const group = groups.get(name) ?? { name, folder: cut >= 0, entries: [] };
    group.entries.push(one);
    groups.set(name, group);
  }
  return [...groups.values()].sort((a, b) => Number(b.folder) - Number(a.folder) || a.name.localeCompare(b.name));
}

function Diff({ serverId, backup, entry }: { serverId: string; backup: string; entry: BackupEntry }) {
  const [rows, setRows] = useState<DiffRow[] | "binary" | "big" | null>(null);
  useEffect(() => {
    let gone = false;
    void Promise.all([
      commands.backupEntryText(serverId, backup, entry.path),
      entry.state === "Removed" ? Promise.resolve("") : commands.readEditableFile(serverId, entry.path).catch(() => null),
    ]).then(
      ([then, now]) => {
        if (gone) return;
        if (then == null || now == null) return setRows("binary");
        const diff = lineDiff(then, now);
        setRows(diff ? foldDiff(diff) : "big");
      },
      () => !gone && setRows("binary"),
    );
    return () => {
      gone = true;
    };
  }, [serverId, backup, entry.path, entry.state]);
  if (rows === null) return <div className="bk-diff muted">{tr("Loading…")}</div>;
  if (rows === "binary") return <div className="bk-diff muted">{tr("Only text files can be compared line by line.")}</div>;
  if (rows === "big") return <div className="bk-diff muted">{tr("Too many changed lines to compare.")}</div>;
  return (
    <div className="bk-diff">
      <div className="bk-diff-legend">
        <span className="bk-line remove">− {tr("Backup")}</span>
        <span className="bk-line add">+ {tr("Now")}</span>
      </div>
      <pre>
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
    </div>
  );
}

export function BackupBrowser({
  serverId,
  backup,
  stopped,
  onBack,
  onRestore,
}: {
  serverId: string;
  backup: BackupFile;
  stopped: boolean;
  onBack: () => void;
  onRestore: (paths: string[]) => Promise<boolean>;
}) {
  const [data, setData] = useState<BackupContents | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>("diff");
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [shown, setShown] = useState<string | null>(null);
  const [round, setRound] = useState(0);

  useEffect(() => {
    let gone = false;
    setData(null);
    setFailed(null);
    void commands.backupContents(serverId, backup.fileName).then(
      (next) => !gone && setData(next),
      (cause) => !gone && setFailed(String(cause)),
    );
    return () => {
      gone = true;
    };
  }, [serverId, backup.fileName, round]);

  const counts = useMemo(() => {
    const out = { Changed: 0, Removed: 0, Added: 0, all: 0 };
    for (const one of data?.entries ?? []) {
      out.all += 1;
      if (one.state in out) out[one.state as "Changed"] += 1;
    }
    return out;
  }, [data]);

  const groups = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const fits = (one: BackupEntry) =>
      (filter === "all" || (filter === "diff" ? one.state !== "Same" : one.state === filter)) &&
      (!needle || one.path.toLowerCase().includes(needle));
    return groupOf((data?.entries ?? []).filter(fits));
  }, [data, filter, query]);

  const toggle = (path: string) =>
    setPicked((was) => {
      const next = new Set(was);
      if (next.has(path)) next.delete(path);
      else {
        for (const one of next) if (one.startsWith(`${path}/`) || path.startsWith(`${one}/`)) next.delete(one);
        next.add(path);
      }
      return next;
    });
  const covered = (path: string) => [...picked].some((one) => path === one || path.startsWith(`${one}/`));

  const row = (one: BackupEntry, label: string) => {
    const restorable = one.state !== "Added";
    const comparable = (one.state === "Changed" || one.state === "Removed") && Number(one.sizeBytes) <= TEXT_LIMIT;
    return (
      <div key={one.path} className="bk-item-wrap">
        <div className={`bk-item ${one.state.toLowerCase()}`}>
          <input
            type="checkbox"
            aria-label={tr("Choose {0}", [one.path])}
            disabled={!restorable}
            checked={covered(one.path)}
            onChange={() => toggle(one.path)}
          />
          <Icon name="file" />
          <span className="bk-name" title={one.path}>{label}</span>
          <span className={`chip subtle bk-state ${one.state.toLowerCase()}`}>{stateLabel(one.state)}</span>
          <code className="bk-size">{fileSize(Number(one.sizeBytes))}</code>
          {comparable ? (
            <button className="button secondary small" aria-expanded={shown === one.path} onClick={() => setShown(shown === one.path ? null : one.path)}>
              {shown === one.path ? tr("Hide") : tr("Compare")}
            </button>
          ) : (
            <span className="bk-spacer" />
          )}
        </div>
        {shown === one.path && <Diff serverId={serverId} backup={backup.fileName} entry={one} />}
      </div>
    );
  };

  const choices: { value: Filter; label: string }[] = [
    { value: "diff", label: `${tr("Differences")} ${counts.Changed + counts.Removed + counts.Added}` },
    { value: "Changed", label: `${tr("Changed")} ${counts.Changed}` },
    { value: "Removed", label: `${tr("Only in backup")} ${counts.Removed}` },
    { value: "Added", label: `${tr("Only now")} ${counts.Added}` },
    { value: "all", label: `${tr("All")} ${counts.all}` },
  ];

  return (
    <div className="tool-content backup-page bk-browser">
      <div className="wb-head">
        <button className="wb-icon" aria-label={tr("Back to backups")} data-tip={tr("Back to backups")} onClick={onBack}>
          <Icon name="arrow-left" />
        </button>
        <div className="wb-head-title">
          <h1>{relativeTime(backup.createdAt)}</h1>
          <code>{backup.fileName}</code>
        </div>
        <span className="live-spacer" />
        <label className="search wb-search-field bk-search">
          <Icon name="search" />
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={tr("Find a file")} />
          {query && (
            <button type="button" className="icon-button" aria-label={tr("Clear search")} onClick={() => setQuery("")}>
              <Icon name="close" />
            </button>
          )}
        </label>
      </div>
      {failed ? (
        <p className="muted tool-empty">{failed}</p>
      ) : !data ? (
        <p className="muted tool-empty">{tr("Comparing the backup with the server…")}</p>
      ) : (
        <>
          <div className="bk-bar">
            <SegmentedControl value={filter} options={choices} onChange={setFilter} />
            {!data.exact && <span className="muted bk-note">{tr("Files of the same size are treated as unchanged on SSH servers.")}</span>}
          </div>
          {groups.length === 0 ? (
            <p className="muted tool-empty">{filter === "diff" && !query ? tr("Nothing differs from the server now.") : tr("No files match.")}</p>
          ) : (
            <div className="bk-list">
              {groups.map((group) => {
                if (!group.folder) return row(group.entries[0], group.name);
                const expanded = open.has(group.name) || !!query;
                const restorable = group.entries.some((one) => one.state !== "Added");
                const differs = group.entries.filter((one) => one.state !== "Same").length;
                return (
                  <div key={group.name} className="bk-group">
                    <div className="bk-item folder">
                      <input
                        type="checkbox"
                        aria-label={tr("Choose {0}", [group.name])}
                        disabled={!restorable}
                        checked={covered(group.name)}
                        onChange={() => toggle(group.name)}
                      />
                      <button
                        className="bk-open"
                        aria-expanded={expanded}
                        onClick={() =>
                          setOpen((was) => {
                            const next = new Set(was);
                            if (next.has(group.name)) next.delete(group.name);
                            else next.add(group.name);
                            return next;
                          })
                        }
                      >
                        <span className={expanded ? "bk-chev" : "bk-chev bk-closed"}>
                          <Icon name="chevron" />
                        </span>
                        <Icon name="folder" />
                        <span className="bk-name">{group.name}/</span>
                      </button>
                      <span className="muted bk-count">
                        {tr("{0, plural, one {# file} other {# files}}", [group.entries.length])}
                        {differs > 0 && filter !== "diff" ? ` · ${tr("{0} differ", [differs])}` : ""}
                      </span>
                      <span className="bk-spacer" />
                    </div>
                    {expanded && (
                      <div className="bk-children">
                        {group.entries.slice(0, SHOWN).map((one) => row(one, one.path.slice(group.name.length + 1)))}
                        {group.entries.length > SHOWN && (
                          <p className="muted bk-more">{tr("{0} more — search to narrow it down", [group.entries.length - SHOWN])}</p>
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
          <div className="bk-foot">
            <span className="muted">
              {picked.size ? tr("{0, plural, one {# item chosen} other {# items chosen}}", [picked.size]) : tr("Choose files or folders to put back.")}
            </span>
            <span className="live-spacer" />
            {picked.size > 0 && (
              <button className="button secondary small" onClick={() => setPicked(new Set())}>
                {tr("Clear")}
              </button>
            )}
            <button
              className="button primary small"
              disabled={!picked.size || !stopped}
              data-tip={!stopped ? tr("Stop the server first.") : undefined}
              onClick={() =>
                void onRestore([...picked]).then((done) => {
                  if (!done) return;
                  setPicked(new Set());
                  setShown(null);
                  setRound((at) => at + 1);
                })
              }
            >
              <Icon name="undo" />
              {tr("Restore chosen")}
            </button>
          </div>
        </>
      )}
    </div>
  );
}

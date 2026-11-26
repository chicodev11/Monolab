import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useState, Fragment } from "react";
import type { RewindEntry, RewindVersions } from "../../lib/bindings";
import { date, relativeTime } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { foldDiff, lineDiff, type DiffRow } from "../../lib/lineDiff";
import { EmptyState, List, ListHeader, ListRow, SearchField, Switch } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { Splitter, usePaneWidth, widthVar } from "../ui/Splitter";
import "./RewindPage.css";

const KIND_LABEL: Record<string, () => string> = {
  added: () => tr("Added"),
  changed: () => tr("Changed"),
  deleted: () => tr("Deleted"),
  restored: () => tr("Restored"),
};

type Compare = "change" | "now";

/** Day headings for the timeline: Today, Yesterday, or the date. */
export function dayLabel(at: number, now = Date.now() / 1000): string {
  const day = (seconds: number) => {
    const one = new Date(seconds * 1000);
    return new Date(one.getFullYear(), one.getMonth(), one.getDate()).getTime();
  };
  const gap = Math.round((day(now) - day(at)) / 86_400_000);
  if (gap === 0) return tr("Today");
  if (gap === 1) return tr("Yesterday");
  return new Date(at * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

function RewindDiff({ versions, entry, compare }: { versions: RewindVersions; entry: RewindEntry; compare: Compare }) {
  const rows = useMemo<DiffRow[] | "big">(() => {
    const before = compare === "now" ? versions.after ?? "" : versions.before ?? "";
    const after = compare === "now" ? versions.current ?? "" : versions.after ?? "";
    const diff = lineDiff(before, after);
    return diff ? foldDiff(diff, 3) : "big";
  }, [versions, compare]);
  if (rows === "big") return <div className="bk-diff muted">{tr("Too many changed lines to compare.")}</div>;
  const same = rows.every((row) => row.kind === "same" || row.kind === "gap");
  const left = compare === "now" ? tr("This version") : entry.kind === "added" ? tr("No file") : tr("Before");
  const right = compare === "now" ? (versions.current === null ? tr("No file now") : tr("Now")) : entry.kind === "deleted" ? tr("No file") : tr("After");
  return (
    <div className="bk-diff">
      <div className="bk-diff-legend">
        <span className="bk-line remove">− {left}</span>
        <span className="bk-line add">+ {right}</span>
      </div>
      {same ? (
        <p className="muted rewind-same">{compare === "now" ? tr("The file is the same as this version now.") : tr("Nothing changed in the text.")}</p>
      ) : (
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
      )}
    </div>
  );
}

/**
 * Rewind: every change to a server's text files, newest first, with a diff and one-click restore.
 * The app keeps the history by itself in the background; worlds, logs and jars are left out.
 */
export function RewindPage({
  serverId,
  onError,
  onNotice,
  confirm,
  file,
}: {
  serverId: string;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
  /** Open on one file: the list is filtered to it and its newest change is shown. */
  file?: string;
}) {
  const listPane = usePaneWidth("rewind-list", 320, 220, 560);
  const [entries, setEntries] = useState<RewindEntry[] | null>(null);
  const [enabled, setEnabled] = useState(true);
  const [query, setQuery] = useState(file ?? "");
  const [picked, setPicked] = useState<RewindEntry | null>(null);
  const [versions, setVersions] = useState<RewindVersions | null>(null);
  const [compare, setCompare] = useState<Compare>("change");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async (scan = false) => {
    try {
      if (scan) await commands.rewindScan(serverId);
      const [list, on] = await Promise.all([commands.rewindList(serverId, null, 500), commands.rewindEnabled(serverId)]);
      setEntries(list);
      setEnabled(on);
    } catch (cause) {
      onError(String(cause));
      setEntries([]);
    }
  }, [serverId, onError]);

  useEffect(() => {
    void load(true);
  }, [load]);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let gone = false;
    void listen<{ serverId: string }>("rewind-changed", (event) => {
      if (event.payload.serverId === serverId) void load();
    })
      .then((unlisten) => {
        if (gone) unlisten();
        else stop = unlisten;
      })
      .catch(() => {});
    return () => {
      gone = true;
      stop?.();
    };
  }, [serverId, load]);

  // Opened on one file: show its newest change once the list is in.
  const [focused, setFocused] = useState(false);
  useEffect(() => {
    if (!file || focused || !entries) return;
    setFocused(true);
    const newest = entries.find((one) => one.path === file);
    if (newest) setPicked(newest);
  }, [file, focused, entries]);

  useEffect(() => {
    if (!picked) return setVersions(null);
    let gone = false;
    setVersions(null);
    commands
      .rewindVersions(serverId, picked.id)
      .then((got) => !gone && setVersions(got))
      .catch((cause) => !gone && onError(String(cause)));
    return () => {
      gone = true;
    };
  }, [serverId, picked, onError]);

  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const kept = (entries ?? []).filter((one) => !needle || one.path.toLowerCase().includes(needle));
    const days = kept.map((one) => dayLabel(one.at));
    return kept.map((one, index) => ({ one, heading: index === 0 || days[index - 1] !== days[index] ? days[index] : null }));
  }, [entries, query]);

  async function restore(side: "before" | "after") {
    if (!picked) return;
    const removes = (side === "before" ? picked.before : picked.after) === null;
    const title = removes ? tr("Delete {0}?", [picked.path]) : side === "before" ? tr("Undo this change to {0}?", [picked.path]) : tr("Put {0} back to this version?", [picked.path]);
    const body = removes
      ? tr("The file didn't exist at that point, so it will be deleted. This is recorded too, so you can bring it back.")
      : tr("The file is replaced with the saved version. What it holds now stays in the history, so you can switch back.");
    if (!(await confirm(title, body, removes ? tr("Delete") : tr("Restore"), removes))) return;
    setBusy(true);
    try {
      const done = await commands.rewindRestore(serverId, picked.id, side);
      onNotice(tr("Restored {0}.", [done.path]));
      await load();
      setPicked(done);
    } catch (cause) {
      onError(String(cause));
    }
    setBusy(false);
  }

  return (
    <div className="git-page rewind-page">
      {!enabled && (
        <div className="msmp-bar rewind-paused">
          <Icon name="history" />
          <span className="muted">{tr("Rewind is paused. Turn it back on in Settings → History.")}</span>
        </div>
      )}
      {entries === null ? (
        <p className="muted rewind-loading">{tr("Loading…")}</p>
      ) : !entries.length ? (
        <EmptyState
          icon="history"
          title={enabled ? tr("No changes yet") : tr("Rewind is paused")}
          hint={enabled ? tr("Edit a config file and it shows up here within a few seconds.") : tr("Turn it on to start keeping changes.")}
        />
      ) : (
        <div className="git-layout with-split" style={widthVar("--side-w", listPane.width)}>
          <div className="git-list">
            <SearchField className="rewind-search fill" value={query} onChange={setQuery} placeholder={tr("Filter by file")} />
            <List variant="plain" density="compact" className="side-rows rewind-rows" empty={null}>
            {shown.map(({ one, heading }) => {
              const name = one.path.split("/").pop() ?? one.path;
              const folder = one.path.slice(0, one.path.length - name.length).replace(/\/$/, "");
              return (
                <Fragment key={one.id}>
                  {heading && <ListHeader className="rewind-day" title={heading} />}
                  <ListRow
                    className="rewind-row"
                    selected={picked?.id === one.id}
                    onClick={() => setPicked(one)}
                    leading={
                      <span className="rewind-mark" aria-hidden>
                        <Icon name={/\.(properties|ya?ml|toml|json5?|conf|cfg|ini)$/i.test(name) ? "file-cog" : "file"} />
                      </span>
                    }
                    title={name}
                    data-tip={one.path}
                    subtitle={
                      <>
                        {folder ? folder.split("/").slice(-2).join("/") : tr("Server folder")} · {relativeTime(one.at)}
                      </>
                    }
                    meta={
                      <span className="rewind-meta">
                        {one.by && <span className="chip subtle rewind-by">{one.by === "host" ? tr("the host") : one.by === "AI" ? tr("AI") : one.by}</span>}
                        <span className={`git-state ${one.kind === "added" ? "added" : one.kind === "deleted" ? "deleted" : ""}`}>{KIND_LABEL[one.kind]?.() ?? one.kind}</span>
                      </span>
                    }
                  />
                </Fragment>
              );
            })}
            </List>
            {!shown.length && <p className="muted rewind-same">{tr("No changes to that file.")}</p>}
          </div>
          <Splitter pane={listPane} side="left" label={tr("Resize the change list")} />
          <div className="git-main">
            {picked ? (
              <>
                <div className="git-main-head">
                  <strong>{picked.path}</strong>
                  <small className="muted">{date(picked.at)}{picked.by ? ` · ${picked.by === "host" ? tr("the host") : picked.by === "AI" ? tr("AI") : picked.by}` : ""}</small>
                  <span className="title-actions">
                    <Dropdown
                      className="rewind-compare-pick"
                      value={compare}
                      onChange={(next) => setCompare(next as Compare)}
                      options={[
                        { value: "change", label: tr("What changed") },
                        { value: "now", label: tr("This version vs now") },
                      ]}
                    />
                    <button type="button" className="button secondary small" disabled={busy} onClick={() => void restore("before")} data-tip={tr("Put the file back as it was before this change")}>
                      <Icon name="undo" />
                      {tr("Undo change")}
                    </button>
                    <button type="button" className="button secondary small" disabled={busy} onClick={() => void restore("after")} data-tip={tr("Put the file back as it was right after this change")}>
                      <Icon name="history" />
                      {tr("Restore this version")}
                    </button>
                  </span>
                </div>
                {versions ? <RewindDiff versions={versions} entry={picked} compare={compare} /> : <div className="bk-diff muted">{tr("Loading…")}</div>}
              </>
            ) : (
              <EmptyState icon="history" title={tr("Pick a change")} hint={tr("See what changed and put the file back with one click.")} />
            )}
          </div>
        </div>
      )}
    </div>
  );
}

/** Settings → History: whether this server keeps every change to its text files. */
export function RewindSettings({ serverId, onError }: { serverId: string; onError: (message: string) => void }) {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  useEffect(() => {
    let gone = false;
    commands.rewindEnabled(serverId).then(
      (on) => !gone && setEnabled(on),
      (cause) => onError(String(cause)),
    );
    return () => {
      gone = true;
    };
  }, [serverId, onError]);
  async function flip(next: boolean) {
    try {
      await commands.setRewindEnabled(serverId, next);
      setEnabled(next);
      if (next) await commands.rewindScan(serverId);
    } catch (cause) {
      onError(String(cause));
    }
  }
  return (
    <section className="settings-group">
      <div className="settings-group-rows">
        <div className="setting-row">
          <div>
            <strong>{tr("Keep changes")}</strong>
            <small>{tr("Every change to configs, scripts and datapack files is kept here, even ones made outside the app. Worlds, logs and jars aren't included, so keep backups for those.")}</small>
          </div>
          <Switch checked={!!enabled} disabled={enabled === null} onChange={(event) => void flip(event.target.checked)} aria-label={tr("Keep changes")} />
        </div>
      </div>
    </section>
  );
}

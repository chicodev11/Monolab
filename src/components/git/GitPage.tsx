import { useCallback, useEffect, useState, type CSSProperties } from "react";
import type { GitCommit, GitFile, GitStatus } from "../../lib/bindings";
import { relativeTime } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { foldDiff, lineDiff, type DiffRow } from "../../lib/lineDiff";
import { EmptyState } from "../primitives";
import { Icon } from "../ui/Icon";

type Shown = { path: string; hash: string; state: string };

const STATE_LABEL: Record<string, () => string> = {
  Changed: () => tr("Changed"),
  Added: () => tr("Added"),
  Deleted: () => tr("Deleted"),
  Renamed: () => tr("Renamed"),
  Untracked: () => tr("New file"),
  Conflict: () => tr("Conflict"),
};

function GitDiffView({ serverId, shown }: { serverId: string; shown: Shown }) {
  const [rows, setRows] = useState<DiffRow[] | "binary" | "big" | null>(null);
  useEffect(() => {
    let gone = false;
    setRows(null);
    commands
      .gitDiff(serverId, shown.path, shown.hash)
      .then((got) => {
        if (gone) return;
        if (got.binary) return setRows("binary");
        const diff = lineDiff(got.before, got.after);
        setRows(diff ? foldDiff(diff) : "big");
      })
      .catch(() => !gone && setRows("binary"));
    return () => {
      gone = true;
    };
  }, [serverId, shown.path, shown.hash]);
  if (rows === null) return <div className="bk-diff muted">{tr("Loading…")}</div>;
  if (rows === "binary") return <div className="bk-diff muted">{tr("Only text files can be compared line by line.")}</div>;
  if (rows === "big") return <div className="bk-diff muted">{tr("Too many changed lines to compare.")}</div>;
  return (
    <div className="bk-diff">
      <div className="bk-diff-legend">
        <span className="bk-line remove">− {shown.hash ? tr("Before the commit") : tr("Last commit")}</span>
        <span className="bk-line add">+ {shown.hash ? tr("This commit") : tr("Now")}</span>
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

/** Git for a server folder that is already a repository: changes, commit, history, restore, pull and push. */
export function GitPage({
  serverId,
  onError,
  onNotice,
  confirm,
}: {
  serverId: string;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
}) {
  const [status, setStatus] = useState<GitStatus | null>(null);
  const [commits, setCommits] = useState<GitCommit[]>([]);
  const [view, setView] = useState<"changes" | "history">("changes");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [message, setMessage] = useState("");
  const [shown, setShown] = useState<Shown | null>(null);
  const [opened, setOpened] = useState<{ commit: GitCommit; files: GitFile[] } | null>(null);
  const [busy, setBusy] = useState("");

  const load = useCallback(async () => {
    try {
      const [next, log] = await Promise.all([commands.gitStatus(serverId), commands.gitLog(serverId).catch(() => [] as GitCommit[])]);
      setStatus(next);
      setCommits(log);
      setPicked((was) => new Set([...was].filter((path) => next.files.some((one) => one.path === path))));
    } catch (cause) {
      onError(String(cause));
      setStatus((was) => was ?? { repo: false, branch: "", upstream: "", ahead: 0, behind: 0, files: [], hasIgnore: true });
    }
  }, [serverId, onError]);
  useEffect(() => {
    void load();
  }, [load]);

  const act = async (name: string, work: () => Promise<unknown>, done?: string) => {
    setBusy(name);
    try {
      await work();
      if (done) onNotice(done);
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy("");
      void load();
    }
  };

  if (!status) return <p className="muted lag-note">{tr("Loading…")}</p>;
  if (!status.repo) {
    return (
      <EmptyState
        icon="git"
        title={tr("This server folder isn't a git repository")}
        hint={tr("Starts a repository in the server folder and adds a .gitignore that leaves out worlds, logs and jar files.")}
        action={
          <button type="button" className="button primary small" disabled={!!busy} onClick={() => void act("init", () => commands.gitInit(serverId), tr("Started a git repository."))}>
            <Icon name="git" />
            {tr("Start git")}
          </button>
        }
      />
    );
  }

  const files = status.files;
  const allPicked = files.length > 0 && files.every((one) => picked.has(one.path));
  const toggle = (path: string) =>
    setPicked((was) => {
      const next = new Set(was);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  const restorable = (one: { state: string }) => one.state === "Changed" || one.state === "Deleted";

  return (
    <div className="git-page">
      <div className="git-bar">
        <span className="git-branch">
          <Icon name="git" />
          <strong>{status.branch || "—"}</strong>
          {status.upstream && <small>→ {status.upstream}</small>}
          {status.ahead > 0 && <small>↑{status.ahead}</small>}
          {status.behind > 0 && <small>↓{status.behind}</small>}
        </span>
        <div className="wb-seg slide" role="tablist" aria-label={tr("View")} style={{ "--at": view === "history" ? 1 : 0 } as CSSProperties}>
          {(
            [
              ["changes", `${tr("Changes")} ${files.length}`],
              ["history", tr("History")],
            ] as const
          ).map(([value, label]) => (
            <button key={value} type="button" role="tab" aria-selected={view === value} className={view === value ? "on" : ""} onClick={() => setView(value)}>
              {label}
            </button>
          ))}
        </div>
        <span className="live-spacer" />
        <button type="button" className="button secondary small" disabled={!!busy || !status.upstream} data-tip={status.upstream ? undefined : tr("No remote branch is set for this branch.")} onClick={() => void act("pull", () => commands.gitSync(serverId, false), tr("Pulled the latest commits."))}>
          <Icon name="import" />
          {busy === "pull" ? tr("Pulling…") : tr("Pull")}
        </button>
        <button type="button" className="button secondary small" disabled={!!busy || !status.upstream} onClick={() => void act("push", () => commands.gitSync(serverId, true), tr("Pushed your commits."))}>
          <Icon name="upload" />
          {busy === "push" ? tr("Pushing…") : tr("Push")}
        </button>
        <button type="button" className="wb-icon" aria-label={tr("Refresh")} data-tip={tr("Refresh")} onClick={() => void load()}>
          <Icon name="refresh" />
        </button>
      </div>
      {!status.hasIgnore && (
        <div className="git-note">
          <Icon name="info" />
          <span>{tr("No .gitignore yet. Worlds, logs and jar files would make the repository very large.")}</span>
          <span className="live-spacer" />
          <button type="button" className="button secondary small" disabled={!!busy} onClick={() => void act("ignore", () => commands.gitAddIgnore(serverId), tr("Created .gitignore."))}>
            {tr("Create .gitignore")}
          </button>
        </div>
      )}
      <div className="git-layout">
        <aside className="git-list">
          {view === "changes" ? (
            files.length === 0 ? (
              <p className="integ-nav-empty">{tr("No changes since the last commit.")}</p>
            ) : (
              <>
                <label className="git-row git-all">
                  <input type="checkbox" checked={allPicked} onChange={() => setPicked(allPicked ? new Set() : new Set(files.map((one) => one.path)))} />
                  <span>{tr("Select all")}</span>
                </label>
                {files.map((one) => (
                  <div key={one.path} className={`git-row ${shown && !shown.hash && shown.path === one.path ? "on" : ""}`} onClick={() => setShown({ path: one.path, hash: "", state: one.state })}>
                    <input type="checkbox" checked={picked.has(one.path)} onClick={(event) => event.stopPropagation()} onChange={() => toggle(one.path)} aria-label={one.path} />
                    <span className="git-path">{one.path}</span>
                    <span className={`git-state ${one.state.toLowerCase()}`}>{STATE_LABEL[one.state]?.() ?? one.state}</span>
                  </div>
                ))}
              </>
            )
          ) : opened ? (
            <>
              <button type="button" className="git-row git-back" onClick={() => setOpened(null)}>
                <Icon name="arrow-left" />
                <span className="git-path">
                  {opened.commit.subject}
                  <small>
                    {opened.commit.short} · {opened.commit.author}
                  </small>
                </span>
              </button>
              {opened.files.map((one) => (
                <div key={one.path} className={`git-row ${shown?.hash === opened.commit.hash && shown.path === one.path ? "on" : ""}`} onClick={() => setShown({ path: one.path, hash: opened.commit.hash, state: one.state })}>
                  <span className="git-path">{one.path}</span>
                  <span className={`git-state ${one.state.toLowerCase()}`}>{STATE_LABEL[one.state]?.() ?? one.state}</span>
                </div>
              ))}
            </>
          ) : commits.length === 0 ? (
            <p className="integ-nav-empty">{tr("No commits yet.")}</p>
          ) : (
            commits.map((one) => (
              <button
                type="button"
                key={one.hash}
                className="git-row git-commit"
                onClick={() =>
                  void commands
                    .gitCommitFiles(serverId, one.hash)
                    .then((got) => {
                      setOpened({ commit: one, files: got });
                      if (got[0]) setShown({ path: got[0].path, hash: one.hash, state: got[0].state });
                    })
                    .catch((cause) => onError(String(cause)))
                }
              >
                <span className="git-path">
                  {one.subject}
                  <small>
                    {one.short} · {one.author} · {relativeTime(one.at)}
                  </small>
                </span>
              </button>
            ))
          )}
          {view === "changes" && files.length > 0 && (
            <div className="git-commit-box">
              <textarea value={message} placeholder={tr("Commit message")} rows={3} onChange={(event) => setMessage(event.target.value)} />
              <button
                type="button"
                className="button primary small"
                disabled={!picked.size || !message.trim() || !!busy}
                onClick={() =>
                  void act(
                    "commit",
                    async () => {
                      await commands.gitCommit(serverId, [...picked], message);
                      setMessage("");
                      setPicked(new Set());
                      setShown(null);
                    },
                    tr("Committed {0, plural, one {# file} other {# files}}.", [picked.size]),
                  )
                }
              >
                <Icon name="check" />
                {busy === "commit" ? tr("Committing…") : tr("Commit {0, plural, one {# file} other {# files}}", [picked.size])}
              </button>
            </div>
          )}
        </aside>
        <section className="git-main">
          {shown ? (
            <>
              <div className="git-main-head">
                <strong>{shown.path}</strong>
                <span className="live-spacer" />
                {!shown.hash && restorable(shown) && (
                  <button
                    type="button"
                    className="button ghost small"
                    disabled={!!busy}
                    onClick={async () => {
                      if (!(await confirm(tr("Restore {0}?", [shown.path]), tr("This file goes back to the last commit. Its current version is backed up first."), tr("Restore"), true))) return;
                      await act("restore", () => commands.gitRestore(serverId, [shown.path]), tr("Restored {0}.", [shown.path]));
                      setShown(null);
                    }}
                  >
                    <Icon name="undo" />
                    {tr("Restore")}
                  </button>
                )}
              </div>
              <GitDiffView serverId={serverId} shown={shown} />
            </>
          ) : (
            <p className="muted git-hint">{view === "changes" ? tr("Pick a file to see what changed.") : tr("Pick a commit to see its files.")}</p>
          )}
        </section>
      </div>
    </div>
  );
}

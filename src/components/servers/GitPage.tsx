import { useCallback, useEffect, useState } from "react";
import { commands, type DevRepo, type GitCommit, type GitState } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Button, Card, EmptyState, List, ListRow } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import "./GitPage.css";

type Confirm = (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;

/** What a status code is called on screen. */
function stateWord(state: string) {
  if (state === "new") return tr("New");
  if (state === "deleted") return tr("Deleted");
  if (state === "added") return tr("Added");
  if (state === "renamed") return tr("Renamed");
  return tr("Changed");
}

function when(at: number) {
  return new Date(at * 1000).toLocaleString();
}

/** A unified diff, one line per row, added/removed lines marked. */
function DiffView({ text }: { text: string }) {
  if (!text.trim()) return <p className="muted">{tr("No differences.")}</p>;
  return (
    <pre className="git-diff">
      {text.split("\n").map((line, index) => {
        const kind = line.startsWith("+++") || line.startsWith("---") || line.startsWith("diff ") || line.startsWith("index ")
          ? "meta"
          : line.startsWith("@@")
            ? "hunk"
            : line.startsWith("+")
              ? "add"
              : line.startsWith("-")
                ? "del"
                : "";
        return (
          <span key={index} className={kind ? `git-line ${kind}` : "git-line"}>
            {line || " "}
            {"\n"}
          </span>
        );
      })}
    </pre>
  );
}

/**
 * Git for one server: the folder's history (changes, commits, diffs, putting files back, push/pull) and
 * plugin repositories that build straight into the server.
 */
export function GitPage({
  serverId,
  onSsh = false,
  onError,
  onNotice,
  confirm,
}: {
  serverId: string;
  /** A server on an SSH machine: git runs there. */
  onSsh?: boolean;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: Confirm;
}) {
  const [state, setState] = useState<GitState | null>(null);
  const [log, setLog] = useState<GitCommit[]>([]);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState("");
  // The diff that is open, shown right under the row it belongs to (`anchor`: a changed file's path or a commit hash).
  const [diff, setDiff] = useState<{ anchor: string; text: string; commit: string | null; path: string | null } | null>(null);
  const [remote, setRemote] = useState<string | null>(null);
  const [repos, setRepos] = useState<DevRepo[]>([]);
  const [repoUrl, setRepoUrl] = useState("");
  const [buildLog, setBuildLog] = useState<{ repo: string; ok: boolean; text: string } | null>(null);

  const load = useCallback(() => {
    void commands.gitState(serverId).then(setState, (cause) => onError(String(cause)));
    void commands.gitLog(serverId, 50).then(setLog, () => setLog([]));
    void commands.devRepos(serverId).then(setRepos, () => setRepos([]));
  }, [serverId, onError]);
  useEffect(load, [load]);

  async function work<T>(label: string, job: () => Promise<T>, done?: (value: T) => void) {
    setBusy(label);
    try {
      const value = await job();
      done?.(value);
    } catch (cause) {
      onError(String(cause));
    }
    setBusy("");
    load();
  }

  /** Opens (or, clicked again, closes) a diff under its row. */
  function showDiff(anchor: string, commit: string | null, path: string | null) {
    if (diff && diff.anchor === anchor && diff.path === path) {
      setDiff(null);
      return;
    }
    void commands.gitDiff(serverId, commit, path).then((text) => setDiff({ anchor, text, commit, path }), (cause) => onError(String(cause)));
  }

  const openAt = (anchor: string) => diff?.anchor === anchor;

  /** The open diff, under its row: for a commit, its files to narrow it down and put one back. */
  const diffPanel = (anchor: string) => {
    if (!diff || diff.anchor !== anchor) return null;
    const files = diff.commit ? [...new Set(diff.text.split("\n").filter((line) => line.startsWith("+++ b/")).map((line) => line.slice(6)))] : [];
    return (
      <div className="git-inline">
        {diff.commit && (files.length > 1 || diff.path) && (
          <div className="git-file-picks">
            {files.length > 1 && !diff.path && <small className="muted">{tr("Pick a file to see only it or put it back:")}</small>}
            {diff.path ? (
              <>
                <Button size="sm" icon="arrow-left" onClick={() => showDiff(anchor, diff.commit, null)}>
                  {tr("All files")}
                </Button>
                <Button size="sm" icon="undo" onClick={() => void putBack(diff.path ?? "", diff.commit)}>
                  {tr("Put back this version")}
                </Button>
              </>
            ) : (
              files.map((path) => (
                <Button key={path} size="sm" onClick={() => showDiff(anchor, diff.commit, path)}>
                  {path}
                </Button>
              ))
            )}
          </div>
        )}
        {diff.commit && files.length === 1 && !diff.path && (
          <div className="git-file-picks">
            <Button size="sm" icon="undo" onClick={() => void putBack(files[0], diff.commit)}>
              {tr("Put back {0} as it was here", [files[0]])}
            </Button>
          </div>
        )}
        <DiffView text={diff.text} />
      </div>
    );
  };

  async function putBack(path: string, commit: string | null) {
    const body = commit
      ? tr("{0} goes back to how it was in this commit. The current version is saved in the history first.", [path])
      : tr("{0} goes back to the last commit. The current version is saved in the history first.", [path]);
    if (!(await confirm(tr("Put back {0}?", [path]), body, tr("Put back")))) return;
    await work("restore", () => commands.gitRestore(serverId, path, commit), () => {
      onNotice(tr("Put back {0}.", [path]));
      setDiff(null);
    });
  }

  if (!state)
    return (
      <div className="page-body git-page">
        <div className="git-loading">
          <Icon name="git" />
          {tr("Loading…")}
        </div>
      </div>
    );

  if (!state.installed) {
    return (
      <div className="page-body git-page">
        <EmptyState
          icon="git"
          title={onSsh ? tr("Git isn't installed on the SSH server") : tr("Git isn't installed")}
          hint={onSsh ? tr("Install it there (for example sudo apt install git), then come back.") : tr("Install Git from git-scm.com, then come back. Monolab uses it to keep this server's history.")}
          action={<Button icon="refresh" onClick={load}>{tr("Check again")}</Button>}
        />
      </div>
    );
  }

  return (
    <div className="page-body git-page">
        {!state.repo ? (
          <Card title={tr("Git history")}>
            <div className="git-start">
              <p className="muted">
                {tr("Keep a history of this server's settings: see what changed, when, and put a file back. Worlds, logs and jars stay out (backups cover worlds).")}
              </p>
              <Button variant="primary" icon="git" disabled={busy !== ""} onClick={() => void work("start", () => commands.gitStart(serverId), () => onNotice(tr("History started.")))}>
                {tr("Start history")}
              </Button>
            </div>
          </Card>
        ) : (
          <>
            <Card
              title={tr("Changes")}
              aside={
                <span className="git-branch">
                  <Icon name="git" />
                  {state.branch}
                  {state.remote && (state.ahead > 0 || state.behind > 0) && (
                    <small className="muted">
                      {state.ahead > 0 && ` ↑${state.ahead}`}
                      {state.behind > 0 && ` ↓${state.behind}`}
                    </small>
                  )}
                </span>
              }
            >
              {state.changes.length === 0 ? (
                <p className="muted">{tr("Nothing changed since the last commit.")}</p>
              ) : (
                <>
                  <List variant="box" density="compact" className="git-files">
                    {state.changes.map((change) => (
                      <ListRow
                        key={change.path}
                        className={openAt(change.path) ? "open" : undefined}
                        leading={<span className="chip">{stateWord(change.state)}</span>}
                        title={<span className="git-file-path">{change.path}</span>}
                        onClick={change.state === "new" ? undefined : () => showDiff(change.path, null, change.path)}
                        mainProps={{
                          "aria-expanded": change.state === "new" ? undefined : openAt(change.path),
                          "data-tip": change.state === "new" ? tr("New file: commit it to start its history") : tr("Show what changed"),
                        }}
                        meta={change.state !== "new" ? <span className="git-chevron"><Icon name="chevron" /></span> : undefined}
                        actions={
                          change.state !== "new" && (
                            <button type="button" className="wb-icon ghost" aria-label={tr("Put back {0}", [change.path])} data-tip={tr("Undo this change")} onClick={() => void putBack(change.path, null)}>
                              <Icon name="undo" />
                            </button>
                          )
                        }
                        detail={diffPanel(change.path)}
                      />
                    ))}
                  </List>
                  <form
                    className="git-commit"
                    onSubmit={(event) => {
                      event.preventDefault();
                      void work("commit", () => commands.gitCommit(serverId, message), () => {
                        setMessage("");
                        onNotice(tr("Saved to history."));
                      });
                    }}
                  >
                    <input value={message} placeholder={tr("What changed? (optional)")} aria-label={tr("Commit message")} onChange={(event) => setMessage(event.target.value)} />
                    <Button type="submit" variant="primary" disabled={busy !== ""}>
                      {tr("Commit {0}", [state.changes.length])}
                    </Button>
                  </form>
                </>
              )}
            </Card>

            <Card title={tr("Remote")}>
              <div className="git-remote">
                {remote === null ? (
                  <>
                    <span className={state.remote ? "git-url" : "muted"}>{state.remote || tr("Not pushed anywhere. Add a GitHub (or other) repository to keep a copy online.")}</span>
                    <span className="git-actions">
                      <Button size="sm" onClick={() => setRemote(state.remote)}>
                        {state.remote ? tr("Change") : tr("Add remote")}
                      </Button>
                      {state.remote && (
                        <>
                          <Button size="sm" icon="import" disabled={busy !== ""} onClick={() => void work("pull", () => commands.gitPull(serverId), () => onNotice(tr("Pulled.")))}>
                            {tr("Pull")}
                          </Button>
                          <Button size="sm" variant="primary" icon="upload" disabled={busy !== ""} onClick={() => void work("push", () => commands.gitPush(serverId), () => onNotice(tr("Pushed.")))}>
                            {busy === "push" ? tr("Pushing…") : tr("Push")}
                          </Button>
                        </>
                      )}
                    </span>
                  </>
                ) : (
                  <form
                    className="git-commit"
                    onSubmit={(event) => {
                      event.preventDefault();
                      void work("remote", () => commands.gitSetRemote(serverId, remote), () => setRemote(null));
                    }}
                  >
                    <input value={remote} autoFocus spellCheck={false} placeholder="https://github.com/you/server.git" aria-label={tr("Remote address")} onChange={(event) => setRemote(event.target.value)} />
                    <Button type="submit" variant="primary">{tr("Save")}</Button>
                    <Button onClick={() => setRemote(null)}>{tr("Cancel")}</Button>
                  </form>
                )}
              </div>
              <small className="muted">{tr("Sign-in uses Git's own (Git Credential Manager or SSH keys). Check the repository is private: server settings can hold passwords.")}</small>
            </Card>

            <Card title={tr("Commits")} aside={<span className="memo-count">{log.length}</span>}>
              {log.length === 0 ? (
                <p className="muted">{tr("No commits yet.")}</p>
              ) : (
                <List variant="box" density="compact" className="git-log">
                  {log.map((commit) => (
                    <ListRow
                      key={commit.hash}
                      className={openAt(commit.hash) ? "open" : undefined}
                      leading={<code className="git-hash">{commit.short}</code>}
                      title={commit.message}
                      subtitle={<>{commit.author} · {when(commit.at)} · {commit.files === 1 ? tr("1 file") : tr("{0} files", [commit.files])}</>}
                      onClick={() => showDiff(commit.hash, commit.hash, null)}
                      mainProps={{ "aria-expanded": openAt(commit.hash), "data-tip": tr("Show what this commit changed") }}
                      detail={diffPanel(commit.hash)}
                    />
                  ))}
                </List>
              )}
            </Card>
          </>
        )}


        <Card title={tr("Plugin repositories")}>
          <p className="muted">{tr("Link the source of a plugin or mod you make. Build pulls it, builds it and puts the new jar into this server (on the next start if it's running).")}</p>
          {repos.length > 0 && (
            <ul className="git-repos">
              {repos.map((repo) => (
                <li key={repo.id}>
                  <div className="git-repo-head">
                    <span className="git-url">{repo.url}</span>
                    <Dropdown
                      value={repo.build}
                      onChange={(next) => void work("set", () => commands.devRepoSet(repo.id, next, (repo.command ?? ""), (repo.output ?? "")))}
                      options={[
                        { value: "gradle", label: "Gradle" },
                        { value: "maven", label: "Maven" },
                        { value: "custom", label: tr("Command") },
                      ]}
                    />
                    <Button
                      size="sm"
                      variant="primary"
                      icon="hammer"
                      disabled={busy !== ""}
                      onClick={() =>
                        void work("build", () => commands.devRepoBuild(repo.id), (result) => {
                          setBuildLog({ repo: repo.id, ok: result.ok, text: result.log });
                          if (result.ok) onNotice(tr("Built and added {0}.", [result.jar]));
                        })
                      }
                    >
                      {busy === "build" ? tr("Building…") : tr("Build")}
                    </Button>
                    <button
                      type="button"
                      className="wb-icon ghost"
                      aria-label={tr("Unlink {0}", [repo.url])}
                      data-tip={tr("Unlink")}
                      onClick={async () => {
                        if (await confirm(tr("Unlink this repository?"), tr("Its copy on this PC is moved aside, not deleted. The server keeps the jars it already has."), tr("Unlink")))
                          void work("remove", () => commands.devRepoRemove(repo.id));
                      }}
                    >
                      <Icon name="close" />
                    </button>
                  </div>
                  {repo.build === "custom" && (
                    <RepoField label={tr("Build command")} value={(repo.command ?? "")} placeholder="./build.sh" onSave={(next) => void work("set", () => commands.devRepoSet(repo.id, repo.build, next, (repo.output ?? "")))} />
                  )}
                  <RepoField label={tr("Jar folder")} value={(repo.output ?? "")} placeholder="build/libs" onSave={(next) => void work("set", () => commands.devRepoSet(repo.id, repo.build, (repo.command ?? ""), next))} />
                  {(repo.lastBuild ?? 0) > 0 && (
                    <small className="muted">
                      {tr("Last build {0}: {1}", [when((repo.lastBuild ?? 0)), (repo.lastJar ?? "")])}
                    </small>
                  )}
                  {buildLog?.repo === repo.id && (
                    <pre className={`git-diff git-build ${buildLog.ok ? "" : "failed"}`}>{buildLog.text}</pre>
                  )}
                </li>
              ))}
            </ul>
          )}
          <form
            className="git-commit"
            onSubmit={(event) => {
              event.preventDefault();
              void work("clone", () => commands.devRepoAdd(serverId, repoUrl), (repo) => {
                setRepoUrl("");
                onNotice(tr("Linked {0}.", [repo.url]));
              });
            }}
          >
            <input value={repoUrl} spellCheck={false} placeholder="https://github.com/you/plugin.git" aria-label={tr("Repository address")} onChange={(event) => setRepoUrl(event.target.value)} />
            <Button type="submit" disabled={!repoUrl.trim() || busy !== ""}>
              {busy === "clone" ? tr("Cloning…") : tr("Link")}
            </Button>
          </form>
        </Card>
    </div>
  );
}

function RepoField({ label, value, placeholder, onSave }: { label: string; value: string; placeholder: string; onSave: (next: string) => void }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <label className="git-field">
      <span>{label}</span>
      <input value={draft} placeholder={placeholder} spellCheck={false} onChange={(event) => setDraft(event.target.value)} onBlur={() => draft !== value && onSave(draft)} />
    </label>
  );
}

import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { commands, type Sent } from "../../lib/ipc";
import type { BoxInfo, FoundFolder, HostKey, RemoteEntry, SshHost, SshHostView } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Icon } from "../ui/Icon";

type Host = Sent<SshHostView>;

type Draft = {
  id: string;
  name: string;
  address: string;
  port: number;
  username: string;
  auth: string;
  keyPath: string;
  fingerprint: string;
  root: string;
  secret: string;
  existing: boolean;
};

const blank = (): Draft => ({
  id: `host-${Date.now().toString(36)}`,
  name: "",
  address: "",
  port: 22,
  username: "",
  auth: "password",
  keyPath: "",
  fingerprint: "",
  root: "",
  secret: "",
  existing: false,
});

function stored(one: Host | Draft): SshHost {
  return {
    id: one.id,
    name: one.name.trim() || one.address.trim(),
    address: one.address.trim(),
    port: one.port,
    username: one.username.trim(),
    auth: one.auth,
    keyPath: one.keyPath,
    fingerprint: one.fingerprint,
    root: one.root.trim(),
  };
}

export function SshHosts({
  confirm,
  onError,
  onNotice,
  onImported,
  onChanged,
  startAdding = false,
}: {
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  onImported: () => void;
  onChanged?: () => void;
  startAdding?: boolean;
}) {
  const [hosts, setHosts] = useState<Host[]>([]);
  const [draft, setDraft] = useState<Draft | null>(() => (startAdding ? blank() : null));
  const [key, setKey] = useState<HostKey | null>(null);
  const [busy, setBusy] = useState("");
  const [seen, setSeen] = useState<Record<string, BoxInfo>>({});
  const [scanned, setScanned] = useState<Record<string, FoundFolder[]>>({});
  const [picking, setPicking] = useState("");

  const refresh = () =>
    commands
      .listSshHosts()
      .then(setHosts)
      .catch((error: unknown) => onError(String(error)));

  useEffect(() => {
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const edit = (one: Host) => {
    setKey(null);
    setPicking("");
    setDraft({
      id: one.id,
      name: one.name,
      address: one.address,
      port: one.port,
      username: one.username,
      auth: one.auth || "key",
      keyPath: one.keyPath,
      fingerprint: one.fingerprint,
      root: one.root,
      secret: "",
      existing: true,
    });
  };

  const probe = async () => {
    if (!draft) return;
    setBusy("probe");
    setKey(null);
    try {
      const found = await commands.probeSshHost(draft.address.trim(), draft.port);
      setKey(found);
      setDraft({ ...draft, fingerprint: found.fingerprint });
    } catch (error) {
      onError(String(error));
    } finally {
      setBusy("");
    }
  };

  const save = async () => {
    if (!draft) return;
    setBusy("save");
    try {
      await commands.saveSshHost(stored(draft), draft.secret ? draft.secret : null);
      const fresh = !draft.existing;
      const id = draft.id;
      setDraft(null);
      setKey(null);
      await refresh();
      onChanged?.();
      if (fresh && !draft.root) {
        try {
          const root = await commands.ensureSshRoot(id);
          await refresh();
          onNotice(tr("SSH server saved. Server folder created at {0}.", [root]));
        } catch (error) {
          onError(String(error));
          setPicking(id);
        }
      } else {
        onNotice(tr("SSH server saved."));
      }
    } catch (error) {
      onError(String(error));
    } finally {
      setBusy("");
    }
  };

  const setRoot = async (one: Host, root: string) => {
    try {
      await commands.saveSshHost({ ...stored(one), root }, null);
      setPicking("");
      setScanned((have) => {
        const next = { ...have };
        delete next[one.id];
        return next;
      });
      await refresh();
      onNotice(tr("Server folder set to {0}.", [root || tr("Home folder")]));
    } catch (error) {
      onError(String(error));
    }
  };

  const test = async (id: string) => {
    setBusy(id);
    try {
      const info = await commands.testSshHost(id);
      setSeen((have) => ({ ...have, [id]: info }));
    } catch (error) {
      onError(String(error));
    } finally {
      setBusy("");
    }
  };

  const sweep = async (id: string) => {
    setBusy(`scan-${id}`);
    try {
      const folders = await commands.scanRemoteServers(id);
      let added = 0;
      for (const one of folders) {
        try {
          await commands.importRemoteInstance(id, one.path);
          added += 1;
        } catch {}
      }
      setScanned((have) => ({ ...have, [id]: folders }));
      await refresh();
      onNotice(
        added > 0
          ? tr("{0, plural, one {Imported # server.} other {Imported # servers.}}", [String(added)])
          : folders.length > 0
            ? tr("{0, plural, one {The # server found is already registered.} other {All # servers found are already registered.}}", [String(folders.length)])
            : tr("No servers found under that folder."),
      );
      if (added > 0) onImported();
    } catch (error) {
      onError(String(error));
    } finally {
      setBusy("");
    }
  };

  const remove = async (one: Host) => {
    const ok = await confirm(
      tr("Remove {0}?", [one.name]),
      tr("Removes the connection and its imported servers from the list. Files and running servers on the SSH server stay unchanged."),
      tr("Remove"),
      true,
    );
    if (!ok) return;
    try {
      const gone = await commands.deleteSshHost(one.id);
      await refresh();
      onChanged?.();
      onImported();
      if (gone > 0) onNotice(tr("Removed the connection and {0, plural, one {# server} other {# servers}} from the list.", [gone]));
    } catch (error) {
      onError(String(error));
    }
  };

  if (draft) {
    return (
      <HostForm
        draft={draft}
        setDraft={setDraft}
        hostKey={key}
        busy={busy}
        onProbe={() => void probe()}
        onSave={() => void save()}
        onCancel={() => {
          setDraft(null);
          setKey(null);
        }}
      />
    );
  }

  return (
    <div className="ssh-cards">
      {hosts.map((one) => (
        <section className="ssh-card" key={one.id}>
          <header className="ssh-card-head">
            <span className="ssh-card-icon" aria-hidden>
              <Icon name="server" />
            </span>
            <div className="ssh-card-title">
              <strong>{one.name}</strong>
              <small>
                {one.username}@{one.address}
                {one.port === 22 ? "" : `:${one.port}`} ·{" "}
                {one.auth === "password" ? tr("Password") : tr("Key file")}
              </small>
            </div>
            <div className="ssh-card-actions">
              <button
                className="button secondary small"
                type="button"
                disabled={busy === one.id}
                onClick={() => void test(one.id)}
              >
                <Icon name="plug" />
                {busy === one.id ? tr("Connecting…") : tr("Test connection")}
              </button>
              <button
                className="icon-button"
                type="button"
                aria-label={tr("Edit")}
                data-tip={tr("Edit")}
                onClick={() => edit(one)}
              >
                <Icon name="pencil" />
              </button>
              <button
                className="icon-button danger"
                type="button"
                aria-label={tr("Remove")}
                data-tip={tr("Remove")}
                onClick={() => void remove(one)}
              >
                <Icon name="trash" />
              </button>
            </div>
          </header>

          <div className={`ssh-root-row ${one.root ? "" : "unset"}`}>
            <Icon name="folder" />
            <div className="ssh-root-text">
              <small>{tr("Servers folder")}</small>
              {one.root ? (
                <code title={one.root}>{one.root}</code>
              ) : (
                <em>{tr("Not set yet · Find servers creates a monolab folder in your home folder")}</em>
              )}
            </div>
            <button
              className="button ghost small"
              type="button"
              onClick={() => setPicking(picking === one.id ? "" : one.id)}
            >
              {picking === one.id ? tr("Close") : one.root ? tr("Change") : tr("Choose folder")}
            </button>
            <button
              className="button primary small"
              type="button"
              disabled={busy === `scan-${one.id}`}
              onClick={() => void sweep(one.id)}
            >
              <Icon name="search" />
              {busy === `scan-${one.id}` ? tr("Searching…") : tr("Find servers")}
            </button>
          </div>

          {picking === one.id && (
            <FolderPicker
              hostId={one.id}
              start={one.root}
              onError={onError}
              onPick={(path) => void setRoot(one, path)}
            />
          )}

          {scanned[one.id] && scanned[one.id].length > 0 && (
            <div className="ssh-scan">
              <small className="ssh-found">
                {tr("{0, plural, one {# server} other {# servers}} found in the server folder", [String(scanned[one.id].length)])}
              </small>
              {scanned[one.id].map((found) => (
                <div className="ssh-found-row" key={found.path}>
                  <code title={found.path}>{found.name}</code>
                  <small className="muted">
                    {found.core} {found.version} · :{found.port}
                  </small>
                </div>
              ))}
            </div>
          )}
          {seen[one.id] && <BoxSummary info={seen[one.id]} />}
        </section>
      ))}

      <button className="ssh-add-card" type="button" onClick={() => setDraft(blank())}>
        <Icon name="plus" />
        <span>
          <strong>{tr("Add SSH server")}</strong>
          <small>{tr("Connect to servers on another computer, such as an Ubuntu machine, over SSH. Nothing is installed on that computer.")}</small>
        </span>
      </button>
    </div>
  );
}

function HostForm({
  draft,
  setDraft,
  hostKey,
  busy,
  onProbe,
  onSave,
  onCancel,
}: {
  draft: Draft;
  setDraft: (next: Draft) => void;
  hostKey: HostKey | null;
  busy: string;
  onProbe: () => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const addressDone = !!draft.address.trim() && !!draft.username.trim();
  const keyDone = !!draft.fingerprint;
  return (
    <div className="ssh-form">
      <div className="ssh-form-title">
        <strong>{draft.existing ? tr("Edit {0}", [draft.name || draft.address]) : tr("New SSH connection")}</strong>
      </div>

      <section className={`ssh-step ${addressDone ? "done" : ""}`}>
        <div className="ssh-step-head">
          <span className="ssh-step-no">{addressDone ? <Icon name="check" /> : "1"}</span>
          <strong>{tr("Address")}</strong>
        </div>
        <div className="ssh-fields">
          <label className="field-block ssh-wide">
            <span>{tr("Name")}</span>
            <input
              value={draft.name}
              placeholder={tr("Living-room Ubuntu")}
              onChange={(event) => setDraft({ ...draft, name: event.target.value })}
            />
          </label>
          <label className="field-block">
            <span>{tr("Address")}</span>
            <input
              value={draft.address}
              placeholder="192.168.0.10"
              onChange={(event) => setDraft({ ...draft, address: event.target.value, fingerprint: "" })}
            />
          </label>
          <label className="field-block ssh-port">
            <span>{tr("Port")}</span>
            <input
              type="number"
              value={draft.port}
              onChange={(event) =>
                setDraft({ ...draft, port: Number(event.target.value) || 22, fingerprint: "" })
              }
            />
          </label>
          <label className="field-block ssh-wide">
            <span>{tr("Username")}</span>
            <input
              value={draft.username}
              placeholder="ubuntu"
              onChange={(event) => setDraft({ ...draft, username: event.target.value })}
            />
          </label>
        </div>
      </section>

      <section className={`ssh-step ${keyDone ? "done" : ""}`}>
        <div className="ssh-step-head">
          <span className="ssh-step-no">{keyDone ? <Icon name="check" /> : "2"}</span>
          <strong>{tr("Check host key")}</strong>
          <button
            className="button secondary small"
            type="button"
            disabled={!draft.address.trim() || busy === "probe"}
            onClick={onProbe}
          >
            <Icon name="shield" />
            {busy === "probe" ? tr("Checking…") : keyDone ? tr("Check again") : tr("Get host key")}
          </button>
        </div>
        {hostKey ? (
          <div className={`ssh-fingerprint ${hostKey.changed ? "warn" : ""}`}>
            <code>
              {hostKey.algorithm} {hostKey.fingerprint}
            </code>
            <small>
              {hostKey.changed
                ? tr("⚠ The fingerprint has changed since this server was added. Don't save unless you reinstalled the SSH server.")
                : tr("Compare the fingerprint on the SSH server using `ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub`.")}
            </small>
          </div>
        ) : keyDone ? (
          <small className="ssh-step-note">
            <code>{draft.fingerprint}</code>
          </small>
        ) : (
          <small className="ssh-step-note">
            {tr("Before sending a password, we check the fingerprint to confirm it is the right computer. No login details are sent.")}
          </small>
        )}
      </section>

      <section className="ssh-step">
        <div className="ssh-step-head">
          <span className="ssh-step-no">3</span>
          <strong>{tr("Sign in")}</strong>
          <div className="segmented ssh-auth" role="radiogroup">
            <button
              type="button"
              role="radio"
              aria-checked={draft.auth === "password"}
              className={draft.auth === "password" ? "on" : ""}
              onClick={() => setDraft({ ...draft, auth: "password" })}
            >
              {tr("Password")}
            </button>
            <button
              type="button"
              role="radio"
              aria-checked={draft.auth === "key"}
              className={draft.auth === "key" ? "on" : ""}
              onClick={() => setDraft({ ...draft, auth: "key" })}
            >
              {tr("Key file")}
            </button>
          </div>
        </div>
        {draft.auth === "key" && (
          <div className="ssh-key-file">
            <code title={draft.keyPath}>{draft.keyPath || tr("Not selected")}</code>
            <button
              className="button secondary small"
              type="button"
              onClick={() =>
                void open({ multiple: false, directory: false }).then((picked) => {
                  if (typeof picked === "string") setDraft({ ...draft, keyPath: picked });
                })
              }
            >
              {tr("Choose private key")}
            </button>
          </div>
        )}
        <label className="field-block">
          <span>{draft.auth === "key" ? tr("Key passphrase (leave empty if none)") : tr("Password")}</span>
          <input
            type="password"
            value={draft.secret}
            placeholder={draft.existing ? tr("Leave empty to keep the current value") : ""}
            onChange={(event) => setDraft({ ...draft, secret: event.target.value })}
          />
        </label>
      </section>

      <div className="ssh-form-buttons">
        <button className="button ghost small" type="button" onClick={onCancel}>
          {tr("Cancel")}
        </button>
        <button
          className="button primary small"
          type="button"
          disabled={!addressDone || !keyDone || busy === "save"}
          data-tip={!keyDone ? tr("Check the host key first") : undefined}
          onClick={onSave}
        >
          {busy === "save" ? tr("Saving…") : tr("Save")}
        </button>
      </div>
    </div>
  );
}

function FolderPicker({
  hostId,
  start,
  onPick,
  onError,
}: {
  hostId: string;
  start: string;
  onPick: (path: string) => void;
  onError: (message: string) => void;
}) {
  const [path, setPath] = useState("");
  const [entries, setEntries] = useState<RemoteEntry[]>([]);
  const [busy, setBusy] = useState(false);

  const go = async (next: string) => {
    setBusy(true);
    try {
      const found = await commands.browseRemoteDir(hostId, next);
      setEntries(found);
      setPath(next || (found[0]?.path.replace(/\/[^/]*$/, "") ?? ""));
    } catch (error) {
      onError(String(error));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void go(start);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostId]);

  const crumbs = path.split("/").filter(Boolean);
  const servers = entries.filter((one) => one.looksLikeServer).length;

  return (
    <div className="ssh-picker">
      <div className="ssh-picker-bar">
        <button
          className="icon-button"
          type="button"
          aria-label={tr("Parent folder")}
          data-tip={tr("Parent folder")}
          disabled={busy || crumbs.length === 0}
          onClick={() => void go("/" + crumbs.slice(0, -1).join("/"))}
        >
          <Icon name="arrow" />
        </button>
        <code title={path}>{path || tr("Home folder")}</code>
      </div>
      <div className="remote-list">
        {busy && entries.length === 0 ? (
          <p className="muted">{tr("Loading…")}</p>
        ) : entries.length === 0 ? (
          <p className="muted">{tr("No subfolders.")}</p>
        ) : (
          entries.map((one) => (
            <button
              className={`remote-row ${one.looksLikeServer ? "is-server" : ""}`}
              type="button"
              key={one.path}
              disabled={busy}
              onClick={() => void go(one.path)}
            >
              <Icon name="folder" />
              <span>{one.name}</span>
              {one.looksLikeServer && <em className="chip">{tr("Server")}</em>}
            </button>
          ))
        )}
      </div>
      <div className="ssh-picker-foot">
        <small>
          {servers > 0
            ? tr("{0, plural, one {# server} other {# servers}} in this folder.", [String(servers)])
            : tr("Open the folder containing your server folders, then select it.")}
        </small>
        <button
          className="button primary small"
          type="button"
          disabled={busy || !path}
          onClick={() => onPick(path)}
        >
          {tr("Use this folder")}
        </button>
      </div>
    </div>
  );
}

function BoxSummary({ info }: { info: BoxInfo }) {
  const holder = info.tmux ? "tmux ✓" : info.screen ? "screen ✓" : tr("No tmux or screen");
  return (
    <div className="ssh-seen">
      <div className="ssh-seen-grid">
        <span>{tr("OS")}</span>
        <b>{info.os || tr("Unknown")}</b>
        <span>Java</span>
        <b className={info.java ? "" : "warn"}>{info.java || tr("Not found — sudo apt install openjdk-21-jre-headless")}</b>
        <span>{tr("Console")}</span>
        <b className={info.tmux || info.screen ? "" : "warn"}>{holder}</b>
        <span>{tr("Running servers")}</span>
        <b>{tr("count.items", [String(info.servers.length)])}</b>
      </div>
      {!info.tmux && !info.screen && (
        <small className="ssh-found">
          {tr("Install tmux to start and stop servers or use the console: sudo apt install tmux")}
        </small>
      )}
    </div>
  );
}

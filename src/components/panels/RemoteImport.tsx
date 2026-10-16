import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { commands, type Sent } from "../../lib/ipc";
import type { RemoteEntry, SshHostView } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Icon } from "../ui/Icon";

type Host = Sent<SshHostView>;

export function RemoteImport({
  onDone,
  onError,
  locals,
}: {
  onDone: (name: string) => void;
  onError: (message: string) => void;
  locals: { id: string; name: string }[];
}) {
  const [hosts, setHosts] = useState<Host[]>([]);
  const [hostId, setHostId] = useState("");
  const [path, setPath] = useState("");
  const [entries, setEntries] = useState<RemoteEntry[]>([]);
  const [busy, setBusy] = useState(false);
  const [localId, setLocalId] = useState("");
  const [sending, setSending] = useState("");

  useEffect(() => {
    commands
      .listSshHosts()
      .then((found) => {
        setHosts(found);
        if (found.length > 0) setHostId((now) => now || found[0].id);
      })
      .catch((error: unknown) => onError(String(error)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const go = async (next: string) => {
    if (!hostId) return;
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
    if (hostId) void go("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostId]);

  const importHere = async () => {
    if (!hostId || !path) return;
    setBusy(true);
    try {
      const made = await commands.importRemoteInstance(hostId, path);
      onDone(made.name);
    } catch (error) {
      onError(String(error));
    } finally {
      setBusy(false);
    }
  };

  const sendArchive = async () => {
    if (!hostId) return;
    const picked = await open({
      title: tr("Archive to upload"),
      filters: [{ name: "zip", extensions: ["zip"] }],
    });
    if (typeof picked !== "string") return;
    setSending(tr("Uploading… Large packs may take a few minutes."));
    try {
      const made = await commands.importRemoteArchive(hostId, picked, null);
      onDone(made.name);
    } catch (error) {
      onError(String(error));
    } finally {
      setSending("");
    }
  };
  const sendLocal = async () => {
    if (!hostId || !localId) return;
    setSending(tr("Compressing and uploading… Large worlds may take a few minutes."));
    try {
      const made = await commands.uploadInstanceToHost(localId, hostId);
      onDone(made.name);
    } catch (error) {
      onError(String(error));
    } finally {
      setSending("");
    }
  };

  if (hosts.length === 0) {
    return (
      <div className="remote-pick empty-block">
        <p>{tr("No SSH servers added yet.")}</p>
        <small>{tr("Use Add SSH above the server list to register a connection first.")}</small>
      </div>
    );
  }

  const crumbs = path.split("/").filter(Boolean);

  return (
    <div className="remote-pick">
      <div className="remote-pick-head">
        <select value={hostId} onChange={(event) => setHostId(event.target.value)}>
          {hosts.map((one) => (
            <option key={one.id} value={one.id}>
              {one.name} ({one.address})
            </option>
          ))}
        </select>
        <button
          className="button ghost small"
          type="button"
          disabled={busy || crumbs.length === 0}
          onClick={() => void go("/" + crumbs.slice(0, -1).join("/"))}
        >
          <Icon name="arrow" />
          {tr("Up")}
        </button>
      </div>

      <div className="remote-path" title={path}>
        <code>{path || tr("Loading…")}</code>
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

      <div className="remote-pick-foot">
        <small>{tr("Open the server folder, then select the button below.")}</small>
        <button
          className="button primary small"
          type="button"
          disabled={busy || !!sending || !path}
          onClick={() => void importHere()}
        >
          {busy ? tr("Checking…") : tr("Import this folder")}
        </button>
      </div>

      <div className="remote-send">
        <div className="rail-section" aria-hidden>
          <span>{tr("Upload and create")}</span>
        </div>
        <div className="remote-send-row">
          <span>{tr("Archive (zip)")}</span>
          <button
            className="button secondary small"
            type="button"
            disabled={busy || !!sending}
            onClick={() => void sendArchive()}
          >
            {tr("Choose zip")}
          </button>
        </div>
        <div className="remote-send-row">
          <select
            value={localId}
            disabled={busy || !!sending || locals.length === 0}
            onChange={(event) => setLocalId(event.target.value)}
          >
            <option value="">
              {locals.length === 0 ? tr("No local servers") : tr("Local server")}
            </option>
            {locals.map((one) => (
              <option key={one.id} value={one.id}>
                {one.name}
              </option>
            ))}
          </select>
          <button
            className="button secondary small"
            type="button"
            disabled={busy || !!sending || !localId}
            onClick={() => void sendLocal()}
          >
            {tr("Upload entire server")}
          </button>
        </div>
        <small>
          {sending || tr("Copies the server without changing the original. Stop the server first.")}
        </small>
      </div>
    </div>
  );
}

import { open as openFile, save as saveFile } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import type { StorageItem, StorageView } from "../../lib/bindings";
import { call, webSession } from "../../lib/bridge";
import { downloadFromHost } from "../../lib/download";
import { tr, trBack } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { pickRawFiles, uploadRaw } from "../../lib/pickFile";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import "./TeamStorage.css";
import { List, ListRow } from "../primitives";

type Server = { id: string; name: string };

function size(bytes: number) {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

function who(name: string) {
  return name === "host" ? tr("the host") : name;
}

/**
 * F6: the team's Storage. The host's window (`team`) works on its own folder; a teammate's server window
 * (`serverId`) asks the host over Monolink, and downloads and uploads in pieces.
 */
export function TeamStorage({
  team,
  serverId,
  servers,
  canTrash,
  onNotice,
  onError,
  confirm,
}: {
  /** The host's window: this team's storage on this PC. */
  team?: string;
  /** A teammate's window: the server it shows (the storage is its host's). */
  serverId?: string;
  /** Servers a file can be put into. */
  servers: Server[];
  /** Admin or Owner: trash, restore, empty the trash. */
  canTrash: boolean;
  onNotice: (message: string) => void;
  onError: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
}) {
  const host = !!team;
  const [view, setView] = useState<StorageView | null>(null);
  const [busy, setBusy] = useState(false);
  const [showTrash, setShowTrash] = useState(false);
  const [into, setInto] = useState<{ item: StorageItem; server: string; folder: string } | null>(null);
  const [fromPath, setFromPath] = useState("");

  const load = useCallback(async () => {
    try {
      setView(host ? await commands.storageView(team!) : await call<StorageView>("__storage_view", { id: serverId }));
    } catch (cause) {
      onError(trBack(String(cause)));
    }
  }, [host, team, serverId, onError]);
  useEffect(() => {
    void load();
  }, [load]);

  const run = async (work: () => Promise<unknown>, done?: string) => {
    setBusy(true);
    try {
      await work();
      if (done) onNotice(done);
      await load();
    } catch (cause) {
      onError(trBack(String(cause)));
    } finally {
      setBusy(false);
    }
  };

  const add = () =>
    run(async () => {
      if (host) {
        const chosen = await openFile({ title: tr("Add to Storage"), multiple: true, directory: false });
        const list = Array.isArray(chosen) ? chosen : typeof chosen === "string" ? [chosen] : [];
        for (const path of list) await commands.storagePutFile(team!, path, path.split(/[\\/]/).pop() ?? path, "", []);
        return;
      }
      for (const file of await pickRawFiles("", true)) {
        const upload = await uploadRaw(file, webSession());
        await call("__storage_put", { id: serverId, upload, name: file.name, note: "", tags: [] });
      }
    }, tr("Added to Storage."));

  const download = (item: StorageItem, version: number) =>
    run(async () => {
      if (host) {
        const target = await saveFile({ title: tr("Save a copy"), defaultPath: item.name });
        if (typeof target === "string") await commands.storageSaveCopy(team!, item.id, version, target);
        return;
      }
      await downloadFromHost({ id: serverId, kind: "storage", item: item.id, version });
    });

  const trash = (item: StorageItem, trashed: boolean) =>
    run(() => (host ? commands.storageTrash(team!, item.id, trashed) : call("__storage_trash", { id: serverId, item: item.id, trashed })));

  const emptyTrash = async () => {
    if (!(await confirm(tr("Empty the Storage trash?"), tr("Files in the trash are deleted for good."), tr("Empty trash"), true))) return;
    await run(() => (host ? commands.storageEmptyTrash(team!) : call("__storage_empty_trash", { id: serverId })), tr("The trash is empty."));
  };

  const putInto = () => {
    if (!into) return;
    const { item, server, folder } = into;
    void run(async () => {
      const path = host ? await commands.storageToServer(team!, item.id, 0, server, folder) : await call<string>("__storage_to_server", { id: server, item: item.id, version: 0, folder });
      onNotice(tr("Put {0} in {1}.", [item.name, path]));
      setInto(null);
    });
  };

  const fromServer = () => {
    const path = fromPath.trim();
    const server = host ? servers[0]?.id : serverId;
    if (!path || !server) return;
    void run(async () => {
      if (host) await commands.storagePutFromServer(team!, server, path);
      else await call("__storage_from_server", { id: server, path });
      setFromPath("");
    }, tr("Added to Storage."));
  };

  if (!view) return <p className="muted team-storage-empty">{tr("Loading…")}</p>;
  const shown = view.items.filter((one) => one.trashed === showTrash);
  const used = Number(view.used ?? 0);
  const limit = Number(view.limit ?? 0);
  const share = Math.min(1, limit ? used / limit : 0);
  return (
    <section className="team-storage">
      <div className="team-storage-head">
        <div className="team-storage-space" aria-label={tr("Space used")}>
          <span>{tr("{0} of {1}", [size(used), size(limit)])}</span>
          <span className="team-storage-bar" aria-hidden>
            <i style={{ width: `${Math.round(share * 100)}%` }} />
          </span>
        </div>
        <button type="button" className="button ghost small" onClick={() => setShowTrash(!showTrash)}>
          <Icon name="trash" />
          {showTrash ? tr("Back to files") : tr("Trash")}
        </button>
        <button type="button" className="button primary small" disabled={busy} onClick={() => void add()}>
          <Icon name="plus" />
          {tr("Add files")}
        </button>
      </div>
      {!showTrash && servers.length > 0 && (
        <form
          className="team-storage-from"
          onSubmit={(event) => {
            event.preventDefault();
            fromServer();
          }}
        >
          <input value={fromPath} placeholder={tr("A file in the server, e.g. plugins/MyPlugin/config.yml")} aria-label={tr("Keep a server file in Storage")} onChange={(event) => setFromPath(event.target.value)} />
          <button type="submit" className="button secondary small" disabled={busy || !fromPath.trim()}>
            {tr("Keep in Storage")}
          </button>
        </form>
      )}
      {shown.length === 0 ? (
        <p className="muted team-storage-empty">{showTrash ? tr("The trash is empty.") : tr("Nothing here yet. Keep models, packs and plugin files for later.")}</p>
      ) : (
        <List variant="box" className="team-storage-list">
          {shown.map((item) => {
            const last = item.versions[item.versions.length - 1];
            return (
              <ListRow
                key={item.id}
                leading={<Icon name="file" />}
                title={item.name}
                subtitle={
                  <>
                    {last ? `${size(Number(last.size ?? 0))} · ${who(last.by)} · ${new Date(Number(last.at ?? 0) * 1000).toLocaleString()}` : ""}
                    {item.versions.length > 1 ? ` · ${tr("{0} versions", [item.versions.length])}` : ""}
                  </>
                }
                detail={
                  (item.note || (item.tags ?? []).length > 0) && (
                  <>
                  {item.note && <small className="team-storage-note">{item.note}</small>}
                  {(item.tags ?? []).length > 0 && (
                    <span className="team-storage-tags">
                      {(item.tags ?? []).map((tag) => (
                        <span key={tag} className="chip subtle">
                          {tag}
                        </span>
                      ))}
                    </span>
                  )}
                  </>
                  )
                }
                actions={
                <>
                  {!showTrash && (
                    <>
                      <button type="button" className="icon-button" aria-label={tr("Download")} data-tip={tr("Download")} disabled={busy} onClick={() => void download(item, 0)}>
                        <Icon name="import" />
                      </button>
                      {servers.length > 0 && (
                        <button type="button" className="button ghost small" disabled={busy} onClick={() => setInto({ item, server: serverId ?? servers[0].id, folder: "" })}>
                          {tr("Put in a server")}
                        </button>
                      )}
                    </>
                  )}
                  {canTrash && (
                    <button type="button" className="icon-button" aria-label={showTrash ? tr("Restore") : tr("Move to trash")} data-tip={showTrash ? tr("Restore") : tr("Move to trash")} disabled={busy} onClick={() => void trash(item, !showTrash)}>
                      <Icon name={showTrash ? "history" : "trash"} />
                    </button>
                  )}
                </>
                }
              />
            );
          })}
        </List>
      )}
      {showTrash && canTrash && shown.length > 0 && (
        <button type="button" className="button ghost small danger" disabled={busy} onClick={() => void emptyTrash()}>
          {tr("Empty trash")}
        </button>
      )}
      {into && (
        <div className="team-storage-into" role="group" aria-label={tr("Put in a server")}>
          <strong>{into.item.name}</strong>
          {servers.length > 1 && <Dropdown value={into.server} onChange={(server) => setInto({ ...into, server })} options={servers.map((one) => ({ value: one.id, label: one.name }))} />}
          <input value={into.folder} placeholder={tr("Folder in the server, e.g. plugins")} aria-label={tr("Folder in the server")} onChange={(event) => setInto({ ...into, folder: event.target.value })} />
          <button type="button" className="button primary small" disabled={busy} onClick={putInto}>
            {tr("Put it there")}
          </button>
          <button type="button" className="button ghost small" onClick={() => setInto(null)}>
            {tr("Cancel")}
          </button>
        </div>
      )}
    </section>
  );
}

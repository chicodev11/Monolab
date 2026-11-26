import { useCallback, useEffect, useState } from "react";
import { commands, type DbHostView, type ServerDb, type SshHostView } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Button, IconButton, List, ListRow } from "../primitives";
import { Dropdown } from "../ui/Dropdown";

export type Connection = { address: string; port: string; username: string; password: string; sshHost: string };
type Confirm = (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;

export const BLANK: Connection = { address: "", port: "3306", username: "", password: "", sshHost: "" };
export const portOf = (text: string) => {
  const port = Number(text);
  return Number.isInteger(port) && port > 0 && port < 65536 ? port : 0;
};

export function ConnectionFields({
  value,
  onChange,
  sshHosts,
  editing,
}: {
  value: Connection;
  onChange: (next: Connection) => void;
  sshHosts: SshHostView[];
  editing?: boolean;
}) {
  const set = (part: Partial<Connection>) => onChange({ ...value, ...part });
  return (
    <>
      <div className="db-form-row">
        <label className="field-block">
          <span className="field-label">{tr("Address")}</span>
          <input value={value.address} placeholder="db.example.com" onChange={(event) => set({ address: event.target.value })} />
        </label>
        <label className="field-block">
          <span className="field-label">{tr("Port")}</span>
          <input value={value.port} inputMode="numeric" onChange={(event) => set({ port: event.target.value })} />
        </label>
      </div>
      <div className="db-form-row">
        <label className="field-block">
          <span className="field-label">{tr("User")}</span>
          <input value={value.username} autoComplete="off" onChange={(event) => set({ username: event.target.value })} />
        </label>
        <label className="field-block">
          <span className="field-label">{tr("Password")}</span>
          <input
            type="password"
            value={value.password}
            autoComplete="new-password"
            placeholder={editing ? tr("Leave empty to keep it") : ""}
            onChange={(event) => set({ password: event.target.value })}
          />
        </label>
      </div>
      {!!sshHosts.length && (
        <div className="field-block">
          <span className="field-label">{tr("Connect through SSH")}</span>
          <Dropdown
            value={value.sshHost}
            options={[{ value: "", label: tr("Direct") }, ...sshHosts.map((one) => ({ value: one.id, label: one.name }))]}
            onChange={(sshHost) => set({ sshHost })}
          />
        </div>
      )}
    </>
  );
}

/** The MySQL and MariaDB accounts every server in the app shares, kept in Settings. */
export function DbHosts({ onError, onNotice, confirm }: { onError: (message: string) => void; onNotice: (message: string) => void; confirm: Confirm }) {
  const [hosts, setHosts] = useState<DbHostView[]>([]);
  const [orphans, setOrphans] = useState<ServerDb[]>([]);
  const [sshHosts, setSshHosts] = useState<SshHostView[]>([]);
  const [hostForm, setHostForm] = useState<(Connection & { id?: string; name: string }) | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setHosts(await commands.dbHosts());
      setOrphans(await commands.orphanDatabases());
    } catch (cause) {
      onError(String(cause));
    }
  }, [onError]);
  useEffect(() => {
    void load();
    void commands.listSshHosts().then(setSshHosts).catch(() => {});
  }, [load]);

  const run = async (work: () => Promise<unknown>, done?: string) => {
    setBusy(true);
    try {
      await work();
      if (done) onNotice(done);
      await load();
      return true;
    } catch (cause) {
      onError(String(cause));
      return false;
    } finally {
      setBusy(false);
    }
  };
  const saveHost = async () => {
    if (!hostForm) return;
    const { id, name, address, username, password, sshHost } = hostForm;
    const saved = await run(async () => {
      const version = await commands.saveDbHost({ id: id ?? null, name, address, port: portOf(hostForm.port), username, password: password || null, sshHost });
      onNotice(tr("Connected to {0}.", [version]));
    });
    if (saved) setHostForm(null);
  };

  return (
    <div className="db-hosts">
      <div className="pt-head">
        <p className="muted">{tr("Accounts the app uses to create databases. All servers share these hosts.")}</p>
        <Button size="sm" icon="plus" onClick={() => setHostForm(hostForm ? null : { ...BLANK, name: "" })}>
          {tr("Add host")}
        </Button>
      </div>
      {hostForm && (
        <div className="pt-form">
          <label className="field-block">
            <span className="field-label">{tr("Name")}</span>
            <input autoFocus value={hostForm.name} onChange={(event) => setHostForm({ ...hostForm, name: event.target.value })} />
          </label>
          <ConnectionFields value={hostForm} onChange={(next) => setHostForm({ ...hostForm, ...next })} sshHosts={sshHosts} editing={!!hostForm.id} />
          <p className="muted">{tr("Use an account that can create databases and users.")}</p>
          <div className="pt-actions">
            <Button variant="ghost" onClick={() => setHostForm(null)}>
              {tr("Cancel")}
            </Button>
            <Button variant="primary" disabled={busy || !hostForm.address.trim() || !hostForm.username.trim()} onClick={() => void saveHost()}>
              {tr("Save")}
            </Button>
          </div>
        </div>
      )}
      {!hosts.length && !hostForm ? (
        <p className="muted">{tr("No database hosts.")}</p>
      ) : (
        <List variant="box" className="pt-list">
          {hosts.map((one) => (
            <ListRow
              key={one.id}
              title={<>{one.name}
                  {one.plain && <span className="chip warn" data-tip={tr("Logins to this host cross the internet without encryption. Connect through SSH if you can.")}>{tr("Not encrypted")}</span>}
                  {!one.hasPassword && <span className="chip warn">{tr("Password needed")}</span>}</>}
              subtitle={<span className="mono">{one.username}@{one.address}:{one.port}
                  {one.sshHost ? ` · SSH ${sshHosts.find((ssh) => ssh.id === one.sshHost)?.name ?? ""}` : ""}</span>}
              actions={<><IconButton
                  label={tr("Test connection")}
                  icon="plug"
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      const version = await commands.checkDbHost(one.id);
                      onNotice(tr("Connected to {0}.", [version]));
                    })
                  }
                />
                <IconButton
                  label={tr("Edit")}
                  icon="pencil"
                  onClick={() => setHostForm({ id: one.id, name: one.name, address: one.address, port: String(one.port), username: one.username, password: "", sshHost: one.sshHost })}
                />
                <IconButton
                  label={tr("Remove")}
                  icon="trash"
                  disabled={busy}
                  onClick={async () => {
                    const body = one.databases ? tr("The app forgets this host and its {0} databases. They stay on the host with their data.", [one.databases]) : tr("The app forgets this host.");
                    if (await confirm(tr("Remove this database host?"), body, tr("Remove"))) await run(() => commands.deleteDbHost(one.id), tr("Removed the database host."));
                  }}
                /></>}
            />
          ))}
        </List>
      )}
      {orphans.length > 0 && (
        <div className="db-orphans">
          <strong>{tr("Databases of deleted servers")}</strong>
          <p className="muted">{tr("Their servers are gone from the app, but the data is still on the host.")}</p>
          <List variant="box" className="pt-list">
            {orphans.map((one) => (
              <ListRow
                key={one.id}
                title={<>{one.database}</>}
                subtitle={<span className="mono">{one.address}:{one.port}</span>}
                actions={<><IconButton
                    label={one.hostId ? tr("Delete") : tr("Remove")}
                    icon="trash"
                    disabled={busy}
                    onClick={async () => {
                      const sure = one.hostId
                        ? await confirm(tr("Delete this database?"), tr("All data inside will be deleted. This can't be undone."), tr("Delete"), true)
                        : await confirm(tr("Remove this database from the list?"), tr("The database and its data stay where they are."), tr("Remove"));
                      if (sure) await run(() => commands.deleteOrphanDatabase(one.id), one.hostId ? tr("Deleted the database.") : tr("Removed the database from the list."));
                    }}
                  /></>}
              />
            ))}
          </List>
        </div>
      )}
    </div>
  );
}

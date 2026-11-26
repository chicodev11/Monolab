import { useCallback, useEffect, useState } from "react";
import { onWeb } from "../../lib/bridge";
import { commands, type DbHostView, type ServerDb, type SshHostView } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Banner, Button, Card, DropdownMenu, EmptyState, IconButton, List, ListRow, Skeleton } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { SqliteFiles, TableBrowser } from "./DatabaseViews";
import "./Databases.css";
import { BLANK, ConnectionFields, portOf, type Connection } from "./DbHosts";

type Confirm = (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
type Open = { kind: "mysql"; db: ServerDb } | { kind: "sqlite"; path: string };
const isLoopback = (address: string) => ["localhost", "127.0.0.1", "::1"].includes(address.trim().toLowerCase());

export function DatabasesPage({
  serverId,
  remoteHost,
  onOpenHosts,
  onError,
  onNotice,
  confirm,
}: {
  serverId: string;
  remoteHost: string;
  onOpenHosts: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: Confirm;
}) {
  const [rows, setRows] = useState<ServerDb[] | null>(null);
  const [hosts, setHosts] = useState<DbHostView[]>([]);
  const [sshHosts, setSshHosts] = useState<SshHostView[]>([]);
  const [open, setOpen] = useState<Open | null>(null);
  const [form, setForm] = useState<"create" | "link" | null>(null);
  const [plugins, setPlugins] = useState<string[]>([]);
  const [shown, setShown] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [newName, setNewName] = useState("");
  const [newHost, setNewHost] = useState("");
  const [link, setLink] = useState<Connection & { database: string }>({ ...BLANK, database: "" });

  const load = useCallback(async () => {
    try {
      setRows(await commands.serverDatabases(serverId));
      if (!onWeb) setHosts(await commands.dbHosts());
    } catch (cause) {
      setRows((now) => now ?? []);
      onError(String(cause));
    }
  }, [serverId, onError]);
  useEffect(() => {
    void load();
    if (!onWeb) void commands.listSshHosts().then(setSshHosts).catch(() => {});
    if (!onWeb) void commands.dbPluginTargets(serverId).then(setPlugins).catch(() => {});
  }, [load, serverId]);

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
  const copy = (text: string) => {
    void navigator.clipboard.writeText(text);
    onNotice(tr("Copied."));
  };

  if (open?.kind === "mysql") {
    const db = open.db;
    return (
      <div className="pt-page">
        <TableBrowser
          title={db.database}
          loadTables={() => commands.databaseTables(serverId, db.id)}
          loadRows={(table, offset) => commands.databaseRows(serverId, db.id, table, offset)}
          onBack={() => setOpen(null)}
        />
      </div>
    );
  }
  if (open?.kind === "sqlite") {
    const path = open.path;
    return (
      <div className="pt-page">
        <TableBrowser
          title={path}
          loadTables={() => commands.sqliteTables(serverId, path)}
          loadRows={(table, offset) => commands.sqliteRows(serverId, path, table, offset)}
          onBack={() => setOpen(null)}
        />
      </div>
    );
  }
  if (!rows) return <Skeleton height={120} />;

  const pickedHost = hosts.find((one) => one.id === newHost);
  const farFromServer = !!pickedHost && isLoopback(pickedHost.address) && pickedHost.sshHost !== remoteHost;
  return (
    <div className="pt-page">
      <div className="pt-body">
        <div className="pt-head db-head">
          <span className="db-head-icon" aria-hidden>
            <Icon name="database" />
          </span>
          <p className="muted">{tr("MySQL and MariaDB databases for plugins.")}</p>
          {!onWeb && (
            <>
              <Button icon="settings" onClick={onOpenHosts}>
                {tr("Manage hosts")}
              </Button>
              <Button
                onClick={() => {
                  setForm(form === "link" ? null : "link");
                  setLink({ ...BLANK, database: "" });
                }}
              >
                {tr("Add existing database")}
              </Button>
              <Button
                variant="primary"
                icon="plus"
                disabled={!hosts.length}
                onClick={() => {
                  setForm(form === "create" ? null : "create");
                  setNewName("");
                  setNewHost(hosts.length === 1 ? hosts[0].id : "");
                }}
              >
                {tr("Add database")}
              </Button>
            </>
          )}
        </div>

        {form === "create" && (
          <div className="pt-form">
            <div className="field-block">
              <span className="field-label">{tr("Database host")}</span>
              <Dropdown value={newHost} options={hosts.map((one) => ({ value: one.id, label: one.name, hint: `${one.address}:${one.port}` }))} onChange={setNewHost} />
            </div>
            <label className="field-block">
              <span className="field-label">{tr("Name")}</span>
              <input autoFocus value={newName} placeholder="luckperms" onChange={(event) => setNewName(event.target.value)} />
            </label>
            {farFromServer && <Banner tone="warn">{tr("This host's address is local to another computer, so the server may not reach it.")}</Banner>}
            <div className="pt-actions">
              <Button variant="ghost" onClick={() => setForm(null)}>
                {tr("Cancel")}
              </Button>
              <Button
                variant="primary"
                disabled={busy || !newHost || !newName.trim()}
                onClick={async () => {
                  if (await run(() => commands.createServerDatabase(serverId, newHost, newName.trim()), tr("Created the database."))) setForm(null);
                }}
              >
                {tr("Create")}
              </Button>
            </div>
          </div>
        )}

        {form === "link" && (
          <div className="pt-form">
            <label className="field-block">
              <span className="field-label">{tr("Database name")}</span>
              <input autoFocus value={link.database} onChange={(event) => setLink({ ...link, database: event.target.value })} />
            </label>
            <ConnectionFields value={link} onChange={(next) => setLink({ ...link, ...next })} sshHosts={sshHosts} />
            <div className="pt-actions">
              <Button variant="ghost" onClick={() => setForm(null)}>
                {tr("Cancel")}
              </Button>
              <Button
                variant="primary"
                disabled={busy || !link.database.trim() || !link.address.trim() || !link.username.trim()}
                onClick={async () => {
                  const added = await run(
                    () => commands.linkServerDatabase(serverId, { address: link.address, port: portOf(link.port), database: link.database, username: link.username, password: link.password, sshHost: link.sshHost }),
                    tr("Added the database."),
                  );
                  if (added) setForm(null);
                }}
              >
                {tr("Add")}
              </Button>
            </div>
          </div>
        )}

        {!rows.length ? (
          <EmptyState
            icon="database"
            title={tr("No databases")}
            hint={onWeb ? tr("Databases are added in the app.") : hosts.length ? tr("For plugins that use MySQL, such as LuckPerms or CoreProtect, create a database and add the connection details to their settings.") : tr("Add a database host in Manage hosts to create databases, or add one you already have.")}
          />
        ) : (
          <Card title={tr("Databases")}>
          <List variant="box" className="pt-list">
            {rows.map((one) => {
              const address = `${one.address}:${one.port}`;
              const password = shown[one.id];
              const jdbc = `jdbc:mysql://${address}/${one.database}`;
              return (
                <ListRow
                  key={one.id}
                  className="db-row"
                  title={<>{one.database}
                      {one.plain && <span className="chip warn" data-tip={tr("Logins to this database cross the internet without encryption. Connect through SSH if you can.")}>{tr("Not encrypted")}</span>}</>}
                  actions={<>{!onWeb && plugins.length > 0 && (
                      <DropdownMenu
                        trigger={
                          <Button size="sm" icon="plug">
                            {tr("Fill plugin settings")}
                          </Button>
                        }
                        items={plugins.map((plugin) => ({
                          label: plugin,
                          onSelect: async () => {
                            if (!(await confirm(tr("Use this database in {0}?", [plugin]), tr("Writes the login into its settings, and applies after a restart. Existing plugin data isn't moved to the new database."), tr("Apply")))) return;
                            await run(() => commands.fillPluginDatabase(serverId, one.id, plugin), tr("Updated the settings of {0}. Restart the server to apply them.", [plugin]));
                          },
                        }))}
                      />
                    )}
                    <Button size="sm" icon="eye" onClick={() => setOpen({ kind: "mysql", db: one })}>
                      {tr("View tables")}
                    </Button>
                    {!onWeb && !!one.hostId && (
                      <IconButton
                        label={tr("Change password")}
                        icon="refresh"
                        disabled={busy}
                        onClick={async () => {
                          if (!(await confirm(tr("Generate a new password?"), tr("The old password in your plugin settings will no longer work."), tr("Change")))) return;
                          setShown((now) => ({ ...now, [one.id]: "" }));
                          await run(() => commands.rotateServerDatabasePassword(serverId, one.id), tr("Changed the password."));
                        }}
                      />
                    )}
                    {!onWeb && (
                      <IconButton
                        label={one.hostId ? tr("Delete") : tr("Remove")}
                        icon="trash"
                        disabled={busy}
                        onClick={async () => {
                          const sure = one.hostId
                            ? await confirm(tr("Delete this database?"), tr("All data inside will be deleted. This can't be undone."), tr("Delete"), true)
                            : await confirm(tr("Remove this database from the list?"), tr("The database and its data stay where they are."), tr("Remove"));
                          if (sure) await run(() => commands.deleteServerDatabase(serverId, one.id), one.hostId ? tr("Deleted the database.") : tr("Removed the database from the list."));
                        }}
                      />
                    )}</>}
                  detail={<><span className="pt-kv">
                      <span>{tr("Address")}</span>
                      <button className="mono pt-copy" data-tip={tr("Copy")} onClick={() => copy(address)}>{address}<Icon name="copy" /></button>
                      <span>{tr("User")}</span>
                      <button className="mono pt-copy" data-tip={tr("Copy")} onClick={() => copy(one.username)}>{one.username}<Icon name="copy" /></button>
                      <span>{tr("Password")}</span>
                      {onWeb ? (
                        <span>{tr("Only shown in the app")}</span>
                      ) : (
                        <button
                          className="mono pt-copy"
                          data-tip={password ? tr("Copy") : tr("Show password")}
                          onClick={async () => {
                            if (password) return copy(password);
                            try {
                              const got = await commands.serverDatabasePassword(serverId, one.id);
                              setShown((now) => ({ ...now, [one.id]: got }));
                            } catch (cause) {
                              onError(String(cause));
                            }
                          }}
                        >
                          {password || "••••••••"}
                          <Icon name={password ? "copy" : "eye"} />
                        </button>
                      )}
                      {one.remote && (
                        <>
                          <span>{tr("Allowed connections")}</span>
                          <span>{one.remote === "%" ? tr("Anywhere") : one.remote}</span>
                        </>
                      )}
                      <span>JDBC</span>
                      <button className="mono pt-copy" data-tip={tr("Copy")} onClick={() => copy(jdbc)}>{jdbc}<Icon name="copy" /></button>
                    </span></>}
                />
              );
            })}
          </List>
          </Card>
        )}

        <SqliteFiles serverId={serverId} far={!!remoteHost} onOpen={(path) => setOpen({ kind: "sqlite", path })} />
      </div>
    </div>
  );
}

/* eslint-disable @typescript-eslint/no-explicit-any */
import { useCallback, useEffect, useMemo, useState } from "react";
import type { PanelHostView } from "../../lib/bindings";
import { commands } from "../../lib/ipc";
import { date, relativeTime } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { isoSecs, pcall, plist, type Json } from "../../lib/panelApi";
import { Badge, Banner, Button, EmptyState, IconButton, SegmentedControl, Skeleton } from "../primitives";
import { PanelCreate } from "./PanelCreate";
import { ActivityList } from "./PanelServerTools";

type View = "overview" | "servers" | "users" | "nodes" | "eggs" | "account";

type Props = {
  hosts: PanelHostView[];
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  onSynced: () => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
  ask: (title: string, label: string, value: string, confirmLabel: string, danger?: boolean) => Promise<string | null>;
  openSessions: () => void;
};

const mib = (value: number) => (value >= 1024 ? `${(value / 1024).toFixed(1)} GB` : `${value} MB`);

export function PanelAdmin(props: Props) {
  const { hosts } = props;
  const [hostId, setHostId] = useState(hosts[0]?.id ?? "");
  const host = hosts.find((one) => one.id === hostId) ?? hosts[0];
  const admin = host?.role === "admin";
  const [view, setView] = useState<View>("overview");

  useEffect(() => {
    if (!hosts.some((one) => one.id === hostId) && hosts[0]) setHostId(hosts[0].id);
  }, [hosts, hostId]);

  if (!host) {
    return (
      <EmptyState
        icon="cloud"
        title={tr("No panels registered")}
        hint={tr("Add a panel in Manage sessions above the server list.")}
        action={
          <Button variant="primary" icon="plus" onClick={props.openSessions}>
            {tr("Add panel")}
          </Button>
        }
      />
    );
  }

  const views: { value: View; label: string }[] = admin
    ? [
        { value: "overview", label: tr("Overview") },
        { value: "servers", label: tr("Server") },
        { value: "users", label: tr("User") },
        { value: "nodes", label: tr("Node") },
        { value: "eggs", label: tr("Egg") },
        { value: "account", label: tr("My account") },
      ]
    : [
        { value: "overview", label: tr("Overview") },
        { value: "servers", label: tr("Server") },
        { value: "account", label: tr("My account") },
      ];
  const shown = views.some((one) => one.value === view) ? view : "overview";
  const part = { ...props, hostId: host.id, host, admin };

  return (
    <div className="pa-page">
      {hosts.length > 1 && (
        <SegmentedControl
          value={host.id}
          onChange={(id) => setHostId(id)}
          options={hosts.map((one) => ({ value: one.id, label: one.name }))}
        />
      )}
      <div className="pa-host">
        <span className="mono muted">{host.url}</span>
        <Badge tone={admin ? "ok" : "line"}>{admin ? tr("Admin") : tr("User")}</Badge>
        {admin && !host.hasKey && <Badge tone="warn">{tr("No account key — cannot start or stop")}</Badge>}
      </div>
      <SegmentedControl className="page-tabs" value={shown} onChange={setView} options={views} />
      <div className="pt-body" key={`${host.id}-${shown}`}>
        {shown === "overview" && <Overview {...part} />}
        {shown === "servers" && (admin ? <AdminServers {...part} /> : <MyServers {...part} />)}
        {shown === "users" && <Users {...part} />}
        {shown === "nodes" && <Nodes {...part} />}
        {shown === "eggs" && <Eggs {...part} />}
        {shown === "account" && <Account {...part} />}
      </div>
    </div>
  );
}

type Part = Props & { hostId: string; host: PanelHostView; admin: boolean };

async function act(part: Part, work: () => Promise<unknown>, done: string, reload?: () => Promise<void> | void) {
  try {
    await work();
    if (done) part.onNotice(done);
    if (reload) await reload();
  } catch (cause) {
    part.onError(String(cause));
  }
}

function useLoad<T>(load: () => Promise<T>, onError: (message: string) => void, key: string) {
  const [value, setValue] = useState<T | null>(null);
  const run = useCallback(async () => {
    try {
      setValue(await load());
    } catch (cause) {
      onError(String(cause));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  useEffect(() => {
    void run();
  }, [run]);
  return { value, reload: run };
}

function Overview(part: Part) {
  const { value } = useLoad(
    async () => {
      if (!part.admin) {
        const mine = part.host.hasKey ? await plist(part.hostId, "/api/client?include=allocations") : [];
        return { servers: mine.length, users: 0, nodes: [] as Json[] };
      }
      const [servers, users, nodes] = await Promise.all([
        plist(part.hostId, "/api/application/servers"),
        plist(part.hostId, "/api/application/users"),
        plist(part.hostId, "/api/application/nodes"),
      ]);
      return { servers: servers.length, users: users.length, nodes, suspended: servers.filter((one) => one.suspended).length };
    },
    part.onError,
    `${part.hostId}:${part.admin}`,
  );
  if (!value) return <Skeleton height={140} />;
  return (
    <>
      <div className="pa-stats">
        <div>
          <span>{tr("Server")}</span>
          <strong>{value.servers}</strong>
        </div>
        {part.admin && (
          <>
            <div>
              <span>{tr("User")}</span>
              <strong>{value.users}</strong>
            </div>
            <div>
              <span>{tr("Node")}</span>
              <strong>{value.nodes.length}</strong>
            </div>
            <div>
              <span>{tr("Suspended")}</span>
              <strong>{(value as any).suspended ?? 0}</strong>
            </div>
          </>
        )}
      </div>
      {part.admin &&
        value.nodes.map((node) => {
          const mem = node.allocated_resources?.memory ?? 0;
          const disk = node.allocated_resources?.disk ?? 0;
          const memAll = node.memory * (1 + (node.memory_overallocate > 0 ? node.memory_overallocate / 100 : 0));
          const diskAll = node.disk * (1 + (node.disk_overallocate > 0 ? node.disk_overallocate / 100 : 0));
          return (
            <div className="pa-node" key={node.id}>
              <strong>
                {node.name} <small className="muted mono">{node.fqdn}</small>
                {node.maintenance_mode && <Badge tone="warn">{tr("Maintenance")}</Badge>}
              </strong>
              <Meter label={tr("Memory allocation")} used={mem} all={memAll} text={`${mib(mem)} / ${mib(node.memory)}`} />
              <Meter label={tr("Disk allocation")} used={disk} all={diskAll} text={`${mib(disk)} / ${mib(node.disk)}`} />
            </div>
          );
        })}
    </>
  );
}

function Meter({ label, used, all, text }: { label: string; used: number; all: number; text: string }) {
  const ratio = all > 0 ? Math.min(1, used / all) : 0;
  return (
    <div className="pa-meter">
      <span>{label}</span>
      <div className={`pa-bar ${ratio > 0.9 ? "hot" : ratio > 0.7 ? "warm" : ""}`}>
        <i style={{ width: `${ratio * 100}%` }} />
      </div>
      <small className="mono">{text}</small>
    </div>
  );
}

function MyServers(part: Part) {
  const { value } = useLoad(() => plist(part.hostId, "/api/client?include=allocations"), part.onError, part.hostId);
  if (!value) return <Skeleton height={120} />;
  const hidden = part.host.hidden ?? [];
  return (
    <>
      <Banner>{tr("Your servers and shared servers are added to the list automatically (synced every 5 minutes). Restore servers you removed here.")}</Banner>
      <div className="pt-list">
        {value.map((one) => (
          <div className="pt-row" key={one.identifier}>
            <div className="pt-row-main">
              <strong>
                {one.name} {!one.server_owner && <Badge tone="subtle">{tr("Invited")}</Badge>}
                {one.is_suspended && <Badge tone="danger">{tr("Suspended")}</Badge>}
              </strong>
              <small className="muted mono">
                {one.identifier} · {one.node} · {mib(one.limits?.memory ?? 0)}
              </small>
            </div>
            {hidden.includes(one.identifier) && (
              <Button
                variant="secondary"
                size="sm"
                onClick={() =>
                  void act(part, () => commands.panelUnhide(part.hostId, one.identifier), tr("Added back to the server list."), part.onSynced)
                }
              >
                {tr("Add back to list")}
              </Button>
            )}
          </div>
        ))}
      </div>
    </>
  );
}

function AdminServers(part: Part) {
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<Json | null>(null);
  const [query, setQuery] = useState("");
  const { value, reload } = useLoad(
    async () => {
      const [servers, users, nodes] = await Promise.all([
        plist(part.hostId, "/api/application/servers"),
        plist(part.hostId, "/api/application/users").catch(() => [] as Json[]),
        plist(part.hostId, "/api/application/nodes").catch(() => [] as Json[]),
      ]);
      return { servers, users, nodes };
    },
    part.onError,
    part.hostId,
  );
  const owner = useMemo(() => new Map((value?.users ?? []).map((one) => [one.id, one.username])), [value]);
  const nodeName = useMemo(() => new Map((value?.nodes ?? []).map((one) => [one.id, one.name])), [value]);
  if (creating) {
    return (
      <PanelCreate
        hostId={part.hostId}
        onCancel={() => setCreating(false)}
        onDone={() => {
          setCreating(false);
          void reload();
          part.onSynced();
        }}
        onError={part.onError}
        onNotice={part.onNotice}
      />
    );
  }
  if (!value) return <Skeleton height={160} />;
  const hidden = part.host.hidden ?? [];
  const list = value.servers.filter((one) => !query || `${one.name} ${one.identifier} ${owner.get(one.user) ?? ""}`.toLowerCase().includes(query.toLowerCase()));
  return (
    <>
      <div className="pt-head">
        <label className="search">
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={tr("Search by name or owner")} />
        </label>
        <Button variant="primary" icon="plus" onClick={() => setCreating(true)}>
          {tr("Create server")}
        </Button>
      </div>
      {editing && <Limits part={part} server={editing} onDone={() => { setEditing(null); void reload(); }} />}
      <div className="pt-list">
        {list.map((one) => (
          <div className="pt-row" key={one.id}>
            <div className="pt-row-main">
              <strong>
                {one.name} {one.suspended && <Badge tone="danger">{tr("Suspended")}</Badge>}
                {hidden.includes(one.identifier) && <Badge tone="subtle">{tr("Removed from list")}</Badge>}
              </strong>
              <small className="muted">
                <span className="mono">{one.identifier}</span> · {tr("Owner")} {owner.get(one.user) ?? `#${one.user}`} · {nodeName.get(one.node) ?? `#${one.node}`} ·{" "}
                {mib(one.limits?.memory ?? 0)} · {tr("Disk")} {mib(one.limits?.disk ?? 0)} · CPU {one.limits?.cpu || "∞"}%
              </small>
            </div>
            <div className="pt-row-side">
              {hidden.includes(one.identifier) && (
                <IconButton
                  label={tr("Add back to list")}
                  icon="undo"
                  onClick={() => void act(part, () => commands.panelUnhide(part.hostId, one.identifier), tr("Added back to the server list."), part.onSynced)}
                />
              )}
              <IconButton label={tr("Edit limits")} icon="pencil" onClick={() => setEditing(one)} />
              <IconButton
                label={one.suspended ? tr("Unsuspend") : tr("Suspend")}
                icon={one.suspended ? "play" : "pause"}
                onClick={async () => {
                  if (!one.suspended && !(await part.confirm(tr("Suspend this server?"), tr("{0} will stop, and its owner won't be able to start it.", [one.name]), tr("Suspend"), true))) return;
                  await act(part, () => pcall(part.hostId, "POST", `/api/application/servers/${one.id}/${one.suspended ? "unsuspend" : "suspend"}`), one.suspended ? tr("Unsuspended.") : tr("Suspended."), reload);
                }}
              />
              <IconButton
                label={tr("Reinstall")}
                icon="refresh"
                onClick={async () => {
                  if (!(await part.confirm(tr("Reinstall?"), tr("Runs the installation script for {0} again. Server files may change.", [one.name]), tr("Reinstall"), true))) return;
                  await act(part, () => pcall(part.hostId, "POST", `/api/application/servers/${one.id}/reinstall`), tr("Reinstallation started."), reload);
                }}
              />
              <IconButton
                label={tr("Delete")}
                icon="trash"
                onClick={async () => {
                  const typed = await part.ask(tr("Delete this server?"), tr("This cannot be undone. Type the server name {0} exactly to confirm", [one.name]), "", tr("Delete"), true);
                  if (typed === null) return;
                  if (typed.trim() !== one.name) {
                    part.onError(tr("The server name didn't match. Nothing was deleted."));
                    return;
                  }
                  await act(part, () => pcall(part.hostId, "DELETE", `/api/application/servers/${one.id}`), tr("Deleted the server."), async () => {
                    await reload();
                    part.onSynced();
                  });
                }}
              />
            </div>
          </div>
        ))}
      </div>
    </>
  );
}

function Limits({ part, server, onDone }: { part: Part; server: Json; onDone: () => void }) {
  const [limits, setLimits] = useState({
    memory: String(server.limits?.memory ?? 0),
    disk: String(server.limits?.disk ?? 0),
    cpu: String(server.limits?.cpu ?? 0),
    swap: String(server.limits?.swap ?? 0),
    io: String(server.limits?.io ?? 500),
    databases: String(server.feature_limits?.databases ?? 0),
    allocations: String(server.feature_limits?.allocations ?? 0),
    backups: String(server.feature_limits?.backups ?? 0),
  });
  const field = (key: keyof typeof limits, label: string) => (
    <label className="field-block" key={key}>
      <span className="field-label">{label}</span>
      <input type="number" min={0} value={limits[key]} onChange={(event) => setLimits({ ...limits, [key]: event.target.value })} />
    </label>
  );
  const n = (key: keyof typeof limits) => Number(limits[key]) || 0;
  return (
    <div className="pt-form">
      <strong>
        {server.name} · {tr("Limits")}
      </strong>
      <div className="pt-cron">
        {field("memory", tr("Memory MB"))}
        {field("disk", tr("Disk MB"))}
        {field("cpu", tr("CPU % (0 = unlimited)"))}
        {field("swap", tr("Swap MB"))}
        {field("io", "IO")}
      </div>
      <div className="pt-cron">
        {field("databases", tr("Databases"))}
        {field("allocations", tr("Ports"))}
        {field("backups", tr("Backups"))}
      </div>
      <div className="pt-actions">
        <Button variant="ghost" onClick={onDone}>
          {tr("Cancel")}
        </Button>
        <Button
          variant="primary"
          onClick={() =>
            void act(
              part,
              () =>
                pcall(part.hostId, "PATCH", `/api/application/servers/${server.id}/build`, {
                  allocation: server.allocation,
                  memory: n("memory"),
                  swap: n("swap"),
                  disk: n("disk"),
                  io: Math.min(1000, Math.max(10, n("io"))),
                  cpu: n("cpu"),
                  threads: server.limits?.threads ?? null,
                  feature_limits: { databases: n("databases"), allocations: n("allocations"), backups: n("backups") },
                }),
              tr("Limits updated. Changes apply on the next start."),
            ).then(onDone)
          }
        >
          {tr("Save")}
        </Button>
      </div>
    </div>
  );
}

function Users(part: Part) {
  const { value, reload } = useLoad(() => plist(part.hostId, "/api/application/users"), part.onError, part.hostId);
  const [draft, setDraft] = useState<Json | null>(null);
  if (!value) return <Skeleton height={140} />;
  const save = async () => {
    if (!draft) return;
    const body: Json = {
      email: draft.email,
      username: draft.username,
      first_name: draft.first_name || draft.username,
      last_name: draft.last_name || draft.username,
      root_admin: !!draft.root_admin,
    };
    if (draft.password) body.password = draft.password;
    await act(
      part,
      () => pcall(part.hostId, draft.id ? "PATCH" : "POST", draft.id ? `/api/application/users/${draft.id}` : "/api/application/users", body),
      draft.id ? tr("Updated the user.") : tr("Created the user."),
      reload,
    );
    setDraft(null);
  };
  return (
    <>
      <div className="pt-head">
        <p className="muted">{tr("Panel accounts. Leave the password empty to have the panel send a setup email, if email is configured.")}</p>
        {!draft && (
          <Button variant="primary" icon="plus" onClick={() => setDraft({ email: "", username: "", first_name: "", last_name: "", password: "", root_admin: false })}>
            {tr("Add user")}
          </Button>
        )}
      </div>
      {draft && (
        <div className="pt-form">
          <div className="pt-cron">
            {(["username", "email", "first_name", "last_name", "password"] as const).map((key) => (
              <label className="field-block" key={key}>
                <span className="field-label">
                  {{ username: tr("Username"), email: tr("Email"), first_name: tr("Name"), last_name: tr("Last name"), password: draft.id ? tr("New password (leave empty to keep)") : tr("Password") }[key]}
                </span>
                <input
                  type={key === "password" ? "password" : "text"}
                  value={draft[key] ?? ""}
                  onChange={(event) => setDraft({ ...draft, [key]: event.target.value })}
                />
              </label>
            ))}
          </div>
          <label className="pt-check">
            <input type="checkbox" checked={!!draft.root_admin} onChange={(event) => setDraft({ ...draft, root_admin: event.target.checked })} />
            {tr("Panel admin")}
          </label>
          <div className="pt-actions">
            <Button variant="ghost" onClick={() => setDraft(null)}>
              {tr("Cancel")}
            </Button>
            <Button variant="primary" disabled={!draft.username?.trim() || !draft.email?.includes("@")} onClick={() => void save()}>
              {tr("Save")}
            </Button>
          </div>
        </div>
      )}
      <div className="pt-list">
        {value.map((one) => (
          <div className="pt-row" key={one.id}>
            <div className="pt-row-main">
              <strong>
                {one.username} {one.root_admin && <Badge tone="ok">{tr("Admin")}</Badge>}
                {one["2fa"] && <Badge tone="subtle">2FA</Badge>}
              </strong>
              <small className="muted">
                {one.email} · {tr("Joined")} {one.created_at ? relativeTime(isoSecs(one.created_at)) : "—"}
              </small>
            </div>
            <div className="pt-row-side">
              <IconButton label={tr("Edit")} icon="pencil" onClick={() => setDraft({ ...one, password: "" })} />
              <IconButton
                label={tr("Delete")}
                icon="trash"
                onClick={async () => {
                  if (!(await part.confirm(tr("Delete this user?"), tr("Deletes {0}. Users who still own servers can't be deleted.", [one.username]), tr("Delete"), true))) return;
                  await act(part, () => pcall(part.hostId, "DELETE", `/api/application/users/${one.id}`), tr("Deleted the user."), reload);
                }}
              />
            </div>
          </div>
        ))}
      </div>
    </>
  );
}

function Nodes(part: Part) {
  const { value } = useLoad(() => plist(part.hostId, "/api/application/nodes"), part.onError, part.hostId);
  const [open, setOpen] = useState<number | null>(null);
  if (!value) return <Skeleton height={140} />;
  return (
    <div className="pt-list">
      {value.map((node) => (
        <div className="pt-row pt-block" key={node.id}>
          <div className="pt-row-main">
            <strong>
              {node.name} <small className="muted mono">{node.fqdn}</small>
            </strong>
            <small className="muted">
              {tr("Memory")} {mib(node.memory)} · {tr("Disk")} {mib(node.disk)} · {tr("Daemon port")} {node.daemon_listen}
            </small>
            {open === node.id && <Allocations part={part} node={node} />}
          </div>
          <div className="pt-row-side">
            <Button variant="secondary" size="sm" onClick={() => setOpen(open === node.id ? null : node.id)}>
              {open === node.id ? tr("Collapse") : tr("Allocations")}
            </Button>
          </div>
        </div>
      ))}
    </div>
  );
}

function Allocations({ part, node }: { part: Part; node: Json }) {
  const { value, reload } = useLoad(() => plist(part.hostId, `/api/application/nodes/${node.id}/allocations`), part.onError, `${part.hostId}:${node.id}`);
  if (!value) return <Skeleton height={80} />;
  const free = value.filter((one) => !one.assigned).length;
  return (
    <div className="pa-alloc">
      <div className="pt-head">
        <small className="muted">{tr("{0} in total · {1} free", [value.length, free])}</small>
        <Button
          variant="secondary"
          size="sm"
          icon="plus"
          onClick={async () => {
            const ip = await part.ask(tr("Add allocation"), tr("IP (0.0.0.0 for all)"), "0.0.0.0", tr("Next"));
            if (!ip?.trim()) return;
            const ports = await part.ask(tr("Add allocation"), tr("Ports (comma-separated; ranges such as 25565-25570 supported)"), "", tr("Add"));
            if (!ports?.trim()) return;
            await act(
              part,
              () => pcall(part.hostId, "POST", `/api/application/nodes/${node.id}/allocations`, { ip: ip.trim(), ports: ports.split(",").map((one) => one.trim()).filter(Boolean) }),
              tr("Added the ports."),
              reload,
            );
          }}
        >
          {tr("Add")}
        </Button>
      </div>
      <div className="pa-chips">
        {value.map((one) => (
          <span className={`pa-chip ${one.assigned ? "used" : ""}`} key={one.id} data-tip={one.assigned ? tr("In use by a server") : tr("Free")}>
            <span className="mono">
              {one.alias || one.ip}:{one.port}
            </span>
            {!one.assigned && (
              <button
                aria-label={tr("Delete")}
                onClick={() => void act(part, () => pcall(part.hostId, "DELETE", `/api/application/nodes/${node.id}/allocations/${one.id}`), "", reload)}
              >
                ×
              </button>
            )}
          </span>
        ))}
      </div>
    </div>
  );
}

function Eggs(part: Part) {
  const { value } = useLoad(() => plist(part.hostId, "/api/application/nests?include=eggs"), part.onError, part.hostId);
  if (!value) return <Skeleton height={140} />;
  return (
    <div className="pt-list">
      {value.map((nest) => {
        const eggs: Json[] = (nest.relationships?.eggs?.data ?? []).map((one: Json) => one.attributes ?? one);
        return (
          <div className="pt-row pt-block" key={nest.id}>
            <div className="pt-row-main">
              <strong>{nest.name}</strong>
              <small className="muted">{nest.description}</small>
              <div className="pa-chips">
                {eggs.map((egg) => (
                  <span className="pa-chip" key={egg.id} data-tip={egg.description ?? ""}>
                    {egg.name}
                  </span>
                ))}
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}

function Account(part: Part) {
  const { value, reload } = useLoad(
    async () => {
      if (!part.host.hasKey) return null;
      const [account, keys, activity] = await Promise.all([
        pcall<Json>(part.hostId, "GET", "/api/client/account"),
        pcall<Json>(part.hostId, "GET", "/api/client/account/api-keys"),
        pcall<Json>(part.hostId, "GET", "/api/client/account/activity?per_page=30"),
      ]);
      return {
        account: account?.attributes ?? {},
        keys: (keys?.data ?? []).map((one: Json) => one.attributes ?? one) as Json[],
        activity: (activity?.data ?? []).map((one: Json) => one.attributes ?? one) as Json[],
      };
    },
    part.onError,
    part.hostId,
  );
  if (!part.host.hasKey) {
    return <Banner tone="warn">{tr("No account API key. Add one in Manage sessions to view your account, API keys and activity.")}</Banner>;
  }
  if (!value) return <Skeleton height={140} />;
  return (
    <>
      <div className="pt-row">
        <div className="pt-row-main">
          <strong>
            {value.account.username} {value.account.admin && <Badge tone="ok">{tr("Admin")}</Badge>}
          </strong>
          <small className="muted">{value.account.email}</small>
        </div>
      </div>
      <div className="pt-head">
        <strong>{tr("API key")}</strong>
        <Button
          variant="secondary"
          size="sm"
          icon="plus"
          onClick={async () => {
            const description = await part.ask(tr("New API key"), tr("Description"), "Monolab", tr("Create"));
            if (!description?.trim()) return;
            try {
              const got = await pcall<Json>(part.hostId, "POST", "/api/client/account/api-keys", { description: description.trim(), allowed_ips: [] });
              const secret = `${got?.attributes?.identifier ?? ""}${got?.meta?.secret_token ?? ""}`;
              if (secret) {
                await navigator.clipboard.writeText(secret);
                part.onNotice(tr("Key created and copied. It's only shown once, so save it somewhere safe."));
              }
              await reload();
            } catch (cause) {
              part.onError(String(cause));
            }
          }}
        >
          {tr("Create key")}
        </Button>
      </div>
      <div className="pt-list">
        {value.keys.map((one) => (
          <div className="pt-row" key={one.identifier}>
            <div className="pt-row-main">
              <strong>{one.description || one.identifier}</strong>
              <small className="muted mono">
                {one.identifier}… · {tr("Last used")} {one.last_used_at ? relativeTime(isoSecs(one.last_used_at)) : tr("None")} · {date(isoSecs(one.created_at))}
              </small>
            </div>
            <IconButton
              label={tr("Delete")}
              icon="trash"
              onClick={async () => {
                if (!(await part.confirm(tr("Delete this API key?"), tr("Anything using this key, including this app, will no longer be able to connect."), tr("Remove"), true))) return;
                await act(part, () => pcall(part.hostId, "DELETE", `/api/client/account/api-keys/${one.identifier}`), tr("Deleted the key."), reload);
              }}
            />
          </div>
        ))}
      </div>
      <strong className="pa-sub">{tr("Recent activity")}</strong>
      <ActivityList rows={value.activity} />
    </>
  );
}

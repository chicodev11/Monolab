import { TeamStorage } from "../panels/TeamStorage";
import { listen } from "@tauri-apps/api/event";
import { onWeb } from "../../lib/bridge";
import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import type React from "react";
import type { AccountView, InstanceView, TeamLink } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { DropdownMenu, List, ListRow } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { RolePicker } from "./RolesEditor";
import { Icon } from "../ui/Icon";
import "./MonolinkTeams.css";

/** Monolink: account (Discord sign-in, user ID), teams, roles, invite links. docs/monolink.md section 9.3. */

type Role = "owner" | "admin" | "developer" | "viewer";
type User = { id: string; handle: string; number?: number; avatar: string | null; handle_change_after: number };
type PersonInvite = { team: string; name: string; role: Role; by: string | null };
type TeamSummary = { id: string; name: string; role: Role; version: number };
type Member = User & { role: Role; online: boolean };
type TeamServer = { id: string; name: string; instance_id: string; host_device: string; host: string | null; host_online: boolean; mine: boolean; node?: boolean };
type Device = { id: string; name: string; kind?: string; last_seen: number };

/**
 * The person's monolab-node machines: pair one with a one-time code (typed on the machine), take one away,
 * and put one of its servers in this team (the server id from the node's config).
 */
function Nodes({ team, onChanged, onError, confirm }: { team: string; onChanged: () => void; onError: (message: string) => void; confirm: Confirm }) {
  const [nodes, setNodes] = useState<Device[] | null>(null);
  const [code, setCode] = useState<{ code: string; expires: number } | null>(null);
  const [node, setNode] = useState("");
  const [serverId, setServerId] = useState("");
  const [serverName, setServerName] = useState("");
  // F7: the Owner reads a node's teammate log (who did what on its servers).
  const [log, setLog] = useState<{ node: string; rows: { at: number; who: string; command: string; ok: boolean; error: string }[] } | null>(null);
  const readLog = async (device: string) => {
    if (log?.node === device) {
      setLog(null);
      return;
    }
    try {
      const text = await commands.monolinkCall(device, "__team_log", JSON.stringify({ limit: 200 }), team);
      setLog({ node: device, rows: JSON.parse(text) });
    } catch (cause) {
      onError(problem(cause));
    }
  };
  const load = useCallback(() => {
    api<{ devices: Device[] }>("GET", "/v2/me/devices").then(
      (got) => setNodes(got.devices.filter((one) => one.kind === "node")),
      () => setNodes([]),
    );
  }, []);
  useEffect(load, [load]);
  if (nodes === null) return null;
  const chosen = nodes.some((one) => one.id === node) ? node : (nodes[0]?.id ?? "");
  return (
    <div className="mlt-nodes">
      <div className="mlt-nodes-head">
        <strong>{tr("Nodes")}</strong>
        <small className="muted">{tr("A Linux machine with monolab-node keeps its servers reachable while your PC is off.")}</small>
      </div>
      {nodes.length > 0 && (
        <List variant="card" density="compact" className="mlt-members">
          {nodes.map((one) => (
            <ListRow
              key={one.id}
              leading={<Icon name="server" />}
              title={<>{one.name}</>}
              subtitle={<>{tr("seen {0}", [ago(one.last_seen)])}</>}
              actions={
              <>
              <button type="button" className="button ghost small" aria-expanded={log?.node === one.id} onClick={() => void readLog(one.id)}>
                {tr("Teammate log")}
              </button>
              <button
                type="button"
                className="icon-button"
                aria-label={tr("Remove {0}", [one.name])}
                data-tip={tr("Remove this node")}
                onClick={async () => {
                  if (!(await confirm(tr("Remove {0}?", [one.name]), tr("Its servers leave every team and its sign-in stops working. Pair it again with a new code to bring it back."), tr("Remove"), true))) return;
                  try {
                    await api("DELETE", `/v2/me/devices/${one.id}`);
                    load();
                    onChanged();
                  } catch (cause) {
                    onError(problem(cause));
                  }
                }}
              >
                <Icon name="close" />
              </button>
            </>
            }
            />
          ))}
        </List>
      )}
      {log && (
        <ul className="mlt-activity mlt-node-log" aria-label={tr("Teammate log")}>
          {log.rows.length === 0 && <li className="muted">{tr("Nothing yet.")}</li>}
          {log.rows.map((row, at) => (
            <li key={at}>
              <span>
                <strong>{row.who}</strong> {row.command}
                {!row.ok && <span className="chip subtle">{tr("refused")}</span>}
              </span>
              <small className="muted">{new Date(row.at).toLocaleString()}{row.error ? ` · ${row.error}` : ""}</small>
            </li>
          ))}
        </ul>
      )}
      {code ? (
        <div className="mlt-node-code">
          <p className="muted">{tr("On the Linux machine, run this within 10 minutes:")}</p>
          <code>monolab-node pair {code.code}</code>
          <button type="button" className="button secondary small" onClick={() => void navigator.clipboard.writeText(`monolab-node pair ${code.code}`)}>
            <Icon name="copy" />
            {tr("Copy")}
          </button>
          <button type="button" className="button ghost small" onClick={() => { setCode(null); load(); }}>
            {tr("Done")}
          </button>
        </div>
      ) : (
        <button type="button" className="button secondary small" onClick={() => void api<{ code: string; expires: number }>("POST", "/v2/me/nodes").then(setCode, (cause) => onError(problem(cause)))}>
          <Icon name="plus" />
          {tr("Pair a node")}
        </button>
      )}
      {nodes.length > 0 && (
        <div className="mlt-team-actions">
          <Dropdown value={chosen} onChange={setNode} options={nodes.map((one) => ({ value: one.id, label: one.name }))} />
          <input className="mlt-node-input" placeholder={tr("Server id in its config")} value={serverId} onChange={(event) => setServerId(event.target.value)} />
          <input className="mlt-node-input" placeholder={tr("Name")} value={serverName} onChange={(event) => setServerName(event.target.value)} />
          <button
            type="button"
            className="button primary small"
            disabled={!serverId.trim() || !chosen}
            onClick={async () => {
              try {
                await api("POST", `/v2/teams/${team}/servers`, { instance_id: serverId.trim(), name: serverName.trim() || serverId.trim(), host: chosen });
                setServerId("");
                setServerName("");
                onChanged();
              } catch (cause) {
                onError(problem(cause));
              }
            }}
          >
            {tr("Add node server")}
          </button>
          <small className="muted">{tr("Also list the team and server in the node's config.toml, so it lets the team in.")}</small>
        </div>
      )}
    </div>
  );
}
type Team = { id: string; name: string; owner: string; version: number; role: Role; members: Member[]; servers?: TeamServer[]; waiting: number };
type Invite = {
  id: string;
  prefix: string;
  role: Role;
  created_by: string | null;
  expires: number;
  max_uses: number | null;
  uses: number;
  needs_approval: boolean;
  code?: string;
  url?: string;
};
type Request = { user: User; role: Role; created_at: number };
type Activity = { id: number; actor: string | null; kind: string; data: Record<string, string>; at: number };
type Preview =
  | { valid: true; team: { id: string; name: string; members: number }; by: string | null; role: Role; needs_approval: boolean; expires: number }
  | { valid: false; reason: string };

const rank: Record<Role, number> = { viewer: 0, developer: 1, admin: 2, owner: 3 };
const roleName = (role: Role) => role.charAt(0).toUpperCase() + role.slice(1);
const manages = (role: Role) => rank[role] >= rank.admin;

class ApiError extends Error {
  constructor(
    public code: string,
    public extra: Record<string, unknown>,
  ) {
    super(code);
  }
}

async function api<T>(method: "GET" | "POST" | "PATCH" | "DELETE", path: string, body?: unknown): Promise<T> {
  try {
    const text = await commands.accountApi(method, path, body === undefined ? null : JSON.stringify(body));
    return JSON.parse(text) as T;
  } catch (cause) {
    const text = String(cause);
    try {
      const parsed = JSON.parse(text) as Record<string, unknown>;
      throw new ApiError(String(parsed.error ?? "server"), parsed);
    } catch (inner) {
      if (inner instanceof ApiError) throw inner;
      throw new ApiError("server", { detail: text });
    }
  }
}

/** Words for the codes the account server sends back. */
function problem(cause: unknown): string {
  const code = cause instanceof ApiError ? cause.code : String(cause);
  if (code.startsWith("unreachable")) return tr("Couldn't reach Monolink. Check your internet connection.");
  const words: Record<string, string> = {
    forbidden: tr("Your role in this team doesn't allow that."),
    above_you: tr("You can't give a role higher than your own."),
    peer: tr("Admins can't change or remove other admins."),
    owner: tr("The owner's role can't be changed here."),
    self: tr("You can't change your own role."),
    use_transfer: tr("Hand the team over to make someone the owner."),
    not_admin: tr("Only an admin can become the owner. Make them an admin first."),
    owner_cannot_leave: tr("The owner can't leave. Hand the team over or delete it."),
    owns_team: tr("Hand over or delete the teams you own first."),
    too_many_teams: tr("You already own 10 teams."),
    too_many_invites: tr("This team has too many open invite links. Cancel some first."),
    too_many_devices: tr("Your account has 10 devices. Remove one you don't use."),
    bad_name: tr("Team names are 1 to 40 characters."),
    bad_expiry: tr("Invite links last at most 30 days."),
    taken: tr("That user ID is taken."),
    reserved: tr("That user ID is reserved."),
    too_short: tr("User IDs are at least 3 characters."),
    too_long: tr("User IDs are at most 20 characters."),
    bad_letters: tr("Use only a–z, 0–9 and _."),
    expired: tr("This sign-in expired. Start again."),
    denied: tr("Sign-in was cancelled on Discord."),
    disabled: tr("This account can't be used."),
    not_found: tr("That doesn't exist any more."),
    used_up: tr("This invite link has been used up."),
    revoked: tr("This invite link was cancelled."),
    slow_down: tr("Too many tries. Wait a minute and try again."),
    signed_out: tr("You were signed out. Sign in again."),
    device_revoked: tr("This device was removed from your account."),
    clock: tr("This PC's clock is off. Fix the time and try again."),
    no_such_user: tr("Nobody has that user ID or number."),
    already_member: tr("They're already in this team."),
  };
  if (code === "too_soon" && cause instanceof ApiError && typeof cause.extra.next === "number")
    return tr("You can change your user ID again on {0}.", [new Date(cause.extra.next * 1000).toLocaleDateString()]);
  return words[code] ?? tr("Something went wrong ({0}).", [code]);
}

function ago(at: number) {
  const seconds = Math.max(0, Date.now() / 1000 - at);
  if (seconds < 60) return tr("just now");
  if (seconds < 3600) return tr("{0} min ago", [Math.floor(seconds / 60)]);
  if (seconds < 86_400) return tr("{0} h ago", [Math.floor(seconds / 3600)]);
  return new Date(at * 1000).toLocaleDateString();
}

function until(at: number) {
  const days = Math.round((at - Date.now() / 1000) / 86_400);
  if (days >= 1) return tr("{0} days left", [days]);
  const hours = Math.max(1, Math.round((at - Date.now() / 1000) / 3600));
  return tr("{0} h left", [hours]);
}

function activityText(item: Activity) {
  const who = item.actor ? `@${item.actor}` : tr("Someone");
  const data = item.data ?? {};
  switch (item.kind) {
    case "team_created":
      return tr("{0} made the team", [who]);
    case "team_renamed":
      return tr("{0} renamed the team to {1}", [who, data.to]);
    case "member_joined":
      return tr("@{0} joined as {1}", [data.user, roleName(data.role as Role)]);
    case "person_invited":
      return tr("{0} invited @{1} as {2}", [who, data.user, roleName(data.role as Role)]);
    case "member_left":
      return tr("@{0} left", [data.user]);
    case "member_removed":
      return tr("{0} removed @{1}", [who, data.user]);
    case "role_changed":
      return tr("{0} made @{1} {2}", [who, data.user, roleName(data.to as Role)]);
    case "owner_changed":
      return tr("{0} handed the team to @{1}", [who, data.to]);
    case "handle_changed":
      return tr("@{0} is now @{1}", [data.from, data.to]);
    case "server_attached":
      return tr("{0} added {1}", [who, data.server]);
    case "server_detached":
      return tr("{0} took {1} out", [who, data.server]);
    case "server_started":
      return tr("{0} started", [data.server]);
    case "server_stopped":
      return tr("{0} stopped", [data.server]);
    case "server_crashed":
      return tr("{0} crashed", [data.server]);
    case "file_changed":
      return tr("{0} changed {1} on {2}", [who, data.path, data.server]);
    case "plugin_added":
      return tr("{0} added {1} to {2}", [who, data.name, data.server]);
    case "studio_asset_added":
      return tr("{0} uploaded {1} to the studio of {2}", [who, data.name, data.server]);
    case "storage_added":
      return tr("{0} put {1} in Storage", [who, data.name]);
    case "storage_used":
      return tr("{0} put {1} from Storage into {2}", [who, data.name, data.server]);
    default:
      return `${who} · ${item.kind}`;
  }
}

function Avatar({ user, online }: { user: { handle: string; avatar: string | null }; online?: boolean }) {
  return (
    <span className="mlt-avatar">
      {user.avatar ? <img src={user.avatar} alt="" /> : <span>{user.handle.charAt(0).toUpperCase()}</span>}
      {online !== undefined && <i className={`status-dot status-${online ? "running" : "stopped"}`} aria-label={online ? tr("Online") : tr("Offline")} />}
    </span>
  );
}

type Confirm = (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;

/** Pages beside the team tabs (Activity log, Roles) that App.tsx renders; they share the team's tab row. */
export type MonolinkExtra = { tabs: { value: string; label: string }[]; on: string | null; pick: (value: string | null) => void; content: React.ReactNode };
const ExtraContext = createContext<(MonolinkExtra & { setPanelUp: (up: boolean) => void }) | null>(null);

export function MonolinkTeams({ extra, ...props }: Parameters<typeof MonolinkHome>[0] & { extra?: MonolinkExtra }) {
  const [panelUp, setPanelUp] = useState(false);
  const own = extra && !panelUp;
  return (
    <ExtraContext.Provider value={extra ? { ...extra, setPanelUp } : null}>
      {own && (
        <div className="ui-tabs mlt-tab-row mlt-top-tabs" role="tablist" aria-label="Monolink">
          <button type="button" role="tab" aria-selected={!extra.on} className={`ui-tab ${!extra.on ? "on" : ""}`} onClick={() => extra.pick(null)}>
            {tr("Teams")}
          </button>
          {extra.tabs.map((one) => (
            <button key={one.value} type="button" role="tab" aria-selected={extra.on === one.value} className={`ui-tab ${extra.on === one.value ? "on" : ""}`} onClick={() => extra.pick(one.value)}>
              {one.label}
            </button>
          ))}
        </div>
      )}
      {own && extra.on && extra.content}
      <div className="mlt-home" hidden={!!(own && extra.on)}>
        <MonolinkHome {...props} />
      </div>
    </ExtraContext.Provider>
  );
}

function MonolinkHome({
  invite,
  onInviteSeen,
  onError,
  onNotice,
  confirm,
  onAccount,
}: {
  /** Opens the app's account dialog; when set, signing in happens there instead of here. */
  onAccount?: () => void;
  /** An invite link the app was opened with (monolab://invite/…). */
  invite: string | null;
  onInviteSeen: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: Confirm;
}) {
  const [account, setAccount] = useState<AccountView | null>(null);
  const [signing, setSigning] = useState(false);
  const [suggested, setSuggested] = useState<string | null>(null);
  const user: User | null = account?.user ? (JSON.parse(account.user) as User) : null;

  const load = useCallback(() => {
    void commands.accountState().then(setAccount, () => {});
  }, []);
  useEffect(() => {
    load();
    const off = listen("account-changed", load);
    return () => void off.then((stop) => stop());
  }, [load]);

  async function signIn() {
    setSigning(true);
    try {
      const result = await commands.accountSignIn();
      if (result.state === "needs_handle") setSuggested(result.suggested ?? "");
      else if (result.state === "failed") onError(problem(result.reason ?? "server"));
    } catch (cause) {
      onError(problem(cause));
    }
    setSigning(false);
    load();
  }

  // Signed out: open the account dialog once on the way in, so signing in is the first thing offered.
  const askedAccount = useRef(false);
  const signedOut = account?.status === "signed_out";
  useEffect(() => {
    if (!signedOut || !onAccount || askedAccount.current) return;
    askedAccount.current = true;
    onAccount();
  }, [signedOut, onAccount]);

  if (!account) return null;

  if (suggested !== null) return <PickHandle suggested={suggested} onDone={() => { setSuggested(null); load(); }} onError={onError} />;

  if (account.status === "signed_out") {
    return (
      <section className="settings-group">
        <h3 className="settings-group-title">
          <Icon name="users" />
          {tr("Teams")}
        </h3>
        <div className="settings-group-rows">
          <div className="setting-row">
            <div>
              <strong>{tr("Sign in to Monolink")}</strong>
              <small>
                {tr("Make a team, invite people with a link and build servers together. You can also sign in from the person icon in the left bar.")}
              </small>
            </div>
            {signing ? (
              <div className="mlt-actions">
                <span className="muted">{tr("Finish signing in in your browser…")}</span>
                <button type="button" className="button secondary small" onClick={() => void commands.accountCancelSignIn()}>
                  {tr("Cancel")}
                </button>
              </div>
            ) : (
              <button type="button" className="button primary small" onClick={() => (onAccount ? onAccount() : void signIn())}>
                {tr("Sign in with Discord")}
              </button>
            )}
          </div>
          {invite && (
            <div className="setting-row">
              <div>
                <strong>{tr("You opened an invite link")}</strong>
                <small>{tr("Sign in first, then you can join the team.")}</small>
              </div>
            </div>
          )}
        </div>
      </section>
    );
  }

  return <Teams user={user} invite={invite} onInviteSeen={onInviteSeen} onError={onError} onNotice={onNotice} confirm={confirm} />;
}

/** Settings → Account: sign-in, user ID (handle) and the permanent number, team invites sent to you. */
export function AccountSettings({ onError, onNotice, confirm }: { onError: (message: string) => void; onNotice: (message: string) => void; confirm: Confirm }) {
  const [account, setAccount] = useState<AccountView | null>(null);
  const [signing, setSigning] = useState(false);
  const [suggested, setSuggested] = useState<string | null>(null);
  const [editHandle, setEditHandle] = useState<string | null>(null);
  const [invites, setInvites] = useState<PersonInvite[]>([]);
  const user: User | null = account?.user ? (JSON.parse(account.user) as User) : null;
  const signedIn = account?.status === "signed_in";

  const load = useCallback(() => {
    void commands.accountState().then(setAccount, () => {});
  }, []);
  const loadInvites = useCallback(() => {
    void api<{ invites: PersonInvite[] }>("GET", "/v2/me/invites").then((answer) => setInvites(answer.invites), () => setInvites([]));
  }, []);
  useEffect(() => {
    load();
    const off = listen("account-changed", load);
    const live = listen<Record<string, unknown>>("account-live", (event) => {
      if (event.payload.type === "notice") loadInvites();
    });
    return () => {
      void off.then((stop) => stop());
      void live.then((stop) => stop());
    };
  }, [load, loadInvites]);
  useEffect(() => {
    if (signedIn) loadInvites();
  }, [signedIn, loadInvites]);

  async function signIn() {
    setSigning(true);
    try {
      const result = await commands.accountSignIn();
      if (result.state === "needs_handle") setSuggested(result.suggested ?? "");
      else if (result.state === "failed") onError(problem(result.reason ?? "server"));
    } catch (cause) {
      onError(problem(cause));
    }
    setSigning(false);
    load();
  }

  async function saveHandle() {
    if (editHandle === null) return;
    try {
      await api("PATCH", "/v2/me", { handle: editHandle.replace(/^@/, "") });
      setEditHandle(null);
      load();
    } catch (cause) {
      onError(problem(cause));
    }
  }

  async function signOut() {
    if (!(await confirm(tr("Sign out of Monolink?"), tr("Teams stay as they are. Sign in again with Discord to get back to them."), tr("Sign out")))) return;
    await commands.accountSignOut();
    load();
  }

  async function answer(invite: PersonInvite, accept: boolean) {
    try {
      await api("POST", `/v2/me/invites/${invite.team}`, { accept });
      onNotice(accept ? tr("You joined {0}.", [invite.name]) : tr("Turned down the invite to {0}.", [invite.name]));
      loadInvites();
    } catch (cause) {
      onError(problem(cause));
      loadInvites();
    }
  }

  async function copyNumber(number: number) {
    try {
      await navigator.clipboard.writeText(String(number));
      onNotice(tr("Number copied."));
    } catch {
      onError(tr("Couldn't copy."));
    }
  }

  if (!account) return null;
  if (suggested !== null) return <PickHandle suggested={suggested} onDone={() => { setSuggested(null); load(); }} onError={onError} />;

  if (account.status === "signed_out") {
    return (
      <section className="settings-group">
        <h3 className="settings-group-title">
          <Icon name="user" />
          {tr("Account")}
        </h3>
        <div className="settings-group-rows">
          <div className="setting-row">
            <div>
              <strong>{tr("Not signed in")}</strong>
              <small>
                {tr("A Monolab account is needed for Monolink teams. Your user ID, number, Discord account number, devices and teams are kept on mnlab.cc. Server files, console output and commands never go there.")}
              </small>
            </div>
            {signing ? (
              <div className="mlt-actions">
                <span className="muted">{tr("Finish signing in in your browser…")}</span>
                <button type="button" className="button secondary small" onClick={() => void commands.accountCancelSignIn()}>
                  {tr("Cancel")}
                </button>
              </div>
            ) : (
              <button type="button" className="button primary small" onClick={() => void signIn()}>
                {tr("Sign in with Discord")}
              </button>
            )}
          </div>
        </div>
      </section>
    );
  }

  const liveDot = account.live === "online" ? "running" : account.live === "off" ? "stopped" : "starting";

  return (
    <>
      <section className="settings-group">
        <h3 className="settings-group-title">
          <Icon name="user" />
          {tr("Account")}
        </h3>
        <div className="settings-group-rows">
          <div className="setting-row">
            <div className="mlt-me">
              {user && <Avatar user={user} />}
              <div>
                <strong>{user ? `@${user.handle}` : tr("Signed in")}</strong>
                <small className="mlt-live">
                  <span className={`status-dot status-${account.status === "offline" ? "crashed" : liveDot}`} />
                  {account.status === "offline" ? problem(account.error ?? "unreachable") : account.live === "online" ? tr("Connected") : tr("Connecting…")}
                </small>
              </div>
            </div>
            <div className="mlt-actions">
              {user && editHandle === null && (
                <button
                  type="button"
                  className="button secondary small"
                  onClick={() => {
                    if (Date.now() / 1000 < user.handle_change_after)
                      onError(tr("You can change your user ID again on {0}.", [new Date(user.handle_change_after * 1000).toLocaleDateString()]));
                    else setEditHandle(user.handle);
                  }}
                >
                  {tr("Change ID")}
                </button>
              )}
              <button type="button" className="button secondary small" onClick={() => void signOut()}>
                {tr("Sign out")}
              </button>
            </div>
          </div>
          {user?.number ? (
            <div className="setting-row">
              <div>
                <strong>{tr("Number")}</strong>
                <small>{tr("Your permanent number. It never changes, even when you change your user ID. People can invite you with it or with @{0}.", [user.handle])}</small>
              </div>
              <div className="mlt-actions">
                <code className="mlt-number">{user.number}</code>
                <button type="button" className="icon-button" aria-label={tr("Copy number")} data-tip={tr("Copy number")} onClick={() => void copyNumber(user.number ?? 0)}>
                  <Icon name="copy" />
                </button>
              </div>
            </div>
          ) : null}
          {editHandle !== null && (
            <div className="setting-row">
              <div>
                <strong>{tr("New user ID")}</strong>
                <small>{tr("After this change you can't change it again for 30 days. Your old ID stays reserved for you for 30 days.")}</small>
              </div>
              <div className="setting-input mlt-handle">
                <span className="mlt-at">@</span>
                <input value={editHandle} maxLength={20} spellCheck={false} autoFocus aria-label={tr("New user ID")} onChange={(event) => setEditHandle(event.target.value.toLowerCase())} />
                <button type="button" className="button primary small" onClick={() => void saveHandle()}>
                  {tr("Save")}
                </button>
                <button type="button" className="button secondary small" onClick={() => setEditHandle(null)}>
                  {tr("Cancel")}
                </button>
              </div>
            </div>
          )}
        </div>
      </section>

      {invites.length > 0 && (
        <section className="settings-group">
          <h3 className="settings-group-title">
            <Icon name="users" />
            {tr("Team invites")}
          </h3>
          <div className="settings-group-rows">
            {invites.map((invite) => (
              <div key={invite.team} className="setting-row">
                <div>
                  <strong>{invite.name}</strong>
                  <small>{tr("@{0} invited you as {1}.", [invite.by ?? "?", roleName(invite.role)])}</small>
                </div>
                <div className="mlt-actions">
                  <button type="button" className="button primary small" onClick={() => void answer(invite, true)}>
                    {tr("Join")}
                  </button>
                  <button type="button" className="button secondary small" onClick={() => void answer(invite, false)}>
                    {tr("Turn down")}
                  </button>
                </div>
              </div>
            ))}
          </div>
        </section>
      )}
    </>
  );
}

function PickHandle({ suggested, onDone, onError }: { suggested: string; onDone: () => void; onError: (message: string) => void }) {
  const [handle, setHandle] = useState(suggested);
  const [check, setCheck] = useState<{ ok: boolean; reason: string | null } | null>(null);
  const [busy, setBusy] = useState(false);
  const turn = useRef(0);

  useEffect(() => {
    const mine = ++turn.current;
    setCheck(null);
    if (!handle.trim()) return;
    const timer = window.setTimeout(() => {
      void commands.accountCheckHandle(handle).then(
        (answer) => mine === turn.current && setCheck({ ok: answer.ok, reason: answer.reason }),
        () => {},
      );
    }, 300);
    return () => window.clearTimeout(timer);
  }, [handle]);

  async function finish() {
    setBusy(true);
    try {
      await commands.accountFinish(handle.trim().replace(/^@/, ""));
      onDone();
    } catch (cause) {
      onError(problem(cause));
      setBusy(false);
    }
  }

  return (
    <section className="settings-group">
      <h3 className="settings-group-title">
        <Icon name="user" />
        {tr("Pick your user ID")}
      </h3>
      <div className="settings-group-rows">
        <div className="setting-row">
          <div>
            <strong>{tr("User ID")}</strong>
            <small>{tr("Everyone sees this ID in teams, the console and the activity list. There's no separate nickname. After a change you can't change it again for 30 days.")}</small>
          </div>
          <div className="setting-input mlt-handle">
            <span className="mlt-at">@</span>
            <input
              value={handle}
              maxLength={20}
              spellCheck={false}
              autoFocus
              aria-label={tr("User ID")}
              onChange={(event) => setHandle(event.target.value.toLowerCase())}
              onKeyDown={(event) => event.key === "Enter" && check?.ok && !busy && void finish()}
            />
          </div>
        </div>
        <div className="setting-row">
          <div>
            <small className={check && !check.ok ? "mlt-bad" : undefined}>
              {check === null ? tr("a–z, 0–9 and _, 3 to 20 characters.") : check.ok ? tr("Available") : problem(check.reason ?? "taken")}
            </small>
          </div>
          <button type="button" className="button primary small" disabled={!check?.ok || busy} onClick={() => void finish()}>
            {tr("Create account")}
          </button>
        </div>
      </div>
    </section>
  );
}

function Teams({
  user,
  invite,
  onInviteSeen,
  onError,
  onNotice,
  confirm,
}: {
  user: User | null;
  invite: string | null;
  onInviteSeen: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: Confirm;
}) {
  const [teams, setTeams] = useState<TeamSummary[] | null>(null);
  const [openId, setOpenIdState] = useState<string | null>(() => {
    try {
      return localStorage.getItem("monolab.team");
    } catch {
      return null;
    }
  });
  const [adding, setAdding] = useState(false);
  const [newName, setNewName] = useState("");
  const [link, setLink] = useState("");
  const [preview, setPreview] = useState<{ code: string; preview: Preview } | null>(null);
  const [online, setOnline] = useState<Record<string, boolean>>({});
  const [bump, setBump] = useState(0);

  const setOpenId = useCallback((next: string | null) => {
    setOpenIdState(next);
    setAdding(false);
    try {
      if (next) localStorage.setItem("monolab.team", next);
      else localStorage.removeItem("monolab.team");
    } catch {
      // Only remembers the choice.
    }
  }, []);

  const loadTeams = useCallback(() => {
    void api<{ teams: TeamSummary[] }>("GET", "/v2/me").then(
      (me) => setTeams(me.teams),
      () => setTeams((old) => old ?? []),
    );
  }, []);
  useEffect(loadTeams, [loadTeams]);

  // Live messages from mnlab.cc: who came online, team changes, notices.
  useEffect(() => {
    const off = listen<Record<string, unknown>>("account-live", (event) => {
      const message = event.payload;
      if (message.type === "presence") setOnline((old) => ({ ...old, [String(message.user)]: Boolean(message.online) }));
      if (message.type === "team_changed" || message.type === "activity") {
        loadTeams();
        setBump((value) => value + 1);
      }
      if (message.type === "notice") {
        const data = (message.data ?? {}) as Record<string, string>;
        if (message.kind === "join_request") onNotice(tr("@{0} wants to join {1}.", [data.user, data.name]));
        if (message.kind === "joined") onNotice(tr("You joined {0}.", [data.name]));
        if (message.kind === "rejected") onNotice(tr("Your request to join {0} was turned down.", [data.name]));
        if (message.kind === "removed") onNotice(tr("You were removed from {0}.", [data.name]));
        if (message.kind === "team_deleted") onNotice(tr("{0} was deleted.", [data.name]));
        // "invited" is told app-wide (App.tsx), wherever the user is.
        if (message.kind === "role_changed") onNotice(tr("Your role is now {0}.", [roleName(data.role as Role)]));
        loadTeams();
        setBump((value) => value + 1);
      }
    });
    return () => void off.then((stop) => stop());
  }, [loadTeams, onNotice]);

  const look = useCallback(
    async (text: string) => {
      try {
        const code = text.trim().replace(/\/+$/, "").split("/").pop() ?? "";
        const answer = await api<Preview>("GET", `/v2/invites/${encodeURIComponent(code)}`);
        setPreview({ code, preview: answer });
        setAdding(true);
      } catch (cause) {
        onError(problem(cause));
      }
    },
    [onError],
  );

  // An invite link the app was opened with.
  useEffect(() => {
    if (!invite) return;
    void look(invite);
    onInviteSeen();
  }, [invite, look, onInviteSeen]);

  async function join() {
    if (!preview) return;
    try {
      const answer = await api<{ state: string; team: string; name: string }>("POST", `/v2/invites/${encodeURIComponent(preview.code)}/accept`);
      if (answer.state === "waiting") onNotice(tr("Asked to join {0}. You'll be in once an admin approves.", [answer.name]));
      else {
        onNotice(answer.state === "member" ? tr("You're already in {0}.", [answer.name]) : tr("You joined {0}.", [answer.name]));
        setOpenId(answer.team);
      }
      setPreview(null);
      setLink("");
      loadTeams();
    } catch (cause) {
      onError(problem(cause));
    }
  }

  async function create() {
    try {
      const team = await api<Team>("POST", "/v2/teams", { name: newName.trim() });
      setNewName("");
      loadTeams();
      setOpenId(team.id);
    } catch (cause) {
      onError(problem(cause));
    }
  }

  const list = teams ?? [];
  const shown = adding ? null : list.find((team) => team.id === openId) ?? list[0] ?? null;
  const addView = teams !== null && (adding || !shown);

  const switcher = (
    <DropdownMenu
      align="start"
      trigger={
        <button type="button" className="mlt-switcher" aria-label={tr("Switch team")} data-tip={tr("Switch team")}>
          <Icon name="chevron" />
        </button>
      }
      items={[
        ...list.map((team, index) => ({
          heading: index === 0 ? tr("Your teams") : undefined,
          label: team.name,
          hint: roleName(team.role),
          checked: !addView && shown?.id === team.id,
          onSelect: () => setOpenId(team.id),
        })),
        { heading: list.length ? " " : undefined, label: tr("Make or join a team"), icon: "plus", onSelect: () => setAdding(true) },
      ]}
    />
  );

  return (
    <div className="mlp mlp-single">
      <section className="mlp-main">
        {teams === null ? (
          <p className="muted mlp-loading">{tr("Loading…")}</p>
        ) : addView ? (
          <div className="mlp-add-view">
            <header className="mlp-add-head">
              <h2>{tr("Make or join a team")}</h2>
              <p className="muted">{tr("A team shares servers between PCs. Make your own, or join one with an invite link.")}</p>
            </header>
            {preview && (
              <div className={`mlp-invite ${preview.preview.valid ? "" : "bad"}`}>
                {preview.preview.valid ? (
                  <>
                    <span className="mlp-mark big" aria-hidden>{preview.preview.team.name.slice(0, 1).toUpperCase()}</span>
                    <div className="mlp-invite-text">
                      <strong>{tr("Join {0}?", [preview.preview.team.name])}</strong>
                      <small>
                        {tr("@{0} invited you as {1}. {2} members.", [preview.preview.by ?? "?", roleName(preview.preview.role), preview.preview.team.members])}
                        {preview.preview.needs_approval ? " " + tr("An admin approves before you're in.") : ""}
                      </small>
                    </div>
                    <div className="mlt-actions">
                      <button type="button" className="button secondary small" onClick={() => setPreview(null)}>
                        {tr("Not now")}
                      </button>
                      <button type="button" className="button primary small" onClick={() => void join()}>
                        {preview.preview.needs_approval ? tr("Ask to join") : tr("Join")}
                      </button>
                    </div>
                  </>
                ) : (
                  <>
                    <Icon name="alert" />
                    <div className="mlp-invite-text">
                      <strong>{tr("This invite link can't be used")}</strong>
                      <small>{problem(preview.preview.reason)}</small>
                    </div>
                    <button type="button" className="button secondary small" onClick={() => setPreview(null)}>
                      {tr("Close")}
                    </button>
                  </>
                )}
              </div>
            )}
            <div className="mlp-choices">
              <section className="mlp-choice">
                <span className="mlp-choice-icon" aria-hidden><Icon name="plus" /></span>
                <h3>{tr("Make a team")}</h3>
                <p className="muted">{tr("You become its owner. Add your servers and invite people afterwards.")}</p>
                <label className="field">
                  <span>{tr("Team name")}</span>
                  <input value={newName} maxLength={40} placeholder={tr("Team name")} onChange={(event) => setNewName(event.target.value)} onKeyDown={(event) => event.key === "Enter" && newName.trim() && void create()} />
                </label>
                <button type="button" className="button primary" disabled={!newName.trim()} onClick={() => void create()}>
                  {tr("Make team")}
                </button>
              </section>
              <section className="mlp-choice">
                <span className="mlp-choice-icon" aria-hidden><Icon name="link" /></span>
                <h3>{tr("Join a team")}</h3>
                <p className="muted">{tr("Paste the invite link someone sent you. You see the team before you join.")}</p>
                <label className="field">
                  <span>{tr("Invite link")}</span>
                  <input value={link} placeholder="https://mnlab.cc/invite/…" spellCheck={false} onChange={(event) => setLink(event.target.value)} onKeyDown={(event) => event.key === "Enter" && link.trim() && void look(link)} />
                </label>
                <button type="button" className="button secondary" disabled={!link.trim()} onClick={() => void look(link)}>
                  {tr("Look up invite")}
                </button>
              </section>
            </div>
            {list.length > 0 && (
              <button type="button" className="button ghost small mlp-back" onClick={() => setAdding(false)}>
                <Icon name="arrow-left" />
                {tr("Back to {0}", [(list.find((team) => team.id === openId) ?? list[0]).name])}
              </button>
            )}
          </div>
        ) : shown && user ? (
          <TeamPanel
            key={shown.id}
            id={shown.id}
            me={user}
            online={online}
            bump={bump}
            onError={onError}
            onNotice={onNotice}
            confirm={confirm}
            onGone={() => {
              setOpenId(null);
              loadTeams();
            }}
            onChanged={loadTeams}
            switcher={switcher}
          />
        ) : null}
      </section>
    </div>
  );
}

function TeamPanel({
  id,
  me,
  online,
  bump,
  onError,
  onNotice,
  confirm,
  onGone,
  onChanged,
  switcher,
}: {
  switcher?: React.ReactNode;
  id: string;
  me: User;
  online: Record<string, boolean>;
  bump: number;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: Confirm;
  onGone: () => void;
  onChanged: () => void;
}) {
  const [team, setTeam] = useState<Team | null>(null);
  const [tab, setTab] = useState<"servers" | "members" | "invites" | "activity" | "storage" | "settings">("servers");
  const [links, setLinks] = useState<TeamLink[]>([]);
  const [mineList, setMineList] = useState<InstanceView[]>([]);
  const [invites, setInvites] = useState<Invite[]>([]);
  const [requests, setRequests] = useState<Request[]>([]);
  const [activity, setActivity] = useState<Activity[]>([]);
  const [made, setMade] = useState<Invite | null>(null);
  const [draft, setDraft] = useState({ role: "developer" as Role, days: "7", uses: "0", approval: true });
  const [rename, setRename] = useState<string | null>(null);
  const [person, setPerson] = useState({ who: "", role: "developer" as Role });
  const [handTo, setHandTo] = useState("");
  const [access, setAccess] = useState<string | null>(null);
  const extra = useContext(ExtraContext);
  const setPanelUp = extra?.setPanelUp;
  useEffect(() => {
    setPanelUp?.(true);
    return () => setPanelUp?.(false);
  }, [setPanelUp]);
  const extraOn = extra?.on ?? null;

  const load = useCallback(() => {
    void api<Team>("GET", `/v2/teams/${id}`).then(
      (next) => {
        setTeam(next);
        if (manages(next.role)) {
          void api<{ invites: Invite[] }>("GET", `/v2/teams/${id}/invites`).then((answer) => setInvites(answer.invites), () => {});
          void api<{ requests: Request[] }>("GET", `/v2/teams/${id}/requests`).then((answer) => setRequests(answer.requests), () => {});
        }
        void api<{ items: Activity[] }>("GET", `/v2/teams/${id}/activity`).then((answer) => setActivity(answer.items), () => {});
      },
      (cause) => {
        if (cause instanceof ApiError && cause.code === "not_found") onGone();
        else onError(problem(cause));
      },
    );
  }, [id, onError, onGone]);
  useEffect(load, [load, bump]);
  useEffect(() => {
    void commands.monolinkTeamLinks().then(setLinks, () => {});
  }, [bump]);
  useEffect(() => {
    if ((tab !== "servers" && tab !== "storage") || !team) return;
    void commands.listInstances().then(setMineList, () => {});
  }, [tab, team]);
  // F6: this PC keeps the team's Storage when it hosts one of the team's servers.
  const hosted = links.find((one) => one.team === id)?.servers.map((one) => one.instance) ?? [];

  if (!team) return <div className="mlt-panel muted">{tr("Loading…")}</div>;
  const mine = team.role;

  async function run(work: () => Promise<unknown>) {
    try {
      await work();
      load();
      onChanged();
    } catch (cause) {
      onError(problem(cause));
    }
  }

  const servers = team.servers ?? [];
  const attachedHere = new Set(links.flatMap((link) => link.servers.map((one) => one.instance)));
  const attachable = mineList.filter((one) => !attachedHere.has(one.id));
  const serverLink = (instance: string) => links.find((link) => link.team === id)?.servers.find((one) => one.instance === instance);
  const others = team.members.filter((member) => member.id !== me.id);
  const setAgents = (instance: string, on: boolean, people: string[]) => void local(() => commands.monolinkTeamAgents(id, instance, on, people));
  const adminOnly = (instance: string) => links.some((link) => link.team === id && link.servers.some((one) => one.instance === instance && one.adminOnly));
  const reviewing = (instance: string) => links.some((link) => link.team === id && link.servers.some((one) => one.instance === instance && one.proposals));
  /** This PC hosts servers in the team, so the host may give teammates a role of its own on them. */
  const hostsHere = links.some((link) => link.team === id && link.servers.length > 0);
  const hereRole = (handle: string) => links.find((link) => link.team === id)?.people?.find((one) => one.handle === handle)?.role ?? "";

  /** Opens a teammate's server as a team view over this window, through Monolink with this team's grant. */
  function openServer(server: TeamServer) {
    const name = server.host ? `@${server.host}` : team?.name ?? "";
    void commands.monolinkOpen(server.host_device, name, id, server.instance_id).catch((cause) => onError(String(cause)));
  }

  async function local(work: () => Promise<TeamLink[]>) {
    try {
      setLinks(await work());
      load();
      onChanged();
    } catch (cause) {
      onError(String(cause).startsWith("{") ? problem(new ApiError(String((JSON.parse(String(cause)) as { error?: string }).error ?? "server"), {})) : String(cause));
    }
  }

  const roleChoices = (target: Member): Role[] =>
    (["admin", "developer", "viewer"] as Role[]).filter((role) => rank[role] <= rank[mine] && !(mine === "admin" && target.role === "admin"));

  async function makeInvite() {
    try {
      const body = {
        role: draft.role,
        expires_in: Number(draft.days) * 86_400 || 7 * 86_400,
        max_uses: Number(draft.uses) > 0 ? Number(draft.uses) : null,
        needs_approval: draft.approval,
      };
      const invite = await api<Invite>("POST", `/v2/teams/${id}/invites`, body);
      setMade(invite);
      load();
    } catch (cause) {
      onError(problem(cause));
    }
  }

  async function invitePerson() {
    try {
      const answer = await api<{ user: User }>("POST", `/v2/teams/${id}/members`, { who: person.who.trim(), role: person.role });
      onNotice(tr("Invited @{0}. They join once they accept.", [answer.user.handle]));
      setPerson((old) => ({ ...old, who: "" }));
      load();
    } catch (cause) {
      onError(problem(cause));
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      onNotice(tr("Invite link copied."));
    } catch {
      onError(tr("Couldn't copy. Select the link and copy it yourself."));
    }
  }

  const memberRows = team.members.map((member) => {
    const isMe = member.id === me.id;
    const isOnline = online[member.id] ?? member.online;
    const canEdit = manages(mine) && !isMe && member.role !== "owner" && !(mine === "admin" && member.role === "admin");
    return (
      <ListRow
        key={member.id}
        leading={<Avatar user={member} online={isOnline} />}
        title={<>@{member.handle}</>}
        subtitle={isMe ? tr("you") : undefined}
        actions={
        <>
        {canEdit ? (
          <span className="mlt-role">
            <Dropdown
              value={member.role}
              align="end"
              noPills
              onChange={(next) => void run(() => api("PATCH", `/v2/teams/${id}/members/${member.id}`, { role: next }))}
              options={roleChoices(member).map((role) => ({ value: role, label: roleName(role) }))}
            />
          </span>
        ) : (
          <span className="chip">{roleName(member.role)}</span>
        )}
        {hostsHere && !isMe && (
          <span className="mlt-here-role" data-tip={tr("What they may do on your servers in this team. Set on this PC only.")}>
            <RolePicker
              inherit={tr("On my servers: team role")}
              value={hereRole(member.handle)}
              onChange={(role) => void local(() => commands.monolinkTeamPersonRole(id, member.handle, role))}
            />
          </span>
        )}
        {canEdit && (
          <button
            type="button"
            className="icon-button"
            aria-label={tr("Remove @{0}", [member.handle])}
            data-tip={tr("Remove from team")}
            onClick={async () => {
              if (await confirm(tr("Remove @{0}?", [member.handle]), tr("They lose access to this team's servers right away. They can come back with a new invite."), tr("Remove"), true))
                void run(() => api("DELETE", `/v2/teams/${id}/members/${member.id}`));
            }}
          >
            <Icon name="close" />
          </button>
        )}
        {mine === "owner" && member.role === "admin" && (
          <button
            type="button"
            className="button secondary small"
            onClick={async () => {
              if (await confirm(tr("Hand {0} to @{1}?", [team.name, member.handle]), tr("They become the owner and you become an admin. Only they can hand it back."), tr("Hand over"), true))
                void run(() => api("POST", `/v2/teams/${id}/transfer`, { user: member.id }));
            }}
          >
            {tr("Make owner")}
          </button>
        )}
      </>
      }
      />
    );
  });

  const admins = others.filter((member) => member.role === "admin");
  return (
    <div className="mlt-panel mlt-view">
      <header className="mlt-view-head">
        <span className="mlp-mark big" aria-hidden>{team.name.slice(0, 1).toUpperCase()}</span>
        <div className="mlt-view-title">
          <div className="mlt-view-name">
            <h2>{team.name}</h2>
            {switcher}
          </div>
          <small className="muted">
            {roleName(mine)} · {tr("{0} members", [team.members.length])} · {tr("{0} servers", [servers.length])}
          </small>
        </div>
        <div className="mlt-view-side">
          {manages(mine) && requests.length > 0 && tab !== "invites" && (
            <button type="button" className="button secondary small" onClick={() => setTab("invites")}>
              {tr("{0} waiting to join", [requests.length])}
            </button>
          )}
        </div>
        <div className="ui-tabs mlt-tab-row" role="tablist" aria-label={tr("Team")}>
          {(["servers", "members", ...(manages(mine) ? ["invites"] : []), "activity", ...(hosted.length ? ["storage"] : []), "settings"] as const).map((one) => (
            <button key={one} type="button" role="tab" aria-selected={!extraOn && tab === one} className={`ui-tab ${!extraOn && tab === one ? "on" : ""}`} onClick={() => { extra?.pick(null); setTab(one as typeof tab); }}>
              {one === "servers" ? tr("Servers") : one === "members" ? tr("Members") : one === "invites" ? tr("Invites") : one === "storage" ? tr("Storage") : one === "settings" ? tr("Settings") : tr("Activity")}
              {one === "invites" && requests.length > 0 && <b className="mlt-count">{requests.length}</b>}
            </button>
          ))}
          {extra?.tabs.map((one) => (
            <button key={one.value} type="button" role="tab" aria-selected={extraOn === one.value} className={`ui-tab ${extraOn === one.value ? "on" : ""}`} onClick={() => extra.pick(one.value)}>
              {one.label}
            </button>
          ))}
        </div>
      </header>
      <div className="mlt-view-grid">
      <div className="mlt-view-main">
      {extraOn && <div className="mlt-extra">{extra?.content}</div>}
      {!extraOn && (
      <>

      {tab === "servers" && (
        <>
          {manages(mine) && (
            <div className="mlt-section-head">
              <small className="muted">{tr("Teammates reach your servers through this PC while Monolab is open.")}</small>
              <DropdownMenu
                trigger={
                  <button type="button" className="button primary small" disabled={attachable.length === 0} data-tip={attachable.length === 0 ? tr("All your servers are already in a team.") : undefined}>
                    <Icon name="plus" />
                    {tr("Add server")}
                  </button>
                }
                items={attachable.map((one) => ({ label: one.name || one.id, icon: "server", onSelect: () => void local(() => commands.monolinkTeamAttach(id, one.id)) }))}
              />
            </div>
          )}
          {servers.length === 0 ? (
            <p className="muted mlt-empty">
              {manages(mine) ? tr("No servers yet. Add one of yours below, and the team can open it.") : tr("No servers yet. An admin can add one from their PC.")}
            </p>
          ) : (
            <List variant="card" density="compact" className="mlt-members">
              {servers.map((server) => (
                <ListRow
                  key={server.id}
                  leading={
                    <span className={`mlt-server-mark${server.mine || server.host_online ? "" : " off"}`} data-tip={server.mine || server.host_online ? tr("Online") : tr("Offline")}>
                      <Icon name="server" />
                    </span>
                  }
                  title={<>{server.name}</>}
                  subtitle={<>{server.mine ? tr("on this PC") : server.host ? `@${server.host}` : tr("host left")}{server.node ? ` · ${tr("node")}` : ""}</>}
                  actions={
                  <>
                  {!server.mine && (
                    <button type="button" className="button secondary small" disabled={!server.host_online} onClick={() => openServer(server)}>
                      {server.host_online ? tr("Open") : tr("Offline")}
                    </button>
                  )}
                  {server.mine && (
                    <button
                      type="button"
                      className={`icon-button ${access === server.id ? "on" : ""}`}
                      aria-expanded={access === server.id}
                      aria-label={tr("What teammates may do on {0}", [server.name])}
                      data-tip={tr("What teammates may do")}
                      onClick={() => setAccess((old) => (old === server.id ? null : server.id))}
                    >
                      <Icon name="sliders" />
                    </button>
                  )}
                  {!server.mine && manages(mine) && (
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={tr("Take {0} out of the team", [server.name])}
                      data-tip={tr("Take out of the team")}
                      onClick={async () => {
                        if (await confirm(tr("Take {0} out of {1}?", [server.name, team.name]), tr("Teammates lose access to it right away. The server itself stays as it is."), tr("Take out"), true))
                          void run(() => api("DELETE", `/v2/teams/${id}/servers/${server.id}`));
                      }}
                    >
                      <Icon name="close" />
                    </button>
                  )}
                </>
                }
                detail={
                  server.mine && access === server.id ? (
                    <div className="mlt-access">
                      <label className="mlt-access-row">
                        <span className="mlt-access-text">
                          <strong>{tr("Commands: admins only")}</strong>
                          <small className="muted">{tr("Only admins and the owner can send console commands.")}</small>
                        </span>
                        <span className="switch">
                          <input
                            type="checkbox"
                            aria-label={tr("Commands: admins only")}
                            checked={adminOnly(server.instance_id)}
                            onChange={(event) => void local(() => commands.monolinkTeamAdminOnly(id, server.instance_id, event.target.checked))}
                          />
                          <span />
                        </span>
                      </label>
                      <label className="mlt-access-row">
                        <span className="mlt-access-text">
                          <strong>{tr("Review changes")}</strong>
                          <small className="muted">{tr("Developers' file and studio changes wait until you or an admin merge them (History tab).")}</small>
                        </span>
                        <span className="switch">
                          <input
                            type="checkbox"
                            aria-label={tr("Review changes")}
                            checked={reviewing(server.instance_id)}
                            onChange={(event) => void local(() => commands.monolinkTeamProposals(id, server.instance_id, event.target.checked))}
                          />
                          <span />
                        </span>
                      </label>
                      <label className="mlt-access-row">
                        <span className="mlt-access-text">
                          <strong>{tr("Teammates' agents")}</strong>
                          <small className="muted">{tr("Teammates' own agents may work on this server. You approve every change they make.")}</small>
                        </span>
                        <span className="switch">
                          <input
                            type="checkbox"
                            aria-label={tr("Teammates' agents")}
                            checked={serverLink(server.instance_id)?.agents ?? false}
                            onChange={(event) => setAgents(server.instance_id, event.target.checked, serverLink(server.instance_id)?.agentPeople ?? [])}
                          />
                          <span />
                        </span>
                      </label>
                      {!serverLink(server.instance_id)?.agents && others.length > 0 && (
                        <div className="mlt-access-row">
                          <span className="mlt-access-text">
                            <strong>{tr("Only some agents")}</strong>
                            <small className="muted">{tr("Pick whose agents may work here while the switch above is off.")}</small>
                          </span>
                          <DropdownMenu
                            trigger={
                              <button type="button" className="button secondary small">
                                {(serverLink(server.instance_id)?.agentPeople ?? []).length
                                  ? tr("Agents: {0} allowed", [(serverLink(server.instance_id)?.agentPeople ?? []).length])
                                  : tr("Allow some agents")}
                              </button>
                            }
                            items={others.map((member) => {
                              const people = serverLink(server.instance_id)?.agentPeople ?? [];
                              const on = people.some((one) => one.toLowerCase() === member.handle.toLowerCase());
                              return {
                                label: tr("@{0}'s agent", [member.handle]),
                                icon: "user",
                                checked: on,
                                onSelect: () =>
                                  setAgents(server.instance_id, false, on ? people.filter((one) => one.toLowerCase() !== member.handle.toLowerCase()) : [...people, member.handle]),
                              };
                            })}
                          />
                        </div>
                      )}
                      <div className="mlt-access-row">
                        <span className="mlt-access-text">
                          <strong>{tr("Take out of the team")}</strong>
                          <small className="muted">{tr("Teammates lose access to it right away. The server itself stays as it is.")}</small>
                        </span>
                        <button
                          type="button"
                          className="button secondary small"
                          onClick={async () => {
                            if (await confirm(tr("Take {0} out of {1}?", [server.name, team.name]), tr("Teammates lose access to it right away. The server itself stays as it is."), tr("Take out"), true))
                              void local(() => commands.monolinkTeamDetach(id, server.instance_id));
                          }}
                        >
                          {tr("Take out")}
                        </button>
                      </div>
                    </div>
                  ) : undefined
                }
                />
              ))}
            </List>
          )}
        </>
      )}

      {tab === "members" && (
        <>
          {hostsHere && <p className="muted mlt-empty">{tr("You can give teammates a role of your own on the servers this PC shares with the team. Their team role still decides team matters.")}</p>}
          <List variant="card" density="compact" className="mlt-members">{memberRows}</List>
          {manages(mine) && (
            <div className="mlt-team-actions">
              <span className="setting-input relay-join mlt-invite-person">
                <input
                  value={person.who}
                  placeholder={tr("@user ID or number")}
                  spellCheck={false}
                  aria-label={tr("Invite by user ID or number")}
                  onChange={(event) => setPerson((old) => ({ ...old, who: event.target.value }))}
                  onKeyDown={(event) => event.key === "Enter" && person.who.trim() && void invitePerson()}
                />
                <Dropdown
                  value={person.role}
                  onChange={(next) => setPerson((old) => ({ ...old, role: next as Role }))}
                  options={(["admin", "developer", "viewer"] as Role[]).filter((role) => rank[role] <= rank[mine]).map((role) => ({ value: role, label: roleName(role) }))}
                />
                <button type="button" className="button primary small" disabled={!person.who.trim()} onClick={() => void invitePerson()}>
                  {tr("Invite")}
                </button>
              </span>
            </div>
          )}
        </>
      )}

      {tab === "invites" && manages(mine) && (
        <>
          {requests.length > 0 && (
            <List variant="card" density="compact" className="mlt-members">
              {requests.map((request) => (
                <ListRow
                  key={request.user.id}
                  leading={<Avatar user={request.user} />}
                  title={<>@{request.user.handle}</>}
                  subtitle={<>{tr("wants to join as {0}", [roleName(request.role)])}</>}
                  actions={
                  <>
                  <button type="button" className="button primary small" onClick={() => void run(() => api("POST", `/v2/teams/${id}/requests/${request.user.id}`, { accept: true }))}>
                    {tr("Approve")}
                  </button>
                  <button type="button" className="button secondary small" onClick={() => void run(() => api("POST", `/v2/teams/${id}/requests/${request.user.id}`, { accept: false }))}>
                    {tr("Decline")}
                  </button>
                </>
                }
                />
              ))}
            </List>
          )}
          <div className="mlt-invite-form">
            <label>
              <span>{tr("Role")}</span>
              <Dropdown
                value={draft.role}
                onChange={(next) => setDraft({ ...draft, role: next as Role })}
                options={(["admin", "developer", "viewer"] as Role[]).filter((role) => rank[role] <= rank[mine]).map((role) => ({ value: role, label: roleName(role) }))}
              />
            </label>
            <label>
              <span>{tr("Expires")}</span>
              <Dropdown
                value={draft.days}
                onChange={(next) => setDraft({ ...draft, days: next })}
                options={[
                  { value: "1", label: tr("1 day") },
                  { value: "7", label: tr("7 days") },
                  { value: "30", label: tr("30 days") },
                ]}
              />
            </label>
            <label>
              <span>{tr("Uses")}</span>
              <Dropdown
                value={draft.uses}
                onChange={(next) => setDraft({ ...draft, uses: next })}
                options={[
                  { value: "1", label: "1" },
                  { value: "5", label: "5" },
                  { value: "25", label: "25" },
                  { value: "0", label: tr("No limit") },
                ]}
              />
            </label>
            <label className="mlt-switch">
              <span>{tr("Approve before joining")}</span>
              <span className="switch">
                <input type="checkbox" aria-label={tr("Approve before joining")} checked={draft.approval} onChange={(event) => setDraft({ ...draft, approval: event.target.checked })} />
                <span />
              </span>
            </label>
            <button type="button" className="button primary small" onClick={() => void makeInvite()}>
              {tr("Make invite link")}
            </button>
          </div>
          {made?.url && (
            <div className="mlt-made">
              <code>{made.url}</code>
              <button type="button" className="button secondary small" onClick={() => void copy(made.url ?? "")}>
                <Icon name="copy" />
                {tr("Copy")}
              </button>
              <small className="muted">{tr("The link is shown only now. Make a new one if you lose it.")}</small>
            </div>
          )}
          {invites.length > 0 && (
            <List variant="card" density="compact" className="mlt-invites">
              {invites.map((invite) => (
                <ListRow
                  key={invite.id}
                  title={
                    <>
                      <code>{invite.prefix}…</code> <span className="chip">{roleName(invite.role)}</span>
                    </>
                  }
                  subtitle={
                    <>
                      {invite.max_uses ? tr("{0}/{1} used", [invite.uses, invite.max_uses]) : tr("{0} used", [invite.uses])} · {until(invite.expires)}
                      {invite.needs_approval ? ` · ${tr("approval")}` : ""}
                    </>
                  }
                  actions={
                    <button type="button" className="icon-button" aria-label={tr("Cancel invite link")} data-tip={tr("Cancel link")} onClick={() => void run(() => api("DELETE", `/v2/teams/${id}/invites/${invite.id}`))}>
                      <Icon name="close" />
                    </button>
                  }
                />
              ))}
            </List>
          )}
        </>
      )}

      {tab === "storage" && hosted.length > 0 && (
        <TeamStorage
          team={id}
          servers={mineList.filter((one) => hosted.includes(one.id)).map((one) => ({ id: one.id, name: one.name }))}
          canTrash={manages(mine)}
          onNotice={onNotice}
          onError={onError}
          confirm={confirm}
        />
      )}

      {tab === "activity" &&
        (activity.length === 0 ? (
          <p className="muted mlt-empty">{tr("Nothing yet.")}</p>
        ) : (
          <ul className="mlt-activity">
            {activity.map((item) => (
              <li key={item.id}>
                <span>{activityText(item)}</span>
                <time className="muted" dateTime={new Date(item.at * 1000).toISOString()}>
                  {ago(item.at)}
                </time>
              </li>
            ))}
          </ul>
        ))}

      {tab === "settings" && (
        <div className="mlt-settings">
          {manages(mine) && <Nodes team={id} onChanged={() => { load(); onChanged(); }} onError={onError} confirm={confirm} />}
          <section className="mlt-set">
            <div className="mlt-set-text">
              <strong>{tr("Team name")}</strong>
              <small className="muted">{manages(mine) ? tr("Everyone in the team sees the new name right away.") : tr("Only admins and the owner can rename the team.")}</small>
            </div>
            <span className="setting-input relay-join mlt-rename">
              <input
                value={rename ?? team.name}
                maxLength={40}
                disabled={!manages(mine)}
                aria-label={tr("Team name")}
                onChange={(event) => setRename(event.target.value)}
                onKeyDown={(event) => event.key === "Enter" && rename?.trim() && rename.trim() !== team.name && void run(async () => { await api("PATCH", `/v2/teams/${id}`, { name: rename.trim() }); setRename(null); })}
              />
              {manages(mine) && rename !== null && rename !== team.name && (
                <>
                  <button type="button" className="button secondary small" onClick={() => setRename(null)}>
                    {tr("Cancel")}
                  </button>
                  <button type="button" className="button primary small" disabled={!rename.trim()} onClick={() => void run(async () => { await api("PATCH", `/v2/teams/${id}`, { name: rename.trim() }); setRename(null); })}>
                    {tr("Save")}
                  </button>
                </>
              )}
            </span>
          </section>
          <section className="mlt-set">
            <div className="mlt-set-text">
              <strong>{tr("Your role")}</strong>
              <small className="muted">{mine === "owner" ? tr("To hand the team to someone else, make an admin the owner in Members.") : tr("An admin or the owner can change it in Members.")}</small>
            </div>
            <span className="chip">{roleName(mine)}</span>
          </section>
          {mine === "owner" && (
            <section className="mlt-set">
              <div className="mlt-set-text">
                <strong>{tr("Hand the team over")}</strong>
                <small className="muted">
                  {admins.length ? tr("The admin you pick becomes the owner and you become an admin. Only they can hand it back.") : tr("Make someone an admin in Members first. Only an admin can become the owner.")}
                </small>
              </div>
              {admins.length > 0 && (
                <span className="mlt-set-control">
                  <Dropdown
                    value={handTo && admins.some((one) => one.id === handTo) ? handTo : admins[0].id}
                    onChange={setHandTo}
                    options={admins.map((one) => ({ value: one.id, label: `@${one.handle}` }))}
                  />
                  <button
                    type="button"
                    className="button secondary small"
                    onClick={async () => {
                      const to = admins.find((one) => one.id === handTo) ?? admins[0];
                      if (await confirm(tr("Hand {0} to @{1}?", [team.name, to.handle]), tr("They become the owner and you become an admin. Only they can hand it back."), tr("Hand over"), true))
                        void run(() => api("POST", `/v2/teams/${id}/transfer`, { user: to.id }));
                    }}
                  >
                    {tr("Hand over")}
                  </button>
                </span>
              )}
            </section>
          )}
          <section className="mlt-set danger">
            <div className="mlt-set-text">
              <strong>{mine === "owner" ? tr("Delete team") : tr("Leave team")}</strong>
              <small className="muted">
                {mine === "owner"
                  ? tr("Everyone loses access and the invite links stop working. This can't be undone. To leave without deleting it, hand the team over first.")
                  : tr("You lose access to this team's servers. You need a new invite to come back.")}
              </small>
            </div>
            {mine === "owner" ? (
              <button
                type="button"
                className="button danger small"
                onClick={async () => {
                  if (await confirm(tr("Delete {0}?", [team.name]), tr("Everyone loses access and the invite links stop working. This can't be undone."), tr("Delete team"), true))
                    void run(() => api("DELETE", `/v2/teams/${id}`)).then(onGone);
                }}
              >
                {tr("Delete team")}
              </button>
            ) : (
              <button
                type="button"
                className="button secondary small"
                onClick={async () => {
                  if (await confirm(tr("Leave {0}?", [team.name]), tr("You lose access to this team's servers. You need a new invite to come back."), tr("Leave"), true))
                    void run(() => api("DELETE", `/v2/teams/${id}/members/${me.id}`)).then(onGone);
                }}
              >
                {tr("Leave team")}
              </button>
            )}
          </section>
        </div>
      )}
      </>
      )}
      </div>
      <aside className="mlt-view-aside mlt-roster" aria-label={tr("Members")}>
        {[
          { title: tr("Online"), people: team.members.filter((one) => one.id === me.id || (online[one.id] ?? one.online)), on: true },
          { title: tr("Offline"), people: team.members.filter((one) => one.id !== me.id && !(online[one.id] ?? one.online)), on: false },
        ].map((group) =>
          group.people.length === 0 ? null : (
            <section key={group.title} className="mlt-roster-group">
              <h3>
                {group.title} <span className="muted">{group.people.length}</span>
              </h3>
              <ul className="mlt-people">
                {group.people.map((member) => (
                  <li key={member.id} className={group.on ? "" : "away"}>
                    <Avatar user={member} online={group.on} />
                    <span>@{member.handle}</span>
                    <small className="muted">{roleName(member.role)}</small>
                  </li>
                ))}
              </ul>
            </section>
          ),
        )}
      </aside>
      </div>
    </div>
  );
}

export type TeamChoice = { id: string; name: string; role: Role };

/** The signed-in person and their teams, kept fresh from mnlab.cc's live messages. Empty when signed out. */
export function useMyTeams() {
  const [me, setMe] = useState<User | null>(null);
  const [teams, setTeams] = useState<TeamChoice[] | null>(null);
  const load = useCallback(() => {
    void commands.accountState().then(
      (state) => {
        if (state.status === "signed_out") {
          setMe(null);
          setTeams([]);
          return;
        }
        if (state.user) setMe(JSON.parse(state.user) as User);
        void api<{ teams: TeamSummary[] }>("GET", "/v2/me").then(
          (answer) => setTeams(answer.teams.map((team) => ({ id: team.id, name: team.name, role: team.role }))),
          () => setTeams((old) => old ?? []),
        );
      },
      () => setTeams((old) => old ?? []),
    );
  }, []);
  useEffect(() => {
    // The account and its teams are the host's own: web panel and Monolink guests have none (and no Tauri events).
    if (onWeb) {
      setTeams([]);
      return;
    }
    load();
    const changed = listen("account-changed", load);
    const live = listen<Record<string, unknown>>("account-live", (event) => {
      const kind = event.payload.type;
      if (kind === "team_changed" || kind === "notice") load();
    });
    return () => {
      void changed.then((stop) => stop());
      void live.then((stop) => stop());
    };
  }, [load]);
  return { me, teams, reload: load };
}

/**
 * One team as a place on the home screen (the team switcher): its servers, people, invites and
 * activity, in the space the server list uses.
 */
export function TeamWorkspace({
  team,
  me,
  onGone,
  onChanged,
  onError,
  onNotice,
  confirm,
}: {
  team: TeamChoice;
  me: User;
  onGone: () => void;
  onChanged: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: Confirm;
}) {
  const [online, setOnline] = useState<Record<string, boolean>>({});
  const [bump, setBump] = useState(0);
  useEffect(() => {
    const off = listen<Record<string, unknown>>("account-live", (event) => {
      const message = event.payload;
      if (message.type === "presence") setOnline((old) => ({ ...old, [String(message.user)]: Boolean(message.online) }));
      if (message.type === "team_changed" || message.type === "activity" || message.type === "notice") setBump((value) => value + 1);
    });
    return () => void off.then((stop) => stop());
  }, []);
  return (
    <div className="team-space">
      <TeamPanel key={team.id} id={team.id} me={me} online={online} bump={bump} onError={onError} onNotice={onNotice} confirm={confirm} onGone={onGone} onChanged={onChanged} />
    </div>
  );
}

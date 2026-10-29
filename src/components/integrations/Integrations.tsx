import { useCallback, useEffect, useState } from "react";
import { commands, type EssentialsData, type Extension, type Integrations as Found, type LuckPermsData } from "../../lib/bindings";
import { relativeTime } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { Card } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";

type Choice = { value: string; label: string };
type Props = {
  serverId: string;
  running: boolean;
  found: Found;
  players: string[];
  run: (command: string, done: string) => Promise<boolean>;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
  askText: (title: string, label: string, value: string, confirmLabel: string) => Promise<string | null>;
  askChoice: (title: string, body: string, choices: Choice[], value: string, confirmLabel: string) => Promise<string | null>;
  onError: (message: string) => void;
  notify: (message: string) => void;
  openFile: (path: string) => void;
};

const NAME = /^[A-Za-z0-9_]{1,16}$/;
const GROUP = /^[A-Za-z0-9_-]{1,36}$/;
const NODE = /^[A-Za-z0-9_.*-]{1,128}$/;
const WORD = /^[A-Za-z0-9_-]{1,64}$/;

const coords = (spot: { x: number | null; y: number | null; z: number | null }) => [spot.x, spot.y, spot.z].map((one) => Math.round(one ?? 0)).join(", ");

/** Server tab for plugins the app understands: EssentialsX (files) and LuckPerms (export). */
export function Integrations(props: Props) {
  const { serverId, found, onError } = props;
  const [extensions, setExtensions] = useState<Extension[]>([]);
  const loadExtensions = useCallback(() => {
    if (!found.extensions.length) return;
    commands.extensionData(serverId).then(setExtensions).catch((cause) => onError(String(cause)));
  }, [serverId, found.extensions.length, onError]);
  useEffect(loadExtensions, [loadExtensions]);
  const live = found.extensions.some((one) => one.startsWith("api:"));
  useEffect(() => {
    if (!live) return;
    const timer = window.setInterval(loadExtensions, 5000);
    return () => window.clearInterval(timer);
  }, [live, loadExtensions]);
  const linked = [found.essentials && "essentials", found.luckperms && "luckperms", ...found.extensions].filter(Boolean) as string[];
  const all = linked;
  const [tab, setTab] = useState("");
  const shown = all.includes(tab) ? tab : all[0];
  const label = (one: string) =>
    one === "essentials" ? "EssentialsX" : one === "luckperms" ? "LuckPerms" : (extensions.find((ext) => ext.folder === one)?.name ?? one.split("/").pop() ?? one);
  const extension = extensions.find((one) => one.folder === shown);
  const item = (key: string, text: string, icon: string) => (
    <button key={key} type="button" className={shown === key ? "on" : ""} aria-current={shown === key ? "page" : undefined} onClick={() => setTab(key)}>
      <Icon name={icon} />
      <span>{text}</span>
    </button>
  );
  return (
    <div className="tool-content integrations-page">
      <div className="wb-head">
        <h1>{tr("Integrations")}</h1>
      </div>
      <div className="integ-layout">
        <nav className="integ-nav" aria-label={tr("Integrations")}>
          <span className="integ-nav-title">{tr("Connected plugins")}</span>
          {linked.length ? linked.map((one) => item(one, label(one), one.startsWith("api:") ? "plug" : "puzzle")) : <p className="integ-nav-empty">{tr("None yet")}</p>}
        </nav>
        <section className="integ-main">
          {!shown && (
            <Card title={tr("No integrations yet")}>
              <p className="muted lag-note">{tr("Install EssentialsX or LuckPerms and their settings, warps, kits, groups and permissions show up here.")}</p>
              <p className="muted lag-note">{tr("Your own plugins and mods can add controls too: put a monolab.json in their folder. The Agent tab can write it for you.")}</p>
            </Card>
          )}
          {shown === "essentials" && <Essentials {...props} />}
          {shown === "luckperms" && <LuckPerms {...props} />}
          {extension && <ExtensionView {...props} extension={extension} reload={loadExtensions} />}
        </section>
      </div>
    </div>
  );
}

function ExtensionView({ serverId, running, players, run, confirm, askText, askChoice, onError, notify, extension, reload }: Props & { extension: Extension; reload: () => void }) {
  const [drafts, setDrafts] = useState<Record<number, string>>({});
  async function save(index: number, value: string) {
    try {
      await commands.extensionSet(serverId, extension.folder, index, value);
      reload();
    } catch (cause) {
      onError(String(cause));
    }
  }
  async function perform(action: Extension["actions"][number]) {
    let command = action.command;
    const values: Record<string, string> = {};
    for (const arg of action.args) {
      let value: string | null;
      if (arg.kind === "player" && players.length) {
        value = await askChoice(action.label, arg.label, players.map((one) => ({ value: one, label: one })), players[0], tr("Next"));
      } else {
        value = await askText(action.label, arg.label, "", tr("Next"));
      }
      if (value === null) return;
      value = value.trim();
      if (!value || /[\r\n]/.test(value)) return onError(tr("Enter a value on one line."));
      if (arg.kind === "number" && !/^-?\d+(\.\d+)?$/.test(value)) return onError(tr("Enter a number."));
      if (arg.kind === "player" && !NAME.test(value)) return onError(tr("Enter a player name."));
      command = command.split(`{${arg.name}}`).join(value);
      values[arg.name] = value;
    }
    if (action.call) {
      try {
        const message = await commands.runExtensionAction(serverId, extension.folder, action.label, JSON.stringify(values));
        notify(message || tr("Done: {0}.", [action.label]));
      } catch (cause) {
        onError(String(cause));
      }
      return;
    }
    if (action.confirm && !(await confirm(action.label, `${action.confirm}\n\n${command}`, tr("Run")))) return;
    if (await run(command, tr("Sent {0}.", [command]))) window.setTimeout(reload, 1500);
  }
  if (extension.problem) {
    return (
      <Card title={extension.name}>
        <p className="muted lag-note">{tr("monolab.json couldn't be read: {0}", [extension.problem])}</p>
      </Card>
    );
  }
  return (
    <div className="integ-grid">
      {extension.status.length > 0 && (
        <Card className="integ-wide" title={tr("Status")}>
          <div className="monitor-grid adv-grid">
            {extension.status.map((one, at) => (
              <div key={at}>
                <span>{one.label}</span>
                <strong>{one.value == null ? "—" : `${one.value}${one.unit ? ` ${one.unit}` : ""}`}</strong>
              </div>
            ))}
          </div>
        </Card>
      )}
      {extension.settings.length > 0 && (
        <Card title={tr("Settings")} aside={<span className="muted">{extension.folder}</span>}>
          <div className="integ-settings">
            {extension.settings.map((one, index) => {
              const draft = drafts[index] ?? one.value ?? "";
              return (
                <div className="setting-row" key={index}>
                  <div>
                    <strong>{one.label}</strong>
                    <small>{one.hint || `${one.file} · ${one.key}`}</small>
                  </div>
                  {one.value == null ? (
                    <span className="muted">{tr("Not found")}</span>
                  ) : one.kind === "bool" ? (
                    <label className="switch">
                      <input type="checkbox" checked={one.value === "true"} onChange={(event) => void save(index, String(event.target.checked))} />
                      <span />
                    </label>
                  ) : one.kind === "choice" ? (
                    <Dropdown value={one.value} align="end" options={one.options.map((option) => ({ value: option, label: option }))} onChange={(value) => void save(index, value)} />
                  ) : (
                    <input
                      className="integ-value"
                      value={draft}
                      inputMode={one.kind === "number" ? "decimal" : undefined}
                      onChange={(event) => setDrafts((now) => ({ ...now, [index]: event.target.value }))}
                      onKeyDown={(event) => event.key === "Enter" && (event.target as HTMLInputElement).blur()}
                      onBlur={() => {
                        setDrafts((now) => {
                          const rest = { ...now };
                          delete rest[index];
                          return rest;
                        });
                        if (draft.trim() && draft !== one.value) void save(index, draft);
                      }}
                    />
                  )}
                </div>
              );
            })}
            <p className="muted lag-note">{tr("Most plugins read changes after a reload or restart.")}</p>
          </div>
        </Card>
      )}
      {extension.actions.length > 0 && (
        <Card title={tr("Actions")}>
          <ul className="integ-rows">
            {extension.actions.map((action, at) => (
              <li key={at}>
                <div>
                  <strong>{action.label}</strong>
                  {action.call ? (
                    <small>{action.args.length ? action.args.map((arg) => arg.label).join(" · ") : extension.name}</small>
                  ) : (
                    <small><code>{action.command}</code></small>
                  )}
                </div>
                <button className="button secondary small" disabled={!running} data-tip={running ? undefined : tr("Start the server first.")} onClick={() => void perform(action)}>
                  {tr("Run")}
                </button>
              </li>
            ))}
          </ul>
        </Card>
      )}
    </div>
  );
}

const SETTING_LABELS: Record<string, () => string> = {
  "teleport-cooldown": () => tr("Teleport cooldown"),
  "teleport-delay": () => tr("Teleport delay"),
  "heal-cooldown": () => tr("Heal cooldown"),
  "starting-balance": () => tr("Starting balance"),
  "currency-symbol": () => tr("Currency symbol"),
  "min-money": () => tr("Minimum balance"),
  "max-money": () => tr("Maximum balance"),
  "auto-afk": () => tr("Auto AFK"),
  "auto-afk-kick": () => tr("AFK kick"),
  "change-displayname": () => tr("Change display name"),
};

function Essentials({ serverId, running, players, run, confirm, askText, askChoice, onError }: Props) {
  const [data, setData] = useState<EssentialsData | null>(null);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [changed, setChanged] = useState(false);
  const [open, setOpen] = useState<string | null>(null);
  const load = useCallback(() => {
    commands.essentialsData(serverId).then(setData).catch((cause) => onError(String(cause)));
  }, [serverId, onError]);
  useEffect(load, [load]);

  async function save(key: string, value: string) {
    try {
      await commands.essentialsSetSetting(serverId, key, value);
      setChanged(true);
      load();
    } catch (cause) {
      onError(String(cause));
    }
  }
  async function pickPlayer(title: string) {
    if (!players.length) {
      onError(tr("No one is online."));
      return null;
    }
    return askChoice(title, "", players.map((one) => ({ value: one, label: one })), players[0], tr("Send"));
  }

  if (!data) return <p className="muted">{tr("Loading…")}</p>;
  const needsServer = running ? undefined : tr("Start the server first.");
  return (
    <div className="integ-grid">
      <Card
        title={tr("Settings")}
        aside={
          changed && running ? (
            <button className="button secondary small" onClick={() => void run("essentials reload", tr("EssentialsX reloaded.")).then((ok) => ok && setChanged(false))}>
              {tr("Apply now")}
            </button>
          ) : (
            <span className="muted">config.yml</span>
          )
        }
      >
        {data.settings.length === 0 ? (
          <p className="muted lag-note">{tr("config.yml was not found.")}</p>
        ) : (
          <div className="integ-settings">
            {data.settings.map((one) => {
              const bool = one.value === "true" || one.value === "false";
              const draft = drafts[one.key] ?? one.value;
              return (
                <div className="setting-row" key={one.key}>
                  <div>
                    <strong>{SETTING_LABELS[one.key]?.() ?? one.key}</strong>
                    <small>{one.key}</small>
                  </div>
                  {bool ? (
                    <label className="switch">
                      <input type="checkbox" checked={one.value === "true"} onChange={(event) => void save(one.key, String(event.target.checked))} />
                      <span />
                    </label>
                  ) : (
                    <input
                      className="integ-value"
                      value={draft}
                      onChange={(event) => setDrafts((now) => ({ ...now, [one.key]: event.target.value }))}
                      onKeyDown={(event) => event.key === "Enter" && (event.target as HTMLInputElement).blur()}
                      onBlur={() => {
                        setDrafts((now) => {
                          const rest = { ...now };
                          delete rest[one.key];
                          return rest;
                        });
                        if (draft.trim() && draft !== one.value) void save(one.key, draft);
                      }}
                    />
                  )}
                </div>
              );
            })}
            {changed && !running && <p className="muted lag-note">{tr("Applies when the server starts.")}</p>}
          </div>
        )}
      </Card>
      <Card title={tr("Warps")} aside={<span className="muted">{data.warps.length}</span>}>
        {data.warps.length === 0 ? (
          <p className="muted lag-note">{tr("No warps yet. Players make them with /setwarp.")}</p>
        ) : (
          <ul className="integ-rows">
            {data.warps.map((warp) => (
              <li key={warp.name}>
                <div>
                  <strong>{warp.name}</strong>
                  <small>{warp.world} · {coords(warp)}</small>
                </div>
                <button
                  className="button ghost small"
                  disabled={!running}
                  data-tip={needsServer}
                  onClick={async () => {
                    if (!WORD.test(warp.name)) return;
                    const who = await pickPlayer(tr("Send a player to {0}", [warp.name]));
                    if (who && NAME.test(who)) void run(`warp ${warp.name} ${who}`, tr("Sent {0} to {1}.", [who, warp.name]));
                  }}
                >
                  {tr("Send a player")}
                </button>
                <button
                  className="icon-button"
                  aria-label={tr("Delete")}
                  data-tip={needsServer ?? tr("Delete")}
                  disabled={!running}
                  onClick={async () => {
                    if (!WORD.test(warp.name)) return;
                    if (!(await confirm(tr("Delete {0}?", [warp.name]), tr("Players can no longer use this warp."), tr("Delete"), true))) return;
                    if (await run(`delwarp ${warp.name}`, tr("Deleted {0}.", [warp.name]))) window.setTimeout(load, 800);
                  }}
                >
                  <Icon name="trash" />
                </button>
              </li>
            ))}
          </ul>
        )}
      </Card>
      <Card title={tr("Kits")} aside={<span className="muted">{data.kits.length}</span>}>
        {data.kits.length === 0 ? (
          <p className="muted lag-note">{tr("No kits.")}</p>
        ) : (
          <ul className="integ-rows">
            {data.kits.map((kit) => (
              <li key={kit.name}>
                <div>
                  <strong>{kit.name}</strong>
                  <small>{tr("{0} items · cooldown {1} s", [kit.items, kit.delay || "0"])}</small>
                </div>
                <button
                  className="button ghost small"
                  disabled={!running}
                  data-tip={needsServer}
                  onClick={async () => {
                    if (!WORD.test(kit.name)) return;
                    const who = await pickPlayer(tr("Give {0}", [kit.name]));
                    if (who && NAME.test(who)) void run(`kit ${kit.name} ${who}`, tr("Gave {0} to {1}.", [kit.name, who]));
                  }}
                >
                  {tr("Give")}
                </button>
              </li>
            ))}
          </ul>
        )}
      </Card>
      <Card
        title={tr("Players")}
        aside={<span className="muted">{data.users.length < data.usersTotal ? tr("{0} of {1}, recent first", [data.users.length, data.usersTotal]) : data.usersTotal}</span>}
      >
        {data.users.length === 0 ? (
          <p className="muted lag-note">{tr("No player data yet.")}</p>
        ) : (
          <ul className="integ-rows">
            {data.users.map((user) => (
              <li key={user.uuid} className={open === user.uuid ? "open" : ""}>
                <div>
                  <strong>{user.name}</strong>
                  <small>
                    {[
                      user.money && tr("Balance {0}", [user.money]),
                      tr("{0, plural, one {# home} other {# homes}}", [user.homes.length]),
                      user.lastSeen ? relativeTime(Math.floor(user.lastSeen / 1000)) : "",
                    ].filter(Boolean).join(" · ")}
                  </small>
                  {open === user.uuid && user.homes.length > 0 && (
                    <ul className="integ-homes">
                      {user.homes.map((home) => (
                        <li key={home.name}><code>{home.name}</code> {home.world} · {coords(home)}</li>
                      ))}
                    </ul>
                  )}
                </div>
                {user.homes.length > 0 && (
                  <button className="button ghost small" onClick={() => setOpen(open === user.uuid ? null : user.uuid)}>
                    {open === user.uuid ? tr("Hide homes") : tr("Homes")}
                  </button>
                )}
                <button
                  className="button ghost small"
                  disabled={!running || !NAME.test(user.name)}
                  data-tip={needsServer}
                  onClick={async () => {
                    const amount = await askText(tr("Balance of {0}", [user.name]), tr("New balance"), user.money || "0", tr("Change"));
                    if (amount === null) return;
                    if (!/^\d+(\.\d+)?$/.test(amount.trim())) {
                      onError(tr("Enter a number."));
                      return;
                    }
                    if (await run(`eco set ${user.name} ${amount.trim()}`, tr("Changed the balance of {0}.", [user.name]))) window.setTimeout(load, 1500);
                  }}
                >
                  {tr("Balance")}
                </button>
              </li>
            ))}
          </ul>
        )}
      </Card>
    </div>
  );
}

function LuckPerms({ serverId, running, players, run, confirm, askText, askChoice, onError }: Props) {
  const [data, setData] = useState<LuckPermsData | null>(null);
  const [busy, setBusy] = useState(false);
  const [picked, setPicked] = useState<string | null>(null);
  const [node, setNode] = useState("");
  const [query, setQuery] = useState("");

  const load = useCallback(async () => {
    setBusy(true);
    try {
      setData(await commands.luckpermsData(serverId));
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy(false);
    }
  }, [serverId, onError]);

  async function act(command: string, done: string) {
    if (!data) return;
    if (await run(`${data.command} ${command}`, done)) await load();
  }

  if (!running) {
    return (
      <Card title="LuckPerms">
        <p className="muted lag-note">{tr("Start the server to read groups and permissions. LuckPerms keeps them in its own database.")}</p>
      </Card>
    );
  }
  if (!data) {
    return (
      <Card title="LuckPerms">
        <p className="muted lag-note">{tr("Reading runs {0} in the console and removes the file afterwards.", ["lp export"])}</p>
        <button className="button primary small" disabled={busy} onClick={() => void load()}>
          {busy ? tr("Loading…") : tr("Load groups and permissions")}
        </button>
      </Card>
    );
  }
  const group = data.groups.find((one) => one.name === picked) ?? data.groups[0];
  const groupNames = data.groups.map((one) => ({ value: one.name, label: one.name }));
  const needle = query.trim().toLowerCase();
  const users = data.users.filter((one) => !needle || one.name.toLowerCase().includes(needle));
  return (
    <div className="integ-grid">
      <Card
        className="integ-wide"
        title={tr("Groups")}
        aside={
          <div className="integ-actions">
            <button className="button ghost small" disabled={busy} onClick={() => void load()}>
              <Icon name="refresh" />
              {tr("Refresh")}
            </button>
            <button
              className="button secondary small"
              onClick={async () => {
                const name = await askText(tr("New group"), tr("Group name"), "", tr("Create"));
                if (name === null) return;
                if (!GROUP.test(name.trim())) return onError(tr("Use letters, numbers, - and _ only."));
                await act(`creategroup ${name.trim().toLowerCase()}`, tr("Created {0}.", [name.trim()]));
                setPicked(name.trim().toLowerCase());
              }}
            >
              <Icon name="plus" />
              {tr("New group")}
            </button>
          </div>
        }
      >
        <div className="lp-layout">
          <div className="lp-groups" role="listbox" aria-label={tr("Groups")}>
            {data.groups.map((one) => (
              <button key={one.name} role="option" aria-selected={group?.name === one.name} className={group?.name === one.name ? "on" : ""} onClick={() => setPicked(one.name)}>
                <span>{one.name}</span>
                <small>{one.weight != null ? tr("weight {0}", [one.weight]) : ""}</small>
              </button>
            ))}
          </div>
          {group && (
            <div className="lp-group">
              <div className="lp-group-head">
                <strong>{group.name}</strong>
                {group.prefix && <code>{group.prefix}</code>}
                <span className="live-spacer" />
                <button
                  className="button ghost small"
                  onClick={async () => {
                    const weight = await askText(tr("Weight of {0}", [group.name]), tr("Higher wins when groups conflict."), String(group.weight ?? 0), tr("Change"));
                    if (weight === null) return;
                    if (!/^\d{1,6}$/.test(weight.trim())) return onError(tr("Enter a number."));
                    await act(`group ${group.name} setweight ${weight.trim()}`, tr("Changed the weight of {0}.", [group.name]));
                  }}
                >
                  {tr("Weight")}
                </button>
                {group.name !== "default" && (
                  <button
                    className="icon-button"
                    aria-label={tr("Delete group")}
                    data-tip={tr("Delete group")}
                    onClick={async () => {
                      if (!(await confirm(tr("Delete {0}?", [group.name]), tr("Players in this group lose its permissions."), tr("Delete"), true))) return;
                      await act(`deletegroup ${group.name}`, tr("Deleted {0}.", [group.name]));
                      setPicked(null);
                    }}
                  >
                    <Icon name="trash" />
                  </button>
                )}
              </div>
              <div className="lp-parents">
                <span className="muted">{tr("Inherits")}</span>
                {group.parents.map((parent) => (
                  <span className="chip" key={parent}>
                    {parent}
                    <button aria-label={tr("Remove")} onClick={() => void act(`group ${group.name} parent remove ${parent}`, tr("Removed {0}.", [parent]))}>
                      <Icon name="close" />
                    </button>
                  </span>
                ))}
                <button
                  className="button ghost small"
                  onClick={async () => {
                    const choices = groupNames.filter((one) => one.value !== group.name && !group.parents.includes(one.value));
                    if (!choices.length) return;
                    const parent = await askChoice(tr("Inherit from"), tr("{0} gets every permission of the chosen group.", [group.name]), choices, choices[0].value, tr("Add"));
                    if (parent && GROUP.test(parent)) await act(`group ${group.name} parent add ${parent}`, tr("Added {0}.", [parent]));
                  }}
                >
                  <Icon name="plus" />
                  {tr("Add")}
                </button>
              </div>
              <form
                className="lp-add"
                onSubmit={async (event) => {
                  event.preventDefault();
                  const value = node.trim();
                  if (!NODE.test(value)) return onError(tr("Enter a permission like {0}.", ["essentials.home"]));
                  await act(`group ${group.name} permission set ${value} true`, tr("Added {0}.", [value]));
                  setNode("");
                }}
              >
                <input placeholder="essentials.home" value={node} onChange={(event) => setNode(event.target.value)} aria-label={tr("Permission")} />
                <button className="button secondary small" type="submit" disabled={!node.trim()}>
                  {tr("Add permission")}
                </button>
              </form>
              {group.permissions.length === 0 ? (
                <p className="muted lag-note">{tr("No permissions of its own.")}</p>
              ) : (
                <ul className="integ-rows lp-perms">
                  {group.permissions.map((perm) => (
                    <li key={`${perm.node}|${perm.context}`}>
                      <div>
                        <code>{perm.node}</code>
                        {(perm.context || perm.temporary) && <small>{[perm.context, perm.temporary ? tr("temporary") : ""].filter(Boolean).join(" · ")}</small>}
                      </div>
                      <button
                        className={`chip lp-value ${perm.value ? "" : "off"}`}
                        disabled={!!perm.context || perm.temporary}
                        data-tip={tr("Switch between true and false")}
                        onClick={() => void act(`group ${group.name} permission set ${perm.node} ${!perm.value}`, tr("Changed {0}.", [perm.node]))}
                      >
                        {String(perm.value)}
                      </button>
                      <button
                        className="icon-button"
                        aria-label={tr("Remove")}
                        data-tip={tr("Remove")}
                        disabled={!!perm.context || perm.temporary}
                        onClick={() => void act(`group ${group.name} permission unset ${perm.node}`, tr("Removed {0}.", [perm.node]))}
                      >
                        <Icon name="close" />
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </div>
      </Card>
      <Card
        className="integ-wide"
        title={tr("Players")}
        aside={<input className="integ-search" placeholder={tr("Search")} value={query} onChange={(event) => setQuery(event.target.value)} aria-label={tr("Search")} />}
      >
        {users.length === 0 ? (
          <p className="muted lag-note">{tr("No players.")}</p>
        ) : (
          <ul className="integ-rows">
            {users.slice(0, 300).map((user) => (
              <li key={user.uuid}>
                <div>
                  <strong>{user.name}{players.includes(user.name) && <span className="integ-online" aria-label={tr("Online")} />}</strong>
                  <small>{tr("Primary group {0}", [user.primary])}{user.permissions ? ` · ${tr("{0, plural, one {# own permission} other {# own permissions}}", [user.permissions])}` : ""}</small>
                </div>
                <div className="lp-parents">
                  {user.groups.map((one) => (
                    <span className="chip" key={one}>
                      {one}
                      {user.groups.length > 1 && NAME.test(user.name) && (
                        <button aria-label={tr("Remove")} onClick={() => void act(`user ${user.name} parent remove ${one}`, tr("Removed {0} from {1}.", [user.name, one]))}>
                          <Icon name="close" />
                        </button>
                      )}
                    </span>
                  ))}
                </div>
                <button
                  className="button ghost small"
                  disabled={!NAME.test(user.name)}
                  onClick={async () => {
                    const choices = groupNames.filter((one) => !user.groups.includes(one.value));
                    if (!choices.length) return;
                    const picked = await askChoice(tr("Add {0} to a group", [user.name]), "", choices, choices[0].value, tr("Add"));
                    if (picked && GROUP.test(picked)) await act(`user ${user.name} parent add ${picked}`, tr("Added {0} to {1}.", [user.name, picked]));
                  }}
                >
                  {tr("Add to group")}
                </button>
              </li>
            ))}
          </ul>
        )}
      </Card>
      {data.tracks.length > 0 && (
        <Card title={tr("Tracks")}>
          <ul className="integ-rows">
            {data.tracks.map((track) => (
              <li key={track.name}>
                <div>
                  <strong>{track.name}</strong>
                  <small>{track.groups.join(" → ")}</small>
                </div>
              </li>
            ))}
          </ul>
        </Card>
      )}
    </div>
  );
}

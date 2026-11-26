import { useCallback, useState } from "react";
import type { LpPermission, LuckPermsData } from "../../lib/bindings";
import { tr, trBack } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { EmptyState, List, ListRow, SegmentedControl, Switch } from "../primitives";
import { Icon } from "../ui/Icon";
import { lp, LP_GROUP, LP_NAME, LP_NODE, type LpTarget } from "./luckperms";
import "./PlayerPermissions.css";
import { Splitter, usePaneWidth, widthVar } from "../ui/Splitter";

type Choice = { value: string; label: string };

/**
 * The Players page's Permissions tab: LuckPerms groups and players. Reading runs `lp export` in the console (when
 * the user presses Load) and every change is one `lp` command, so LuckPerms stays the one that keeps them.
 */
export function PlayerPermissions({
  serverId,
  running,
  online,
  run,
  confirm,
  askText,
  askChoice,
  onError,
  onNotice,
}: {
  serverId: string;
  running: boolean;
  online: string[];
  run: (line: string) => Promise<void>;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
  askText: (title: string, label: string, value: string, confirmLabel: string) => Promise<string | null>;
  askChoice: (title: string, body: string, choices: Choice[], value: string, confirmLabel: string) => Promise<string | null>;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}) {
  const groupPane = usePaneWidth("lp-groups", 200, 160, 420);
  const [data, setData] = useState<LuckPermsData | null>(null);
  const [busy, setBusy] = useState(false);
  const [view, setView] = useState<"groups" | "players">("groups");
  const [picked, setPicked] = useState<string | null>(null);
  const [openUser, setOpenUser] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  const load = useCallback(async () => {
    setBusy(true);
    try {
      setData(await commands.luckpermsData(serverId));
    } catch (cause) {
      onError(trBack(String(cause)));
    } finally {
      setBusy(false);
    }
  }, [serverId, onError]);

  /** One change, then read again so the screen shows what LuckPerms has now. */
  async function act(make: (root: string) => string, done: string) {
    if (!data) return;
    let line: string;
    try {
      line = make(data.command);
    } catch {
      onError(tr("Use letters, numbers, dots, - and _ only."));
      return;
    }
    setBusy(true);
    try {
      await run(line);
      onNotice(done);
    } catch (cause) {
      onError(trBack(String(cause)));
    } finally {
      setBusy(false);
    }
    await load();
  }

  if (!running) {
    return <EmptyState icon="shield" title={tr("Start the server to see permissions")} hint={tr("LuckPerms keeps groups and permissions in its own storage, so they are read from the running server.")} />;
  }
  if (!data) {
    return (
      <EmptyState
        icon="shield"
        title={tr("Groups and permissions")}
        hint={tr("Reading runs {0} in the console and removes the file afterwards.", ["lp export"])}
        action={
          <button type="button" className="button primary small" disabled={busy} onClick={() => void load()}>
            {busy ? tr("Loading…") : tr("Load from LuckPerms")}
          </button>
        }
      />
    );
  }

  const groupChoices = data.groups.map((one) => ({ value: one.name, label: one.name }));
  const group = data.groups.find((one) => one.name === picked) ?? data.groups[0];
  const needle = query.trim().toLowerCase();
  const users = data.users.filter((one) => !needle || one.name.toLowerCase().includes(needle));

  const addPermission = async (who: LpTarget) => {
    const value = await askText(tr("Add a permission to {0}", [who.name]), tr("Permission, like essentials.home or worldedit.*"), "", tr("Add"));
    if (value === null) return;
    if (!LP_NODE.test(value.trim())) return onError(tr("Enter a permission like {0}.", ["essentials.home"]));
    await act((root) => lp.setPermission(root, who, value.trim(), true), tr("Added {0}.", [value.trim()]));
  };

  const permissionRows = (who: LpTarget, list: LpPermission[]) =>
    list.length === 0 ? (
      <p className="muted lag-note">{tr("No permissions of its own.")}</p>
    ) : (
      <List variant="box" density="compact" className="integ-rows lp-perms">
        {list.map((perm) => {
          // Ones with a context (world=..., server=...) or an expiry are shown but changed in LuckPerms itself.
          const plain = !perm.context && !perm.temporary;
          return (
            <ListRow
              key={`${perm.node}|${perm.context}`}
              title={<code>{perm.node}</code>}
              subtitle={!plain ? [perm.context, perm.temporary ? tr("temporary") : ""].filter(Boolean).join(" · ") : undefined}
              actions={
              <>
              <Switch
                checked={perm.value}
                disabled={busy || !plain}
                aria-label={tr("Allowed")}
                onChange={(event) => void act((root) => lp.setPermission(root, who, perm.node, event.target.checked), tr("Changed {0}.", [perm.node]))}
              />
              <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} disabled={busy || !plain} onClick={() => void act((root) => lp.unsetPermission(root, who, perm.node), tr("Removed {0}.", [perm.node]))}>
                <Icon name="close" />
              </button>
              </>
              }
            />
          );
        })}
      </List>
    );

  return (
    <div className="lp-page">
      <div className="lp-bar">
        <SegmentedControl
          value={view}
          onChange={setView}
          options={[
            { value: "groups", label: tr("Groups") },
            { value: "players", label: tr("Players") },
          ]}
        />
        {view === "players" && <input className="integ-search" placeholder={tr("Search")} value={query} onChange={(event) => setQuery(event.target.value)} aria-label={tr("Search")} />}
        <span className="live-spacer" />
        {view === "groups" && (
          <button
            type="button"
            className="button secondary small"
            disabled={busy}
            onClick={async () => {
              const name = await askText(tr("New group"), tr("Group name"), "", tr("Create"));
              if (name === null) return;
              if (!LP_GROUP.test(name.trim())) return onError(tr("Use letters, numbers, - and _ only."));
              await act((root) => lp.createGroup(root, name.trim()), tr("Created {0}.", [name.trim()]));
              setPicked(name.trim().toLowerCase());
            }}
          >
            <Icon name="plus" />
            {tr("New group")}
          </button>
        )}
        <button type="button" className="wb-icon" aria-label={tr("Refresh")} data-tip={tr("Refresh")} disabled={busy} onClick={() => void load()}>
          <Icon name="refresh" />
        </button>
      </div>
      {view === "groups" ? (
        <div className="lp-layout with-split" style={widthVar("--side-w", groupPane.width)}>
          <List variant="plain" density="compact" className="side-rows lp-groups" role="listbox" aria-label={tr("Groups")}>
            {data.groups.map((one) => (
              <ListRow
                key={one.name}
                role="presentation"
                selected={group?.name === one.name}
                mainProps={{ role: "option", "aria-selected": group?.name === one.name }}
                onClick={() => setPicked(one.name)}
                title={one.name}
                meta={one.weight != null ? tr("weight {0}", [one.weight]) : undefined}
              />
            ))}
          </List>
          <Splitter pane={groupPane} side="left" label={tr("Resize the group list")} />
          {group && (
            <div className="lp-group">
              <div className="lp-group-head">
                <strong>{group.name}</strong>
                {group.prefix && <code>{group.prefix}</code>}
                <span className="live-spacer" />
                <button
                  type="button"
                  className="button ghost small"
                  disabled={busy}
                  onClick={async () => {
                    const prefix = await askText(tr("Chat prefix of {0}", [group.name]), tr("Shown before the name in chat. Empty takes it off."), group.prefix, tr("Change"));
                    if (prefix === null) return;
                    await act((root) => lp.setPrefix(root, group.name, prefix, group.weight ?? 0), tr("Changed the prefix of {0}.", [group.name]));
                  }}
                >
                  {tr("Prefix")}
                </button>
                <button
                  type="button"
                  className="button ghost small"
                  disabled={busy}
                  onClick={async () => {
                    const weight = await askText(tr("Weight of {0}", [group.name]), tr("Higher wins when groups conflict."), String(group.weight ?? 0), tr("Change"));
                    if (weight === null) return;
                    if (!/^\d{1,6}$/.test(weight.trim())) return onError(tr("Enter a number."));
                    await act((root) => lp.setWeight(root, group.name, Number(weight.trim())), tr("Changed the weight of {0}.", [group.name]));
                  }}
                >
                  {tr("Weight")}
                </button>
                {group.name !== "default" && (
                  <button
                    type="button"
                    className="icon-button"
                    aria-label={tr("Delete group")}
                    data-tip={tr("Delete group")}
                    disabled={busy}
                    onClick={async () => {
                      if (!(await confirm(tr("Delete {0}?", [group.name]), tr("Players in this group lose its permissions."), tr("Delete"), true))) return;
                      await act((root) => lp.deleteGroup(root, group.name), tr("Deleted {0}.", [group.name]));
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
                    <button type="button" aria-label={tr("Remove")} disabled={busy} onClick={() => void act((root) => lp.removeParent(root, { kind: "group", name: group.name }, parent), tr("Removed {0}.", [parent]))}>
                      <Icon name="close" />
                    </button>
                  </span>
                ))}
                <button
                  type="button"
                  className="button ghost small"
                  disabled={busy}
                  onClick={async () => {
                    const choices = groupChoices.filter((one) => one.value !== group.name && !group.parents.includes(one.value));
                    if (!choices.length) return;
                    const parent = await askChoice(tr("Inherit from"), tr("{0} gets every permission of the chosen group.", [group.name]), choices, choices[0].value, tr("Add"));
                    if (parent) await act((root) => lp.addParent(root, { kind: "group", name: group.name }, parent), tr("Added {0}.", [parent]));
                  }}
                >
                  <Icon name="plus" />
                  {tr("Add")}
                </button>
              </div>
              <div className="lp-perm-head">
                <span>{tr("Permissions")}</span>
                <button type="button" className="button ghost small" disabled={busy} onClick={() => void addPermission({ kind: "group", name: group.name })}>
                  <Icon name="plus" />
                  {tr("Add permission")}
                </button>
              </div>
              {permissionRows({ kind: "group", name: group.name }, group.permissions)}
              {data.tracks.some((track) => track.groups.includes(group.name)) && (
                <p className="muted lag-note">
                  {tr("Tracks")}: {data.tracks.filter((track) => track.groups.includes(group.name)).map((track) => `${track.name} (${track.groups.join(" → ")})`).join(", ")}
                </p>
              )}
            </div>
          )}
        </div>
      ) : users.length === 0 ? (
        <p className="muted lag-note">{tr("No players match.")}</p>
      ) : (
        <List variant="box" className="integ-rows">
          {users.slice(0, 300).map((user) => {
            const editable = LP_NAME.test(user.name);
            const opened = openUser === user.uuid;
            return (
              <ListRow
                key={user.uuid}
                className={`lp-user ${opened ? "open" : ""}`}
                title={
                  <>
                    {user.name}
                    {online.includes(user.name) && <span className="integ-online" aria-label={tr("Online")} />}
                  </>
                }
                subtitle={
                  <>
                    {tr("Primary group {0}", [user.primary])}
                    {user.permissions ? ` · ${tr("{0, plural, one {# own permission} other {# own permissions}}", [user.permissions])}` : ""}
                  </>
                }
                actions={
                <>
                  <div className="lp-parents">
                    {user.groups.map((one) => (
                      <span className="chip" key={one}>
                        {one}
                        {user.groups.length > 1 && editable && (
                          <button type="button" aria-label={tr("Remove")} disabled={busy} onClick={() => void act((root) => lp.removeParent(root, { kind: "user", name: user.name }, one), tr("Removed {0} from {1}.", [user.name, one]))}>
                            <Icon name="close" />
                          </button>
                        )}
                      </span>
                    ))}
                  </div>
                  <button
                    type="button"
                    className="button ghost small"
                    disabled={busy || !editable}
                    onClick={async () => {
                      const choices = groupChoices.filter((one) => !user.groups.includes(one.value));
                      if (!choices.length) return;
                      const chosen = await askChoice(tr("Add {0} to a group", [user.name]), "", choices, choices[0].value, tr("Add"));
                      if (chosen) await act((root) => lp.addParent(root, { kind: "user", name: user.name }, chosen), tr("Added {0} to {1}.", [user.name, chosen]));
                    }}
                  >
                    {tr("Add to group")}
                  </button>
                  <button type="button" className="icon-button" aria-expanded={opened} aria-label={tr("Own permissions")} data-tip={tr("Own permissions")} onClick={() => setOpenUser(opened ? null : user.uuid)}>
                    <Icon name="chevron" />
                  </button>
                </>
                }
                detail={opened && (
                  <div className="lp-user-own">
                    <div className="lp-perm-head">
                      <span>{tr("Own permissions")}</span>
                      <button type="button" className="button ghost small" disabled={busy || !editable} onClick={() => void addPermission({ kind: "user", name: user.name })}>
                        <Icon name="plus" />
                        {tr("Add permission")}
                      </button>
                    </div>
                    {permissionRows({ kind: "user", name: user.name }, user.own)}
                  </div>
                )}
              />
            );
          })}
        </List>
      )}
    </div>
  );
}

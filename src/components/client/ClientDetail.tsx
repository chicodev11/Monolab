import { useEffect, useMemo, useState, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { ClientContent as RawContent, ClientContentCard as RawCard, ClientInstance as RawClient } from "../../lib/bindings";
import { onWeb } from "../../lib/bridge";
import { fileSize, relativeTime } from "../../lib/format";
import { tr, trBack } from "../../lib/i18n";
import { commands, type Sent } from "../../lib/ipc";
import { List, ListHeader, ListRow, DropdownMenu } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";

type Client = Sent<RawClient>;
type Content = Sent<RawContent>;
type Card = Sent<RawCard>;
type Kind = "mod" | "resourcepack" | "shader";
type Shown = "all" | Kind;
type Sort = "name" | "name-desc" | "kind";
type Filter = Shown | "update" | "off";
type Tab = "content";

/** A Modrinth card's key: the kind and the file name without `.disabled`, so it survives turning off and on. */
const keyOf = (kind: string, fileName: string) => `${kind}:${fileName.replace(/\.disabled$/, "")}`;
/** A row's own key. `a.jar` and `a.jar.disabled` can both be there, so the full name. */
const rowOf = (one: { kind: string; fileName: string }) => `${one.kind}:${one.fileName}`;

function kindLabel(kind: string) {
  return kind === "mod" ? tr("Mod") : kind === "shader" ? tr("Shader pack") : tr("Resource pack");
}

/** Where to look for more of a kind on Modrinth, for this instance's version and loader. */
function browseUrl(kind: Kind, client: Client) {
  const version = `v=${encodeURIComponent(client.version)}`;
  if (kind === "resourcepack") return `https://modrinth.com/resourcepacks?${version}`;
  if (kind === "shader") return `https://modrinth.com/shaders?${version}`;
  const loader = client.loader === "Vanilla" || client.loader === "Nova" ? "" : `&g=categories:${client.loader.toLowerCase()}`;
  return `https://modrinth.com/mods?${version}${loader}`;
}

function openOutside(url: string) {
  if (onWeb) window.open(url, "_blank", "noopener,noreferrer");
  else void openUrl(url);
}

/** The inside of one Library instance: its header and everything in its mods, resource packs and
 * shader packs folders, with on/off, remove and Modrinth updates. */
export function ClientDetail({
  client,
  playButton,
  menu,
  running,
  onBack,
  onError,
  onNotice,
  confirm,
}: {
  client: Client;
  playButton: ReactNode;
  menu: { label: string; icon: string; danger?: boolean; onSelect: () => void }[];
  /** The game is starting or running; files can't change under it. */
  running: boolean;
  onBack: () => void;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
}) {
  const [items, setItems] = useState<Content[] | null>(null);
  const [cards, setCards] = useState<Record<string, Card>>({});
  const [checking, setChecking] = useState(false);
  const [query, setQuery] = useState("");
  const [shown, setShown] = useState<Filter>("all");
  const [sort, setSort] = useState<Sort>("name");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState<Set<string>>(new Set());
  /** The page's tab. Only Content for now; Files, Worlds, Screenshots and Logs can join this row later. */
  const [tab, setTab] = useState<Tab>("content");

  const fail = (cause: unknown) => onError(trBack(String(cause)));

  async function load() {
    try {
      setItems(await commands.clientContent(client.id));
    } catch (cause) {
      fail(cause);
      setItems([]);
    }
  }
  /** Names, icons and updates from Modrinth; quiet when offline. */
  async function lookUp() {
    setChecking(true);
    try {
      const list = await commands.clientContentCards(client.id);
      setCards(Object.fromEntries(list.map((card) => [keyOf(card.kind, card.fileName), card])));
    } catch {
      // Offline or Modrinth busy: the page keeps what the files say.
    } finally {
      setChecking(false);
    }
  }
  async function refresh() {
    await load();
    await lookUp();
  }
  useEffect(() => {
    setItems(null);
    setCards({});
    setPicked(new Set());
    void refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once per instance
  }, [client.id]);

  const counts = useMemo(() => {
    const all = items ?? [];
    return {
      all: all.length,
      mod: all.filter((one) => one.kind === "mod").length,
      resourcepack: all.filter((one) => one.kind === "resourcepack").length,
      shader: all.filter((one) => one.kind === "shader").length,
    };
  }, [items]);

  const rows = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const list = (items ?? []).filter((one) => {
      if (shown === "update" && !cards[keyOf(one.kind, one.fileName)]?.newVersion) return false;
      if (shown === "off" && one.enabled) return false;
      if (shown !== "all" && shown !== "update" && shown !== "off" && one.kind !== shown) return false;
      if (!needle) return true;
      const card = cards[keyOf(one.kind, one.fileName)];
      return [one.name, one.fileName, card?.title ?? ""].some((text) => text.toLowerCase().includes(needle));
    });
    const title = (one: Content) => (cards[keyOf(one.kind, one.fileName)]?.title ?? one.name).toLowerCase();
    const order = ["mod", "resourcepack", "shader"];
    if (sort === "kind") list.sort((a, b) => order.indexOf(a.kind) - order.indexOf(b.kind) || title(a).localeCompare(title(b)));
    else list.sort((a, b) => (sort === "name" ? title(a).localeCompare(title(b)) : title(b).localeCompare(title(a))));
    return list;
  }, [items, cards, query, shown, sort]);

  const updatable = (items ?? []).filter((one) => cards[keyOf(one.kind, one.fileName)]?.newVersion);
  const pickedRows = rows.filter((one) => picked.has(rowOf(one)));
  const allPicked = rows.length > 0 && pickedRows.length === rows.length;

  function mark(keys: string[], on: boolean) {
    setBusy((now) => {
      const next = new Set(now);
      for (const key of keys) {
        if (on) next.add(key);
        else next.delete(key);
      }
      return next;
    });
  }

  async function setEnabled(list: Content[], enabled: boolean) {
    const keys = list.map(rowOf);
    mark(keys, true);
    try {
      for (const one of list.filter((each) => each.enabled !== enabled)) {
        await commands.setClientContentEnabled(client.id, one.kind, one.fileName, enabled);
      }
    } catch (cause) {
      fail(cause);
    } finally {
      mark(keys, false);
      await load();
    }
  }

  async function remove(list: Content[]) {
    if (list.length === 0) return;
    const go = await confirm(
      list.length === 1 ? tr("Remove {0}?", [cards[keyOf(list[0].kind, list[0].fileName)]?.title ?? list[0].name]) : tr("Remove {0} items?", [String(list.length)]),
      tr("The files move to Monolab's old folder, where you can get them back."),
      tr("Remove"),
      true,
    );
    if (!go) return;
    try {
      for (const kind of ["mod", "resourcepack", "shader"] as const) {
        const names = list.filter((one) => one.kind === kind).map((one) => one.fileName);
        if (names.length) await commands.removeClientContent(client.id, kind, names);
      }
      setPicked(new Set());
    } catch (cause) {
      fail(cause);
    } finally {
      await load();
    }
  }

  async function update(list: Content[]) {
    const wanted = list.filter((one) => cards[keyOf(one.kind, one.fileName)]?.newVersion);
    if (wanted.length === 0) return;
    const keys = wanted.map(rowOf);
    mark(keys, true);
    let done = 0;
    try {
      for (const kind of ["mod", "resourcepack", "shader"] as const) {
        const names = wanted.filter((one) => one.kind === kind).map((one) => one.fileName);
        if (names.length) done += await commands.updateClientContent(client.id, kind, names);
      }
      onNotice(done === 1 ? tr("Updated 1 item.") : tr("Updated {0} items.", [String(done)]));
    } catch (cause) {
      fail(cause);
    } finally {
      mark(keys, false);
      await refresh();
    }
  }

  async function addFiles(kind: Kind) {
    const picked = await open({
      multiple: true,
      filters: [kind === "mod" ? { name: tr("Mods"), extensions: ["jar"] } : { name: kindLabel(kind), extensions: ["zip"] }],
    }).catch(() => null);
    const paths = picked == null ? [] : Array.isArray(picked) ? picked : [picked];
    if (paths.length === 0) return;
    try {
      const added = await commands.addClientContent(client.id, kind, paths);
      onNotice(added === 1 ? tr("Added 1 file.") : tr("Added {0} files.", [String(added)]));
    } catch (cause) {
      fail(cause);
    } finally {
      await refresh();
    }
  }

  const togglePick = (key: string) =>
    setPicked((now) => {
      const next = new Set(now);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const tabs: { id: Tab; label: string; icon: string }[] = [{ id: "content", label: tr("Content"), icon: "package" }];
  const total = counts.all;
  const modded = client.loader !== "Vanilla" && client.loader !== "Nova";
  // The same chips as a server's Mods tab: kinds, then Update and Off.
  const chips: { id: Filter; label: string; count: number }[] = [
    { id: "all", label: tr("All"), count: total },
    { id: "mod", label: tr("Mods"), count: counts.mod },
    { id: "resourcepack", label: tr("Resource packs"), count: counts.resourcepack },
    { id: "shader", label: tr("Shaders"), count: counts.shader },
    { id: "update", label: tr("Update"), count: updatable.length },
    { id: "off", label: tr("Off"), count: (items ?? []).filter((one) => !one.enabled).length },
  ];
  const playedLine = client.lastPlayed ? tr("Played {0}", [relativeTime(client.lastPlayed)]) : tr("Never played");

  return (
    <div className="cd">
      <button type="button" className="cd-back" onClick={onBack}>
        <Icon name="arrow-left" />
        {tr("Library")}
      </button>
      <header className="cd-head">
        <div className="cd-title">
          <h1 title={client.name}>{client.name}</h1>
          <p>
            <span>
              <Icon name="tag" />
              {client.loader} {client.version}
            </span>
            <i aria-hidden>•</i>
            <span>
              <Icon name="clock" />
              {playedLine}
            </span>
          </p>
        </div>
        <div className="cd-head-actions">
          {playButton}
          <DropdownMenu
            trigger={
              <button type="button" className="ch-more" aria-label={tr("More")}>
                <Icon name="dots" />
              </button>
            }
            items={menu}
          />
        </div>
      </header>

      <div className="wb-seg cd-tabs" role="tablist" aria-label={tr("Instance")}>
        {tabs.map((one) => (
          <button
            key={one.id}
            type="button"
            role="tab"
            aria-selected={tab === one.id}
            className={tab === one.id ? "on" : ""}
            onClick={() => setTab(one.id)}
          >
            <Icon name={one.icon} />
            {one.label}
          </button>
        ))}
      </div>

      {tab === "content" && (
      <div className="tool-content addon-page">
        <div className="wb-head addon-bar">
          {total > 0 && (
            <>
              <Dropdown
                value={sort}
                onChange={(next) => setSort(next as Sort)}
                options={[
                  { value: "name", label: "A → Z" },
                  { value: "name-desc", label: "Z → A" },
                  { value: "kind", label: tr("By type") },
                ]}
              />
              <div className="pick-chips">
                {chips
                  .filter((chip) => chip.id === "all" || chip.count > 0)
                  .map((chip) => (
                    <button key={chip.id} type="button" className={`pick-chip ${shown === chip.id ? "on" : ""}`} onClick={() => setShown(chip.id)}>
                      {chip.label}
                      <em>{chip.count}</em>
                    </button>
                  ))}
              </div>
              {updatable.length > 0 && (
                <button type="button" className="button ghost small" disabled={running} onClick={() => void update(updatable)}>
                  <Icon name="import" />
                  {tr("Update all")} {updatable.length}
                </button>
              )}
            </>
          )}
          <span className="live-spacer" />
          {total > 0 && (
            <label className="search addon-search wb-search-field">
              <Icon name="search" />
              <input
                value={query}
                placeholder={tr("Search {0, plural, one {# item} other {# items}}…", [total])}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
          )}
          {total > 0 && (
            <button
              type="button"
              className="wb-icon"
              disabled={checking}
              aria-label={checking ? tr("Checking…") : tr("Refresh")}
              data-tip={checking ? tr("Checking…") : tr("Refresh")}
              onClick={() => void refresh()}
            >
              <Icon name="refresh" />
            </button>
          )}
          {!onWeb && (
            <DropdownMenu
              trigger={
                <button type="button" className="button ghost small" disabled={running} data-tip={running ? tr("Close the game first.") : undefined}>
                  <Icon name="import" />
                  {tr("Add file")}
                </button>
              }
              items={[
                { label: tr("Mods"), icon: "puzzle", disabled: !modded, onSelect: () => void addFiles("mod") },
                { label: tr("Resource packs"), icon: "image", onSelect: () => void addFiles("resourcepack") },
                { label: tr("Shader packs"), icon: "sparkles", onSelect: () => void addFiles("shader") },
              ]}
            />
          )}
          <DropdownMenu
            trigger={
              <button type="button" className="button primary wb-pill">
                <Icon name="plus" />
                {modded ? tr("Add mod") : tr("Add content")}
              </button>
            }
            items={[
              { label: tr("Mods"), icon: "puzzle", disabled: !modded, onSelect: () => openOutside(browseUrl("mod", client)) },
              { label: tr("Resource packs"), icon: "image", onSelect: () => openOutside(browseUrl("resourcepack", client)) },
              { label: tr("Shader packs"), icon: "sparkles", onSelect: () => openOutside(browseUrl("shader", client)) },
            ]}
          />
        </div>

        {running && <p className="muted addon-note">{tr("Close the game to change its files.")}</p>}

        {pickedRows.length > 0 && (
          <div className="file-float" role="toolbar" aria-label={tr("Selected items")}>
            <strong>{tr("{0} selected", [pickedRows.length])}</strong>
            <button type="button" className="icon-button" aria-label={tr("Enable")} data-tip={tr("Enable")} disabled={running} onClick={() => void setEnabled(pickedRows, true)}>
              <Icon name="power" />
            </button>
            <button type="button" className="icon-button" aria-label={tr("Disable")} data-tip={tr("Disable")} disabled={running} onClick={() => void setEnabled(pickedRows, false)}>
              <Icon name="power-off" />
            </button>
            {pickedRows.some((one) => cards[keyOf(one.kind, one.fileName)]?.newVersion) && (
              <button type="button" className="icon-button" aria-label={tr("Update")} data-tip={tr("Update")} disabled={running} onClick={() => void update(pickedRows)}>
                <Icon name="import" />
              </button>
            )}
            <button type="button" className="icon-button danger" aria-label={tr("Move to trash")} data-tip={tr("Move to trash")} disabled={running} onClick={() => void remove(pickedRows)}>
              <Icon name="trash" />
            </button>
            <span className="file-float-gap" />
            <button type="button" className="icon-button" aria-label={tr("Deselect")} data-tip={tr("Deselect")} onClick={() => setPicked(new Set())}>
              <Icon name="close" />
            </button>
          </div>
        )}

        <div className="addon-drop">
          {items === null ? (
            <p className="muted">{tr("Loading…")}</p>
          ) : total === 0 ? (
            <div className="cd-empty">
              <h3>{tr("Nothing added yet")}</h3>
              <p>
                {modded
                  ? tr("Mods, resource packs and shader packs you add show here.")
                  : tr("Resource packs and shader packs you add show here.")}
              </p>
            </div>
          ) : rows.length === 0 ? (
            <p className="muted">{tr("Nothing matches.")}</p>
          ) : (
            <List className="addon-list" columns="minmax(150px, 240px)" actionsWidth="184px">
              <ListHeader
                className="addon-head"
                leading={
                  <>
                    <label className="addon-pick" data-tip={tr("Select all visible")}>
                      <input type="checkbox" checked={allPicked} onChange={(event) => setPicked(event.target.checked ? new Set(rows.map(rowOf)) : new Set())} />
                      <span>
                        <Icon name="check" />
                      </span>
                    </label>
                    <span className="addon-icon-space" />
                  </>
                }
                title={tr("Project")}
                meta={tr("Version")}
                actions={<span className="addon-head-tasks">{tr("Tasks")}</span>}
              />
              {rows.map((one) => {
                const key = rowOf(one);
                const card = cards[keyOf(one.kind, one.fileName)];
                const icon = one.icon || card?.iconUrl;
                const title = card?.title || one.name;
                const working = busy.has(key) || running;
                const plainName = one.fileName.replace(/\.disabled$/, "");
                return (
                  <ListRow
                    key={key}
                    className={`addon-row ${one.enabled ? "" : "off"}`}
                    selected={picked.has(key)}
                    leading={
                    <>
                    <label className="addon-pick">
                      <input type="checkbox" checked={picked.has(key)} aria-label={title} onChange={() => togglePick(key)} />
                      <span>
                        <Icon name="check" />
                      </span>
                    </label>
                    {icon ? (
                      <img className="addon-icon" src={icon} alt="" loading="lazy" />
                    ) : (
                      <span className="addon-icon empty">
                        <Icon name="archive" />
                      </span>
                    )}
                    </>
                    }
                    title={title}
                    subtitle={kindLabel(one.kind)}
                    meta={
                    <div className="addon-ver" data-tip={plainName}>
                      <strong>
                        {one.version || (one.folder ? tr("Folder") : fileSize(one.size ?? 0))}
                        {card?.newVersion && (
                          <button type="button" className="addon-new" data-tip={tr("Update")} disabled={working} onClick={() => void update([one])}>
                            {tr("New version")} {card.newVersion}
                          </button>
                        )}
                      </strong>
                      <small>{plainName}</small>
                    </div>
                    }
                    actions={
                    <>
                      {card && (
                        <button
                          type="button"
                          className="icon-button"
                          aria-label={tr("Open on Modrinth")}
                          data-tip={tr("Open on Modrinth")}
                          onClick={() => openOutside(`https://modrinth.com/project/${card.projectId}`)}
                        >
                          <Icon name="external" />
                        </button>
                      )}
                      {!onWeb && (
                        <button
                          type="button"
                          className="icon-button"
                          aria-label={tr("Show in folder")}
                          data-tip={tr("Show in folder")}
                          onClick={() => void commands.openClientContentFolder(client.id, one.kind).catch(fail)}
                        >
                          <Icon name="folder" />
                        </button>
                      )}
                      <button
                        type="button"
                        className="icon-button danger"
                        aria-label={tr("Move to trash")}
                        data-tip={tr("Move to trash")}
                        disabled={working}
                        onClick={() => void remove([one])}
                      >
                        <Icon name="trash" />
                      </button>
                      <label className="switch" data-tip={one.enabled ? tr("Disable") : tr("Enable")}>
                        <input type="checkbox" aria-label={title} checked={one.enabled} disabled={working} onChange={() => void setEnabled([one], !one.enabled)} />
                        <span />
                      </label>
                    </>
                    }
                  />
                );
              })}
            </List>
          )}
        </div>
      </div>
      )}
    </div>
  );
}

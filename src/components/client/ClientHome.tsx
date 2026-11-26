import { useEffect, useMemo, useRef, useState } from "react";
import type { ClientInstance as RawClient, ClientRun as RawRun } from "../../lib/bindings";
import { tr, trBack } from "../../lib/i18n";
import { commands, type Sent } from "../../lib/ipc";
import { DropdownMenu, SegmentedControl } from "../primitives";
import { CoreIcon } from "../ui/CoreIcon";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { ClientDetail } from "./ClientDetail";
import "./ClientHome.css";
import "./ClientDetail.css";

type Client = Sent<RawClient>;
type Run = Sent<RawRun>;


/** Loaders the launcher can start today. NeoForge and Forge need their installers run first.
 * Nova (Minecraft rebuilt in Rust) is offered when this copy of Monolab bundles it. */
const LOADERS = ["Vanilla", "Fabric", "Quilt", "Nova"] as const;
/** The one Minecraft version Nova plays. */
const NOVA_VERSION = "26.3";
type Loader = (typeof LOADERS)[number];
type Sort = "played" | "name" | "created";

const TILE_COLORS = ["#f2c94c", "#3aa0ff", "#ef4f6b", "#f08a3c", "#a46cff", "#35c26b", "#2fc7c0", "#e86fd1"];

/** A stable colour and letter for an instance without its own icon. */
function Tile({ client, size = "md" }: { client: Client; size?: "sm" | "md" | "lg" }) {
  const hash = [...client.id].reduce((sum, char) => (sum * 31 + char.charCodeAt(0)) >>> 0, 7);
  const color = TILE_COLORS[hash % TILE_COLORS.length];
  const letter = [...client.name.trim()][0]?.toUpperCase() ?? "?";
  return (
    <span className={`ch-tile ${size}`} style={{ "--tile": color } as React.CSSProperties} aria-hidden>
      <span>{letter}</span>
    </span>
  );
}

const loaderLabel = (client: Client) => `${client.loader} ${client.version}`;

function stageLabel(stage: string): string {
  switch (stage) {
    case "java":
      return tr("Getting Java");
    case "libraries":
      return tr("Downloading game files");
    case "assets":
      return tr("Downloading assets");
    case "nova":
      return tr("Preparing Nova");
    case "starting":
      return tr("Starting");
    default:
      return tr("Preparing");
  }
}

export function ClientHome({
  onError,
  onNotice,
  confirm,
}: {
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
}) {
  const [clients, setClients] = useState<Client[] | null>(null);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<Sort>(() => (localStorage.getItem("monolab-client-sort") as Sort | null) ?? "played");
  const [creating, setCreating] = useState(false);
  const [renaming, setRenaming] = useState<Client | null>(null);
  const [offlineName, setOfflineName] = useState<string | null>(null);
  /** Open the name dialog; launch this instance once a name is saved. */
  const [naming, setNaming] = useState<{ then: Client | null } | null>(null);
  const [runs, setRuns] = useState<Record<string, Run>>({});
  /** The instance whose detail page is open, by id. */
  const [openId, setOpenId] = useState<string | null>(null);

  const fail = (cause: unknown) => onError(trBack(String(cause)));
  async function reload() {
    try {
      setClients(await commands.listClients());
    } catch (cause) {
      fail(cause);
      setClients([]);
    }
  }
  useEffect(() => {
    void reload();
    commands
      .getClientAccount()
      .then((account) => setOfflineName(account.offlineName))
      .catch(() => setOfflineName(""));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- load once
  }, []);

  // Installs and running games are watched by polling; it is cheap and works on the web panel too.
  const runsNow = useRef<Record<string, Run>>({});
  useEffect(() => {
    let gone = false;
    let timer = 0;
    async function tick() {
      try {
        const list = await commands.clientRuns();
        if (gone) return;
        const next: Record<string, Run> = {};
        for (const run of list) next[run.id] = run;
        const before = runsNow.current;
        for (const run of list) {
          if (run.stage === "failed") {
            fail(run.error);
            void commands.dismissClientRun(run.id);
            delete next[run.id];
          }
        }
        const changed = Object.keys(before).some((id) => !next[id]) || list.some((run) => run.stage === "running" && before[run.id]?.stage !== "running");
        if (changed) void reload();
        runsNow.current = next;
        setRuns(next);
      } catch {
        // The next tick tries again.
      }
      if (!gone) timer = window.setTimeout(tick, Object.keys(runsNow.current).length ? 700 : 2000);
    }
    void tick();
    return () => {
      gone = true;
      window.clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- one poller for the page
  }, []);

  const shown = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const list = (clients ?? []).filter(
      (one) => !needle || one.name.toLowerCase().includes(needle) || loaderLabel(one).toLowerCase().includes(needle),
    );
    if (sort === "name") list.sort((a, b) => a.name.localeCompare(b.name));
    else if (sort === "created") list.sort((a, b) => b.createdAt - a.createdAt);
    else list.sort((a, b) => Math.max(b.lastPlayed, b.createdAt) - Math.max(a.lastPlayed, a.createdAt));
    return list;
  }, [clients, query, sort]);

  async function play(client: Client) {
    if (!offlineName) {
      setNaming({ then: client });
      return;
    }
    try {
      await commands.launchClient(client.id);
      const run = { id: client.id, stage: "preparing", done: 0, total: 0, error: "" };
      runsNow.current = { ...runsNow.current, [client.id]: run };
      setRuns(runsNow.current);
    } catch (cause) {
      fail(cause);
    }
  }
  async function stop(client: Client) {
    try {
      await commands.stopClient(client.id);
    } catch (cause) {
      fail(cause);
    }
  }
  /** The play control: Play, progress while installing, Stop while running. */
  function playButton(client: Client, compact = false) {
    const run = runs[client.id];
    if (run?.stage === "running") {
      return (
        <button type="button" className={`ch-play stop${compact ? " round" : ""}`} aria-label={tr("Stop")} onClick={() => void stop(client)}>
          <Icon name="stop" />
          {!compact && tr("Stop")}
        </button>
      );
    }
    if (run) {
      const percent = run.total ? Math.floor((run.done * 100) / run.total) : null;
      const label = stageLabel(run.stage);
      return (
        <span className={`ch-play busy${compact ? " round" : ""}`} data-tip={percent == null ? label : `${label} ${percent}%`}>
          <i className="ch-spin" aria-hidden />
          {!compact && (percent == null ? label : `${label} ${percent}%`)}
        </span>
      );
    }
    return (
      <button type="button" className={`ch-play${compact ? " round" : ""}`} aria-label={tr("Play game")} onClick={() => void play(client)}>
        <Icon name="play" />
        {!compact && tr("Play game")}
      </button>
    );
  }
  async function remove(client: Client) {
    const go = await confirm(
      tr("Delete {0}?", [client.name]),
      tr("The instance folder is deleted too, with its worlds, mods and settings."),
      tr("Delete"),
      true,
    );
    if (!go) return;
    try {
      await commands.deleteClient(client.id);
      await reload();
    } catch (cause) {
      fail(cause);
    }
  }
  const menu = (client: Client) => [
    { label: tr("Play game"), icon: "play", onSelect: () => void play(client) },
    { label: tr("Rename"), icon: "pencil", onSelect: () => setRenaming(client) },
    { label: tr("Open folder"), icon: "folder", onSelect: () => void commands.openClientFolder(client.id).catch(fail) },
    { label: tr("Delete"), icon: "trash", danger: true, onSelect: () => void remove(client) },
  ];
  const more = (client: Client) => (
    <DropdownMenu
      trigger={
        <button type="button" className="ch-more" aria-label={tr("More")}>
          <Icon name="dots" />
        </button>
      }
      items={menu(client)}
    />
  );

  const dialogs = (
    <>
      {creating && (
        <CreateDialog
          onClose={() => setCreating(false)}
          onCreated={(made) => {
            setCreating(false);
            onNotice(tr("Created {0}.", [made.name]));
            void reload();
          }}
          onError={fail}
        />
      )}
      {naming && (
        <NameDialog
          current={offlineName ?? ""}
          onClose={() => setNaming(null)}
          onSaved={(name) => {
            setOfflineName(name);
            const then = naming.then;
            setNaming(null);
            if (then) {
              void commands.launchClient(then.id).catch(fail);
            }
          }}
          onError={fail}
        />
      )}
      {renaming && (
        <RenameDialog
          client={renaming}
          onClose={() => setRenaming(null)}
          onDone={() => {
            setRenaming(null);
            void reload();
          }}
          onError={fail}
        />
      )}
    </>
  );

  const opened = openId ? (clients ?? []).find((one) => one.id === openId) : undefined;
  if (opened) {
    return (
      <div className="ch">
        <div className="ch-main">
          <ClientDetail
            client={opened}
            playButton={playButton(opened)}
            menu={menu(opened).filter((item) => item.icon !== "play")}
            running={Boolean(runs[opened.id])}
            onBack={() => setOpenId(null)}
            onError={onError}
            onNotice={onNotice}
            confirm={confirm}
          />
        </div>
        {dialogs}
      </div>
    );
  }

  return (
    <div className="ch">
      <div className="ch-main">
        <section className="ch-section">
          <div className="ch-headline">
            <h2 className="ch-heading">{tr("Library")}</h2>
            <div className="ch-account" aria-label={tr("Playing as")}>
              <span className="ch-account-face" aria-hidden>
                <Icon name="user" />
              </span>
              <b>{offlineName || tr("No player name")}</b>
              <span>{tr("Offline account")}</span>
              <button type="button" className="ch-link" onClick={() => setNaming({ then: null })}>
                {offlineName ? tr("Change") : tr("Set")}
              </button>
            </div>
          </div>
          <div className="ch-tools">
            <label className="ch-search">
              <Icon name="search" />
              <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={tr("Search")} />
            </label>
            <button type="button" className="ch-new" onClick={() => setCreating(true)}>
              <Icon name="plus" />
              {tr("New instance")}
            </button>
          </div>
          <div className="ch-tools">
            <Dropdown
              className="ch-sort"
              value={sort}
              options={[
                { value: "played", label: tr("Last played") },
                { value: "name", label: tr("Name") },
                { value: "created", label: tr("Date created") },
              ]}
              onChange={(next) => {
                setSort(next as Sort);
                localStorage.setItem("monolab-client-sort", next);
              }}
            />
          </div>
          {clients === null ? null : clients.length === 0 ? (
            <div className="ch-welcome">
              <div className="empty-cores">
                {["Vanilla", "Fabric"].map((core) => (
                  <span key={core} className="core-mark">
                    <CoreIcon core={core} />
                  </span>
                ))}
              </div>
              <h2>{tr("Create your first instance")}</h2>
              <p>{tr("Pick a Minecraft version and a mod loader, then play.")}</p>
              <button type="button" className="button primary" onClick={() => setCreating(true)}>
                <Icon name="plus" />
                {tr("Create instance")}
              </button>
            </div>
          ) : shown.length === 0 ? (
            <p className="ch-none">{tr("Nothing matches.")}</p>
          ) : (
            <div className="ch-grid">
              {shown.map((client) => (
                <div
                  key={client.id}
                  className={`ch-card${runs[client.id] ? " live" : ""}`}
                  onClick={(event) => {
                    // The play and menu buttons inside the card do their own thing.
                    if (!(event.target as HTMLElement).closest("button, [role='menu']")) setOpenId(client.id);
                  }}
                >
                  <div className="ch-card-art">
                    <Tile client={client} size="lg" />
                    <div className="ch-card-play">{playButton(client, true)}</div>
                  </div>
                  <div className="ch-card-text">
                    <button type="button" className="ch-card-open" onClick={() => setOpenId(client.id)}>
                      <b title={client.name}>{client.name}</b>
                    </button>
                    <span>{loaderLabel(client)}</span>
                  </div>
                  <div className="ch-card-more">{more(client)}</div>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
      {dialogs}
    </div>
  );
}

function Modal({ title, onClose, children }: { title: string; onClose: () => void; children: React.ReactNode }) {
  useEffect(() => {
    const key = (event: KeyboardEvent) => event.key === "Escape" && onClose();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);
  return (
    <div className="ch-backdrop" onMouseDown={onClose}>
      <div className="ch-dialog" role="dialog" aria-modal="true" aria-label={title} onMouseDown={(event) => event.stopPropagation()}>
        <h2>{title}</h2>
        {children}
      </div>
    </div>
  );
}

function CreateDialog({ onClose, onCreated, onError }: {
  onClose: () => void;
  onCreated: (made: Client) => void;
  onError: (cause: unknown) => void;
}) {
  const [name, setName] = useState("");
  const [loader, setLoader] = useState<Loader>("Vanilla");
  const [versions, setVersions] = useState<string[] | null>(null);
  const [version, setVersion] = useState("");
  const [busy, setBusy] = useState(false);
  const [nova, setNova] = useState(false);
  useEffect(() => {
    commands.bundledForks().then((forks) => setNova(forks.nova)).catch(() => setNova(false));
  }, []);
  // Fabric and Quilt follow Fabric's list; the others start from every release.
  const source = loader === "Fabric" || loader === "Quilt" ? "Fabric" : "Vanilla";
  useEffect(() => {
    if (loader === "Nova") {
      setVersion(NOVA_VERSION);
      return;
    }
    let gone = false;
    setVersions(null);
    commands
      .listCoreVersions(source, false)
      .then((list) => {
        if (gone) return;
        setVersions(list);
        setVersion((now) => (list.includes(now) ? now : (list[0] ?? "")));
      })
      .catch((cause) => {
        if (gone) return;
        setVersions([]);
        onError(cause);
      });
    return () => {
      gone = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- refetch per list source
  }, [source, loader === "Nova"]);
  const shownName = name.trim() || (version ? `${loader === "Vanilla" ? "" : `${loader} `}${version}` : "");
  async function create() {
    if (!shownName || !version) return;
    setBusy(true);
    try {
      onCreated(await commands.createClient(shownName, version, loader));
    } catch (cause) {
      onError(cause);
    } finally {
      setBusy(false);
    }
  }
  return (
    <Modal title={tr("New instance")} onClose={onClose}>
      <label className="ch-field">
        <span>{tr("Name")}</span>
        <input autoFocus value={name} placeholder={shownName || tr("Name")} onChange={(event) => setName(event.target.value)} onKeyDown={(event) => event.key === "Enter" && void create()} />
      </label>
      <div className="ch-field">
        <span>{tr("Mod loader")}</span>
        <SegmentedControl
          value={loader}
          options={LOADERS.filter((one) => one !== "Nova" || nova).map((one) => ({ value: one, label: one === "Nova" ? tr("Nova (Rust)") : one }))}
          onChange={setLoader}
        />
      </div>
      <div className="ch-field">
        <span>{tr("Minecraft version")}</span>
        {loader === "Nova" ? (
          <p className="ch-dialog-copy">{tr("Nova plays Minecraft {0}, rebuilt in Rust. The first start prepares its game files once.", [NOVA_VERSION])}</p>
        ) : versions?.length === 0 ? (
          <input value={version} placeholder="1.21.5" onChange={(event) => setVersion(event.target.value)} />
        ) : (
        <Dropdown
          value={version}
          disabled={!versions?.length}
          placeholder={versions === null ? tr("Loading…") : tr("Choose")}
          options={(versions ?? []).map((one) => ({ value: one, label: one }))}
          onChange={setVersion}
          search={tr("Search")}
        />
        )}
      </div>
      <div className="modal-actions">
        <button type="button" className="button secondary" onClick={onClose}>
          {tr("Cancel")}
        </button>
        <button type="button" className="button primary" disabled={busy || !version || !shownName} onClick={() => void create()}>
          {tr("Create")}
        </button>
      </div>
    </Modal>
  );
}

function RenameDialog({ client, onClose, onDone, onError }: {
  client: Client;
  onClose: () => void;
  onDone: () => void;
  onError: (cause: unknown) => void;
}) {
  const [name, setName] = useState(client.name);
  async function save() {
    try {
      await commands.renameClient(client.id, name);
      onDone();
    } catch (cause) {
      onError(cause);
    }
  }
  return (
    <Modal title={tr("Rename")} onClose={onClose}>
      <label className="ch-field">
        <span>{tr("Name")}</span>
        <input autoFocus value={name} onChange={(event) => setName(event.target.value)} onFocus={(event) => event.currentTarget.select()} onKeyDown={(event) => event.key === "Enter" && void save()} />
      </label>
      <div className="modal-actions">
        <button type="button" className="button secondary" onClick={onClose}>
          {tr("Cancel")}
        </button>
        <button type="button" className="button primary" disabled={!name.trim()} onClick={() => void save()}>
          {tr("Save")}
        </button>
      </div>
    </Modal>
  );
}

function NameDialog({ current, onClose, onSaved, onError }: {
  current: string;
  onClose: () => void;
  onSaved: (name: string) => void;
  onError: (cause: unknown) => void;
}) {
  const [name, setName] = useState(current);
  const valid = /^[A-Za-z0-9_]{3,16}$/.test(name.trim());
  async function save() {
    if (!valid) return;
    try {
      const account = await commands.setOfflineName(name.trim());
      onSaved(account.offlineName);
    } catch (cause) {
      onError(cause);
    }
  }
  return (
    <Modal title={tr("Offline player name")} onClose={onClose}>
      <p className="ch-dialog-copy">{tr("Offline mode plays singleplayer, LAN and servers with online-mode off. The name is what other players see.")}</p>
      <label className="ch-field">
        <span>{tr("Player name")}</span>
        <input autoFocus value={name} maxLength={16} placeholder="Steve" onChange={(event) => setName(event.target.value)} onKeyDown={(event) => event.key === "Enter" && void save()} />
        <small className={valid || !name ? "" : "bad"}>{tr("3 to 16 letters, numbers or underscores.")}</small>
      </label>
      <div className="modal-actions">
        <button type="button" className="button secondary" onClick={onClose}>
          {tr("Cancel")}
        </button>
        <button type="button" className="button primary" disabled={!valid} onClick={() => void save()}>
          {tr("Save")}
        </button>
      </div>
    </Modal>
  );
}

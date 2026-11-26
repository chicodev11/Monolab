import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import type { StudioAssets, StudioOverview, StudioResource } from "../../lib/bindings";
import { listenTo, onWeb } from "../../lib/bridge";
import { relativeTime } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { DropdownMenu, EmptyState, FilterTabs, List, ListRow, SearchField } from "../primitives";
import { Icon } from "../ui/Icon";
import { StudioFields } from "./StudioFields";
import { StudioGraph } from "./StudioGraph";
import { hasPreview, StudioPreview, type StudioData } from "./StudioPreview";
import { StudioAssetLibrary } from "./StudioAssets";
import { StudioExpansions, useStudioExpansions } from "./StudioActions";
import { StudioBundles } from "./StudioBundles";
import { StudioHeldItem } from "./StudioHeldItem";
import { StudioRuleTest } from "./StudioRuleTest";
import { StudioSideSlot } from "./StudioPack3d";
import { StudioFormToc } from "./StudioFormToc";
import { StudioSync } from "./StudioSync";
import { EditingChip } from "./EditingChip";
import { FONT_ICON, fontKindHint, fontKindLabel } from "./studioKinds";
import { reloadGlyphs } from "./studioGlyphs";
import "./Studio.css";
import { Splitter, usePaneWidth, widthVar } from "../ui/Splitter";

const CodeEditor = lazy(() => import("../files/CodeEditor").then((module) => ({ default: module.CodeEditor })));

/** Studio kinds in the order the add-on lists them; labels are made when drawn, never at module load. */
export const STUDIO_KINDS = [
  "scoreboard",
  "tab",
  "motd",
  "chat",
  "message",
  "announcement",
  "bossbar",
  "hologram",
  "npc",
  "dialog",
  "command",
  "menu",
  "item",
  "recipe",
  "loot",
  "advancement",
  "rule",
  "world",
  "font",
  "model",
  "block",
  "furniture",
  "mob",
  "enchant",
  "song",
  "painting",
  "cosmetic",
  "trade",
  "function",
  "quest",
  "structure",
  "biome",
  "shop",
] as const;
export type StudioKind = (typeof STUDIO_KINDS)[number];
/** Kinds the node editor opens for (`rootsOf` in StudioGraph), besides rules. */
const NODE_KINDS: string[] = ["command", "function", "quest", "item", "block", "furniture", "enchant", "npc", "mob"];

export function kindLabel(kind: string): string {
  switch (kind) {
    case "scoreboard":
      return tr("Scoreboard");
    case "tab":
      return tr("Tab list");
    case "motd":
      return tr("Server list (MOTD)");
    case "chat":
      return tr("Chat format");
    case "message":
      return tr("Join & leave messages");
    case "announcement":
      return tr("Announcements");
    case "bossbar":
      return tr("Boss bar");
    case "hologram":
      return tr("Holograms");
    case "npc":
      return tr("NPCs");
    case "dialog":
      return tr("Dialogs");
    case "loot":
      return tr("Loot tables");
    case "advancement":
      return tr("Advancements");
    case "world":
      return tr("Worlds");
    case "font":
      return fontKindLabel();
    case "model":
      return tr("Models");
    case "block":
      return tr("Blocks");
    case "furniture":
      return tr("Furniture");
    case "mob":
      return tr("Mobs");
    case "enchant":
      return tr("Enchantments");
    case "song":
      return tr("Music discs");
    case "painting":
      return tr("Paintings");
    case "cosmetic":
      return tr("Cosmetics");
    case "trade":
      return tr("Trades");
    case "shop":
      return tr("Shops");
    case "function":
      return tr("Functions");
    case "quest":
      return tr("Quests");
    case "structure":
      return tr("Structures");
    case "biome":
      return tr("Biomes");
    case "command":
      return tr("Commands");
    case "menu":
      return tr("Menus");
    case "item":
      return tr("Items");
    case "recipe":
      return tr("Recipes");
    case "rule":
      return tr("Rules");
    default:
      return kind;
  }
}

/** The kinds in groups, for the start page and the New menu. Group names are made when drawn. */
const KIND_GROUPS: { id: string; kinds: string[] }[] = [
  { id: "look", kinds: ["scoreboard", "tab", "motd", "chat", "message", "announcement", "bossbar", "hologram", "font"] },
  { id: "items", kinds: ["item", "recipe", "loot", "enchant", "song", "painting", "cosmetic", "trade", "shop"] },
  { id: "things", kinds: ["block", "furniture", "model", "mob", "npc"] },
  { id: "logic", kinds: ["rule", "command", "function", "quest", "menu", "dialog", "advancement"] },
  { id: "world", kinds: ["world", "structure", "biome"] },
];
function groupLabel(id: string): string {
  switch (id) {
    case "look":
      return tr("Looks and chat");
    case "items":
      return tr("Items and trading");
    case "things":
      return tr("Blocks, models and mobs");
    case "logic":
      return tr("Logic and menus");
    case "world":
      return tr("Worlds");
    default:
      return id;
  }
}

/** One icon per kind, for the list and the start cards. */
const KIND_ICON: Record<string, string> = {
  scoreboard: "list",
  tab: "users",
  motd: "server",
  chat: "message",
  message: "bell",
  announcement: "schedule",
  bossbar: "skull",
  hologram: "sparkles",
  npc: "user",
  dialog: "panel-open",
  loot: "package",
  advancement: "shield-check",
  world: "mountain",
  font: FONT_ICON,
  model: "box",
  block: "blocks",
  furniture: "home",
  mob: "ghost",
  enchant: "sparkles",
  song: "disc",
  painting: "image",
  cosmetic: "wand",
  trade: "coins",
  shop: "tag",
  function: "workflow",
  quest: "map",
  structure: "layers",
  biome: "leaf",
  command: "terminal",
  menu: "grid",
  item: "sword",
  recipe: "hammer",
  rule: "wand",
};

/** What each kind changes in the game, for the start cards. */
function kindHint(kind: string): string {
  switch (kind) {
    case "scoreboard":
      return tr("The sidebar on the right of the screen");
    case "tab":
      return tr("Header and footer of the player list");
    case "motd":
      return tr("The text under the server's name in the list");
    case "chat":
      return tr("How chat lines look");
    case "message":
      return tr("Lines shown when players join or leave");
    case "announcement":
      return tr("Messages sent on a timer");
    case "bossbar":
      return tr("The bar at the top of the screen");
    case "hologram":
      return tr("Floating text at a spot in a world");
    case "npc":
      return tr("A mob that stands still and runs actions when clicked");
    case "dialog":
      return tr("A window with text, inputs and buttons");
    case "loot":
      return tr("Random drops for blocks, mobs and rewards");
    case "advancement":
      return tr("Your own tab in the advancements screen");
    case "world":
      return tr("Extra worlds: flat, void, amplified and more");
    case "font":
      return fontKindHint();
    case "model":
      return tr("Blockbench models with animations, standing in a world");
    case "block":
      return tr("New blocks with their own look, drops and break time");
    case "furniture":
      return tr("Chairs, tables and decorations players can place and sit on");
    case "mob":
      return tr("Custom mobs with stats, models, drops, spawning and skills");
    case "enchant":
      return tr("Your own enchantments that run actions on hits, mining and hurts");
    case "song":
      return tr("Jukebox songs from your own sounds, for music disc items");
    case "painting":
      return tr("Paintings players can hang, from your own pictures");
    case "cosmetic":
      return tr("Hats, back pieces and walking trails players can wear");
    case "trade":
      return tr("Villager trade screens with your own offers");
    case "shop":
      return tr("Chest shops that buy and sell for a variable or Vault money");
    case "function":
      return tr("Action lists you can run from anywhere, so steps aren't copied around");
    case "quest":
      return tr("Steps players go through, with rewards at the end");
    case "structure":
      return tr("Builds from .schem or .nbt files, put down by an action");
    case "biome":
      return tr("Your own biomes with their sky, fog, water and grass colours, for world generators");
    case "command":
      return tr("Your own /commands that run actions");
    case "menu":
      return tr("Chest menus with clickable slots");
    case "item":
      return tr("Named items with lore, enchantments and use actions");
    case "recipe":
      return tr("New crafting and smelting recipes");
    case "rule":
      return tr("When something happens, check and do things");
    default:
      return "";
  }
}

/** File names the add-on accepts: a-z, 0-9, - and _, up to 48. */
export function studioName(input: string): string | null {
  const name = input.trim().toLowerCase().replace(/\s+/g, "-");
  return /^[a-z0-9_-]{1,48}$/.test(name) ? name : null;
}

const POLL_MS = 3000;

type Open = { kind: string; id: string; text: string; hash: string; edited: string | null };
/** One thing the add-on reported over the live line: a join or leave, or a rule that ran. */
type Heard = { key: number; at: number; type: "player" | "trace"; text: string; rule?: string; passed?: boolean };
const MOST_HEARD = 60;

export function StudioPage({
  serverId,
  running,
  onError,
  onNotice,
  confirm,
  askText,
  openAddons,
  openHistory,
  addonInstalled = true,
}: {
  serverId: string;
  running: boolean;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
  askText: (title: string, label: string, value: string, confirmLabel: string) => Promise<string | null>;
  openAddons: () => void;
  /** Opens History's automatic saves on one file (a path in the server folder). Missing where there are none (SSH, panel). */
  openHistory?: (path: string) => void;
  /** False when the page above already shows the install strip, so the note here stays quiet. */
  addonInstalled?: boolean;
}) {
  const listPane = usePaneWidth("studio-list", 320, 220, 560);
  const previewPane = usePaneWidth("studio-preview", 460, 260, 900);
  const [sideSlot, setSideSlot] = useState<HTMLElement | null>(null);
  const [overview, setOverview] = useState<StudioOverview | null>(null);
  /** The Minecraft pictures this server's studio needs; the studio opens only once they are on this PC. */
  const [assets, setAssets] = useState<StudioAssets | null>(null);
  const [assetsProblem, setAssetsProblem] = useState<string | null>(null);
  const [downloading, setDownloading] = useState(false);
  useEffect(() => {
    let alive = true;
    commands.studioAssets(serverId).then(
      (found) => {
        if (alive) setAssets(found);
      },
      (cause) => {
        if (alive) setAssetsProblem(String(cause));
      },
    );
    return () => {
      alive = false;
    };
  }, [serverId]);
  async function downloadAssets() {
    setDownloading(true);
    setAssetsProblem(null);
    try {
      setAssets(await commands.downloadStudioAssets(serverId));
    } catch (cause) {
      setAssetsProblem(String(cause));
    } finally {
      setDownloading(false);
    }
  }
  const art = assets?.ready ? assets.version : null;
  const [open, setOpen] = useState<Open | null>(null);
  const [showAssets, setShowAssets] = useState(false);
  const [fileQuery, setFileQuery] = useState("");
  const [startQuery, setStartQuery] = useState("");
  // Kinds folded in the file list; remembered per PC.
  const [folded, setFolded] = useState<string[]>(() => {
    try {
      const saved: unknown = JSON.parse(localStorage.getItem("monolab.studio.folded") ?? "[]");
      return Array.isArray(saved) ? saved.filter((one): one is string => typeof one === "string") : [];
    } catch {
      return [];
    }
  });
  const fold = (kind: string) =>
    setFolded((was) => {
      const next = was.includes(kind) ? was.filter((one) => one !== kind) : [...was, kind];
      try {
        localStorage.setItem("monolab.studio.folded", JSON.stringify(next));
      } catch {
        // Only remembered for this visit.
      }
      return next;
    });
  const [liveOpen, setLiveOpen] = useState(true);
  const [showHeld, setShowHeld] = useState(false);
  const [bundles, setBundles] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const expansions = useStudioExpansions(serverId);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState("");
  const [mode, setMode] = useState<"form" | "nodes" | "text">("form");
  const [heard, setHeard] = useState<Heard[]>([]);
  const [listening, setListening] = useState(false);
  // Keys for live rows stay unique across reconnects.
  const heardSerial = useRef(0);
  // Plugins the user chose to run alongside ours anyway: their warning stays hidden on this server.
  const [keptBoth, setKeptBoth] = useState<string[]>(() => {
    try {
      const saved: unknown = JSON.parse(localStorage.getItem(`monolab.studio.keepBoth.${serverId}`) ?? "[]");
      return Array.isArray(saved) ? saved.filter((one): one is string => typeof one === "string") : [];
    } catch {
      return [];
    }
  });

  // Glyph pictures are read once per server; a font file made, changed or turned off anywhere (a teammate,
  // an AI, the files page) shows here as a different list, and the previews read them again.
  const fonts = useRef("");
  const load = useCallback(async () => {
    try {
      const next = await commands.studioOverview(serverId);
      const seen = next.resources
        .filter((one) => one.kind === "font")
        .map((one) => `${one.id}:${one.enabled}:${one.modified ?? ""}`)
        .join("|");
      if (fonts.current && fonts.current !== seen) reloadGlyphs(serverId);
      fonts.current = seen;
      setOverview(next);
    } catch (cause) {
      onError(String(cause));
    }
  }, [serverId, onError]);

  useEffect(() => {
    void load();
  }, [load]);

  // Joins, leaves and rule runs, live from the add-on while a server on this PC or over SSH runs.
  useEffect(() => {
    if (!running) {
      setListening(false);
      return;
    }
    let gone = false;
    const off = listenTo<{ serverId: string; data: string }>("studio-live", (event) => {
      if (event.serverId !== serverId) return;
      if (!event.data) {
        setListening(false);
        return;
      }
      let message: Record<string, unknown>;
      try {
        message = JSON.parse(event.data) as Record<string, unknown>;
      } catch {
        return;
      }
      const name = typeof message.player === "string" ? message.player : typeof message.name === "string" ? message.name : "";
      const entry: Heard =
        message.type === "trace"
          ? {
              key: heardSerial.current++,
              at: typeof message.at === "number" ? message.at : Date.now(),
              type: "trace",
              rule: String(message.rule ?? ""),
              passed: message.passed === true,
              text: Array.isArray(message.ran) ? (message.ran as unknown[]).map(String).join(", ") : "",
            }
          : { key: heardSerial.current++, at: Date.now(), type: "player", text: message.event === "leave" ? tr("{0} left", [name]) : tr("{0} joined", [name]) };
      if (entry.type === "trace" && name) entry.text = entry.text ? `${name} · ${entry.text}` : name;
      setHeard((was) => [entry, ...was].slice(0, MOST_HEARD));
    });
    // The add-on writes live.json a moment after it starts; try again a few times.
    const tryWatch = (left: number) => {
      void commands.studioWatch(serverId).then(
        (ok) => {
          if (gone) return;
          setListening(ok);
          if (!ok && left > 0) window.setTimeout(() => tryWatch(left - 1), 4000);
        },
        () => undefined,
      );
    };
    tryWatch(5);
    return () => {
      gone = true;
      void off.then((stop) => stop());
      void commands.studioUnwatch(serverId);
    };
  }, [running, serverId]);

  // The add-on rereads files every 2 seconds; while the server runs, keep its verdicts fresh.
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => void load(), POLL_MS);
    return () => window.clearInterval(timer);
  }, [running, load]);

  // The kinds this server's add-on runs (Pumpkin: the looks only); a kind with files left over still shows.
  const runs = useMemo<StudioKind[]>(() => STUDIO_KINDS.filter((kind) => !overview || overview.kinds.includes(kind)), [overview]);
  const groups = useMemo(() => {
    const list = overview?.resources ?? [];
    const needle = fileQuery.trim().toLowerCase();
    return STUDIO_KINDS.map((kind) => {
      const all = list.filter((one) => one.kind === kind);
      const kindHit = needle !== "" && kindLabel(kind).toLowerCase().includes(needle);
      return { kind, items: needle && !kindHit ? all.filter((one) => one.id.toLowerCase().includes(needle)) : all };
    }).filter(({ items }) => items.length > 0);
  }, [overview, fileQuery]);
  const fileOf = (kind: string, id: string) => `${overview?.root ?? "plugins/Monolab/studio"}/${kind}/${id}.json`;

  /** Every saved version of the open file, in History. Unsaved edits would be lost, so ask first. */
  async function showHistory() {
    if (!open || !openHistory) return;
    if (dirty && !(await confirm(tr("Leave without saving?"), tr("Your changes to {0} aren't saved yet.", [open.id]), tr("Leave"), true))) return;
    openHistory(fileOf(open.kind, open.id));
  }

  const current = open ? overview?.resources.find((one) => one.kind === open.kind && one.id === open.id) : undefined;
  const names = useMemo(
    () => ({
      menu: (overview?.resources ?? []).filter((one) => one.kind === "menu").map((one) => one.id),
      item: (overview?.resources ?? []).filter((one) => one.kind === "item").map((one) => one.id),
      dialog: (overview?.resources ?? []).filter((one) => one.kind === "dialog").map((one) => one.id),
      loot: (overview?.resources ?? []).filter((one) => one.kind === "loot").map((one) => one.id),
      advancement: (overview?.resources ?? []).filter((one) => one.kind === "advancement").map((one) => one.id),
      model: (overview?.resources ?? []).filter((one) => one.kind === "model").map((one) => one.id),
      mob: (overview?.resources ?? []).filter((one) => one.kind === "mob").map((one) => one.id),
      enchant: (overview?.resources ?? []).filter((one) => one.kind === "enchant").map((one) => one.id),
      song: (overview?.resources ?? []).filter((one) => one.kind === "song").map((one) => one.id),
      cosmetic: (overview?.resources ?? []).filter((one) => one.kind === "cosmetic").map((one) => one.id),
      trade: (overview?.resources ?? []).filter((one) => one.kind === "trade").map((one) => one.id),
      function: (overview?.resources ?? []).filter((one) => one.kind === "function").map((one) => one.id),
      quest: (overview?.resources ?? []).filter((one) => one.kind === "quest").map((one) => one.id),
      structure: (overview?.resources ?? []).filter((one) => one.kind === "structure").map((one) => one.id),
      biome: (overview?.resources ?? []).filter((one) => one.kind === "biome").map((one) => one.id),
      shop: (overview?.resources ?? []).filter((one) => one.kind === "shop").map((one) => one.id),
    }),
    [overview],
  );
  const hasNodes = open?.kind === "rule" || (open ? NODE_KINDS.includes(open.kind) : false);
  const shownMode = mode === "nodes" && !hasNodes ? "form" : mode;
  const views: [typeof mode, string][] = hasNodes ? [["form", tr("Fields")], ["nodes", tr("Nodes")], ["text", tr("Editor")]] : [["form", tr("Fields")], ["text", tr("Editor")]];
  const ruleRuns = open?.kind === "rule" ? heard.filter((one) => one.type === "trace" && one.rule === open.id) : [];
  const dirty = open?.edited != null && open.edited !== open.text;
  const shownText = open ? (open.edited ?? open.text) : "";
  const parsed = useMemo<StudioData | null>(() => {
    try {
      const value: unknown = JSON.parse(shownText);
      return value && typeof value === "object" && !Array.isArray(value) ? (value as StudioData) : null;
    } catch {
      return null;
    }
  }, [shownText]);

  function edit(next: string) {
    setOpen((now) => (now ? { ...now, edited: next === now.text ? null : next } : now));
  }

  async function pick(one: StudioResource) {
    if (dirty && !(await confirm(tr("Discard changes?"), tr("Your edits to this file haven't been saved."), tr("Discard"), true))) return;
    try {
      const file = await commands.studioRead(serverId, one.kind, one.id);
      setOpen({ kind: one.kind, id: one.id, text: file.text, hash: file.hash, edited: null });
      setShowAssets(false);
      setShowHeld(false);
      setProblem("");
    } catch (cause) {
      onError(String(cause));
    }
  }

  async function create(kind: StudioKind) {
    const typed = await askText(tr("New {0}", [kindLabel(kind)]), tr("Name (a-z, 0-9, - and _)"), "default", tr("Create"));
    if (typed === null) return;
    const name = studioName(typed);
    if (!name) return onError(tr("Names may only use a-z, 0-9, - and _ (up to 48)."));
    setBusy(true);
    try {
      const text = await commands.studioTemplate(kind, name);
      const { hash } = await commands.studioSave(serverId, kind, name, text, "");
      setOpen({ kind, id: name, text, hash, edited: null });
      setShowAssets(false);
      setShowHeld(false);
      setProblem("");
      await load();
    } catch (cause) {
      onError(String(cause));
    }
    setBusy(false);
  }

  async function save() {
    if (!open || open.edited === null) return;
    setBusy(true);
    try {
      const { hash, applied } = await commands.studioSave(serverId, open.kind, open.id, open.edited, open.hash);
      setOpen({ ...open, text: open.edited, hash, edited: null });
      setProblem("");
      onNotice(
        applied
          ? tr("Saved and applied on the server.")
          : running
            ? tr("Saved. The server picks it up within a few seconds.")
            : tr("Saved. It applies when the server starts."),
      );
      await load();
    } catch (cause) {
      setProblem(String(cause));
    }
    setBusy(false);
  }

  async function reload() {
    if (!open) return;
    try {
      const file = await commands.studioRead(serverId, open.kind, open.id);
      setOpen({ ...open, text: file.text, hash: file.hash, edited: null });
      setProblem("");
    } catch (cause) {
      onError(String(cause));
    }
  }

  /** Turns studio kinds on or off for this server (`studio-<kind>` in the add-on's config.yml). */
  async function setKinds(kinds: string[], on: boolean) {
    setBusy(true);
    try {
      for (const kind of kinds) await commands.setAddonConfig(serverId, `studio-${kind}`, on);
      await load();
      onNotice(on ? tr("Turned back on. The add-on picks it up within a few seconds.") : tr("Turned ours off. The add-on picks it up within a few seconds."));
    } catch (cause) {
      onError(String(cause));
    }
    setBusy(false);
  }

  function keepBoth(plugin: string) {
    const next = [...keptBoth, plugin];
    setKeptBoth(next);
    try {
      localStorage.setItem(`monolab.studio.keepBoth.${serverId}`, JSON.stringify(next));
    } catch {
      // Only remembered for this visit.
    }
  }

  async function remove() {
    if (!open) return;
    const ok = await confirm(
      tr("Move {0} to the trash?", [open.id]),
      tr("The file goes to the server's trash, so you can bring it back."),
      tr("Move to trash"),
      true,
    );
    if (!ok) return;
    try {
      await commands.trashServerPath(serverId, fileOf(open.kind, open.id));
      setOpen(null);
      await load();
    } catch (cause) {
      onError(String(cause));
    }
  }

  const section = showHeld ? "held" : showAssets ? "assets" : "files";
  const goSection = (next: "files" | "assets" | "held") => {
    setShowAssets(next === "assets");
    setShowHeld(next === "held");
  };
  const title = <h1>{tr("Studio")}</h1>;
  if (!overview) return <div className="studio-page"><div className="wb-head">{title}</div><p className="muted">{tr("Loading…")}</p></div>;
  if (!overview.supported) {
    return (
      <div className="studio-page">
        <div className="wb-head">{title}</div>
        <EmptyState icon="sparkles" title={tr("Not on this server yet")} hint={tr("The studio works on Paper, Purpur, Folia and Leaf servers for now.")} />
      </div>
    );
  }
  if (!assets?.ready) {
    // The studio draws with the game's own pictures, so they must be here first, for this exact version.
    return (
      <div className="studio-page">
      <div className="wb-head">{title}</div>
      <EmptyState
        icon="import"
        title={assets ? tr("Download the Minecraft {0} pictures", [assets.version]) : assetsProblem ? tr("Can't tell which pictures the studio needs") : tr("Checking the game pictures…")}
        hint={
          assetsProblem ??
          (assets
            ? tr("The studio shows items, menus and text the way the game draws them. Monolab downloads the Minecraft client file for this version from Mojang (about 40 MB) and keeps it on this PC. A new version needs its own download.")
            : undefined)
        }
        action={
          assets && (
            <button type="button" className="button primary" disabled={downloading} onClick={() => void downloadAssets()}>
              <Icon name="import" />
              {downloading ? tr("Downloading…") : tr("Download and open the studio")}
            </button>
          )
        }
      />
      </div>
    );
  }

  const addonMissing = !overview.hasStatus && addonInstalled;
  const offKinds = overview.off ?? [];
  const clashes = (overview.overlaps ?? []).filter((one) => !keptBoth.includes(one.plugin) && one.kinds.some((kind) => !offKinds.includes(kind)));
  const kindList = (kinds: string[]) => kinds.map((kind) => kindLabel(kind)).join(", ");
  const state = !current
    ? null
    : current.error
      ? { tone: "bad", label: tr("Error") }
      : offKinds.includes(current.kind)
        ? { tone: "off", label: tr("Turned off") }
        : !current.enabled
        ? { tone: "off", label: tr("Turned off") }
        : !running
          ? { tone: "off", label: tr("Applies on start") }
          : current.seen
            ? { tone: "ok", label: tr("Live") }
            : { tone: "wait", label: tr("Waiting") };
  const newItems = KIND_GROUPS.flatMap((group) =>
    group.kinds
      .filter((kind) => runs.includes(kind as StudioKind))
      .map((kind, at) => ({ heading: at === 0 ? groupLabel(group.id) : undefined, label: kindLabel(kind), icon: KIND_ICON[kind], onSelect: () => void create(kind as StudioKind) })),
  );
  const startNeedle = startQuery.trim().toLowerCase();
  const homeGroups = STUDIO_KINDS.map((kind) => ({
    kind,
    items: overview.resources
      .filter((one) => one.kind === kind && (!startNeedle || `${one.id} ${kindLabel(kind)}`.toLowerCase().includes(startNeedle)))
      .sort((x, y) => (y.modified ?? 0) - (x.modified ?? 0)),
  })).filter(({ items }) => items.length > 0);
  const startGroups = KIND_GROUPS.map((group) => ({
    ...group,
    kinds: group.kinds.filter((kind) => runs.includes(kind as StudioKind) && (!startNeedle || `${kindLabel(kind)} ${kindHint(kind)}`.toLowerCase().includes(startNeedle))),
  })).filter((group) => group.kinds.length > 0);
  const counts = new Map<string, number>();
  for (const one of overview.resources) counts.set(one.kind, (counts.get(one.kind) ?? 0) + 1);
  return (
    <div className="git-page studio-page">
      <div className="wb-head studio-bar">
        {title}
        <FilterTabs
          value={section}
          onChange={goSection}
          label={tr("Studio")}
          options={[
            { value: "files", label: tr("Files"), count: overview.resources.length },
            { value: "assets", label: tr("Assets") },
            ...(!onWeb ? [{ value: "held" as const, label: tr("Held item") }] : []),
          ]}
        />
        <span className="live-spacer" />
        <button type="button" className="button secondary small" data-tip={tr("Share and import")} onClick={() => setBundles(true)}>
          <Icon name="package" />
          {tr("Share")}
        </button>
        {!onWeb && (
          <button type="button" className="button secondary small" data-tip={tr("Copy to other servers")} onClick={() => setSyncing(true)}>
            <Icon name="network" />
            {tr("Copy")}
          </button>
        )}
        <DropdownMenu
          trigger={
            <button type="button" className="button primary wb-pill" disabled={busy}>
              <Icon name="plus" />
              {tr("New")}
            </button>
          }
          items={newItems}
        />
      </div>
      {syncing && <StudioSync serverId={serverId} onClose={() => setSyncing(false)} onNotice={onNotice} />}
      {bundles && (
        <StudioBundles
          serverId={serverId}
          resources={overview.resources}
          kindLabel={kindLabel}
          onClose={() => setBundles(false)}
          onError={onError}
          onNotice={onNotice}
          onImported={() => void load()}
        />
      )}
      {addonMissing ? (
        <div className="git-note">
          <Icon name="puzzle" />
          <span>{tr("The studio runs inside the Monolab add-on. Once it's installed, start the server once so it can report here.")}</span>
        </div>
      ) : overview.hasStatus && !overview.on ? (
        <div className="git-note">
          <Icon name="alert" />
          <span>{tr("The studio is turned off in the add-on settings, so these files aren't used.")}</span>
          <button type="button" className="button small secondary" onClick={openAddons}>
            {tr("Open add-on settings")}
          </button>
        </div>
      ) : null}
      {overview.on &&
        clashes.map((one) => (
          <div key={one.plugin} className="git-note" data-tip={one.file}>
            <Icon name="alert" />
            <span>{tr("{0} is installed and also does {1}. Running both can clash, so you may want only one.", [one.plugin, kindList(one.kinds.filter((kind) => !offKinds.includes(kind)))])}</span>
            <button type="button" className="button small secondary" disabled={busy} onClick={() => void setKinds(one.kinds.filter((kind) => !offKinds.includes(kind)), false)}>
              {tr("Turn ours off")}
            </button>
            <button type="button" className="button small ghost" onClick={() => keepBoth(one.plugin)}>
              {tr("Keep both")}
            </button>
          </div>
        ))}
      {overview.on && offKinds.length > 0 && (
        <div className="git-note">
          <Icon name="power" />
          <span>{tr("Turned off on this server: {0}. Their files are kept but not used.", [kindList(offKinds)])}</span>
          <button type="button" className="button small secondary" disabled={busy} onClick={() => void setKinds(offKinds, true)}>
            {tr("Turn back on")}
          </button>
        </div>
      )}
      {section === "held" ? (
        <div className="studio-wide">
          <StudioHeldItem serverId={serverId} onNotice={onNotice} />
        </div>
      ) : section === "assets" ? (
        <div className="studio-wide">
          <StudioAssetLibrary serverId={serverId} pumpkin={overview.root.startsWith("plugins/data/")} onError={onError} onNotice={onNotice} confirm={confirm} />
        </div>
      ) : (
      <div className="git-layout with-split" style={widthVar("--side-w", listPane.width)}>
        <div className="git-list studio-list">
          <SearchField className="fill studio-find" value={fileQuery} onChange={setFileQuery} placeholder={tr("Find a file")} />
          <div className="studio-groups">
          {groups.length === 0 && (
            <p className="studio-list-empty">{fileQuery.trim() ? tr("No matching files.") : tr("No files yet. Start one with New.")}</p>
          )}
          {groups.map(({ kind, items }) => {
            const closed = folded.includes(kind) && !fileQuery.trim();
            return (
            <div key={kind} className={`studio-group ${closed ? "closed" : ""}`}>
              <div className="studio-group-head">
                <button type="button" className="studio-group-toggle" aria-expanded={!closed} onClick={() => fold(kind)}>
                  <Icon name="chevron" />
                  <Icon name={KIND_ICON[kind]} />
                  <span>{kindLabel(kind)}</span>
                  <small>{offKinds.includes(kind) ? tr("Off") : items.length}</small>
                </button>
                <button type="button" className="icon-button" disabled={busy} aria-label={tr("New {0}", [kindLabel(kind)])} data-tip={tr("New")} onClick={() => void create(kind)}>
                  <Icon name="plus" />
                </button>
              </div>
              {!closed && (
              <List variant="plain" density="compact" className="side-rows studio-rows">
              {items.map((one) => (
                <ListRow
                  key={one.id}
                  className={`studio-row ${one.enabled ? "" : "off"}`}
                  selected={open?.kind === one.kind && open.id === one.id}
                  onClick={() => void pick(one)}
                  title={one.id}
                  subtitle={!one.enabled ? tr("Turned off") : undefined}
                  meta={
                    <span className="studio-row-meta">
                      {one.error ? (
                        <span className="studio-chip bad" data-tip={one.error}>
                          <Icon name="alert" />
                        </span>
                      ) : !one.seen && running ? (
                        <span className="studio-chip" data-tip={tr("Waiting for the add-on to read it")}>
                          <Icon name="clock" />
                        </span>
                      ) : null}
                      <span className="studio-row-time">{relativeTime((one.modified ?? 0) / 1000)}</span>
                    </span>
                  }
                />
              ))}
              </List>
              )}
            </div>
            );
          })}
          </div>
          {running && (
            <div className={`studio-live ${liveOpen ? "" : "closed"}`}>
              <div className="studio-group-head">
                <button type="button" className="studio-group-toggle" aria-expanded={liveOpen} onClick={() => setLiveOpen((now) => !now)}>
                  <Icon name="chevron" />
                  <Icon name="activity" />
                  <span>{tr("Live activity")}</span>
                  <small>{listening ? tr("Listening") : tr("Not connected")}</small>
                </button>
              </div>
              {liveOpen && (heard.length === 0 ? (
                <p className="studio-live-empty">{listening ? tr("Joins and rule runs show up here.") : tr("Waiting for the add-on's live line…")}</p>
              ) : (
                heard.slice(0, 12).map((one) => (
                  <div key={one.key} className="studio-live-row" data-tip={new Date(one.at).toLocaleTimeString()}>
                    <Icon name={one.type === "trace" ? "wand" : "user"} />
                    <span>
                      {one.type === "trace" && <strong>{one.rule}</strong>}
                      {one.type === "trace" && !one.passed && <em>{tr("otherwise")}</em>}
                      {one.text}
                    </span>
                  </div>
                ))
              ))}
            </div>
          )}
        </div>
        <Splitter pane={listPane} side="left" label={tr("Resize the file list")} />
        <div className="git-main">
          {open ? (
            <>
              <div className="git-main-head studio-head">
                <span className="studio-mark" aria-hidden>
                  <Icon name={KIND_ICON[open.kind] ?? "file"} />
                </span>
                <span className="studio-head-title">
                  <strong>{open.id}</strong>
                  <small>
                    {kindLabel(open.kind)} · {fileOf(open.kind, open.id)}
                  </small>
                </span>
                {state && <span className={`studio-state ${state.tone}`}>{state.label}</span>}
                {dirty && <span className="studio-state wait studio-unsaved">{tr("Not saved")}</span>}
                <EditingChip key={`${open.kind}/${open.id}`} serverId={serverId} path={`studio/${open.kind}/${open.id}.json`} />
                <span className="title-actions">
                  <DropdownMenu
                    trigger={
                      <button type="button" className="icon-button" aria-label={tr("More")} data-tip={tr("More")}>
                        <Icon name="dots" />
                      </button>
                    }
                    items={[
                      { label: tr("Reload from the server"), icon: "refresh", onSelect: () => void reload() },
                      ...(openHistory ? [{ label: tr("Change history"), icon: "history" as const, onSelect: () => void showHistory() }] : []),
                      { label: tr("Add-on settings"), icon: "puzzle", onSelect: openAddons },
                      { label: tr("Move to trash"), icon: "trash", danger: true, onSelect: () => void remove() },
                    ]}
                  />
                  <button type="button" className="button primary small" disabled={!dirty || busy} onClick={() => void save()}>
                    {tr("Save")}
                  </button>
                </span>
              </div>
              <div className="studio-views">
                <div className={`wb-seg slide ${views.length === 3 ? "three" : ""}`} role="tablist" aria-label={tr("View")} style={{ "--at": Math.max(0, views.findIndex(([value]) => value === shownMode)) } as CSSProperties}>
                  {views.map(([value, label]) => (
                    <button key={value} type="button" role="tab" aria-selected={shownMode === value} className={shownMode === value ? "on" : ""} onClick={() => setMode(value)}>
                      {label}
                    </button>
                  ))}
                </div>
              </div>
              {(problem || current?.error) && (
                <div className="studio-alert" role="alert">
                  <Icon name="alert" />
                  <span>{problem || tr("The add-on couldn't use this file: {0}", [current?.error ?? ""])}</span>
                </div>
              )}
              {ruleRuns.length > 0 && (
                <div className="studio-runs">
                  <Icon name="activity" />
                  <span>
                    {tr("{0, plural, one {Ran # time} other {Ran # times}} while you watched", [ruleRuns.length])}
                    {" · "}
                    {ruleRuns[0].passed ? tr("last run passed its conditions") : tr("last run went to Otherwise")}
                    {ruleRuns[0].text && ` (${ruleRuns[0].text})`}
                  </span>
                </div>
              )}
              {open.kind === "rule" && !onWeb && parsed && <StudioRuleTest serverId={serverId} rule={open.id} data={parsed} dirty={dirty} />}
              <StudioExpansions.Provider value={expansions}>
              <StudioSideSlot.Provider value={shownMode !== "nodes" && hasPreview(open.kind) ? sideSlot : null}>
              <div className={`studio-body ${shownMode === "nodes" || !hasPreview(open.kind) ? "nodes" : "with-split"}`} style={shownMode === "nodes" ? undefined : widthVar("--preview-w", previewPane.width)}>
                {shownMode === "nodes" && parsed ? (
                  <StudioGraph kind={open.kind} data={parsed} names={names} onChange={(next) => edit(`${JSON.stringify(next, null, 2)}\n`)} />
                ) : shownMode === "form" && parsed ? (
                  <StudioFormToc key={`${open.kind}/${open.id}`}>
                    <StudioFields kind={open.kind} id={open.id} data={parsed} names={names} serverId={serverId} art={art} onChange={(next) => edit(`${JSON.stringify(next, null, 2)}\n`)} />
                  </StudioFormToc>
                ) : (
                  <div className="studio-editor">
                    {shownMode !== "text" && <p className="studio-form-broken">{tr("The JSON has a mistake, so the fields can't show. Fix it here first.")}</p>}
                    <Suspense fallback={null}>
                      <CodeEditor
                        key={`${open.kind}/${open.id}/${open.hash}/${shownMode}`}
                        path={`${open.id}.json`}
                        value={open.edited ?? open.text}
                        readOnly={false}
                        onChange={edit}
                        onSave={() => void save()}
                        onCursor={() => undefined}
                      />
                    </Suspense>
                  </div>
                )}
                {shownMode !== "nodes" && hasPreview(open.kind) && <Splitter pane={previewPane} side="right" label={tr("Resize the preview")} className="studio-preview-splitter" />}
                {shownMode !== "nodes" && hasPreview(open.kind) && (
                  // The right column: the 3D view from the form (when it has one) above the game preview.
                  <div className="studio-side">
                    <div ref={setSideSlot} className="studio-side-3d" />
                    {open.kind === "model" ? (
                      <p className="muted studio-preview-none studio-side-hint">{tr("Pick a Blockbench model to turn it in 3D here.")}</p>
                    ) : (
                      <StudioPreview kind={open.kind} data={parsed} art={art} serverId={serverId} />
                    )}
                  </div>
                )}
              </div>
              </StudioSideSlot.Provider>
              </StudioExpansions.Provider>
            </>
          ) : (
            overview.resources.length > 0 ? (
            // Home with files: everything already made, by kind, ready to open. New things start from New.
            <div className="studio-start studio-home">
              <div className="studio-start-head">
                <div>
                  <h2>{tr("Your files")}</h2>
                  <p className="muted">{tr("Pick one to edit it. New ones start from New at the top.")}</p>
                </div>
                <SearchField value={startQuery} onChange={setStartQuery} placeholder={tr("Find a file")} />
              </div>
              <div className="studio-home-grid">
                {homeGroups.flatMap(({ kind, items }) =>
                  items.map((one) => (
                    <button key={`${kind}/${one.id}`} type="button" className={`studio-home-card ${one.enabled ? "" : "off"}`} onClick={() => void pick(one)}>
                      <span className="studio-mark" aria-hidden>
                        <Icon name={KIND_ICON[kind] ?? "file"} />
                      </span>
                      <span className="studio-home-text">
                        <strong>{one.id}</strong>
                        <small>
                          {kindLabel(kind)} · {relativeTime((one.modified ?? 0) / 1000)}
                          {(!one.enabled || offKinds.includes(kind)) && ` · ${tr("Turned off")}`}
                        </small>
                      </span>
                      {one.error ? (
                        <span className="studio-chip bad" data-tip={one.error}>
                          <Icon name="alert" />
                        </span>
                      ) : !one.seen && running ? (
                        <span className="studio-chip" data-tip={tr("Waiting for the add-on to read it")}>
                          <Icon name="clock" />
                        </span>
                      ) : null}
                    </button>
                  )),
                )}
              </div>
              {homeGroups.length === 0 && <p className="muted">{tr("No matching files.")}</p>}
            </div>
            ) : (
            <div className="studio-start">
              <div className="studio-start-head">
                <div>
                  <h2>{overview.resources.length ? tr("Pick a file, or start a new one") : tr("What do you want to change in the game?")}</h2>
                  <p className="muted">{tr("Each item is one JSON file in {0}. Edit it with fields and see a preview before you save.", [overview.root])}</p>
                </div>
                <SearchField value={startQuery} onChange={setStartQuery} placeholder={tr("Find a kind")} />
              </div>
              {startGroups.map((group) => (
                <section key={group.id} className="studio-start-group">
                  <h3>{groupLabel(group.id)}</h3>
                  <div className="studio-start-grid">
                    {group.kinds.map((kind) => (
                      <button key={kind} type="button" className="studio-start-card" disabled={busy} onClick={() => void create(kind as StudioKind)}>
                        <span className="studio-mark" aria-hidden>
                          <Icon name={KIND_ICON[kind]} />
                        </span>
                        <span className="studio-start-text">
                          <strong>
                            {kindLabel(kind)}
                            {(counts.get(kind) ?? 0) > 0 && <b>{counts.get(kind)}</b>}
                          </strong>
                          <small>{kindHint(kind)}</small>
                        </span>
                      </button>
                    ))}
                  </div>
                </section>
              ))}
              {startGroups.length === 0 && <p className="muted">{tr("No kind matches.")}</p>}
            </div>
            )
          )}
        </div>
      </div>
      )}
    </div>
  );
}

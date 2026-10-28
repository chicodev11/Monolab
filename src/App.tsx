import { keepSame } from "./lib/same";
import { Fragment, Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from "react";
import appIcon from "./assets/app-icon.png";
import { commands } from "./lib/ipc";
import { panelKindName, panelsName } from "./lib/panelKind";
import { setServerNames, startJobEvents, useJobs } from "./lib/jobs";
import type {
  PanelHostView,
  SshHostView,
  InstanceView as GenInstance,
  WebPanelView as GenWebPanelView,
  WebAccountView as GenWebAccount,
  CoreBuild as GenCoreBuild,
  AddonStatus as GenAddonStatus,
  LiveState_Serialize as GenLiveState,
  LoadedChunks,
  RunEntry,
  ServerLogEvent as GenLog,
  ParsedLogEntry as GenParsedLog,
  PropertyInfo as GenProperty,
  LogFile as GenLogFile,
  TimelineEvent as GenTimelineEvent,
  DailySummary as GenDailySummary,
  Bookmark as GenBookmark,
  ResourceSnapshot as GenResource,
  TpsSnapshot as GenTps,
  PingSnapshot as GenPing,
  PlayerEntry as GenPlayer,
  PlayerActivityEntry as GenPlayerActivity,
  ResourceSample as GenResourceSample,
  AddonReport as GenAddonReport,
  AddonUpdate as GenAddonUpdate,
  AddonRollbackEntry as GenAddonRollback,
  InstallPlan as GenInstallPlan,
  PlayerSpot as GenPlayerSpot,
  PlayerSnapshot as GenPlayerSnapshot,
  GameEvent_Serialize as GenGameEvent,
  WorldSettings as GenWorldSettings,
  CrashReport as GenCrashReport,
  AppSettings as GenAppSettings,
  CoreBuildNews as GenCoreBuildNews,
  JavaRuntime as GenJavaRuntime,
  Task as GenTask,
  Status as GenPlayitStatus,
  Waiting as GenWaitingAddon,
  AddonAddResult as GenAddonAddResult,
  JavaProcess as GenJavaProcess,
  Pack as GenCfPack,
  CatalogHit as GenCatalogHit,
  ProjectDetail as GenProjectDetail,
  GalleryItem as GenGalleryItem,
  TrashFile as GenTrashFile,
  AddonVersion as GenAddonVersion,
  AddonEntry as GenAddon,
  AutomationHistoryEntry as GenAutomationHistory,
  UpgradeHistoryEntry as GenUpgradeHistory,
  BrowseEntry as GenBrowseEntry,
  WorldEntry as GenWorld,
  MrpackFileEntry as GenMrpackEntry,
  BackupFile as GenBackupFile,
  Finding as GenFinding,
  AuditEntry as GenAuditEntry,
  TrashEntry as GenTrashEntry,
  CommandBook as GenCommandBook,
  LagDiagnosis,
  LagFinding,
  CustomProgram,
} from "./lib/bindings";
import { lagLine, tpCommand, type LagAction } from "./lib/lag";
import { enchantLevel, maxDurability } from "./lib/items";
const SPARK_MODRINTH_ID = "l6YH9Als";
import type { Sent } from "./lib/sent";
import { listenTo, onWeb, webSession } from "./lib/bridge";
import { ContextMenu, type ContextItem } from "./components/ui/ContextMenu";
import { WebLogin } from "./components/dialogs/WebLogin";
import { offerDownload, pickFiles, pickRawFiles, uploadRaw } from "./lib/pickFile";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";

const CHANGELOG_URL = "https://mnlab.cc/changelog";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { Icon } from "./components/ui/Icon";
import { CoreIcon, coreInfo } from "./components/ui/CoreIcon";
import { Dropdown } from "./components/ui/Dropdown";
import { GameFacts, GamePicker } from "./components/create/GamePicker";
import { CustomCreate, CustomProgramCard } from "./components/create/CustomProgram";
import { GameFilter } from "./components/servers/GameFilter";
import { PlayerPeek } from "./components/servers/PlayerPeek";
import { AskDialog, type AskRequest } from "./components/dialogs/AskDialog";
import { Sparkline } from "./components/ui/Sparkline";
import { RemoteImport } from "./components/panels/RemoteImport";
import { PanelHosts } from "./components/panels/PanelHosts";
import type { PanelView } from "./components/panels/PanelServerTools";
import { TasksPage } from "./components/panels/TasksPage";
import { PanelStartupCard } from "./components/panels/PanelStartupCard";
import { GameLaunchCard } from "./components/panels/GameLaunchCard";
import { addonKind, coreLabel, customProgram, findGame, gameOf, has, isCustom, isGame, kindOf, minecraftCores, portsOf, type Protocol } from "./lib/kinds";
import { SshHosts } from "./components/panels/SshHosts";
import { CommandPalette } from "./components/dialogs/CommandPalette";
import { MarksPanel } from "./components/panels/MarksPanel";
import { CrashPanel } from "./components/panels/CrashPanel";
import { ErrorBoundary } from "./components/ui/ErrorBoundary";
import { TitleBar } from "./components/panels/TitleBar";
import { Tooltip } from "./components/ui/Tooltip";
import { type MapPin } from "./components/map/pins";
const WorldMap = lazy(() =>
  import("./components/map/WorldMap").then((module) => ({ default: module.WorldMap })),
);
import { DragScroll } from "./components/ui/DragScroll";
import { DependencyDialog } from "./components/dialogs/DependencyDialog";
import { VersionDialog, type AddonVersion } from "./components/dialogs/VersionDialog";
import { CompareDialog } from "./components/dialogs/CompareDialog";
import type { MapArea } from "./components/map/WorldMap";
import { WorldSide } from "./components/map/WorldSide";
import { MapShareDialog } from "./components/dialogs/MapShareDialog";
import { DatapackPanel } from "./components/panels/DatapackPanel";
import { TrimDialog } from "./components/dialogs/TrimDialog";
import { DiskDialog } from "./components/dialogs/DiskDialog";
import { PregenDialog } from "./components/dialogs/PregenDialog";
import { UpdateDialog, type UpdateChoice, type UpdateRow } from "./components/dialogs/UpdateDialog";
import { format, language, languageCodes, languageNames, setLanguage, t, tr, trBack, wordsFor } from "./lib/i18n";
import { logStore } from "./lib/logStore";
import { period, useAttention } from "./lib/polling";
import { CATEGORIES, propertyMatches, propertyMeta, propertyOrder, type Category } from "./lib/properties";
import { Badge, Banner, Button, Card, DropdownMenu, EmptyState, IconButton, SegmentedControl } from "./components/primitives";
import { QrCode } from "./components/panels/QrCode";
import { ConsoleLevels, ConsoleOutput, LogLines, type ConsoleLevel } from "./components/console/ConsoleOutput";
import { ServerStats } from "./components/console/ServerStats";
import { FileRows } from "./components/files/FileRows";
import { TextureView } from "./components/files/TextureView";
import type { Cursor } from "./components/files/CodeEditor";
import { languageName } from "./lib/fileLang";
const CodeEditor = lazy(() => import("./components/files/CodeEditor").then((module) => ({ default: module.CodeEditor })));
const LangEditor = import.meta.env.DEV
  ? lazy(() => import("./components/dev/LangEditor").then((module) => ({ default: module.LangEditor })))
  : null;
const PanelAdmin = lazy(() => import("./components/panels/PanelAdmin").then((module) => ({ default: module.PanelAdmin })));
const PanelServerTools = lazy(() => import("./components/panels/PanelServerTools").then((module) => ({ default: module.PanelServerTools })));
const SftpPage = lazy(() => import("./components/panels/SftpPage").then((module) => ({ default: module.SftpPage })));
const AgentPage = lazy(() => import("./components/panels/AgentPage").then((module) => ({ default: module.AgentPage })));
import { applyCompact, applyTheme, followSystem, readCompact, readTheme, type Theme } from "./lib/theme";
import {
  bytes,
  used,
  date,
  fileSize,
  roughSize,
  isImage,
  isProxy,
  realVersion,
  relativeTime,
  shownStatus,
  statusInfo,
} from "./lib/format";
import "./App.css";
import "./polish.css";

type Instance = Sent<GenInstance>;
type WebPanelView = Sent<GenWebPanelView>;
type WebAccount = Sent<GenWebAccount>;
type CoreBuild = Sent<GenCoreBuild>;
type AddonStatus = Sent<GenAddonStatus>;
type LiveState = GenLiveState;
type Log = Sent<GenLog>;
type ParsedLog = Sent<GenParsedLog>;
type Property = Sent<GenProperty>;
type LogFile = Sent<GenLogFile>;
type TimelineEvent = Sent<GenTimelineEvent>;
type DailySummary = Sent<GenDailySummary>;
type Bookmark = Sent<GenBookmark>;
type Resource = Sent<GenResource>;
type Tps = Sent<GenTps>;
type Ping = Sent<GenPing>;
type Player = Sent<GenPlayer>;
type PlayerActivity = Sent<GenPlayerActivity>;
type ResourceSample = Sent<GenResourceSample>;
type AddonReport = Sent<GenAddonReport>;
type AddonUpdate = Sent<GenAddonUpdate>;
type AddonRollback = Sent<GenAddonRollback>;
type InstallPlan = Sent<GenInstallPlan>;
type PlayerSpot = Sent<GenPlayerSpot>;
type PlayerSnapshot = Sent<GenPlayerSnapshot>;
type WorldSettings = Sent<GenWorldSettings>;
type CrashReport = Sent<GenCrashReport>;
type AppSettings = Sent<GenAppSettings>;
type CoreBuildNews = Sent<GenCoreBuildNews>;
type JavaRuntime = Sent<GenJavaRuntime>;
type Task = Sent<GenTask>;
type PlayitStatus = Sent<GenPlayitStatus>;
type WaitingAddon = Sent<GenWaitingAddon>;
type AddonAddResult = Sent<GenAddonAddResult>;
type JavaProcess = Sent<GenJavaProcess>;
type CfPack = Sent<GenCfPack>;
type CatalogHit = Sent<GenCatalogHit>;
type ProjectDetail = Sent<GenProjectDetail>;
type ProjectShot = Sent<GenGalleryItem>;
type TrashFile = Sent<GenTrashFile>;
type ProjectVersion = Sent<GenAddonVersion>;
type Addon = Sent<GenAddon>;
type AutomationHistory = Sent<GenAutomationHistory>;
type UpgradeHistory = Sent<GenUpgradeHistory>;
type BrowseEntry = Sent<GenBrowseEntry>;
type World = Sent<GenWorld>;
type MrpackEntry = Sent<GenMrpackEntry>;
type GameEvent = GenGameEvent;
type BackupFile = Sent<GenBackupFile>;
type Finding = Sent<GenFinding>;
type AuditEntry = Sent<GenAuditEntry>;
type TrashEntry = Sent<GenTrashEntry>;
type CommandBook = Sent<GenCommandBook>;

type Modal =
  | "none"
  | "import"
  | "create"
  | "settings"
  | "tasks"
  | "web"
  | "java"
  | "precheck"
  | "mrpack"
  | "hydro"
  | "sftp"
  | "trash";
const modalShell: Record<Modal, string> = {
  none: "",
  import: "narrow",
  mrpack: "mid",
  precheck: "narrow",
  create: "as-panel",
  java: "as-panel",
  tasks: "as-panel",
  web: "as-panel",
  settings: "as-panel",
  hydro: "as-panel",
  sftp: "as-panel",
  trash: "as-panel",
};
type Tool =
  | "overview"
  | "console"
  | "properties"
  | "files"
  | "worlds"
  | "logs"
  | "crashes"
  | "backups"
  | "monitor"
  | "players"
  | "addons"
  | "claude"
  | "reach"
  | PanelView;
type PlayerAction = "whitelist" | "operator" | "ban" | "kick";
function sayWhatHappened(result: AddonAddResult) {
  const said: string[] = [];
  if (result.replaced.length) said.push(tr("Updated {0} to newer versions", [result.replaced.length]));
  const fresh = result.added.length - result.replaced.length;
  if (fresh > 0) said.push(tr("Added {0} new items", [fresh]));
  if (result.queued.length)
    said.push(tr("{0} downloaded. Will apply when the server restarts", [result.queued.length]));
  return said.length ? `${said.join(", ")}.` : tr("There was nothing to add.");
}

function packFitsServer(
  pack: { gameVersion: string; loader: string },
  server: { version: string; core: string },
) {
  const sameVersion =
    !pack.gameVersion || pack.gameVersion === server.version;
  const core = server.core.toLowerCase();
  const loader = pack.loader.toLowerCase();
  const sameLoader = !loader || core.includes(loader);
  return sameVersion && sameLoader;
}

function orDash(value: number | null | undefined, format: (value: number) => string) {
  return value == null ? "—" : format(value);
}

function lanHost(address: string) {
  return address.replace(/^https?:\/\//, "").replace(/:\d+\/?$/, "") || "?";
}

const CATALOG_CATEGORIES: { id: string; ko: string; icon: string }[] = [
  { id: "game-mechanics", ko: "Game mechanics", icon: "cog" },
  { id: "economy", ko: "Economy", icon: "coins" },
  { id: "management", ko: "Manage", icon: "shield-check" },
  { id: "technology", ko: "Technology", icon: "cpu" },
  { id: "utility", ko: "Utility", icon: "wrench" },
  { id: "library", ko: "Library", icon: "library" },
  { id: "magic", ko: "Magic", icon: "wand" },
  { id: "adventure", ko: "Adventure", icon: "compass" },
  { id: "mobs", ko: "Mobs", icon: "skull" },
  { id: "minigame", ko: "Minigame", icon: "gamepad" },
  { id: "storage", ko: "Storage", icon: "package" },
  { id: "worldgen", ko: "World generation", icon: "mountain" },
  { id: "social", ko: "Social", icon: "message" },
  { id: "transportation", ko: "Transportation", icon: "truck" },
  { id: "food", ko: "Food", icon: "utensils" },
  { id: "equipment", ko: "Equipment", icon: "sword" },
  { id: "decoration", ko: "Decoration", icon: "sparkles" },
  { id: "optimization", ko: "Optimization", icon: "gauge" },
  { id: "cursed", ko: "Cursed", icon: "ghost" },
];
const LOADER_CORE: Record<string, string> = {
  paper: "Paper", folia: "Folia", purpur: "Purpur", leaf: "Leaf", fabric: "Fabric", forge: "Forge", neoforge: "NeoForge",
  velocity: "Velocity", bungeecord: "BungeeCord", vanilla: "Vanilla",
};
function loaderChip(loader: string) {
  const core = LOADER_CORE[loader];
  return (
    <b key={loader} data-loader={loader}>
      {core && <CoreIcon core={core} />}
      {loader}
    </b>
  );
}
function isLoaderName(name: string) {
  return ["paper", "folia", "purpur", "spigot", "bukkit", "fabric", "forge", "neoforge", "quilt", "velocity", "bungeecord", "waterfall", "sponge", "datapack", "rift", "liteloader", "modloader", "canvas", "iris", "optifine", "vanilla", "minecraft"].includes(name);
}
function sortLoaders(loaders: string[], core: string) {
  const mine = core.toLowerCase();
  return [...loaders].sort((a, b) => Number(b === mine) - Number(a === mine));
}
function hasWorlds(core: string) {
  return !["Pumpkin", "Velocity", "BungeeCord", "Waterfall"].includes(core);
}
function compactCount(value: number) {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(value >= 10_000_000 ? 0 : 1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(value >= 10_000 ? 0 : 1)}k`;
  return String(value);
}
function versionRank(version: string) {
  return version.split(/[.\-+ ]/).map((part) => Number.parseInt(part, 10) || 0);
}
function compareVersions(a: string, b: string) {
  const x = versionRank(a), y = versionRank(b);
  for (let at = 0; at < Math.max(x.length, y.length); at += 1) {
    const d = (x[at] ?? 0) - (y[at] ?? 0);
    if (d) return d;
  }
  return 0;
}
function releaseVersions(list: string[]) {
  const only = list.filter((one) => /^[\d.]+$/.test(one));
  return only.length ? only : list;
}
function versionSpan(list: string[]) {
  if (!list.length) return "";
  const sorted = [...releaseVersions(list)].sort(compareVersions);
  const low = sorted[0], high = sorted[sorted.length - 1];
  return low === high ? low : `${low} – ${high}`;
}

const REMOTE_TOOLS: string[] = [
  "overview",
  "claude",
  "console",
  "properties",
  "files",
  "worlds",
  "addons",
  "players",
  "backups",
  "logs",
  "monitor",
  "databases",
  "network",
  "subusers",
  "startup",
  "schedules",
  "psettings",
  "activity",
  "software",
];
const PANEL_VIEWS: string[] = ["databases", "network", "subusers", "startup", "schedules", "psettings", "activity", "software"];
function sideLabel(detail: ProjectDetail): { text: string; icon: string } | null {
  const client = detail.clientSide;
  const server = detail.serverSide;
  const known = (one: string) => one === "required" || one === "optional" || one === "unsupported";
  if (!known(client) && !known(server)) return null;
  if (server === "unsupported") return { text: tr("Client only"), icon: "alert" };
  if (client === "unsupported") return { text: tr("Server only"), icon: "check" };
  return { text: tr("Server and client"), icon: "plug" };
}

type NavPoint = {
  page: "home" | "instance";
  instanceId: string | null;
  tool: Tool;
  browsePath?: string;
  activeFile?: string | null;
  browse?: boolean;
  project?: CatalogHit | null;
  panel?: Modal;
};
function samePlace(a: NavPoint, b: NavPoint) {
  return (
    a.page === b.page &&
    a.instanceId === b.instanceId &&
    a.tool === b.tool &&
    (a.browsePath ?? "") === (b.browsePath ?? "") &&
    (a.activeFile ?? null) === (b.activeFile ?? null) &&
    !!a.browse === !!b.browse &&
    (a.project?.projectId ?? null) === (b.project?.projectId ?? null)
  );
}
type DownloadProgress = {
  stage: string;
  core: string;
  version: string;
  received?: number;
  total?: number;
};
type ArchiveProgress = { stage: string; done?: number; total?: number };
type SshJob = { kind: string; name: string; stage: string; sent: number; total: number };

const cores = minecraftCores;

const isNative = (core: string) => !has(core, "java");
const toolAllowed = (core: string, tool: Tool) => {
  if (tool === "properties") return has(core, "settings");
  if (tool === "worlds") return has(core, "worlds");
  if (tool === "players") return has(core, "players");
  if (tool === "addons") return kindOf(core).addons !== null;
  return true;
};

function isClientOnly(
  one: { compatibility: string },
  card?: { compatibility: string } | null,
): boolean {
  return (card?.compatibility ?? one.compatibility) === "Client only";
}

const gameRuleLabels: Record<string, { name: string; hint: string }> = {
  doDaylightCycle: { name: tr("Daylight cycle"), hint: tr("When off, time stays frozen.") },
  doWeatherCycle: { name: tr("Weather cycle"), hint: tr("When off, rain and thunder don't start automatically.") },
  keepInventory: { name: tr("Keep inventory"), hint: tr("When on, you keep your inventory on death.") },
  doFireTick: { name: tr("Fire spreads"), hint: tr("When off, fire does not spread.") },
  mobGriefing: { name: tr("Mob griefing"), hint: tr("Things like creeper explosions and endermen moving blocks.") },
  doMobSpawning: { name: tr("Natural mob spawning"), hint: tr("When off, natural spawning stops. Spawners are unaffected.") },
  doMobLoot: { name: tr("Mobs drop loot"), hint: "" },
  doTileDrops: { name: tr("Blocks drop items"), hint: tr("When off, mining drops nothing.") },
  doEntityDrops: { name: tr("Vehicles and item frames drop items"), hint: "" },
  naturalRegeneration: { name: tr("Natural regeneration"), hint: tr("When off, only items like golden apples restore health.") },
  fallDamage: { name: tr("Fall damage"), hint: "" },
  fireDamage: { name: tr("Fire damage"), hint: "" },
  drowningDamage: { name: tr("Drowning damage"), hint: "" },
  freezeDamage: { name: tr("Freeze damage"), hint: "" },
  pvp: { name: tr("PvP"), hint: tr("This option is also available in Server settings.") },
  showDeathMessages: { name: tr("Show death messages"), hint: tr("Death messages appear in chat.") },
  announceAdvancements: { name: tr("Announce advancements"), hint: "" },
  doImmediateRespawn: { name: tr("Immediate respawn"), hint: tr("When on, the death screen is skipped.") },
  spawnRadius: { name: tr("Spawn radius"), hint: tr("The radius around spawn where players can appear, in blocks.") },
  randomTickSpeed: { name: tr("Random tick speed"), hint: tr("The default is 3. Higher values increase server load.") },
  maxEntityCramming: { name: tr("Max entity cramming"), hint: tr("Exceeding this number of entities in one spot causes cramming damage. 0 disables it.") },
  doInsomnia: { name: tr("Phantoms spawn if you don't sleep"), hint: "" },
  doPatrolSpawning: { name: tr("Pillager patrols spawn"), hint: "" },
  doTraderSpawning: { name: tr("Wandering trader spawns"), hint: "" },
  doWardenSpawning: { name: tr("Wardens spawn"), hint: "" },
  disableRaids: { name: tr("Disable raids"), hint: tr("When on, village raids are disabled.") },
  playersSleepingPercentage: { name: tr("Players needed to skip the night"), hint: tr("A percentage. At 0, only one player needs to sleep to skip the night.") },
  sendCommandFeedback: { name: tr("Show command output in chat"), hint: "" },
  commandBlockOutput: { name: tr("Command block output to ops"), hint: "" },
  logAdminCommands: { name: tr("Log admin commands"), hint: "" },
  reducedDebugInfo: { name: tr("Reduce F3 debug info"), hint: tr("When on, hides details such as coordinates.") },
  doLimitedCrafting: { name: tr("Limited crafting"), hint: "" },
  spectatorsGenerateChunks: { name: tr("Spectators generate chunks"), hint: tr("When off, spectators don't generate new chunks.") },
  universalAnger: { name: tr("Universal anger"), hint: tr("Mobs attack everyone nearby, not just the attacker.") },
  forgiveDeadPlayers: { name: tr("Forgive dead players"), hint: "" },
  enderPearlsVanishOnDeath: { name: tr("Ender pearls vanish on death"), hint: "" },
  projectilesCanBreakBlocks: { name: tr("Projectiles break blocks"), hint: "" },
  tntExplodes: { name: tr("TNT explodes"), hint: "" },
  blockExplosionDropDecay: { name: tr("Explosions destroy block drops"), hint: tr("When on, only some blocks drop as items.") },
  mobExplosionDropDecay: { name: tr("Mob explosions destroy block drops"), hint: "" },
  tntExplosionDropDecay: { name: tr("TNT destroys block drops"), hint: "" },
  waterSourceConversion: { name: tr("Infinite water"), hint: "" },
  lavaSourceConversion: { name: tr("Infinite lava"), hint: "" },
  snowAccumulationHeight: { name: tr("Snow accumulation height"), hint: "" },
  globalSoundEvents: { name: tr("Global sounds"), hint: tr("Plays global sounds, like when the Ender Dragon dies, to the whole server.") },
  doVinesSpread: { name: tr("Vines spread"), hint: "" },
  maxCommandChainLength: { name: tr("Max command chain length"), hint: "" },
  spawnChunkRadius: { name: tr("Spawn chunk radius"), hint: tr("Higher values increase server load.") },
  minecartMaxSpeed: { name: tr("Max minecart speed"), hint: "" },
  locatorBar: { name: tr("Location bar"), hint: "" },
};

function ruleKey(key: string): string {
  if (!key.includes(":")) return key;
  const bare = key.slice(key.indexOf(":") + 1);
  const renamed: Record<string, string> = {
    advance_time: "doDaylightCycle",
    advance_weather: "doWeatherCycle",
    block_drops: "doTileDrops",
    entity_drops: "doMobLoot",
    mob_griefing: "mobGriefing",
  };
  return renamed[bare] ?? bare.replace(/_([a-z0-9])/g, (_, letter: string) => letter.toUpperCase());
}
function gameRuleName(key: string): string {
  return gameRuleLabels[ruleKey(key)]?.name ?? key;
}

function ruleMatches(key: string, query: string): boolean {
  const want = query.trim().toLowerCase();
  if (!want) return true;
  const known = gameRuleLabels[ruleKey(key)];
  return `${key} ${known?.name ?? ""} ${known?.hint ?? ""}`
    .toLowerCase()
    .includes(want);
}


const SLASH_TAIL = /[\\/][^\\/]*$/;


function realmFolder(dimension: string): string {
  if (dimension.endsWith("the_nether")) return "w_nether";
  if (dimension.endsWith("the_end")) return "w_the_end";
  return "w";
}

function inRealm(world: string | null | undefined, realm: string): boolean {
  const name = world ?? "";
  if (name.endsWith("_nether")) return realm === "nether";
  if (name.endsWith("_the_end")) return realm === "end";
  if (name.includes(":")) return realm === name.toLowerCase().replace(/[:/]/g, ".");
  return realm === "overworld";
}

const instanceNav: {
  id: Tool;
  label: string;
  icon: string;
}[] = [
  { id: "overview", label: tr("Overview"), icon: "server" },
  { id: "claude", label: tr("Agent"), icon: "sparkles" },
  { id: "properties", label: tr("Server settings"), icon: "settings" },
  { id: "files", label: tr("Files"), icon: "folder" },
  { id: "worlds", label: tr("Worlds"), icon: "grid" },
  { id: "addons", label: tr("Mods & plugins"), icon: "copy" },
  { id: "players", label: tr("Players"), icon: "list" },
  { id: "backups", label: tr("Backups"), icon: "archive" },
  { id: "logs", label: tr("Logs"), icon: "console" },
  { id: "monitor", label: tr("Monitor"), icon: "cpu" },
  { id: "reach", label: tr("Network"), icon: "network" },
];
const panelNav: { id: Tool; label: string; icon: string; section?: string }[] = [
  { id: "overview", label: tr("Console"), icon: "terminal" },
  { id: "claude", label: tr("Agent"), icon: "sparkles" },
  { id: "files", label: tr("Files"), icon: "folder" },
  { id: "addons", label: tr("Mods & plugins"), icon: "import" },
  { id: "databases", label: tr("Databases"), icon: "database" },
  { id: "backups", label: tr("Backups"), icon: "cloud-up" },
  { id: "network", label: tr("Network"), icon: "network" },
  { id: "subusers", label: tr("User"), icon: "users" },
  { id: "startup", label: tr("Startup"), icon: "sliders" },
  { id: "schedules", label: tr("Schedules"), icon: "schedule" },
  { id: "psettings", label: tr("Settings"), icon: "settings" },
  { id: "activity", label: tr("Activity"), icon: "activity" },
  { id: "software", label: tr("Software"), icon: "gamepad" },
  { id: "properties", label: tr("Server settings"), icon: "cog", section: tr("Minecraft") },
  { id: "worlds", label: tr("Worlds"), icon: "grid" },
  { id: "players", label: tr("Players"), icon: "list" },
  { id: "logs", label: tr("Logs"), icon: "console" },
  { id: "monitor", label: tr("Monitor"), icon: "cpu" },
];
const fallbackVersions = [
  "1.21.4",
  "1.21.3",
  "1.21.1",
  "1.20.6",
  "1.20.4",
  "1.20.2",
  "1.19.4",
  "1.18.2",
];

function wantsPlayerName(name: string | null | undefined): boolean {
  return /^(targets?|players?)$/.test(name ?? "");
}

const consoleCommands: { name: string; args?: "player"; hint: string }[] = [
  { name: "help", hint: tr("Command list") },
  { name: "list", hint: tr("Show players") },
  { name: "say", hint: tr("Announce to everyone") },
  { name: "tell", args: "player", hint: tr("Whisper") },
  { name: "tp", args: "player", hint: tr("Teleport") },
  { name: "gamemode survival", args: "player", hint: tr("to survival") },
  { name: "gamemode creative", args: "player", hint: tr("to creative") },
  { name: "gamemode spectator", args: "player", hint: tr("to spectator") },
  { name: "give", args: "player", hint: tr("Give item") },
  { name: "kick", args: "player", hint: tr("Export") },
  { name: "ban", args: "player", hint: tr("Ban") },
  { name: "pardon", hint: tr("Unban") },
  { name: "op", args: "player", hint: tr("as operator") },
  { name: "deop", args: "player", hint: tr("Remove operator") },
  { name: "whitelist add", hint: tr("Add to whitelist") },
  { name: "whitelist reload", hint: tr("Reload whitelist") },
  { name: "time set day", hint: tr("Set day") },
  { name: "time set night", hint: tr("Set night") },
  { name: "weather clear", hint: tr("Clear weather") },
  { name: "difficulty", hint: tr("Difficulty") },
  { name: "gamerule", hint: tr("Game rules") },
  { name: "seed", hint: tr("Show seed") },
  { name: "save-all", hint: tr("Save now") },
  { name: "stop", hint: tr("Stop server") },
];
const pumpkinCommands: { name: string; args?: "player"; hint: string }[] = [
  { name: "tps", hint: tr("Show TPS") },
  { name: "plugins", hint: tr("List plugins") },
  { name: "plugin reload", hint: tr("Reload plugins") },
  { name: "pumpkin", hint: tr("Server info") },
];

const aikarJvmArgs =
  "-XX:+UseG1GC -XX:+ParallelRefProcEnabled -XX:MaxGCPauseMillis=200 -XX:+UnlockExperimentalVMOptions -XX:+DisableExplicitGC -XX:+AlwaysPreTouch -XX:G1NewSizePercent=30 -XX:G1MaxNewSizePercent=40 -XX:G1HeapRegionSize=8M -XX:G1ReservePercent=20 -XX:G1HeapWastePercent=5 -XX:G1MixedGCCountTarget=4 -XX:InitiatingHeapOccupancyPercent=15 -XX:G1MixedGCLiveThresholdPercent=90 -XX:G1RSetUpdatingPauseTimePercent=5 -XX:SurvivorRatio=32 -XX:+PerfDisableSharedMem -XX:MaxTenuringThreshold=1";
function App() {
  const [instances, setInstances] = useState<Instance[]>([]);
  const jobs = useJobs();
  useEffect(() => {
    startJobEvents();
  }, []);
  useEffect(() => {
    setServerNames((id) => instances.find((one) => one.id === id)?.name ?? "");
  }, [instances]);
  const instancesRef = useRef<Instance[]>([]);
  instancesRef.current = instances;
  useEffect(() => {
    if (onWeb) return;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/window").then(({ getCurrentWindow }) =>
      getCurrentWindow().onCloseRequested(async (event) => {
        const alive = instancesRef.current.filter(
          (instance) =>
            !instance.remoteHost &&
            !instance.panelHost &&
            (instance.status === "running" || instance.status === "stopping"),
        );
        if (alive.length) {
          event.preventDefault();
          setCloseBlockedBy(alive);
          return;
        }
        const panel = await commands.webPanelState().catch(() => null);
        if (!panel?.on) return;
        const ok = await confirmAction(
          tr("The web panel is on"),
          panel.tunnel
            ? tr("Closing the app turns off the web panel and its outside address. People using it, and anyone with a map share link, will lose access.")
            : tr("Closing the app turns off the web panel. People using it will lose access."),
          tr("Close anyway"),
        );
        if (!ok) event.preventDefault();
      }).then((stop) => { unlisten = stop; }),
    );
    return () => unlisten?.();
  }, []);
  const [ask, setAsk] = useState<AskRequest | null>(null);
  const [closeBlockedBy, setCloseBlockedBy] = useState<Instance[]>([]);
  const [startingServer, setStartingServer] = useState<{ id: string; name: string; at: number; done?: boolean } | null>(null);
  const [history, setHistory] = useState<ResourceSample[]>([]);
  const [useRcon, setUseRcon] = useState(false);
  const [playerSnapshot, setPlayerSnapshot] = useState<PlayerSnapshot | null>(
    null,
  );
  const [heads, setHeads] = useState<Record<string, string>>({});
  const [worldSettings, setWorldSettings] = useState<WorldSettings | null>(null);
  const [perfDays, setPerfDays] = useState<string[]>([]);
  const [perfDay, setPerfDay] = useState("");
  const [perfHistory, setPerfHistory] = useState<GameEvent[]>([]);
  const [chartView, setChartView] = useState<"now" | "past">("now");
  const [logLevelView, setLogLevelView] = useState<ConsoleLevel>("all");
  const [lagReport, setLagReport] = useState<LagDiagnosis | null>(null);
  const [sparkStartedAt, setSparkStartedAt] = useState<number | null>(null);
  const [mapStart, setMapStart] = useState<{
    x: number;
    z: number;
    level: number;
  } | null>(null);

  const [liveHeads, setLiveHeads] = useState<Record<string, string>>({});
  const [mapOpen, setMapOpen] = useState(false);
  const [playerSpots, setPlayerSpots] = useState<PlayerSpot[]>([]);
  const [mapRealm, setMapRealm] = useState("overworld");
  const [mapCave, setMapCave] = useState<boolean | null>(null);
  const [mapChunksOn, setMapChunksOn] = useState(false);
  const [loadedChunks, setLoadedChunks] = useState<LoadedChunks | null>(null);
  const [mapBusy, setMapBusy] = useState(false);
  const [updatePlan, setUpdatePlan] = useState<UpdateRow[] | null>(null);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [versionPick, setVersionPick] = useState<{
    addon: Addon;
    projectId: string;
    title: string;
    iconUrl: string | null;
    list: AddonVersion[] | null;
  } | null>(null);
  const [depAsk, setDepAsk] = useState<{
    item: CatalogHit;
    plan: InstallPlan;
    chosen: string[];
  } | null>(null);
  const [dialogBusy, setDialogBusy] = useState(false);
  const [ruleQuery, setRuleQuery] = useState("");
  const [worldDraft, setWorldDraft] = useState<WorldSettings | null>(null);
  const [crashReports, setCrashReports] = useState<CrashReport[]>([]);
  const [openCrash, setOpenCrash] = useState<string | null>(null);
  const [catalogHits, setCatalogHits] = useState<CatalogHit[]>([]);
  const [catalogPage, setCatalogPage] = useState(0);
  const [catalogMore, setCatalogMore] = useState(false);
  const [catalogBusy, setCatalogBusy] = useState(false);
  const [catalogCategories, setCatalogCategories] = useState<string[]>([]);
  const catalogEnvironment = "server";
  const [catalogVersions, setCatalogVersions] = useState<string[]>([]);
  const [browseVersionList, setBrowseVersionList] = useState<string[]>([]);
  const [browseVersionsOpen, setBrowseVersionsOpen] = useState(false);
  const [projectView, setProjectView] = useState<{
    hit: CatalogHit;
    tab: "overview" | "gallery" | "versions" | "changelog";
    detail: ProjectDetail | null;
    versions: ProjectVersion[] | null;
  } | null>(null);
  const [projectShown, setProjectShown] = useState(40);
  const projectFromRef = useRef<"browse" | "installed">("browse");
  const [shotView, setShotView] = useState<{ list: ProjectShot[]; at: number } | null>(null);
  function openInstalledProject(one: Addon) {
    const card = addonCards[one.fileName];
    if (!card?.projectId || !selected) return;
    projectFromRef.current = "installed";
    setBrowseKind("addon");
    setBrowseOpen(true);
    void openProject({
      source: "Modrinth",
      projectId: card.projectId,
      title: card.title || one.name,
      description: "",
      projectType: one.kind === "plugin" ? "plugin" : "mod",
      installable: true,
      pageUrl: `https://modrinth.com/project/${card.projectId}`,
      iconUrl: card.iconUrl ?? one.icon ?? null,
      author: null,
      downloads: null,
      follows: null,
      updatedAt: null,
      loaders: [],
      versions: [],
    });
  }
  function closeProject() {
    setProjectView(null);
    if (projectFromRef.current === "installed") setBrowseOpen(false);
    projectFromRef.current = "browse";
  }
  async function openProject(hit: CatalogHit) {
    if (!selected || hit.source !== "Modrinth") return;
    setProjectView({ hit, tab: "overview", detail: null, versions: null });
    setProjectShown(40);
    try {
      const [detail, versions] = await Promise.all([
        commands.modrinthProjectDetail(hit.projectId),
        commands.listAddonVersions(selected.id, hit.projectId, hit.projectType).catch(() => [] as ProjectVersion[]),
      ]);
      setProjectView((now) => (now && now.hit.projectId === hit.projectId ? { ...now, detail, versions } : now));
    } catch (cause) {
      setError(String(cause));
      setProjectView(null);
    }
  }
  async function installVersion(hit: CatalogHit, version: ProjectVersion) {
    if (!selected) return;
    const installed = addons.find((one) => addonCards[one.fileName]?.projectId === hit.projectId);
    markInstalling(hit.projectId, true);
    await call(
      async () => {
        if (installed) await commands.deleteAddon(selected.id, installed.fileName, installed.kind);
        await commands.installModrinth(selected.id, hit.projectId, hit.projectType, version.id);
      },
      installed
        ? tr("Changed {0} to {1}.", [hit.title, version.name])
        : tr("Installed {0} {1}.", [hit.title, version.name]),
    );
    await refreshAddons(selected);
    markInstalling(hit.projectId, false);
  }
  function openBodyLink(event: React.MouseEvent) {
    const anchor = (event.target as HTMLElement).closest<HTMLAnchorElement>("a[href]");
    if (!anchor) return;
    event.preventDefault();
    void openExternal(anchor.href);
  }
  const [browseOpen, setBrowseOpen] = useState(false);
  const lastSearchedRef = useRef<string | null>(null);
  const [addonReport, setAddonReport] = useState<AddonReport | null>(null);
  const [addonUpdates, setAddonUpdates] = useState<AddonUpdate[] | null>(null);
  const [waiting, setWaiting] = useState<WaitingAddon[]>([]);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [addonQuery, setAddonQuery] = useState("");
  const [addonFilter, setAddonFilter] = useState("all");
  const [addonTab, setAddonTab] = useState<"plugin" | "mod">("plugin");
  const [addonSort, setAddonSort] = useState("name");
  const [pickedAddons, setPickedAddons] = useState<string[]>([]);
  const addonDrag = useRef<{ index: number; base: string[]; on: boolean } | null>(null);
  const addonDragged = useRef(false);
  const [rollbacks, setRollbacks] = useState<AddonRollback[]>([]);
  const [logFiles, setLogFiles] = useState<LogFile[]>([]);
  const [sharingLog, setSharingLog] = useState(false);
  const [activeLogFile, setActiveLogFile] = useState("latest.log");
  const [readLogFile, setReadLogFile] = useState("latest.log");
  const [timeline, setTimeline] = useState<TimelineEvent[]>([]);
  const [timelineKind, setTimelineKind] = useState("All");
  const [dailyStats, setDailyStats] = useState<DailySummary[]>([]);
  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);
  const [markForm, setMarkForm] = useState({
    name: "",
    dimension: "overworld",
    x: "0",
    y: "64",
    z: "0",
    note: "",
  });
  const [toolLoading, setToolLoading] = useState(false);
  const historyRef = useRef<NavPoint[]>([]);
  const historyAt = useRef(-1);
  const travelling = useRef(false);
  const [navState, setNavState] = useState({ back: false, forward: false });
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; items: ContextItem[] } | null>(null);
  const contextBuilder = useRef<(target: HTMLElement) => ContextItem[]>(() => []);
  const closeContextMenu = useCallback(() => setContextMenu(null), []);
  const menuBox = useRef<HTMLElement>(null);
  const [menuMark, setMenuMark] = useState<{ left: number; width: number } | null>(null);
  useEffect(() => {
    const nav = menuBox.current;
    if (!nav) return;
    const place = () => {
      const on = nav.querySelector<HTMLElement>("button.on");
      setMenuMark((was) => {
        if (!on) return null;
        const next = { left: on.offsetLeft, width: on.offsetWidth };
        return was && was.left === next.left && was.width === next.width ? was : next;
      });
    };
    place();
    const watch = new ResizeObserver(place);
    watch.observe(nav);
    nav.querySelectorAll("button").forEach((one) => watch.observe(one));
    return () => watch.disconnect();
  });
  useEffect(() => {
    function open(event: MouseEvent) {
      if (event.defaultPrevented) return;
      const target = event.target as HTMLElement | null;
      if (!target) return;
      event.preventDefault();
      const items = contextBuilder.current(target);
      if (!items.some((one) => one !== "-")) return;
      setContextMenu({ x: event.clientX, y: event.clientY, items });
    }
    document.addEventListener("contextmenu", open);
    return () => document.removeEventListener("contextmenu", open);
  }, []);
  const [serversRoot, setServersRoot] = useState("");
  const [taskList, setTaskList] = useState<Task[]>([]);
  const [taskDraft, setTaskDraft] = useState<Task | null>(null);
  const [javaProcs, setJavaProcs] = useState<JavaProcess[]>([]);
  const [addonCards, setAddonCards] = useState<
    Record<
      string,
      {
        title: string;
        iconUrl: string | null;
        compatibility: string;
        warn: boolean;
        projectId: string;
      }
    >
  >({});
  const [appVersion, setAppVersion] = useState("");
  const [updateState, setUpdateState] = useState<{
    stage:
      | "idle"
      | "checking"
      | "found"
      | "none"
      | "downloading"
      | "stopping"
      | "ready";
    version?: string;
    percent?: number;
  }>({ stage: "idle" });
  const [settings, setSettings] = useState<AppSettings>({
    autoBackupOnSave: false,
    openConsoleOnStart: true,
    defaultMemoryMib: 4096,
    keepBackupDays: 30,
    keepAutoBackups: 0,
    consoleLineLimit: 2000,
    backupRoot: "",
    backupWorldsOnly: false,
    webPanel: false,
    webPanelPort: 25566,
    webTunnel: false,
    webDirect: false,
    webAccounts: [],
  });
  const navRef = useRef<HTMLElement>(null);
  const [navMark, setNavMark] = useState({ top: 0, height: 0, ready: false });
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [paletteQuery, setPaletteQuery] = useState("");
  const [paletteIndex, setPaletteIndex] = useState(0);
  const [trashEntries, setTrashEntries] = useState<TrashEntry[]>([]);
  const [filter, setFilter] = useState("");
  const [draggedInstanceId, setDraggedInstanceId] = useState<string | null>(
    null,
  );
  const [modal, setModal] = useState<Modal>("none");
  const [createStep, setCreateStep] = useState<"core" | "version" | "modpack">("core");
  const [createPage, setCreatePage] = useState<"game" | "core" | "finish" | "confirm">("game");
  const [createVersionQuery, setCreateVersionQuery] = useState("");
  const [createVersionMore, setCreateVersionMore] = useState(false);
  const [createSnapshots, setCreateSnapshots] = useState(false);
  const [upgradeSnapshots, setUpgradeSnapshots] = useState(false);
  const [createWhere, setCreateWhere] = useState("local");
  const [createGame, setCreateGame] = useState("Minecraft");
  const [createFree, setCreateFree] = useState<number | null>(null);
  const [packCreating, setPackCreating] = useState<string | null>(null);
  const [dropping, setDropping] = useState(false);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const copiedTimer = useRef<number | null>(null);
  function flashCopied(key: string) {
    setCopiedKey(key);
    if (copiedTimer.current) window.clearTimeout(copiedTimer.current);
    copiedTimer.current = window.setTimeout(() => setCopiedKey(null), 1400);
  }
  const fileDropRef = useRef({ browsePath: "", activeFile: null as string | null, locked: false });
  const [page, setPage] = useState<"home" | "instance">("home");
  const attention = useAttention();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [tool, setTool] = useState<Tool>("properties");
  const [loading, setLoading] = useState(true);
  const [webReady, setWebReady] = useState<boolean | null>(onWeb ? null : true);
  useEffect(() => {
    if (onWeb && !webReady) return;
    void commands.appVersion().then(setAppVersion).catch(() => {});
  }, [webReady]);
  const [sshHosts, setSshHosts] = useState<Record<string, string>>({});
  const [sshHostViews, setSshHostViews] = useState<SshHostView[]>([]);
  const reloadSshHosts = useCallback(() => {
    if (onWeb) return;
    void commands
      .listSshHosts()
      .then((found) => {
        setSshHosts(Object.fromEntries(found.map((one) => [one.id, one.name])));
        setSshHostViews(found);
      })
      .catch(() => {});
  }, []);
  useEffect(() => reloadSshHosts(), [reloadSshHosts]);
  const [sessionOpen, setSessionOpen] = useState<null | { tab: "ssh" | "panel"; adding: boolean }>(null);
  const [panelHosts, setPanelHosts] = useState<Record<string, string>>({});
  const [panelHostViews, setPanelHostViews] = useState<PanelHostView[]>([]);
  const reloadPanelHosts = useCallback(() => {
    if (onWeb) return;
    commands
      .listPanelHosts()
      .then((found) => {
        setPanelHosts(Object.fromEntries(found.map((one) => [one.id, one.name])));
        setPanelHostViews(found);
      })
      .catch(() => setPanelHosts({}));
  }, []);
  useEffect(() => reloadPanelHosts(), [reloadPanelHosts]);
  const hostChip = (instance: { remoteHost: string; panelHost?: string }) =>
    instance.panelHost ? (
      <span className="chip subtle host-chip" data-tip={tr("A server on a {0} panel", [panelKindName(panelHostViews.find((one) => one.id === instance.panelHost)?.kind)])}>
        <Icon name="cloud" />
        {panelHosts[instance.panelHost] ?? tr("Panel")}
      </span>
    ) : instance.remoteHost ? (
      <span className="chip subtle host-chip" data-tip={tr("Server on another computer (SSH)")}>
        <Icon name="plug" />
        {sshHosts[instance.remoteHost] ?? tr("SSH server")}
      </span>
    ) : null;
  const [remotePick, setRemotePick] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [noticeLeaving, setNoticeLeaving] = useState(false);
  const [noticePending, setNoticePending] = useState(false);
  const [noticeMorph, setNoticeMorph] = useState<string | null>(null);
  function showPending(text: string) {
    setNoticeMorph(null);
    setNoticePending(true);
    setNotice(text);
  }
  function morphNotice(text: string) {
    setNoticePending(false);
    setNoticeMorph(text);
    setNotice((now) => now ?? text);
  }
  const stoppingRef = useRef<Map<string, string>>(new Map());
  const [errorLeaving, setErrorLeaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [parsedLogs, setParsedLogs] = useState<ParsedLog[]>([]);
  const logLines = useMemo(
    () => parsedLogs.map((entry) => ({ serverId: "", line: entry.line, stream: "stdout", noise: false, ansi: "" })),
    [parsedLogs],
  );
  const [systemMemoryMib, setSystemMemoryMib] = useState<number | null>(null);
  const [javaNeed, setJavaNeed] = useState<number | null>(null);
  const [auditEntries, setAuditEntries] = useState<AuditEntry[]>([]);
  const [desktopAlertsEnabled, setDesktopAlertsEnabled] = useState(
    () => localStorage.getItem("monolab-desktop-alerts") === "enabled",
  );
  const [logView, setLogView] = useState("read");
  const [runs, setRuns] = useState<RunEntry[] | null>(null);
  const [compareOpen, setCompareOpen] = useState(false);
  const [trimOpen, setTrimOpen] = useState(false);
  const [diskOpen, setDiskOpen] = useState(false);
  const [pregenOpen, setPregenOpen] = useState(false);
  const [pregenArea, setPregenArea] = useState<{ x: number; z: number; rx: number; rz: number } | null>(null);
  const [logQuery, setLogQuery] = useState("");
  const logLevel = "All";
  const [command, setCommand] = useState("");
  const [hintAt, setHintAt] = useState(0);
  const [heldHints, setHeldHints] = useState<
    { text: string; hint: string }[] | null
  >(null);
  const [serverCommands, setServerCommands] = useState<CommandBook>({
    nodes: [],
    flat: [],
  });
  const [consoleFilter, setConsoleFilter] = useState("");
  const [railCompact] = useState(() => localStorage.getItem("monolab-rail") === "compact");
  useEffect(() => {
    const buttons = document.querySelectorAll<HTMLElement>(".sidebar button");
    for (const button of buttons) {
      if (railCompact) {
        if (!button.dataset.tip) {
          const label = (button.querySelector(".nav-label")?.textContent ?? button.textContent)?.replace(/\s+/g, " ").trim();
          if (label) {
            button.dataset.tip = label;
            button.dataset.tipAuto = "1";
          }
        }
      } else if (button.dataset.tipAuto) {
        delete button.dataset.tip;
        delete button.dataset.tipAuto;
      }
    }
  }, [railCompact, page, selectedId, instances, modal, tool]);
  const [consoleLevel, setConsoleLevel] = useState<ConsoleLevel>("all");
  const [historyIndex, setHistoryIndex] = useState(-1);
  const [consoleHistory, setConsoleHistory] = useState<string[]>(() => {
    try {
      return JSON.parse(
        localStorage.getItem("monolab-console-history") || "[]",
      ) as string[];
    } catch {
      return [];
    }
  });
  const [properties, setProperties] = useState<Property[]>([]);
  const [savedProperties, setSavedProperties] = useState<Property[]>([]);
  const [propertyQuery, setPropertyQuery] = useState("");
  const [propsTab, setPropsTab] = useState<Category>("Gameplay");
  const [wide, setWide] = useState(() => typeof window !== "undefined" && !!window.matchMedia?.("(min-width: 1280px)").matches);
  useEffect(() => {
    const query = window.matchMedia?.("(min-width: 1280px)");
    if (!query) return;
    const change = () => setWide(query.matches);
    query.addEventListener("change", change);
    return () => query.removeEventListener("change", change);
  }, []);
  const [resource, setResource] = useState<Resource | null>(null);
  const [tps, setTps] = useState<Tps | null>(null);
  const [resources, setResources] = useState<Record<string, Resource>>({});
  const [pings, setPings] = useState<Record<string, Ping>>({});
  const [players, setPlayers] = useState<Player[]>([]);
  const [playerActivity, setPlayerActivity] = useState<PlayerActivity[]>([]);
  const [playerName, setPlayerName] = useState("");
  const [addons, setAddons] = useState<Addon[]>([]);
  const addonUpdateBy = useMemo(
    () => new Map((addonUpdates ?? []).map((one) => [one.fileName, one])),
    [addonUpdates],
  );
  const tabAddons = useMemo(() => addons.filter((one) => one.kind === addonTab), [addons, addonTab]);
  const shownAddons = useMemo(() => {
    const needle = addonQuery.trim().toLowerCase();
    const list = tabAddons.filter((one) => {
      const card = addonCards[one.fileName];
      const title = (card?.title || one.name).toLowerCase();
      if (needle && !title.includes(needle) && !one.fileName.toLowerCase().includes(needle)) {
        return false;
      }
      const clientOnly = isClientOnly(one, card);
      switch (addonFilter) {
        case "update":
          return addonUpdateBy.has(one.fileName);
        case "off":
          return !one.enabled;
        case "client":
          return clientOnly;
        case "all":
          return true;
        default:
          return one.kind === addonFilter;
      }
    });
    const nameOf = (one: Addon) => (addonCards[one.fileName]?.title || one.name).toLowerCase();
    return list.sort((a, b) => {
      if (addonSort === "name-desc") return nameOf(b).localeCompare(nameOf(a), "ko");
      if (addonSort === "kind") {
        const byKind = a.kind.localeCompare(b.kind);
        if (byKind) return byKind;
      }
      return nameOf(a).localeCompare(nameOf(b), "ko");
    });
  }, [tabAddons, addonCards, addonQuery, addonFilter, addonSort, addonUpdateBy]);
  const [installing, setInstalling] = useState<Set<string>>(new Set());
  const [addonProgress, setAddonProgress] = useState<Record<string, { file: string; received: number; total: number | null }>>({});
  function installProgressLabel(projectId: string) {
    const one = addonProgress[projectId];
    if (!one) return "";
    const mb = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    if (one.total) return `${Math.min(100, Math.round((one.received / one.total) * 100))}%`;
    return mb(one.received);
  }
  const markInstalling = (projectId: string, on: boolean) => {
    if (!on) setAddonProgress((now) => { const next = { ...now }; delete next[projectId]; return next; });
    setInstalling((current) => {
      const next = new Set(current);
      if (on) next.add(projectId);
      else next.delete(projectId);
      return next;
    });
  };
  const installedProjectIds = useMemo(
    () =>
      new Set(
        Object.values(addonCards)
          .map((card) => card.projectId)
          .filter(Boolean),
      ),
    [addonCards],
  );
  const addonCounts = useMemo(() => {
    const kinds: Record<string, number> = {};
    let off = 0;
    let client = 0;
    for (const one of addons) {
      kinds[one.kind] = (kinds[one.kind] ?? 0) + 1;
      if (!one.enabled) off += 1;
      const card = addonCards[one.fileName];
      if (isClientOnly(one, card)) client += 1;
    }
    return { kinds, off, client, update: addonUpdates?.length ?? 0 };
  }, [addons, addonCards, addonUpdates]);

  const [addon, setAddon] = useState<AddonStatus | null>(null);
  const [marksOpen, setMarksOpen] = useState(false);
  const [shareOpen, setShareOpen] = useState(false);
  const [mapFocus, setMapFocus] = useState<{ x: number; z: number; n: number } | null>(null);
  const [mapSpawn, setMapSpawn] = useState<[number, number] | null>(null);
  const [backupList, setBackupList] = useState<BackupFile[]>([]);
  const [backupBusy, setBackupBusy] = useState(false);
  const [findings, setFindings] = useState<Finding[] | null>(null);
  const [theme, setThemeState] = useState<Theme>(readTheme);
  const [compact, setCompactState] = useState(readCompact);
  const [languageLabels, setLanguageLabels] = useState<Record<string, string>>({});
  useEffect(() => {
    void languageNames().then(setLanguageLabels);
  }, []);
  const themeRef = useRef<Theme>(theme);
  const [playit, setPlayit] = useState<PlayitStatus | null>(null);
  const [playitBusy, setPlayitBusy] = useState(false);
  const [lanIp, setLanIp] = useState("");
  const [reachProtocol, setReachProtocol] = useState<Protocol>("tcp");
  const [itemNames, setItemNames] = useState<Record<string, string>>({});
  const [itemIcons, setItemIcons] = useState<Record<string, string>>({});
  const [enchantNames, setEnchantNames] = useState<Record<string, string>>({});
  const [invTab, setInvTab] = useState<"bag" | "ender">("bag");
  const [live, setLive] = useState<LiveState | null>(null);
  const [automationHistory, setAutomationHistory] = useState<
    AutomationHistory[]
  >([]);
  const [upgradeHistory, setUpgradeHistory] = useState<UpgradeHistory[]>([]);
  const [upgradeVersion, setUpgradeVersion] = useState("");
  const [upgradeVersions, setUpgradeVersions] = useState<string[]>([]);
  const [worlds, setWorlds] = useState<World[]>([]);
  const [activeFile, setActiveFile] = useState<string | null>(null);
  const [fileImage, setFileImage] = useState<string | null>(null);
  const [trashList, setTrashList] = useState<TrashFile[] | null>(null);
  const [trashBusy, setTrashBusy] = useState(false);
  const [browsePath, setBrowsePath] = useState("");
  const [browseEntries, setBrowseEntries] = useState<BrowseEntry[]>([]);
  const [browseTotal, setBrowseTotal] = useState(0);
  const [browseMoreBusy, setBrowseMoreBusy] = useState(false);
  const fileListBox = useRef<HTMLDivElement>(null);
  const [serverIcons, setServerIcons] = useState<Record<string, string>>({});
  const [fileContent, setFileContent] = useState("");
  const [fileSaved, setFileSaved] = useState("");
  const [openTabs, setOpenTabs] = useState<string[]>([]);
  const tabBuffers = useRef(new Map<string, { content: string; saved: string; image: string | null }>());
  const [editorCursor, setEditorCursor] = useState<Cursor>({ line: 1, column: 1, selected: 0, lines: 1 });
  useEffect(() => {
    if (activeFile) setOpenTabs((now) => (now.includes(activeFile) ? now : [...now, activeFile]));
  }, [activeFile]);
  const [fileSort, setFileSort] = useState<{ by: "name" | "size" | "date"; desc: boolean }>({
    by: "name",
    desc: false,
  });
  const [pickedFiles, setPickedFiles] = useState<Set<string>>(new Set());
  const [fileQuery, setFileQuery] = useState("");
  const [catalogQuery, setCatalogQuery] = useState("");
  const [browseKind, setBrowseKind] = useState<"addon" | "modpack" | "datapack">("addon");
  const catalogLoader = "";
  const [webPanel, setWebPanel] = useState<WebPanelView>({
    on: false,
    port: 25566,
    key: "",
    address: "",
    tunnel: false,
    tunnelAddress: "",
    wantOutside: false,
    accounts: 0,
    viewers: 0,
    direct: false,
  });
  const [webAccounts, setWebAccounts] = useState<WebAccount[]>([]);
  const [accountDraft, setAccountDraft] = useState<
    (WebAccount & { password: string; isNew: boolean }) | null
  >(null);
  useEffect(() => {
    if (modal !== "tasks") setTaskDraft(null);
    if (modal !== "web") setAccountDraft(null);
    setSessionOpen(null);
  }, [modal]);
  const [tunnelBusy, setTunnelBusy] = useState(false);
  const [openOutside, setOpenOutside] = useState(false);
  const [openDirect, setOpenDirect] = useState(false);
  const [versions, setVersions] = useState(fallbackVersions);
  const [versionsBusy, setVersionsBusy] = useState(false);
  const [coreBuilds, setCoreBuilds] = useState<CoreBuild[]>([]);
  const [buildNews, setBuildNews] = useState<CoreBuildNews | null>(null);
  const [versionOpen, setVersionOpen] = useState(false);
  const [gameUpdating, setGameUpdating] = useState(false);
  const [upgradeBuilds, setUpgradeBuilds] = useState<CoreBuild[]>([]);
  const [upgradeBuild, setUpgradeBuild] = useState("");
  const [upgradeBuildsBusy, setUpgradeBuildsBusy] = useState(false);
  const [upgradeBusy, setUpgradeBusy] = useState(false);
  const [buildsBusy, setBuildsBusy] = useState(false);
  const [memoryMib, setMemoryMib] = useState(4096);
  const [minMemoryMib, setMinMemoryMib] = useState(4096);
  const [memoryHandlesSwapped, setMemoryHandlesSwapped] = useState(false);
  const [lowOnTop, setLowOnTop] = useState(false);
  const grabRef = useRef<{
    id: string;
    y: number;
    moved: boolean;
    offsetY: number;
    offsetX: number;
    left: number;
    width: number;
    html: string;
    className: string;
  } | null>(null);
  const [dragY, setDragY] = useState<number | null>(null);
  const [dragX, setDragX] = useState<number | null>(null);
  const [arrivingServerId, setArrivingServerId] = useState<string | null>(null);

  const draggedJustNow = useRef(false);
  const [javaList, setJavaList] = useState<JavaRuntime[]>([]);
  const [remoteJavas, setRemoteJavas] = useState<JavaRuntime[] | null>(null);
  const ownedJavas = javaList.filter((one) => one.managed);
  const [jvmArgs, setJvmArgs] = useState("");
  const [customCommand, setCustomCommand] = useState("");
  const [customMode, setCustomMode] = useState(false);
  const [scriptMode, setScriptMode] = useState(false);
  const [scriptPick, setScriptPick] = useState("");
  const [scriptChoices, setScriptChoices] = useState<string[] | null>(null);
  const [javaPath, setJavaPath] = useState("java");
  const [creating, setCreating] = useState(false);
  const [suggestedName, setSuggestedName] = useState(tr("Server 1"));
  const [importing, setImporting] = useState(false);
  const [archive, setArchive] = useState<ArchiveProgress | null>(null);
  const [sshUpload, setSshUpload] = useState<SshJob | null>(null);
  const [importWhere, setImportWhere] = useState<"keep" | "managed">("keep");
  const [download, setDownload] = useState<DownloadProgress | null>(null);
  const [createOperationId, setCreateOperationId] = useState<string | null>(
    null,
  );
  const [mrpackPath, setMrpackPath] = useState("");
  const [packQuery, setPackQuery] = useState("");
  const [packHits, setPackHits] = useState<CatalogHit[]>([]);
  const [packBusy, setPackBusy] = useState(false);
  const [packGetting, setPackGetting] = useState("");
  const [packSearched, setPackSearched] = useState(false);
  const [cfPack, setCfPack] = useState<CfPack | null>(null);
  const [mrpackEntries, setMrpackEntries] = useState<MrpackEntry[]>([]);
  const [includeClientOnlyMods, setIncludeClientOnlyMods] = useState(false);
  const [form, setForm] = useState({
    name: "",
    core: "Paper",
    version: "1.21.4",
    coreBuild: "",
    maxMemoryMib: 4096,
    eulaAccepted: false,
  });
  const selected = useMemo(
    () => instances.find((instance) => instance.id === selectedId) ?? null,
    [instances, selectedId],
  );
  const javaChoices = selected?.remoteHost ? (remoteJavas ?? []) : javaList;
  const [place, setPlace] = useState("local");
  const [gameFilter, setGameFilter] = useState(() => localStorage.getItem("monolab-game-filter") ?? "");
  const gameCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const one of instances) counts.set(gameOf(one.core), (counts.get(gameOf(one.core)) ?? 0) + 1);
    return counts;
  }, [instances]);
  const shownGame = gameFilter && gameCounts.has(gameFilter) ? gameFilter : "";
  const placeBar = useRef<HTMLDivElement | null>(null);
  const [placeMark, setPlaceMark] = useState({ left: 0, width: 0, ready: false });
  useEffect(() => {
    const bar = placeBar.current;
    if (!bar) return;
    const on = bar.querySelector<HTMLElement>("button.on");
    if (!on) return;
    setPlaceMark({ left: on.offsetLeft, width: on.offsetWidth, ready: true });
  }, [place, instances, sshHosts, panelHosts]);
  const list = useMemo(
    () =>
      instances.filter(
        (item) =>
          item.name.toLowerCase().includes(filter.toLowerCase()) &&
          (!shownGame || gameOf(item.core) === shownGame) &&
          (place === "local"
            ? !item.remoteHost && !item.panelHost
            : place.startsWith("panel:")
              ? item.panelHost === place.slice(6)
              : item.remoteHost === place),
      ),
    [instances, filter, place, shownGame],
  );
  const remotePlaces = useMemo(
    () => [
      ...new Set([
        ...Object.keys(sshHosts),
        ...instances.map((one) => one.remoteHost).filter((id): id is string => Boolean(id)),
      ]),
    ],
    [sshHosts, instances],
  );
  const adoptRemoteServers = useCallback(
    async (hostId: string) => {
      try {
        const folders = await commands.scanRemoteServers(hostId);
        let added = 0;
        for (const one of folders) {
          try {
            await commands.importRemoteInstance(hostId, one.path);
            added += 1;
          } catch {}
        }
        if (added > 0) {
          setNotice(tr("Found {0, plural, one {# new server} other {# new servers}} in the SSH server folder and added them.", [added]));
          void refresh();
        }
      } catch {}
    },
    [],
  );
  useEffect(() => {
    if (place !== "local" && !place.startsWith("panel:") && remotePlaces.includes(place)) void adoptRemoteServers(place);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [place]);
  useEffect(() => {
    const panelAlive = place.startsWith("panel:") && place.slice(6) in panelHosts;
    if (place !== "local" && !panelAlive && !remotePlaces.includes(place)) setPlace("local");
  }, [place, remotePlaces, panelHosts]);

  const [, setLangTick] = useState(0);
  useEffect(() => {
    const bump = () => setLangTick((n) => n + 1);
    window.addEventListener("monolab-lang-edit", bump);
    return () => window.removeEventListener("monolab-lang-edit", bump);
  }, []);
  const summaryPills = (() => {
    const liveOnes = list.filter((one) => one.status === "running" || one.status === "stopping");
    const online = liveOnes.reduce((sum, one) => sum + (pings[one.id]?.online ?? 0), 0);
    const onlineNames = [...new Set(liveOnes.flatMap((one) => pings[one.id]?.names ?? []))];
    const ram = liveOnes.reduce((sum, one) => sum + (resources[one.id]?.memoryMib ?? 0), 0);
    const ramCap = liveOnes.reduce((sum, one) => sum + (has(one.core, "java") ? one.maxMemoryMib : 0), 0);
    return list.length ? (
      <div className="monitor-grid list-summary">
        <div>
          <span>{tr("Server")}</span>
          <strong>{list.length}</strong>
        </div>
        <div>
          <span>{tr("Running")}</span>
          <strong className={liveOnes.length ? "on" : ""}>{liveOnes.length}</strong>
        </div>
        <div className="player-peek-host">
          <span>{tr("Players")}</span>
          <strong>{liveOnes.length ? online : "—"}</strong>
          <PlayerPeek names={onlineNames} online={online} />
        </div>
        <div>
          <span>{tr("Memory use")}</span>
          <strong>{liveOnes.length ? <>{used(ram)} <small>/ {bytes(ramCap)}</small></> : "—"}</strong>
        </div>
      </div>
    ) : null;
  })();

  function confirmAction(
    title: string,
    body: string,
    confirmLabel: string,
    danger = false,
  ): Promise<boolean> {
    return new Promise((resolve) =>
      setAsk({
        title,
        body,
        confirmLabel,
        danger,
        resolve: (value) => resolve(value !== null),
      }),
    );
  }
  function askText(
    title: string,
    label: string,
    value: string,
    confirmLabel: string,
    danger = false,
  ): Promise<string | null> {
    return new Promise((resolve) =>
      setAsk({
        title,
        confirmLabel,
        danger,
        input: { label, value },
        resolve,
      }),
    );
  }
  function askChoice(
    title: string,
    body: string,
    choices: { value: string; label: string }[],
    value: string,
  ): Promise<string | null> {
    return new Promise((resolve) =>
      setAsk({ title, body, confirmLabel: tr("Download"), choices, value, resolve }),
    );
  }
  async function refreshInstances() {
    try {
      const next = keepSame(instancesRef.current, await commands.listInstances());
      instancesRef.current = next;
      setInstances(next);
    } catch {}
  }
  const NOTICE_LIFE = 10_000;
  const [noticeKey, setNoticeKey] = useState(0);
  const lastNoticeRef = useRef<string | null>(null);
  useEffect(() => {
    if (!notice) {
      lastNoticeRef.current = null;
      setNoticeMorph(null);
      setNoticePending(false);
      return;
    }
    if (notice !== lastNoticeRef.current) {
      lastNoticeRef.current = notice;
      setNoticeKey((key) => key + 1);
    }
    setNoticeLeaving(false);
    const life = noticePending ? 60_000 : NOTICE_LIFE;
    const fade = window.setTimeout(() => setNoticeLeaving(true), life - 220);
    const gone = window.setTimeout(() => setNotice(null), life);
    return () => {
      window.clearTimeout(fade);
      window.clearTimeout(gone);
    };
  }, [notice, noticeMorph, noticePending]);
  useEffect(() => {
    for (const [id, name] of stoppingRef.current) {
      const one = instances.find((instance) => instance.id === id);
      if (!one || one.status === "stopped" || one.status === "crashed") {
        stoppingRef.current.delete(id);
        morphNotice(tr("Stopped {0}.", [name]));
      }
    }
  }, [instances]);
  useEffect(() => {
    if (error) setErrorLeaving(false);
  }, [error]);
  function dismissError() {
    setErrorLeaving(true);
    window.setTimeout(() => setError(null), 220);
  }
  function dismissNotice() {
    setNoticeLeaving(true);
    window.setTimeout(() => setNotice(null), 220);
  }
  useEffect(() => {
    const move = (event: PointerEvent) => dragInstance(event);
    const up = () => dropInstance();
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
  });
  async function askWhoAmI() {
    if (!onWeb) return;
    try {
      const answer = await fetch("/whoami", {
        method: "POST",
        headers: { "X-SE-Session": webSession() },
      });
      setWebReady(answer.ok);
    } catch {
      setWebReady(false);
    }
  }
  async function refresh() {
    try {
      setError(null);
      const [nextInstances, nextTrash, nextRoot] = await Promise.all([
        commands.listInstances(),
        commands.listTrashInstances(),
        commands.getServersRoot().catch(() => ""),
        commands.getAppSettings()
          .then((next) => setSettings((old) => keepSame(old, next)))
          .catch(() => undefined),
        commands.listTasks()
          .then((next) => setTaskList((old) => keepSame(old, next)))
          .catch(() => undefined),
      ]);
      const kept = keepSame(instancesRef.current, nextInstances);
      instancesRef.current = kept;
      setInstances(kept);
      setTrashEntries((old) => keepSame(old, nextTrash));
      if (nextRoot) setServersRoot(nextRoot);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setLoading(false);
    }
  }
  async function refreshResources(running: Instance[], withPing: boolean) {
    if (!running.length) {
      setResources({});
      setPings({});
      return;
    }
    const snapshots = await Promise.all(
      running.map(async (instance) => {
        const [resource, ping] = await Promise.allSettled([
          commands.getResourceSnapshot(instance.id),
          withPing && has(instance.core, "ping") ? commands.getServerPing(instance.id) : Promise.reject(new Error("skip")),
        ]);
        return { instance, resource, ping };
      }),
    );
    const nextResources: Record<string, Resource> = {};
    for (const snapshot of snapshots) {
      if (snapshot.resource.status === "fulfilled")
        nextResources[snapshot.instance.id] = snapshot.resource.value;
    }
    setResources(nextResources);
    if (withPing) {
      const nextPings: Record<string, Ping> = {};
      for (const snapshot of snapshots) {
        if (snapshot.ping.status === "fulfilled")
          nextPings[snapshot.instance.id] = snapshot.ping.value;
      }
      setPings(nextPings);
    }
  }
  useEffect(() => {
    void askWhoAmI();
  }, []);
  useEffect(() => {
    if (webReady !== true) return;
    let tries = 0;
    const load = () => {
      void refresh().then(() => {
        if (instancesRef.current.length || tries >= 4) return;
        tries += 1;
        window.setTimeout(load, 1200);
      });
    };
    load();
    const log = listenTo<Log[]>("log:batch", (batch) => logStore.append(batch));
    const status = listenTo("server-status", () => void refresh());
    const progress = listenTo<DownloadProgress>("server-download", (payload) =>
      setDownload(payload),
    );
    const addonPulls = listenTo<{ serverId: string; projectId: string; file: string; received: number; total: number | null }>(
      "addon-download",
      (payload) => setAddonProgress((now) => ({ ...now, [payload.projectId]: { file: payload.file, received: payload.received, total: payload.total } })),
    );
    const unpack = listenTo<ArchiveProgress>("archive-import", (payload) =>
      setArchive(payload),
    );
    const sshSend = listenTo<SshJob>("remote-upload", (payload) =>
      setSshUpload(payload.stage === "done" ? null : payload),
    );
    const alert = listenTo<{ kind: string; message: string }>(
      "server-alert",
      (payload) => {
        if (desktopAlertsEnabled)
          sendNotification({
            title: `Monolab · ${payload.kind}`,
            body: payload.message,
          });
      },
    );
    const lag = listenTo<{ serverId: string; name: string; tps: number; findings: LagFinding[] }>(
      "server-lag",
      (payload) => {
        const why = payload.findings.map((one) => lagLine(one).title).join(" · ");
        const message =
          tr("{0} is lagging · {1} TPS", [payload.name, payload.tps.toFixed(1)]) +
          (why ? ` — ${why}` : ` — ${tr("See the lag diagnosis on the Monitor tab.")}`);
        setError(message);
        if (desktopAlertsEnabled)
          sendNotification({ title: `Monolab · ${tr("Lag")}`, body: message });
      },
    );
    return () => {
      void lag.then((remove) => remove());
      void log.then((remove) => remove());
      void status.then((remove) => remove());
      void progress.then((remove) => remove());
      void addonPulls.then((remove) => remove());
      void unpack.then((remove) => remove());
      void sshSend.then((remove) => remove());
      void alert.then((remove) => remove());
    };
  }, [desktopAlertsEnabled, webReady]);
  useEffect(() => {
    if (!startingServer || startingServer.done) return;
    const current = instances.find((instance) => instance.id === startingServer.id);
    const settled = Date.now() - startingServer.at > 3_000;
    if (!current || ((current.status === "stopped" || current.status === "crashed") && settled)) {
      setStartingServer(null);
      morphNotice(tr("{0} stopped.", [startingServer.name]));
      return;
    }
    if (current.ready) {
      setStartingServer(null);
      morphNotice(tr("{0} started!", [startingServer.name]));
    }
  }, [instances, startingServer]);
  useEffect(() => {
    for (const instance of instances) {
      if (instance.id in serverIcons) continue;
      void commands.getServerIcon(instance.id)
        .then((icon) =>
          setServerIcons((current) => ({ ...current, [instance.id]: icon ?? "" })),
        )
        .catch(() => undefined);
    }
  }, [instances, serverIcons]);
  const javaPeriod = period(15_000, attention);
  useEffect(() => {
    if (javaPeriod === null) return;
    void refreshJavaProcs();
    const timer = window.setInterval(() => void refreshJavaProcs(), javaPeriod);
    return () => window.clearInterval(timer);
  }, [javaPeriod]);
  useEffect(() => followSystem(() => themeRef.current), []);
  function setTheme(next: Theme) {
    themeRef.current = next;
    setThemeState(next);
    applyTheme(next);
  }
  function setCompact(next: boolean) {
    setCompactState(next);
    applyCompact(next);
  }
  const reachServer = page === "instance" && tool === "reach" ? selected?.id ?? null : null;
  const reachCustom = reachServer && selected && isCustom(selected.core);
  useEffect(() => {
    if (!reachServer || webReady !== true) return;
    void commands.lanIp().then(setLanIp).catch(() => {});
    if (reachCustom) {
      void commands
        .readCustomProgram(reachServer)
        .then((one) => setReachProtocol(one.protocol === "udp" || one.protocol === "both" ? one.protocol : "tcp"))
        .catch(() => setReachProtocol("tcp"));
    }
  }, [reachServer, reachCustom, webReady]);
  const playitPeriod = period(4_000, attention);
  useEffect(() => {
    if (webReady !== true || page !== "instance") return;
    if (tool !== "overview" && tool !== "console" && tool !== "reach") return;
    void loadPlayit();
    if (playitPeriod === null) return;
    const timer = window.setInterval(() => void loadPlayit(), playitPeriod);
    return () => window.clearInterval(timer);
  }, [webReady, page, tool, playitPeriod]);
  const liveInvPeriod = period(5_000, attention);
  const liveInvKey = playerSnapshot?.live && selected ? `${selected.id}/${playerSnapshot.uuid}` : null;
  useEffect(() => {
    if (!liveInvKey || liveInvPeriod === null) return;
    const [server, uuid] = liveInvKey.split("/");
    const timer = window.setInterval(() => {
      void commands.readPlayerNbt(server, uuid).then(
        (next) => setPlayerSnapshot((was) => (was && was.uuid === uuid ? next : was)),
        () => undefined,
      );
    }, liveInvPeriod);
    return () => window.clearInterval(timer);
  }, [liveInvKey, liveInvPeriod]);
  const chunkPeriod = period(5_000, attention);
  const chunkServer = mapOpen ? selected?.id ?? null : null;
  useEffect(() => {
    if (webReady !== true || !chunkServer) {
      setLoadedChunks(null);
      return;
    }
    const pull = () =>
      void commands.readLoadedChunks(chunkServer).then(setLoadedChunks, () => setLoadedChunks(null));
    pull();
    if (!mapChunksOn || chunkPeriod === null) return;
    const timer = window.setInterval(pull, chunkPeriod);
    return () => window.clearInterval(timer);
  }, [webReady, chunkServer, mapChunksOn, chunkPeriod]);
  const mapChunks = useMemo(() => {
    if (!loadedChunks || !selected) return null;
    const loaded: number[] = [];
    const forced: number[] = [];
    for (const one of loadedChunks.worlds) {
      if (!inRealm(one.world, mapRealm)) continue;
      loaded.push(...one.chunks);
      forced.push(...one.forced);
    }
    return { loaded, forced, known: selected.core !== "Fabric" };
  }, [loadedChunks, mapRealm, selected]);
  const runServer = selected?.id ?? null;
  useEffect(() => {
    if (webReady !== true || tool !== "logs" || logView !== "runs" || !runServer) return;
    const id = runServer;
    setRuns(null);
    const pull = () => void commands.listRuns(id).then(setRuns, () => setRuns([]));
    pull();
    const timer = window.setInterval(pull, 10_000);
    return () => window.clearInterval(timer);
  }, [webReady, tool, logView, runServer]);
  const busyPeriod = period(1_500, attention);
  useEffect(() => {
    const busy = instances.some(
      (instance) =>
        instance.status === "stopping" || instance.status === "running" || instance.checking || !!instance.remoteHost || !!instance.panelHost,
    );
    if (!busy || busyPeriod === null) return;
    const timer = window.setInterval(() => void refreshInstances(), busyPeriod);
    return () => window.clearInterval(timer);
  }, [instances, busyPeriod]);
  const resourcePeriod = period(1_000, attention);
  const visibleRunning =
    modal !== "none"
      ? ""
      : page === "home"
        ? instances
            .filter((instance) => instance.status === "running")
            .map((instance) => instance.id)
            .join(",")
        : selected?.status === "running"
          ? selected.id
          : "";
  useEffect(() => {
    if (!visibleRunning) {
      setResources({});
      setPings({});
      return;
    }
    if (resourcePeriod === null) return;
    let tick = 0;
    const read = () => {
      tick += 1;
      const wanted = visibleRunning.split(",");
      const running = instancesRef.current.filter((instance) => instance.status === "running" && wanted.includes(instance.id));
      void refreshResources(running, tick % 4 === 1).catch(() => undefined);
    };
    read();
    const timer = window.setInterval(read, resourcePeriod);
    return () => window.clearInterval(timer);
  }, [visibleRunning, resourcePeriod]);
  const monitorPeriod = period(selected?.remoteHost ? 3_000 : 1_000, attention);
  const recentHistory = history.slice(-100);
  const monitorBusy = useRef(false);
  const monitorId = tool === "monitor" && selected?.status === "running" ? selected.id : null;
  useEffect(() => {
    if (!monitorId) return;
    if (monitorPeriod === null) return;
    const id = monitorId;
    const read = () => {
      if (monitorBusy.current) return;
      monitorBusy.current = true;
      void Promise.all([
        commands.getResourceSnapshot(id),
        commands.getTpsSnapshot(id),
        commands.getResourceHistory(id),
      ])
        .then(([nextResource, nextTps, nextHistory]) => {
          setResource(nextResource);
          setTps(nextTps);
          setHistory(nextHistory);
        })
        .catch(() => undefined)
        .finally(() => {
          monitorBusy.current = false;
        });
    };
    read();
    const timer = window.setInterval(read, monitorPeriod);
    return () => window.clearInterval(timer);
  }, [monitorId, monitorPeriod]);
  async function call(work: () => Promise<unknown>, success?: string, pending?: string) {
    try {
      setError(null);
      if (pending) showPending(pending);
      await work();
      if (success) {
        if (pending) morphNotice(success);
        else setNotice(success);
      } else if (pending) dismissNotice();
      await refresh();
      return true;
    } catch (cause) {
      if (pending) dismissNotice();
      setError(String(cause));
      return false;
    }
  }
  function resetForm() {
    setForm({
      name: "",
      core: "",
      version: versions[0] ?? "1.21.4",
      coreBuild: "",
      maxMemoryMib: settings.defaultMemoryMib,
      eulaAccepted: false,
    });
    setCoreBuilds([]);
  }
  async function openCreate() {
    resetForm();
    setCreateStep("core");
    setCreatePage("game");
    setCreateVersionQuery("");
    setCreateVersionMore(false);
    setCreateSnapshots(false);
    setCreateWhere("local");
    setCreateGame("");
    setCreateFree(null);
    setModal("create");
    if (!onWeb) void commands.serversRootFreeBytes().then(setCreateFree).catch(() => setCreateFree(null));
    try {
      setSuggestedName(await commands.suggestServerName());
    } catch {
      setSuggestedName(tr("New server"));
    }
  }
  function pickGame(game: string) {
    setCreateGame(game);
    if (game === "Minecraft") {
      if (isGame(form.core)) setForm({ ...form, core: "", version: "", coreBuild: "" });
      setCreatePage("core");
      return;
    }
    setCreatePage("finish");
    setCreateWhere("local");
    void chooseCore(game);
  }
  async function chooseCore(core: string) {
    const versionless = !has(core, "versions");
    setForm({ ...form, core, version: versionless ? "latest" : "", coreBuild: "" });
    setCoreBuilds([]);
    if (versionless) return;
    setVersionsBusy(true);
    try {
      const list = await commands.listCoreVersions(core, createSnapshots);
      setVersions(list.length ? list : fallbackVersions);
    } catch (cause) {
      setError(String(cause));
      setVersions(fallbackVersions);
    } finally {
      setVersionsBusy(false);
    }
  }
  async function loadCoreBuilds(core: string, version: string) {
    if (!core || !version || isProxy(core)) {
      setCoreBuilds([]);
      return;
    }
    setBuildsBusy(true);
    try {
      const list = await commands.listCoreBuilds(core, version);
      setCoreBuilds(list);
      const first = list.find((one) => one.stable) ?? list[0];
      if (first) setForm((now) => ({ ...now, coreBuild: first.id }));
    } catch {
      setCoreBuilds([]);
    } finally {
      setBuildsBusy(false);
    }
  }
  async function refreshWebPanel() {
    try {
      const next = await commands.webPanelState();
      setWebPanel(next);
      setOpenOutside(next.wantOutside);
      setOpenDirect(next.direct);
    } catch {}
  }
  async function toggleWebPanel(on: boolean, port?: number) {
    if (
      on &&
      !(await confirmAction(
        tr("Others on the same Wi-Fi can see this"),
        tr("The web panel uses unencrypted HTTP. Anyone on the same network (Wi-Fi or router) could intercept traffic, including your login password. Only enable it on a trusted network, such as your home Wi-Fi. The external address is encrypted."),
        tr("I understand, turn it on"),
      ))
    ) {
      return;
    }
    setTunnelBusy(on && openOutside);
    try {
      const next = await commands.setWebPanel(on, port ?? webPanel.port);
      setWebPanel(next);
      if (!on) {
        setNotice(tr("Web panel stopped."));
        return;
      }
      if (!openOutside && !openDirect) {
        setNotice(tr("Web panel started!"));
        return;
      }
      try {
        if (openDirect) {
          setWebPanel(await commands.setWebDirect(true));
          setNotice(tr("Web panel started! If port forwarding is set up on your router, it's also accessible from outside your network."));
        } else {
          setWebPanel(await commands.setWebTunnel(true));
          setNotice(tr("Web panel started! You can also access it from outside your network."));
        }
      } catch (cause) {
        setError(String(cause));
        setNotice(tr("The web panel started, but public access failed."));
      }
    } catch (cause) {
      setError(String(cause));
    } finally {
      setTunnelBusy(false);
    }
  }
  async function toggleDirect(on: boolean) {
    if (on) {
      const ok = await confirmAction(
        tr("Use port forwarding for external access?"),
        tr("Forward port {0} to this computer ({1}) in your router settings to allow external access. The app won't set this up for you. The connection isn't encrypted, so your password is sent over the internet in plain text and port-scanning bots can find the panel. A Cloudflare tunnel is safer.", [webPanel.port, lanHost(webPanel.address)]),
        tr("I understand, use it"),
        true,
      );
      if (!ok) return;
    }
    setTunnelBusy(true);
    try {
      setWebPanel(await commands.setWebDirect(on));
      setNotice(on ? tr("Port-forwarding mode is on.") : tr("Port-forwarding mode is off."));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setTunnelBusy(false);
    }
  }
  async function toggleTunnel(on: boolean) {
    if (on) {
      const ok = await confirmAction(
        tr("Allow access from outside your network?"),
        tr("Makes this panel accessible from anywhere on the internet. Anyone with the address ") +
          tr("Server names and players are visible, so don't post the link publicly. ") +
          tr("You cannot start or stop servers from here either."),
        tr("Enable"),
      );
      if (!ok) return;
    }
    setTunnelBusy(true);
    try {
      setWebPanel(await commands.setWebTunnel(on));
      setNotice(on ? tr("Now accessible from outside your network!") : tr("Tunnel stopped."));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setTunnelBusy(false);
    }
  }
  async function refreshWebAccounts() {
    try {
      setWebAccounts(await commands.listWebAccounts());
    } catch {}
  }
  async function saveWebAccount() {
    if (!accountDraft) return;
    try {
      await commands.saveWebAccount(accountDraft.name, accountDraft.password, accountDraft.role, accountDraft.servers);
      setAccountDraft(null);
      await refreshWebAccounts();
      await refreshWebPanel();
      setNotice(tr("Saved account {0}!", [accountDraft.name]));
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function deleteWebAccount(name: string) {
    const ok = await confirmAction(
      tr("Delete account {0}?", [name]),
      tr("This account will no longer be able to sign in. Anyone signed in with it will be signed out."),
      tr("Delete"),
      true,
    );
    if (!ok) return;
    try {
      await commands.deleteWebAccount(name);
      await refreshWebAccounts();
      await refreshWebPanel();
      setNotice(tr("Deleted account {0}.", [name]));
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function pickFolderToImport() {
    try {
      const picked = await open({
        directory: true,
        multiple: false,
        title: tr("Choose the server folder"),
      });
      if (typeof picked !== "string") return;
      const copy = importWhere === "managed";
      if (copy) {
        setModal("none");
        setImporting(true);
        setArchive({ stage: "copying" });
      }
      try {
        const ok = await call(
          () => commands.importInstance(picked, copy),
          copy ? tr("Server copied to the app folder and imported.") : tr("Server imported."),
        );
        if (ok) setModal("none");
      } finally {
        if (copy) {
          setImporting(false);
          setArchive(null);
        }
      }
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function pickArchiveToImport() {
    try {
      const picked = await open({
        multiple: false,
        title: tr("Choose a server archive"),
        filters: [{ name: tr("Archive"), extensions: ["zip"] }],
      });
      if (typeof picked !== "string") return;
      const beside = SLASH_TAIL.test(picked)
        ? picked.replace(SLASH_TAIL, "")
        : "";
      let into: string | null = null;
      if (importWhere === "keep") {
        const chosen = await open({
          directory: true,
          multiple: false,
          title: tr("Choose where to extract the server"),
          defaultPath: beside || undefined,
        });
        if (typeof chosen !== "string") return;
        into = chosen;
      }
      setModal("none");
      setImporting(true);
      setArchive({ stage: "extracting" });
      await call(
        () => commands.importInstanceArchive(picked, into),
        tr("Imported the server from the archive."),
      );
    } catch (cause) {
      setError(String(cause));
    } finally {
      setImporting(false);
      setArchive(null);
    }
  }
  async function openServerFolder(instance: Instance) {
    if (onWeb || instance.remoteHost || instance.panelHost) {
      openManage(instance);
      void loadTool("files", instance);
      return;
    }
    try {
      await commands.openServerFolder(instance.id, null);
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function createFromJar() {
    try {
      const picked = await open({
        multiple: false,
        title: tr("Choose a server core file"),
        filters: [{ name: tr("Server core"), extensions: ["jar"] }],
      });
      if (typeof picked !== "string") return;
      const name = suggestedName;
      setModal("none");
      const ok = await call(
        () => commands.createInstanceFromJar(picked),
        tr("Created {0}.", [name]),
      );
      if (ok) resetForm();
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function updateSettings(patch: Partial<AppSettings>) {
    const next = { ...settings, ...patch };
    setSettings(next);
    try {
      setSettings(
        await commands.setAppSettings(next),
      );
    } catch (cause) {
      setError(String(cause));
    }
  }
  const snoozedUpdate = useRef<string | null>(null);
  async function checkForUpdate(quiet = false) {
    setUpdateState({ stage: "checking" });
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const found = await check();
      if (!found) {
        setUpdateState({ stage: "none" });
        if (!quiet) setNotice(tr("You're on the latest version."));
        return;
      }
      if (quiet && snoozedUpdate.current === found.version) {
        setUpdateState({ stage: "none" });
        return;
      }
      setUpdateState({
        stage: "found",
        version: found.version,
      });
    } catch (cause) {
      setUpdateState({ stage: "idle" });
      if (!quiet) setError(String(cause));
    }
  }
  async function installUpdate() {
    const running = instances.filter(
      (instance) => !instance.remoteHost && instance.status === "running",
    );
    if (running.length) {
      const ok = await confirmAction(
        tr("Updating now will stop the server"),
        tr("{0} is running. Before updating, the app sends a stop command and waits up to 25 s for the server to shut down. Players will be disconnected. Continue?", [running.map((instance) => instance.name).join(", ")]),
        tr("Update anyway"),
      );
      if (!ok) return;
    }
    try {
      setError(null);
      const { check } = await import("@tauri-apps/plugin-updater");
      const found = await check();
      if (!found) {
        setUpdateState({ stage: "none" });
        return;
      }
      let total = 0;
      let done = 0;
      setUpdateState((current) => ({ ...current, stage: "downloading", percent: 0 }));
      await found.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          done += event.data.chunkLength;
          setUpdateState((current) => ({
            ...current,
            stage: "downloading",
            percent: total ? Math.round((done / total) * 100) : 0,
          }));
        } else if (event.event === "Finished") {
          setUpdateState((current) => ({ ...current, stage: "ready" }));
        }
      });
      if (running.length) {
        setUpdateState((current) => ({ ...current, stage: "stopping" }));
        await commands.stopServersForUpdate();
      }
      setUpdateState((current) => ({ ...current, stage: "ready" }));
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (cause) {
      setUpdateState({ stage: "idle" });
      setError(String(cause));
    }
  }
  async function restartInstance(instance: Instance) {
    if (
      !(await confirmAction(
        tr("Restart {0}?", [instance.name]),
        tr("Saves the world, then restarts the server. Players are briefly disconnected."),
        tr("Restart"),
      ))
    )
      return;
    await call(
      () => commands.restartInstance(instance.id),
      tr("Restarting {0}.", [instance.name]),
    );
  }
  async function forceStop(instance: Instance) {
    if (
      !(await confirmAction(
        tr("Force quit {0}?", [instance.name]),
        tr("Stops the process immediately without saving. Recent progress may be lost. Only use this if a normal stop fails."),
        tr("End task"),
        true,
      ))
    )
      return;
    await call(
      () => commands.forceStopInstance(instance.id),
      tr("Terminated {0}.", [instance.name]),
    );
  }
  async function purgeTrash(entry: TrashEntry) {
    if (
      !(await confirmAction(
        tr("Permanently delete {0}?", [entry.name]),
        tr("The server folder and its worlds are permanently deleted. This cannot be undone."),
        tr("Delete permanently"),
        true,
      ))
    )
      return;
    try {
      setTrashEntries(
        await commands.purgeTrashInstance(entry.id),
      );
      setNotice(tr("Deleted {0}.", [entry.name]));
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function emptyTrash() {
    if (
      !(await confirmAction(
        tr("Delete all {0, plural, one {# item} other {# items}} in the trash?", [trashEntries.length]),
        tr("The server folder and worlds are permanently deleted. This cannot be undone."),
        tr("Delete all"),
        true,
      ))
    )
      return;
    try {
      const count = await commands.emptyTrash();
      setTrashEntries([]);
      setNotice(tr("Deleted {0, plural, one {# item} other {# items}}.", [count]));
    } catch (cause) {
      setError(String(cause));
      void refresh();
    }
  }
  async function refreshJavaProcs() {
    try {
      setJavaProcs(await commands.listJavaProcesses());
    } catch {
      setJavaProcs([]);
    }
  }
  async function killJavaProc(proc: JavaProcess) {
    if (
      !(await confirmAction(
        tr("Terminate {0}?", [proc.label]),
        tr("Stops the process immediately without saving. The app didn't start this server, so it can't send a stop command."),
        tr("Terminate"),
        true,
      ))
    )
      return;
    try {
      await commands.killJavaProcess(proc.pid);
      setNotice(tr("Terminated {0}.", [proc.label]));
      await refreshJavaProcs();
      void refreshInstances();
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function pickBackupRoot() {
    try {
      const picked = await open({
        directory: true,
        title: tr("Choose a folder for backups"),
      });
      if (typeof picked !== "string") return;
      await updateSettings({ backupRoot: picked });
      setNotice(tr("Future backups will be stored here."));
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function pickServersRoot() {
    try {
      const picked = await open({ directory: true, title: tr("Choose a folder for servers") });
      if (typeof picked !== "string") return;
      const next = await commands.setServersRoot(picked);
      setServersRoot(next);
      setNotice(tr("New servers will be stored here."));
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function resetServersRoot() {
    try {
      setServersRoot(await commands.resetServersRoot());
      setNotice(tr("Storage location reset to default."));
    } catch (cause) {
      setError(String(cause));
    }
  }
  const [javaBusy, setJavaBusy] = useState(false);
  async function pickServerIcon() {
    if (!selected) return;
    try {
      setError(null);
      let icon: string;
      if (onWeb) {
        const [file] = await pickFiles("image/*");
        if (!file) return;
        icon = await commands.setServerIconUpload(selected.id, file.name, file.data);
      } else {
        const picked = await open({
          multiple: false,
          title: tr("Choose an image for the server icon"),
          filters: [{ name: tr("Image"), extensions: ["png", "jpg", "jpeg", "webp"] }],
        });
        if (typeof picked !== "string") return;
        icon = await commands.setServerIcon(selected.id, picked);
      }
      setServerIcons((current) => ({ ...current, [selected.id]: icon }));
      setNotice(tr("Server icon updated and saved at 64×64."));
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function clearServerIcon() {
    if (!selected) return;
    try {
      await commands.clearServerIcon(selected.id);
      setServerIcons((current) => ({ ...current, [selected.id]: "" }));
      setNotice(tr("Server icon removed."));
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function getJava() {
    if (!selected) return;
    const suggested = await commands.requiredJava(selected.id);
    const picked = await askChoice(
      tr("Choose the Java version to install"),
      tr("Downloaded from Adoptium to the Monolab folder, without a system-wide installation."),
      [8, 11, 17, 21, 25].map((major) => ({
        value: String(major),
        label: `Java ${major}`,
      })),
      String(suggested),
    );
    if (!picked) return;
    const major = Number(picked);
    setJavaBusy(true);
    try {
      setError(null);
      const runtime = await commands.downloadJava(major);
      setJavaPath(runtime.path);
      await refreshJavaList();
      setNotice(tr("Downloaded Java {0} and assigned it to this server.", [runtime.major]));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setJavaBusy(false);
    }
  }
  async function removeJava(runtime: JavaRuntime) {
    if (
      !(await confirmAction(
        tr("Remove Java {0}?", [runtime.major]),
        tr("Only removes files downloaded by Monolab. Servers using this Java switch back to automatic selection. You can download it again from the server screen."),
        tr("Remove"),
        true,
      ))
    )
      return;
    try {
      setError(null);
      setNotice(
        await commands.removeJavaRuntime(runtime.path),
      );
      await refreshJavaList();
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function refreshJavaList() {
    try {
      setJavaList(await commands.listJavaRuntimes());
    } catch {
      setJavaList([]);
    }
  }
  async function refreshRemoteJavas(id: string) {
    setRemoteJavas(null);
    try {
      setRemoteJavas(await commands.listRemoteJavas(id));
    } catch (cause) {
      setRemoteJavas([]);
      setError(String(cause));
    }
  }
  async function typeRemoteJava() {
    const typed = await askText(
      tr("Java path on the SSH server"),
      tr("Enter the full path ending in /bin/java (e.g. /opt/jdk-25/bin/java)"),
      javaPath !== "java" ? javaPath : "",
      tr("Use"),
    );
    if (typed?.trim()) setJavaPath(typed.trim());
  }
  async function pickJavaPath() {
    try {
      const picked = await open({
        multiple: false,
        title: tr("Choose java executable"),
        filters: [{ name: tr("Java executable"), extensions: ["exe"] }],
      });
      if (typeof picked === "string") setJavaPath(picked);
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function pickMrpackFile() {
    try {
      let picked: string | null = null;
      if (onWeb) {
        const [file] = await pickFiles(".mrpack,.zip");
        if (!file) return;
        picked = await commands.stashUpload(file.name, file.data);
      } else {
        const chosen = await open({
          multiple: false,
          title: tr("Choose a modpack file"),
          filters: [
            { name: tr("Modpack"), extensions: ["mrpack", "zip"] },
          ],
        });
        picked = typeof chosen === "string" ? chosen : null;
      }
      if (typeof picked !== "string") return;
      setMrpackPath(picked);
      await previewPack(picked);
    } catch (cause) {
      setError(String(cause));
    }
  }
  function enrichAddons(instance: Instance) {
    return commands.enrichAddons(instance.id)
      .then((cards) =>
        setAddonCards(
          Object.fromEntries(
            cards.map((card) => [
              card.fileName,
              {
                title: card.title,
                iconUrl: card.iconUrl,
                compatibility: card.compatibility,
                warn: card.warn,
                projectId: card.projectId,
              },
            ]),
          ),
        ),
      )
      .catch(() => undefined);
  }
  async function refreshAddons(instance = selected) {
    if (!instance) return;
    setAddons(await commands.scanAddons(instance.id));
    await enrichAddons(instance);
    void loadWaiting(instance);
  }
  async function switchLanguage(lang: string) {
    if (lang === language()) return;
    const table = await wordsFor(lang);
    const say = (key: string) => format(table[key] ?? key, { 0: languageLabels[lang] ?? lang });
    const words = { title: say("Switch to {0}?"), body: say("The app will reload."), ok: say("Change"), no: say("Cancel") };
    setAsk({
      title: words.title,
      body: words.body,
      confirmLabel: words.ok,
      cancelLabel: words.no,
      resolve: (value) => {
        if (value === null) return;
        setLanguage(lang);
        window.setTimeout(() => window.location.reload(), 200);
      },
    });
  }
  async function loadTool(next: Tool, instance = selected, keepBrowse = false) {
    if (!instance) return;
    if (!toolAllowed(instance.core, next)) next = "overview";
    if (!keepBrowse) {
      setBrowseOpen(false);
      setProjectView(null);
      setLogView("read");
      setFileQuery("");
      setPickedFiles(new Set());
      setMapOpen(false);
      setMarksOpen(false);
      setWorldSettings(null);
      setWorldDraft(null);
      setOpenCrash(null);
      setPlayerSnapshot(null);
      setPlayerTab("all");
      setAddonQuery("");
      setAddonFilter("all");
      setPickedAddons([]);
      setLogQuery("");
      setLogLevelView("all");
      setChartView("now");
      setShotView(null);
      setVersionOpen(false);
    }
    setTool(next);
    setToolLoading(true);
    try {
      if (next === "properties") {
        const read = await commands.readServerProperties(instance.id);
        setProperties(read);
        setSavedProperties(read);
        setPropertyQuery("");
        setAutomationHistory(
          await commands.listAutomationHistory(instance.id),
        );
      }
      if (next === "monitor") {
        const [nextResource, nextTps, nextHistory] = await Promise.all([
          commands.getResourceSnapshot(instance.id),
          commands.getTpsSnapshot(instance.id),
          commands.getResourceHistory(instance.id),
        ]);
        setResource(nextResource);
        setTps(nextTps);
        setHistory(nextHistory);
        void loadPerfHistory();
        void commands.addonStatus(instance.id).then(setAddon).catch(() => {});
      }
      if (next === "crashes") {
        setCrashReports(
          await commands.listCrashReports(instance.id),
        );
        setOpenCrash(null);
      }
      if (next === "players") {
        const entries = await commands.listPlayers(instance.id);
        setPlayers(entries);
        setPlayerActivity([]);
        void commands.listPlayerActivity(instance.id).then(setPlayerActivity).catch(() => setPlayerActivity([]));
        setPlayerSnapshot(null);
        const uuids = entries
          .map((player) => player.uuid)
          .filter((uuid) => uuid.length === 36);
        if (uuids.length)
          void commands.getPlayerHeads(uuids)
            .then((found) => setHeads((current) => ({ ...current, ...found })))
            .catch(() => undefined);
      }
      if (next === "addons") {
        setAddonTab(addonKind(instance.core) ?? "plugin");
        setAddons(await commands.scanAddons(instance.id));
        setAddonUpdates(null);
        setAddonQuery("");
        setAddonFilter("all");
        setPickedAddons([]);
        void checkAddonUpdates(true);
        void loadWaiting(instance);
        void commands.inspectAddons(instance.id)
          .then(setAddonReport)
          .catch(() => setAddonReport(null));
        void commands.listAddonRollbacks(instance.id)
          .then(setRollbacks)
          .catch(() => setRollbacks([]));
        void enrichAddons(instance);
      }

      if (next === "files") {
        const page = await commands.browseServerFiles(instance.id, "", null, null);
        setBrowseEntries(page.entries);
        setBrowseTotal(page.total);
        setBrowsePath("");
        setActiveFile(null);
        setFileContent("");
        setFileSaved("");
        setOpenTabs([]);
        tabBuffers.current.clear();
      }
      if (next === "worlds") {
        void commands.listBookmarks(instance.id)
          .then(setBookmarks)
          .catch(() => setBookmarks([]));
        setAddon(null);
        setMapStart(null);
        setMapSpawn(null);
        const checking = commands.addonStatus(instance.id).catch(() => null);
        const list = await commands.listWorlds(instance.id);
        setWorlds(list);
        setWorldSettings(null);
        setWorldDraft(null);
        const home = list.find((one) => one.hasLevelData && (one.realm || "overworld") === "overworld") ?? list.find((one) => one.hasLevelData);
        if (home) {
          void commands.readWorldSettings(instance.id, home.name)
            .then((found) => setMapSpawn([found.spawn[0], found.spawn[2]]))
            .catch(() => setMapSpawn(null));
        }
        const realms = list.map((one) => one.realm || "overworld");
        const first = realms.includes(mapRealm) ? mapRealm : realms.includes("overworld") ? "overworld" : realms[0] ?? "overworld";
        setMapRealm(first);
        void checking.then((status) => {
          setAddon(status ?? { supported: false, reason: "", kind: "", installed: false, installedVersion: null, latestVersion: 0, needsUpdate: false, running: false, folder: "" });
          if (status?.installed && list.length) void openMap(first, instance);
        });
      }
      if (next === "addons") {
        setAddon(
          await commands.addonStatus(instance.id),
        );
        return;
      }
      if (next === "overview") {
        if (systemMemoryMib == null) void commands.systemMemoryMib().then(setSystemMemoryMib).catch(() => {});
        setJavaNeed(null);
        void commands.requiredJava(instance.id).then(setJavaNeed).catch(() => {});
        setBuildNews(null);
        void commands.checkCoreBuild(instance.id)
          .then(setBuildNews)
          .catch(() => setBuildNews(null));
      }
      if (next === "logs") {
        setLogFiles(
          await commands.listLogFiles(instance.id),
        );
        void commands.listCrashReports(instance.id).then(setCrashReports).catch(() => setCrashReports([]));
        void commands.summarizeLogs(instance.id)
          .then(setDailyStats)
          .catch(() => setDailyStats([]));
        void commands.readLogTimeline(instance.id, "latest.log", "All")
          .then(setTimeline)
          .catch(() => setTimeline([]));
        const [entries, nextAuditEntries] = await Promise.all([
          commands.listLogEntries(instance.id, "", "All", readLogFile),
          commands.listAuditEntries(instance.id),
        ]);
        setParsedLogs(entries);
        setAuditEntries(nextAuditEntries);
      }
      if (next === "backups") {
        setBackupList(
          await commands.listBackups(instance.id).catch(
            () => [] as BackupFile[],
          ),
        );
        return;
      }
    } catch (cause) {
      setError(String(cause));
    } finally {
      setToolLoading(false);
    }
  }
  const [serverTabs, setServerTabs] = useState<string[]>(() => {
    try {
      const saved = JSON.parse(localStorage.getItem("monolab-tabs") ?? "[]");
      return Array.isArray(saved) ? saved.filter((one): one is string => typeof one === "string") : [];
    } catch {
      return [];
    }
  });
  useEffect(() => {
    if (page === "instance" && selectedId)
      setServerTabs((old) => (old.includes(selectedId) ? old : [...old, selectedId]));
  }, [page, selectedId]);
  useEffect(() => {
    try {
      localStorage.setItem("monolab-tabs", JSON.stringify(serverTabs));
    } catch {
      /* private window */
    }
  }, [serverTabs]);
  async function retrySsh(host: string) {
    await commands.retrySshHost(host).catch(() => {});
    setNotice(tr("Trying to connect again…"));
    window.setTimeout(() => void refresh(), 4000);
  }
  function goHome() {
    setModal("none");
    setTaskDraft(null);
    setSessionOpen(null);
    setPage("home");
    setSelectedId(null);
  }
  function closeServerTab(id: string) {
    const rest = serverTabs.filter((one) => one !== id);
    setServerTabs(rest);
    if (selectedId !== id) return;
    const next = rest
      .map((one) => instances.find((item) => item.id === one))
      .filter((one): one is Instance => !!one)
      .at(-1);
    if (next) openManage(next);
    else goHome();
  }
  function openManage(instance: Instance) {
    setModal("none");
    setTaskDraft(null);
    setSelectedId(instance.id);
    setMemoryMib(instance.maxMemoryMib);
    setMinMemoryMib(instance.minMemoryMib || instance.maxMemoryMib);
    setMemoryHandlesSwapped(false);
    setJvmArgs(instance.jvmArgs);
    setCustomCommand(instance.customCommand ?? "");
    setCustomMode(!!instance.customCommand?.trim());
    setScriptMode(!!instance.startScript?.trim());
    setScriptPick(instance.startScript ?? "");
    setScriptChoices(null);
    if (instance.startScript?.trim()) {
      void commands.listStartScripts(instance.id).then(setScriptChoices, () => setScriptChoices([]));
    }
    setJavaPath(instance.javaPath || "java");
    void refreshJavaList();
    if (instance.remoteHost) void refreshRemoteJavas(instance.id);
    else setRemoteJavas(null);
    void loadConsoleLogs(instance);
    setUpgradeVersion(instance.version);
    setPage("instance");
    void loadTool(instance.status === "running" || instance.status === "stopping" ? "console" : "overview", instance);
  }
  async function openConsole(instance: Instance) {
    setSelectedId(instance.id);
    setPage("instance");
    setModal("none");
    void loadTool("console", instance);
    void commands.getRconStatus(instance.id)
      .then((status) => {
        setUseRcon(
          status.enabled &&
            status.hasPassword &&
            instance.status !== "running",
        );
      })
      .catch(() => setUseRcon(false));
    await loadConsoleLogs(instance);
  }
  async function loadConsoleLogs(instance: Instance) {
    try {
      const stored = await commands.getConsoleLogs(instance.id);
      logStore.replace(instance.id, stored);
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function create() {
    const before = new Set(instancesRef.current.map((instance) => instance.id));
    const operationId = crypto.randomUUID();
    const name = suggestedName;
    setCreating(true);
    setCreateOperationId(operationId);
    setDownload({ stage: "resolving", core: form.core, version: form.version });
    setModal("none");
    const ok = await call(
      () =>
        commands.createInstance({
            ...form,
            minMemoryMib: form.maxMemoryMib,
            name: "",
            eulaAccepted: true,
            operationId,
          }),
      tr("Created {0}.", [name]),
    );
    setCreating(false);
    setCreateOperationId(null);
    setDownload(null);
    if (!ok) return;
    if (!(await moveToChosenHost(before, name))) return;
    celebrateCreatedServer(before);
    resetForm();
  }
  async function moveToChosenHost(before: Set<string>, name: string) {
    const host = createWhere;
    if (host === "local") return true;
    await refresh();
    const made = instancesRef.current.find((instance) => !before.has(instance.id));
    if (!made) return true;
    const moved = await call(() => commands.moveInstanceToHost(made.id, host), tr("Moved {0} to the SSH server.", [name]));
    await refresh();
    return !!moved;
  }
  async function createCustom(folder: string, port: number, program: CustomProgram) {
    const before = new Set(instancesRef.current.map((instance) => instance.id));
    const ok = await call(() => commands.createCustomServer(folder, "", port, program), tr("Server added."));
    if (!ok) return false;
    setModal("none");
    celebrateCreatedServer(before);
    resetForm();
    return true;
  }
  async function createFromModpack(hit: CatalogHit) {
    const before = new Set(instancesRef.current.map((instance) => instance.id));
    const operationId = crypto.randomUUID();
    setCreating(true);
    setPackCreating(hit.title);
    setCreateOperationId(operationId);
    setDownload({ stage: "pack", core: "", version: "" });
    setModal("none");
    const ok = await call(
      () =>
        commands.createFromModpack({ projectId: hit.projectId, path: null, operationId, maxMemoryMib: form.maxMemoryMib }),
      tr("Created server {0}!", [hit.title]),
    );
    setCreating(false);
    setPackCreating(null);
    setCreateOperationId(null);
    setDownload(null);
    if (ok && (await moveToChosenHost(before, hit.title))) {
      celebrateCreatedServer(before);
      resetForm();
    }
  }
  function celebrateCreatedServer(before: Set<string>) {
    const created = instancesRef.current.find((instance) => !before.has(instance.id));
    if (!created) return;
    const ordered = [
      ...instancesRef.current.filter((instance) => instance.id !== created.id),
      created,
    ];
    instancesRef.current = ordered;
    setInstances(ordered);
    void commands.reorderInstances(ordered.map((instance) => instance.id));
    setPage("home");
    setArrivingServerId(created.id);
  }
  useEffect(() => {
    if (!arrivingServerId) return;
    const frame = window.requestAnimationFrame(() => {
      document.querySelector(`[data-server-id="${CSS.escape(arrivingServerId)}"]`)?.scrollIntoView({ block: "nearest" });
      setArrivingServerId(null);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [arrivingServerId]);
  async function createFromModpackFile() {
    const before = new Set(instancesRef.current.map((instance) => instance.id));
    try {
      let path: string | null = null;
      if (onWeb) {
        const [file] = await pickFiles(".mrpack");
        if (!file) return;
        path = await commands.stashUpload(file.name, file.data);
      } else {
        const picked = await open({
          multiple: false,
          title: tr("Choose a modpack file"),
          filters: [{ name: tr("Modrinth modpack"), extensions: ["mrpack"] }],
        });
        path = typeof picked === "string" ? picked : null;
      }
      if (!path) return;
      const operationId = crypto.randomUUID();
      setCreating(true);
      setPackCreating(tr("Selected modpack"));
      setCreateOperationId(operationId);
      setDownload({ stage: "pack", core: "", version: "" });
      setModal("none");
      const ok = await call(
        () => commands.createFromModpack({ projectId: "", path, operationId, maxMemoryMib: form.maxMemoryMib }),
        tr("Modpack server created!"),
      );
      setCreating(false);
      setPackCreating(null);
      setCreateOperationId(null);
      setDownload(null);
      if (ok && (await moveToChosenHost(before, tr("Selected modpack")))) {
        celebrateCreatedServer(before);
        resetForm();
      }
    } catch (cause) {
      setCreating(false);
      setPackCreating(null);
      setCreateOperationId(null);
      setDownload(null);
      setError(String(cause));
    }
  }
  async function cancelCreate() {
    if (!createOperationId) return;
    await commands.cancelCreateDownload(createOperationId);
    setNotice(tr("Download cancellation requested."));
  }
  async function toggle(instance: Instance, skipCheck = false) {
    if (instance.checking) {
      setNotice(tr("Still checking whether {0} is running — try again in a moment.", [instance.name]));
      return;
    }
    if (instance.missing && instance.status !== "running") {
      setError(tr("The folder of {0} is missing. Put it back, or remove the server from the list.", [instance.name]));
      return;
    }
    const starting = instance.status !== "running";
    if (starting && !skipCheck) {
      const found = await commands.precheckInstance(instance.id).catch(() => [] as Finding[]);
      if (found.length) {
        setFindings(found);
        setModal("precheck");
        return;
      }
    }
    if (starting) {
      setStartingServer({ id: instance.id, name: instance.name, at: Date.now() });
      logStore.clear(instance.id);
    }
    if (!starting) stoppingRef.current.set(instance.id, instance.name);
    showPending(
      starting
        ? tr("Starting {0}…", [instance.name])
        : tr("Stopping {0}.", [instance.name]),
    );
    const ok = await call(() =>
      instance.status === "running"
        ? commands.stopInstance(instance.id)
        : commands.startInstance(instance.id),
    );
    if (!ok) {
      setStartingServer(null);
      stoppingRef.current.delete(instance.id);
      dismissNotice();
    }
    if (starting) void loadTool("console", instance);
  }
  async function renameInstance(instance: Instance) {
    const name = (
      await askText(tr("Rename server"), tr("New name"), instance.name, tr("Rename"))
    )?.trim();
    if (!name || name === instance.name) return;
    await call(
      () => commands.renameInstance({ id: instance.id, name }),
      tr("Renamed the server to {0}.", [name]),
    );
  }
  async function duplicateInstance(instance: Instance) {
    if (
      !(await confirmAction(
        tr("Duplicate {0}?", [instance.name]),
        tr("Copies the entire server folder except backups. Assigns the copy an unused port and saves it in the configuration."),
        tr("Duplicate"),
      ))
    )
      return;
    await call(
      () => commands.duplicateInstance(instance.id).then((copy) => setArrivingServerId(copy.id)),
      tr("Duplicate created!"),
      tr("Duplicating…"),
    );
  }
  async function trashInstance(instance: Instance) {
    if (instance.linked || instance.remoteHost) {
      if (!(await confirmAction(
        tr("Remove {0} from the list?", [instance.name]),
        tr("Only the entry is removed. The server folder and files stay where they are: {0}", [instance.path]),
        tr("Remove from list"),
      ))) return;
      const ok = await call(
        () => commands.trashInstance(instance.id),
        tr("Removed {0} from the list. The folder is untouched.", [instance.name]),
      );
      if (ok && selected?.id === instance.id) {
        setSelectedId(null);
        setPage("home");
      }
      return;
    }
    if (!(await confirmAction(
      tr("Move {0} to the trash?", [instance.name]),
      tr("Moves the server folder and settings to the trash. Restore them later from the sidebar."),
      tr("Move to trash"),
    ))) return;
    const ok = await call(
      () => commands.trashInstance(instance.id),
      tr("Server moved to the app's trash."),
    );
    if (ok && selected?.id === instance.id) {
      setSelectedId(null);
      setPage("home");
    }
  }
  async function restoreTrashInstance(entry: TrashEntry) {
    if (
      !(await confirmAction(
        tr("Restore {0}?", [entry.name]),
        tr("Restores the folder and registers the server again with its previous memory, JVM flags and backup schedule."),
        tr("Restore"),
      ))
    )
      return;
    let restored: Instance | null = null;
    const ok = await call(
      async () => {
        restored = await commands.restoreTrashInstance(entry.id);
      },
      tr("Restored {0}.", [entry.name]),
    );
    if (!ok || !restored) return;
    const view = restored as Instance;
    setPage("home");
    setArrivingServerId(view.id);
  }
  function grabInstance(event: React.PointerEvent, id: string) {
    if (event.button !== 0 || filter) return;
    if ((event.target as HTMLElement).closest("button, a, input, label")) return;
    const row = event.currentTarget as HTMLElement;
    const rect = row.getBoundingClientRect();
    grabRef.current = {
      id,
      y: event.clientY,
      moved: false,
      offsetY: event.clientY - rect.top,
      offsetX: event.clientX - rect.left,
      left: rect.left,
      width: rect.width,
      html: row.innerHTML,
      className: row.className,
    };
  }
  function dragInstance(event: PointerEvent) {
    const grab = grabRef.current;
    if (!grab) return;
    if (!grab.moved) {
      if (Math.abs(event.clientY - grab.y) < 5 && Math.abs(event.clientX - (grab.left + grab.offsetX)) < 5) return;
      grab.moved = true;
      setDraggedInstanceId(grab.id);
    }
    setDragY(event.clientY);
    setDragX(event.clientX);
    const row = (
      document.elementFromPoint(event.clientX, event.clientY) as HTMLElement | null
    )?.closest<HTMLElement>("[data-server-id]");
    const overId = row?.dataset.serverId;
    if (!overId || overId === grab.id) return;
    setInstances((current) => {
      const from = current.findIndex((one) => one.id === grab.id);
      const to = current.findIndex((one) => one.id === overId);
      if (from < 0 || to < 0 || from === to) return current;
      const next = [...current];
      const [moved] = next.splice(from, 1);
      next.splice(to, 0, moved);
      return next;
    });
  }
  function dropInstance() {
    const grab = grabRef.current;
    grabRef.current = null;
    if (!grab?.moved) return;
    setDraggedInstanceId(null);
    setDragY(null);
    setDragX(null);
    draggedJustNow.current = true;
    window.setTimeout(() => {
      draggedJustNow.current = false;
    }, 0);
    void saveInstanceOrder();
  }
  async function saveInstanceOrder() {
    try {
      await commands.reorderInstances(instancesRef.current.map((one) => one.id));
    } catch (cause) {
      setError(String(cause));
      void refresh();
    }
  }
  async function sendConsoleCommand(value = command) {
    if (!selected || !value.trim()) return;
    const typed = value.trim();
    const sent = typed.replace(/^\//, "");
    setHeldHints(null);
    setHintAt(0);
    const ok = await call(async () => {
      if (useRcon) {
        const output = await commands.sendRconCommand(selected.id, sent);
        logStore.append([
          { serverId: selected.id, line: sent, stream: "input", noise: false, ansi: "" },
          ...(output.trim()
            ? output
                .split("\n")
                .map((line) => ({
                  serverId: selected.id,
                  noise: false,
                  ansi: "",
                  line,
                  stream: "stdout",
                }))
            : []),
        ]);
        return;
      }
      await commands.sendConsoleCommand(selected.id, sent);
    });
    if (!ok) return;
    setCommand("");
    setConsoleHistory((current) => {
      const next = [sent, ...current.filter((item) => item !== sent)].slice(
        0,
        30,
      );
      localStorage.setItem(
        "monolab-console-history",
        JSON.stringify(next),
      );
      return next;
    });
  }
  const livePeriod = period(1_000, attention);
  const liveId = selected?.id ?? null;
  useEffect(() => {
    const wants =
      tool === "monitor" ||
      tool === "worlds" ||
      tool === "players";
    if (!wants || !liveId) return;
    const read = () =>
      void commands.readLive(liveId)
        .then(setLive)
        .catch(() => setLive(null));
    read();
    if (livePeriod === null) return;
    const timer = window.setInterval(read, livePeriod);
    return () => window.clearInterval(timer);
  }, [tool, liveId, livePeriod]);
  const lagPeriod = period(15_000, attention);
  const selectedRunning = selected?.status === "running" ? selected.id : null;
  useEffect(() => {
    if (tool !== "monitor" || !selectedRunning) {
      setLagReport(null);
      return;
    }
    const read = () =>
      void commands.diagnoseLag(selectedRunning)
        .then(setLagReport)
        .catch(() => undefined);
    read();
    if (lagPeriod === null) return;
    const timer = window.setInterval(read, lagPeriod);
    return () => window.clearInterval(timer);
  }, [tool, selectedRunning, lagPeriod]);
  const sparkReport = lagReport?.spark?.report ?? null;
  const sparkBusy = sparkStartedAt !== null && !(sparkReport && sparkReport.at >= sparkStartedAt);
  useEffect(() => {
    if (!sparkBusy || !selectedRunning) return;
    const timer = window.setInterval(() => {
      if (sparkStartedAt !== null && Date.now() - sparkStartedAt > 90_000) {
        setSparkStartedAt(null);
        return;
      }
      void commands.diagnoseLag(selectedRunning).then(setLagReport).catch(() => undefined);
    }, 5_000);
    return () => window.clearInterval(timer);
  }, [sparkBusy, sparkStartedAt, selectedRunning]);
  async function startSpark() {
    if (!selectedRunning) return;
    setSparkStartedAt(Date.now());
    await sendConsoleCommand("spark profiler start --timeout 30");
    setNotice(tr("spark will profile for 30 seconds. A results button appears here when it finishes."));
  }
  async function installSpark() {
    if (!selected) return;
    const ok = await call(
      () => commands.installModrinth(selected.id, SPARK_MODRINTH_ID, "mod", null),
      tr("spark installed. It applies on the next start."),
      tr("Downloading spark."),
    );
    if (ok && selectedRunning)
      void commands.diagnoseLag(selectedRunning).then(setLagReport).catch(() => undefined);
  }
  async function runLagAction(action: LagAction, key: string) {
    if (action.kind === "overview" || action.kind === "properties") {
      void loadTool(action.kind);
      return;
    }
    if (action.kind === "copy") {
      await navigator.clipboard.writeText(action.command).catch(() => {});
      flashCopied(key);
      setNotice(tr("Command copied! Paste it into the in-game chat."));
      return;
    }
    const ok = await confirmAction(tr("Remove entities?"), action.ask, tr("Remove"), true);
    if (!ok) return;
    await sendConsoleCommand(action.command);
    const id = selectedRunning;
    if (id)
      window.setTimeout(() => {
        void commands.diagnoseLag(id).then(setLagReport).catch(() => undefined);
      }, 2500);
  }
  useEffect(() => {
    const names = (live?.players ?? [])
      .map((one) => one.name)
      .filter((name) => !(name in liveHeads));
    if (!names.length) return;
    void commands.getHeadsByName(names)
      .then((found) =>
        setLiveHeads((current) => {
          const next = { ...current };
          for (const name of names) next[name] = found[name] ?? "";
          return next;
        }),
      )
      .catch(() => undefined);
  }, [live, liveHeads]);
  async function loadItemNames(instance: Instance, ids: string[]) {
    const fresh = [...new Set(ids)].filter((id) => !(id in itemNames));
    if (!fresh.length) return;
    const found = await commands.itemNames(instance.version, fresh).catch(() => ({}) as Record<string, string>);
    if (Object.keys(found).length) setItemNames((was) => ({ ...was, ...found }));
  }
  async function loadEnchantNames(instance: Instance, ids: string[]) {
    const fresh = [...new Set(ids)].filter((id) => !(id in enchantNames));
    if (!fresh.length) return;
    const found = await commands.enchantmentNames(instance.version, fresh).catch(() => ({}) as Record<string, string>);
    if (Object.keys(found).length) setEnchantNames((was) => ({ ...was, ...found }));
  }
  async function loadItemIcons(instance: Instance, ids: string[]) {
    const fresh = [...new Set(ids)].filter((id) => !(id in itemIcons));
    if (!fresh.length) return;
    const found = await commands.itemIcons(instance.version, fresh).catch(() => ({}) as Record<string, string>);
    setItemIcons((was) => {
      const next = { ...was };
      for (const id of fresh) next[id] = found[id] ?? "";
      return next;
    });
  }
  async function makeBackup() {
    if (!selected) return;
    const instance = selected;
    if (
      instance.status === "running" &&
      !(await confirmAction(
        tr("The server is running. Back up anyway?"),
        tr("World files may still be saving, so the last few minutes of a restored backup could be inconsistent. Stop the server before backing up for a reliable backup."),
        tr("Back up as is"),
      ))
    ) {
      return;
    }
    setBackupBusy(true);
    await call(
      async () => {
        await commands.createBackup(instance.id, "Manual");
        setBackupList(
          await commands.listBackups(instance.id),
        );
      },
      tr("Backed up!"),
      tr("Backing up…"),
    );
    setBackupBusy(false);
  }
  async function restoreBackup(one: BackupFile) {
    if (!selected) return;
    const instance = selected;
    if (
      !(await confirmAction(
        tr("Restore the backup from {0}?", [date(one.createdAt)]),
        tr("Replaces the server folder with this backup. The current state is backed up first, so you can restore it if you choose the wrong backup."),
        tr("Restore"),
      ))
    ) {
      return;
    }
    setBackupBusy(true);
    await call(
      async () => {
        await commands.restoreBackup(instance.id, one.fileName);
        setBackupList(
          await commands.listBackups(instance.id),
        );
      },
      tr("Restored from the backup!"),
      tr("Restoring the backup…"),
    );
    setBackupBusy(false);
  }
  async function deleteBackup(one: BackupFile) {
    if (!selected) return;
    const instance = selected;
    if (
      !(await confirmAction(
        tr("Delete this backup?"),
        tr("Deleted backups cannot be recovered."),
        tr("Delete"),
        true,
      ))
    ) {
      return;
    }
    await call(
      async () => {
        await commands.deleteBackup(instance.id, one.fileName);
        setBackupList(
          await commands.listBackups(instance.id),
        );
      },
      tr("Backup deleted."),
    );
  }
  async function loadBookmarks() {
    if (!selected) return;
    setBookmarks(
      await commands.listBookmarks(selected.id).catch(
        () => [] as Bookmark[],
      ),
    );
  }
  async function addAddonFiles(paths: string[]) {
    if (!selected || !paths.length) return;
    const native = isNative(selected.core);
    const jars = paths.filter((one) => {
      const low = one.toLowerCase();
      return native
        ? low.endsWith(".wasm") || low.endsWith(".dll")
        : low.endsWith(".jar");
    });
    if (!jars.length) {
      setError(
        native
          ? tr("Only Pumpkin plugin (.wasm) files can be added.")
          : tr("Only .jar files can be added."),
      );
      return;
    }
    const instance = selected;
    await call(() =>
      commands.addAddonFiles(instance.id, jars).then((result) => setNotice(sayWhatHappened(result))),
    );
    await refreshAddons();
    void loadWaiting();
  }
  async function pickAddonFiles() {
    if (onWeb) {
      if (!selected) return;
      const files = await pickFiles(isNative(selected.core) ? ".wasm,.dll" : ".jar", true);
      if (!files.length) return;
      const instance = selected;
      await call(() =>
        commands.addAddonUpload(instance.id, files).then((result) => {
          setNotice(sayWhatHappened(result));
          void loadTool("addons", instance);
          void loadWaiting(instance);
        }),
      );
      return;
    }
    if (!selected) return;
    const picked = await open({
      multiple: true,
      filters: [
        isNative(selected.core)
          ? { name: tr("Pumpkin plugin"), extensions: ["wasm", "dll"] }
          : { name: tr("Mods & plugins"), extensions: ["jar"] },
      ],
    });
    if (!picked) return;
    await addAddonFiles(Array.isArray(picked) ? picked : [picked]);
  }
  async function installAddon() {
    if (!selected) return;
    const instance = selected;
    await call(
      () =>
        commands.installAddon(instance.id).then(setAddon),
      tr("Installed Monolab {0}. Start the server to see data in the Monitor tab.", [addon?.kind ?? tr("add-on")]),
    );
  }
  async function removeAddon() {
    if (!selected) return;
    const instance = selected;
    if (
      !(await confirmAction(
        tr("Remove Monolab {0}?", [addon?.kind ?? tr("add-on")]),
        tr("Only removes the installed file. TPS, player data and map positions will no longer be shown."),
        tr("Remove"),
      ))
    )
      return;
    await call(
      () =>
        commands.removeAddon(instance.id).then(setAddon),
      tr("Removed Monolab {0}.", [addon?.kind ?? tr("add-on")]),
    );
  }
  async function updateGameFiles() {
    if (!selected) return;
    const instance = selected;
    setGameUpdating(true);
    await call(() => commands.updateGameServer(instance.id), tr("Updated the {0} server files.", [instance.core]));
    setGameUpdating(false);
  }
  async function openVersionDialog() {
    if (!selected) return;
    const instance = selected;
    setVersionOpen(true);
    setUpgradeVersion(instance.version);
    setUpgradeBuilds([]);
    setUpgradeBuild("");
    void commands.listUpgradeHistory(instance.id).then(setUpgradeHistory).catch(() => setUpgradeHistory([]));
    setUpgradeSnapshots(false);
    void commands.listCoreVersions(instance.core, false).then(setUpgradeVersions).catch(() => setUpgradeVersions([]));
    void loadUpgradeBuilds(instance.version);
  }
  async function loadUpgradeBuilds(version: string) {
    if (!selected || isProxy(selected.core)) {
      setUpgradeBuilds([]);
      setUpgradeBuild("");
      return;
    }
    setUpgradeBuildsBusy(true);
    try {
      const list = await commands.listCoreBuilds(selected.core, version);
      setUpgradeBuilds(list);
      const first = list.find((one) => one.stable) ?? list[0];
      setUpgradeBuild(first?.id ?? "");
    } catch {
      setUpgradeBuilds([]);
      setUpgradeBuild("");
    } finally {
      setUpgradeBuildsBusy(false);
    }
  }
  async function saveLaunchSettings(quiet = false) {
    if (!selected) return;
    const save = () =>
      commands.updateLaunchSettings({
          id: selected.id,
          maxMemoryMib: Number(memoryMib),
          minMemoryMib: Number(minMemoryMib),
          jvmArgs: jvmArgs.trim(),
          javaPath: javaPath.trim() || "java",
        });
    if (quiet) {
      try {
        await save();
        await refresh();
      } catch {}
      return;
    }
    await call(save, tr("Launch settings saved!"));
  }
  useEffect(() => {
    if (!selected || tool !== "overview" || selected.status !== "stopped") return;
    const timer = window.setTimeout(() => void saveLaunchSettings(true), 600);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [memoryMib, minMemoryMib, jvmArgs, javaPath]);
  useEffect(() => {
    if (onWeb || !selected || (tool !== "addons" && tool !== "files") || modal === "sftp") {
      setDropping(false);
      return;
    }
    let stop: (() => void) | undefined;
    let gone = false;
    void (async () => {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === "over") {
          setDropping(true);
        } else if (event.payload.type === "drop") {
          setDropping(false);
          if (tool === "files") void addDroppedFiles(event.payload.paths);
          else void addAddonFiles(event.payload.paths);
        } else {
          setDropping(false);
        }
      });
      if (gone) unlisten();
      else stop = unlisten;
    })();
    return () => {
      gone = true;
      stop?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected?.id, tool, modal]);
  async function loadPlayit() {
    try {
      setPlayit(await commands.playitState());
    } catch {}
  }
  async function beginPlayitClaim() {
    setPlayitBusy(true);
    try {
      const url = await commands.playitClaim();
      await loadPlayit();
      await openExternal(url);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setPlayitBusy(false);
    }
  }
  async function setPlayitExposed(on: boolean) {
    if (!selected) return;
    if (
      on &&
      !(await confirmAction(
        tr("This server will be public"),
        tr("Anyone with the address can try to join. Keep the whitelist on and don't disable online-mode. This address only exposes the Minecraft port, not the web panel."),
        tr("I understand, open it"),
      ))
    ) {
      return;
    }
    setPlayitBusy(true);
    try {
      const next = await (on
        ? commands.playitExpose(selected.id)
        : commands.playitUnexpose(selected.id));
      setPlayit(next);
      setNotice(on ? tr("Public access enabled. Your address will appear shortly.") : tr("Public access disabled."));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setPlayitBusy(false);
    }
  }
  async function forgetPlayit() {
    const yes = await confirmAction(
      tr("Unlink playit?"),
      tr("Deletes the secret key stored on this computer and stops the daemon. Public servers won't be accessible until you link again."),
      tr("Unlink"),
      true,
    );
    if (!yes) return;
    try {
      setPlayit(await commands.playitForget());
    } catch (cause) {
      setError(String(cause));
    }
  }
  function playitCard() {
    if (!selected) return null;
    return (
      <div className="setting-card">
        {(() => {
          const tunnel = playit?.tunnels.find((one) => one.localPort === selected.port);
          const claim = playit?.claim;
          const status = !playit
            ? tr("Loading…")
            : !playit.hasSecret
              ? claim?.state === "waiting"
                ? tr("Approve it in your browser.")
                : claim?.state === "rejected"
                  ? tr("Approval was declined in the browser. You can try again.")
                  : claim?.state === "expired"
                    ? tr("Approval timed out. Try again.")
                    : claim?.state === "error"
                      ? `${tr("Connection failed:")} ${claim.error}`
                      : tr("Connect playit.gg so friends can join without exposing your home IP or setting up port forwarding.")
              : tunnel
                ? tunnel.disabled
                  ? tr("This tunnel is disabled in the playit dashboard.")
                  : playit.running
                    ? tr("playit connected")
                    : tr("playit is off — starting a server turns it back on.")
                : tr("Public access is off. Enable it to get an address for friends to join.");
          return (
            <div className="setting-row playit-row">
              <div>
                <strong>
                  {tr("Public address")}
                  <small className="muted"> playit.gg</small>
                </strong>
                {tunnel ? (
                  <button
                    className="playit-address"
                    disabled={!tunnel.address}
                    data-tip={tr("Click to copy")}
                    onClick={() => {
                      void navigator.clipboard.writeText(tunnel.address);
                      setNotice(tr("Address copied!"));
                    }}
                  >
                    <code>{tunnel.address || tr("Assigning an address…")}</code>
                    <Icon name="copy" />
                    <small>{status}{playit?.error ? ` · ${playit.error}` : ""}</small>
                  </button>
                ) : (
                  <small data-tip={status}>
                    {status}
                    {claim?.state === "waiting" && (
                      <>
                        {" "}
                        <button className="playit-url" onClick={() => void openExternal(claim.url)}>
                          {claim.url}
                        </button>
                      </>
                    )}
                    {playit?.error ? ` · ${playit.error}` : ""}
                  </small>
                )}
              </div>
              {playit && !playit.hasSecret && (
                <button
                  className="button secondary small"
                  disabled={playitBusy || claim?.state === "waiting"}
                  onClick={() => void beginPlayitClaim()}
                >
                  <Icon name="cloud" />
                  {claim?.state === "waiting" ? tr("Waiting for approval…") : tr("Link playit")}
                </button>
              )}
              {playit?.hasSecret && (
                <>
                  <button
                    className="button secondary small"
                    disabled={playitBusy}
                    onClick={() => void setPlayitExposed(!tunnel)}
                  >
                    <Icon name={tunnel ? "stop" : "cloud"} />
                    {playitBusy ? tr("One moment…") : tunnel ? tr("Disable public access") : tr("Enable public access")}
                  </button>
                  <IconButton
                    className="small"
                    label={tr("Unlink playit")}
                    icon="exit"
                    onClick={() => void forgetPlayit()}
                  />
                </>
              )}
            </div>
          );
        })()}
      </div>
    );
  }
  async function setStartWithApp(on: boolean) {
    if (!selected) return;
    await call(
      () => commands.setStartWithApp(selected.id, on),
      on ? tr("Starts with the app.") : tr("Doesn't start with the app."),
    );
  }
  async function setCrashRestart(on: boolean) {
    if (!selected) return;
    await call(
      () => commands.setCrashRestart(selected.id, on),
      on ? tr("Restarts after a crash.") : tr("Doesn't restart after a crash."),
    );
  }
  async function applyAikarPreset() {
    if (
      jvmArgs.trim() &&
      jvmArgs.trim() !== aikarJvmArgs &&
      !(await confirmAction(
        tr("Replace the JVM flags with Aikar's preset?"),
        tr("Replaces all current flags with the preset. Copy any custom flags first. Changes won't apply until you select Save."),
        tr("Use preset"),
      ))
    )
      return;
    setJvmArgs(aikarJvmArgs);
  }
  async function saveProperties() {
    if (!selected) return;
    const ok = await call(
      () => commands.saveServerProperties(selected.id, properties),
      tr("Settings saved!"),
    );
    if (ok) setSavedProperties(properties);
  }
  useEffect(() => {
    if (tool !== "logs" || logView !== "read" || readLogFile !== "latest.log") return;
    if (!selected || selected.status !== "running") return;
    const every = period(3000, attention);
    if (every === null) return;
    const id = selected.id;
    const timer = window.setInterval(() => {
      void commands.listLogEntries(id, "", "All", readLogFile).then(setParsedLogs).catch(() => {});
    }, every);
    return () => window.clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tool, logView, readLogFile, selected?.id, selected?.status, attention]);
  async function refreshParsedLogs(level = logLevel, fileName = readLogFile) {
    if (!selected) return;
    try {
      const [entries, nextAuditEntries] = await Promise.all([
        commands.listLogEntries(selected.id, "", level, fileName),
        commands.listAuditEntries(selected.id),
      ]);
      setParsedLogs(entries);
      setAuditEntries(nextAuditEntries);
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function enableDesktopAlerts() {
    try {
      setError(null);
      let allowed = await isPermissionGranted();
      if (!allowed) {
        allowed = (await requestPermission()) === "granted";
      }
      if (!allowed) {
        setError(
          tr("Windows is blocking notifications. Enable Monolab under Settings > System > Notifications."),
        );
        return;
      }
      localStorage.setItem("monolab-desktop-alerts", "enabled");
      setDesktopAlertsEnabled(true);
      sendNotification({
        title: "Monolab",
        body: tr("This is a sample alert. Alerts appear when a server stops or a task sends a notification."),
      });
      setNotice(tr("Windows notifications are on."));
    } catch (cause) {
      setError(String(cause));
    }
  }
  function disableDesktopAlerts() {
    localStorage.removeItem("monolab-desktop-alerts");
    setDesktopAlertsEnabled(false);
    setNotice(tr("Windows notifications are off."));
  }

  const upgradeUnchanged =
    !!selected &&
    upgradeVersion === selected.version &&
    (!upgradeBuild || upgradeBuild === selected.coreBuild);
  async function upgradeInstance(backup: boolean) {
    if (!selected || !upgradeVersion || upgradeUnchanged) return;
    const instance = selected;
    const sameVersion = upgradeVersion === instance.version;
    const buildWord = upgradeBuild ? ` · ${tr("Build")} ${upgradeBuild}` : "";
    if (
      !(await confirmAction(
        sameVersion
          ? tr("Switch to build {0}?", [upgradeBuild])
          : tr("Update {0} to {1}?", [instance.name, upgradeVersion + buildWord]),
        backup
          ? tr("The entire server folder is backed up first, and the old core file is kept for rollback. Mods and plugins may not support the new version. Check compatibility before starting.")
          : tr("Replaces only the core file, without backing up the server folder. The old core file is kept for rollback, but worlds and settings aren't backed up. Mods and plugins may not support the new version; check before starting."),
        backup ? tr("Back up & change") : tr("Change"),
      ))
    )
      return;
    setUpgradeBusy(true);
    const ok = await call(
      () =>
        commands.upgradeInstance({
          id: instance.id,
          targetVersion: upgradeVersion,
          targetBuild: upgradeBuild || null,
          skipBackup: !backup,
        }),
      backup
        ? tr("Backed up the server and replaced the core file. Check mod and plugin compatibility before starting.")
        : tr("Core file replaced. Check mod and plugin compatibility before starting."),
      backup ? tr("Backing up and replacing the core…") : tr("Replacing the core…"),
    );
    setUpgradeBusy(false);
    if (ok) {
      setVersionOpen(false);
      await refresh();
      setBuildNews(null);
      void commands.checkCoreBuild(instance.id).then(setBuildNews).catch(() => setBuildNews(null));
    }
  }
  async function rollbackUpgrade() {
    if (!selected || !upgradeHistory.length) return;
    if (
      !(await confirmAction(
        tr("Roll back the latest upgrade?"),
        tr("Creates a full backup first, then restores the previous core file. Worlds and settings stay unchanged."),
        tr("Roll back"),
      ))
    )
      return;
    const ok = await call(
      () => commands.rollbackLastUpgrade(selected.id),
      tr("Backed up the current state and restored the previous jar."),
    );
    if (ok) {
      setVersionOpen(false);
      await refresh();
    }
  }
  async function browseTo(path: string) {
    if (!selected) return;
    try {
      setError(null);
      const page = await commands.browseServerFiles(selected.id, path, null, null);
      setBrowseEntries(page.entries);
      setBrowseTotal(page.total);
      setBrowsePath(path);
      setFileQuery("");
      setPickedFiles((now) => new Set([...now].filter((one) => page.entries.some((entry) => entry.path === one))));
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function browseMore() {
    if (!selected || browseMoreBusy) return;
    setBrowseMoreBusy(true);
    try {
      const page = await commands.browseServerFiles(
        selected.id,
        browsePath,
        browseEntries.length,
        null,
      );
      const seen = new Set(browseEntries.map((entry) => entry.path));
      setBrowseEntries((now) => now.concat(page.entries.filter((entry) => !seen.has(entry.path))));
      setBrowseTotal(page.total);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBrowseMoreBusy(false);
    }
  }
  function stashActive() {
    if (activeFile) tabBuffers.current.set(activeFile, { content: fileContent, saved: fileSaved, image: fileImage });
  }
  function showBuffer(path: string) {
    const kept = tabBuffers.current.get(path);
    tabBuffers.current.delete(path);
    setActiveFile(path);
    setFileContent(kept?.content ?? "");
    setFileSaved(kept?.saved ?? "");
    setFileImage(kept?.image ?? null);
  }
  function clearActive() {
    setActiveFile(null);
    setFileImage(null);
    setFileContent("");
    setFileSaved("");
  }
  function dirtyTabs(): string[] {
    const out: string[] = [];
    if (activeFile && fileContent !== fileSaved) out.push(activeFile);
    for (const [path, one] of tabBuffers.current) if (one.content !== one.saved) out.push(path);
    return out;
  }
  async function mayLeaveFile(): Promise<boolean> {
    const dirty = dirtyTabs();
    if (!dirty.length) return true;
    return confirmAction(
      tr("Leave without saving?"),
      tr("Your changes to {0} will be lost.", [dirty.map((one) => one.split("/").pop()).join(", ")]),
      tr("Exit"),
      true,
    );
  }
  async function goFolder(path: string) {
    await browseTo(path);
  }
  async function closeFile() {
    if (!(await mayLeaveFile())) return;
    tabBuffers.current.clear();
    setOpenTabs([]);
    clearActive();
  }
  function forgetTab(path: string) {
    const rest = openTabs.filter((one) => one !== path);
    setOpenTabs(rest);
    if (path !== activeFile) {
      tabBuffers.current.delete(path);
      return;
    }
    const at = openTabs.indexOf(path);
    const next = rest[Math.min(Math.max(at, 0), rest.length - 1)];
    if (next) showBuffer(next);
    else clearActive();
  }
  async function closeTab(path: string) {
    const kept = path === activeFile ? { content: fileContent, saved: fileSaved } : tabBuffers.current.get(path);
    if (
      kept &&
      kept.content !== kept.saved &&
      !(await confirmAction(
        tr("Close without saving?"),
        tr("Your changes to {0} will be lost.", [path.split("/").pop() ?? path]),
        tr("Close"),
        true,
      ))
    )
      return;
    forgetTab(path);
  }
  function switchTab(path: string) {
    if (path === activeFile) return;
    stashActive();
    showBuffer(path);
  }
  async function openFile(path: string) {
    if (!selected) return;
    if (path === activeFile) return;
    if (tabBuffers.current.has(path)) {
      switchTab(path);
      return;
    }
    try {
      if (isImage(path)) {
        const url = await commands.readServerImage(selected.id, path);
        stashActive();
        setActiveFile(path);
        setFileImage(url);
        setFileContent("");
        setFileSaved("");
        return;
      }
      const text = await commands.readEditableFile(selected.id, path);
      stashActive();
      setActiveFile(path);
      setFileImage(null);
      setFileContent(text);
      setFileSaved(text);
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function renameEntry(entry: BrowseEntry) {
    if (!selected) return;
    const next = await askText(
      tr("Rename"),
      tr("New name"),
      entry.name,
      tr("Change"),
    );
    if (next === null || next.trim() === entry.name) return;
    const instance = selected;
    const ok = await call(
      () =>
        commands.renameServerPath(instance.id, entry.path, next).then((path) => {
          const moved = (one: string) =>
            one === entry.path ? path : one.startsWith(`${entry.path}/`) ? path + one.slice(entry.path.length) : one;
          setOpenTabs((now) => now.map(moved));
          for (const [old, kept] of [...tabBuffers.current]) {
            if (moved(old) !== old) {
              tabBuffers.current.delete(old);
              tabBuffers.current.set(moved(old), kept);
            }
          }
          if (activeFile && moved(activeFile) !== activeFile) setActiveFile(moved(activeFile));
        }),
      tr("Renamed."),
    );
    if (ok) void browseTo(browsePath);
  }
  async function trashEntry(entry: BrowseEntry) {
    if (!selected) return;
    if (
      !(await confirmAction(
        tr("Delete {0}?", [entry.name]),
        entry.isDir
          ? tr("Moves the folder and all its contents to the app's storage folder. It's removed from the server folder.")
          : tr("Moves it to the app's storage folder instead of deleting it from disk. It's removed from the server folder."),
        tr("Delete"),
        true,
      ))
    )
      return;
    const instance = selected;
    const ok = await call(
      () => commands.trashServerPath(instance.id, entry.path),
      tr("Deleted {0}.", [entry.name]),
    );
    if (ok) {
      for (const one of openTabs) if (one === entry.path || one.startsWith(`${entry.path}/`)) forgetTab(one);
      void browseTo(browsePath);
    }
  }
  async function downloadEntry(entry: BrowseEntry) {
    if (!selected) return;
    const instance = selected;
    try {
      if (onWeb) {
        setError(null);
        const handed = await commands.readServerFileDownload(instance.id, entry.path);
        offerDownload(handed.name, handed.data);
        setNotice(tr("Downloaded {0}.", [entry.name]));
        return;
      }
      const target = await save({ title: tr("Save to another location"), defaultPath: entry.name });
      if (typeof target !== "string") return;
      await call(
        () => commands.copyServerFileOut(instance.id, entry.path, target),
        tr("Saved {0}.", [entry.name]),
      );
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function zipEntries(paths: string[]) {
    if (!selected || !paths.length) return;
    const first = paths[0].split("/").pop() ?? "archive";
    const guess = paths.length === 1 ? first.replace(/\.[^.]+$/, "") : (browsePath.split("/").pop() || selected.name);
    const name = await askText(tr("Compress"), tr("Zip file name"), `${guess}.zip`, tr("Compress"));
    if (name === null || !name.trim()) return;
    const instance = selected;
    const here = browsePath;
    const ok = await call(
      () => commands.zipServerPaths(instance.id, paths, name).then((made) => setNotice(tr("Compressed to {0}.", [made]))),
    );
    if (ok) {
      setPickedFiles(new Set());
      void browseTo(here);
    }
  }
  async function unzipEntry(entry: BrowseEntry) {
    if (!selected) return;
    const instance = selected;
    const here = browsePath;
    const ok = await call(
      () => commands.unzipServerPath(instance.id, entry.path).then((made) => setNotice(tr("Extracted to {0}.", [made]))),
    );
    if (ok) void browseTo(here);
  }
  async function trashPicked() {
    if (!selected || !pickedFiles.size) return;
    const list = [...pickedFiles];
    if (
      !(await confirmAction(
        tr("Delete {0, plural, one {# item} other {# items}}?", [list.length]),
        tr("Moves it to the app's storage folder instead of deleting it from disk. It's removed from the server folder."),
        tr("Delete"),
        true,
      ))
    )
      return;
    const instance = selected;
    const here = browsePath;
    let failed = 0;
    for (const one of list) {
      try {
        await commands.trashServerPath(instance.id, one);
      } catch (cause) {
        failed += 1;
        setError(String(cause));
      }
    }
    setNotice(tr("Deleted {0, plural, one {# item} other {# items}}.", [list.length - failed]));
    setPickedFiles(new Set());
    for (const one of openTabs) if (list.includes(one)) forgetTab(one);
    void browseTo(here);
  }
  async function newFolderHere() {
    if (!selected) return;
    const name = await askText(tr("New folder"), tr("Folder name"), "", tr("Create"));
    if (name === null || !name.trim()) return;
    const instance = selected;
    const ok = await call(
      () => commands.createServerFolder(instance.id, browsePath, name),
      tr("Folder created."),
    );
    if (ok) void browseTo(browsePath);
  }
  async function openTrash() {
    if (!selected) return;
    setTrashList([]);
    setTrashBusy(true);
    try {
      setTrashList(await commands.listServerTrash(selected.id));
    } catch (cause) {
      setTrashList(null);
      setError(String(cause));
    } finally {
      setTrashBusy(false);
    }
  }
  async function reloadTrash() {
    if (!selected) return;
    setTrashList(await commands.listServerTrash(selected.id).catch(() => [] as TrashFile[]));
  }
  async function newFileHere() {
    if (!selected) return;
    const name = await askText(tr("New file"), tr("File name (e.g. config.yml)"), "", tr("Create"));
    if (name === null || !name.trim()) return;
    const instance = selected;
    const here = browsePath;
    try {
      setError(null);
      const path = await commands.createServerFile(instance.id, here, name);
      setNotice(tr("Created {0}.", [name.trim()]));
      await browseTo(here);
      stashActive();
      setActiveFile(path);
      setFileImage(null);
      setFileContent("");
      setFileSaved("");
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function askFolderMerge(instance: Instance, here: string, sources: string[]) {
    const there = await commands.foldersInTheWay(instance.id, here, sources);
    if (!there.length) return false;
    const ok = await confirmAction(
      tr("{0} already exists. Merge folders?", [there.join(", ")]),
      tr("Adds the folder's contents. Files with the same name are replaced. Existing files that aren't in the new folder are kept."),
      tr("Merge"),
    );
    return ok ? true : null;
  }
  async function addDroppedFiles(paths: string[]) {
    if (!selected || !paths.length) return;
    const { browsePath: here, activeFile: open, locked: shut } = fileDropRef.current;
    if (open || shut) return;
    const instance = selected;
    let merge = false;
    try {
      const asked = await askFolderMerge(instance, here, paths);
      if (asked === null) return;
      merge = asked;
    } catch (cause) {
      setError(String(cause));
      return;
    }
    const ok = await call(
      () => commands.addServerFiles(instance.id, here, paths, merge),
      tr("Files added."),
    );
    if (ok) await browseTo(here);
  }
  async function addDroppedWebFiles(files: File[]) {
    if (!selected || !files.length) return;
    const { browsePath: here, activeFile: open, locked: shut } = fileDropRef.current;
    if (open || shut) return;
    const instance = selected;
    try {
      const tokens: string[] = [];
      for (const [at, file] of files.entries()) {
        setNotice(tr("Uploading… {0}/{1} · {2}", [at + 1, files.length, file.name]));
        tokens.push(await uploadRaw(file, webSession()));
      }
      const ok = await call(
        () => commands.addServerUploads(instance.id, here, tokens),
        tr("Files added."),
      );
      if (ok) await browseTo(here);
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function addFilesHere() {
    if (!selected) return;
    const instance = selected;
    const here = browsePath;
    try {
      if (onWeb) {
        const files = await pickRawFiles("*", true);
        if (!files.length) return;
        const tokens: string[] = [];
        for (const [at, file] of files.entries()) {
          setNotice(tr("Uploading… {0}/{1} · {2}", [at + 1, files.length, file.name]));
          tokens.push(await uploadRaw(file, webSession()));
        }
        const ok = await call(
          () => commands.addServerUploads(instance.id, here, tokens),
          tr("Files added."),
        );
        if (ok) void browseTo(here);
        return;
      }
      const picked = await open({ multiple: true, title: tr("Add files") });
      if (!picked) return;
      const sources = Array.isArray(picked) ? picked : [picked];
      const ok = await call(
        () => commands.addServerFiles(instance.id, here, sources, false),
        tr("Files added."),
      );
      if (ok) void browseTo(here);
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function addFolderHere() {
    if (!selected) return;
    const instance = selected;
    const here = browsePath;
    try {
      const picked = await open({ multiple: true, directory: true, title: tr("Add folder") });
      if (!picked) return;
      const sources = Array.isArray(picked) ? picked : [picked];
      const merge = await askFolderMerge(instance, here, sources);
      if (merge === null) return;
      const count = await call(
        () => commands.addServerFolders(instance.id, here, sources, merge),
        merge ? tr("Merged the folder.") : tr("Added the folder."),
        tr("Adding the folder…"),
      );
      if (count !== null) void browseTo(here);
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function saveFile() {
    if (!selected || !activeFile) return;
    const kept = fileContent;
    const ok = await call(
      () =>
        commands.saveEditableFile(selected.id, activeFile, fileContent),
      tr("File saved!"),
    );
    if (ok) {
      setFileSaved(kept);
      void browseTo(browsePath);
    }
  }
  const actionLabels: Record<PlayerAction, string> = {
    whitelist: tr("Whitelist"),
    operator: "OP",
    ban: tr("Ban"),
    kick: tr("Export"),
  };
  function askAbout(name: string, action: PlayerAction, off: boolean) {
    if (action === "kick") return tr("Kick {0} from the server?", [name]);
    if (action === "ban")
      return off ? tr("Unban {0}?", [name]) : tr("Ban {0}?", [name]);
    if (action === "whitelist")
      return off
        ? tr("Remove {0} from the whitelist?", [name])
        : tr("Add {0} to the whitelist?", [name]);
    return off ? tr("Remove operator status from {0}?", [name]) : tr("Give {0} operator status?", [name]);
  }
  async function applyPlayerAccess(
    name: string,
    uuid: string,
    action: PlayerAction,
    current?: boolean,
  ) {
    if (!selected) return false;
    const live = selected.status === "running";
    const label = actionLabels[action];
    const turningOff = action !== "kick" && current === true;
    const body = live
      ? tr("The server is running, so this is applied right away with a console command.") + (action === "kick" ? "" : " " + tr("The server updates its list file as well."))
      : tr("The server is stopped, so the list file is edited directly.");
    if (
      !(await confirmAction(
        askAbout(name, action, turningOff),
        body,
        action === "kick"
          ? tr("Export")
          : action === "ban"
            ? turningOff
              ? tr("Unban")
              : tr("Ban")
            : turningOff
              ? tr("Remove")
              : tr("Add"),
        action === "kick" || action === "ban",
      ))
    )
      return false;
    const ok = await call(
      () =>
        commands.togglePlayerAccess({ id: selected.id, name, uuid, action }),
      action === "kick"
        ? tr("Kicked {0}.", [name])
        : action === "ban"
          ? turningOff
            ? tr("Unbanned {0}.", [name])
            : tr("Banned {0}.", [name])
          : tr("Changed {1} for {0}.", [name, label]),
    );
    if (ok) void loadTool("players");
    return ok;
  }
  async function updatePlayer(player: Player, action: PlayerAction) {
    const current =
      action === "whitelist"
        ? player.whitelisted
        : action === "operator"
          ? player.operator
          : player.banned;
    await applyPlayerAccess(player.name, player.uuid, action, current);
  }
  const [playerAddKind, setPlayerAddKind] = useState<"whitelist" | "operator" | "ban">("whitelist");
  const [playerTab, setPlayerTab] = useState<"online" | "all" | "whitelist" | "operator" | "ban" | "recent">("all");
  const [playerQuery, setPlayerQuery] = useState("");
  async function addPlayerHere() {
    if (playerAddKind === "ban") {
      const ok = await confirmAction(
        tr("Ban {0}?", [playerName.trim()]),
        tr("They can't join the server. If they're online, they'll be disconnected immediately. You can unban them from the Banned tab."),
        tr("Ban"),
        true,
      );
      if (!ok) return;
    }
    await addPlayer(playerAddKind);
  }
  async function banPlayer(player: Player) {
    if (!player.banned) {
      const ok = await confirmAction(
        tr("Ban {0}?", [player.name]),
        tr("They can't join the server. If they're online, they'll be disconnected immediately. You can unban them from the Banned tab."),
        tr("Ban"),
        true,
      );
      if (!ok) return;
    }
    await updatePlayer(player, "ban");
  }
  async function addPlayer(action: PlayerAction) {
    if (!selected || !playerName.trim()) return;
    const live = selected.status === "running";
    let uuid = "";
    if (!live) {
      setError(null);
      uuid = await findUuid(playerName.trim());
      if (!uuid) {
        setError(
          tr("Couldn't find {0} on Mojang. Check the name and try again. If the server is running, you can use just the name.", [playerName.trim()]),
        );
        return;
      }
    }
    const ok = await applyPlayerAccess(
      playerName.trim(),
      uuid,
      action,
      false,
    );
    if (ok) setPlayerName("");
  }
  async function loadPlayerSnapshot(player: Player) {
    if (!selected) return;
    try {
      setError(null);
      const snapshot = await commands.readPlayerNbt(selected.id, player.uuid);
      setPlayerSnapshot(snapshot);
      setInvTab("bag");
      const items = [...snapshot.inventory, ...snapshot.enderItems];
      const ids = items.map((one) => one.id);
      void loadItemNames(selected, ids);
      void loadItemIcons(selected, ids);
      void loadEnchantNames(selected, items.flatMap((one) => one.enchantments.map((e) => e.id)));
    } catch (cause) {
      setPlayerSnapshot(null);
      setError(String(cause));
    }
  }
  async function editSavedItem(item: { slot: number; count: number }, ender: boolean) {
    if (!selected || !playerSnapshot) return;
    const text = await askText(tr("Change item count"), tr("Count (0 deletes it)"), String(item.count), tr("Change"));
    if (text == null) return;
    const count = Number(text.trim());
    if (!Number.isInteger(count) || count < 0 || count > 99) {
      setError(tr("Enter a number from 0 to 99."));
      return;
    }
    if (count === item.count) return;
    try {
      setError(null);
      await commands.editPlayerItem(selected.id, playerSnapshot.uuid, ender, item.slot, count);
      setPlayerSnapshot(await commands.readPlayerNbt(selected.id, playerSnapshot.uuid));
      setNotice(count === 0 ? tr("Deleted the item.") : tr("Changed the item count."));
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function findUuid(name: string): Promise<string> {
    try {
      const profile = await commands.lookupMojangProfile(name);
      setPlayerName(profile.name);
      if (profile.head)
        setHeads((current) => ({ ...current, [profile.uuid]: profile.head }));
      return profile.uuid;
    } catch {
      return "";
    }
  }
  const worldTree = useMemo(() => {
    const order = ["overworld", "nether", "end"];
    const groups = new Map<string, World[]>();
    for (const one of worlds) {
      const list = groups.get(one.world) ?? [];
      list.push(one);
      groups.set(one.world, list);
    }
    return [...groups.entries()]
      .map(([name, list]) => {
        const parent = list.find((one) => one.hasLevelData) ?? list[0];
        const children = list
          .filter((one) => one !== parent)
          .sort((a, b) => {
            const rank = (one: World) => (order.includes(one.realm) ? order.indexOf(one.realm) : order.length);
            return rank(a) - rank(b) || a.realm.localeCompare(b.realm);
          });
        const ownMib = children.reduce(
          (left, one) => left - (one.insideParent ? one.sizeMib ?? 0 : 0),
          parent.sizeMib ?? 0,
        );
        return { name, parent, children, ownMib };
      })
      .sort((a, b) => a.name.localeCompare(b.name))
      .flatMap(({ parent, children, ownMib }) => [
        { world: parent, sizeMib: Math.max(0, ownMib) },
        ...children.map((one) => ({ world: one, sizeMib: one.sizeMib ?? 0 })),
      ]);
  }, [worlds]);
  async function loadWorldSettings(world: World) {
    if (!selected) return;
    try {
      setError(null);
      const settings = await commands.readWorldSettings(selected.id, world.name);
      setWorldSettings(settings);
      setWorldDraft(settings);
    } catch (cause) {
      setWorldSettings(null);
      setWorldDraft(null);
      setError(String(cause));
    }
  }
  async function saveWorldSettings() {
    if (!selected || !worldSettings || !worldDraft) return;
    const changedRules = worldDraft.gameRules.filter((rule) => {
      const before = worldSettings.gameRules.find(
        (item) => item.name === rule.name,
      );
      return before && before.value !== rule.value;
    });
    if (
      !(await confirmAction(
        tr("Save the world settings for {0}?", [worldSettings.levelName]),
        tr("Edits level.dat directly and records the original hash in the audit log. {0} game rules will change.", [changedRules.length]),
        tr("Save"),
        true,
      ))
    )
      return;
    const ok = await call(
      () =>
        commands.saveWorldSettings({
            id: selected.id,
            world: worldSettings.levelName || "world",
            difficulty: worldDraft.difficulty,
            hardcore: worldDraft.hardcore,
            spawn: [worldDraft.spawn[0] ?? 0, worldDraft.spawn[1] ?? 0, worldDraft.spawn[2] ?? 0],
            gameRules: changedRules,
          }),
      tr("World settings saved!"),
    );
    if (ok) void loadTool("worlds");
  }
  async function loadWaiting(instance = selected) {
    if (!instance) return;
    try {
      setWaiting(
        await commands.listPendingAddons(instance.id),
      );
    } catch {
      setWaiting([]);
    }
  }
  async function cancelWaiting(entry: WaitingAddon) {
    if (!selected) return;
    const ok = await call(
      () =>
        commands.cancelPendingAddon(selected.id, entry.id),
      tr("Removed {0} from the queue.", [entry.displayName]),
    );
    if (ok) void loadWaiting();
  }
  async function checkAddonUpdates(quiet = false) {
    if (!selected) return;
    setCheckingUpdates(true);
    try {
      if (!quiet) setError(null);
      const found = await commands.checkAddonUpdates(selected.id);
      setAddonUpdates(found);
    } catch (cause) {
      if (!quiet) setError(String(cause));
      setAddonUpdates(null);
    } finally {
      setCheckingUpdates(false);
    }
  }
  function openUpdatePlan() {
    const rows: UpdateRow[] = (addonUpdates ?? []).map((update) => {
      const card = addonCards[update.fileName];
      const addon = addons.find((one) => one.fileName === update.fileName);
      return {
        update,
        title: card?.title || update.displayName || addon?.name || update.fileName,
        iconUrl: card?.iconUrl ?? null,
        projectId: card?.projectId ?? null,
        kind: addon?.kind === "plugin" ? "plugin" : "mod",
      };
    });
    if (rows.length) setUpdatePlan(rows);
  }
  async function runUpdatePlan(choices: UpdateChoice[]) {
    if (!selected || !updatePlan) return;
    const latest = choices.filter((one) => one.versionId === null).map((one) => one.fileName);
    const pinned = choices.filter((one) => one.versionId !== null);
    if (pinned.length && selected.status !== "stopped") {
      setError(tr("Stop the server before choosing a version other than the latest."));
      return;
    }
    setUpdateBusy(true);
    try {
      if (latest.length) await applyAddonUpdates(latest, false);
      for (const one of pinned) {
        const row = updatePlan.find((item) => item.update.fileName === one.fileName);
        const addon = addons.find((item) => item.fileName === one.fileName);
        if (!row?.projectId || !addon || !one.versionId) continue;
        const versionId = one.versionId;
        await call(
          () =>
            commands.deleteAddon(selected.id, addon.fileName, addon.kind)
              .then(() => commands.installModrinth(selected.id, row.projectId!, row.kind, versionId)),
          tr("Changed {0} to {1}.", [row.title, one.versionName]),
        );
      }
      void refreshAddons();
    } finally {
      setUpdateBusy(false);
      setUpdatePlan(null);
    }
  }
  async function applyAddonUpdates(fileNames: string[], ask = true) {
    if (!selected || !fileNames.length) return;
    if (
      ask &&
      !(await confirmAction(
        fileNames.length === 1
          ? tr("Update to the latest version?")
          : tr("Update {0, plural, one {# item} other {# items}} to the latest version?", [fileNames.length]),
        selected.status === "stopped"
          ? tr("Downloads the new file and verifies its SHA‑512 hash before removing the old file. The old file is kept so you can roll back.")
          : tr("The server is running, so this can't be changed yet. The new file will be downloaded now and applied on the next start. The current version keeps running until then."),
        selected.status === "stopped" ? tr("Apply update") : tr("Download and queue"),
      ))
    )
      return;
    const done = await call(
      () =>
        commands.applyAddonUpdates(selected.id, fileNames).then((result) => {
          setNotice(
            result.queued
              ? tr("{0} downloaded. Will apply when the server restarts.", [result.queued])
              : tr("Update applied. The previous file was kept."),
          );
          return result;
        }),
    );
    if (done) {
      void refreshAddons();
      void loadWaiting();
    }
  }
  async function bulkToggleAddons(on: boolean) {
    if (!selected) return;
    const targets = addons.filter(
      (one) => pickedAddons.includes(one.fileName) && one.enabled !== on,
    );
    if (!targets.length) return;
    const ok = await call(async () => {
      for (const one of targets) {
        await commands.toggleAddon(selected.id, one.fileName, one.kind);
      }
    }, on ? tr("Turned on {0}.", [targets.length]) : tr("Turned off {0}.", [targets.length]));
    if (ok) {
      setPickedAddons([]);
      void refreshAddons();
    }
  }
  async function bulkDeleteAddons() {
    if (!selected || !pickedAddons.length) return;
    if (
      !(await confirmAction(
        tr("Delete {0, plural, one {# item} other {# items}}?", [pickedAddons.length]),
        tr("Moves it to the app's storage folder instead of deleting it. It's removed from the server folder, but you can restore it later."),
        tr("Delete"),
        true,
      ))
    )
      return;
    const targets = addons.filter((one) => pickedAddons.includes(one.fileName));
    const ok = await call(async () => {
      for (const one of targets) {
        await commands.deleteAddon(selected.id, one.fileName, one.kind);
      }
    }, tr("Moved {0, plural, one {# item} other {# items}} to the trash.", [targets.length]));
    if (ok) {
      setPickedAddons([]);
      void refreshAddons();
    }
  }
  async function rollbackAddon(entry: AddonRollback) {
    if (!selected) return;
    if (
      !(await confirmAction(
        tr("Restore to {0}?", [entry.fileName]),
        tr("Restores the saved file to its original folder. The newer file stays, so disable one if they conflict."),
        tr("Restore"),
      ))
    )
      return;
    const ok = await call(
      () =>
        commands.rollbackAddonUpdate(selected.id, entry.storedName),
      tr("Restored the previous version!"),
    );
    if (ok) void refreshAddons();
  }
  async function loadTimeline(fileName = activeLogFile, kind = timelineKind) {
    if (!selected) return;
    try {
      setError(null);
      setActiveLogFile(fileName);
      setTimelineKind(kind);
      setTimeline(
        await commands.readLogTimeline(selected.id, fileName, kind),
      );
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function saveBookmark() {
    if (!selected || !markForm.name.trim()) return;
    const ok = await call(
      () =>
        commands.saveBookmark({
            id: null,
            serverId: selected.id,
            name: markForm.name.trim(),
            dimension: markForm.dimension,
            x: Math.round(Number(markForm.x) || 0),
            y: Math.round(Number(markForm.y) || 0),
            z: Math.round(Number(markForm.z) || 0),
            note: markForm.note,
          }),
      tr("Saved {0}.", [markForm.name.trim()]),
    );
    if (ok) {
      setMarkForm({ name: "", dimension: "overworld", x: "0", y: "64", z: "0", note: "" });
      void loadBookmarks();
    }
  }
  async function saveMapPng(png: string) {
    if (!selected) return;
    const name = `${selected.name}-${mapRealm}.png`;
    if (onWeb) {
      offerDownload(name, png, "image/png");
      return;
    }
    const target = await save({
      title: tr("Save PNG"),
      defaultPath: name,
      filters: [{ name: "PNG", extensions: ["png"] }],
    });
    if (typeof target !== "string") return;
    await call(() => commands.saveMapPng(target, png), tr("Saved the map as a picture."));
  }
  async function bookmarkSpot(x: number, z: number) {
    if (!selected) return;
    const name = (
      await askText(tr("Bookmark X {0}, Z {1}", [x, z]), tr("Name"), "", tr("Save"))
    )?.trim();
    if (!name) return;
    const y = (await commands.mapSurfaceHeight(selected.id, mapRealm, x, z).catch(() => null)) ?? 64;
    const ok = await call(
      () =>
        commands.saveBookmark({
          id: null,
          serverId: selected.id,
          name,
          dimension: mapRealm,
          x,
          y,
          z,
          note: "",
        }),
      tr("Saved {0}.", [name]),
    );
    if (ok) void loadBookmarks();
  }
  async function removeBookmark(mark: Bookmark) {
    if (
      !(await confirmAction(
        tr("Delete ‘{0}’?", [mark.name]),
        tr("Only the bookmark is deleted. Server files are untouched."),
        tr("Delete"),
        true,
      ))
    )
      return;
    await call(() => commands.deleteBookmark(mark.id), tr("Bookmark deleted."));
    void loadBookmarks();
  }
  async function exportMrpack() {
    if (!selected) return;
    try {
      if (onWeb) {
        setError(null);
        const made = await commands.exportMrpackDownload(selected.id);
        offerDownload(made.name, made.data);
        setNotice(tr("Modpack exported."));
        return;
      }
      const target = await save({
        title: tr("Export as modpack"),
        defaultPath: `${selected.name}.mrpack`,
        filters: [{ name: tr("Modrinth modpack"), extensions: ["mrpack"] }],
      });
      if (typeof target !== "string") return;
      await call(
        () => commands.exportMrpack(selected.id, target),
        tr("Exported as a modpack."),
      );
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function exportSettings() {
    try {
      const target = await save({
        title: tr("Export settings"),
        defaultPath: "monolab-settings.json",
        filters: [{ name: tr("Config files"), extensions: ["json"] }],
      });
      if (typeof target !== "string") return;
      await call(
        () => commands.exportSettings(target),
        tr("Settings exported to a file."),
      );
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function importSettings() {
    try {
      const source = await open({
        multiple: false,
        title: tr("Import settings"),
        filters: [{ name: tr("Config files"), extensions: ["json"] }],
      });
      if (typeof source !== "string") return;
      if (
        !(await confirmAction(
          tr("Import settings?"),
          tr("Adds servers and coordinate bookmarks without deleting existing items. Only servers with an existing folder are added."),
          tr("Import"),
        ))
      )
        return;
      const summary = await commands.importSettings(source);
      setNotice(
        tr("Imported {0, plural, one {# server} other {# servers}} and {1, plural, one {# bookmark} other {# bookmarks}}.{2}", [summary.instances, summary.bookmarks, summary.skipped ? " " + tr("Skipped {0} that already exist or have no folder.", [summary.skipped]) : ""]),
      );
      await refresh();
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function openExternal(url: string) {
    try {
      if (onWeb) {
        window.open(url, "_blank", "noopener,noreferrer");
        return;
      }
      await openUrl(url);
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function openMap(realm = mapRealm, inst = selected) {
    if (!inst) return;
    setMapOpen(true);
    void commands.listPlayerSpots(inst.id)
      .then((spots) => {
        setPlayerSpots(spots);
        const uuids = spots
          .map((one) => one.uuid)
          .filter((uuid) => uuid.length === 36);
        if (uuids.length)
          void commands.getPlayerHeads(uuids)
            .then((found) => setHeads((current) => ({ ...current, ...found })))
            .catch(() => undefined);
      })
      .catch(() => setPlayerSpots([]));
    await drawMap(realm, inst);
  }
  async function loadPerfHistory(day = "") {
    if (!selected) return;
    try {
      const [rows, days] = await Promise.all([
        commands.perfHistory(selected.id, day),
        commands.listEventDays(selected.id),
      ]);
      setPerfHistory(rows);
      setPerfDays(days);
      setPerfDay(day || days[0] || "");
    } catch {
      setPerfHistory([]);
      setPerfDays([]);
    }
  }
  async function shareLog(fileName: string) {
    if (!selected || !fileName) return;
    let body = tr("Uploads {0} to mclo.gs. Anyone with the link can read it.", [fileName]);
    try {
      const preview = await commands.previewShareLog(selected.id, fileName);
      const hidden = preview.masked.ips + preview.masked.paths + preview.masked.secrets;
      body += "\n\n" + tr("{0, plural, one {# line} other {# lines}} · masking {1, plural, one {# IP} other {# IPs}}, {2, plural, one {# home path} other {# home paths}}, {3, plural, one {# secret} other {# secrets}}.", [
        preview.lines,
        preview.masked.ips,
        preview.masked.paths,
        preview.masked.secrets,
      ]);
      if (hidden > 0 && preview.sample.length) {
        body += "\n\n" + tr("Masked examples:") + "\n" + preview.sample.map((line) => `  ${line}`).join("\n");
      }
    } catch {
      body += "\n\n" + tr("IP addresses, home paths and secrets are masked before upload.");
    }
    const ok = await confirmAction(tr("Upload the log"), body, tr("Upload"));
    if (!ok) return;
    setSharingLog(true);
    try {
      setError(null);
      const url = await commands.shareLog(selected.id, fileName);
      await navigator.clipboard.writeText(url).catch(() => {});
      setNotice(tr("{0} — address copied.", [url]));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setSharingLog(false);
    }
  }
  async function drawMap(realm = mapRealm, inst = selected) {
    if (!inst) return;
    setMapRealm(realm);
    setMapBusy(true);
    void commands.listBookmarks(inst.id)
      .then(setBookmarks)
      .catch(() => {});
    try {
      setError(null);
      setMapCave(null);
      if (realm === "overworld") {
        const id = inst.id;
        void commands.worldMapBounds(id, "overworld-cave").then(
          () => setMapCave(false),
          () => setMapCave(null),
        );
      }
      const [minX, minZ, maxX, maxZ] = await commands.worldMapBounds(inst.id, realm);
      const box = 900;
      const level = Math.max(
        0,
        Math.ceil(Math.log2(Math.max(maxX - minX, maxZ - minZ) / box)),
      );
      setMapStart({
        x: Math.round((minX + maxX) / 2 - (box * 2 ** level) / 2),
        z: Math.round((minZ + maxZ) / 2 - (600 * 2 ** level) / 2),
        level,
      });
    } catch (cause) {
      setError(String(cause));
    } finally {
      setMapBusy(false);
    }
  }
  async function fetchMapTile(z: number, tx: number, ty: number, night: boolean) {
    if (!selected) throw new Error(tr("No servers found."));
    const layer = mapCave && mapRealm === "overworld" ? "overworld-cave" : mapRealm;
    return await commands.renderMapTile(selected.id, layer, z, tx, ty, night);
  }
  async function fetchMapChanges(): Promise<[number, number][]> {
    if (!selected) return [];
    const layer = mapCave && mapRealm === "overworld" ? "overworld-cave" : mapRealm;
    return (await commands.mapChanges(selected.id, layer)) as [number, number][];
  }
  const mapPins: MapPin[] = [
    ...(mapSpawn && mapRealm === "overworld"
      ? [{ key: "spawn", x: mapSpawn[0], z: mapSpawn[1], label: tr("Spawn"), kind: "spawn" }]
      : []),
    ...(live?.players ?? [])
      .filter((one) => inRealm(one.world, mapRealm))
      .map((one) => ({
        key: `p-${one.name}`,
        x: one.x,
        z: one.z,
        label: one.name,
        kind: "player",
        icon: liveHeads[one.name] || null,
        online: true,
      })),
    ...playerSpots
      .filter((one) => inRealm(realmFolder(one.dimension), mapRealm) && !(live?.players ?? []).some((who) => who.name === one.name))
      .map((one) => ({
        key: `away-${one.uuid}`,
        x: one.x,
        z: one.z,
        label: one.name,
        kind: "player",
        icon: heads[one.uuid] || null,
        online: false,
        savedAt: one.savedAt,
      })),
    ...bookmarks
      .filter((one) => one.dimension === mapRealm)
      .map((one) => ({ key: `mark-${one.id}`, x: one.x, z: one.z, label: one.name, kind: "mark" })),
  ] as MapPin[];
  async function deleteAreaChunks(area: MapArea) {
    if (!selected) return;
    if (selected.status !== "stopped" && selected.status !== "crashed") {
      setError(tr("Stop the server first. Chunks can only be deleted while it's off."));
      return;
    }
    const box = [area.minX, area.minZ, area.maxX, area.maxZ];
    let preview: Awaited<ReturnType<typeof commands.trimChunks>>;
    try {
      setError(null);
      preview = await commands.trimChunks(selected.id, mapRealm, 0, 0, false, box);
    } catch (cause) {
      setError(trBack(String(cause)));
      return;
    }
    if (!preview.removed) {
      setNotice(tr("There are no saved chunks in this area."));
      return;
    }
    const first = await confirmAction(
      tr("Delete {0} chunks in this area?", [preview.removed.toLocaleString()]),
      tr("X {0} ~ {1}, Z {2} ~ {3}. Everything built there is lost, and Minecraft makes fresh terrain the next time someone goes there. A backup of the server is made first.", [area.minX, area.maxX, area.minZ, area.maxZ]),
      tr("Continue"),
      true,
    );
    if (!first) return;
    const typed = (
      await askText(
        tr("Type the server name to delete"),
        tr("This can only be undone by restoring the backup. Type {0} to delete {1} chunks.", [selected.name, preview.removed.toLocaleString()]),
        "",
        tr("Delete chunks"),
        true,
      )
    )?.trim();
    if (typed === undefined) return;
    if (typed !== selected.name) {
      setError(tr("The server name didn't match, so nothing was deleted."));
      return;
    }
    const done = await call(
      () => commands.trimChunks(selected.id, mapRealm, 0, 0, true, box),
      tr("Deleted the chunks. A backup was made first."),
    );
    if (done) void openMap(mapRealm);
  }
  async function fetchChunkStats(metric: string, rx: number, rz: number) {
    if (!selected) return null;
    return await commands.chunkStats(selected.id, mapRealm, metric, rx, rz);
  }
  async function openVersionPick(one: Addon) {
    if (!selected) return;
    const card = addonCards[one.fileName];
    const projectId = card?.projectId;
    if (!projectId) {
      setError(
        tr("Couldn't find {0} on Modrinth, so you can't change its version here. Download and add it manually.", [one.name]),
      );
      return;
    }
    const title = card?.title ?? one.name;
    setVersionPick({
      addon: one,
      projectId,
      title,
      iconUrl: card?.iconUrl ?? null,
      list: null,
    });
    try {
      const list = await commands.listAddonVersions(selected.id, projectId, one.kind === "plugin" ? "plugin" : "mod");
      setVersionPick((open) => (open ? { ...open, list } : open));
    } catch (cause) {
      setVersionPick(null);
      setError(String(cause));
    }
  }
  async function swapVersion(version: AddonVersion) {
    if (!selected || !versionPick) return;
    const { addon, projectId, title } = versionPick;
    setDialogBusy(true);
    await call(
      () =>
        commands.deleteAddon(selected.id, addon.fileName, addon.kind)
          .then(() =>
            commands.installModrinth(selected.id, projectId, addon.kind === "plugin" ? "plugin" : "mod", version.id),
          )
          .then(() => refreshAddons()),
      tr("Changed {0} to {1}.", [title, version.name]),
    );
    setDialogBusy(false);
    setVersionPick(null);
  }
  async function deleteAddon(one: Addon) {
    if (!selected) return;
    if (
      !(await confirmAction(
        tr("Delete {0}?", [one.name]),
        tr("Moves it to the app's storage folder instead of deleting it. It's removed from the server folder, but you can restore it later."),
        tr("Delete"),
        true,
      ))
    )
      return;
    await call(
      () =>
        commands.deleteAddon(selected.id, one.fileName, one.kind).then(() => refreshAddons()),
      tr("Deleted {0}.", [one.name]),
    );
  }
  useEffect(() => {
    if (!browseOpen || projectView) return;
    if (catalogQuery === lastSearchedRef.current) return;
    if (browseKind === "datapack") return;
    const timer = window.setTimeout(() => void (browseKind === "modpack" ? searchPacks(catalogQuery) : searchCatalog(catalogQuery, 0)), 300);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [catalogQuery, browseOpen, browseKind, projectView]);
  useEffect(() => {
    if (modal !== "create" || createStep !== "modpack") return;
    if (packQuery === lastSearchedRef.current) return;
    const timer = window.setTimeout(() => void searchPacks(packQuery), 300);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [packQuery, modal, createStep]);
  useEffect(() => {
    if (!browseOpen) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "/" || event.ctrlKey || event.metaKey || event.altKey) return;
      const target = event.target as HTMLElement;
      if (target.closest("input, textarea, select, [contenteditable]")) return;
      const box = document.querySelector<HTMLInputElement>(".browse-search input");
      if (!box) return;
      event.preventDefault();
      box.focus();
      box.select();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [browseOpen]);
  async function openBrowse() {
    if (!selected) return;
    setProjectView(null);
    const versions = realVersion(selected.version) ? [selected.version] : [];
    setCatalogQuery("");
    setCatalogHits([]);
    setCatalogCategories([]);
    setCatalogVersions(versions);
    setBrowseVersionList(versions);
    setBrowseKind("addon");
    setBrowseOpen(true);
    void searchCatalog("", 0, [], catalogEnvironment, versions, catalogLoader);
    const core = selected.core;
    void commands.listCoreVersions(core, false)
      .then((list) => {
        const merged =
          !realVersion(selected.version) || list.includes(selected.version)
            ? list
            : [selected.version, ...list];
        setBrowseVersionList(merged);
      })
      .catch(() => {});
  }
  async function installWithDependencies(item: CatalogHit) {
    if (!selected) return;
    let plan: InstallPlan;
    try {
      setError(null);
      plan = await commands.planAddonInstall(selected.id, item.projectId, [...installedProjectIds]);
    } catch (cause) {
      setError(String(cause));
      return;
    }
    const pending = plan.dependencies.filter(
      (dependency) => !dependency.alreadyInstalled,
    );
    if (!pending.length) {
      await runInstall(item, []);
      return;
    }
    setDepAsk({
      item,
      plan,
      chosen: pending
        .filter((dependency) => dependency.required)
        .map((dependency) => dependency.projectId),
    });
  }
  async function runInstall(item: CatalogHit, chosen: string[]) {
    if (!selected) return;
    setDialogBusy(true);
    markInstalling(item.projectId, true);
    const ok = await call(
      () =>
        commands.installAddonWithDependencies(selected.id, item.projectId, chosen, [...installedProjectIds]),
      (chosen.length
        ? tr("Installed {0} with {1} required dependencies.", [item.title, chosen.length])
        : tr("Installed {0}.", [item.title])) + (selected.status !== "stopped" ? " " + tr("Applies on the next start.") : ""),
    );
    setDialogBusy(false);
    setDepAsk(null);
    if (ok) await refreshAddons(selected);
    markInstalling(item.projectId, false);
  }
  async function searchCatalog(
    query = catalogQuery,
    page = 0,
    categories = catalogCategories,
    environment = catalogEnvironment,
    versions = catalogVersions,
    loader = catalogLoader,
  ) {
    if (!selected) return;
    try {
      setError(null);
      setCatalogBusy(true);
      lastSearchedRef.current = query;
      const result = await commands.searchAddonCatalog(selected.id, query, page, categories, environment, versions, loader);
      setCatalogHits(result.hits);
      setCatalogMore(result.hasMore);
      setCatalogPage(result.page);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setCatalogBusy(false);
    }
  }
  async function searchPacks(query = packQuery) {
    try {
      setError(null);
      setPackBusy(true);
      setPackSearched(true);
      lastSearchedRef.current = query;
      const result = await commands.searchModpacks(query, 0);
      setPackHits(result.hits);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setPackBusy(false);
    }
  }
  async function getPack(hit: CatalogHit) {
    try {
      setError(null);
      setPackGetting(hit.projectId);
      setMrpackEntries([]);
      setCfPack(null);
      const path = await commands.downloadModpack(hit.projectId);
      setMrpackPath(path);
      await previewPack(path);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setPackGetting("");
    }
  }
  async function previewPack(path = mrpackPath) {
    if (!path.trim()) return;
    setCfPack(null);
    setMrpackEntries([]);
    setError(null);
    try {
      setMrpackEntries(
        await commands.previewMrpack(path.trim()),
      );
      return;
    } catch {}
    try {
      setCfPack(await commands.previewCfpack(path.trim()));
    } catch (cause) {
      setError(String(cause));
    }
  }
  async function importCfPack() {
    if (!selected || !cfPack) return;
    if (
      !(await confirmAction(
        tr("Import settings from {0}?", [cfPack.name]),
        tr("{0, plural, one {# config file} other {# config files}} will overwrite the current settings. You'll need to download {1, plural, one {# mod} other {# mods}} from CurseForge manually. A download list is saved in the server folder. The entire server folder is backed up first.", [cfPack.overrideCount, cfPack.mods.length]),
        tr("Import settings"),
      ))
    )
      return;
    const ok = await call(
      () =>
        commands.importCfpack(selected.id, mrpackPath.trim()),
      tr("Settings imported! Use the list to download the mods manually."),
    );
    if (ok) void refreshAddons();
  }
  async function importMrpack() {
    if (!selected || !mrpackPath.trim() || !mrpackEntries.length) return;
    const clientOnly = mrpackEntries.filter(
      (entry) => entry.compatibility === "Client only",
    ).length;
    if (
      !(await confirmAction(
        tr("Import {0, plural, one {# item} other {# items}} from the modpack?", [mrpackEntries.length]),
        tr("Config files in the modpack overwrite the current ones. {0}", [includeClientOnlyMods ? tr("Client-only mods are installed too.") : tr("{0} client-only mods are left out.", [clientOnly])]),
        tr("Add modpack"),
      ))
    )
      return;
    const ok = await call(
      () =>
        commands.importMrpack({
            id: selected.id,
            path: mrpackPath.trim(),
            includeClientOnly: includeClientOnlyMods,
          }),
      tr("Modpack added!"),
    );
    if (ok) void refreshAddons();
  }
  const commandTyped = (() => {
    const typed = command.trimStart();
    return typed.startsWith("/") ? typed.slice(1) : typed;
  })();
  const putCommand = (text: string) => setCommand(text);
  const consoleHints = (() => {
    const typed = commandTyped;
    if (!typed) return [] as { text: string; hint: string }[];
    const nodes = serverCommands.nodes;
    const flat = serverCommands.flat;
    const describe = (name: string, extra?: string | null) =>
      consoleCommands.find((one) => one.name === name)?.hint ?? extra ?? "";
    const people = (prefix: string) =>
      (live?.players ?? [])
        .filter((who) => who.name.toLowerCase().startsWith(prefix))
        .map((who) => who.name);

    if (nodes.length) {
      const parts = typed.split(" ");
      const tail = (typed.endsWith(" ") ? "" : (parts.pop() ?? "")).toLowerCase();
      const done = parts.filter((one) => one.length > 0);
      const head = typed.slice(0, typed.length - tail.length);

      let at = 0;
      let lost = false;
      for (const word of done) {
        const node = nodes[at];
        if (!node) {
          lost = true;
          break;
        }
        const kids = node.children.length
          ? node.children
          : nodes[node.redirect ?? -1]
            ? nodes[node.redirect as number].children
            : [];
        const low = word.toLowerCase();
        const literal = kids.find(
          (kid) => nodes[kid]?.kind === "literal" && nodes[kid]?.name?.toLowerCase() === low,
        );
        const argument = kids.find((kid) => nodes[kid]?.kind === "argument");
        const next = literal ?? argument;
        if (next === undefined) {
          lost = true;
          break;
        }
        at = next;
      }
      if (lost) return [];

      const node = nodes[at];
      const kids = node?.children.length
        ? node.children
        : nodes[node?.redirect ?? -1]
          ? nodes[node!.redirect as number].children
          : [];
      const out: { text: string; hint: string }[] = [];
      for (const kid of kids) {
        const child = nodes[kid];
        if (!child) continue;
        if (child.kind === "literal") {
          const name = child.name ?? "";
          if (!name.toLowerCase().startsWith(tail)) continue;
          out.push({
            text: head + name,
            hint: at === 0 ? describe(name, child.from) : (child.from ?? ""),
          });
        } else if (wantsPlayerName(child.name)) {
          for (const who of people(tail)) {
            out.push({ text: head + who, hint: tr("Online") });
          }
        }
      }
      if (!out.length && !tail) {
        for (const kid of kids) {
          const child = nodes[kid];
          if (child?.kind === "argument") {
            return [{ text: "", hint: tr("Enter <{0}>", [child.name ?? tr("value")]) }];
          }
        }
      }
      return out.slice(0, 8);
    }

    const at = typed.lastIndexOf(" ");
    if (at < 0) {
      const low = typed.toLowerCase();
      const pool = flat.length
        ? flat.map((one) => ({ name: one.name, hint: describe(one.name, one.from ?? one.usage) }))
        : (selected?.core === "Pumpkin"
            ? [...consoleCommands, ...pumpkinCommands]
            : consoleCommands
          ).map((one) => ({ name: one.name, hint: one.hint }));
      return pool
        .filter((one) => one.name.startsWith(low))
        .slice(0, 8)
        .map((one) => ({ text: one.name, hint: one.hint }));
    }
    const head = typed.slice(0, at);
    const tail = typed.slice(at + 1).toLowerCase();
    const first = head.trimStart().split(" ")[0].toLowerCase();
    const found = flat.find((one) => one.name === first);
    const out: { text: string; hint: string }[] = [];
    if (found && head.trim().toLowerCase() === first) {
      for (const arg of found.args) {
        if (arg.toLowerCase().startsWith(tail)) {
          out.push({ text: `${head} ${arg}`, hint: found.from ?? "" });
        }
      }
    }
    const wantsPlayer =
      /<(targets?|players?)>/.test(found?.usage ?? "") ||
      consoleCommands.some(
        (one) => one.args === "player" && head.toLowerCase().startsWith(one.name),
      );
    if (wantsPlayer) {
      for (const who of people(tail)) {
        out.push({ text: `${head} ${who}`, hint: tr("Online") });
      }
    }
    return out.slice(0, 8);
  })();
  const shownHints = heldHints ?? consoleHints;
  useEffect(() => {
    if (tool !== "overview" || !selected) return;
    if (selected.status !== "running" && selected.status !== "stopping") return;
    void loadTool("console", selected);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tool, selected?.status]);
  useEffect(() => {
    if (tool !== "console" || !selected) return;
    void commands.serverCommands(selected.id)
      .then(setServerCommands)
      .catch(() => setServerCommands({ nodes: [], flat: [] }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tool, selected?.id, selected?.status]);
  useEffect(() => {
    if (tool !== "console" || !selected) return;
    if (selected.status !== "stopped" && selected.status !== "crashed") return;
    void loadTool("overview", selected);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tool, selected?.status]);
  const paletteItems = useMemo(() => {
    const items: {
      id: string;
      label: string;
      hint: string;
      run: () => void;
    }[] = [
      {
        id: "create",
        label: tr("New server"),
        hint: tr("Create a new server"),
        run: () => void openCreate(),
      },
      {
        id: "import",
        label: tr("Import an existing server"),
        hint: tr("Folder or zip"),
        run: () => setModal("import"),
      },
    ];
    for (const instance of instances) {
      items.push({
        id: `open-${instance.id}`,
        label: tr("Manage {0}", [instance.name]),
        hint: `${instance.core} ${instance.version}`,
        run: () => openManage(instance),
      });
      items.push({
        id: `console-${instance.id}`,
        label: tr("{0} console", [instance.name]),
        hint: tr("Live log"),
        run: () => void openConsole(instance),
      });
      items.push({
        id: `toggle-${instance.id}`,
        label:
          instance.status === "running"
            ? tr("Stop {0}", [instance.name])
            : tr("Start {0}", [instance.name]),
        hint: instance.status === "running" ? tr("Stop server") : tr("Start server"),
        run: () => void toggle(instance),
      });
      items.push({
        id: `folder-${instance.id}`,
        label: tr("Open {0} folder", [instance.name]),
        hint: instance.path,
        run: () => void openServerFolder(instance),
      });
    }
    return items;
  }, [instances]);
  const paletteMatches = useMemo(() => {
    const needle = paletteQuery.trim().toLowerCase();
    if (!needle) return paletteItems.slice(0, 12);
    return paletteItems
      .filter(
        (item) =>
          item.label.toLowerCase().includes(needle) ||
          item.hint.toLowerCase().includes(needle),
      )
      .slice(0, 12);
  }, [paletteItems, paletteQuery]);
  useEffect(() => {
    const shortcut = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteQuery("");
        setPaletteIndex(0);
        setPaletteOpen((previous) => !previous);
      }
    };
    document.addEventListener("keydown", shortcut);
    return () => document.removeEventListener("keydown", shortcut);
  }, []);
  useEffect(() => {
    if (page !== "instance") return;
    const nav = navRef.current;
    const active = nav?.querySelector<HTMLElement>("button.selected");
    if (!nav) return;
    if (!active) {
      setNavMark((now) => ({ ...now, ready: false }));
      return;
    }
    setNavMark({
      top: active.offsetTop,
      height: active.offsetHeight,
      ready: true,
    });
  }, [tool, page]);
  function syncNavState() {
    setNavState({
      back: historyAt.current > 0,
      forward: historyAt.current < historyRef.current.length - 1,
    });
  }
  useEffect(() => {
    const point: NavPoint = {
      page,
      instanceId: selected?.id ?? null,
      tool,
      ...(page === "instance" && tool === "files" ? { browsePath, activeFile } : {}),
      ...(page === "instance" && tool === "addons"
        ? { browse: browseOpen, project: projectView?.hit ?? null }
        : {}),
      ...(modalShell[modal] === "as-panel" && modal !== "create" ? { panel: modal } : {}),
    };
    const current = historyRef.current[historyAt.current];
    const same = !!current && samePlace(current, point) && current.panel === point.panel;
    if (
      travelling.current &&
      current &&
      current.page === "instance" &&
      point.page === "instance" &&
      current.instanceId === point.instanceId &&
      current.tool === "overview" &&
      point.tool === "console"
    ) {
      historyRef.current[historyAt.current] = point;
      travelling.current = false;
      syncNavState();
      return;
    }
    if (travelling.current) {
      if (same) travelling.current = false;
      syncNavState();
      return;
    }
    if (same) return;
    if (
      current &&
      current.page === "instance" &&
      point.page === "instance" &&
      current.instanceId === point.instanceId &&
      current.tool === "overview" &&
      point.tool === "console"
    ) {
      historyRef.current[historyAt.current] = point;
      travelling.current = false;
      syncNavState();
      return;
    }
    const next = [
      ...historyRef.current.slice(0, historyAt.current + 1),
      point,
    ].slice(-60);
    historyRef.current = next;
    historyAt.current = next.length - 1;
    if (onWeb) {
      try {
        if (next.length === 1) window.history.replaceState({ at: 0 }, "");
        else window.history.pushState({ at: historyAt.current }, "");
      } catch {}
    }
    syncNavState();
  }, [page, selected, tool, browsePath, activeFile, browseOpen, projectView?.hit.projectId, modal]);
  useEffect(() => {
    if (!onWeb) return;
    function onPop(event: PopStateEvent) {
      const at = (event.state as { at?: number } | null)?.at;
      if (typeof at !== "number") return;
      const step = at - historyAt.current;
      if (step) travelTo(step);
    }
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  });
  contextBuilder.current = (target: HTMLElement) => {
    const items: ContextItem[] = [];
    const field = target.closest("input, textarea, [contenteditable='true'], .cm-content") as HTMLElement | null;
    const editable =
      !!field &&
      !(field instanceof HTMLInputElement && ["checkbox", "radio", "range", "button", "file"].includes(field.type)) &&
      !((field as HTMLInputElement).readOnly || (field as HTMLInputElement).disabled);
    const inputPick =
      field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement
        ? field.value.slice(field.selectionStart ?? 0, field.selectionEnd ?? 0)
        : "";
    const picked = inputPick || (window.getSelection()?.toString() ?? "");
    if (editable) {
      field!.focus();
      items.push(
        { label: tr("Cut"), icon: "scissors", hint: "Ctrl+X", disabled: !picked, onSelect: () => document.execCommand("cut") },
        { label: tr("Copy"), icon: "copy", hint: "Ctrl+C", disabled: !picked, onSelect: () => document.execCommand("copy") },
        {
          label: tr("Paste"),
          icon: "paste",
          hint: "Ctrl+V",
          onSelect: () => {
            field!.focus();
            void navigator.clipboard
              .readText()
              .then((text) => {
                field!.focus();
                document.execCommand("insertText", false, text);
              })
              .catch(() => setError(tr("Press Ctrl+V to paste.")));
          },
        },
        { label: tr("Select all"), icon: "select-all", hint: "Ctrl+A", onSelect: () => { field!.focus(); document.execCommand("selectAll"); } },
        "-",
      );
    } else if (picked.trim()) {
      items.push({ label: tr("Copy"), icon: "copy", hint: "Ctrl+C", onSelect: () => void navigator.clipboard.writeText(picked) }, "-");
    }
    const link = target.closest("a[href]") as HTMLAnchorElement | null;
    if (link && /^https?:/.test(link.href)) {
      items.push(
        { label: tr("Open link"), icon: "external", onSelect: () => (onWeb ? void window.open(link.href, "_blank", "noopener") : void openUrl(link.href)) },
        { label: tr("Copy link"), icon: "copy", onSelect: () => void navigator.clipboard.writeText(link.href) },
        "-",
      );
    }
    const cardId = (target.closest("[data-server-id]") as HTMLElement | null)?.dataset.serverId;
    const server = cardId ? instances.find((one) => one.id === cardId) : page === "instance" && !editable ? selected : null;
    if (server) {
      const off = server.status === "stopped";
      const local = !onWeb && !server.remoteHost && !server.panelHost;
      if (cardId && !(page === "instance" && selected?.id === server.id)) {
        items.push({ label: tr("Open"), icon: "arrow", onSelect: () => openManage(server) });
      }
      items.push(
        off
          ? { label: t("card.start"), icon: "play", disabled: !!server.checking || !!server.unreachable || !!server.missing, onSelect: () => void toggle(server) }
          : { label: t("card.stop"), icon: "stop", disabled: server.status === "stopping", onSelect: () => void toggle(server) },
      );
      if (!off) {
        items.push({ label: tr("Restart"), icon: "refresh", disabled: server.status === "stopping", onSelect: () => void restartInstance(server) });
        items.push({
          label: tr("Console"),
          icon: "terminal",
          onSelect: () => {
            if (selected?.id !== server.id || page !== "instance") openManage(server);
            void loadTool("console", server);
          },
        });
      }
      if (local) items.push({ label: t("card.folder"), icon: "folder", onSelect: () => void openServerFolder(server) });
      items.push({ label: t("card.rename"), icon: "pencil", onSelect: () => void renameInstance(server) });
      if (local) items.push({ label: tr("Duplicate"), icon: "copy", disabled: !off, onSelect: () => void duplicateInstance(server) });
      items.push({ label: tr("Copy name"), icon: "copy", onSelect: () => void navigator.clipboard.writeText(server.name) });
      items.push({ label: tr("Move to trash"), icon: "trash", danger: true, disabled: !off, onSelect: () => void trashInstance(server) });
      items.push("-");
    }
    if (!editable) {
      items.push(
        { label: tr("Back"), icon: "arrow-left", disabled: !navState.back, onSelect: () => travelTo(-1) },
        { label: tr("Forward"), icon: "arrow", disabled: !navState.forward, onSelect: () => travelTo(1) },
        { label: tr("Refresh"), icon: "refresh", onSelect: () => { void refresh(); if (page === "instance" && selected && modal === "none") void loadTool(tool, selected, true); } },
        "-",
      );
      if (page === "home" && modal === "none") {
        items.push(
          { label: tr("Create server"), icon: "plus", onSelect: () => void openCreate() },
          { label: tr("Import server"), icon: "import", onSelect: () => setModal("import") },
        );
      }
      if (page === "instance" && modal === "none") items.push({ label: tr("My servers"), icon: "home", onSelect: goHome });
      if (!onWeb) items.push({ label: tr("Settings"), icon: "settings", onSelect: () => setModal("settings") });
    }
    return items;
  };
  function travelTo(step: number) {
    const target = historyAt.current + step;
    const point = historyRef.current[target];
    if (!point) return;
    const from = historyRef.current[historyAt.current];
    historyAt.current = target;
    travelling.current = true;
    setModal(point.panel ?? "none");
    if (from && samePlace(from, point)) {
      syncNavState();
      return;
    }
    if (point.page === "home" || !point.instanceId) {
      setPage("home");
      setSelectedId(null);
      
      syncNavState();
      return;
    }
    const instance = instances.find((item) => item.id === point.instanceId);
    if (!instance) {
      setPage("home");
      setSelectedId(null);
      
      syncNavState();
      return;
    }
    setPage("instance");
    setSelectedId(instance.id);
    if (point.tool === "addons" && tool === "addons" && instance.id === selected?.id) {
      setBrowseOpen(!!point.browse);
      if (point.project) {
        if (projectView?.hit.projectId !== point.project.projectId) void openProject(point.project);
      } else {
        setProjectView(null);
      }
      travelling.current = false;
      syncNavState();
      return;
    }
    if (point.tool === "files" && tool === "files" && instance.id === selected?.id) {
      void (async () => {
        try {
          if ((point.browsePath ?? "") !== browsePath) await browseTo(point.browsePath ?? "");
          if (point.activeFile) {
            await openFile(point.activeFile);
          } else {
            stashActive();
            setActiveFile(null);
            setFileImage(null);
            setFileContent("");
            setFileSaved("");
          }
        } catch (cause) {
          setError(String(cause));
          travelling.current = false;
        }
      })();
      syncNavState();
      return;
    }
    void loadTool(point.tool, instance).then(async () => {
      if (point.tool === "addons") {
        setBrowseOpen(!!point.browse);
        if (point.project) void openProject(point.project);
        return;
      }
      if (point.tool !== "files") return;
      if (point.browsePath) await browseTo(point.browsePath);
      if (point.activeFile) await openFile(point.activeFile);
    });
    syncNavState();
  }
  useEffect(() => {
    function onMouse(event: MouseEvent) {
      if (onWeb) return;
      if (event.button !== 3 && event.button !== 4) return;
      event.preventDefault();
      travelTo(event.button === 3 ? -1 : 1);
    }
    function block(event: MouseEvent) {
      if (onWeb) return;
      if (event.button === 3 || event.button === 4) event.preventDefault();
    }
    window.addEventListener("mouseup", onMouse);
    window.addEventListener("mousedown", block);
    return () => {
      window.removeEventListener("mouseup", onMouse);
      window.removeEventListener("mousedown", block);
    };
  });
  const updateStageRef = useRef(updateState.stage);
  updateStageRef.current = updateState.stage;
  useEffect(() => {
    if (onWeb) return;
    const timer = setTimeout(() => void checkForUpdate(true), 4000);
    const again = setInterval(() => {
      if (updateStageRef.current === "idle" || updateStageRef.current === "none") void checkForUpdate(true);
    }, 10 * 60_000);
    return () => { clearTimeout(timer); clearInterval(again); };
  }, []);
  const browsePage = selected ? (
        <div className="tool-content browse-page">
            {projectView ? (
            <div className="project-page">
              <button className="rail-back page-back" onClick={() => closeProject()}>
                <Icon name="arrow" />
                {tr("Back to list")}</button>
              {(() => {
                const { hit, detail, versions, tab } = projectView;
                const title = detail?.title ?? hit.title;
                const icon = detail?.iconUrl ?? hit.iconUrl;
                const gameVersions = detail?.gameVersions ?? hit.versions;
                const supported = gameVersions.includes(selected.version);
                const newest = gameVersions.length ? [...releaseVersions(gameVersions)].sort(compareVersions).at(-1) : null;
                const installButton = installedProjectIds.has(hit.projectId) ? (
                  <span className="catalog-done">
                    {tr("Installed")}
                  </span>
                ) : installing.has(hit.projectId) ? (
                  <button className="button secondary installing" disabled>
                    {installProgressLabel(hit.projectId)
                      ? tr("Downloading {0}", [installProgressLabel(hit.projectId)])
                      : tr("Installing…")}</button>
                ) : (
                  <button className="button primary" onClick={() => void installWithDependencies(hit)}>
                    <Icon name="import" />
                    {tr("Install")}</button>
                );
                const links = detail
                  ? [
                      detail.sourceUrl && { label: tr("Source"), url: detail.sourceUrl, icon: "console" },
                      detail.issuesUrl && { label: tr("Issues"), url: detail.issuesUrl, icon: "alert" },
                      detail.wikiUrl && { label: tr("Wiki"), url: detail.wikiUrl, icon: "file" },
                      detail.discordUrl && { label: "Discord", url: detail.discordUrl, icon: "message" },
                      { label: tr("View on Modrinth"), url: detail.pageUrl, icon: "arrow" },
                    ].filter((one): one is { label: string; url: string; icon: string } => !!one)
                  : [];
                return (
                  <>
                    <header className="project-head" data-catalog-id={hit.projectId}>
                      {icon ? (
                        <img className="project-icon" src={icon} alt="" />
                      ) : (
                        <span className="project-icon empty"><Icon name="archive" /></span>
                      )}
                      <div className="project-title">
                        <h2>{title}</h2>
                        <p>{detail?.description ?? hit.description}</p>
                        <div className="catalog-facts">
                          {(detail?.team.length ?? 0) > 0 ? (
                            <span><Icon name="user" />{detail!.team.join(", ")}</span>
                          ) : hit.author ? (
                            <span><Icon name="user" />{hit.author}</span>
                          ) : null}
                          {(detail?.downloads ?? hit.downloads) != null && <span><Icon name="import" />{compactCount(detail?.downloads ?? hit.downloads ?? 0)}</span>}
                          {(detail?.followers ?? hit.follows) != null && <span><Icon name="heart" />{compactCount(detail?.followers ?? hit.follows ?? 0)}</span>}
                          {detail?.updated && <span data-tip={date(Math.floor(Date.parse(detail.updated) / 1000))}><Icon name="clock" />{tr("Update")} {relativeTime(Math.floor(Date.parse(detail.updated) / 1000))}</span>}
                          {detail?.license && <span><Icon name="shield" />{detail.license}</span>}
                        </div>
                        <div className="project-chips">
                          {(detail?.categories ?? []).filter((one) => !isLoaderName(one)).map((one) => {
                            const known = CATALOG_CATEGORIES.find((row) => row.id === one);
                            return <span className="chip subtle" key={one}>{known ? tr(known.ko) : one}</span>;
                          })}
                          <span className="catalog-loaders">
                            {sortLoaders(detail?.loaders ?? hit.loaders, selected.core).map(loaderChip)}
                          </span>
                          {gameVersions.length > 0 && <span className="project-span">{versionSpan(gameVersions)}</span>}
                        </div>
                      </div>
                      <div className="project-actions">
                        {supported ? (
                          <span className="catalog-fit ok"><Icon name="check" />{tr("Supports {0}", [selected.version])}</span>
                        ) : newest ? (
                          <span className="catalog-fit warn"><Icon name="alert" />{tr("Up to {0}", [newest])}</span>
                        ) : (
                          <span />
                        )}
                        {installButton}
                      </div>
                    </header>
                    <SegmentedControl
                      className="project-tabs"
                      value={tab}
                      onChange={(next) => setProjectView((now) => (now ? { ...now, tab: next } : now))}
                      options={[
                        { value: "overview", label: tr("Overview") },
                        { value: "gallery", label: `${tr("Gallery")}${detail ? ` ${detail.gallery.length}` : ""}` },
                        { value: "versions", label: `${tr("Version")}${versions ? ` ${versions.length}` : ""}` },
                        { value: "changelog", label: tr("Changelog") },
                      ]}
                    />
                    <div className="project-body">
                      {!detail ? (
                        <div className="catalog-card skeleton project-skeleton" />
                      ) : tab === "overview" ? (
                        <div className="project-overview">
                          {detail.bodyHtml ? (
                            <article className="md-body" onClick={openBodyLink} dangerouslySetInnerHTML={{ __html: detail.bodyHtml }} />
                          ) : (
                            <p className="muted">{tr("No description.")}</p>
                          )}
                          {links.length > 0 && (
                            <aside className="project-links">
                              {sideLabel(detail) && (
                                <p className="project-side">
                                  <Icon name={sideLabel(detail)!.icon} />
                                  {sideLabel(detail)!.text}
                                </p>
                              )}
                              <h4>{tr("Links")}</h4>
                              {links.map((one) => (
                                <button className="rail-pill" key={one.url} onClick={() => void openExternal(one.url)}>
                                  <Icon name={one.icon} />{one.label}
                                </button>
                              ))}
                            </aside>
                          )}
                        </div>
                      ) : tab === "gallery" ? (
                        detail.gallery.length ? (
                          <div className="project-gallery">
                            {detail.gallery.map((shot, index) => (
                              <figure key={shot.url} onClick={() => setShotView({ list: detail.gallery, at: index })}>
                                <img src={shot.url} alt={shot.title ?? ""} loading="lazy" />
                                {(shot.title || shot.description) && (
                                  <figcaption>
                                    {shot.title && <strong>{shot.title}</strong>}
                                    {shot.description && <span>{shot.description}</span>}
                                  </figcaption>
                                )}
                              </figure>
                            ))}
                          </div>
                        ) : (
                          <EmptyState icon="map" title={tr("The gallery is empty.")} />
                        )
                      ) : tab === "versions" ? (
                        versions?.length ? (
                          <div className="project-versions">
                            {versions.slice(0, projectShown).map((one) => (
                              <div className={`project-version ${one.fits ? "" : "dim"}`} key={one.id}>
                                <div className="pv-name">
                                  <strong>{one.name}</strong>
                                  <span className={`chip subtle pv-${one.channel}`}>{one.channel}</span>
                                  {one.fits && addons.some((have) => addonCards[have.fileName]?.projectId === hit.projectId && have.version === one.name) && <span className="chip subtle">{tr("Now")}</span>}
                                </div>
                                <span className="pv-games">{versionSpan(one.gameVersions)}</span>
                                <span className="catalog-loaders">{one.loaders.map(loaderChip)}</span>
                                <span className="pv-meta" data-tip={date(Math.floor(Date.parse(one.published) / 1000))}>{relativeTime(Math.floor(Date.parse(one.published) / 1000))}</span>
                                <span className="pv-meta"><Icon name="import" />{compactCount(one.downloads)}</span>
                                <button
                                  className="button secondary small"
                                  disabled={!one.fits || installing.has(hit.projectId)}
                                  data-tip={one.fits ? undefined : tr("Not compatible with this server ({0} · {1}).", [selected.core, selected.version])}
                                  onClick={() => void installVersion(hit, one)}
                                >
                                  {installedProjectIds.has(hit.projectId) ? tr("Use this version") : tr("Install")}</button>
                              </div>
                            ))}
                            {versions.length > projectShown && (
                              <button className="button ghost small project-more" onClick={() => setProjectShown((n) => n + 40)}>
                                {tr("More ({0})", [versions.length - projectShown])}
                              </button>
                            )}
                          </div>
                        ) : (
                          <EmptyState icon="tag" title={versions ? tr("No versions listed.") : tr("Searching…")} />
                        )
                      ) : versions?.some((one) => one.changelogHtml) ? (
                        <div className="project-changelog">
                          {versions.filter((one) => one.changelogHtml).slice(0, projectShown).map((one) => (
                            <section key={one.id}>
                              <h3>
                                {one.name}
                                <span className={`chip subtle pv-${one.channel}`}>{one.channel}</span>
                                <small>{relativeTime(Math.floor(Date.parse(one.published) / 1000))}</small>
                              </h3>
                              <article className="md-body" onClick={openBodyLink} dangerouslySetInnerHTML={{ __html: one.changelogHtml ?? "" }} />
                            </section>
                          ))}
                          {versions.filter((one) => one.changelogHtml).length > projectShown && (
                            <button className="button ghost small project-more" onClick={() => setProjectShown((n) => n + 40)}>
                              {tr("More ({0})", [versions.filter((one) => one.changelogHtml).length - projectShown])}
                            </button>
                          )}
                        </div>
                      ) : (
                        <EmptyState icon="file" title={tr("No changelog provided.")} />
                      )}
                    </div>
                  </>
                );
              })()}
            </div>
            ) : (
            <>
            <div className="browse-head">
              <button
                className="rail-back page-back"
                onClick={() => setBrowseOpen(false)}
              >
                <Icon name="arrow" />
                {tr("Back")}</button>
              <h2>
                {browseKind === "modpack"
                  ? tr("Browse modpacks")
                  : browseKind === "datapack"
                    ? tr("Browse datapacks")
                  : addonKind(selected.core) === "plugin"
                    ? tr("Browse plugins")
                    : tr("Browse mods")}
                <small>{selected.core} {selected.version} · {selected.name}</small>
              </h2>
              <form
                className="search browse-search"
                onSubmit={(event) => {
                  event.preventDefault();
                  if (browseKind === "modpack") {
                    void searchPacks(catalogQuery);
                    return;
                  }
                  if (browseKind === "datapack") return;
                  void searchCatalog(catalogQuery, 0);
                }}
              >
                <Icon name="search" />
                <input
                  autoFocus
                  value={catalogQuery}
                  placeholder={browseKind === "modpack" ? tr("Search modpacks by name") : browseKind === "datapack" ? tr("Search datapacks by name") : tr("Search by name or function · leave empty for popular results")}
                  onChange={(event) => setCatalogQuery(event.target.value)}
                />
              </form>
            </div>
            {hasWorlds(selected.core) && (
            <div className="extras-tabs browse-kinds">
              {[
                { id: "addon" as const, label: addonKind(selected.core) === "plugin" ? tr("Plugins") : tr("Mods") },
                ...(addonKind(selected.core) === "plugin" ? [] : [{ id: "modpack" as const, label: tr("Modpack") }]),
                { id: "datapack" as const, label: tr("Datapacks") },
              ].map((one) => (
                <button
                  key={one.id}
                  className={browseKind === one.id ? "chosen" : ""}
                  onClick={() => {
                    setBrowseKind(one.id);
                    if (one.id === "modpack" && !packHits.length) {
                      void searchPacks("");
                    }
                  }}
                >
                  {one.label}
                </button>
              ))}
            </div>
            )}
            {browseKind === "datapack" ? (
              <DatapackPanel serverId={selected.id} query={catalogQuery} onNotice={setNotice} />
            ) : browseKind === "modpack" ? (
              <div className="browse-main packs">
                {packBusy ? (
                  <div className="catalog-grid">
                    {[1, 2, 3, 4].map((k) => <div className="catalog-card skeleton" key={k} />)}
                  </div>
                ) : packHits.length ? (
                  <div className="catalog-grid">
                    {packHits.map((hit) => (
                      <article className="catalog-card" key={hit.projectId}>
                        {hit.iconUrl ? (
                          <img className="catalog-icon" src={hit.iconUrl} alt="" loading="lazy" />
                        ) : (
                          <span className="catalog-icon empty"><Icon name="archive" /></span>
                        )}
                        <div className="catalog-main">
                          <div className="catalog-title">
                            <strong>{hit.title}</strong>
                            {hit.author && <span>by {hit.author}</span>}
                          </div>
                          <p>{hit.description}</p>
                          <div className="catalog-facts">
                            {hit.downloads != null && <span><Icon name="import" />{compactCount(hit.downloads)}</span>}
                            {hit.follows != null && <span><Icon name="heart" />{compactCount(hit.follows)}</span>}
                          </div>
                        </div>
                        <div className="catalog-side">
                          <span />
                          <button
                            className="button primary small"
                            disabled={!!packGetting}
                            onClick={() => {
                              setBrowseOpen(false);
                              setModal("mrpack");
                              void getPack(hit);
                            }}
                          >
                            {packGetting === hit.projectId ? tr("Downloading…") : tr("Download")}
                          </button>
                        </div>
                      </article>
                    ))}
                  </div>
                ) : (
                  <EmptyState icon="search" title={tr("No matching modpack found.")} />
                )}
              </div>
            ) : (
            <div className="browse-body">
              <aside className="browse-rail">
                <div className="rail-group">
                  <h4>{tr("Category")}</h4>
                  <button
                    className={`rail-pill ${catalogCategories.length === 0 ? "on" : ""}`}
                    onClick={() => { setCatalogCategories([]); void searchCatalog(catalogQuery, 0, []); }}
                  >
                    <Icon name="grid" />{tr("All")}
                  </button>
                  {CATALOG_CATEGORIES.map((one) => (
                    <button
                      key={one.id}
                      className={`rail-pill ${catalogCategories[0] === one.id ? "on" : ""}`}
                      onClick={() => { setCatalogCategories([one.id]); void searchCatalog(catalogQuery, 0, [one.id]); }}
                    >
                      <Icon name={one.icon} />{tr(one.ko)}
                    </button>
                  ))}
                </div>
                <div className="rail-group">
                  <h4>{tr("Version")}</h4>
                  <div className={`rail-versions ${browseVersionsOpen ? "open" : ""}`}>
                    {(browseVersionsOpen ? browseVersionList : browseVersionList.slice(0, 8)).map((version) => {
                      const on = catalogVersions.includes(version);
                      return (
                        <label key={version} className={on ? "on" : ""}>
                          <input
                            type="checkbox"
                            checked={on}
                            onChange={() => {
                              const next = on ? catalogVersions.filter((one) => one !== version) : [...catalogVersions, version];
                              setCatalogVersions(next);
                              void searchCatalog(catalogQuery, 0, catalogCategories, catalogEnvironment, next, catalogLoader);
                            }}
                          />
                          <span>{version}</span>
                          {version === selected.version && <small>{tr("this server")}</small>}
                        </label>
                      );
                    })}
                  </div>
                  {browseVersionList.length > 8 && (
                    <button className="button ghost small rail-more" onClick={() => setBrowseVersionsOpen((open) => !open)}>
                      {browseVersionsOpen ? tr("Collapse") : tr("More ({0})", [browseVersionList.length - 8])}
                    </button>
                  )}
                </div>
              </aside>
              <div className="browse-main">
                {catalogBusy ? (
                  <div className="catalog-grid">
                    {[1, 2, 3, 4].map((n) => <div className="catalog-card skeleton" key={n} />)}
                  </div>
                ) : catalogHits.length ? (
                  <div className="catalog-grid">
                    {catalogHits.map((item) => {
                      const supported = item.versions.includes(selected.version);
                      const newest = item.versions.length ? [...releaseVersions(item.versions)].sort(compareVersions).at(-1) : null;
                      return (
                        <article
                          className="catalog-card opens"
                          key={`${item.source}-${item.projectId}`}
                          data-catalog-id={item.projectId}
                          onClick={(event) => {
                            if ((event.target as HTMLElement).closest("button, a")) return;
                            void openProject(item);
                          }}
                        >
                          {item.iconUrl ? (
                            <img className="catalog-icon" src={item.iconUrl} alt="" loading="lazy" />
                          ) : (
                            <span className="catalog-icon empty"><Icon name="archive" /></span>
                          )}
                          <div className="catalog-main">
                            <div className="catalog-title">
                              <strong>{item.title}</strong>
                              {item.author && <span>by {item.author}</span>}
                            </div>
                            <p>{item.description}</p>
                            <div className="catalog-facts">
                              {item.downloads != null && <span><Icon name="import" />{compactCount(item.downloads)}</span>}
                              {item.follows != null && <span><Icon name="heart" />{compactCount(item.follows)}</span>}
                              {item.updatedAt != null && <span data-tip={date(item.updatedAt)}><Icon name="clock" />{relativeTime(item.updatedAt)}</span>}
                              {item.loaders.length > 0 && (
                                <span className="catalog-loaders">
                                  {sortLoaders(item.loaders, selected.core).slice(0, 8).map(loaderChip)}
                                  {item.loaders.length > 8 && <b>+{item.loaders.length - 8}</b>}
                                </span>
                              )}
                              {item.versions.length > 0 && <span>{versionSpan(item.versions)}</span>}
                            </div>
                          </div>
                          <div className="catalog-side">
                            {supported ? (
                              <span className="catalog-fit ok"><Icon name="check" />{tr("Supports {0}", [selected.version])}</span>
                            ) : newest ? (
                              <span className="catalog-fit warn"><Icon name="alert" />{tr("Up to {0}", [newest])}</span>
                            ) : (
                              <span />
                            )}
                            {installedProjectIds.has(item.projectId) ? (
                              <span className="catalog-done">
                                {tr("Installed")}
                              </span>
                            ) : installing.has(item.projectId) ? (
                              <button className="button secondary small installing" disabled>
                                {installProgressLabel(item.projectId)
                                  ? tr("Downloading {0}", [installProgressLabel(item.projectId)])
                                  : tr("Installing…")}</button>
                            ) : item.installable ? (
                              <button
                                className="button primary small"
                                onClick={() => void installWithDependencies(item)}
                              >
                                {tr("Install")}</button>
                            ) : (
                              <button
                                className="button ghost small"
                                onClick={() => void openExternal(item.pageUrl)}
                              >
                                {tr("Open page")}</button>
                            )}
                          </div>
                        </article>
                      );
                    })}
                  </div>
                ) : (
                  <EmptyState
                    icon="search"
                    title={tr("No matching items found.")}
                    hint={tr("Try a shorter search or broaden the version and category filters on the left.")}
                  />
                )}
                <div className="browse-pager">
                  <button
                    className="button ghost small"
                    disabled={catalogPage === 0 || catalogBusy}
                    onClick={() => void searchCatalog(catalogQuery, catalogPage - 1)}
                  >
                    {tr("Previous")}</button>
                  <span>{catalogPage + 1}{tr("suffix.page")}</span>
                  <button
                    className="button ghost small"
                    disabled={!catalogMore || catalogBusy}
                    onClick={() => void searchCatalog(catalogQuery, catalogPage + 1)}
                  >
                    {tr("Next")}</button>
                </div>
              </div>
            </div>
            )}
            </>
            )}
        </div>
  ) : null;
  const percent =
    download?.stage === "installing"
      ? null
      : download?.total
        ? Math.min(
            100,
            Math.round(((download.received ?? 0) / download.total) * 100),
          )
        : null;
  const logTabs = (
    <div className="wb-seg" role="tablist" aria-label={tr("Logs")}>
        {[
          { id: "read", label: tr("Logs") },
          { id: "events", label: tr("Summary") },
          { id: "crash", label: crashReports.length ? tr("Crashes {0}", [crashReports.length]) : tr("Crashes") },
          { id: "runs", label: tr("Run history") },
          { id: "audit", label: tr("App log") },
        ].map((one) => (
          <button
            key={one.id}
            type="button"
            role="tab"
            aria-selected={logView === one.id}
            className={logView === one.id ? "on" : ""}
            onClick={() => setLogView(one.id)}
          >
            {one.label}
          </button>
        ))}
    </div>
  );
  const propsChanged = properties.filter((entry) => {
    const before = savedProperties.find((one) => one.key === entry.key);
    return before ? before.value !== entry.value : true;
  });

  const lagCardView = () => selected && (
    <Card
      className="lag-card"
      title={tr("Lag diagnosis")}
      aside={
        lagReport ? (
          !lagReport.hasLive ? (
            <Badge tone="subtle">{tr("No add-on")}</Badge>
          ) : lagReport.lagging ? (
            <Badge tone="danger">{tr("Lagging")}{lagReport.tps != null ? ` · ${lagReport.tps.toFixed(1)} TPS` : ""}</Badge>
          ) : (
            <Badge tone="ok">{tr("Smooth")}{lagReport.tps != null ? ` · ${lagReport.tps.toFixed(1)} TPS` : ""}</Badge>
          )
        ) : (
          <Badge tone="subtle">{tr("Checking")}</Badge>
        )
      }
    >
      {!lagReport ? (
        <p className="muted lag-note">{tr("Collecting server and system metrics.")}</p>
      ) : (
        <>
          {!lagReport.hasLive && (
            <p className="muted lag-note">
              {tr("Monolab {0} is required to view server details (entities, chunks and heap). Only memory settings and CPU have been checked.", [addon?.kind ?? tr("add-on")])}
            </p>
          )}
          {lagReport.findings.length === 0 ? (
            <p className="muted lag-note">
              {lagReport.lagging
                ? tr("No cause found inside the server. A plugin or mod may be responsible. Try disabling recent additions one at a time, or use the spark profiler to find what's slowing down ticks.")
                : tr("If TPS drops, the cause appears here.")}
            </p>
          ) : (
            <ul className="lag-list">
              {lagReport.findings.map((one, index) => {
                const line = lagLine(one);
                const action = line.action;
                const key = `lag-${index}`;
                return (
                  <li key={key} className={`lag-row ${one.level}`}>
                    <span className="lag-mark" aria-hidden="true" />
                    <div className="lag-body">
                      <strong>{line.title}</strong>
                      {line.detail && <small>{line.detail}</small>}
                    </div>
                    {action && (
                      <button
                        type="button"
                        className={`button small ${action.kind === "console" ? "danger" : "ghost"} ${action.kind === "copy" ? "copy-mark" : ""} ${copiedKey === key ? "done" : ""}`}
                        onClick={() => void runLagAction(action, key)}
                      >
                        {action.kind === "copy" && <Icon name={copiedKey === key ? "check" : "copy"} />}
                        {action.kind === "overview"
                          ? tr("Open overview")
                          : action.kind === "properties"
                            ? tr("Open server settings")
                            : action.kind === "copy"
                              ? tr("Copy command")
                              : tr("Remove")}
                      </button>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
          {lagReport.spark && (lagReport.spark.available || lagReport.spark.installable || selected.core === "Paper" || selected.core === "Purpur" || selected.core === "Folia" || selected.core === "Leaf") && (
            <div className="spark-row">
              <div className="lag-body">
                <strong>{tr("spark profiler")}</strong>
                <small>
                  {lagReport.spark.available
                    ? sparkBusy
                      ? tr("Profiling… The results link will appear here in 30 seconds.")
                      : tr("Profiles for 30 seconds to find plugins or mods slowing down ticks. View the results on the spark website.")
                    : lagReport.spark.installable
                      ? tr("Not installed yet. Install it to see which mods are slowing down ticks. Applies on the next start.")
                      : tr("Included with Paper 1.21 and later. For this version, add spark to the plugins folder manually.")}
                  {sparkReport && !sparkBusy && (
                    <> · {tr("Last result")} {relativeTime(Math.floor(sparkReport.at / 1000))}</>
                  )}
                </small>
              </div>
              {lagReport.spark.available ? (
                <div className="spark-actions">
                  {sparkReport && (
                    <button type="button" className="button small ghost" onClick={() => void openExternal(sparkReport.url)}>
                      <Icon name="arrow" />{tr("View results")}
                    </button>
                  )}
                  <button type="button" className="button small secondary" disabled={sparkBusy} onClick={() => void startSpark()}>
                    <Icon name={sparkBusy ? "clock" : "gauge"} />{sparkBusy ? tr("Profiling") : tr("Profile for 30 s")}
                  </button>
                </div>
              ) : lagReport.spark.installable ? (
                <button type="button" className="button small secondary" onClick={() => void installSpark()}>
                  <Icon name="import" />{tr("Install spark")}
                </button>
              ) : (
                <button type="button" className="button small ghost" onClick={() => void openExternal("https://spark.lucko.me/download")}>
                  {tr("Download")}
                </button>
              )}
            </div>
          )}
        </>
      )}
    </Card>
  );

  const sidePanel = page === "instance" && (tool === "console" || (tool === "properties" && propsChanged.length > 0));
  const sideShown = !!selected && sidePanel && wide;
  const runControls = selected ? (
  <div className="wb-run">
    {!onWeb && !selected.remoteHost && !selected.panelHost && (
      <button type="button" className="wb-icon ghost" aria-label={t("card.folder")} data-tip={t("card.folder")} onClick={() => void openServerFolder(selected)}>
        <Icon name="folder" />
      </button>
    )}
    <button type="button" className="wb-icon ghost" aria-label={t("card.rename")} data-tip={t("card.rename")} onClick={() => void renameInstance(selected)}>
      <Icon name="pencil" />
    </button>
    {selected.status !== "stopped" && (
      <button type="button" className="wb-icon ghost danger" aria-label={tr("End task")} data-tip={tr("End task")} onClick={() => void forceStop(selected)}>
        <Icon name="alert" />
      </button>
    )}
    {selected.status === "stopped" && selected.unreachable && selected.remoteHost ? (
      <button type="button" className="button secondary wb-pill" onClick={() => void retrySsh(selected.remoteHost)}>
        <Icon name="refresh" />
        {tr("Retry connection")}
      </button>
    ) : selected.status === "stopped" ? (
      <button
        type="button"
        className="button primary wb-pill"
        disabled={!!selected.checking || !!selected.unreachable || !!selected.missing}
        onClick={() => void toggle(selected)}
      >
        <Icon name="play" />
        {t("card.start")}
      </button>
    ) : (
      <>
        <button
          type="button"
          className="wb-icon"
          aria-label={tr("Restart")}
          data-tip={tr("Restart")}
          disabled={selected.status === "stopping"}
          onClick={() => void restartInstance(selected)}
        >
          <Icon name="refresh" />
        </button>
        <button
          type="button"
          className="wb-icon danger"
          aria-label={t("card.stop")}
          data-tip={selected.status === "stopping" ? t("status.stopping") : t("card.stop")}
          disabled={selected.status === "stopping"}
          onClick={() => void toggle(selected)}
        >
          <Icon name="stop" />
        </button>
      </>
    )}
  </div>
  ) : null;

  const manage = selected && (
    <>
      {toolLoading && (
        <div className="tool-loading">
          {[1, 2, 3, 4].map((n) => (
            <div className="skeleton-row" key={n} />
          ))}
        </div>
      )}
      {updatePlan && selected && (
        <UpdateDialog
          rows={updatePlan}
          busy={updateBusy}
          listVersions={(projectId, kind) => commands.listAddonVersions(selected.id, projectId, kind)}
          onCancel={() => !updateBusy && setUpdatePlan(null)}
          onApply={(choices) => void runUpdatePlan(choices)}
        />
      )}
      {versionPick && selected && (
        <VersionDialog
          title={versionPick.title}
          iconUrl={versionPick.iconUrl}
          currentFile={versionPick.addon.fileName}
          versions={versionPick.list}
          gameVersion={selected.version}
          busy={dialogBusy}
          onCancel={() => !dialogBusy && setVersionPick(null)}
          onSwap={(version) => void swapVersion(version)}
        />
      )}
      {!toolLoading && tool === "console" && (
        <div className="tool-content overview-console">
          <div className="console-split">
          <div className="console-main">
          <div className="wb-head live-head">
            <h1>{tr("Console")}</h1>
            <ConsoleLevels serverId={selected.id} value={consoleLevel} onChange={setConsoleLevel} />
            <span className="live-spacer" />
            <label className="search console-search wb-search-field">
              <Icon name="search" />
              <input
                value={consoleFilter}
                onChange={(event) => setConsoleFilter(event.target.value)}
                placeholder={tr("Search logs")}
              />
            </label>
          </div>
          <div className="wb-console-box">
          <ConsoleOutput serverId={selected.id} filter={consoleFilter} level={consoleLevel} />
          <form
            className="console-input wb-console-input"
            onSubmit={(event) => {
              event.preventDefault();
              setHistoryIndex(-1);
              void sendConsoleCommand();
            }}
          >
            <div className="console-command-field">
              <input
                value={command}
                onChange={(event) => {
                  setCommand(event.target.value);
                  setHistoryIndex(-1);
                  setHeldHints(null);
                  setHintAt(0);
                }}
                onKeyDown={(event) => {
                  const list = shownHints.filter((one) => one.text);
                  if (event.key === "Tab" && list.length) {
                    event.preventDefault();
                    const next = heldHints ? (hintAt + 1) % list.length : hintAt;
                    setHeldHints(list);
                    setHintAt(next);
                    putCommand(list[next].text);
                    return;
                  }
                  if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
                  if (list.length) {
                    event.preventDefault();
                    const step = event.key === "ArrowUp" ? -1 : 1;
                    setHeldHints(list);
                    setHintAt((at) => (at + step + list.length) % list.length);
                    return;
                  }
                  if (!consoleHistory.length) return;
                  event.preventDefault();
                  const next =
                    event.key === "ArrowUp"
                      ? Math.min(historyIndex + 1, consoleHistory.length - 1)
                      : historyIndex - 1;
                  setHistoryIndex(next);
                  setCommand(next < 0 ? "" : consoleHistory[next]);
                }}
                placeholder={
                  tr("Type a command · Tab to autocomplete")
                }
                disabled={selected.status !== "running"}
              />
            </div>
            {shownHints.length > 0 && selected.status === "running" && (
              <div className="console-hints">
                {shownHints.map((one, index) => (
                  <button
                    type="button"
                    key={one.text || one.hint}
                    className={index === hintAt && one.text ? "first" : ""}
                    disabled={!one.text}
                    onMouseDown={(event) => {
                      event.preventDefault();
                      setHeldHints(shownHints.filter((two) => two.text));
                      setHintAt(index);
                      putCommand(one.text);
                    }}
                  >
                    <span>{one.text}</span>
                    <em>{one.hint}</em>
                  </button>
                ))}
              </div>
            )}
            <button
              className="wb-send"
              aria-label={tr("Send")}
              data-tip={tr("Send")}
              disabled={selected.status !== "running"}
            >
              <Icon name="arrow-up" />
            </button>
          </form>
          </div>
          </div>
          {!wide && <ServerStats server={{ id: selected.id, name: selected.name, panelHost: selected.panelHost, maxMemoryMib: kindOf(selected.core).family === "minecraft" ? selected.maxMemoryMib : 0, port: selected.port, status: selected.status, uptimeSeconds: selected.uptimeSeconds }} onOpenPlayer={() => void loadTool("players")} />}
          </div>
        </div>
      )}
      {!toolLoading && tool === "overview" && (
        <div className="tool-content overview">
          <div className="overview-scroll">
          <div className="wb-head overview-head">
            <h1>{tr("Overview")}</h1>
            {selected.status === "stopped" && runControls}
          </div>
          {selected.crashReason && (
            <Banner tone="warn">{tr("Exit reason")}: {trBack(selected.crashReason)}</Banner>
          )}
          {selected.status !== "stopped" && (
            <p className="locked-note">
              <Icon name="alert" />
              {tr("Stop the server before changing these settings.")}</p>
          )}
          {selected.panelHost ? (
            <PanelStartupCard
              id={selected.id}
              panelName={panelHosts[selected.panelHost] ?? tr("Panel")}
              onError={(message) => setError(message)}
              onNotice={(message) => setNotice(message)}
            />
          ) : (
          <>
          <div className={`overview-grid ${isNative(selected.core) ? "overview-one" : "overview-pair"}`}>
          {!isNative(selected.core) && (
          <fieldset className="overview-lock overview-lock-cell" disabled={selected.status !== "stopped"}>
            <div className="setting-card java-card">
              <span className="setting-title">{tr("Java executable")}</span>
              <div className="java-row">
                <Dropdown
                  value={javaPath || "java"}
                  onChange={setJavaPath}
                  options={[
                    {
                      value: "java",
                      label: tr("Auto"),
                      hint: selected.remoteHost ? tr("java on the Ubuntu PATH") : tr("Recommended"),
                    },
                    ...javaChoices.map((item) => ({
                      value: item.path,
                      label: `Java ${item.major}`,
                      hint: item.source,
                    })),
                    ...(javaPath &&
                    javaPath !== "java" &&
                    !javaChoices.some((item) => item.path === javaPath)
                      ? [{ value: javaPath, label: javaPath, hint: tr("Browse") }]
                      : []),
                  ]}
                />
              </div>
              {selected.remoteHost ? (
              <span className="title-actions java-actions">
                <button className="icon-button" aria-label={tr("Scan again")} data-tip={tr("Scans the SSH server for installed Java versions again.")} disabled={remoteJavas === null} onClick={() => void refreshRemoteJavas(selected.id)}>
                  <Icon name="refresh" />
                </button>
                <button className="icon-button" aria-label={tr("Enter path")} data-tip={tr("Enter the path to a Java executable that isn't listed.")} onClick={() => void typeRemoteJava()}>
                  <Icon name="pencil" />
                </button>
              </span>
              ) : (
              <span className="title-actions java-actions">
                <button className="icon-button" aria-label={tr("Scan again")} data-tip={tr("Scans this computer for installed Java versions again.")} onClick={() => void refreshJavaList()}>
                  <Icon name="refresh" />
                </button>
                <button className="icon-button" aria-label={tr("Install Java")} disabled={javaBusy} data-tip={javaBusy ? tr("Downloading…") : tr("Downloads and installs the right Java for this server from Adoptium.")} onClick={() => void getJava()}>
                  <Icon name="import" />
                </button>
                {!onWeb && (
                  <button className="icon-button" aria-label={tr("Browse")} data-tip={tr("Browse")} onClick={() => void pickJavaPath()}>
                    <Icon name="folder" />
                  </button>
                )}
              </span>
              )}
              {(() => {
                const picked = javaPath && javaPath !== "java" ? javaChoices.find((one) => one.path === javaPath) : null;
                const auto = selected.remoteHost
                  ? javaChoices.find((one) => one.source === "PATH default") ?? null
                  : javaNeed != null ? javaList.filter((one) => one.major >= javaNeed).sort((a, b) => a.major - b.major)[0] ?? null : null;
                const fits = picked
                  ? (javaNeed == null || picked.major >= javaNeed)
                  : selected.remoteHost && auto && javaNeed != null
                    ? auto.major >= javaNeed
                    : true;
                return (
                  <p className={`memory-note java-note ${fits ? "" : "warn"}`}>
                    <Icon name={fits ? "check" : "alert"} />
                    {javaNeed != null ? tr("This server needs Java {0}+", [javaNeed]) : tr("Checking Java requirements…")}
                    {selected.remoteHost && remoteJavas === null
                      ? ` · ${tr("Looking for Java on the SSH server…")}`
                      : picked
                      ? ` · ${tr("Selected")}: Java ${picked.major} (${picked.source})`
                      : javaPath && javaPath !== "java"
                        ? ` · ${tr("Selected")}: ${javaPath}`
                      : auto
                        ? ` · ${tr("Automatic")}: Java ${auto.major} (${auto.source})`
                        : selected.remoteHost
                          ? ` · ${tr("No Java on the Ubuntu PATH")}`
                        : javaNeed != null && javaList.length
                          ? ` · ${tr("No compatible Java — downloads on start")}`
                          : ""}
                    {!fits && <em>{picked ? tr("The selected Java version is too old.") : tr("This Java version is too old. Choose a newer one from the list.")}</em>}
                  </p>
                );
              })()}
            </div>

          </fieldset>
          )}
          <div className="setting-card core-card">
            <span className="setting-title">{tr("Core")}</span>
            <strong className="strip-core core-value">
              {`${coreLabel(selected.core, selected.version)}${selected.coreBuild ? ` · ${selected.coreBuild}` : ""}`}
            </strong>
            {isCustom(selected.core) ? (
            <p className="memory-note core-note">
              <Icon name="terminal" />
              {tr("Custom program · runs on this PC only")}
            </p>
            ) : isGame(selected.core) ? (
            <>
            <span className="title-actions">
              <button
                type="button"
                className="button secondary small"
                disabled={gameUpdating || selected.status !== "stopped"}
                data-tip={tr("Downloads the newest server files. Your worlds and settings stay.")}
                onClick={() => void updateGameFiles()}
              >
                <Icon name="refresh" />
                {gameUpdating ? tr("Updating…") : tr("Update server files")}
              </button>
            </span>
            <p className="memory-note core-note">
              <Icon name="gamepad" />
              {tr("Game server · runs on this PC only")}
            </p>
            </>
            ) : (
            <>
            <span className="title-actions">
              <button
                type="button"
                className="button secondary small"
                data-tip={tr("Change or roll back the Minecraft version.")}
                onClick={() => void openVersionDialog()}
              >
                <Icon name="tag" />
                {tr("Change version")}
                {buildNews?.behind && <span className="new-dot" aria-label={tr("A new build is available")} />}
              </button>
            </span>
            <p className={`memory-note core-note ${buildNews?.behind ? "warn" : ""}`}>
              <Icon name={buildNews?.behind ? "alert" : buildNews && !buildNews.unknown ? "check" : "tag"} />
              {buildNews == null
                ? tr("Checking for a newer build…")
                : buildNews.behind
                  ? `${tr("A new build is available")} · ${buildNews.current || "?"} → ${buildNews.newest}`
                  : buildNews.unknown
                    ? tr("Can't check for updates without the build number")
                    : tr("Latest build")}
            </p>
            </>
            )}
          </div>
          </div>
          <fieldset
            className="overview-lock"
            disabled={selected.status !== "stopped"}
          >
          {isCustom(selected.core) ? (
            <CustomProgramCard
              id={selected.id}
              folder={selected.path}
              port={selected.port}
              onError={(message) => setError(message)}
              onNotice={(message) => setNotice(message)}
            />
          ) : isGame(selected.core) ? (
            <GameLaunchCard
              id={selected.id}
              onError={(message) => setError(message)}
              onNotice={(message) => setNotice(message)}
            />
          ) : isNative(selected.core) ? (
            <div className="setting-card">
              <span className="setting-title">{tr("Launch settings")}</span>
              <p>
                {tr("This server doesn't run on Java, so memory and launch options aren't available. Change the port, game mode, whitelist and other options in Server settings (pumpkin.toml).")}
              </p>
            </div>
          ) : (
          <>
          <div className="overview-grid overview-one">
            <div className="setting-card memory-card">
              <span className="setting-title">
                {tr("Memory")}<strong className={`memory-value ${systemMemoryMib != null && memoryMib / systemMemoryMib > 0.6 ? "warn" : ""}`}>
                  {minMemoryMib === memoryMib
                    ? `${(memoryMib / 1024).toFixed(1)} GB`
                    : `${(minMemoryMib / 1024).toFixed(1)} ~ ${(memoryMib / 1024).toFixed(1)} GB`}
                </strong>
              </span>
              <div
                className="range-pair"
                onPointerDown={(event) => {
                  const box = event.currentTarget.getBoundingClientRect();
                  const ratio = (event.clientX - box.left) / box.width;
                  const at = 512 + ratio * (16384 - 512);
                  setLowOnTop(
                    minMemoryMib !== memoryMib &&
                      Math.abs(at - minMemoryMib) <= Math.abs(at - memoryMib),
                  );
                }}
              >
                {(() => {
                  const pct = (mib: number) => ((mib - 512) / (16384 - 512)) * 100;
                  const same = memoryMib === minMemoryMib;
                  return (
                    <>
                      {!same && <div className="range-base" style={{ right: `${100 - pct(minMemoryMib)}%` }} aria-hidden />}
                      <div
                        className="range-fill"
                        style={{
                          left: same ? 0 : `${pct(minMemoryMib)}%`,
                          right: `${100 - pct(memoryMib)}%`,
                        }}
                        aria-hidden
                      />
                      <span className="range-knob low" style={{ left: `calc(${pct(minMemoryMib) / 100} * (100% - var(--knob)))` }} aria-hidden />
                      <span className="range-knob high" style={{ left: `calc(${pct(memoryMib) / 100} * (100% - var(--knob)))` }} aria-hidden />
                    </>
                  );
                })()}
                <input
                  className={`memory-slider ${memoryHandlesSwapped ? "high" : "low"} ${!memoryHandlesSwapped && lowOnTop ? "on-top" : ""}`}
                  type="range"
                  min="512"
                  max="16384"
                  step="512"
                  aria-label={memoryHandlesSwapped ? tr("Max memory") : tr("Initial memory")}
                  value={memoryHandlesSwapped ? memoryMib : minMemoryMib}
                  onChange={(event) => {
                    const next = Number(event.target.value);
                    if (memoryHandlesSwapped) {
                      if (next < minMemoryMib) {
                        setMemoryMib(minMemoryMib);
                        setMinMemoryMib(next);
                        setMemoryHandlesSwapped(false);
                        setLowOnTop(true);
                      } else {
                        setMemoryMib(next);
                      }
                    } else if (next > memoryMib) {
                      setMinMemoryMib(memoryMib);
                      setMemoryMib(next);
                      setMemoryHandlesSwapped(true);
                      setLowOnTop(false);
                    } else {
                      setMinMemoryMib(next);
                    }
                  }}
                />
                <input
                  className={`memory-slider ${memoryHandlesSwapped ? "low" : "high"} ${memoryHandlesSwapped && lowOnTop ? "on-top" : ""}`}
                  type="range"
                  min="512"
                  max="16384"
                  step="512"
                  aria-label={memoryHandlesSwapped ? tr("Initial memory") : tr("Max memory")}
                  value={memoryHandlesSwapped ? minMemoryMib : memoryMib}
                  onChange={(event) => {
                    const next = Number(event.target.value);
                    if (memoryHandlesSwapped) {
                      if (next > memoryMib) {
                        setMinMemoryMib(memoryMib);
                        setMemoryMib(next);
                        setMemoryHandlesSwapped(false);
                        setLowOnTop(false);
                      } else {
                        setMinMemoryMib(next);
                      }
                    } else if (next < minMemoryMib) {
                      setMemoryMib(minMemoryMib);
                      setMinMemoryMib(next);
                      setMemoryHandlesSwapped(true);
                      setLowOnTop(true);
                    } else {
                      setMemoryMib(next);
                    }
                  }}
                />
              </div>
              <div className="slider-scale">
                <span>0.5 GB</span>
                <span>16 GB</span>
              </div>
              {systemMemoryMib != null && (
                <p className="memory-note">
                  <Icon name="memory" />
                  {tr("{1} of this computer's {0} ({2}%)", [bytes(systemMemoryMib), bytes(memoryMib), Math.round((memoryMib / systemMemoryMib) * 100)])}
                  {memoryMib / systemMemoryMib > 0.6 && <em>{tr("Allocating too much memory slows down your computer.")}</em>}
                </p>
              )}
            </div>
          </div>

          </>
          )}

          </fieldset>
          {!isNative(selected.core) && (
          <fieldset className="overview-lock" disabled={selected.status !== "stopped"}>
          {(() => {
            const command = `${javaPath && javaPath !== "java" ? javaPath : "java"} -Xms${minMemoryMib}M -Xmx${memoryMib}M ${jvmArgs.trim()} -jar server.jar nogui`.replace(
              /\s+/g,
              " ",
            );
            const savedCustom = (selected.customCommand ?? "").trim();
            const savedScript = (selected.startScript ?? "").trim();
            const scriptExt = selected.remoteHost ? ".sh" : ".bat·.cmd";
            const pickScript = (name: string) => {
              setScriptPick(name);
              if (name && name !== savedScript) {
                void call(() => commands.setStartScript(selected.id, name), tr("Set to start with {0}!", [name]));
              }
            };
            const loadScripts = () => {
              setScriptChoices(null);
              void commands.listStartScripts(selected.id).then(
                (found) => {
                  setScriptChoices(found);
                  const keep = savedScript && found.includes(savedScript) ? savedScript : found[0] ?? "";
                  pickScript(keep);
                },
                (cause) => {
                  setScriptChoices([]);
                  setError(String(cause));
                },
              );
            };
            return (
          <div className="setting-card command-card compact">
            <span className="setting-title">
              {tr("Start command")}
              {!onWeb && (
              <SegmentedControl
                value={scriptMode ? "script" : customMode ? "custom" : "auto"}
                options={[
                  { value: "auto", label: tr("Auto") },
                  { value: "custom", label: tr("Custom") },
                  { value: "script", label: tr("Startup file") },
                ]}
                onChange={(next) => {
                  if (next === "script") {
                    setScriptMode(true);
                    setCustomMode(false);
                    loadScripts();
                    return;
                  }
                  setScriptMode(false);
                  if (next === "custom") {
                    setCustomMode(true);
                    if (!customCommand.trim()) setCustomCommand(command);
                    return;
                  }
                  setCustomMode(false);
                  if (savedCustom) {
                    void call(() => commands.setCustomCommand(selected.id, ""), tr("Switched back to the automatic command."));
                  }
                  if (savedScript) {
                    void call(() => commands.setStartScript(selected.id, ""), tr("Switched back to the automatic command."));
                  }
                }}
              />
              )}
              {onWeb && savedCustom && <span className="chip subtle">{tr("Custom command — editable in the app only")}</span>}
              {onWeb && savedScript && <span className="chip subtle">{tr("Startup file {0} — editable in the app only", [savedScript])}</span>}
              {!customMode && !scriptMode && (
                <button
                  className="button secondary small"
                  data-tip={tr("Fills the input with Aikar's G1GC flags.")}
                  onClick={() => void applyAikarPreset()}
                >
                  {tr("Aikar's flags")}</button>
              )}
            </span>
            {scriptMode ? (
              <>
                <p>
                  {selected.remoteHost
                    ? tr("Runs the .sh file in the server folder using bash, with its java and memory settings.")
                    : tr("Runs the .bat/.cmd file in the server folder as written, using its java and memory settings. The app handles a trailing pause when the server stops.")}
                </p>
                <div className="overview-actions script-pick">
                  {scriptChoices === null ? (
                    <span className="muted">{tr("Searching…")}</span>
                  ) : scriptChoices.length === 0 ? (
                    <span className="muted">{tr("No {0} file found directly inside the server folder.", [scriptExt])}</span>
                  ) : (
                    <Dropdown value={scriptPick} options={scriptChoices.map((one) => ({ value: one, label: one }))} onChange={(value: string) => pickScript(value)} />
                  )}
                  <button className="icon-button" aria-label={tr("Scan again")} data-tip={tr("Scan again")} onClick={loadScripts}>
                    <Icon name="refresh" />
                  </button>
                </div>
                <small className="muted">{tr("The web panel and Claude can start it only if the file hasn't changed since you last started it in the app (for safety).")}</small>
              </>
            ) : customMode ? (
              <>
                <p>{tr("Runs this command as entered. The first word must be java; the rest runs without validation. You can only change it in the app.")}</p>
                <textarea
                  className="command-custom"
                  value={customCommand}
                  onChange={(event) => setCustomCommand(event.target.value)}
                  spellCheck={false}
                  rows={3}
                />
                <div className="overview-actions">
                  <button
                    className="button ghost small"
                    onClick={() => setCustomCommand(command)}
                  >
                    {tr("Use automatic command")}
                  </button>
                  <button
                    className="button primary small"
                    disabled={customCommand.trim() === savedCustom || !customCommand.trim()}
                    onClick={() =>
                      void call(
                        () => commands.setCustomCommand(selected.id, customCommand.trim()),
                        tr("Start command saved!"),
                      )
                    }
                  >
                    {tr("Save")}
                  </button>
                  {savedCustom && customCommand.trim() === savedCustom && (
                    <span>{tr("Saved")}</span>
                  )}
                </div>
              </>
            ) : (
              <>
            <label className="jvm-field">
              <span>{tr("JVM options")}</span>
              <textarea
                value={jvmArgs}
                onChange={(event) => setJvmArgs(event.target.value)}
                placeholder={tr("Leave empty to use the default (-XX:+UseG1GC). Example: -XX:+UseG1GC -XX:MaxGCPauseMillis=200")}
                spellCheck={false}
                rows={1}
              />
            </label>
            {(() => {
              return (
                <div className="command-preview one-line" aria-label={tr("Command that will run")} data-tip={command}>
                  <code>{command}</code>
                  <button
                    type="button"
                    className={`icon-button copy-mark ${copiedKey === "command" ? "done" : ""}`}
                    aria-label={tr("Copy command")}
                    data-tip={tr("Copy command")}
                    onClick={() => {
                      void navigator.clipboard.writeText(command);
                      flashCopied("command");
                      setNotice(tr("Command copied!"));
                    }}
                  >
                    <Icon name={copiedKey === "command" ? "check" : "copy"} />
                  </button>
                </div>
              );
            })()}
              </>
            )}
          </div>
            );
          })()}
          </fieldset>
          )}
          <div className="overview-grid overview-one">
          <div className="setting-card">
            <div className="setting-row">
              <div>
                <strong>{tr("Start automatically")}</strong>
                <small>{tr("The server starts automatically when the app opens.")}</small>
              </div>
              <label className="switch">
                <input
                  type="checkbox"
                  checked={!!selected.startWithApp}
                  onChange={(event) => void setStartWithApp(event.target.checked)}
                />
                <span />
              </label>
            </div>
            {!selected.panelHost && (
              <div className="setting-row">
                <div>
                  <strong>{tr("Restart on crash")}</strong>
                  <small>
                    {selected.remoteHost
                      ? tr("The SSH server restarts it by itself, even while the app is closed. Up to 3 times in 10 minutes. Applies from the next start.")
                      : tr("Restarts after an unexpected shutdown, excluding the Stop button or stop command. Up to 3 times in 10 minutes.")}
                  </small>
                </div>
                <label className="switch">
                  <input
                    type="checkbox"
                    checked={!!selected.crashRestart}
                    onChange={(event) => void setCrashRestart(event.target.checked)}
                  />
                  <span />
                </label>
              </div>
            )}
          </div>
          </div>
          </>
          )}
          </div>
        </div>
      )}
      {!toolLoading && tool === "properties" && (() => {
        const locked = false;
        const running = selected.status !== "stopped";
        const changed = new Set(
          properties
            .filter((entry) => {
              const before = savedProperties.find((one) => one.key === entry.key);
              return before ? before.value !== entry.value : true;
            })
            .map((entry) => entry.key),
        );
        const searching = propertyQuery.trim().length > 0;
        const grouped = new Map<Category, Property[]>();
        for (const entry of properties) {
          const meta = propertyMeta(entry.key);
          if (!propertyMatches(meta, propertyQuery)) continue;
          const list = grouped.get(meta.category) ?? [];
          list.push(entry);
          grouped.set(meta.category, list);
        }
        for (const list of grouped.values())
          list.sort((x, y) => propertyOrder(x.key) - propertyOrder(y.key) || x.key.localeCompare(y.key));
        const update = (key: string, value: string) =>
          setProperties(properties.map((item) => (item.key === key ? { ...item, value } : item)));
        const iconCard = (
          <div className="setting-card props-icon">
            <span className="setting-title">{tr("Server icon")}</span>
            <p>
              {tr("The image shown in the Minecraft server list. Any image is resized to 64×64.")}</p>
            <div className="icon-row">
              {serverIcons[selected.id] ? (
                <img
                  className="server-icon"
                  src={serverIcons[selected.id]}
                  alt=""
                />
              ) : (
                <span className="server-icon empty">
                  <CoreIcon core={selected.core} />
                </span>
              )}
              <button
                className="button secondary small"
                onClick={() => void pickServerIcon()}
              >
                {tr("Choose image")}</button>
              {serverIcons[selected.id] && (
                <button
                  className="button ghost small"
                  onClick={() => void clearServerIcon()}
                >
                  {tr("Delete")}</button>
              )}
            </div>
          </div>
        );
        return (
        <div className="tool-content props-page">
          <div className="wb-head">
            <div className="wb-head-title">
              <h1>{tr("Server settings")}</h1>
              <code>{isNative(selected.core) ? "pumpkin.toml" : "server.properties"}</code>
            </div>
            <span className="live-spacer" />
          <label className="search props-search wb-search-field">
            <Icon name="search" />
            <input
              value={propertyQuery}
              onChange={(event) => setPropertyQuery(event.target.value)}
              placeholder={tr("Find a setting — label, key or description")}
            />
            {searching && (
              <button
                type="button"
                className="icon-button"
                aria-label={tr("Clear search")}
                onClick={() => setPropertyQuery("")}
              >
                <Icon name="close" />
              </button>
            )}
          </label>
          <button className="wb-icon props-compare" aria-label={tr("Compare with another server")} data-tip={tr("Compare with another server")} onClick={() => setCompareOpen(true)}>
            <Icon name="sliders" />
          </button>
          </div>
          {running && (
            <Banner tone="info">
              {tr("The server is running. Saved settings apply on the next start.")}
            </Banner>
          )}
          {properties.length === 0 && (
            <p className="muted tool-empty">
              {isNative(selected.core)
                ? tr("No pumpkin.toml yet. Start the server once to create it.")
                : tr("No server.properties yet. Start the server once to create it.")}</p>
          )}
          {properties.length > 0 && searching && grouped.size === 0 && (
            <p className="muted tool-empty">{tr("No matching settings.")}</p>
          )}
          {(() => {
            const tabs = CATEGORIES.filter((category) => (grouped.get(category) ?? []).length > 0 || (category === "General" && !searching && properties.length > 0));
            const tab = tabs.includes(propsTab) ? propsTab : tabs[0];
            const shown = searching ? tabs : tabs.filter((category) => category === tab);
            const segment = (entry: Property) =>
              entry.kind === "choice" && entry.options.length <= 4 && entry.options.join("").length <= 40;
            return (
              <>
              {properties.length > 0 && !searching && (
                <div className="wb-chips" role="tablist" aria-label={tr("Settings categories")}>
                  {tabs.map((category) => (
                    <button
                      type="button"
                      role="tab"
                      key={category}
                      aria-selected={category === tab}
                      className={`wb-chip ${category === tab ? "on" : ""}`}
                      onClick={() => setPropsTab(category)}
                    >
                      {tr(category)}
                      {[...changed].some((key) => propertyMeta(key).category === category) && <i className="wb-chip-dot" aria-label={tr("Changed")} />}
                    </button>
                  ))}
                </div>
              )}
          <fieldset className="props-lock" disabled={locked}>
            {shown.map((category) => {
              const rows = grouped.get(category) ?? [];
              const showIcon = category === "General" && !searching && properties.length > 0;
              if (!rows.length && !showIcon) return null;
              return (
                <section className="props-section wb-props" id={`props-${category}`} key={category}>
                  {searching && <h2 className="wb-sec">{tr(category)} <b>{rows.length}</b></h2>}
                  <div className="property-list">
                    {showIcon && iconCard}
                    {rows.map((entry) => {
                      const meta = propertyMeta(entry.key);
                      const step = (by: number) => {
                        let next = (Number(entry.value) || 0) + by;
                        if (entry.min != null) next = Math.max(entry.min, next);
                        if (entry.max != null) next = Math.min(entry.max, next);
                        update(entry.key, String(next));
                      };
                      return (
                        <div
                          className={`property-row ${changed.has(entry.key) ? "changed" : ""}`}
                          key={entry.key}
                        >
                          <div className="property-label">
                            <strong>
                              {meta.label}
                              {!meta.known && (
                                <span className="chip subtle" data-tip={tr("This setting isn't recognized, so its original name is shown.")}>
                                  {tr("Unnamed")}
                                </span>
                              )}
                              {changed.has(entry.key) && <span className="chip changed-chip">{tr("Changed")}</span>}
                            </strong>
                            {meta.description && <small>{meta.description}</small>}
                            {meta.known && <code>{entry.key}</code>}
                          </div>
                          {entry.kind === "toggle" ? (
                            <label className="switch property-switch">
                              <input
                                type="checkbox"
                                checked={entry.value === "true"}
                                onChange={(event) =>
                                  update(entry.key, event.target.checked ? "true" : "false")
                                }
                              />
                              <span />
                            </label>
                          ) : segment(entry) ? (
                            <div className="wb-seg" role="radiogroup" aria-label={meta.label}>
                              {entry.options.map((option) => (
                                <button
                                  type="button"
                                  role="radio"
                                  key={option}
                                  aria-checked={entry.value === option}
                                  className={entry.value === option ? "on" : ""}
                                  onClick={() => update(entry.key, option)}
                                >
                                  {option}
                                </button>
                              ))}
                            </div>
                          ) : entry.kind === "choice" ? (
                            <Dropdown
                              value={entry.value}
                              onChange={(value) => update(entry.key, value)}
                              options={entry.options.map((option) => ({
                                value: option,
                                label: option,
                              }))}
                            />
                          ) : entry.kind === "number" ? (
                            <div className="wb-stepper">
                              <button type="button" aria-label={tr("Decrease")} onClick={() => step(-1)}>−</button>
                              <input
                                type="number"
                                value={entry.value}
                                aria-label={meta.label}
                                min={entry.min ?? undefined}
                                max={entry.max ?? undefined}
                                onChange={(event) => update(entry.key, event.target.value)}
                              />
                              <button type="button" aria-label={tr("Increase")} onClick={() => step(1)}>+</button>
                            </div>
                          ) : (
                            <input
                              value={entry.value}
                              onChange={(event) => update(entry.key, event.target.value)}
                            />
                          )}
                        </div>
                      );
                    })}
                  </div>
                </section>
              );
            })}
          </fieldset>
              </>
            );
          })()}
          {changed.size > 0 && !locked && (
            <div className="save-bar">
              <span>{tr("{0} changed", [changed.size])}</span>
              <button className="icon-button" aria-label={tr("Roll back")} data-tip={tr("Roll back")} onClick={() => setProperties(savedProperties)}>
                <Icon name="undo" />
              </button>
              <button className="button primary" onClick={() => void saveProperties()}>
                {tr("Save")}
              </button>
            </div>
          )}
          {automationHistory.length > 0 && (
            <div className="automation-history">
              <span>{tr("Recent app activity")}</span>
              {automationHistory.slice(0, 5).map((entry, index) => (
                <p key={`${entry.createdAt}-${index}`}>
                  <strong>{trBack(entry.action)}</strong> {trBack(entry.detail)}
                  <time>{date(entry.createdAt)}</time>
                </p>
              ))}
            </div>
          )}
        </div>
        );
      })()}
      {!toolLoading && tool === "files" && (() => {
        const locked = false;
        const loaded = browseEntries.length >= browseTotal;
        const query = fileQuery.trim().toLowerCase();
        const rows = [...browseEntries].filter((entry) => !query || entry.name.toLowerCase().includes(query)).sort((a, b) => {
          if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
          const sign = fileSort.desc ? -1 : 1;
          if (fileSort.by === "size") {
            const left = a.isDir ? a.itemCount : a.sizeBytes;
            const right = b.isDir ? b.itemCount : b.sizeBytes;
            return sign * (left - right);
          }
          if (fileSort.by === "date") return sign * (a.modifiedAt - b.modifiedAt);
          return sign * a.name.localeCompare(b.name);
        });
        const sortBy = (by: "name" | "size" | "date") =>
          setFileSort((now) => ({ by, desc: now.by === by ? !now.desc : false }));
        const head = (by: "name" | "size" | "date", label: string) => (
          <button
            type="button"
            className={`file-sort ${fileSort.by === by ? "on" : ""} ${fileSort.by === by && fileSort.desc ? "desc" : ""}`}
            disabled={!loaded}
            data-tip={
              loaded
                ? tr("Sort by this column.")
                : tr("Not all items are loaded yet. Select Show more at the end of the list.")
            }
            onClick={() => sortBy(by)}
          >
            {label}
            {fileSort.by === by && <Icon name="chevron" />}
          </button>
        );
        fileDropRef.current = { browsePath, activeFile, locked };
        const canDrop = !activeFile && !locked;
        return (
        <div
          className={`tool-content file-page ${dropping ? "dropping" : ""}`}
          onDragOver={(event) => {
            if (!onWeb || !canDrop) return;
            event.preventDefault();
            setDropping(true);
          }}
          onDragLeave={(event) => {
            if (!onWeb) return;
            if (event.currentTarget.contains(event.relatedTarget as Node | null)) return;
            setDropping(false);
          }}
          onDrop={(event) => {
            if (!onWeb || !canDrop) return;
            event.preventDefault();
            setDropping(false);
            void addDroppedWebFiles([...event.dataTransfer.files]);
          }}
        >
          {dropping && canDrop && (
            <div className="drop-veil">
              <Icon name="import" />
              <strong>{tr("Drop here to add to this folder")}</strong>
              <small>{browsePath ? `/${browsePath}` : selected.name}</small>
            </div>
          )}
          {!activeFile && (
          <>
          <div className="wb-head">
            <h1>{tr("Files")}</h1>
            <span className="live-spacer" />
            <label className="search file-search wb-search-field">
              <Icon name="search" />
              <input
                value={fileQuery}
                onChange={(event) => setFileQuery(event.target.value)}
                placeholder={tr("Find in this folder")}
              />
            </label>
            <button
              className="wb-icon"
              aria-label={tr("Trash")}
              data-tip={tr("Trash — restore files or delete them permanently")}
              onClick={() => void openTrash()}
            >
              <Icon name="trash" />
            </button>
            <button className="wb-icon" aria-label={tr("Refresh")} data-tip={tr("Refresh")} onClick={() => void browseTo(browsePath)}>
              <Icon name="refresh" />
            </button>
            <DropdownMenu
              trigger={
                <button className="button primary wb-pill" disabled={locked}>
                  <Icon name="plus" />
                  {tr("Add")}
                </button>
              }
              items={[
                { label: tr("New file"), icon: "file", onSelect: () => void newFileHere() },
                { label: tr("New folder"), icon: "folder", onSelect: () => void newFolderHere() },
                { label: tr("Upload files"), icon: "import", onSelect: () => void addFilesHere() },
                ...(onWeb
                  ? []
                  : [{ label: tr("Upload folder"), icon: "folder", onSelect: () => void addFolderHere() }]),
                ...(onWeb || selected.remoteHost || selected.panelHost
                  ? []
                  : [{
                      label: tr("Open in Explorer"),
                      icon: "archive",
                      onSelect: () => {
                        void commands.openServerFolder(selected.id, browsePath || null).catch((cause) => setError(String(cause)));
                      },
                    }]),
              ]}
            />
          </div>
          <nav className="wb-crumbs" aria-label={tr("Path")}>
            <button className={browsePath ? "" : "on"} onClick={() => void goFolder("")}>
              <Icon name="home" />
              {selected.name}
            </button>
            {browsePath
              .split("/")
              .filter(Boolean)
              .map((piece, at, all) => (
                <Fragment key={`${piece}-${at}`}>
                  <span className="wb-crumb-gap" aria-hidden>›</span>
                  <button
                    className={at === all.length - 1 ? "on" : ""}
                    onClick={() => void goFolder(all.slice(0, at + 1).join("/"))}
                  >
                    {piece}
                  </button>
                </Fragment>
              ))}
            <span className="wb-crumb-count">{tr("count.items", [browseTotal])}</span>
          </nav>
          </>
          )}
          {activeFile ? (
            <div className="ide">
              <aside className="ide-explorer" aria-label={tr("Explorer")}>
                <div className="ide-explorer-head">
                  <span>{tr("Explorer")}</span>
                  <button
                    className="icon-button"
                    aria-label={tr("New file")}
                    data-tip={tr("New file")}
                    disabled={locked}
                    onClick={() => void newFileHere()}
                  >
                    <Icon name="plus" />
                  </button>
                  <button className="icon-button" aria-label={tr("Refresh")} data-tip={tr("Refresh")} onClick={() => void browseTo(browsePath)}>
                    <Icon name="refresh" />
                  </button>
                </div>
                <div className="ide-explorer-path" title={`/${browsePath}`}>
                  <Icon name="folder" />
                  {browsePath ? browsePath.split("/").pop() : selected.name}
                </div>
                <div className="ide-explorer-list">
                  {browsePath && (
                    <button onClick={() => void browseTo(browsePath.split("/").slice(0, -1).join("/"))}>
                      <Icon name="undo" />
                      <span>..</span>
                    </button>
                  )}
                  {[...browseEntries]
                    .sort((left, right) => Number(right.isDir) - Number(left.isDir) || left.name.localeCompare(right.name))
                    .map((entry) => {
                      const openable = entry.isDir || entry.editable || isImage(entry.name);
                      const dirty =
                        (entry.path === activeFile && fileContent !== fileSaved) ||
                        (() => {
                          const kept = tabBuffers.current.get(entry.path);
                          return !!kept && kept.content !== kept.saved;
                        })();
                      return (
                        <button
                          key={entry.path}
                          className={`${entry.path === activeFile ? "on" : ""} ${openable ? "" : "dim"}`}
                          disabled={!openable}
                          title={entry.name}
                          onClick={() => void (entry.isDir ? browseTo(entry.path) : openFile(entry.path))}
                        >
                          <Icon name={entry.isDir ? "folder" : isImage(entry.name) ? "image" : "file"} />
                          <span>{entry.name}</span>
                          {dirty && <i className="ide-dot" aria-label={tr("Unsaved")} />}
                        </button>
                      );
                    })}
                </div>
              </aside>
              <section className="ide-main">
                <div className="ide-tabs" role="tablist">
                  {openTabs.map((path) => {
                    const on = path === activeFile;
                    const kept = on ? { content: fileContent, saved: fileSaved } : tabBuffers.current.get(path);
                    const dirty = !!kept && kept.content !== kept.saved;
                    return (
                      <div
                        key={path}
                        role="tab"
                        aria-selected={on}
                        className={`ide-tab ${on ? "on" : ""} ${dirty ? "dirty" : ""}`}
                        title={path}
                        onClick={() => switchTab(path)}
                        onAuxClick={(event) => {
                          if (event.button === 1) void closeTab(path);
                        }}
                      >
                        <Icon name={isImage(path) ? "image" : "file"} />
                        <span>{path.split("/").pop()}</span>
                        <button
                          className="ide-tab-close"
                          aria-label={tr("Close")}
                          onClick={(event) => {
                            event.stopPropagation();
                            void closeTab(path);
                          }}
                        >
                          <i className="ide-dot" aria-hidden />
                          <Icon name="close" />
                        </button>
                      </div>
                    );
                  })}
                </div>
                <div className="ide-crumbs">
                  <button onClick={() => void goFolder("")}>{selected.name}</button>
                  {activeFile
                    .split("/")
                    .slice(0, -1)
                    .map((piece, at, all) => (
                      <button key={`${piece}-${at}`} onClick={() => void goFolder(all.slice(0, at + 1).join("/"))}>
                        <Icon name="chevron" />
                        {piece}
                      </button>
                    ))}
                  <span>
                    <Icon name="chevron" />
                    {activeFile.split("/").pop()}
                  </span>
                  {locked && <span className="chip subtle">{tr("Read-only")}</span>}
                  <span className="ide-crumbs-gap" />
                  {!fileImage && !locked && fileContent !== fileSaved && (
                    <>
                      <button className="ide-action" onClick={() => setFileContent(fileSaved)}>
                        <Icon name="undo" />
                        {tr("Roll back")}
                      </button>
                      <button className="ide-action strong" onClick={() => void saveFile()} data-tip="Ctrl+S">
                        {tr("Save")}
                      </button>
                    </>
                  )}
                  <button
                    className="ide-action icon"
                    aria-label={tr("Close all and return to list")}
                    data-tip={tr("Close all and return to list")}
                    onClick={() => void closeFile()}
                  >
                    <Icon name="close" />
                  </button>
                </div>
                <div className="ide-body">
                  {fileImage ? (
                    <TextureView serverId={selected.id} path={activeFile} src={fileImage} />
                  ) : (
                    <Suspense fallback={null}>
                    <CodeEditor
                      key={activeFile}
                      path={activeFile}
                      value={fileContent}
                      readOnly={locked}
                      onChange={setFileContent}
                      onSave={() => {
                        if (!locked) void saveFile();
                      }}
                      onCursor={setEditorCursor}
                    />
                    </Suspense>
                  )}
                </div>
                <div className="ide-status">
                  {fileImage ? (
                    <span>{tr("Image preview")}</span>
                  ) : fileContent !== fileSaved && !locked ? (
                    <>
                      <span className="ide-status-dirty">
                        <i className="ide-dot" aria-hidden />
                        {tr("Unsaved changes")}
                      </span>
                    </>
                  ) : (
                    <span>{locked ? tr("Read-only") : tr("Saved")}</span>
                  )}
                  <span className="ide-status-gap" />
                  {!fileImage && (
                    <>
                      <span>
                        {tr("Ln {0}, Col {1}", [editorCursor.line, editorCursor.column])}
                        {editorCursor.selected > 0 && ` (${tr("{0, plural, one {# char} other {# chars}} selected", [editorCursor.selected])})`}
                      </span>
                      <span>{tr("{0, plural, one {# line} other {# lines}}", [editorCursor.lines])}</span>
                      <span>UTF-8</span>
                      <span>{fileContent.includes("\r\n") ? "CRLF" : "LF"}</span>
                      <span>{languageName(activeFile)}</span>
                    </>
                  )}
                </div>
              </section>
            </div>
          ) : (
          <>
            <div className="file-list" ref={fileListBox}>
              <div className="file-head">
                <input
                  type="checkbox"
                  className="file-pick"
                  aria-label={tr("Select all visible")}
                  checked={rows.length > 0 && rows.every((one) => pickedFiles.has(one.path))}
                  onChange={(event) =>
                    setPickedFiles(event.target.checked ? new Set(rows.map((one) => one.path)) : new Set())
                  }
                />
                {head("name", tr("Name"))}
                {head("size", tr("Size"))}
                {head("date", tr("Modified"))}
                <span />
              </div>
              {pickedFiles.size > 0 && (
                <div className="file-float" role="toolbar" aria-label={tr("Selected files")}>
                  {(() => {
                    const chosen = rows.filter((row) => pickedFiles.has(row.path));
                    const bytes = chosen.reduce((sum, row) => sum + (row.isDir ? 0 : row.sizeBytes ?? 0), 0);
                    return chosen.length === 1 ? (
                      <strong className="file-float-one">
                        {chosen[0].name}
                        {!chosen[0].isDir && <span className="muted"> · {fileSize(bytes)}</span>}
                      </strong>
                    ) : (
                      <strong>
                        {tr("{0} selected", [pickedFiles.size])}
                        {bytes > 0 && <span className="muted"> · {fileSize(bytes)}</span>}
                      </strong>
                    );
                  })()}
                  <button className="icon-button" aria-label={tr("Compress")} data-tip={tr("Compress")} onClick={() => void zipEntries([...pickedFiles])}>
                    <Icon name="archive" />
                  </button>
                  <button className="icon-button danger" aria-label={tr("Delete")} data-tip={tr("Delete")} disabled={locked} onClick={() => void trashPicked()}>
                    <Icon name="trash" />
                  </button>
                  <span className="file-float-gap" />
                  <button className="icon-button" aria-label={tr("Deselect")} data-tip={tr("Deselect")} onClick={() => setPickedFiles(new Set())}>
                    <Icon name="close" />
                  </button>
                </div>
              )}
              {rows.length ? (
                <FileRows
                  entries={rows}
                  activeFile={activeFile}
                  scroll={fileListBox}
                  locked={locked}
                  picked={pickedFiles}
                  onPickSet={setPickedFiles}
                  onPick={(entry, on) =>
                    setPickedFiles((now) => {
                      const next = new Set(now);
                      if (on) next.add(entry.path);
                      else next.delete(entry.path);
                      return next;
                    })
                  }
                  onZip={(entry) => void zipEntries([entry.path])}
                  onUnzip={(entry) => void unzipEntry(entry)}
                  onOpen={(entry) =>
                    void (entry.isDir ? browseTo(entry.path) : openFile(entry.path))
                  }
                  onRename={(entry) => void renameEntry(entry)}
                  onDownload={(entry) => void downloadEntry(entry)}
                  onTrash={(entry) => void trashEntry(entry)}
                />
              ) : (
                <p className="muted file-empty">
                  <Icon name="folder" />
                  {tr("This folder is empty.")}
                </p>
              )}
              {browseEntries.length < browseTotal && (
                <button
                  className="file-more"
                  disabled={browseMoreBusy}
                  onClick={() => void browseMore()}
                >
                  <Icon name="arrow" />
                  <span className="file-name">
                    {tr("Show more · {0} left", [browseTotal - browseEntries.length])}
                  </span>
                  <span />
                </button>
              )}
            </div>
          </>
          )}
        </div>
        );
      })()}
      {!toolLoading && tool === "worlds" && (
        <div className="tool-content world-page">
          <div className="wb-head world-head">
            <h1>{tr("Worlds")}</h1>
          </div>
          <div className="world-stage map-page">
            <WorldSide
              worlds={worldTree.map(({ world, sizeMib }) => ({
                key: world.name,
                realm: world.realm || "overworld",
                folder: world.name,
                sizeMib,
                modifiedAt: world.modifiedAt,
                hasLevelData: world.hasLevelData,
              }))}
              realm={mapRealm}
              showSize={!selected.panelHost}
              onRealm={(realm) => (addon?.installed ? void openMap(realm) : setMapRealm(realm))}
              onSettings={(key) => {
                const found = worlds.find((one) => one.name === key);
                if (found) void loadWorldSettings(found);
              }}
              onRefresh={() => void loadTool("worlds")}
              spots={mapPins.map((pin) => ({
                key: pin.key,
                label: pin.label,
                x: Math.round(pin.x),
                z: Math.round(pin.z),
                kind: pin.kind === "spawn" ? "spawn" : pin.kind === "mark" ? "mark" : "player",
                online: pin.online,
                icon: pin.icon,
                id: pin.kind === "mark" ? pin.key.slice("mark-".length) : undefined,
              }))}
              onFocus={(x, z) => setMapFocus((was) => ({ x, z, n: (was?.n ?? 0) + 1 }))}
              onRemoveMark={(id) => {
                const mark = bookmarks.find((one) => one.id === id);
                if (mark) void removeBookmark(mark);
              }}
              onManageMarks={() => {
                setMarkForm((was) => ({ ...was, dimension: mapRealm }));
                setMarksOpen(true);
              }}
              onPregen={has(selected.core, "pregen") ? () => { setPregenArea(null); setPregenOpen(true); } : null}
              onTrim={!selected.panelHost && worlds.length > 0 ? () => setTrimOpen(true) : null}
              onShare={!onWeb && addon?.installed && worlds.length > 0 ? () => setShareOpen(true) : null}
            />
            {addon === null ? (
              <div className="tool-loading">
                <div className="skeleton-row" />
              </div>
            ) : !addon.installed ? (
              <EmptyState
                icon="map"
                title={tr("The map needs the Monolab addon")}
                hint={tr("The addon draws the map inside the server. Install it from the Monitor tab, then restart the server.")}
                action={
                  <button className="button primary" onClick={() => void loadTool("monitor")}>
                    {tr("Open Monitor")}
                  </button>
                }
              />
            ) : !worlds.length ? (
              <EmptyState
                icon="map"
                title={tr("No world folders found.")}
                hint={tr("Start the server once to create a world. For imported servers, the folder containing level.dat must be directly inside the server folder.")}
              />
            ) : mapBusy ? (
              <div className="map-loading">
                <p>{tr("Rendering the map…")}</p>
                <div className="booting-bar">
                  <span />
                </div>
              </div>
            ) : mapStart ? (
              <Suspense fallback={<div className="tool-loading"><div className="skeleton-row" /></div>}>
                <WorldMap
                  key={`${selected.id}/${mapRealm}`}
                  cacheKey={`${selected.id}/${mapRealm}`}
                  start={mapStart}
                  fetchTile={fetchMapTile}
                  changes={fetchMapChanges}
                  pins={mapPins}
                  focus={mapFocus}
                  onPick={(x, z) => void bookmarkSpot(x, z)}
                  onExport={(png) => void saveMapPng(png)}
                  chunks={mapChunks && { ...mapChunks, on: mapChunksOn, toggle: () => setMapChunksOn((was) => !was) }}
                  cave={mapCave === null ? null : { on: mapCave, toggle: () => setMapCave((was) => !was) }}
                  stats={selected.panelHost ? null : fetchChunkStats}
                  onDeleteArea={!selected.panelHost && ["overworld", "nether", "end"].includes(mapRealm) ? (area) => void deleteAreaChunks(area) : null}
                  onPregenArea={
                    has(selected.core, "pregen") && ["overworld", "nether", "end"].includes(mapRealm)
                      ? (area) => {
                          setPregenArea({
                            x: Math.round((area.minX + area.maxX + 1) / 2),
                            z: Math.round((area.minZ + area.maxZ + 1) / 2),
                            rx: Math.ceil((area.maxX - area.minX + 1) / 2),
                            rz: Math.ceil((area.maxZ - area.minZ + 1) / 2),
                          });
                          setPregenOpen(true);
                        }
                      : null
                  }
                />
              </Suspense>
            ) : (
              <EmptyState icon="map" title={tr("This dimension hasn't been generated yet.")} hint={tr("Players need to visit it first, or pre-generate it.")} />
            )}
          </div>
        </div>
      )}
      {shareOpen && selected && (
        <MapShareDialog server={selected} onClose={() => setShareOpen(false)} onNotice={setNotice} onOpen={(url) => void openExternal(url)} />
      )}
      {marksOpen && selected && (
        <div className="modal-backdrop roomy" onClick={() => setMarksOpen(false)}>
          <div className="add-modal marks-dialog" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
            <div className="tool-top">
              <p>
                <strong>{tr("Bookmarks")}</strong> · {selected.name}
              </p>
              <div className="modal-top-actions">
                <button className="close in-flow" aria-label={tr("Close")} onClick={() => setMarksOpen(false)}>
                  <Icon name="close" />
                </button>
              </div>
            </div>
            <MarksPanel
              bookmarks={bookmarks}
              markForm={markForm}
              setMarkForm={setMarkForm}
              onSave={() => void saveBookmark()}
              onRemove={(mark) => void removeBookmark(mark)}
            />
          </div>
        </div>
      )}
      {!toolLoading && tool === "logs" && (
        <div className="tool-content log-page">
          <div className="wb-head">
            <h1>{tr("Logs")}</h1>
            <span className="live-spacer" />
            {logTabs}
          </div>
          {logView === "audit" && (
            <div className="log-view audit">
          <section className="audit-center">
            <div className="audit-heading">
              <div>
                <strong>{tr("Audit log")}</strong>
                <p>{tr("Records starts, stops, settings and file saves, backups and upgrades.")}</p>
              </div>
              <span>{auditEntries.length}{tr("suffix.count")}</span>
            </div>
            {auditEntries.length ? (
              <div className="audit-list">
                {auditEntries.slice(0, 12).map((entry) => (
                  <article key={entry.id}>
                    <b>{entry.category}</b>
                    <div>
                      <strong>{trBack(entry.action)}</strong>
                      <p>{trBack(entry.detail)}</p>
                    </div>
                    <time>{date(entry.createdAt)}</time>
                  </article>
                ))}
              </div>
            ) : (
              <p className="alert-empty">{tr("No audit entries for this server yet.")}</p>
            )}
          </section>
            </div>
          )}
          {logView === "runs" && (
            <div className="log-view">
              {runs === null ? (
                <p className="muted tool-empty">{tr("Loading…")}</p>
              ) : runs.length === 0 ? (
                <EmptyState
                  icon="clock"
                  title={tr("No run history yet")}
                  hint={tr("Starts, stops and crashes are recorded here from now on. SSH and panel servers are recorded while the app is open.")}
                />
              ) : (
                <div className="run-list">
                  {runs.map((one, index) => {
                    const look = {
                      start: { label: tr("Started"), tone: "ok" },
                      stop: { label: tr("Stopped"), tone: "muted" },
                      crash: { label: tr("Crashed"), tone: "danger" },
                      restart: { label: tr("Restarted"), tone: "warn" },
                    }[one.kind] ?? { label: one.kind, tone: "muted" };
                    return (
                      <article key={`${one.at}-${index}`} className={`run-${look.tone}`}>
                        <span className="run-mark" />
                        <strong>{look.label}</strong>
                        <p>{trBack(one.detail)}</p>
                        <time data-tip={date(one.at)}>{relativeTime(one.at)}</time>
                      </article>
                    );
                  })}
                </div>
              )}
            </div>
          )}
          {logView === "crash" && (
            <CrashPanel
              reports={crashReports}
              openReport={openCrash}
              onToggle={setOpenCrash}
              onReload={() => void commands.listCrashReports(selected.id).then(setCrashReports).catch((cause) => setError(String(cause)))}
            />
          )}
          {logView === "read" && (
            <div className="log-view read overview-console log-console">
          <div className="live-head">
            <SegmentedControl
              className="console-levels"
              value={logLevelView}
              onChange={setLogLevelView}
              options={[
                { value: "all", label: tr("All") },
                { value: "error", label: tr("Errors") },
                { value: "warn", label: tr("Warnings") },
                { value: "info", label: tr("Info") },
              ]}
            />
            <span className="live-spacer" />
            <Dropdown
              value={readLogFile}
              onChange={(file) => {
                setReadLogFile(file);
                void refreshParsedLogs("All", file);
              }}
              options={
                logFiles.length
                  ? logFiles.map((file) => ({
                      value: file.fileName,
                      label: file.fileName,
                      hint: file.compressed ? tr("Compression") : undefined,
                    }))
                  : [{ value: "latest.log", label: "latest.log" }]
              }
              placeholder={tr("Log file")}
            />
            <label className="search console-search">
              <Icon name="search" />
              <input value={logQuery} onChange={(event) => setLogQuery(event.target.value)} placeholder={tr("Search logs")} />
            </label>
            <button
              className="button secondary small"
              disabled={sharingLog}
              data-tip={tr("Uploads {0} to mclo.gs and creates a link. IP addresses, home paths and secrets are masked.", [readLogFile])}
              onClick={() => void shareLog(readLogFile)}
            >
              {sharingLog ? tr("Uploading…") : tr("Share")}
            </button>
          </div>
          {crashReports.length > 0 && (
            <Banner
              tone="danger"
              action={
                <button className="button secondary small" onClick={() => { setLogView("crash"); setOpenCrash(crashReports[0].fileName); }}>
                  {tr("Details")}
                </button>
              }
            >
              <strong>{tr("Last crash")}</strong> · {relativeTime(crashReports[0].createdAt)} — {crashReports[0].description}
              {crashReports[0].exception && <code className="crash-inline">{crashReports[0].exception}</code>}
            </Banner>
          )}
          <LogLines
            logs={logLines}
            filter={logQuery}
            level={logLevelView}
            emptyText={tr("No logs to show.")}
          />
            </div>
          )}
          {logView === "events" && (
            <div className="log-view events">
          {dailyStats.length > 0 && (
            <div className="daily-stats">
              <span>{tr("Daily summary")}</span>
              <div>
                {dailyStats.map((day) => (
                  <button
                    type="button"
                    key={day.fileName}
                    className={activeLogFile === day.fileName ? "chosen" : ""}
                    onClick={() => void loadTimeline(day.fileName, timelineKind)}
                  >
                    <strong>{day.date}</strong>
                    <p>
                      {tr("Joins")}{" "}{day.joins} {tr("· chat")}{" "}{day.chats} {tr("· deaths")}{" "}{day.deaths} {tr("· errors")}{" "}{day.errors}
                    </p>
                    <small>
                      {tr("Players")}{" "}{day.uniquePlayers}{tr("suffix.people")}{day.busiestPlayer && tr(" · most active {0}", [day.busiestPlayer])}
                    </small>
                  </button>
                ))}
              </div>
            </div>
          )}
          <div className="timeline-panel">
            <div className="timeline-head">
              <Dropdown
                value={activeLogFile}
                onChange={(file) => void loadTimeline(file, timelineKind)}
                options={logFiles.map((file) => ({
                  value: file.fileName,
                  label: file.fileName,
                  hint: file.compressed ? tr("Compression") : undefined,
                }))}
                placeholder={tr("Log file")}
              />
              <Dropdown
                value={timelineKind}
                onChange={(kind) => void loadTimeline(activeLogFile, kind)}
                options={["All", "Join", "Leave", "Chat", "Command", "Death", "Error"].map(
                  (kind) => ({ value: kind, label: tr(kind) }),
                )}
              />
              <span>{timeline.length}{tr("suffix.items")}</span>
            </div>
            {timeline.length ? (
              <div className="timeline-list">
                {timeline.map((event, index) => (
                  <div key={`${index}-${event.time}-${event.text}`}>
                    <time>{event.time || "--:--:--"}</time>
                    <b className={`event-${event.kind}`}>{trBack(event.kind)}</b>
                    {event.player && <span>{event.player}</span>}
                    <p>{event.text}</p>
                  </div>
                ))}
              </div>
            ) : (
              <p className="muted tool-empty">
                {tr("No matching events in this log.")}</p>
            )}
          </div>
            </div>
          )}
        </div>
      )}
      {!toolLoading && tool === "backups" && (
        <div className="tool-content backup-page">
          <div className="wb-head">
            <h1>{tr("Backups")}</h1>
            <span className="live-spacer" />
            <button
              className="button primary wb-pill"
              data-tip={tr("Compresses and stores the entire server folder.")}
              disabled={backupBusy}
              onClick={() => void makeBackup()}
            >
              <Icon name="archive" />
              {backupBusy ? tr("Backing up…") : tr("Back up now")}
            </button>
          </div>
          <div className="monitor-grid backup-tiles">
            <div>
              <span>{tr("Backups")}</span>
              <strong>{tr("{0, plural, one {# backup} other {# backups}}", [backupList.length])}</strong>
            </div>
            <div>
              <span>{tr("Total size")}</span>
              <strong>{backupList.reduce((sum, one) => sum + (one.sizeMib ?? 0), 0).toFixed(1)} MB</strong>
            </div>
            <div>
              <span>{tr("Last")}</span>
              <strong data-tip={backupList.length ? date(backupList[0].createdAt) : undefined}>
                {backupList.length ? relativeTime(backupList[0].createdAt) : "—"}
              </strong>
            </div>
            <div data-tip={tr("A backup is created automatically before upgrades. Backups are deleted after the configured retention period.")}>
              <span>{tr("Automatic backups")}</span>
              <strong className="backup-auto">
                <Icon name="info" />
                {tr("Before upgrades")}
              </strong>
            </div>
          </div>
          {backupList.length ? (
            <div className="backup-list">
              {backupList.map((one) => (
                <article className="backup-row" key={one.fileName}>
                  <span className="backup-mark">
                    <Icon name="archive" />
                  </span>
                  <div className="backup-body">
                    <strong data-tip={date(one.createdAt)}>{relativeTime(one.createdAt)}</strong>
                    <small>
                      {trBack(one.note)}
                      {one.worldsOnly ? "" : tr(" · full")} · {one.fileName}
                    </small>
                  </div>
                  <code className="backup-size">
                    {(one.sizeMib ?? 0).toFixed(1)} MB
                  </code>
                  <button
                    className="button secondary small"
                    disabled={backupBusy || selected.status !== "stopped"}
                    data-tip={
                      selected.status !== "stopped"
                        ? tr("Stop the server first.")
                        : tr("Overwrites the server folder with this snapshot.")
                    }
                    onClick={() => void restoreBackup(one)}
                  >
                    {tr("Restore")}</button>
                  <button
                    className="icon-button danger"
                    aria-label={tr("Delete {0}", [one.fileName])}
                    data-tip={tr("Delete")}
                    onClick={() => void deleteBackup(one)}
                  >
                    <Icon name="trash" />
                  </button>
                </article>
              ))}
            </div>
          ) : (
            <p className="muted tool-empty">
              {tr("No backups yet. Click \"Back up now\" at the top right.")}</p>
          )}
        </div>
      )}
      {!toolLoading && PANEL_VIEWS.includes(tool) && selected?.panelHost && (
        <Suspense fallback={null}>
        <PanelServerTools
          key={selected.id}
          view={tool as PanelView}
          instanceId={selected.id}
          panelName={panelHosts[selected.panelHost] ?? tr("Panel")}
          hostId={selected.panelHost}
          daemon={selected.panelDaemon}
          ident={selected.panelServer}
          name={selected.name}
          onError={setError}
          onNotice={setNotice}
          onRenamed={() => void commands.syncPanelServers().then(() => refresh()).catch(() => {})}
          confirm={confirmAction}
          ask={askText}
        />
        </Suspense>
      )}
      {!toolLoading && tool === "claude" && !onWeb && (
        <Suspense fallback={null}>
        <AgentPage key={selected.id} server={{ id: selected.id, name: selected.name }} onError={setError} />
        </Suspense>
      )}
      {!toolLoading && tool === "monitor" && (() => {
        const mc = kindOf(selected.core).family === "minecraft";
        return (
        <div className="tool-content monitor-page">
          {addon?.supported && (!addon.installed || addon.needsUpdate) && (
            <div className="addon-strip">
              <span className="addon-strip-name">
                Monolab {addon.kind}
              </span>
              {addon.installed ? (
                <>
                  <span className="addon-on">
                    <Icon name="check" />v{addon.installedVersion}
                  </span>
                  {addon.needsUpdate && addon.running && <span className="muted">{tr("Stop the server to update the add-on.")}</span>}
                  {addon.needsUpdate && (
                    <button
                      className="button primary small"
                      disabled={addon.running}
                      onClick={() => void installAddon()}
                    >
                      v{addon.latestVersion}{tr(": Update")}</button>
                  )}
                  <button
                    className="icon-button"
                    disabled={addon.running}
                    data-tip={tr("Remove")}
                    onClick={() => void removeAddon()}
                  >
                    <Icon name="trash" />
                  </button>
                </>
              ) : (
                <>
                  <span className="muted">
                    {addon.running
                      ? tr("Stop the server first.")
                      : tr("Not installed. Install it to see TPS, players and map positions.")}
                  </span>
                  <button
                    className="button primary small"
                    disabled={addon.running}
                    onClick={() => void installAddon()}
                  >
                    <Icon name="plus" />
                    {tr("Install")}</button>
                </>
              )}
            </div>
          )}
          <div className="wb-head">
            <h1>{tr("Monitor")}</h1>
            <span className="live-spacer" />
            {chartView === "past" && perfHistory.length > 0 && perfDays.length > 1 && (
              <Dropdown
                value={perfDay}
                onChange={(day) => void loadPerfHistory(day)}
                options={perfDays.slice(0, 30).map((day) => ({ value: day, label: day }))}
              />
            )}
            {perfHistory.length > 0 && (
              <div className="wb-seg" role="tablist" aria-label={tr("Period")}>
                {([["now", tr("Now")], ["past", tr("History")]] as const).map(([value, label]) => (
                  <button
                    key={value}
                    type="button"
                    role="tab"
                    aria-selected={chartView === value}
                    className={chartView === value ? "on" : ""}
                    onClick={() => setChartView(value)}
                  >
                    {label}
                  </button>
                ))}
              </div>
            )}
            {!selected.panelHost && (
              <button className="wb-icon" aria-label={tr("Disk usage")} data-tip={tr("Disk usage")} onClick={() => setDiskOpen(true)}>
                <Icon name="disk" />
              </button>
            )}
          </div>
          <div className="tool-top wb-note">
            <p>
              {!mc
                ? selected.status === "running"
                  ? tr("Updates every second")
                  : tr("Updates every second while the server is running.")
                : live && selected.status === "running"
                ? selected.remoteHost
                  ? tr("TPS and entities every second, CPU and memory every 3 seconds (SSH)")
                  : tr("Updates every second")
                : selected.status !== "running"
                  ? tr("Updates every second while the server is running. Past records are below.")
                  : addon?.installed
                    ? tr("Data appears once the add-on sends it.")
                    : tr("TPS and player data require Monolab {0}.", [addon?.kind ?? tr("plugin")])}
            </p>
          </div>
          <div className="monitor-grid">
            <div>
              <span>CPU</span>
              <strong>
                {orDash(selected.status === "running" && resource ? resource.cpuPercent ?? 0 : null, (v) => `${v.toFixed(1)}%`)}
              </strong>
            </div>
            <div>
              <span>{tr("Memory use")}</span>
              <strong>
                {orDash(selected.status !== "running" ? null : live?.memoryMib != null ? live.memoryMib : resource ? resource.memoryMib : null, (v) => (v < 1024 ? fileSize(v * 1048576) : bytes(v)))}
              </strong>
            </div>
            {live?.chunks != null && (
              <div>
                <span>{tr("Loaded chunks")}</span>
                <strong>{orDash(live.chunks, (v) => Math.round(v).toLocaleString())}</strong>
              </div>
            )}
            {live?.entities != null && (
              <div>
                <span>{tr("Entities")}</span>
                <strong>{orDash(live.entities, (v) => Math.round(v).toLocaleString())}</strong>
              </div>
            )}
            {mc && (
              <div className={(() => {
                const value = live?.tps ?? tps?.tps ?? null;
                return value == null || selected.status !== "running" ? "" : value < 15 ? "danger" : value < 18 ? "warn" : "";
              })()}>
                <span>TPS</span>
                <strong>
                  {orDash(live?.tps ?? tps?.tps ?? null, (v) => v.toFixed(2))}
                </strong>
              </div>
            )}
            {mc && (
              <div>
                <span>MSPT</span>
                <strong>
                  {orDash(live?.mspt ?? tps?.mspt ?? null, (v) => `${v.toFixed(1)} ms`)}
                </strong>
              </div>
            )}
          </div>
          {mc && selected.status === "running" && (
            <div className="monitor-insight">
              {lagCardView()}
              <Card
                className="entity-card"
                title={tr("Entities")}
                aside={
                  live?.entities != null ? (
                    <span className="muted">{orDash(live.entities, (v) => tr("{0} in total", [Math.round(v).toLocaleString()]))}</span>
                  ) : null
                }
              >
                {!live || live.entityKinds.length === 0 ? (
                  <p className="muted lag-note">
                    {!live
                      ? tr("Data is grouped by type once Monolab {0} starts reporting.", [addon?.kind ?? tr("add-on")])
                      : live.entities
                        ? tr("Counts by type require add-on 15 or later. Update it above.")
                        : tr("No living entities.")}
                  </p>
                ) : (
                  <>
                    <ul className="entity-kinds">
                      {live.entityKinds.map((one) => (
                        <li key={one.type} title={one.type}>
                          <span
                            className="entity-bar"
                            style={{ width: `${Math.max(2, (one.count / Math.max(1, live.entityKinds[0].count)) * 100)}%` }}
                            aria-hidden="true"
                          />
                          <code>{one.type}</code>
                          <strong>{one.count.toLocaleString()}</strong>
                        </li>
                      ))}
                    </ul>
                    {live.worlds.length > 1 && (
                      <p className="muted entity-worlds">
                        {live.worlds.map((world) => `${world.name} ${world.entities.toLocaleString()}`).join(" · ")}
                      </p>
                    )}
                    {live.hotspots.length > 0 && (
                      <div className="entity-hotspots">
                        <h4>{tr("Crowded chunks")}</h4>
                        <ul>
                          {live.hotspots.map((spot) => {
                            const x = spot.x * 16 + 8;
                            const z = spot.z * 16 + 8;
                            const key = `spot-${spot.world ?? ""}-${spot.x}-${spot.z}`;
                            return (
                              <li key={key}>
                                <span className="hotspot-where">
                                  {spot.world ?? "world"} <code>{x}, {z}</code>
                                </span>
                                <span className="hotspot-what">
                                  <strong>{spot.count.toLocaleString()}</strong>
                                  {spot.top && <code>{spot.top}</code>}
                                </span>
                                <button
                                  type="button"
                                  className={`icon-button copy-mark ${copiedKey === key ? "done" : ""}`}
                                  aria-label={tr("Copy teleport command")}
                                  data-tip={tr("Copy teleport command")}
                                  onClick={() => void runLagAction({ kind: "copy", command: tpCommand(spot.world, x, z) }, key)}
                                >
                                  <Icon name={copiedKey === key ? "check" : "copy"} />
                                </button>
                              </li>
                            );
                          })}
                        </ul>
                      </div>
                    )}
                  </>
                )}
              </Card>
            </div>
          )}
          {chartView === "past" && perfHistory.length > 0 ? null : selected.status !== "running" && history.length === 0 ? (
            <EmptyState
              icon="cpu"
              title={mc ? tr("Start the server to see CPU, memory and TPS graphs.") : tr("Start the server to see CPU and memory graphs.")}
              hint={!mc ? undefined : perfHistory.length > 0 ? tr("Past records from the add-on are below.") : tr("With the add-on installed, performance is recorded every minute, so you can look back at times the app was closed.")}
            />
          ) : (
          <div className="chart-stack">
            <Sparkline
              label="CPU"
              unit="%"
              max={100}
              format={(value) => value.toFixed(0)}
              samples={recentHistory.map((sample) => ({
                at: sample.at,
                value: sample.cpuPercent,
              }))}
            />
            <Sparkline
              label={tr("Memory")}
              unit=" GB"
              max={mc ? selected.maxMemoryMib / 1024 : undefined}
              format={(value) => value.toFixed(1)}
              samples={recentHistory.map((sample) => ({
                at: sample.at,
                value: sample.memoryMib == null ? null : sample.memoryMib / 1024,
              }))}
            />
            {mc && (
            <Sparkline
              label="TPS"
              unit=""
              max={20}
              threshold={18}
              format={(value) => value.toFixed(1)}
              samples={recentHistory.map((sample) => ({
                at: sample.at,
                value: sample.tps,
              }))}
            />
            )}
          </div>
          )}
          {chartView === "past" && perfHistory.length > 0 && (
            <div className="perf-past">
              <div className="chart-stack">
                <Sparkline
                  label="TPS"
                  unit=""
                  max={20}
                  threshold={18}
                  format={(value) => value.toFixed(1)}
                  samples={perfHistory.map((one) => ({
                    at: Math.round(one.at / 1000),
                    value: one.tps ?? 0,
                  }))}
                />
                <Sparkline
                  label={tr("Memory")}
                  unit=" GB"
                  max={selected.maxMemoryMib / 1024}
                  format={(value) => value.toFixed(1)}
                  samples={perfHistory.map((one) => ({
                    at: Math.round(one.at / 1000),
                    value: (one.memoryMib ?? 0) / 1024,
                  }))}
                />
                <Sparkline
                  label={tr("Players")}
                  unit={tr("suffix.people")}
                  format={(value) => value.toFixed(0)}
                  samples={perfHistory.map((one) => ({
                    at: Math.round(one.at / 1000),
                    value: one.players ?? 0,
                  }))}
                />
              </div>
            </div>
          )}
        </div>
        );
      })()}
      {!toolLoading && tool === "reach" && (() => {
        const ports = isCustom(selected.core) ? { protocol: reachProtocol, count: 1 } : portsOf(selected.core);
        const span = ports.count > 1 ? `${selected.port}-${selected.port + ports.count - 1}` : String(selected.port);
        const proto = ports.protocol === "both" ? "TCP+UDP" : ports.protocol.toUpperCase();
        const tunnel = playit?.tunnels.find((one) => one.localPort === selected.port);
        const rows = [
          { key: "local", icon: "monitor", label: tr("This PC"), who: tr("This PC only"), value: `localhost:${selected.port}` },
          { key: "lan", icon: "home", label: tr("Same Wi-Fi"), who: tr("Friends on your Wi-Fi"), value: lanIp ? `${lanIp}:${selected.port}` : "" },
          { key: "playit", icon: "cloud", label: "playit.gg", who: tr("Friends anywhere"), value: tunnel?.address ?? "" },
        ];
        return (
        <div className="tool-content reach-page">
          <div className="wb-head">
            <h1>{tr("Network")}</h1>
            <span className="live-spacer" />
            <span className="reach-port" data-tip={tr("Addresses friends can use to join.")}>{proto} {span}</span>
          </div>
          <div className="reach-tiles">
            {rows.map((row) => (
              <div className={`reach-tile${row.value ? "" : " off"}`} key={row.key}>
                <div className="reach-tile-head">
                  <span className="reach-icon"><Icon name={row.icon} /></span>
                  <div>
                    <strong>{row.label}</strong>
                    <small>{row.who}</small>
                  </div>
                </div>
                {row.value ? (
                  <button
                    className={`reach-address${copiedKey === "reach-" + row.key ? " done" : ""}`}
                    data-tip={tr("Click to copy")}
                    onClick={() => {
                      flashCopied("reach-" + row.key);
                      void navigator.clipboard.writeText(row.value);
                      setNotice(tr("Address copied!"));
                    }}
                  >
                    <code>{row.value}</code>
                    <Icon name={copiedKey === "reach-" + row.key ? "check" : "copy"} />
                  </button>
                ) : (
                  <span className="reach-address empty">{tr("Off")}</span>
                )}
              </div>
            ))}
          </div>
          {playitCard()}
          <div className="setting-card">
            <span className="setting-title">{tr("Port forwarding")}</span>
            <p className="reach-note">
              {tr("In your router, forward {0} {1} to {2}. Friends then join with your public IP.", [proto, span, lanIp || tr("this PC")])}
            </p>
            <small className="muted">{tr("Your home IP is visible to friends.")}</small>
          </div>
        </div>
        );
      })()}
      {trimOpen && selected && (
        <TrimDialog
          server={selected}
          realms={(() => {
            const found = ["overworld", "nether", "end"].filter((one) => worlds.some((world) => (world.realm || "overworld") === one));
            const list = found.length ? found : ["overworld"];
            return list.includes(mapRealm) ? [mapRealm, ...list.filter((one) => one !== mapRealm)] : list;
          })()}
          confirm={confirmAction}
          onClose={() => setTrimOpen(false)}
          onDone={(message) => {
            setNotice(message);
            if (mapOpen) void openMap(mapRealm);
            else void loadTool("worlds");
          }}
        />
      )}
      {pregenOpen && selected && (
        <PregenDialog
          server={selected}
          realm={Math.max(0, ["overworld", "nether", "end"].indexOf(mapRealm))}
          area={pregenArea}
          onClose={() => setPregenOpen(false)}
          onNotice={setNotice}
        />
      )}
      {diskOpen && selected && (
        <DiskDialog
          server={selected}
          confirm={confirmAction}
          onClose={() => setDiskOpen(false)}
          onDone={(message) => {
            setNotice(message);
            if (tool === "files") void browseTo(browsePath);
          }}
        />
      )}
      {compareOpen && selected && (
        <CompareDialog
          current={selected}
          others={instances.filter((one) => one.id !== selected.id)}
          onClose={() => setCompareOpen(false)}
        />
      )}
      {versionOpen && selected && (
        <div className="modal-backdrop roomy" onClick={() => setVersionOpen(false)}>
          <div
            className="add-modal version-dialog"
            role="dialog"
            aria-modal="true"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="tool-top">
              <p>
                <strong>{tr("Change core version")}</strong> · {selected.core} {selected.version}
                {selected.coreBuild ? ` · ${tr("Build")} ${selected.coreBuild}` : ""}
              </p>
              <div className="modal-top-actions">
                <button className="close in-flow" data-tip={tr("Close")} aria-label={tr("Close")} onClick={() => setVersionOpen(false)}>
                  <Icon name="close" />
                </button>
              </div>
            </div>
            {selected.status !== "stopped" && (
              <Banner tone="warn">{tr("The server is running. Stop it first to change the version.")}</Banner>
            )}
            {buildNews?.behind && (
              <Banner tone="info">
                {tr("A new build is available")} · {tr("Now")} <code>{buildNews.current || "?"}</code> → <code>{buildNews.newest}</code>
                {buildNews.channel ? ` (${buildNews.channel})` : ""}
              </Banner>
            )}
            <div className="version-pick">
              <div className="version-field">
                <span className="field-label">
                  {tr("Minecraft version")}
                  <label className="wb-toggle small">
                    <span>{tr("Snapshots")}</span>
                    <span className="switch">
                      <input
                        type="checkbox"
                        checked={upgradeSnapshots}
                        onChange={(event) => {
                          const on = event.target.checked;
                          setUpgradeSnapshots(on);
                          void commands.listCoreVersions(selected.core, on).then(setUpgradeVersions).catch(() => {});
                        }}
                      />
                      <span />
                    </span>
                  </label>
                </span>
                <Dropdown
                  value={upgradeVersion}
                  onChange={(version) => {
                    setUpgradeVersion(version);
                    void loadUpgradeBuilds(version);
                  }}
                  search={tr("Search versions")}
                  placeholder={upgradeVersions.length ? tr("Choose a version") : tr("Loading versions…")}
                  disabled={!upgradeVersions.length}
                  options={upgradeVersions.map((version) => ({
                    value: version,
                    label: version === selected.version ? tr("{0} (now)", [version]) : version,
                  }))}
                />
              </div>
              {!isProxy(selected.core) && (upgradeBuildsBusy || upgradeBuilds.length > 0) && (
                <div className="version-field">
                  <span className="field-label">
                    {selected.core === "Fabric" ? tr("Loader version") : tr("Core build")}
                  </span>
                  <Dropdown
                    className="mono"
                    search={tr("Search builds")}
                    disabled={upgradeBuildsBusy}
                    value={upgradeBuild}
                    placeholder={upgradeBuildsBusy ? tr("Loading builds…") : tr("Choose a build")}
                    options={upgradeBuilds.map((build) => ({
                      value: build.id,
                      label:
                        build.id === selected.coreBuild && upgradeVersion === selected.version
                          ? tr("{0} (now)", [build.label])
                          : build.label,
                      hint: build.channel || undefined,
                    }))}
                    onChange={setUpgradeBuild}
                  />
                </div>
              )}
            </div>
            <p className="muted">
              {tr("The current core file is kept for rollback. Mods and plugins may not support the new version. Back up & change backs up the entire server folder first.")}
            </p>
            <div className="modal-actions">
              <button
                className="button secondary"
                disabled={upgradeBusy || upgradeUnchanged || selected.status !== "stopped" || upgradeBuildsBusy}
                onClick={() => void upgradeInstance(true)}
              >
                {tr("Back up & change")}
              </button>
              <button
                className="button primary"
                disabled={upgradeBusy || upgradeUnchanged || selected.status !== "stopped" || upgradeBuildsBusy}
                onClick={() => void upgradeInstance(false)}
              >
                {upgradeBusy ? tr("Changing…") : tr("Change")}
              </button>
            </div>
            {upgradeHistory.length > 0 && (
              <p className="upgrade-rollback">
                <button
                  className="button ghost small"
                  disabled={upgradeBusy || selected.status !== "stopped"}
                  data-tip={`${upgradeHistory[0].toVersion} → ${upgradeHistory[0].fromVersion} · ${date(upgradeHistory[0].createdAt)}`}
                  onClick={() => void rollbackUpgrade()}
                >
                  <Icon name="undo" />
                  {tr("Restore previous core")}
                </button>
              </p>
            )}
          </div>
        </div>
      )}
      {!toolLoading && tool === "worlds" && worldDraft && worldSettings && (
        <div
          className="modal-backdrop roomy"
          onClick={() => {
            setWorldSettings(null);
            setWorldDraft(null);
          }}
        >
          <div
            className="add-modal world-editor"
            role="dialog"
            aria-modal="true"
            onClick={(event) => event.stopPropagation()}
          >
          <div className="tool-top">
            <p>
              <strong>{worldSettings.levelName}</strong> {tr("· seed")}{" "}
              <code>{worldSettings.seed}</code>
              {worldSettings.lastPlayed > 0 &&
                tr(" · last played {0}", [date(Math.round(worldSettings.lastPlayed / 1000))])}
            </p>
            <div className="modal-top-actions">
              <button
                className="button primary"
                disabled={selected.status !== "stopped"}
                onClick={() => void saveWorldSettings()}
              >
                {tr("Save")}</button>
              <button
                className="close in-flow"
                data-tip={tr("Close")}
                onClick={() => {
                  setWorldSettings(null);
                  setWorldDraft(null);
                }}
              >
                <Icon name="close" />
              </button>
            </div>
          </div>
          {selected.status !== "stopped" && (
            <p className="empty-stop-help">
              {tr("World settings can only be changed while the server is fully stopped.")}</p>
          )}
          <div className="world-fields">
            <label className="field">
              <span>{tr("Difficulty")}</span>
              <Dropdown
                value={String(worldDraft.difficulty)}
                onChange={(value) =>
                  setWorldDraft({ ...worldDraft, difficulty: Number(value) })
                }
                options={[
                  { value: "0", label: tr("Peaceful") },
                  { value: "1", label: tr("Easy") },
                  { value: "2", label: tr("Normal") },
                  { value: "3", label: tr("Hard") },
                ]}
              />
            </label>
            <label className="field">
              <span>{tr("Spawn point")}</span>
              <div className="spawn-fields">
                {[0, 1, 2].map((axis) => (
                  <input
                    key={axis}
                    type="number"
                    value={worldDraft.spawn[axis]}
                    onChange={(event) => {
                      const spawn = [...worldDraft.spawn] as [
                        number,
                        number,
                        number,
                      ];
                      spawn[axis] = Number(event.target.value);
                      setWorldDraft({ ...worldDraft, spawn });
                    }}
                  />
                ))}
              </div>
            </label>
            <label className="check">
              <input
                type="checkbox"
                checked={worldDraft.hardcore}
                onChange={(event) =>
                  setWorldDraft({
                    ...worldDraft,
                    hardcore: event.target.checked,
                  })
                }
              />
              {tr("Hardcore")}</label>
          </div>
          <div className="rule-search">
            <label className="search">
              <Icon name="search" />
              <input
                value={ruleQuery}
                placeholder={tr("Search game rules (e.g. mob, keepInventory)")}
                onChange={(event) => setRuleQuery(event.target.value)}
              />
            </label>
            <span className="muted">
              {worldDraft.gameRules.length}{tr("suffix.of")}{" "}
              {
                worldDraft.gameRules.filter((rule) =>
                  ruleMatches(rule.name, ruleQuery),
                ).length
              }
              {tr("suffix.count")}</span>
          </div>
          <div className="rule-grid">
            {worldDraft.gameRules.map((rule, index) => {
              if (!ruleMatches(rule.name, ruleQuery)) return null;
              const known = gameRuleLabels[ruleKey(rule.name)];
              const yesNo = rule.value === "true" || rule.value === "false";
              return (
                <div className="rule-item" key={rule.name}>
                  <div className="rule-what">
                    <strong>{gameRuleName(rule.name)}</strong>
                    {gameRuleName(rule.name) !== rule.name && <code>{rule.name}</code>}
                    {known?.hint && <small>{known.hint}</small>}
                  </div>
                  {yesNo ? (
                    <label className="switch">
                      <input
                        type="checkbox"
                        checked={rule.value === "true"}
                        onChange={(event) => {
                          const rules = [...worldDraft.gameRules];
                          rules[index] = {
                            ...rule,
                            value: event.target.checked ? "true" : "false",
                          };
                          setWorldDraft({ ...worldDraft, gameRules: rules });
                        }}
                      />
                      <span />
                    </label>
                  ) : (
                    <input
                      className="rule-number"
                      type="number"
                      value={rule.value}
                      onChange={(event) => {
                        const rules = [...worldDraft.gameRules];
                        rules[index] = { ...rule, value: event.target.value };
                        setWorldDraft({ ...worldDraft, gameRules: rules });
                      }}
                    />
                  )}
                </div>
              );
            })}
          </div>
          </div>
        </div>
      )}
      {!toolLoading && tool === "players" && (
        <div className="tool-content player-page">
          <div className="wb-head">
            <h1>{tr("Players")}</h1>
            <span className="live-spacer" />
            <label className="search wb-search-field">
              <Icon name="search" />
              <input
                value={playerQuery}
                onChange={(event) => setPlayerQuery(event.target.value)}
                placeholder={tr("Find a name")}
              />
            </label>
            <button className="wb-icon" aria-label={tr("Refresh")} data-tip={tr("Refresh")} onClick={() => void loadTool("players")}>
              <Icon name="refresh" />
            </button>
          </div>
          <div className="player-add">
            <input
              value={playerName}
              onChange={(event) => setPlayerName(event.target.value)}
              placeholder={tr("Player name")}
              data-tip={
                selected.status === "running"
                  ? tr("The server is running, so changes apply through a console command. Only the name is needed.")
                  : tr("The server is stopped, so the list file is edited directly. The UUID is found automatically using the name.")
              }
              onKeyDown={(event) => {
                if (event.key === "Enter" && playerName.trim()) void addPlayerHere();
              }}
            />
            <SegmentedControl
              value={playerAddKind}
              options={[
                { value: "whitelist", label: tr("Whitelist") },
                { value: "operator", label: "OP" },
                { value: "ban", label: tr("Ban") },
              ]}
              onChange={setPlayerAddKind}
            />
            <button
              className={`button wb-pill ${playerAddKind === "ban" ? "danger" : "primary"}`}
              disabled={!playerName.trim()}
              onClick={() => void addPlayerHere()}
            >
              {playerAddKind === "ban" ? tr("Ban") : <><Icon name="plus" />{tr("Add")}</>}
            </button>
          </div>
          {playerSnapshot && (
            <div
              className="modal-backdrop roomy"
              onClick={() => setPlayerSnapshot(null)}
            >
            <div
              className="add-modal player-detail"
              role="dialog"
              aria-modal="true"
              onClick={(event) => event.stopPropagation()}
            >
              <div className="player-detail-head">
                {heads[playerSnapshot.uuid] && (
                  <img
                    className="player-head large"
                    src={heads[playerSnapshot.uuid]}
                    alt=""
                  />
                )}
                <div>
                  <strong>{playerSnapshot.live ? tr("Live player data") : tr("Saved player data")}</strong>
                  <small>
                    {playerSnapshot.dimension || tr("No dimension info")}
                    {playerSnapshot.live
                      ? <> · {tr("Refreshes every 5 seconds")}</>
                      : playerSnapshot.modifiedAt > 0 && <> · {date(playerSnapshot.modifiedAt)} {tr("Threshold")}</>}</small>
                </div>
                <button
                  className="close in-flow"
                  data-tip={tr("Close")}
                  onClick={() => setPlayerSnapshot(null)}
                >
                  <Icon name="close" />
                </button>
              </div>
              <div className="monitor-grid">
                <div>
                  <span>{tr("Coordinates")}</span>
                  <strong>
                    {playerSnapshot.position
                      .map((value) => Math.round(value ?? 0))
                      .join(", ")}
                  </strong>
                </div>
                <div>
                  <span>{tr("Health")}</span>
                  <strong>{(playerSnapshot.health ?? 0).toFixed(1)}</strong>
                </div>
                <div>
                  <span>{tr("Hunger")}</span>
                  <strong>{playerSnapshot.food}</strong>
                </div>
                <div>
                  <span>{tr("XP level")}</span>
                  <strong>{playerSnapshot.experienceLevel}</strong>
                </div>
                <div>
                  <span>{tr("Game mode")}</span>
                  <strong className="text-value">
                    {playerSnapshot.gameMode}
                  </strong>
                </div>
              </div>
              {(playerSnapshot.inventory.length > 0 || playerSnapshot.enderItems.length > 0) && (() => {
                const shown = invTab === "ender" ? playerSnapshot.enderItems : playerSnapshot.inventory;
                const editable = selected.status === "stopped" && !selected.panelHost && !playerSnapshot.live;
                const bySlot = new Map(shown.map((one) => [one.slot, one]));
                const row = (from: number, count: number) =>
                  Array.from({ length: count }, (_, at) => from + at);
                const slot = (at: number, mark?: string) => {
                  const item = bySlot.get(at);
                  if (!item) {
                    return (
                      <div className="inv-slot empty" key={at}>
                        {mark && <span className="inv-mark">{mark}</span>}
                      </div>
                    );
                  }
                  const name =
                    itemNames[item.id] ?? item.id.replace("minecraft:", "");
                  const icon = itemIcons[item.id];
                  const lines = [
                    `${item.name || name}${item.count > 1 ? ` ×${item.count}` : ""}`,
                    ...(item.name ? [`(${name})`] : []),
                    ...item.enchantments.map((one) =>
                      [enchantNames[one.id] ?? one.id.replace("minecraft:", ""), enchantLevel(one.id, one.level)]
                        .filter(Boolean)
                        .join(" "),
                    ),
                  ];
                  const most = item.maxDamage || maxDurability(item.id);
                  if (editable) lines.push(tr("Click to change the count"));
                  if (item.damage > 0) {
                    lines.push(
                      most
                        ? tr("Durability: {0} / {1}", [Math.max(most - item.damage, 0), most])
                        : tr("Damage: {0}", [item.damage]),
                    );
                  }
                  return (
                    <div
                      className={`inv-slot${item.enchantments.length ? " enchanted" : ""}${editable ? " editable" : ""}`}
                      key={at}
                      onClick={editable ? () => void editSavedItem(item, invTab === "ender") : undefined}
                      data-tip={lines.join("\n")}
                    >
                      {icon ? (
                        <img src={icon} alt="" />
                      ) : (
                        <span className="inv-unknown">?</span>
                      )}
                      {item.count > 1 && <b>{item.count}</b>}
                    </div>
                  );
                };
                const unknown = shown
                  .filter((one) => !itemIcons[one.id])
                  .map(
                    (one) => itemNames[one.id] ?? one.id.replace("minecraft:", ""),
                  );
                return (
                  <>
                  <SegmentedControl
                    className="page-tabs"
                    value={invTab}
                    onChange={setInvTab}
                    options={[
                      { value: "bag", label: tr("Inventory") },
                      { value: "ender", label: `${tr("Ender chest")} ${playerSnapshot.enderItems.length}` },
                    ]}
                  />
                  {invTab === "ender" ? (
                  <div className="inv-screen">
                    <div className="inv-bag">
                      {row(0, 27).map((at) => slot(at))}
                    </div>
                    {unknown.length > 0 && (
                      <p className="inv-unknown-list">
                        <b>?</b> {[...new Set(unknown)].join(" · ")}
                      </p>
                    )}
                  </div>
                  ) : (
                  <div className="inv-screen">
                    <div className="inv-gear">
                      <div className="inv-row four">
                        {[
                          [103, tr("Head")],
                          [102, tr("Chest")],
                          [101, tr("Legs")],
                          [100, tr("Feet")],
                        ].map(([at, mark]) => slot(at as number, mark as string))}
                      </div>
                      <span className="inv-label">{tr("Armor")}</span>
                      <div className="inv-row one">{slot(-106, tr("Offhand"))}</div>
                      <span className="inv-label">{tr("Offhand")}</span>
                    </div>
                    <div className="inv-bag">
                      {row(9, 27).map((at) => slot(at))}
                    </div>
                    <div className="inv-hotbar">
                      {row(0, 9).map((at) => slot(at))}
                    </div>
                    {unknown.length > 0 && (
                      <p className="inv-unknown-list">
                        <b>?</b> {[...new Set(unknown)].join(" · ")}
                      </p>
                    )}
                  </div>
                  )}
                  </>
                );
              })()}
            </div>
            </div>
          )}
          {(() => {
            const onlineNames = new Set((live?.players ?? []).map((one) => one.name));
            const tabs = [
              ...(selected.status === "running" ? [{ id: "online" as const, label: tr("Online"), count: players.filter((one) => onlineNames.has(one.name)).length }] : []),
              { id: "all" as const, label: tr("All"), count: players.length },
              { id: "whitelist" as const, label: tr("Whitelist"), count: players.filter((one) => one.whitelisted).length },
              { id: "operator" as const, label: "OP", count: players.filter((one) => one.operator).length },
              { id: "ban" as const, label: tr("Ban"), count: players.filter((one) => one.banned).length },
              { id: "recent" as const, label: tr("Recent"), count: playerActivity.length },
            ];
            const needle = playerQuery.trim().toLowerCase();
            const picked =
              playerTab === "online"
                ? players.filter((one) => onlineNames.has(one.name))
                : playerTab === "whitelist"
                ? players.filter((one) => one.whitelisted)
                : playerTab === "operator"
                  ? players.filter((one) => one.operator)
                  : playerTab === "ban"
                    ? players.filter((one) => one.banned)
                    : players;
            const shown = needle ? picked.filter((one) => one.name.toLowerCase().includes(needle)) : picked;
            const emptyTitle =
              needle
                ? tr("No matching players.")
                : playerTab === "online"
                ? tr("No one is online.")
                : playerTab === "whitelist"
                ? tr("The whitelist is empty")
                : playerTab === "operator"
                  ? tr("No operators")
                  : playerTab === "ban"
                    ? tr("No banned players")
                    : tr("No player data yet");
            return (
              <>
              <div className="wb-chips" role="tablist" aria-label={tr("Player lists")}>
                {tabs.map((one) => (
                  <button
                    key={one.id}
                    type="button"
                    role="tab"
                    aria-selected={playerTab === one.id}
                    className={`wb-chip ${playerTab === one.id ? "on" : ""}`}
                    onClick={() => setPlayerTab(one.id)}
                  >
                    {one.label}
                    <b>{one.count}</b>
                  </button>
                ))}
              </div>
              {playerTab === "recent" ? (
                playerActivity.length ? (
                  <div className="player-rows wb-list">
                    {playerActivity.slice(0, 30).map((entry) => (
                      <div className="player-row" key={entry.name}>
                        <span className="player-cell">
                          <span className="player-face off"><span className="player-head empty" /></span>
                          {entry.name}
                        </span>
                        <span className="player-meta">
                          {tr("{0, plural, one {# session} other {# sessions}}", [entry.sessions])}
                          {" · "}
                          <span data-tip={entry.lastSeenAt ? date(entry.lastSeenAt) : undefined}>
                            {entry.lastSeenAt ? relativeTime(entry.lastSeenAt) : tr("No records")}
                          </span>
                        </span>
                      </div>
                    ))}
                  </div>
                ) : (
                  <EmptyState
                    icon="list"
                    title={tr("No recent connections found in the logs.")}
                    hint={tr("Players recorded in latest.log appear here.")}
                  />
                )
              ) : shown.length ? (
                <div className="player-grid">
                  {shown.map((player) => {
                    const here = (live?.players ?? []).some((one) => one.name === player.name);
                    return (
                      <div
                        className={`player-card ${here ? "here" : ""}`}
                        key={player.uuid}
                        role="button"
                        tabIndex={0}
                        onClick={() => void loadPlayerSnapshot(player)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") void loadPlayerSnapshot(player);
                        }}
                      >
                        <span className="player-cell">
                          <span
                            className={`player-face ${here ? "on" : "off"}`}
                            data-tip={here ? tr("Online now") : tr("Offline")}
                          >
                            {heads[player.uuid] ? (
                              <img className="player-head" src={heads[player.uuid]} alt="" />
                            ) : (
                              <span className="player-head empty" />
                            )}
                            <i />
                          </span>
                          <span className="player-card-text">
                            <strong>{player.name}</strong>
                            <small className={player.banned ? "danger" : ""}>
                              {[
                                player.banned ? tr("Banned") : null,
                                player.operator ? "OP" : null,
                                player.whitelisted ? tr("Whitelist") : null,
                              ].filter(Boolean).join(" · ") || (here ? tr("Online now") : tr("Offline"))}
                            </small>
                          </span>
                        </span>
                        <span className="player-quick" onClick={(event) => event.stopPropagation()} onKeyDown={(event) => event.stopPropagation()}>
                          <button
                            type="button"
                            className={`player-quick-button ${player.whitelisted ? "on" : ""}`}
                            onClick={() => void updatePlayer(player, "whitelist")}
                            data-tip={player.whitelisted ? tr("Remove from whitelist") : tr("Add to whitelist")}
                          >
                            <Icon name="list" />
                            {tr("Whitelist")}
                          </button>
                          <button
                            type="button"
                            className={`player-quick-button ${player.operator ? "on" : ""}`}
                            onClick={() => void updatePlayer(player, "operator")}
                            data-tip={player.operator ? tr("Remove operator") : tr("Make operator")}
                          >
                            <Icon name="shield" />
                            OP
                          </button>
                          {selected.status === "running" && here && (
                            <button
                              type="button"
                              className="player-quick-button"
                              onClick={() => void applyPlayerAccess(player.name, player.uuid, "kick")}
                              data-tip={tr("Kick from server")}
                            >
                              <Icon name="exit" />
                              {tr("Kick")}
                            </button>
                          )}
                          <button
                            type="button"
                            className={`player-quick-button danger ${player.banned ? "on" : ""}`}
                            onClick={() => void banPlayer(player)}
                            data-tip={player.banned ? tr("Unban") : tr("Ban from server")}
                          >
                            <Icon name="ban" />
                            {player.banned ? tr("Unban") : tr("Ban")}
                          </button>
                        </span>
                        <span className="player-card-menu" onClick={(event) => event.stopPropagation()} onKeyDown={(event) => event.stopPropagation()}>
                        <DropdownMenu
                          trigger={
                            <button className="icon-button" aria-label={tr("{0} menu", [player.name])}>
                              <Icon name="dots" />
                            </button>
                          }
                          items={[
                            {
                              label: tr("Copy UUID"),
                              icon: "copy",
                              onSelect: () => {
                                void navigator.clipboard.writeText(player.uuid);
                                setNotice(tr("UUID copied."));
                              },
                            },
                            { label: tr("Details"), icon: "info", onSelect: () => void loadPlayerSnapshot(player) },
                          ]}
                        />
                        </span>
                      </div>
                    );
                  })}
                </div>
              ) : (
                <EmptyState
                  icon="list"
                  title={emptyTitle}
                  hint={
                    playerTab === "all"
                      ? tr("Players appear here when they join. You can also enter a name above to add them in advance.")
                      : tr("Enter a name above to add a player.")
                  }
                />
              )}
              </>
            );
          })()}
        </div>
      )}
      {!toolLoading && tool === "addons" && (
        <div className={`tool-content addon-page ${dropping ? "dropping" : ""}`}>
          {dropping && (
            <div className="drop-veil">
              <Icon name="import" />
              <strong>
                {selected.status === "stopped"
                  ? tr("Drop here to install")
                  : tr("Drop here to queue for the next restart")}
              </strong>
              <small>
                {tr(".jar files only · existing plugins are replaced with the new version")}</small>
            </div>
          )}
          {addonReport && addonReport.conflicts.length > 0 && (
            <div className="conflict-box">
              <p>
                <Icon name="settings" />
                {tr("Duplicates")}{" "}{addonReport.conflicts.length}{tr("suffix.count")}</p>
              {addonReport.conflicts.map((conflict) => (
                <div key={conflict.message}>
                  <strong>{conflict.message}</strong>
                  <span>{conflict.files.join(", ")}</span>
                </div>
              ))}
            </div>
          )}
          <div className="wb-head addon-bar">
            <h1>{addonKind(selected.core) === "mod" ? tr("Mods") : tr("Plugins")}</h1>
            <span className="live-spacer" />
            {!addonKind(selected.core) && tabAddons.length === 0 && (
              <p className="muted addon-note">
                {isNative(selected.core)
                  ? tr("Uses its own plugin system. Add files manually.")
                  : tr("This core doesn't support automatic installation. Add the file manually.")}
              </p>
            )}
            {tabAddons.length > 0 && (
            <label className="search addon-search wb-search-field">
              <Icon name="search" />
              <input
                value={addonQuery}
                placeholder={tr("Search {0, plural, one {# item} other {# items}}…", [tabAddons.length])}
                onChange={(event) => setAddonQuery(event.target.value)}
              />
            </label>
            )}
            <button
              className="button ghost small"
              disabled={selected.status !== "stopped"}
              data-tip={
                selected.status !== "stopped"
                  ? tr("Stop the server first.")
                  : onWeb ? undefined : tr("You can also drag and drop it onto this window.")
              }
              onClick={() => void pickAddonFiles()}
            >
              <Icon name="import" />
              {tr("Add file")}</button>
            {!isNative(selected.core) && (
            <>
            {addonKind(selected.core) !== "plugin" && (
            <button
              className="button ghost small"
              disabled={!addonKind(selected.core)}
              data-tip={tr("Unpack a Modrinth modpack (.mrpack) or bundle the installed mods.")}
              onClick={() => {
                setMrpackEntries([]);
                setModal("mrpack");
              }}
            >
              <Icon name="archive" />
              {tr("Modpack")}</button>
            )}
            <button
              className="button primary wb-pill"
              disabled={!addonKind(selected.core) || addonTab !== addonKind(selected.core)}
              data-tip={addonKind(selected.core) && addonTab !== addonKind(selected.core) ? tr("Install from a tab supported by this server.") : undefined}
              onClick={() => void openBrowse()}
            >
              <Icon name="plus" />
              {addonTab === "plugin"
                ? tr("Add plugin")
                : tr("Add mod")}
            </button>
            </>
            )}
          </div>

          {tabAddons.length > 0 && (
          <div className="addon-filters">
            <Dropdown
              value={addonSort}
              onChange={setAddonSort}
              options={[
                { value: "name", label: "A → Z" },
                { value: "name-desc", label: "Z → A" },
                { value: "kind", label: tr("By type") },
              ]}
            />
            <div className="pick-chips">
              {[
                { id: "all", label: tr("All"), count: tabAddons.length },
                ...Object.entries(addonCounts.kinds).map(([kind, count]) => ({
                  id: kind,
                  label: kind === "plugin" ? tr("Plugins") : tr("Mods"),
                  count,
                })),
                { id: "update", label: tr("Update"), count: addonCounts.update },
                { id: "off", label: tr("Off"), count: addonCounts.off },
                {
                  id: "client",
                  label: tr("Client only"),
                  count: addonCounts.client,
                },
              ]
                .filter(
                  (chip) =>
                    chip.id === "all" ||
                    (chip.count > 0 &&
                      !(
                        Object.keys(addonCounts.kinds).length < 2 &&
                        chip.id in addonCounts.kinds
                      )),
                )
                .map((chip) => (
                  <button
                    key={chip.id}
                    className={`pick-chip ${addonFilter === chip.id ? "on" : ""}`}
                    onClick={() => setAddonFilter(chip.id)}
                  >
                    {chip.label}
                    <em>{chip.count}</em>
                  </button>
                ))}
            </div>
            <span className="addon-gap" />
            {addonCounts.update > 0 && (
              <button
                className="button ghost small"
                data-tip={selected.status !== "stopped" && !selected.panelHost ? tr("Downloads now and applies when the server restarts.") : undefined}
                onClick={openUpdatePlan}
              >
                <Icon name="import" />
                {selected.status === "stopped" || selected.remoteHost ? tr("Update all") : tr("Download and queue")}{" "}
                {addonCounts.update}
              </button>
            )}
            <button
              className="button ghost small"
              disabled={checkingUpdates}
              onClick={() => void loadTool("addons")}
            >
              <Icon name="refresh" />
              {checkingUpdates ? tr("Checking…") : tr("Refresh")}
            </button>
          </div>
          )}

          {waiting.length > 0 && (
            <div className="addon-waiting">
              <div className="addon-waiting-head">
                <Icon name="refresh" />
                <strong>{tr("Applied on restart")}</strong>
                <span className="muted">
                  {waiting.length}{tr(" downloaded and waiting")}</span>
              </div>
              {waiting.map((one) => (
                <div className="addon-waiting-row" key={one.id}>
                  <div>
                    <b>{one.displayName}</b>
                    <small>
                      {one.replaces
                        ? `${one.fromVersion || tr("Current version")} → ${one.toVersion || tr("New version")}`
                        : tr("New")}
                      {" · "}
                      {one.source}
                    </small>
                  </div>
                  <button
                    className="button ghost small"
                    onClick={() => void cancelWaiting(one)}
                  >
                    {tr("Remove")}</button>
                </div>
              ))}
            </div>
          )}

          {pickedAddons.length > 0 && (
            <div className="file-float" role="toolbar" aria-label={tr("Selected items")}>
              <strong>{tr("{0} selected", [pickedAddons.length])}</strong>
              <button className="icon-button" aria-label={tr("Enable")} data-tip={tr("Enable")} onClick={() => void bulkToggleAddons(true)}>
                <Icon name="power" />
              </button>
              <button className="icon-button" aria-label={tr("Disable")} data-tip={tr("Disable")} onClick={() => void bulkToggleAddons(false)}>
                <Icon name="power-off" />
              </button>
              <button className="icon-button danger" aria-label={tr("Delete")} data-tip={tr("Delete")} onClick={() => void bulkDeleteAddons()}>
                <Icon name="trash" />
              </button>
              <span className="file-float-gap" />
              <button className="icon-button" aria-label={tr("Deselect")} data-tip={tr("Deselect")} onClick={() => setPickedAddons([])}>
                <Icon name="close" />
              </button>
            </div>
          )}

          <div className="addon-drop">
          {tabAddons.length ? (
            <div
              className="addon-table"
              onPointerDown={(event) => {
                if (event.button !== 0) return;
                const target = event.target as HTMLElement;
                if (target.closest("button, a, .switch, .addon-ver, .addon-head")) return;
                const index = Number(target.closest<HTMLElement>("[data-addon-index]")?.dataset.addonIndex);
                const names = shownAddons.map((one) => one.fileName);
                if (!Number.isFinite(index) || !names[index]) return;
                addonDrag.current = { index, base: pickedAddons, on: !pickedAddons.includes(names[index]) };
                addonDragged.current = false;
                let scroller: HTMLElement | null = event.currentTarget.parentElement;
                while (scroller && !/(auto|scroll)/.test(getComputedStyle(scroller).overflowY)) {
                  scroller = scroller.parentElement;
                }
                const onMove = (move: PointerEvent) => {
                  const start = addonDrag.current;
                  if (!start) return;
                  const here = Number(
                    document
                      .elementFromPoint(move.clientX, move.clientY)
                      ?.closest<HTMLElement>("[data-addon-index]")?.dataset.addonIndex,
                  );
                  if (!Number.isFinite(here) || (here === start.index && !addonDragged.current)) return;
                  addonDragged.current = true;
                  window.getSelection()?.removeAllRanges();
                  const [from, to] = here < start.index ? [here, start.index] : [start.index, here];
                  const span = names.slice(from, to + 1);
                  setPickedAddons(
                    start.on
                      ? [...new Set([...start.base, ...span])]
                      : start.base.filter((name) => !span.includes(name)),
                  );
                  if (scroller) {
                    const edge = scroller.getBoundingClientRect();
                    if (move.clientY > edge.bottom - 28) scroller.scrollTop += 14;
                    else if (move.clientY < edge.top + 28) scroller.scrollTop -= 14;
                  }
                };
                const onUp = () => {
                  window.removeEventListener("pointermove", onMove);
                  addonDrag.current = null;
                  if (addonDragged.current) window.setTimeout(() => { addonDragged.current = false; }, 0);
                };
                window.addEventListener("pointermove", onMove);
                window.addEventListener("pointerup", onUp, { once: true });
                window.addEventListener("pointercancel", onUp, { once: true });
              }}
              onClickCapture={(event) => {
                if (!addonDragged.current) return;
                event.preventDefault();
                event.stopPropagation();
              }}
            >
              <div className="addon-head">
                <label className="addon-pick" data-tip={tr("Select all visible")}>
                  <input
                    type="checkbox"
                    checked={
                      shownAddons.length > 0 &&
                      shownAddons.every((one) =>
                        pickedAddons.includes(one.fileName),
                      )
                    }
                    onChange={(event) =>
                      setPickedAddons(
                        event.target.checked
                          ? shownAddons.map((one) => one.fileName)
                          : [],
                      )
                    }
                  />
                  <span>
                    <Icon name="check" />
                  </span>
                </label>
                <span className="addon-head-cell">{tr("Project")}</span>
                <span className="addon-head-cell">{tr("Version")}</span>
                <span className="addon-head-cell right">{tr("Tasks")}</span>
              </div>
              {shownAddons.map((one, index) => {
                const card = addonCards[one.fileName];
                const fit = card?.compatibility ?? one.compatibility;
                const clientOnly = isClientOnly(one, card);
                const mismatch = !clientOnly && !!card?.warn;
                const update = addonUpdateBy.get(one.fileName);
                const picked = pickedAddons.includes(one.fileName);
                return (
                  <article
                    className={`addon-row ${one.enabled ? "" : "off"} ${
                      clientOnly ? "client-only" : ""
                    } ${picked ? "picked" : ""} ${card?.projectId ? "opens" : ""}`}
                    key={`${one.kind}-${one.fileName}`}
                    data-addon-index={index}
                    onClick={(event) => {
                      if ((event.target as HTMLElement).closest("button, a, input, label, .addon-ver")) return;
                      openInstalledProject(one);
                    }}
                  >
                    <label className="addon-pick">
                      <input
                        type="checkbox"
                        checked={picked}
                        onChange={() =>
                          setPickedAddons((current) =>
                            current.includes(one.fileName)
                              ? current.filter((name) => name !== one.fileName)
                              : [...current, one.fileName],
                          )
                        }
                      />
                      <span>
                        <Icon name="check" />
                      </span>
                    </label>
                    {one.icon || card?.iconUrl ? (
                      <img
                        className="addon-icon"
                        src={one.icon || card!.iconUrl!}
                        alt=""
                      />
                    ) : (
                      <span className="addon-icon empty">
                        <Icon name="archive" />
                      </span>
                    )}
                    <div className="addon-main">
                      <strong>{card?.title || one.name}</strong>
                      <small className={clientOnly || mismatch ? "warn" : ""}>
                        {one.kind === "plugin" ? tr("Plugins") : tr("Mods")}
                        {fit ? ` · ${trBack(fit)}` : ""}
                      </small>
                    </div>
                    <div
                      className={`addon-ver ${card?.projectId ? "opens" : ""}`}
                      data-tip={card?.projectId ? tr("Change version") : one.fileName.replace(/\.disabled$/, "")}
                      onClick={() => { if (card?.projectId) void openVersionPick(one); }}
                    >
                      <strong>
                        {one.version || "—"}
                        {update && (
                          <button
                            className="addon-new"
                            data-tip={tr("Change version")}
                            onClick={(event) => {
                              event.stopPropagation();
                              void openVersionPick(one);
                            }}
                          >
                            {tr("New version")}{" "}{update.newVersion}
                          </button>
                        )}
                      </strong>
                      <small>{one.fileName.replace(/\.disabled$/, "")}</small>
                    </div>
                    <div className="addon-act">
                      <button
                        type="button"
                        className="icon-button"
                        aria-label={tr("Change version")}
                        data-tip={tr("Change version")}
                        onClick={() => void openVersionPick(one)}
                      >
                        <Icon name="tag" />
                      </button>
                      {!onWeb && (
                        <button
                          type="button"
                          className="icon-button"
                          aria-label={tr("Show in folder")}
                          data-tip={tr("Show in folder")}
                          onClick={() =>
                            void revealItemInDir(
                              `${selected.path}\\${one.kind === "plugin" ? "plugins" : "mods"}\\${one.fileName}`,
                            ).catch(() => undefined)
                          }
                        >
                          <Icon name="folder" />
                        </button>
                      )}
                      <button
                        type="button"
                        className="icon-button danger"
                        aria-label={tr("Delete")}
                        data-tip={tr("Delete")}
                        onClick={() => void deleteAddon(one)}
                      >
                        <Icon name="trash" />
                      </button>
                      <label
                        className="switch"
                        data-tip={one.enabled ? tr("Disable") : tr("Enable")}
                      >
                        <input
                          type="checkbox"
                          checked={one.enabled}
                          onChange={() =>
                            void call(
                              () =>
                                commands.toggleAddon(selected.id, one.fileName, one.kind).then((renamed) => {
                                  setAddons((list) =>
                                    list.map((it) =>
                                      it.fileName === one.fileName
                                        ? { ...it, fileName: renamed, enabled: !it.enabled }
                                        : it,
                                    ),
                                  );
                                  setAddonCards((cards) => {
                                    const { [one.fileName]: moved, ...rest } = cards;
                                    return moved ? { ...rest, [renamed]: moved } : cards;
                                  });
                                }),
                              one.enabled
                                ? tr("Disabled {0}.", [card?.title || one.name])
                                : tr("Enabled {0}.", [card?.title || one.name]),
                            )
                          }
                        />
                        <span />
                      </label>
                    </div>
                  </article>
                );
              })}
              {!shownAddons.length && (
                <p className="muted tool-empty">
                  {tr("Nothing matches your filters.")}</p>
              )}
            </div>
          ) : (
            <EmptyState
              icon="archive"
              title={
                addonTab === "plugin"
                  ? tr("No plugins installed")
                  : tr("No mods installed")
              }
              hint={
                onWeb
                  ? tr("Install from Browse, or upload a file using Add file.")
                  : tr("Install from Browse, or drag and drop a file onto this window.")
              }
              action={
                addonKind(selected.core) === addonTab ? (
                  <button
                    className="button secondary"
                    onClick={() => void openBrowse()}
                  >
                    <Icon name="search" />
                    {addonKind(selected.core) === "plugin" ? tr("Browse plugins") : tr("Browse mods")}
                  </button>
                ) : undefined
              }
            />
          )}
          </div>
          {addonReport?.offline && (
            <p className="muted addon-note">
              {tr("Can't connect to the internet. Only information from the jar files is shown. You can check for updates once you're online.")}</p>
          )}
          {rollbacks.length > 0 && (
            <div className="rollback-list">
              <span>{tr("Previous files")}</span>
              {rollbacks.slice(0, 6).map((entry) => (
                <button
                  key={entry.storedName}
                  onClick={() => void rollbackAddon(entry)}
                >
                  <code>{entry.fileName}</code>
                  <small>{date(entry.createdAt)}</small>
                  <b>{tr("Restore")}</b>
                </button>
              ))}
            </div>
          )}
        </div>
      )}
    </>
  );
  if (webReady === false) {
    return <WebLogin onDone={() => setWebReady(true)} />;
  }
  if (webReady === null) return null;
  return (
    <main className="app-frame">
      <Tooltip />
      <DragScroll />
      <TitleBar
        title="Monolab"
        nav={{
          back: navState.back,
          forward: navState.forward,
          onBack: () => travelTo(-1),
          onForward: () => travelTo(1),
        }}
      />
      <div className={`app-shell wb ${selected && page === "instance" ? "in-server" : ""}`}>
      <header className="wb-top">
        <button
          type="button"
          className="wb-brand"
          aria-label={t("nav.servers")}
          onClick={goHome}
        >
          <img className="brand-img" src={appIcon} alt="" draggable={false} />
          <span>Monolab</span>
        </button>
        <nav className="wb-menu" aria-label={tr("Menu")} ref={menuBox}>
          <span className="wb-menu-mark" aria-hidden style={menuMark ?? { opacity: 0 }} />
          <button
            type="button"
            className={modal === "none" ? "on" : ""}
            onClick={goHome}
          >
            <Icon name="server" />
            {t("nav.servers")}
          </button>
          {!onWeb && (
            <button
              type="button"
              className={modal === "web" ? "on" : ""}
              onClick={() => {
                setModal("web");
                setAccountDraft(null);
                void refreshWebPanel();
                void refreshWebAccounts();
              }}
            >
              <Icon name="cloud" />
              {tr("Web panel")}
              {webPanel.on && <b className="wb-dot ok" aria-label={tr("On")} />}
            </button>
          )}
          {!onWeb && (sshHostViews.length > 0 || panelHostViews.length > 0) && (
            <button
              type="button"
              className={modal === "sftp" ? "on" : ""}
              onClick={() => {
                reloadSshHosts();
                setModal("sftp");
              }}
            >
              <Icon name="truck" />
              SFTP
            </button>
          )}
          {!onWeb && panelHostViews.length > 0 && (
            <button
              type="button"
              className={modal === "hydro" ? "on" : ""}
              onClick={() => {
                reloadPanelHosts();
                setModal("hydro");
              }}
            >
              <Icon name="blocks" />
              {panelsName(panelHostViews)}
            </button>
          )}
          <button
            type="button"
            className={modal === "tasks" ? "on" : ""}
            onClick={() => setModal("tasks")}
          >
            <Icon name="refresh" />
            {tr("Tasks")}
          </button>
          <button
            type="button"
            className={modal === "java" ? "on" : ""}
            onClick={() => {
              void refreshJavaProcs();
              setModal("java");
            }}
          >
            <Icon name={javaProcs.some((proc) => proc.orphan) ? "alert" : "cpu"} />
            {tr("Java processes")}
            {javaProcs.some((proc) => proc.orphan) && <b className="wb-dot warn" />}
          </button>
          {!onWeb && (
            <button
              type="button"
              className={modal === "settings" ? "on" : ""}
              onClick={() => {
                setModal("settings");
                void refreshJavaList();
              }}
            >
              <Icon name="settings" />
              {tr("Settings")}
            </button>
          )}
          <button
            type="button"
            className={modal === "trash" ? "on" : ""}
            onClick={() => setModal("trash")}
          >
            <Icon name="trash" />
            {tr("Trash")}
            {trashEntries.length > 0 && <span className="wb-count">{trashEntries.length}</span>}
          </button>
        </nav>
        <div className="wb-top-end">
          <button
            type="button"
            className="wb-search"
            onClick={() => {
              setPaletteQuery("");
              setPaletteIndex(0);
              setPaletteOpen(true);
            }}
          >
            <Icon name="search" />
            {t("nav.quickRun")}
            <kbd>Ctrl K</kbd>
          </button>
        </div>
      </header>
      {selected && page === "instance" && (
        <div className="wb-tabs" role="tablist" aria-label={tr("Open servers")}>
          {serverTabs.map((id) => {
            const one = instances.find((item) => item.id === id);
            if (!one) return null;
            const on = one.id === selected.id;
            return (
              <div key={id} className={`wb-tab ${on ? "on" : ""}`}>
                <button
                  type="button"
                  role="tab"
                  aria-selected={on}
                  onClick={() => !on && openManage(one)}
                >
                  <span className={`status-dot status-${shownStatus(one)}`} />
                  {one.name}
                </button>
                <button
                  type="button"
                  className="wb-tab-close"
                  aria-label={tr("Close {0}", [one.name])}
                  onClick={() => closeServerTab(id)}
                >
                  <Icon name="close" />
                </button>
              </div>
            );
          })}
          <button
            type="button"
            className="wb-tab-add"
            aria-label={tr("Open another server")}
            data-tip={tr("Open another server")}
            onClick={goHome}
          >
            <Icon name="plus" />
          </button>
          <span className="wb-tabs-gap" />
          {!sideShown && !(tool === "overview" && selected.status === "stopped") && runControls}
        </div>
      )}
      <div className="wb-body">
        {selected && page === "instance" && (
          <nav className="wb-rail" ref={navRef} aria-label={tr("Server tools")}>
            <span
              className={`nav-mark ${navMark.ready ? "ready" : ""}`}
              style={{ top: `${navMark.top}px`, height: `${navMark.height}px` }}
              aria-hidden
            />
            {(selected.panelHost ? panelNav : instanceNav).map((item) => {
              if (!toolAllowed(selected.core, item.id)) return null;
              if (item.id === "claude" && onWeb) return null;
              if (item.id === "software" && (panelHostViews.find((one) => one.id === selected.panelHost)?.kind ?? "hydrodactyl") !== "hydrodactyl") return null;
              const live = selected.status === "running" || selected.status === "stopping";
              const slot =
                item.id === "overview" && live
                  ? { id: "console" as Tool, label: tr("Console"), icon: selected.panelHost ? "terminal" : "console" }
                  : item;
              const section = "section" in item ? (item.section as string | undefined) : undefined;
              const locked = (!!selected.remoteHost || !!selected.panelHost) && !REMOTE_TOOLS.includes(slot.id);
              const label = slot.id === "addons" ? (addonKind(selected.core) === "mod" ? tr("Mods") : tr("Plugins")) : slot.label;
              return (
                <Fragment key={slot.id}>
                  {section && <span className="nav-section">{section}</span>}
                  <button
                    type="button"
                    className={`${tool === slot.id ? "selected" : ""} ${locked ? "locked" : ""}`}
                    data-tool={slot.id}
                    disabled={locked}
                    aria-current={tool === slot.id ? "page" : undefined}
                    data-tip={
                      locked
                        ? selected.panelHost
                          ? tr("This tab isn't available for panel servers yet.")
                          : tr("Not available for SSH servers yet.")
                        : undefined
                    }
                    onClick={() => void loadTool(slot.id)}
                  >
                    <Icon name={slot.icon} />
                    <span>{label}</span>
                  </button>
                </Fragment>
              );
            })}
          </nav>
        )}
      <section className="workspace" key={selected && page === "instance" ? `in-${selected.id}` : "home"}>
        {selected && page === "instance" ? (
          <>
            <header className="topbar instance-topbar">
              <div className="instance-logo" data-tip={coreLabel(selected.core, selected.version)}>
                {serverIcons[selected.id] ? (
                  <img className="server-icon card" src={serverIcons[selected.id]} alt="" />
                ) : (
                  <CoreIcon core={selected.core} />
                )}
              </div>
              <div className="instance-title">
                <h1>{selected.name}</h1>
                <p>
                  <span className="chip" data-core={selected.core}>
                    <CoreIcon core={selected.core} />
                    {isCustom(selected.core) ? tr(customProgram.id) : selected.core}
                  </span>
                  {kindOf(selected.core).family === "minecraft" && selected.version && <span className="chip subtle">{selected.version}</span>}
                  <span className="chip subtle">:{selected.port}</span>
                  {hostChip(selected)}
                  <span className={`status-badge ${statusInfo(shownStatus(selected)).tone}`}>
                    <span className={`status-dot status-${shownStatus(selected)}`} />
                    {statusInfo(shownStatus(selected)).label}
                  </span>
                </p>
              </div>
              <div className="instance-actions">
                <button
                  className="icon-button"
                  data-tip={t("card.folder")}
                  onClick={() => void openServerFolder(selected)}
                >
                  <Icon name="folder" />
                </button>
                <button
                  className="icon-button"
                  data-tip={t("card.rename")}
                  onClick={() => void renameInstance(selected)}
                >
                  <Icon name="pencil" />
                </button>
                {selected.status !== "stopped" && (
                  <>
                    <button
                      className="icon-button"
                      data-tip={tr("Stops the server, then starts it again.")}
                      onClick={() => void restartInstance(selected)}
                    >
                      <Icon name="refresh" />
                    </button>
                    <button
                      className="icon-button force-stop"
                      data-tip={tr("End task — only use this if a normal stop fails. Unsaved progress may be lost.")}
                      onClick={() => void forceStop(selected)}
                    >
                      <Icon name="alert" />
                    </button>
                  </>
                )}
                <button
                  className={`run-button run-pill ${selected.status === "running" ? "stop" : ""} ${selected.status === "stopping" ? "busy" : ""}`}
                  disabled={selected.status === "stopping" || (selected.status === "stopped" && (!!selected.checking || !!selected.unreachable))}
                  onClick={() => void toggle(selected)}
                >
                  <Icon
                    name={selected.status === "running" ? "stop" : "play"}
                  />
                  {selected.status === "running"
                    ? t("card.stop")
                    : selected.status === "stopping"
                      ? t("status.stopping")
                      : t("card.start")}
                </button>
              </div>
            </header>
            <div className="workspace-body instance-body" key={tool}>
              <ErrorBoundary
                onReset={() => {
                  setPage("home");
                  setSelectedId(null);
                  
                }}
              >
                {browseOpen ? browsePage : manage}
                {depAsk && (
                  <DependencyDialog
                    title={depAsk.item.title}
                    dependencies={depAsk.plan.dependencies}
                    chosen={depAsk.chosen}
                    busy={dialogBusy}
                    onToggle={(projectId) =>
                      setDepAsk((open) =>
                        open
                          ? {
                              ...open,
                              chosen: open.chosen.includes(projectId)
                                ? open.chosen.filter((one) => one !== projectId)
                                : [...open.chosen, projectId],
                            }
                          : open,
                      )
                    }
                    onCancel={() => !dialogBusy && setDepAsk(null)}
                    onConfirm={() => void runInstall(depAsk.item, depAsk.chosen)}
                  />
                )}
              </ErrorBoundary>
            </div>
          </>
        ) : (
          <>
        <header className="topbar">
          <div className="topbar-title">
            <h1>{t("top.title")}</h1>
          </div>
          <div className="topbar-actions">
            <label className="search">
              <Icon name="search" />
              <input
                value={filter}
                onChange={(event) => setFilter(event.target.value)}
                placeholder={t("top.search")}
              />
            </label>
            <GameFilter
              value={shownGame}
              counts={gameCounts}
              total={instances.length}
              onChange={(next) => {
                setGameFilter(next);
                localStorage.setItem("monolab-game-filter", next);
              }}
            />
            {!place.startsWith("panel:") && (
              <>
                <button
                  className="button secondary"
                  onClick={() => setModal("import")}
                >
                  <Icon name="import" />
                  {tr("Import server")}</button>
                <button className="button primary" onClick={() => void openCreate()}>
                  <Icon name="plus" />
                  {t("top.create")}
                </button>
              </>
            )}
          </div>
        </header>
        <div className="workspace-body">
          {!loading && (
              <div className="place-row">
              {(remotePlaces.length > 0 || Object.keys(panelHosts).length > 0) && (
                <div className="place-tabs" role="tablist" ref={placeBar}>
                  <span
                    className={`place-mark ${placeMark.ready ? "ready" : ""}`}
                    style={{ left: `${placeMark.left}px`, width: `${placeMark.width}px` }}
                    aria-hidden
                  />
                  {[
                    {
                      id: "local",
                      label: tr("Local"),
                      count: instances.filter((one) => !one.remoteHost && !one.panelHost).length,
                    },
                    ...remotePlaces.map((id) => ({
                      id,
                      label: sshHosts[id] ?? tr("SSH"),
                      count: instances.filter((one) => one.remoteHost === id).length as number | null,
                    })),
                    ...Object.entries(panelHosts).map(([id, name]) => ({
                      id: `panel:${id}`,
                      label: name,
                      count: instances.filter((one) => one.panelHost === id).length as number | null,
                    })),
                  ].map((tab) => (
                    <button
                      key={tab.id}
                      type="button"
                      role="tab"
                      aria-selected={place === tab.id}
                      className={place === tab.id ? "on" : ""}
                      onClick={() => setPlace(tab.id)}
                    >
                      {tab.id.startsWith("panel:") ? <Icon name="cloud" /> : tab.id !== "local" && <Icon name="plug" />}
                      {tab.label}
                      {tab.count !== null && <b>{tab.count}</b>}
                    </button>
                  ))}
                </div>
              )}
              {summaryPills}
              {!onWeb && (
                <div className="place-actions">
                  <button
                    className="button ghost small"
                    type="button"
                    onClick={() => setSessionOpen({ tab: "ssh", adding: true })}
                  >
                    <Icon name="plus" />
                    {tr("Add session")}
                  </button>
                  {(remotePlaces.length > 0 || Object.keys(panelHosts).length > 0) && (
                    <button
                      className="icon-button"
                      type="button"
                      data-tip={tr("Manage sessions")}
                      aria-label={tr("Manage sessions")}
                      onClick={() =>
                        setSessionOpen({ tab: remotePlaces.length ? "ssh" : "panel", adding: false })
                      }
                    >
                      <Icon name="settings" />
                    </button>
                  )}
                </div>
              )}
              </div>
          )}
          {loading ? (
            <div className="booting">
              <span className="brand-mark">
            <img className="brand-img" src={appIcon} alt="" draggable={false} />
          </span>
              <strong>{tr("Loading servers")}</strong>
              <div className="booting-bar">
                <span />
              </div>
            </div>
          ) : place.startsWith("panel:") && !list.length ? (
            <EmptyState
              icon="cloud"
              title={filter ? tr("No matching servers") : tr("None of your servers are on this panel")}
              hint={tr("Your servers and shared servers are added automatically (synced every 5 minutes). An account API key is required. Find other users' servers in the {0} tab.", [panelsName(panelHostViews)])}
              action={
                <Button
                  variant="secondary"
                  icon="refresh"
                  onClick={() =>
                    void commands
                      .syncPanelServers()
                      .then((got) => {
                        void refresh();
                        if (got.failed.length) setError(got.failed.join("\n"));
                      })
                      .catch((cause) => setError(String(cause)))
                  }
                >
                  {tr("Sync now")}
                </Button>
              }
            />
          ) : list.length || creating ? (
            <>
            {(() => {
              const card = (item: Instance) => {
                const live = item.status === "running" || item.status === "stopping";
                const res = resources[item.id];
                const info = statusInfo(shownStatus(item));
                const keep = (event: { stopPropagation: () => void }) => event.stopPropagation();
                return (
                  <article
                    className={`server-card wb-card ${live ? "live" : "off"} ${item.status} ${draggedInstanceId === item.id ? "dragging" : ""}`}
                    key={item.id}
                    data-server-id={item.id}
                    onClick={() => {
                      if (draggedJustNow.current) return;
                      openManage(item);
                    }}
                    onPointerDown={(event) => grabInstance(event, item.id)}
                  >
                    <div className="sc-logo" data-tip={coreLabel(item.core, item.version)}>
                      {serverIcons[item.id] ? (
                        <img className="server-icon card" src={serverIcons[item.id]} alt="" />
                      ) : (
                        <CoreIcon core={item.core} />
                      )}
                    </div>
                    <div className="sc-body">
                      <p className="server-name">
                        <span className="server-name-text">{item.name}</span>
                      </p>
                      <p className="sc-meta">
                        <span className={`wb-state ${info.tone}`}>
                          <span className={`status-dot status-${shownStatus(item)}`} />
                          {info.label}
                        </span>
                        <span className="sc-meta-text">
                          {coreLabel(item.core, item.version)} · :{item.port}
                        </span>
                        {hostChip(item)}
                      </p>
                    </div>
                    <div className="sc-side" onClick={keep} onPointerDown={keep}>
                      {live ? (
                        <>
                          <button
                            type="button"
                            className="wb-icon"
                            aria-label={tr("Restart")}
                            data-tip={tr("Restart")}
                            disabled={item.status === "stopping"}
                            onClick={() => void restartInstance(item)}
                          >
                            <Icon name="refresh" />
                          </button>
                          <button
                            type="button"
                            className="wb-icon danger"
                            aria-label={t("card.stop")}
                            data-tip={item.status === "stopping" ? t("status.stopping") : t("card.stop")}
                            disabled={item.status === "stopping"}
                            onClick={() => void toggle(item)}
                          >
                            <Icon name="stop" />
                          </button>
                        </>
                      ) : item.unreachable && item.remoteHost ? (
                        <button type="button" className="button secondary small wb-run" onClick={() => void retrySsh(item.remoteHost)}>
                          <Icon name="refresh" />
                          {tr("Retry connection")}
                        </button>
                      ) : (
                        <button
                          type="button"
                          className="button primary small wb-run"
                          disabled={!!item.checking || !!item.unreachable}
                          onClick={() => void toggle(item)}
                        >
                          <Icon name="play" />
                          {t("card.start")}
                        </button>
                      )}
                      <DropdownMenu
                        trigger={
                          <button type="button" className="wb-icon ghost" aria-label={tr("More")}>
                            <Icon name="dots" />
                          </button>
                        }
                        items={[
                          { label: t("card.rename"), icon: "pencil", onSelect: () => void renameInstance(item) },
                          ...(!item.panelHost && item.status === "stopped"
                            ? [{ label: t("card.duplicate"), icon: "copy", onSelect: () => void duplicateInstance(item) }]
                            : []),
                          ...(item.status === "stopped"
                            ? [{
                                label: item.linked || item.remoteHost ? tr("Remove from list") : t("card.trash"),
                                icon: "trash",
                                danger: true,
                                onSelect: () => void trashInstance(item),
                              }]
                            : []),
                        ]}
                      />
                    </div>
                    {live && (
                      <div className="sc-tiles">
                        <div>
                          <span>CPU</span>
                          <strong>{res ? orDash(res.cpuPercent ?? 0, (v) => `${Math.round(v)}%`) : "—"}</strong>
                        </div>
                        <div>
                          <span>{tr("Memory")}</span>
                          <strong>
                            {res ? used(res.memoryMib) : "—"}
                            {has(item.core, "java") && <small> / {bytes(item.maxMemoryMib)}</small>}
                          </strong>
                        </div>
                        {has(item.core, "ping") && (
                          <div className="player-peek-host">
                            <span>{tr("Players")}</span>
                            <strong>
                              {pings[item.id] ? (
                                <>
                                  {pings[item.id].online}
                                  <small> / {pings[item.id].max}</small>
                                </>
                              ) : (
                                "—"
                              )}
                            </strong>
                            {pings[item.id] && <PlayerPeek names={pings[item.id].names} online={pings[item.id].online} />}
                          </div>
                        )}
                      </div>
                    )}
                  </article>
                );
              };
              const liveList = list.filter((one) => one.status === "running" || one.status === "stopping");
              const offList = list.filter((one) => !(one.status === "running" || one.status === "stopping"));
              return (
                <>
                  {liveList.length > 0 && <div className="server-layout cards wb-live">{liveList.map(card)}</div>}
                  {offList.length > 0 && (
                    <section className="wb-off" aria-label={tr("Stopped servers")}>
                      {liveList.length > 0 && (
                        <h2 className="wb-sec">
                          {tr("Stopped servers")} <b>{offList.length}</b>
                        </h2>
                      )}
                      <div className="server-layout rows">{offList.map(card)}</div>
                    </section>
                  )}
                </>
              );
            })()}
            {draggedInstanceId && dragY !== null && grabRef.current && (
              <div className="server-layout cards drag-ghost-layer" aria-hidden>
                <article
                  className={`${grabRef.current.className} drag-ghost`}
                  style={{
                    top: dragY - grabRef.current.offsetY,
                    left: dragX !== null ? dragX - grabRef.current.offsetX : grabRef.current.left,
                    width: grabRef.current.width,
                  }}
                  dangerouslySetInnerHTML={{ __html: grabRef.current.html }}
                />
              </div>
            )}
            </>
          ) : filter || shownGame ? (
            <div className="empty-state">
              <div className="empty-symbol">
                <Icon name="search" />
              </div>
              <h2>{t("empty.noMatch")}</h2>
              <p>{tr("Clear the search to see the full list.")}</p>
              <div className="empty-actions">
                <button
                  className="button secondary"
                  onClick={() => {
                    setFilter("");
                    setGameFilter("");
                    localStorage.setItem("monolab-game-filter", "");
                  }}
                >
                  {t("empty.clearSearch")}
                </button>
              </div>
            </div>
          ) : (
            <div className="empty-state welcome">
              <div className="empty-cores">
                {["Paper", "Fabric", "Vanilla", "Purpur", "NeoForge"].map(
                  (core) => (
                    <span
                      key={core}
                      className="core-mark"
                    >
                      <CoreIcon core={core} />
                    </span>
                  ),
                )}
              </div>
              <h2>{t("empty.title")}</h2>
              <p>{t("empty.body")}</p>
              <div className="empty-actions">
                <button
                  className="button secondary"
                  onClick={() => setModal("import")}
                >
                  <Icon name="import" />
                  {t("top.import")}
                </button>
                <button
                  className="button primary"
                  onClick={() => void openCreate()}
                >
                  <Icon name="plus" />
                  {t("top.create")}
                </button>
              </div>
            </div>
          )}
        </div>
          </>
        )}
      </section>
        {selected && sidePanel && (
          <aside className="wb-side" aria-label={tr("Server status")}>
            <div className="wb-side-head">
              <div className="wb-side-title">
                <span className={`wb-state ${statusInfo(shownStatus(selected)).tone}`}>
                  <span className={`status-dot status-${shownStatus(selected)}`} />
                  {statusInfo(shownStatus(selected)).label}
                </span>
                <strong>{selected.name}</strong>
                {tool === "console" && (
                  <small>
                    {`${coreLabel(selected.core, selected.version)}${selected.coreBuild ? ` · ${selected.coreBuild}` : ""} · :${selected.port}`}
                  </small>
                )}
              </div>
              {runControls}
            </div>
            {hostChip(selected)}
            {tool === "console" && wide && selected.status === "running" && (
              <ServerStats bare server={{ id: selected.id, name: selected.name, panelHost: selected.panelHost, maxMemoryMib: kindOf(selected.core).family === "minecraft" ? selected.maxMemoryMib : 0, port: selected.port, status: selected.status, uptimeSeconds: selected.uptimeSeconds }} onOpenPlayer={() => void loadTool("players")} />
            )}
            {tool === "properties" && propsChanged.length > 0 && (
              <div className="wb-changes">
                <span className="wb-side-label">{tr("Not saved yet")}</span>
                <strong className="wb-side-big">{tr("{0} changed", [propsChanged.length])}</strong>
                <ul>
                  {propsChanged.slice(0, 12).map((entry) => {
                    const before = savedProperties.find((one) => one.key === entry.key);
                    return (
                      <li key={entry.key}>
                        <span>{propertyMeta(entry.key).label}</span>
                        <code>
                          <s>{before ? before.value || "—" : "—"}</s>
                          <i aria-hidden>→</i>
                          <b>{entry.value || "—"}</b>
                        </code>
                      </li>
                    );
                  })}
                </ul>
                {selected.status !== "stopped" && <small>{tr("Applies on the next start.")}</small>}
                <div className="wb-side-actions">
                  <button className="button primary wb-start" onClick={() => void saveProperties()}>
                    {tr("Save")}
                  </button>
                  <button className="button secondary wb-start" onClick={() => setProperties(savedProperties)}>
                    <Icon name="undo" />
                    {tr("Roll back")}
                  </button>
                </div>
              </div>
            )}
          </aside>
        )}
      </div>
      </div>
      {modal !== "none" && (
        <div
          className={`modal-backdrop ${modalShell[modal]}`}
          onClick={() => {
            if (creating) return;
            setModal("none");
            setTaskDraft(null);
          }}
        >
          <section
            key={modal}
            className={`add-modal modal-${modal}`}
            role="dialog"
            aria-modal="true"
            onClick={(event) => event.stopPropagation()}
          >
            {(modalShell[modal] !== "as-panel" || modal === "create") && (
              <button
                className="close"
                data-tip={tr("Close")}
                onClick={() => {
                  if (creating) return;
                  setModal("none");
                  setTaskDraft(null);
                }}
              >
                <Icon name="close" />
              </button>
            )}
            {modal === "precheck" && findings && (
              <>
                <h2>{tr("Before starting")}</h2>
                <p className="modal-copy">
                  {tr("Checked for common issues.")}{" "}<b>{tr("Blocked")}</b>{tr(" will keep the server from starting.")}</p>
                <div className="player-card">
                  {findings.map((one) => (
                    <div className="setting-card" key={one.title}>
                      <span className="setting-title">
                        <b className={one.level === "Blocked" ? "stop" : "warn"}>
                          {trBack(one.level)}
                        </b>{" "}
                        {trBack(one.title)}
                      </span>
                      <p>{trBack(one.fix)}</p>
                    </div>
                  ))}
                </div>
                <div className="ask-buttons">
                  <button
                    className="button primary"
                    onClick={() => {
                      setModal("none");
                      const go = selected;
                      if (go) void toggle(go, true);
                    }}
                  >
                    {tr("Start anyway")}</button>
                </div>
              </>
            )}
            {modal === "java" && (
              <>
                <div className="panel-head">
                  <div>
                    <h2>{tr("Running Java processes")}</h2>
                    <p className="modal-copy">
                      {tr("Uses the command line to")}{" "}<b>{tr("identify the server")}</b>{tr(". Servers the app doesn't recognize appear here too.")}{" "}<b>{tr("Servers outside the app")}</b>{tr(": shown in yellow.")}</p>
                  </div>
                  <button
                    className="button secondary"
                    onClick={() => void refreshJavaProcs()}
                  >
                    <Icon name="refresh" />
                    {tr("Refresh")}</button>
                </div>
                <div className="java-page">
                  {javaProcs.length ? (
                    javaProcs.map((proc) => (
                      <article
                        className={`java-proc ${proc.orphan ? "orphan" : ""}`}
                        key={proc.pid}
                      >
                        <span className="java-mark">
                          <Icon name={proc.orphan ? "alert" : "server"} />
                        </span>
                        <div className="java-body">
                          <strong>{trBack(proc.label)}</strong>
                          <small>
                            {proc.detail ? `${trBack(proc.detail)} · ` : ""}
                            {proc.pid}{tr("suffix.times")}{proc.orphan ? tr(" · running outside the app") : ""}
                          </small>
                        </div>
                        <span className="java-mem">{proc.memoryMib}MB</span>
                        <button
                          className="button secondary small"
                          onClick={() => void killJavaProc(proc)}
                        >
                          {tr("Terminate")}</button>
                      </article>
                    ))
                  ) : (
                    <EmptyState
                      icon="cpu"
                      title={tr("No Java processes are running.")}
                      hint={tr("Servers appear here when they start. You can also end servers the app lost track of after restarting.")}
                    />
                  )}
                </div>
              </>
            )}
            {modal === "web" && accountDraft && (
              <>
                <h2>{accountDraft.isNew ? tr("Add account") : tr("Edit account")}</h2>
                <p className="modal-copy">
                  {tr("Sign in on the web with this account.")}</p>
                <div className="task-form">
                  <label className="field-block">
                    <span className="field-label">{tr("Name")}</span>
                    <input
                      autoFocus
                      value={accountDraft.name}
                      disabled={!accountDraft.isNew}
                      placeholder={tr("e.g. friend")}
                      onChange={(event) =>
                        setAccountDraft({ ...accountDraft, name: event.target.value })
                      }
                    />
                  </label>
                  <label className="field-block">
                    <span className="field-label">{tr("Password")}</span>
                    <input
                      type="password"
                      value={accountDraft.password}
                      placeholder={
                        accountDraft.isNew
                          ? tr("At least 8 characters")
                          : tr("Leave empty to keep the current password")
                      }
                      onChange={(event) =>
                        setAccountDraft({ ...accountDraft, password: event.target.value })
                      }
                    />
                  </label>
                  <div className="field-block">
                    <span className="field-label">{tr("Role")}</span>
                    <Dropdown
                      value={accountDraft.role}
                      options={[
                        { value: "Admin", label: tr("Admin · all servers") },
                        { value: "Manager", label: tr("Manager · assigned servers only") },
                      ]}
                      onChange={(role) => setAccountDraft({ ...accountDraft, role })}
                    />
                  </div>
                  {accountDraft.role === "Manager" && (
                    <div className="field-block">
                      <span className="field-label">{tr("Assigned servers")}</span>
                      {instances.length ? (
                        <div className="check-list">
                          {instances.map((instance) => (
                            <label className="check-row" key={instance.id}>
                              <input
                                type="checkbox"
                                checked={accountDraft.servers.includes(instance.id)}
                                onChange={(event) =>
                                  setAccountDraft({
                                    ...accountDraft,
                                    servers: event.target.checked
                                      ? [...accountDraft.servers, instance.id]
                                      : accountDraft.servers.filter(
                                          (one) => one !== instance.id,
                                        ),
                                  })
                                }
                              />
                              <span>{instance.name}</span>
                              <small>
                                {instance.core} {instance.version}
                              </small>
                            </label>
                          ))}
                        </div>
                      ) : (
                        <p className="tool-empty">{tr("No servers added yet.")}</p>
                      )}
                    </div>
                  )}
                </div>
                <div className="modal-actions">
                  <button
                    className="button secondary"
                    onClick={() => setAccountDraft(null)}
                  >
                    {tr("Cancel")}</button>
                  <button className="button primary" onClick={() => void saveWebAccount()}>
                    {tr("Save")}</button>
                </div>
              </>
            )}
            {modal === "web" && !accountDraft && (
              <>
                <div className="panel-head">
                  <div>
                    <h2>{tr("Web panel")}</h2>
                    <p className="modal-copy">{tr("View and manage this app's servers from your phone or laptop browser.")}</p>
                  </div>
                </div>
                <div className="web-layout">
                  <section className={`web-card ${webPanel.on ? "live" : ""}`}>
                    <header className="web-card-head">
                      <span className="web-mark">
                        <Icon name="cloud" />
                      </span>
                      <div>
                        <strong>{webPanel.on ? tr("The web panel is running") : tr("The web panel is off")}</strong>
                        <small>
                          {webPanel.on
                            ? tr("{0} viewing", [webPanel.viewers])
                            : tr("Turn it on to check servers from a browser. Sign in to start, stop and send commands.")}
                        </small>
                      </div>
                      <Badge tone={webPanel.on ? "ok" : "subtle"}>{webPanel.on ? tr("On") : tr("Off")}</Badge>
                    </header>
                    {webPanel.on ? (
                      <>
                        <div className="web-live">
                          <div className="web-links">
                            <button
                              className={`web-link copy-mark ${copiedKey === "web-in" ? "done" : ""}`}
                              onClick={() => {
                                flashCopied("web-in");
                                void navigator.clipboard.writeText(webPanel.address);
                                setNotice(tr("Address copied!"));
                              }}
                            >
                              <span>{tr("Local")}</span>
                              <code>{webPanel.address}</code>
                              <Icon name={copiedKey === "web-in" ? "check" : "copy"} />
                            </button>
                            {webPanel.tunnel && webPanel.tunnelAddress && (
                              <button
                                className={`web-link copy-mark ${copiedKey === "web-out" ? "done" : ""}`}
                                onClick={() => {
                                  flashCopied("web-out");
                                  void navigator.clipboard.writeText(webPanel.tunnelAddress);
                                  setNotice(tr("Address copied!"));
                                }}
                              >
                                <span>{tr("Public")}</span>
                                <code>{webPanel.tunnelAddress}</code>
                                <Icon name={copiedKey === "web-out" ? "check" : "copy"} />
                              </button>
                            )}
                            {webPanel.direct && (
                              <span className="web-link web-link-note" data-tip={tr("You can find your public IP in your router's admin settings.")}>
                                <span>{tr("Direct")}</span>
                                <code>http://{tr("Public IP")}:{webPanel.port}/</code>
                              </span>
                            )}
                            {!webPanel.accounts && <p className="muted web-note">{tr("No accounts yet. You can only view the panel.")}</p>}
                          </div>
                          <QrCode text={webPanel.tunnel && webPanel.tunnelAddress ? webPanel.tunnelAddress : webPanel.address} />
                        </div>
                        {webPanel.direct && (
                          <Banner tone="danger">
                            {tr("If your router forwards port {0} to {1}, the panel is accessible from the internet. The connection isn't encrypted, so your password is sent in plain text.", [webPanel.port, lanHost(webPanel.address)])}
                          </Banner>
                        )}
                        <div className="web-actions">
                          <button className="button secondary" disabled={tunnelBusy} onClick={() => void toggleTunnel(!webPanel.tunnel)}>
                            <Icon name="cloud" />
                            {tunnelBusy ? tr("Connecting…") : webPanel.tunnel ? tr("Stop public access") : tr("Open to the internet")}
                          </button>
                          <button className="button ghost" disabled={tunnelBusy} onClick={() => void toggleDirect(!webPanel.direct)}>
                            <Icon name="shield" />
                            {webPanel.direct ? tr("Turn off port-forwarding mode") : tr("Use port forwarding")}
                          </button>
                          <span className="live-spacer" />
                          <button className="button danger" onClick={() => void toggleWebPanel(false)}>
                            <Icon name="stop" />
                            {tr("Disable")}
                          </button>
                        </div>
                      </>
                    ) : (
                      <>
                        <div className="web-ways" role="radiogroup" aria-label={tr("Where to open it")}>
                          {[
                            { id: "none", icon: "home", label: tr("Same Wi-Fi"), hint: tr("Only devices at home can open it.") },
                            { id: "tunnel", icon: "cloud", label: tr("Cloudflare Tunnel"), hint: tr("Encrypted. The address changes each time it starts.") },
                            { id: "direct", icon: "plug", label: tr("Port forwarding"), hint: tr("If your router forwards the port, it isn't encrypted and the address stays the same.") },
                          ].map((way) => {
                            const on = way.id === "tunnel" ? openOutside : way.id === "direct" ? openDirect : !openOutside && !openDirect;
                            return (
                              <label className={`web-way ${on ? "on" : ""}`} key={way.id}>
                                <input
                                  type="radio"
                                  name="web-way"
                                  checked={on}
                                  onChange={() => {
                                    setOpenOutside(way.id === "tunnel");
                                    setOpenDirect(way.id === "direct");
                                  }}
                                />
                                <i className="web-way-icon">
                                  <Icon name={way.icon} />
                                </i>
                                <span>
                                  <b>{way.label}</b>
                                  <small>{way.hint}</small>
                                </span>
                                <span className="web-way-radio" aria-hidden />
                              </label>
                            );
                          })}
                        </div>
                        {webAccounts.length === 0 && (
                          <Banner tone="info">{tr("Signing in is required, so create an admin account first (on the right).")}</Banner>
                        )}
                        <div className="web-actions">
                          <label className="web-port">
                            {tr("Port")}
                            <input
                              type="number"
                              min={1024}
                              max={65535}
                              value={webPanel.port}
                              onChange={(event) => setWebPanel({ ...webPanel, port: Number(event.target.value) || 25566 })}
                            />
                          </label>
                          <span className="live-spacer" />
                          <button className="button primary" disabled={tunnelBusy || webAccounts.length === 0} onClick={() => void toggleWebPanel(true)}>
                            <Icon name="play" />
                            {tunnelBusy ? tr("Connecting…") : tr("Start web panel")}
                          </button>
                        </div>
                      </>
                    )}
                  </section>
                  <section className="web-card web-accounts">
                    <header className="web-card-head">
                      <div>
                        <strong>{tr("Accounts")}</strong>
                        <small>{webAccounts.length ? tr("{0, plural, one {# account} other {# accounts}} · sign in with these", [webAccounts.length]) : tr("None yet")}</small>
                      </div>
                      <button
                        className={`button ${webAccounts.length ? "secondary" : "primary"} small`}
                        onClick={() => setAccountDraft({ name: "", role: "Admin", servers: [], password: "", isNew: true })}
                      >
                        <Icon name="plus" />
                        {webAccounts.length ? tr("Add") : tr("Create admin account")}
                      </button>
                    </header>
                    {webAccounts.map((account) => (
                      <div className="web-account" key={account.name}>
                        <span className="web-account-mark">{account.name.slice(0, 1).toUpperCase()}</span>
                        <div>
                          <strong>{account.name}</strong>
                          <small>
                            {trBack(account.role)}
                            {account.role === "Manager"
                              ? ` · ${account.servers.length ? tr("{0, plural, one {# server} other {# servers}}", [account.servers.length]) : tr("No servers assigned")}`
                              : tr(" · all servers")}
                          </small>
                        </div>
                        <IconButton label={tr("Edit")} icon="pencil" onClick={() => setAccountDraft({ ...account, password: "", isNew: false })} />
                        <IconButton label={tr("Delete")} icon="trash" className="danger" onClick={() => void deleteWebAccount(account.name)} />
                      </div>
                    ))}
                  </section>
                </div>
              </>
            )}
            {modal === "hydro" && (
              <>
                <div className="panel-head">
                  <div>
                    <h2>{panelsName(panelHostViews)}</h2>
                  </div>
                  <Button
                    variant="ghost"
                    icon="refresh"
                    onClick={() =>
                      void commands
                        .syncPanelServers()
                        .then((got) => {
                          void refresh();
                          setNotice(tr("Synced the server list · added {0} · removed {1}", [got.added, got.removed]));
                          if (got.failed.length) setError(got.failed.join("\n"));
                        })
                        .catch((cause) => setError(String(cause)))
                    }
                  >
                    {tr("Sync server list")}
                  </Button>
                </div>
                <Suspense fallback={null}>
                <PanelAdmin
                  hosts={panelHostViews}
                  onError={setError}
                  onNotice={setNotice}
                  onSynced={() => {
                    reloadPanelHosts();
                    void refresh();
                  }}
                  confirm={confirmAction}
                  ask={askText}
                  openSessions={() => {
                    setModal("none");
                    setSessionOpen({ tab: "panel", adding: true });
                  }}
                />
                </Suspense>
              </>
            )}
            {modal === "sftp" && (
              <>
                <div className="panel-head">
                  <div>
                    <h2>SFTP</h2>
                    <p className="modal-copy">{tr("Choose local or a server at the top of each pane. Select files and use the button below to send them across, or drag and drop them.")}</p>
                  </div>
                </div>
                <Suspense fallback={null}>
                <SftpPage
                  sshHosts={sshHostViews}
                  panelServers={instances.filter((one) => one.panelHost).map((one) => ({ id: one.id, name: one.name }))}
                  onError={setError}
                  onNotice={setNotice}
                  onSshChanged={(host) => void adoptRemoteServers(host)}
                  confirm={confirmAction}
                  ask={askText}
                />
                </Suspense>
              </>
            )}
            {modal === "trash" && (
              <>
                <div className="panel-head">
                  <div>
                    <h2>{tr("Trash")}</h2>
                    <p className="modal-copy">{tr("Servers removed from the list. Restore them with their previous settings.")}</p>
                  </div>
                  {trashEntries.length > 0 && (
                    <button className="button danger" type="button" onClick={() => void emptyTrash()}>
                      <Icon name="trash" />
                      {tr("Empty trash")}
                    </button>
                  )}
                </div>
                {trashEntries.length === 0 ? (
                  <EmptyState icon="trash" title={tr("Trash is empty")} />
                ) : (
                  <div className="trash-list">
                    {trashEntries.map((entry) => (
                      <div className="trash-item" key={entry.id} data-trash-id={entry.id}>
                        <span className="rail-tile big">
                          {entry.instance ? <CoreIcon core={entry.instance.core} /> : <Icon name="server" />}
                        </span>
                        <span className="trash-item-body">
                          <strong>{entry.name}</strong>
                          <small>
                            {entry.instance ? `${coreLabel(entry.instance.core, entry.instance.version)} · ` : ""}
                            {tr("Delete {0}", [relativeTime(entry.deletedAt)])}
                          </small>
                        </span>
                        <button className="button secondary small" type="button" onClick={() => void restoreTrashInstance(entry)}>
                          <Icon name="undo" />
                          {t("nav.restore")}
                        </button>
                        <IconButton
                          icon="trash"
                          label={tr("Delete permanently")}
                          className="danger"
                          onClick={() => void purgeTrash(entry)}
                        />
                      </div>
                    ))}
                  </div>
                )}
              </>
            )}
            {modal === "tasks" && (
              <TasksPage
                tasks={taskList}
                servers={instances.map((one) => ({ id: one.id, name: one.name, core: one.core, version: one.version }))}
                draft={taskDraft}
                onDraft={setTaskDraft}
                onChanged={setTaskList}
                onNotice={setNotice}
                onError={setError}
                confirm={confirmAction}
              />
            )}
            {modal === "settings" && (
              <>
                <h2>{tr("Settings")}</h2>
                <div className="settings-list">
                  <section className="settings-group">
                    <h3 className="settings-group-title"><Icon name="sparkles" />{tr("Display")}</h3>
                    <div className="settings-group-rows">
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Language")}</strong>
                    </div>
                    <div className="setting-input theme-pick">
                      <Dropdown
                        value={language()}
                        onChange={(next) => switchLanguage(next)}
                        options={languageCodes().map((code) => ({
                          value: code,
                          label: languageLabels[code] ?? code,
                        }))}
                      />
                    </div>
                  </div>
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Theme")}</strong>
                    </div>
                    <div className="setting-input theme-pick">
                      <Dropdown
                        value={theme}
                        onChange={(next) => setTheme(next as Theme)}
                        options={[
                          { value: "dark", label: tr("Dark") },
                          { value: "light", label: tr("Light") },
                          { value: "oled", label: tr("OLED black") },
                          { value: "system", label: tr("System") },
                        ]}
                      />
                    </div>
                  </div>
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Compact lists")}</strong>
                    </div>
                    <label className="switch">
                      <input
                        type="checkbox"
                        aria-label={tr("Compact lists")}
                        checked={compact}
                        onChange={(event) => setCompact(event.target.checked)}
                      />
                      <span />
                    </label>
                  </div>
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Console history size (lines)")}</strong>
                    </div>
                    <div className="setting-input">
                      <input
                        type="number"
                        min={200}
                        max={20000}
                        step={100}
                        value={settings.consoleLineLimit}
                        onChange={(event) =>
                          void updateSettings({
                            consoleLineLimit: Number(event.target.value) || 2000,
                          })
                        }
                      />
                      <span>{tr(" lines")}</span>
                    </div>
                  </div>
                    </div>
                  </section>
                  <section className="settings-group">
                    <h3 className="settings-group-title"><Icon name="server" />{tr("Server")}</h3>
                    <div className="settings-group-rows">
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Server location")}</strong>
                      <small className="setting-path">
                        {serversRoot || tr("Loading…")}
                      </small>
                    </div>
                    <div className="setting-buttons">
                      <button
                        className="button secondary small"
                        onClick={() => void pickServersRoot()}
                      >
                        {tr("Choose folder")}</button>
                      <button
                        className="button ghost small"
                        onClick={() => void resetServersRoot()}
                      >
                        {tr("Restore")}</button>
                    </div>
                  </div>
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Default memory for new servers")}</strong>
                    </div>
                    <div className="setting-input">
                      <input
                        type="number"
                        min={512}
                        max={65536}
                        step={512}
                        value={settings.defaultMemoryMib}
                        onChange={(event) =>
                          void updateSettings({
                            defaultMemoryMib: Number(event.target.value) || 4096,
                          })
                        }
                      />
                      <span>MB</span>
                    </div>
                  </div>
                  <div className="setting-row java-owned">
                    <div>
                      <strong>{tr("Downloaded Java")}</strong>
                      <small>
                        {ownedJavas.length
                          ? tr("Items downloaded by Monolab. You can remove anything you no longer use.")
                          : tr("No Java downloaded yet. Use \"Install Java\" on a server screen.")}
                      </small>
                      {ownedJavas.length > 0 && (
                        <div className="java-owned-list">
                          {ownedJavas.map((one) => (
                            <div className="java-owned-row" key={one.path}>
                              <b>Java {one.major}</b>
                              <code>{one.path}</code>
                              <button
                                className="button ghost small"
                                onClick={() => void removeJava(one)}
                              >
                                {tr("Remove")}</button>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                    <button
                      className="button secondary small"
                      data-tip={tr("Scans this computer for installed Java versions again.")}
                      onClick={() => void refreshJavaList()}
                    >
                      <Icon name="refresh" />
                      {tr("Scan again")}</button>
                  </div>
                    </div>
                  </section>
                  <section className="settings-group">
                    <h3 className="settings-group-title"><Icon name="archive" />{tr("Backups")}</h3>
                    <div className="settings-group-rows">
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Backup location")}</strong>
                      <small className="setting-path">
                        {settings.backupRoot ||
                          tr("Inside the server folder (a different drive is recommended)")}
                      </small>
                    </div>
                    <div className="setting-buttons">
                      <button
                        className="button secondary small"
                        onClick={() => void pickBackupRoot()}
                      >
                        {tr("Choose folder")}</button>
                      {settings.backupRoot && (
                        <button
                          className="button ghost small"
                          onClick={() => void updateSettings({ backupRoot: "" })}
                        >
                          {tr("Restore")}</button>
                      )}
                    </div>
                  </div>
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Scheduled backups: worlds only")}</strong>
                      <small>
                        {tr("With lots of mods, hundreds of MB are compressed again every day, even though only the world changes.")}{" "}<b>{tr("Back up now always includes")}</b>{" "}
                        {tr("the entire server folder.")}</small>
                    </div>
                    <label className="switch">
                      <input
                        type="checkbox"
                        checked={!!settings.backupWorldsOnly}
                        onChange={(event) =>
                          void updateSettings({ backupWorldsOnly: event.target.checked })
                        }
                      />
                      <span />
                    </label>
                  </div>
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Keep backups for")}</strong>
                      <small>
                        {tr("Backups older than this are removed automatically. 0 keeps them forever.")}</small>
                    </div>
                    <div className="setting-input">
                      <input
                        type="number"
                        min={0}
                        max={3650}
                        value={settings.keepBackupDays}
                        onChange={(event) =>
                          void updateSettings({
                            keepBackupDays: Number(event.target.value) || 0,
                          })
                        }
                      />
                      <span>{tr(" days")}</span>
                    </div>
                  </div>
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Scheduled backups to keep")}</strong>
                      <small>
                        {tr("Older scheduled backups are removed once there are more than this. Manual backups stay. 0 keeps them all.")}</small>
                    </div>
                    <div className="setting-input">
                      <input
                        type="number"
                        min={0}
                        max={1000}
                        value={settings.keepAutoBackups}
                        onChange={(event) =>
                          void updateSettings({
                            keepAutoBackups: Number(event.target.value) || 0,
                          })
                        }
                      />
                      <span>{tr(" backups")}</span>
                    </div>
                  </div>
                    </div>
                  </section>
                  <section className="settings-group">
                    <h3 className="settings-group-title"><Icon name="cloud" />{tr("Notifications & updates")}</h3>
                    <div className="settings-group-rows">
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Windows notifications")}</strong>
                      <small>
                        {desktopAlertsEnabled
                          ? tr("Enabled. You'll get Windows notifications when a server crashes, lags (TPS below 15 for a minute) or a task sends an alert.")
                          : tr("Get Windows notifications when a server crashes, lags (TPS below 15 for a minute) or a task sends an alert. Windows asks for permission the first time.")}
                      </small>
                    </div>
                    <button
                      className={`button ${desktopAlertsEnabled ? "secondary" : "primary"} small`}
                      onClick={() =>
                        desktopAlertsEnabled
                          ? disableDesktopAlerts()
                          : void enableDesktopAlerts()
                      }
                    >
                      {desktopAlertsEnabled ? tr("Disable") : tr("Enable")}
                    </button>
                  </div>
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Update")}</strong>
                      <small>
                        {updateState.stage === "found"
                          ? tr("Version {0} is available.", [updateState.version])
                          : updateState.stage === "downloading"
                            ? tr("Downloading… {0}%", [updateState.percent ?? 0])
                            : updateState.stage === "ready"
                              ? tr("Download complete. Restarting the app.")
                              : updateState.stage === "checking"
                                ? tr("Checking…")
                                : tr("You're on the latest version.")}
                      </small>
                    </div>
                    <div className="setting-buttons">
                      {updateState.stage === "found" ? (
                        <button
                          className="button primary small"
                          onClick={() => void installUpdate()}
                        >
                          {tr("Download and install")}</button>
                      ) : (
                        <button
                          className="button secondary small"
                          disabled={
                            updateState.stage === "checking" ||
                            updateState.stage === "downloading"
                          }
                          onClick={() => void checkForUpdate()}
                        >
                          {tr("Check now")}</button>
                      )}
                    </div>
                  </div>
                    </div>
                  </section>
                  <section className="settings-group">
                    <h3 className="settings-group-title"><Icon name="import" />{tr("Data")}</h3>
                    <div className="settings-group-rows">
                  <div className="setting-row">
                    <div>
                      <strong>{tr("Go to settings")}</strong>
                      <small>
                        {tr("Transfer your server list and settings to another computer using a file.")}</small>
                    </div>
                    <div className="setting-buttons">
                      <button
                        className="button secondary small"
                        onClick={() => void exportSettings()}
                      >
                        {tr("Export")}</button>
                      <button
                        className="button ghost small"
                        onClick={() => void importSettings()}
                      >
                        {tr("Import")}</button>
                    </div>
                  </div>
                    </div>
                  </section>
                  <section className="settings-group">
                    <h3 className="settings-group-title"><Icon name="info" />{tr("About")}</h3>
                    <div className="settings-group-rows">
                  <div className="setting-row about-row">
                    <div>
                      <strong>{tr("About")}</strong>
                      <small>Monolab {appVersion ? `v${appVersion}` : ""}</small>
                      <small className="about-notice">
                        {tr("This app is a personal project and is not affiliated with Mojang or Microsoft.")}
                        {" "}
                        NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR MICROSOFT.
                      </small>
                      <small className="about-notice">
                        {tr("Server files (Vanilla, Paper, Purpur, Leaf, Folia, Fabric, Forge, NeoForge, Pumpkin) are not bundled; they are downloaded from each project's official source and are subject to that project's terms.")}
                      </small>
                      <small className="about-notice">
                        {tr("The Paper and Velocity logos are PaperMC brand assets. The Purpur, Fabric, Forge and NeoForge logos belong to their respective projects. All logos use their original colors and proportions.")}
                      </small>
                      <small className="about-notice">
                        {tr("See the open-source notices for the full licenses of Inter, JetBrains Mono, Noto Sans KR (SIL OFL 1.1) and all open-source components used.")}
                      </small>
                      <small className="about-notice">
                        {tr("Provided as is. This is a personal project, and the developer isn't liable for issues. Keep your own backups of the server folder, especially worlds.")}
                      </small>
                      <small className="about-notice">
                        {tr("No usage data is sent anywhere. The internet is used only to download server files, plugins and mods from their distributors and to check for updates. The web panel and tunnel open only when you turn them on.")}
                      </small>
                    </div>
                    {!onWeb && (
                      <div className="setting-buttons">
                        <button
                          className="button secondary small"
                          onClick={() => void call(() => commands.openNotices(), tr("Opened the open-source notices."))}
                        >
                          {tr("Open-source notices")}</button>
                      </div>
                    )}
                  </div>
                    </div>
                  </section>
                </div>
              </>
            )}
            {modal === "mrpack" && (
              <>
                <h2>{tr("Modpack")}</h2>
                <p className="modal-copy">
                  <b>Modrinth(.mrpack)</b>{tr(": downloads mods too,")}<b> CurseForge(.zip)</b>{tr(": extracts settings only.")}</p>
                <div className="mrpack-page">
            <section className="mrpack-panel">
              <div>
                <strong>{tr("From a local file")}</strong>
                <p>{tr("Choose a file you already downloaded.")}</p>
              </div>
              <div className="mrpack-input">
                <button
                  className="path-picker"
                  onClick={() => {
                    setMrpackEntries([]);
                    setCfPack(null);
                    void pickMrpackFile();
                  }}
                >
                  <Icon name="archive" />
                  <span>{mrpackPath || tr("Choose a modpack file (.mrpack · .zip)")}</span>
                </button>
              </div>
              {cfPack && (
                <>
                  <div className="cf-head">
                    <strong>
                      {cfPack.name} {cfPack.version}
                    </strong>
                    <span className="muted">
                      {tr("Minecraft")}{" "}{cfPack.gameVersion}
                      {cfPack.loader ? ` · ${cfPack.loader} ${cfPack.loaderVersion}` : ""}
                      {cfPack.author ? ` · ${cfPack.author}` : ""}
                    </span>
                  </div>
                  {selected && !packFitsServer(cfPack, selected) && (
                    <p className="cf-warn">
                      {tr("This pack is for")}{" "}<b>{cfPack.gameVersion}</b>
                      {cfPack.loader ? ` ${cfPack.loader}` : ""}{tr(" but this server is")}{" "}
                      <b>
                        {selected.version} {selected.core}
                      </b>
                      {tr(". Installing it as is may keep the server from starting.")}</p>
                  )}
                  <div className="mrpack-summary">
                    <span>{tr("Config files")}{" "}{cfPack.overrideCount}{tr("suffix.count")}</span>
                    <span>{tr("Mods in the pack")}{" "}{cfPack.bundledMods}{tr("suffix.count")}</span>
                    <span>{tr("Mods to download manually")}{" "}{cfPack.mods.length}{tr("suffix.count")}</span>
                  </div>
                  <p className="cf-note">
                    {tr("CurseForge lets authors block third-party downloads, so the app doesn't download those mods for you.")}{" "}<b>{tr("Config files and bundled mods are added")}</b>{tr(", the rest are listed so you can download them yourself.")}</p>
                  {cfPack.mods.length > 0 && (
                    <div className="cf-mods">
                      {cfPack.mods.slice(0, 40).map((one) => (
                        <button
                          key={`${one.project}-${one.file}`}
                          className="cf-mod"
                          onClick={() => void openExternal(one.url)}
                        >
                          <Icon name="arrow" />
                          <span>{one.project}</span>
                          {!one.required && <em>{tr("Select")}</em>}
                        </button>
                      ))}
                      {cfPack.mods.length > 40 && (
                        <span className="muted">
                          {tr("and")}{" "}{cfPack.mods.length - 40}{tr(" — all listed in the text file")}</span>
                      )}
                    </div>
                  )}
                  <button
                    className="button primary"
                    onClick={() => void importCfPack()}
                  >
                    {tr("Import settings")}</button>
                </>
              )}
              {mrpackEntries.length > 0 && (
                <>
                  <div className="mrpack-summary">
                    <span>{tr("Total")}{" "}{mrpackEntries.length}{tr(" files")}</span>
                    <span>
                      {tr("Server")}{" "}
                      {
                        mrpackEntries.filter((entry) =>
                          ["Server", "Server only"].includes(entry.compatibility),
                        ).length
                      }
                      {tr("suffix.count")}</span>
                    <span>
                      {tr("Client only")}{" "}
                      {
                        mrpackEntries.filter(
                          (entry) => entry.compatibility === "Client only",
                        ).length
                      }
                      {tr("suffix.count")}</span>
                    <span>
                      {tr("Needs review")}{" "}
                      {
                        mrpackEntries.filter(
                          (entry) => entry.compatibility === "Needs review",
                        ).length
                      }
                      {tr("suffix.count")}</span>
                    <span>
                      {tr("No download URL")}{" "}
                      {
                        mrpackEntries.filter((entry) => entry.downloadCount === 0)
                          .length
                      }
                      {tr("suffix.count")}</span>
                  </div>
                  <label className="mrpack-option">
                    <input
                      type="checkbox"
                      checked={includeClientOnlyMods}
                      onChange={(event) =>
                        setIncludeClientOnlyMods(event.target.checked)
                      }
                    />
                    {tr("Also install client-only mods on the server")}</label>
                  <div className="mrpack-preview">
                    {mrpackEntries.slice(0, 30).map((entry) => (
                      <p
                        key={entry.path}
                        className={
                          entry.compatibility === "Client only"
                            ? "client-only"
                            : ""
                        }
                      >
                        <code>{entry.path}</code>
                        <span>{trBack(entry.compatibility)}</span>
                      </p>
                    ))}
                    {mrpackEntries.length > 30 && (
                      <p className="muted">
                        {tr("…more")}{" "}{mrpackEntries.length - 30}{tr(" files")}</p>
                    )}
                  </div>
                  <button
                    className="button primary"
                    disabled={
                      selected?.status !== "stopped" ||
                      mrpackEntries.some((entry) => entry.downloadCount === 0)
                    }
                    onClick={() => void importMrpack()}
                  >
                    {tr("Add modpack")}</button>
                </>
              )}
            </section>
            <section className="addon-panel">
              <div>
                <strong>{tr("Export as .mrpack")}</strong>
                <p>{tr("Bundles installed mods into a single modpack file.")}</p>
              </div>
              <div className="addon-actions">
                <button
                  className="button secondary"
                  onClick={() => void exportMrpack()}
                >
                  <Icon name="archive" />
                  {tr("Export")}</button>
              </div>
            </section>
                </div>
              </>
            )}
            {modal === "import" && (
              <div className="import-flow">
                <h2>{tr("Import server")}</h2>
                <p className="modal-copy">{tr("Add a server you already have. Its files stay as they are.")}</p>
                <div className="import-choices">
                  <button type="button" className="import-choice" onClick={() => void pickFolderToImport()}>
                    <span className="choice-mark">
                      <Icon name="folder" />
                    </span>
                    <span>
                      <strong>{tr("Folder")}</strong>
                      <small>{tr("A server folder with server.jar or server.properties")}</small>
                    </span>
                    <Icon name="arrow" />
                  </button>
                  <button type="button" className="import-choice" onClick={() => void pickArchiveToImport()}>
                    <span className="choice-mark">
                      <Icon name="archive" />
                    </span>
                    <span>
                      <strong>{tr("Zip file")}</strong>
                      <small>{tr("A zip of the whole server folder")}</small>
                    </span>
                    <Icon name="arrow" />
                  </button>
                  {!onWeb && (
                    <button type="button" className={`import-choice ${remotePick ? "on" : ""}`} onClick={() => setRemotePick((now) => !now)}>
                      <span className="choice-mark">
                        <Icon name="plug" />
                      </span>
                      <span>
                        <strong>{tr("SSH server")}</strong>
                        <small>{tr("Manage a server on another computer where it is")}</small>
                      </span>
                      <Icon name={remotePick ? "chevron" : "arrow"} />
                    </button>
                  )}
                </div>
                {remotePick && !onWeb ? (
                  <RemoteImport
                    locals={instances.filter((one) => !one.remoteHost && !one.panelHost).map((one) => ({ id: one.id, name: one.name }))}
                    onError={(message) => setError(message)}
                    onDone={(name) => {
                      setRemotePick(false);
                      setModal("none");
                      void refresh();
                      setNotice(tr("Imported {0}.", [name]));
                    }}
                  />
                ) : (
                  <div className="import-where">
                    <span>{tr("Folder or zip:")}</span>
                    <SegmentedControl
                      value={importWhere}
                      onChange={setImportWhere}
                      options={[
                        { value: "keep", label: tr("Keep the original location") },
                        { value: "managed", label: tr("Copy into the app folder") },
                      ]}
                    />
                  </div>
                )}
              </div>
            )}
            {modal === "create" && createStep === "modpack" && (
              <div className="create-flow">
                <div className="create-flow-head">
                  <button className="button ghost small step-back" onClick={() => setCreateStep("core")}>
                    <Icon name="arrow" />
                    {tr("Back")}
                  </button>
                  <h2>{tr("Create from a modpack")}</h2>
                </div>
                <div className="create-pack-actions">
                  <form
                    className="search inline-search"
                    onSubmit={(event) => {
                      event.preventDefault();
                      void searchPacks();
                    }}
                  >
                    <Icon name="search" />
                    <input autoFocus value={packQuery} placeholder={tr("Search Modrinth modpacks")} onChange={(event) => setPackQuery(event.target.value)} />
                  </form>
                  <button className="button secondary" onClick={() => void createFromModpackFile()}>
                    <Icon name="import" />
                    {tr("From file")}
                  </button>
                </div>
                {packBusy ? (
                  <div className="pack-list">{[1, 2, 3, 4].map((k) => <div className="pack-row skeleton" key={k} />)}</div>
                ) : packHits.length ? (
                  <div className="pack-list">
                    {packHits.map((hit) => (
                      <button type="button" className="pack-row" key={hit.projectId} disabled={creating} onClick={() => void createFromModpack(hit)}>
                        {hit.iconUrl ? <img src={hit.iconUrl} alt="" loading="lazy" /> : <span className="pack-icon"><Icon name="archive" /></span>}
                        <span className="pack-text">
                          <strong>{hit.title}</strong>
                          <small>{hit.description}</small>
                        </span>
                        {hit.downloads != null && <span className="pack-count"><Icon name="import" />{compactCount(hit.downloads)}</span>}
                      </button>
                    ))}
                  </div>
                ) : (
                  <p className="muted tool-empty">{packSearched ? tr("No matching modpack found.") : tr("Type a pack name and press Enter.")}</p>
                )}
                <p className="consent-note">{tr("Pick a pack to create the server right away. Creating it means you accept the Minecraft EULA.")}</p>
              </div>
            )}
            {modal === "create" && createStep !== "modpack" && (() => {
              const isMinecraft = createGame === "Minecraft";
              const isCustom = createGame === customProgram.id;
              const hasVersionStep = isMinecraft && !!form.core && has(form.core, "versions");
              const pages: { id: "game" | "core" | "finish" | "confirm"; label: string }[] = [
                { id: "game", label: tr("Game") },
                ...(isMinecraft || !createGame ? [{ id: "core" as const, label: tr("Core") }] : []),
                ...(isCustom
                  ? [{ id: "finish" as const, label: tr(customProgram.id) }]
                  : [
                      ...((isMinecraft || !createGame) && (!form.core || hasVersionStep) ? [{ id: "finish" as const, label: tr("Version") }] : []),
                      { id: "confirm" as const, label: tr("Confirm") },
                    ]),
              ];
              const page = !isCustom && createPage === "finish" && !hasVersionStep && !(isMinecraft && !form.core) ? "confirm" : createPage;
              const at = Math.max(0, pages.findIndex((one) => one.id === page));
              const back = () => setCreatePage(pages[Math.max(0, at - 1)].id);
              const where = createWhere === "local" ? tr("Local") : sshHostViews.find((one) => one.id === createWhere)?.name ?? createWhere;
              const ready = !!form.core && !versionsBusy && (isProxy(form.core) || !!form.version);
              const game = !isMinecraft && !isCustom ? findGame(createGame) : undefined;
              return (
              <div className="create-flow create-wizard wb-create">
                <div className="wb-create-top">
                  <ol className="wizard-steps">
                    {pages.map((one, index) => (
                      <li key={one.id} className={index === at ? "now" : index < at ? "done" : ""}>
                        <button type="button" disabled={index >= at} aria-current={index === at ? "step" : undefined} onClick={() => setCreatePage(one.id)}>
                          <b>{index < at ? <Icon name="check" /> : index + 1}</b>
                          {one.label}
                        </button>
                      </li>
                    ))}
                  </ol>
                </div>
                <div className="wizard-body" key={page}>
                  <h1 className="wb-create-title">
                    {page === "game"
                      ? tr("Which game?")
                      : page === "core"
                        ? tr("Which server should we make?")
                        : page === "finish" && !isCustom
                          ? tr("Which version?")
                          : page === "finish"
                            ? tr(customProgram.id)
                            : tr("Create it like this?")}
                  </h1>
                  {page === "game" && (
                    <GamePicker value={createGame} onPick={pickGame} gamesLocked={onWeb} />
                  )}
                  {page === "core" && (
                    <>
                      {!onWeb && sshHostViews.length > 0 && (
                        <div className="wizard-where">
                          <span>{tr("Create on")}</span>
                          <SegmentedControl
                            value={createWhere}
                            onChange={setCreateWhere}
                            options={[{ value: "local", label: tr("Local") }, ...sshHostViews.map((one) => ({ value: one.id, label: one.name }))]}
                          />
                        </div>
                      )}
                      <div className="core-rows">
                        {cores.map((core) => (
                          <button
                            type="button"
                            key={core}
                            className={`core-row ${form.core === core ? "on" : ""}`}
                            aria-pressed={form.core === core}
                            onClick={() => {
                              void chooseCore(core);
                              setCreatePage("finish");
                            }}
                          >
                            <span className="core-row-mark"><CoreIcon core={core} /></span>
                            <strong className="core-row-name">{core}</strong>
                            {coreInfo[core]?.kind && <em>{coreInfo[core].kind}</em>}
                          </button>
                        ))}
                      </div>
                      <div className="core-more">
                        {createWhere === "local" && (
                          <button type="button" className="button ghost small" onClick={() => void createFromJar()}>
                            <Icon name="folder" />
                            {tr("Choose a file")}
                          </button>
                        )}
                        <button
                          type="button"
                          className="button ghost small"
                          onClick={() => {
                            setCreateStep("modpack");
                            if (!packHits.length) void searchPacks("");
                          }}
                        >
                          <Icon name="package" />
                          {tr("From a modpack")}
                        </button>
                      </div>
                    </>
                  )}
                  {page === "finish" && isCustom && <CustomCreate onCreate={createCustom} />}
                  {page === "finish" && !isCustom && (() => {
                    const needle = createVersionQuery.trim().toLowerCase();
                    const found = versions.filter((one) => !needle || one.toLowerCase().includes(needle));
                    const shown = createVersionMore || needle ? found : found.slice(0, 12);
                    return (
                      <div className="wb-version-step">
                        <div className="wb-version-tools">
                        <label className="search wb-search-field wb-version-search">
                          <Icon name="search" />
                          <input
                            value={createVersionQuery}
                            onChange={(event) => setCreateVersionQuery(event.target.value)}
                            placeholder={tr("Search versions")}
                          />
                        </label>
                        {form.core !== "Pumpkin" && (
                          <label className="wb-toggle">
                            <span>{tr("Snapshots")}</span>
                            <span className="switch">
                              <input
                                type="checkbox"
                                checked={createSnapshots}
                                onChange={(event) => {
                                  const on = event.target.checked;
                                  setCreateSnapshots(on);
                                  setVersionsBusy(true);
                                  void commands
                                    .listCoreVersions(form.core, on)
                                    .then((list) => setVersions(list.length ? list : versions))
                                    .catch(() => {})
                                    .finally(() => setVersionsBusy(false));
                                }}
                              />
                              <span />
                            </span>
                          </label>
                        )}
                        </div>
                        <div className="wb-version-list" role="radiogroup" aria-label={tr("Version")}>
                          {versionsBusy && <p className="muted">{tr("Loading…")}</p>}
                          {!versionsBusy && shown.map((version) => (
                            <button
                              type="button"
                              role="radio"
                              key={version}
                              aria-checked={form.version === version}
                              className={form.version === version ? "on" : ""}
                              onClick={() => {
                                if (version !== form.version) {
                                  setForm({ ...form, version, coreBuild: "" });
                                  void loadCoreBuilds(form.core, version);
                                }
                                setCreatePage("confirm");
                              }}
                            >
                              <span className="wb-radio" aria-hidden />
                              <strong>{version}</strong>
                              {version === versions[0] && <span className="chip subtle">{tr("Latest")}</span>}
                            </button>
                          ))}
                          {!versionsBusy && found.length === 0 && <p className="muted">{tr("No matching versions.")}</p>}
                          {!versionsBusy && !needle && !createVersionMore && found.length > 12 && (
                            <button type="button" className="wb-version-more" onClick={() => setCreateVersionMore(true)}>
                              {tr("Show more")} · {found.length - 12}
                            </button>
                          )}
                        </div>
                      </div>
                    );
                  })()}
                  {page === "confirm" && (
                    <div className="wizard-finish">
                      <div className="wizard-pick">
                        <span className="wizard-pick-mark"><CoreIcon core={form.core || createGame} /></span>
                        <span>
                          <strong>{form.core || createGame}</strong>
                          {isMinecraft && <small>{`Minecraft · ${where}`}</small>}
                        </span>
                        <button type="button" className="button ghost small" onClick={back}>
                          {tr("Change")}
                        </button>
                      </div>
                      <dl className="wb-summary">
                        <div><dt>{tr("Name")}</dt><dd>{suggestedName}</dd></div>
                        {isMinecraft && form.version && (
                          <div>
                            <dt>{form.core === "Pumpkin" ? tr("Release") : tr("Minecraft version")}</dt>
                            <dd>{form.version}{hasVersionStep && <button type="button" className="button ghost small" onClick={() => setCreatePage("finish")}>{tr("Change")}</button>}</dd>
                          </div>
                        )}
                        {(buildsBusy || coreBuilds.length > 0) && (
                          <div>
                            <dt>{form.core === "Fabric" ? tr("Loader version") : tr("Core build")}</dt>
                            <dd className="wb-summary-pick">
                              <Dropdown
                                value={form.coreBuild}
                                disabled={buildsBusy}
                                placeholder={buildsBusy ? tr("Loading…") : tr("Choose")}
                                search={coreBuilds.length > 12 ? tr("Search builds") : undefined}
                                options={coreBuilds.map((build) => ({ value: build.id, label: build.label, hint: build.channel || undefined }))}
                                onChange={(coreBuild) => setForm({ ...form, coreBuild })}
                              />
                            </dd>
                          </div>
                        )}
                        {isMinecraft && <div><dt>{tr("Create on")}</dt><dd>{where}</dd></div>}
                        <div><dt>{tr("Memory")}</dt><dd>{bytes(form.maxMemoryMib)}</dd></div>
                      </dl>
                      {isMinecraft && form.core && (kindOf(form.core).sizeBytes > 0 || (createWhere === "local" && createFree !== null)) && (
                        <div className="game-facts">
                          {kindOf(form.core).sizeBytes > 0 && (
                            <div>
                              <span>{tr("Install size")}</span>
                              <strong>{tr("About {0}", [roughSize(kindOf(form.core).sizeBytes)])}</strong>
                            </div>
                          )}
                          {createWhere === "local" && createFree !== null && (
                            <div className={createFree < kindOf(form.core).sizeBytes * 1.2 ? "warn" : ""}>
                              <span>{tr("Free space")}</span>
                              <strong>{fileSize(createFree)}</strong>
                            </div>
                          )}
                        </div>
                      )}
                      {game && <GameFacts game={game} freeBytes={createFree} />}
                      {form.core === "Pumpkin" && (
                        <p className="create-warn">
                          <Icon name="alert" />
                          <span>{tr("This server is still in development. Don't use it with a world you care about.")}</span>
                        </p>
                      )}
                    </div>
                  )}
                </div>
                <div className="wizard-foot">
                  {at > 0 ? (
                    <button type="button" className="button ghost wizard-back" onClick={back}>
                      <Icon name="arrow" />
                      {tr("Back")}
                    </button>
                  ) : (
                    <span />
                  )}
                  {page === "confirm" && (
                    <div className="wizard-go">
                      <span className="consent-note">{isGame(form.core) ? tr("Creating a server means you accept the game's server terms. Large games can take a while to download.") : isProxy(form.core) ? tr("Creating a server means you accept its terms.") : tr("Creating a server means you accept the Minecraft EULA.")}</span>
                      <button className="button primary wb-pill" disabled={creating || !ready} onClick={() => void create()}>
                        <Icon name="plus" />
                        {tr("Create")}
                      </button>
                    </div>
                  )}
                </div>
              </div>
              );
            })()}
          </section>
        </div>
      )}
      {contextMenu && (
        <ContextMenu x={contextMenu.x} y={contextMenu.y} items={contextMenu.items} onClose={closeContextMenu} />
      )}
      {LangEditor && !onWeb && (
        <Suspense fallback={null}>
          <LangEditor />
        </Suspense>
      )}
      {paletteOpen && (
        <CommandPalette
          query={paletteQuery}
          activeIndex={paletteIndex}
          matches={paletteMatches}
          onQuery={(value) => {
            setPaletteQuery(value);
            setPaletteIndex(0);
          }}
          onIndex={setPaletteIndex}
          onPick={(item) => {
            setPaletteOpen(false);
            item.run();
          }}
          onClose={() => setPaletteOpen(false)}
        />
      )}

      {closeBlockedBy.length > 0 && (
        <div
          className="modal-backdrop ask-backdrop"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) setCloseBlockedBy([]);
          }}
        >
          <section className="ask-dialog close-guard-dialog" role="alertdialog" aria-modal="true">
            <div className="close-guard-icon"><Icon name="server" /></div>
            <h2>{tr("Stop your servers before closing the app.")}</h2>
            <p className="modal-copy">
              {tr("These servers are still running. You can close the app after they shut down safely.")}
            </p>
            <div className="close-server-list" aria-label={tr("Running servers")}>
              {closeBlockedBy.map((instance) => {
                const status = statusInfo(shownStatus(instance));
                return (
                  <div className="close-server-row" key={instance.id}>
                    <span className={`status-dot status-${shownStatus(instance)}`} />
                    <strong>{instance.name}</strong>
                    <small>{status.label}</small>
                  </div>
                );
              })}
            </div>
            <div className="modal-actions">
              <button className="button primary" onClick={() => setCloseBlockedBy([])}>
                {tr("OK")}
              </button>
            </div>
          </section>
        </div>
      )}
      {sessionOpen && !onWeb && (
        <div className="modal-backdrop as-panel session-page">
          <section className="add-modal modal-session" role="dialog" aria-modal="true">
            <button className="close session-close" data-tip={tr("Close")} aria-label={tr("Close")} onClick={() => setSessionOpen(null)}>
              <Icon name="close" />
            </button>
            <div className="session-wrap">
              <h1 className="wb-create-title">{sessionOpen.adding ? tr("New session") : tr("Manage sessions")}</h1>
              <div className="session-kinds" role="tablist">
                {([
                  { value: "ssh", icon: "plug", label: "SSH", hint: tr("Another computer over SSH") },
                  { value: "panel", icon: "cloud", label: tr("Panel"), hint: "Pterodactyl · Pelican · Hydrodactyl" },
                ] as const).map((one) => (
                  <button
                    key={one.value}
                    type="button"
                    role="tab"
                    aria-selected={sessionOpen.tab === one.value}
                    className={sessionOpen.tab === one.value ? "on" : ""}
                    onClick={() => setSessionOpen({ ...sessionOpen, tab: one.value })}
                  >
                    <span className="session-kind-mark"><Icon name={one.icon} /></span>
                    <span className="session-kind-text">
                      <strong>{one.label}</strong>
                      <small>{one.hint}</small>
                    </span>
                  </button>
                ))}
              </div>
              <div className="ssh-dialog-body session-body" key={sessionOpen.tab}>
                {sessionOpen.tab === "ssh" ? (
                  <SshHosts
                    startAdding={sessionOpen.adding}
                    confirm={confirmAction}
                    onError={(message) => setError(message)}
                    onNotice={(message) => setNotice(message)}
                    onImported={() => void refresh()}
                    onChanged={reloadSshHosts}
                  />
                ) : (
                  <PanelHosts
                    startAdding={sessionOpen.adding}
                    confirm={confirmAction}
                    onError={(message) => setError(message)}
                    onNotice={(message) => setNotice(message)}
                    onChanged={reloadPanelHosts}
                  />
                )}
              </div>
            </div>
          </section>
        </div>
      )}
      {trashList && selected && (
        <div className="modal-backdrop roomy" onClick={() => setTrashList(null)} role="presentation">
          <div className="add-modal trash-dialog" onClick={(event) => event.stopPropagation()}>
            <header>
              <h3>{tr("Trash")}</h3>
              <button className="close" aria-label={tr("Close")} onClick={() => setTrashList(null)}>
                <Icon name="close" />
              </button>
            </header>
            <p className="muted trash-note">
              {selected.remoteHost || selected.panelHost
                ? tr("Files deleted from SSH or panel servers are kept in the server folder (.monolab-trash). You can restore them or delete them permanently.")
                : tr("Files deleted from the Files tab are kept here instead of being removed immediately. You can restore them or delete them permanently.")}
            </p>
            {trashBusy && !trashList.length ? (
              <div className="skeleton-row" />
            ) : trashList.length === 0 ? (
              <EmptyState icon="trash" title={tr("The trash is empty.")} hint={tr("Files deleted from the Files tab are kept here.")} />
            ) : (
              <div className="trash-files">
                {trashList.map((one) => (
                  <div className="trash-file" key={one.stored}>
                    <Icon name={one.directory ? "folder" : "file"} />
                    <div className="trash-what">
                      <strong>{one.name}</strong>
                      <small data-tip={one.at ? date(one.at) : undefined}>
                        {one.cameFrom}
                        {one.directory ? "" : ' · ' + fileSize(one.sizeBytes)}
                        {one.at ? ' · ' + relativeTime(one.at) : ""}
                      </small>
                    </div>
                    <button
                      className="button ghost small"
                      onClick={() => {
                        void (async () => {
                          const back = await call(
                            () => commands.restoreServerTrash(selected.id, one.stored),
                            tr("Restored to {0}.", [one.cameFrom]),
                          );
                          if (back !== null) {
                            await reloadTrash();
                            await browseTo(browsePath);
                          }
                        })();
                      }}
                    >
                      <Icon name="undo" />
                      {tr("Restore")}
                    </button>
                    <button
                      className="icon-button danger"
                      aria-label={tr("Delete permanently")}
                      data-tip={tr("Delete permanently")}
                      onClick={() => {
                        void (async () => {
                          if (!(await confirmAction(
                            tr("Permanently delete {0}?", [one.name]),
                            tr("Also deletes it from the trash. This can't be undone."),
                            tr("Remove"),
                            true,
                          ))) return;
                          const gone = await call(
                            () => commands.deleteServerTrash(selected.id, one.stored),
                            tr("Deleted {0}.", [one.name]),
                          );
                          if (gone !== null) await reloadTrash();
                        })();
                      }}
                    >
                      <Icon name="trash" />
                    </button>
                  </div>
                ))}
              </div>
            )}
            {trashList.length > 0 && (
              <footer className="trash-foot">
                <span className="muted">
                  {tr("{0} items · {1} in total", [
                    trashList.length,
                    fileSize(trashList.reduce((sum, one) => sum + one.sizeBytes, 0)),
                  ])}
                </span>
                <button
                  className="button danger"
                  onClick={() => {
                    void (async () => {
                      if (!(await confirmAction(
                        tr("Empty the trash?"),
                        tr("Permanently deletes {0} items. This can't be undone.", [trashList.length]),
                        tr("Empty"),
                        true,
                      ))) return;
                      const count = await call(
                        () => commands.emptyServerTrash(selected.id),
                        tr("Emptied the trash."),
                      );
                      if (count !== null) await reloadTrash();
                    })();
                  }}
                >
                  <Icon name="trash" />
                  {tr("Empty")}
                </button>
              </footer>
            )}
          </div>
        </div>
      )}
      {ask && <AskDialog request={ask} onClose={() => setAsk(null)} />}
      {shotView && (() => {
        const shot = shotView.list[shotView.at];
        const go = (step: number) =>
          setShotView((now) =>
            now ? { ...now, at: (now.at + step + now.list.length) % now.list.length } : now,
          );
        return (
          <div
            className="modal-backdrop shot-back"
            onClick={() => setShotView(null)}
            onKeyDown={(event) => {
              if (event.key === "ArrowLeft") go(-1);
              if (event.key === "ArrowRight") go(1);
            }}
            role="presentation"
          >
            <div className="shot-view" onClick={(event) => event.stopPropagation()}>
              <img src={shot.url} alt={shot.title ?? ""} />
              {(shot.title || shot.description) && (
                <figcaption className="shot-words">
                  {shot.title && <strong>{shot.title}</strong>}
                  {shot.description && <span>{shot.description}</span>}
                </figcaption>
              )}
              {shotView.list.length > 1 && (
                <>
                  <IconButton
                    className="shot-step before"
                    icon="arrow"
                    label={tr("Previous image")}
                    onClick={() => go(-1)}
                  />
                  <IconButton
                    className="shot-step after"
                    icon="arrow"
                    label={tr("Next image")}
                    onClick={() => go(1)}
                  />
                  <span className="shot-count">{shotView.at + 1} / {shotView.list.length}</span>
                </>
              )}
              <IconButton
                className="close"
                icon="close"
                label={tr("Close")}
                onClick={() => setShotView(null)}
              />
            </div>
          </div>
        );
      })()}
      <div className="toast-stack">
        {jobs.map((job) => {
          const percent = job.total ? Math.min(100, Math.round(((job.sent ?? 0) / job.total) * 100)) : null;
          return (
            <div className="task-toast" key={job.id}>
              <div className="task-head">
                <span className="core-mark small neutral">
                  <Icon name={job.icon} />
                </span>
                <div>
                  <strong>{job.title}</strong>
                  {(job.detail || job.total) && (
                    <small>
                      {job.detail}
                      {job.total ? `${job.detail ? " · " : ""}${fileSize(job.sent ?? 0)} / ${fileSize(job.total)}` : ""}
                    </small>
                  )}
                </div>
              </div>
              <div className={`task-bar ${percent == null ? "waiting" : ""}`}>
                <i style={percent == null ? undefined : { width: `${percent}%` }} />
              </div>
              {percent != null && <span className="task-percent">{percent}%</span>}
            </div>
          );
        })}
        {creating && (
          <div className="task-toast">
            <div className="task-head">
              <span
                className="core-mark small"
              >
                <CoreIcon core={download?.core || form.core} />
              </span>
              <div>
                <strong>{packCreating ?? suggestedName} {tr("Creating")}</strong>
                <small>
                  {packCreating
                    ? download?.core
                      ? `${download.core} ${download.version} · `
                      : ""
                    : `${form.core} ${isProxy(form.core) ? tr("Latest stable") : form.version} · `}
                  {download?.stage === "pack"
                    ? tr("Downloading modpack")
                    : download?.stage === "mods"
                      ? tr("Downloading mods {0} / {1}", [download.received ?? 0, download.total ?? 0])
                      : download?.stage === "resolving"
                        ? tr("Checking official downloads")
                        : download?.stage === "cancelled"
                          ? tr("Cancelling")
                          : download?.stage === "installing"
                            ? tr("Running the installer (takes a few minutes)")
                            : tr("Downloading server files")}
                </small>
              </div>
              <button
                className="task-cancel"
                onClick={() => void cancelCreate()}
              >
                {tr("Cancel")}</button>
            </div>
            <div className={`task-bar ${percent == null ? "waiting" : ""} ${percent === 100 ? "full" : ""}`}>
              <i style={percent == null ? undefined : { width: `${percent}%` }} />
            </div>
            <span className="task-percent">
              {download?.stage === "installing"
                ? tr("Installing")
                : percent == null
                  ? tr("Preparing")
                  : `${percent}%`}
            </span>
          </div>
        )}
        {sshUpload && (
          <div className="task-toast">
            <div className="task-head">
              <span className="core-mark small neutral">
                <Icon name="plug" />
              </span>
              <div>
                <strong>
                  {sshUpload.kind === "download"
                    ? tr("Downloading {0}", [sshUpload.name])
                    : sshUpload.kind === "backup"
                      ? tr("Backing up {0}", [sshUpload.name])
                      : sshUpload.kind === "upgrade"
                        ? tr("Changing the core for {0}", [sshUpload.name])
                        : sshUpload.stage === "files"
                          ? tr("Adding folder {0}", [sshUpload.name])
                          : tr("Uploading {0} to the SSH server", [sshUpload.name])}
                </strong>
                <small>
                  {sshUpload.stage === "packing"
                    ? tr("Compressing on this computer")
                    : sshUpload.stage === "reading"
                      ? tr("Reading the archive")
                      : sshUpload.stage === "unpacking"
                        ? tr("Extracting on the SSH server")
                        : sshUpload.stage === "taring"
                          ? tr("Compressing on the SSH server")
                          : sshUpload.stage === "fetching"
                            ? tr("Downloading from the provider")
                            : sshUpload.stage === "swapping"
                              ? tr("Replacing the core file")
                              : sshUpload.stage === "files"
                                ?
                                  tr("{0} / {1} files", [sshUpload.sent, sshUpload.total])
                                : `${fileSize(sshUpload.sent)} / ${fileSize(sshUpload.total)}`}
                </small>
              </div>
            </div>
            <div
              className={`task-bar ${sshUpload.stage === "uploading" || sshUpload.stage === "downloading" || sshUpload.stage === "files" ? "" : "waiting"} ${sshUpload.stage === "unpacking" ? "full" : ""}`}
            >
              <i
                style={
                  (sshUpload.stage === "uploading" ||
                    sshUpload.stage === "downloading" ||
                    sshUpload.stage === "files") &&
                  sshUpload.total > 0
                    ? { width: `${Math.round((sshUpload.sent / sshUpload.total) * 100)}%` }
                    : undefined
                }
              />
            </div>
            {(sshUpload.stage === "uploading" || sshUpload.stage === "files") && sshUpload.total > 0 && (
              <span className="task-percent">
                {Math.round((sshUpload.sent / sshUpload.total) * 100)}%
              </span>
            )}
          </div>
        )}
        {importing && (
          <div className="task-toast">
            <div className="task-head">
              <span className="core-mark small neutral">
                <Icon name="archive" />
              </span>
              <div>
                <strong>
                  {archive?.stage === "copying"
                    ? tr("Copying the server to the app folder")
                    : tr("Importing from archive")}
                </strong>
                <small>
                  {archive?.total
                    ? tr("{0} / {1, plural, one {# file} other {# files}}", [archive.done ?? 0, archive.total])
                    : archive?.stage === "copying"
                      ? tr("Counting files")
                      : tr("Extracting archive")}
                </small>
              </div>
            </div>
            <div
              className={`task-bar ${archive?.total ? "" : "waiting"} ${archive?.total && (archive.done ?? 0) >= archive.total ? "full" : ""}`}
            >
              <i
                style={
                  archive?.total
                    ? {
                        width: `${Math.round(((archive.done ?? 0) / archive.total) * 100)}%`,
                      }
                    : undefined
                }
              />
            </div>
          </div>
        )}
        {(updateState.stage === "found" ||
          updateState.stage === "downloading" ||
          updateState.stage === "stopping" ||
          updateState.stage === "ready") && (
          <div className="task-toast update-toast">
            <div className="task-head">
              <span className="core-mark small">
                <Icon name="refresh" />
              </span>
              <div>
                <strong>{tr("New version")}{" "}{updateState.version} {tr("is available")}</strong>
                <small>
                  {updateState.stage === "downloading"
                    ? tr("Downloading… {0}%", [updateState.percent ?? 0])
                    : updateState.stage === "stopping"
                      ? tr("Stopping running servers…")
                      : updateState.stage === "ready"
                        ? tr("Download complete. Restarting the app.")
                        : tr("Click to update now.")}
                </small>
              </div>
              {updateState.stage === "found" && (
                <button
                  className="task-cancel"
                  data-tip={tr("Later")}
                  onClick={() => { snoozedUpdate.current = updateState.version ?? null; setUpdateState({ stage: "none" }); }}
                >
                  {tr("Later")}</button>
              )}
            </div>
            {updateState.stage === "found" ? (
              <div className="update-actions">
                <button className="button secondary" onClick={() => void openExternal(CHANGELOG_URL)}>
                  {tr("What's new")}
                </button>
                <button
                  className="button primary update-now"
                  onClick={() => void installUpdate()}
                >
                  {tr("Update now")}</button>
              </div>
            ) : (
              <div
                className={`task-bar ${updateState.stage === "ready" ? "" : "waiting"} ${updateState.percent === 100 ? "full" : ""}`}
              >
                <i
                  style={
                    updateState.percent != null
                      ? { width: `${updateState.percent}%` }
                      : undefined
                  }
                />
              </div>
            )}
          </div>
        )}
        {notice && (
          <button
            key={noticeKey}
            className={`toast ${noticeLeaving ? "leaving" : ""} ${noticePending ? "pending" : ""}`}
            onClick={() => dismissNotice()}
          >
            {noticePending ? (
              <span className="toast-spin"><Icon name="refresh" /></span>
            ) : (
              <Icon name="check" />
            )}
            <span className="toast-text">{trBack(noticeMorph ?? notice)}</span>
          </button>
        )}
        {error && (
          <div className={`toast error ${errorLeaving ? "leaving" : ""}`} role="alert">
            <Icon name="alert" />
            <span>{trBack(error)}</span>
            <button className="toast-close" aria-label={tr("Close")} data-tip={tr("Close")} onClick={() => dismissError()}>
              <Icon name="close" />
            </button>
          </div>
        )}
      </div>
    </main>
  );
}

export default App;

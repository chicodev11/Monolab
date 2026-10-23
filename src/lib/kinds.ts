import table from "../../src-tauri/src/kinds.json";
import { tr } from "./i18n";

export type AddonKind = "mod" | "plugin" | "wasm";
export type Feature = "java" | "versions" | "settings" | "worlds" | "players" | "ping" | "remote" | "modpacks" | "pregen";

export type Kind = {
  id: string;
  family: "minecraft" | "game" | "custom";
  create: boolean;
  addons: AddonKind | null;
  proxy: boolean;
  /** Rough size of a fresh server before any world, in bytes. */
  sizeBytes: number;
  features: Set<Feature>;
};

export type GameInfo = {
  id: string;
  icon: string;
  logo: string;
  source: "steam" | "official";
  port: number;
  sizeBytes: number;
};

const minecraftFeatures: Feature[] = ["settings", "worlds", "players", "ping", "remote", "modpacks"];

function minecraftKind(entry: {
  id: string;
  create: boolean;
  java: boolean;
  versions: boolean;
  addons: string | null;
  proxy: boolean;
  pregen: boolean;
  sizeBytes?: number;
}): Kind {
  const features = new Set<Feature>(minecraftFeatures);
  if (entry.java) features.add("java");
  if (entry.versions) features.add("versions");
  if (entry.pregen) features.add("pregen");
  return { id: entry.id, family: "minecraft", create: entry.create, addons: entry.addons as AddonKind | null, proxy: entry.proxy, sizeBytes: entry.sizeBytes ?? 0, features };
}

export const gameInfo: GameInfo[] = table.games.map((game) => ({
  id: game.id,
  icon: game.icon,
  logo: game.logo,
  source: typeof game.source === "object" ? "steam" : "official",
  port: game.port,
  sizeBytes: game.sizeBytes,
}));

const kinds = new Map<string, Kind>([
  ...table.minecraft.map((entry) => [entry.id, minecraftKind(entry)] as const),
  ...gameInfo.map(
    (game) => [game.id, { id: game.id, family: "game", create: true, addons: null, proxy: false, sizeBytes: game.sizeBytes, features: new Set<Feature>() }] as const,
  ),
]);

/** A program the user already has, run with their own launch settings. */
export const customProgram = table.custom;
kinds.set(customProgram.id, { id: customProgram.id, family: "custom", create: true, addons: null, proxy: false, sizeBytes: 0, features: new Set<Feature>() });

const unknownCore = minecraftKind({ id: "", create: false, java: true, versions: true, addons: null, proxy: false, pregen: true });

export const kindOf = (core: string): Kind => kinds.get(core) ?? unknownCore;

export const has = (core: string, feature: Feature) => kindOf(core).features.has(feature);

export const minecraftCores = table.minecraft.filter((entry) => entry.create).map((entry) => entry.id);

export const games = gameInfo.map((one) => one.id);

export const isGame = (core: string) => kindOf(core).family === "game";

export const isCustom = (core: string) => kindOf(core).family === "custom";

export const isProxy = (core: string) => kindOf(core).proxy;

export const findGame = (core: string) => gameInfo.find((one) => one.id === core);

export const gameOf = (core: string) => (kindOf(core).family === "minecraft" ? "Minecraft" : core);

/** Mod or plugin folder for the add-ons tab; null for cores without one (Vanilla, Pumpkin's own plugins, games). */
export const addonKind = (core: string): "mod" | "plugin" | null => {
  const kind = kindOf(core).addons;
  return kind === "mod" || kind === "plugin" ? kind : null;
};

export const coreLabel = (core: string, version: string) =>
  isCustom(core) ? tr(customProgram.id) : isGame(core) ? core : `${core} ${version}`;

export type Protocol = "tcp" | "udp" | "both";

/** What a game's port speaks and how many ports from it up. Minecraft is one TCP port. */
export const portsOf = (core: string): { protocol: Protocol; count: number } => {
  const game = table.games.find((one) => one.id === core) as { protocol?: string; portCount?: number } | undefined;
  const protocol = game?.protocol === "udp" || game?.protocol === "both" ? game.protocol : "tcp";
  return { protocol, count: Math.max(1, game?.portCount ?? 1) };
};

export const launchFile = "monolab-launch.txt";

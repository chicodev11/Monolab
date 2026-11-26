import table from "../../src-tauri/src/kinds.json";
import { tr } from "./i18n";

export type AddonKind = "mod" | "plugin" | "wasm";
export type Feature = "java" | "versions" | "settings" | "worlds" | "players" | "ping" | "remote" | "modpacks" | "pregen";

export type Kind = {
  id: string;
  create: boolean;
  addons: AddonKind | null;
  proxy: boolean;
  /** Rough size of a fresh server before any world, in bytes. */
  sizeBytes: number;
  features: Set<Feature>;
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
  return { id: entry.id, create: entry.create, addons: entry.addons as AddonKind | null, proxy: entry.proxy, sizeBytes: entry.sizeBytes ?? 0, features };
}

const kinds = new Map<string, Kind>(table.minecraft.map((entry) => [entry.id, minecraftKind(entry)] as const));

const unknownCore = minecraftKind({ id: "", create: false, java: true, versions: true, addons: null, proxy: false, pregen: true });

export const kindOf = (core: string): Kind => kinds.get(core) ?? unknownCore;

export const has = (core: string, feature: Feature) => kindOf(core).features.has(feature);

export const minecraftCores = table.minecraft.filter((entry) => entry.create).map((entry) => entry.id);

export const isProxy = (core: string) => kindOf(core).proxy;

/** Mod or plugin folder for the add-ons tab; null for cores without one (Vanilla, Pumpkin's own plugins). */
export const addonKind = (core: string): "mod" | "plugin" | null => {
  const kind = kindOf(core).addons;
  return kind === "mod" || kind === "plugin" ? kind : null;
};

/** The Pumpkin version that runs the PumpkinMC fork bundled with Monolab. */
export const PUMPKIN_FORK = "monolab-fork";

/** A version as people read it: the bundled fork gets a name instead of its tag. */
export const versionLabel = (version: string) => (version === PUMPKIN_FORK ? tr("PumpkinMC fork (built in)") : version);

export const coreLabel = (core: string, version: string) => `${core} ${versionLabel(version)}`;

export const launchFile = "monolab-launch.txt";

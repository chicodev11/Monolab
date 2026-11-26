import { createContext, useContext, useEffect, useState } from "react";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { DropdownMenu, Switch } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import type { StudioData } from "./StudioPreview";
import { NO_NAMES, type StudioNames } from "./studioKinds";

/** What the add-on can do in a command, menu slot, item use or rule. Matches StudioActions.ACTIONS. */
export const ACTION_TYPES = ["message", "broadcast", "title", "actionbar", "sound", "command", "teleport", "give", "menu", "close", "set", "wait", "dialog", "advancement", "loot", "animation", "damage", "heal", "potion", "launch", "particle", "projectile", "summon", "enchant", "cosmetic", "emote", "trade", "function", "quest", "structure", "shop", "money", "if", "repeat", "each_player", "cancel", "stop", "take", "xp", "gamemode", "kick", "block", "fill", "bossbar", "food", "health", "set_permission", "set_group", "manage_world", "region"] as const;
/** Game modes, as the game names them. */
export const GAMEMODES = ["survival", "creative", "adventure", "spectator"] as const;
/** What the projectile action can fire (`studio_format::mob::PROJECTILES`). */
export const PROJECTILES = ["arrow", "snowball", "fireball", "small_fireball", "wither_skull", "trident"] as const;
/** A few vanilla effects and particles to pick from; any other game name works too. */
export const EFFECTS = ["speed", "slowness", "haste", "mining_fatigue", "strength", "instant_health", "instant_damage", "jump_boost", "nausea", "regeneration", "resistance", "fire_resistance", "water_breathing", "invisibility", "blindness", "night_vision", "hunger", "weakness", "poison", "wither", "health_boost", "absorption", "glowing", "levitation", "slow_falling", "darkness"];
const PARTICLES = ["flame", "soul_fire_flame", "smoke", "large_smoke", "explosion", "heart", "angry_villager", "happy_villager", "crit", "enchanted_hit", "witch", "portal", "end_rod", "dragon_breath", "cloud", "snowflake", "totem_of_undying", "lava", "dripping_water", "note", "sweep_attack", "damage_indicator", "electric_spark", "sculk_soul"];
/** What a rule or menu slot can check. Matches StudioActions.CONDITIONS. */
export const CONDITION_TYPES = ["permission", "world", "variable", "chance", "online", "holding", "advancement", "quest", "money", "has_item", "gamemode", "health", "level", "sneaking", "time", "weather", "group"] as const;

export function actionLabel(type: string, expansions?: Expansion[]): string {
  switch (type) {
    case "message":
      return tr("Send a message");
    case "broadcast":
      return tr("Message everyone");
    case "title":
      return tr("Show a title");
    case "actionbar":
      return tr("Action bar text");
    case "sound":
      return tr("Play a sound");
    case "command":
      return tr("Run a command");
    case "teleport":
      return tr("Teleport");
    case "give":
      return tr("Give an item");
    case "menu":
      return tr("Open a menu");
    case "close":
      return tr("Close the menu");
    case "set":
      return tr("Change a variable");
    case "wait":
      return tr("Wait");
    case "dialog":
      return tr("Open a dialog");
    case "advancement":
      return tr("Grant an advancement");
    case "quest":
      return tr("Start or end a quest");
    case "structure":
      return tr("Place a structure");
    case "loot":
      return tr("Give loot");
    case "enchant":
      return tr("Enchant the held item");
    case "cosmetic":
      return tr("Wear a cosmetic");
    case "emote":
      return tr("Play an emote");
    case "trade":
      return tr("Open trades");
    case "shop":
      return tr("Open a shop");
    case "money":
      return tr("Give or take Vault money");
    case "function":
      return tr("Run function");
    case "animation":
      return tr("Play a model animation");
    case "damage":
      return tr("Hurt");
    case "heal":
      return tr("Heal");
    case "potion":
      return tr("Give a potion effect");
    case "launch":
      return tr("Launch");
    case "particle":
      return tr("Show particles");
    case "projectile":
      return tr("Shoot a projectile");
    case "summon":
      return tr("Summon a mob");
    case "if":
      return tr("If");
    case "repeat":
      return tr("Repeat");
    case "each_player":
      return tr("For each player");
    case "cancel":
      return tr("Cancel the event");
    case "stop":
      return tr("Stop here");
    case "take":
      return tr("Take an item");
    case "xp":
      return tr("Give or take experience");
    case "gamemode":
      return tr("Change game mode");
    case "kick":
      return tr("Kick the player");
    case "block":
      return tr("Set a block");
    case "fill":
      return tr("Fill an area");
    case "bossbar":
      return tr("Show a boss bar");
    case "food":
      return tr("Set hunger");
    case "health":
      return tr("Set health");
    case "set_permission":
      return tr("Give or take a permission");
    case "set_group":
      return tr("Change permission group");
    case "manage_world":
      return tr("Manage a world");
    case "region":
      return tr("Copy or paste an area");
    default:
      return expansionStep(expansions, "actions", type)?.label || type;
  }
}

export function conditionLabel(type: string, expansions?: Expansion[]): string {
  switch (type) {
    case "permission":
      return tr("Has a permission");
    case "world":
      return tr("Is in a world");
    case "variable":
      return tr("Variable is");
    case "chance":
      return tr("Random chance");
    case "online":
      return tr("Players online");
    case "holding":
      return tr("Holds an item");
    case "advancement":
      return tr("Has an advancement");
    case "quest":
      return tr("Quest state");
    case "money":
      return tr("Has Vault money");
    case "has_item":
      return tr("Has items");
    case "gamemode":
      return tr("Game mode is");
    case "health":
      return tr("Health is");
    case "level":
      return tr("Level is");
    case "sneaking":
      return tr("Is sneaking");
    case "time":
      return tr("Time of day");
    case "weather":
      return tr("Weather is");
    case "group":
      return tr("Is in a permission group");
    default:
      return expansionStep(expansions, "conditions", type)?.label || type;
  }
}

/** A short line saying what one action does, for lists and nodes. */
export function actionSummary(one: StudioData): string {
  const text = (key: string) => (typeof one[key] === "string" ? (one[key] as string) : "");
  const number = (key: string, fallback: number) => (typeof one[key] === "number" ? (one[key] as number) : fallback);
  switch (one.type) {
    case "message":
    case "broadcast":
    case "actionbar":
      return text("text");
    case "title":
      return [text("title"), text("subtitle")].filter(Boolean).join(" / ");
    case "sound":
      return text("sound");
    case "command":
      return `/${text("command").replace(/^\//, "")}${one.as === "player" ? ` (${tr("as the player")})` : ""}`;
    case "teleport":
      return `${text("world") || tr("same world")} ${number("x", 0)} ${number("y", 64)} ${number("z", 0)}`;
    case "give":
      return `${number("amount", 1)} × ${text("item")}`;
    case "menu":
      return text("menu");
    case "set":
      return `${text("variable")} ${one.op === "add" ? "+=" : one.op === "subtract" ? "-=" : "="} ${typeof one.value === "number" ? one.value : text("value")}`;
    case "wait":
      return tr("{0} ticks", [number("ticks", 20)]);
    case "dialog":
      return text("dialog");
    case "advancement":
      return one.revoke === true ? `${tr("take back")} ${text("advancement")}` : text("advancement");
    case "loot":
      return text("loot");
    case "enchant":
      return `${text("enchant")} ${number("level", 1)}`;
    case "cosmetic":
      return `${text("cosmetic")} · ${text("mode") || "toggle"}`;
    case "emote":
      return [text("model"), text("animation") || "emote"].filter(Boolean).join(" · ");
    case "trade":
      return text("trade");
    case "shop":
      return text("shop");
    case "money":
      return `${text("op") || "give"} ${number("amount", 0)}`;
    case "function":
      return text("function");
    case "quest":
      return `${text("quest")} · ${text("mode") || "start"}`;
    case "structure":
      return `${text("structure")} · ${one.at === "spot" ? `${text("world") || "world"} ${number("x", 0)} ${number("y", 64)} ${number("z", 0)}` : tr("at the player")}`;
    case "animation":
      return [text("model"), text("animation")].filter(Boolean).join(" · ");
    case "damage":
      return tr("{0} damage", [number("amount", 4)]);
    case "heal":
      return tr("{0} health", [number("amount", 4)]);
    case "potion":
      return `${text("effect")} ${number("level", 1)} · ${tr("{0} s", [number("seconds", 5)])}`;
    case "launch":
      return tr("up {0}, forward {1}", [number("up", 0.8), number("forward", 0)]);
    case "particle":
      return `${number("count", 10)} × ${text("particle")}`;
    case "projectile":
      return `${text("projectile") || "arrow"} · ${number("speed", 1.5)}`;
    case "summon":
      return `${number("count", 1)} × ${text("mob")}`;
    case "if":
      return listOf(one.if).map(conditionSummary).filter(Boolean).join(" · ");
    case "repeat":
      return tr("{0} times", [number("times", 3)]);
    case "each_player":
      return tr("Everyone online");
    case "cancel":
    case "stop":
      return "";
    case "take":
      return `${number("amount", 1)} × ${text("item")}`;
    case "xp":
      return `${text("op") || "give"} ${number("amount", 10)} ${one.levels === true ? tr("levels") : tr("points")}`;
    case "gamemode":
      return text("mode") || "survival";
    case "kick":
      return text("text");
    case "block":
      return `${text("block")} · ${spotText(one, "")}`;
    case "fill":
      return `${text("block")} · ${spotText(one, "1")} → ${spotText(one, "2")}`;
    case "bossbar":
      return one.op === "hide" ? tr("hide") : text("text");
    case "food":
    case "health":
      return String(number("amount", 20));
    case "set_permission":
      return `${text("op") || "give"} ${text("permission")}`;
    case "set_group":
      return `${text("op") || "add"} ${text("group")}`;
    case "manage_world":
      return `${text("op") || "time"} · ${text("world") || tr("player's world")}`;
    case "region":
      return `${text("op") || "copy"} ${text("name")}`;
    default:
      return rawSummary(one);
  }
}

export function conditionSummary(one: StudioData): string {
  const text = (key: string) => (typeof one[key] === "string" ? (one[key] as string) : "");
  const not = one.negate === true ? `${tr("not")} ` : "";
  switch (one.type) {
    case "permission":
      return not + text("permission");
    case "world":
      return not + (Array.isArray(one.worlds) ? (one.worlds as string[]).join(", ") : "");
    case "variable":
      return `${not}${one.scope === "server" ? "var" : "pvar"}:${text("variable")} ${text("op") || "="} ${typeof one.value === "number" ? one.value : text("value")}`;
    case "chance":
      return `${not}${typeof one.percent === "number" ? one.percent : 50}%`;
    case "online":
      return `${not}${text("op") || ">="} ${text("value") || "1"}`;
    case "holding":
      return not + (text("item") || tr("anything"));
    case "advancement":
      return not + text("advancement");
    case "quest":
      return `${not}${text("quest")} · ${text("state") || "active"}`;
    case "money":
      return `${not}>= ${typeof one.amount === "number" ? one.amount : 0}`;
    case "has_item":
      return `${not}${typeof one.amount === "number" ? one.amount : 1} × ${text("item")}`;
    case "gamemode":
      return not + (Array.isArray(one.modes) ? (one.modes as string[]).join(", ") : "");
    case "health":
    case "level":
      return `${not}${text("op") || ">="} ${typeof one.value === "number" ? one.value : 0}`;
    case "sneaking":
      return not.trim();
    case "time":
    case "weather":
      return not + (text("is") || (one.type === "time" ? "day" : "clear"));
    case "group":
      return not + text("group");
    default:
      return not + rawSummary(one);
  }
}

/** A new action of a type, with the fields it needs filled in. */
export function newAction(type: string): StudioData {
  switch (type) {
    case "message":
    case "broadcast":
    case "actionbar":
      return { type, text: "" };
    case "title":
      return { type, title: "", subtitle: "", stayTicks: 60 };
    case "sound":
      return { type, sound: "entity.experience_orb.pickup", volume: 1, pitch: 1 };
    case "command":
      return { type, command: "", as: "console" };
    case "teleport":
      return { type, world: "world", x: 0, y: 64, z: 0 };
    case "give":
      return { type, item: "diamond", amount: 1 };
    case "menu":
      return { type, menu: "" };
    case "set":
      return { type, variable: "", scope: "player", op: "set", value: "1" };
    case "wait":
      return { type, ticks: 20 };
    case "dialog":
      return { type, dialog: "" };
    case "advancement":
      return { type, advancement: "" };
    case "loot":
      return { type, loot: "" };
    case "enchant":
      return { type, enchant: "", level: 1 };
    case "cosmetic":
      return { type, cosmetic: "", mode: "toggle" };
    case "emote":
      return { type, model: "", animation: "emote" };
    case "trade":
      return { type, trade: "" };
    case "shop":
      return { type, shop: "" };
    case "money":
      return { type, op: "give", amount: 10 };
    case "function":
      return { type, function: "" };
    case "quest":
      return { type, quest: "", mode: "start" };
    case "structure":
      return { type, structure: "", at: "player", rotation: "" };
    case "animation":
      return { type, model: "", animation: "idle" };
    case "damage":
    case "heal":
      return { type, amount: 4 };
    case "potion":
      return { type, effect: "speed", seconds: 5, level: 1 };
    case "launch":
      return { type, up: 0.8, forward: 0 };
    case "particle":
      return { type, particle: "flame", count: 10 };
    case "projectile":
      return { type, projectile: "arrow", speed: 1.5 };
    case "summon":
      return { type, mob: "", count: 1 };
    case "if":
      return { type, if: [], then: [], else: [] };
    case "repeat":
      return { type, times: 3, actions: [] };
    case "each_player":
      return { type, actions: [] };
    case "take":
      return { type, item: "diamond", amount: 1 };
    case "xp":
      return { type, op: "give", amount: 10, levels: false };
    case "gamemode":
      return { type, mode: "survival" };
    case "kick":
      return { type, text: "" };
    case "block":
      return { type, block: "stone", world: "", x: "~", y: "~", z: "~" };
    case "fill":
      return { type, block: "air", world: "", x1: "~-2", y1: "~", z1: "~-2", x2: "~2", y2: "~2", z2: "~2" };
    case "bossbar":
      return { type, op: "show", text: "", color: "white", progress: 1, seconds: 10 };
    case "food":
      return { type, amount: 20 };
    case "health":
      return { type, amount: 20 };
    case "set_permission":
      return { type, op: "give", permission: "", seconds: 0 };
    case "set_group":
      return { type, op: "add", group: "" };
    case "manage_world":
      return { type, op: "time", world: "", value: "day" };
    case "region":
      return { type, op: "copy", name: "", world: "", x1: "~-2", y1: "~", z1: "~-2", x2: "~2", y2: "~2", z2: "~2" };
    default:
      return { type };
  }
}

/** A spot's x y z as written (`~` = the player's own), for summaries. */
function spotText(one: StudioData, suffix: string): string {
  const part = (axis: string) => {
    const value = one[`${axis}${suffix}`];
    return typeof value === "number" ? String(value) : typeof value === "string" && value ? value : "~";
  };
  return `${part("x")} ${part("y")} ${part("z")}`;
}

export function newCondition(type: string): StudioData {
  switch (type) {
    case "permission":
      return { type, permission: "" };
    case "world":
      return { type, worlds: ["world"] };
    case "variable":
      return { type, variable: "", scope: "player", op: "=", value: "1" };
    case "chance":
      return { type, percent: 50 };
    case "online":
      return { type, op: ">=", value: "1" };
    case "holding":
      return { type, item: "" };
    case "advancement":
      return { type, advancement: "" };
    case "quest":
      return { type, quest: "", state: "active" };
    case "money":
      return { type, amount: 10 };
    case "has_item":
      return { type, item: "diamond", amount: 1 };
    case "gamemode":
      return { type, modes: ["survival"] };
    case "health":
      return { type, op: ">=", value: 10 };
    case "level":
      return { type, op: ">=", value: 10 };
    case "time":
      return { type, is: "day" };
    case "weather":
      return { type, is: "clear" };
    case "group":
      return { type, group: "" };
    default:
      return { type };
  }
}

export function listOf(value: unknown): StudioData[] {
  return Array.isArray(value) ? value.filter((one): one is StudioData => !!one && typeof one === "object" && !Array.isArray(one)) : [];
}


/* Studio maker M6: plugins add their own actions, conditions, triggers and placeholders. The add-on runs them
   (MonolabApi.studioAction…); a plugin's plugins/<folder>/monolab-studio.json tells the app how to draw their forms. */

export type ExpansionFieldType = "text" | "number" | "bool" | "choice" | "player";
export type ExpansionField = { key: string; label: string; type: ExpansionFieldType; default?: unknown; choices: string[] };
export type ExpansionStep = { type: string; label: string; fields: ExpansionField[] };
export type ExpansionTrigger = { event: string; label: string; vars: string[] };
export type ExpansionPlaceholder = { key: string; label: string };
/** One plugin's monolab-studio.json, as `studio_expansions` reads it. */
export type Expansion = { folder: string; owner: string; actions: ExpansionStep[]; conditions: ExpansionStep[]; triggers: ExpansionTrigger[]; placeholders: ExpansionPlaceholder[]; problems: string[] };

const FIELD_TYPES: readonly string[] = ["text", "number", "bool", "choice", "player"];
const NAMESPACED = /^[a-z0-9_-]+:[a-z0-9_-]+$/;

const recordOf = (value: unknown): Record<string, unknown> => (value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {});
const arrayOf = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);
const stringOf = (value: unknown): string => (typeof value === "string" ? value : typeof value === "number" || typeof value === "boolean" ? String(value) : "");

function fieldOf(value: unknown): ExpansionField | null {
  const one = recordOf(value);
  const key = stringOf(one.key).trim();
  if (!key || key === "type" || key === "negate") return null;
  const type = FIELD_TYPES.includes(stringOf(one.type)) ? (stringOf(one.type) as ExpansionFieldType) : "text";
  return { key, label: stringOf(one.label) || key, type, default: one.default, choices: arrayOf(one.choices).map(stringOf).filter(Boolean) };
}

function stepOf(value: unknown): ExpansionStep | null {
  const one = recordOf(value);
  const type = stringOf(one.type).trim();
  if (!NAMESPACED.test(type)) return null;
  return { type, label: stringOf(one.label) || type, fields: arrayOf(one.fields).map(fieldOf).filter((field): field is ExpansionField => field !== null) };
}

/** Reads what `studio_expansions` returned, whatever its exact shape, keeping only what can be drawn. */
export function expansionsOf(value: unknown): Expansion[] {
  return arrayOf(value).map((raw) => {
    const one = recordOf(raw);
    const folder = stringOf(one.folder);
    return {
      folder,
      owner: stringOf(one.owner) || folder,
      actions: arrayOf(one.actions).map(stepOf).filter((step): step is ExpansionStep => step !== null),
      conditions: arrayOf(one.conditions).map(stepOf).filter((step): step is ExpansionStep => step !== null),
      triggers: arrayOf(one.triggers)
        .map((item) => {
          const trigger = recordOf(item);
          const event = stringOf(trigger.event).trim();
          return { event, label: stringOf(trigger.label) || event, vars: arrayOf(trigger.vars).map(stringOf).filter(Boolean) };
        })
        .filter((trigger) => NAMESPACED.test(trigger.event)),
      placeholders: arrayOf(one.placeholders)
        .map((item) => {
          const placeholder = recordOf(item);
          const key = stringOf(placeholder.key).trim();
          return { key, label: stringOf(placeholder.label) || key };
        })
        .filter((placeholder) => NAMESPACED.test(placeholder.key)),
      problems: arrayOf(one.problems).map(stringOf).filter(Boolean),
    };
  });
}

// One read per server, shared by every step list on the page, read again when a little old (plugins come and go).
const expansionReads = new Map<string, { at: number; list: Promise<Expansion[]> }>();

function readExpansions(serverId: string): Promise<Expansion[]> {
  const known = expansionReads.get(serverId);
  if (known && Date.now() - known.at < 10000) return known.list;
  const list = (commands.studioExpansions(serverId) as Promise<unknown>).then(expansionsOf, () => [] as Expansion[]);
  expansionReads.set(serverId, { at: Date.now(), list });
  return list;
}

/** The plugins on this server that describe studio additions (`plugins/<folder>/monolab-studio.json`). */
export function useStudioExpansions(serverId: string | undefined): Expansion[] {
  const [list, setList] = useState<Expansion[]>([]);
  useEffect(() => {
    setList([]);
    if (!serverId) return;
    let live = true;
    void readExpansions(serverId).then((next) => {
      if (live) setList(next);
    });
    return () => {
      live = false;
    };
  }, [serverId]);
  return list;
}

/** What plugins add on the open server; step lists and step fields read it. Provide it once around the form. */
export const StudioExpansions = createContext<Expansion[]>([]);

/** A plugin's action or condition by its "plugin:name" type. */
export function expansionStep(expansions: Expansion[] | undefined, side: "actions" | "conditions", type: string): ExpansionStep | undefined {
  if (!expansions || !type.includes(":")) return undefined;
  for (const one of expansions) {
    const found = one[side].find((step) => step.type === type);
    if (found) return found;
  }
  return undefined;
}

/** A plugin's rule trigger by its "plugin:name" event, with who adds it. */
export function expansionTrigger(expansions: Expansion[], event: string): (ExpansionTrigger & { owner: string }) | undefined {
  for (const one of expansions) {
    const found = one.triggers.find((trigger) => trigger.event === event);
    if (found) return { ...found, owner: one.owner };
  }
  return undefined;
}

/** Rule events plugins add, for the rule's "When" list (after the built-in ones). */
export function expansionEventOptions(expansions: Expansion[]): { value: string; label: string; hint: string }[] {
  return expansions.flatMap((one) => one.triggers.map((trigger) => ({ value: trigger.event, label: trigger.label, hint: one.owner })));
}

/** Under a rule whose event comes from a plugin: who adds it and the values it hands to the actions. */
export function ExpansionEventNote({ event }: { event: string }) {
  const expansions = useContext(StudioExpansions);
  if (!event.includes(":")) return null;
  const trigger = expansionTrigger(expansions, event);
  if (!trigger) return <p className="studio-placeholders">{tr("{0} comes from a plugin that doesn't describe it here. The rule still runs when the plugin fires it.", [event])}</p>;
  return (
    <p className="studio-placeholders">
      {trigger.vars.length > 0 ? tr("Added by {0}. Use {1} in text.", [trigger.owner, trigger.vars.map((one) => `<${one}>`).join(" ")]) : tr("Added by {0}.", [trigger.owner])}
    </p>
  );
}

/** Placeholders plugins add, as they are written in studio text. */
export function expansionPlaceholders(expansions: Expansion[]): string[] {
  return expansions.flatMap((one) => one.placeholders.map((placeholder) => `<ext:${placeholder.key}>`));
}

/** The value a new step starts with for one plugin field. */
function fieldStart(field: ExpansionField): unknown {
  switch (field.type) {
    case "number":
      return typeof field.default === "number" ? field.default : Number(field.default) || 0;
    case "bool":
      return field.default === true || field.default === "true";
    case "choice":
      return stringOf(field.default) || field.choices[0] || "";
    default:
      return stringOf(field.default);
  }
}

/** A new plugin step with its declared fields at their defaults. */
export function newExpansionStep(step: ExpansionStep): StudioData {
  const next: StudioData = { type: step.type };
  for (const field of step.fields) next[field.key] = fieldStart(field);
  return next;
}

/** "key value, key value" for a step Monolab can't draw, so lists still say something. */
function rawSummary(one: StudioData): string {
  const type = typeof one.type === "string" ? one.type : "";
  if (!type.includes(":")) return "";
  return Object.entries(one)
    .filter(([key]) => key !== "type" && key !== "negate")
    .map(([key, value]) => `${key} ${typeof value === "object" && value !== null ? JSON.stringify(value) : String(value)}`)
    .join(", ");
}

const OPS = ["=", "!=", ">", "<", ">=", "<="];

/** The fields of one action or condition. Unknown keys stay as they were. */
export function StepFields({ step, onChange, names = NO_NAMES, conditions = false, nested = true }: { step: StudioData; onChange: (next: StudioData) => void; names?: StudioNames; conditions?: boolean; nested?: boolean }) {
  const expansions = useContext(StudioExpansions);
  const menus = names.menu;
  const items = names.item;
  const set = (key: string, value: unknown) => onChange({ ...step, [key]: value });
  const decimal = (key: string, label: string, fallback: number, least?: number, most?: number) => (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input
        type="number"
        step={0.1}
        min={least}
        max={most}
        value={typeof step[key] === "number" ? (step[key] as number) : fallback}
        onChange={(event) => {
          const value = Number(event.target.value);
          if (event.target.value !== "" && Number.isFinite(value)) set(key, Math.min(most ?? Number.MAX_SAFE_INTEGER, Math.max(least ?? -Number.MAX_SAFE_INTEGER, value)));
        }}
      />
    </label>
  );
  const suggest = (key: string, label: string, options: readonly string[], placeholder: string) => (
    <label className="field studio-field">
      <span>{label}</span>
      <input list={`studio-${key}-options`} value={typeof step[key] === "string" ? (step[key] as string) : ""} placeholder={placeholder} onChange={(event) => set(key, event.target.value.trim())} spellCheck={false} />
      <datalist id={`studio-${key}-options`}>
        {options.map((one) => (
          <option key={one} value={one} />
        ))}
      </datalist>
    </label>
  );
  const text = (key: string, label: string, placeholder?: string, wide = false) => (
    <label className={`field studio-field ${wide ? "wide" : ""}`}>
      <span>{label}</span>
      <input value={typeof step[key] === "string" ? (step[key] as string) : typeof step[key] === "number" ? String(step[key]) : ""} placeholder={placeholder} onChange={(event) => set(key, event.target.value)} spellCheck={false} />
    </label>
  );
  const number = (key: string, label: string, fallback: number, least?: number) => (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input
        type="number"
        min={least}
        value={typeof step[key] === "number" ? (step[key] as number) : fallback}
        onChange={(event) => {
          const value = Number(event.target.value);
          if (Number.isFinite(value)) set(key, least === undefined ? value : Math.max(least, value));
        }}
      />
    </label>
  );
  const choose = (key: string, label: string, options: { value: string; label: string }[], fallback: string) => (
    <label className="field studio-field">
      <span>{label}</span>
      <Dropdown value={typeof step[key] === "string" ? (step[key] as string) : fallback} onChange={(value) => set(key, value)} options={options} />
    </label>
  );
  const scope = choose("scope", tr("For"), [
    { value: "player", label: tr("Each player") },
    { value: "server", label: tr("The whole server") },
  ], "player");
  const pickFrom = (key: string, label: string, names: string[], prefix: string, placeholder: string) => (
    <label className="field studio-field">
      <span>{label}</span>
      <input list={`studio-${key}-names`} value={typeof step[key] === "string" ? (step[key] as string) : ""} placeholder={placeholder} onChange={(event) => set(key, event.target.value)} spellCheck={false} />
      <datalist id={`studio-${key}-names`}>
        {names.map((name) => (
          <option key={name} value={prefix + name} />
        ))}
      </datalist>
    </label>
  );
  switch (step.type) {
    case "message":
    case "broadcast":
    case "actionbar":
      return <div className="studio-step-fields">{text("text", tr("Text"), "<green>Hello, <player>!", true)}</div>;
    case "title":
      return (
        <div className="studio-step-fields">
          {text("title", tr("Title"))}
          {text("subtitle", tr("Subtitle"))}
          {number("stayTicks", tr("Stays (ticks)"), 60, 10)}
        </div>
      );
    case "sound":
      return (
        <div className="studio-step-fields">
          {text("sound", tr("Sound"), "entity.experience_orb.pickup", true)}
          {number("volume", tr("Volume"), 1, 0)}
          {number("pitch", tr("Pitch"), 1, 0)}
        </div>
      );
    case "command":
      return (
        <div className="studio-step-fields">
          {text("command", tr("Command"), "give <player> diamond 1", true)}
          {choose("as", tr("Runs as"), [
            { value: "console", label: tr("The console") },
            { value: "player", label: tr("The player") },
          ], "console")}
        </div>
      );
    case "teleport":
      return (
        <div className="studio-step-fields">
          {text("world", tr("World"), "world")}
          {number("x", "X", 0)}
          {number("y", "Y", 64)}
          {number("z", "Z", 0)}
        </div>
      );
    case "give":
      return (
        <div className="studio-step-fields">
          {pickFrom("item", tr("Item"), items, "studio:", "diamond")}
          {number("amount", tr("Amount"), 1, 1)}
        </div>
      );
    case "menu":
      return <div className="studio-step-fields">{pickFrom("menu", tr("Menu"), menus, "", "shop")}</div>;
    case "close":
      return null;
    case "set":
      return (
        <div className="studio-step-fields">
          {text("variable", tr("Variable"), "coins")}
          {scope}
          {choose("op", tr("How"), [
            { value: "set", label: tr("Set to") },
            { value: "add", label: tr("Add") },
            { value: "subtract", label: tr("Subtract") },
          ], "set")}
          {text("value", tr("Value"), "1")}
        </div>
      );
    case "wait":
      return <div className="studio-step-fields">{number("ticks", tr("Ticks (20 = 1 second)"), 20, 1)}</div>;
    case "dialog":
      return <div className="studio-step-fields">{pickFrom("dialog", tr("Dialog"), names.dialog, "", "welcome")}</div>;
    case "loot":
      return <div className="studio-step-fields">{pickFrom("loot", tr("Loot table"), names.loot, "", "crate")}</div>;
    case "enchant":
      return (
        <div className="studio-step-fields">
          {pickFrom("enchant", tr("Enchantment"), names.enchant, "", "lifesteal")}
          {number("level", tr("Level (0 takes it off)"), 1, 0)}
        </div>
      );
    case "cosmetic":
      return (
        <div className="studio-step-fields">
          {pickFrom("cosmetic", tr("Cosmetic"), names.cosmetic, "", "crown")}
          {choose("mode", tr("Do"), [{ value: "toggle", label: tr("Put on or take off") }, { value: "wear", label: tr("Put on") }, { value: "remove", label: tr("Take off") }], "toggle")}
        </div>
      );
    case "animation":
      return (
        <div className="studio-step-fields">
          {pickFrom("model", tr("Model"), names.model, "", "dragon")}
          {text("animation", tr("Animation"), "idle")}
        </div>
      );
    case "trade":
      return <div className="studio-step-fields">{pickFrom("trade", tr("Trade list"), names.trade, "", "blacksmith")}</div>;
    case "shop":
      return <div className="studio-step-fields">{pickFrom("shop", tr("Shop"), names.shop, "", "general")}</div>;
    case "money":
      return (
        <div className="studio-step-fields">
          {!conditions && choose("op", tr("Do"), [{ value: "give", label: tr("Give") }, { value: "take", label: tr("Take") }, { value: "set", label: tr("Set to") }], "give")}
          {decimal("amount", conditions ? tr("At least") : tr("Amount"), 10, 0, 1e12)}
          <p className="studio-placeholders">{tr("Vault money, from an economy plugin like EssentialsX. Paper only.")}</p>
        </div>
      );
    case "structure":
      return (
        <div className="studio-step-fields">
          {pickFrom("structure", tr("Structure"), names.structure, "", "tower")}
          {choose("at", tr("Where"), [
            { value: "player", label: tr("At the player") },
            { value: "spot", label: tr("At a spot") },
          ], "player")}
          {step.at === "spot" && (
            <>
              {text("world", tr("World"), "world")}
              {number("x", "X", 0)}
              {number("y", "Y", 64)}
              {number("z", "Z", 0)}
            </>
          )}
          {choose("rotation", tr("Turned (degrees)"), [{ value: "", label: tr("As the structure says") }, ...["0", "90", "180", "270", "random"].map((value) => ({ value, label: value }))], "")}
        </div>
      );
    case "function":
      return <div className="studio-step-fields">{pickFrom("function", tr("Function"), names.function, "", "welcome")}</div>;
    case "quest":
      return (
        <div className="studio-step-fields">
          {pickFrom("quest", tr("Quest"), names.quest, "", "lost-sheep")}
          {conditions
            ? choose("state", tr("State"), [
                { value: "active", label: tr("Doing it now") },
                { value: "done", label: tr("Finished it") },
                { value: "new", label: tr("Never started") },
              ], "active")
            : choose("mode", tr("Do"), [
                { value: "start", label: tr("Start it") },
                { value: "abandon", label: tr("Drop it") },
                { value: "complete", label: tr("Finish it now") },
              ], "start")}
        </div>
      );
    case "emote":
      return (
        <div className="studio-step-fields">
          {pickFrom("model", tr("Model"), names.model, "", "wave")}
          {text("animation", tr("Animation (played once on the player)"), "emote")}
        </div>
      );
    case "advancement":
      return (
        <div className="studio-step-fields">
          {pickFrom("advancement", tr("Advancement"), names.advancement, "", "first_steps")}
          {!conditions && (
            <label className="studio-toggle">
              <span>{tr("Take it back instead")}</span>
              <Switch checked={step.revoke === true} onChange={(event) => set("revoke", event.target.checked)} />
            </label>
          )}
        </div>
      );
    case "permission":
      return <div className="studio-step-fields">{text("permission", tr("Permission"), "server.vip", true)}</div>;
    case "world":
      return (
        <div className="studio-step-fields">
          <label className="field studio-field wide">
            <span>{tr("Worlds")}</span>
            <input
              value={Array.isArray(step.worlds) ? (step.worlds as string[]).join(", ") : ""}
              placeholder="world, world_nether"
              onChange={(event) => set("worlds", event.target.value.split(",").map((one) => one.trim()).filter(Boolean))}
              spellCheck={false}
            />
          </label>
        </div>
      );
    case "variable":
      return (
        <div className="studio-step-fields">
          {text("variable", tr("Variable"), "coins")}
          {scope}
          {choose("op", tr("Is"), OPS.map((one) => ({ value: one, label: one })), "=")}
          {text("value", tr("Value"), "1")}
        </div>
      );
    case "chance":
      return <div className="studio-step-fields">{number("percent", tr("Percent"), 50, 0)}</div>;
    case "online":
      return (
        <div className="studio-step-fields">
          {choose("op", tr("Is"), OPS.map((one) => ({ value: one, label: one })), ">=")}
          {text("value", tr("Players"), "1")}
        </div>
      );
    case "holding":
      return <div className="studio-step-fields">{pickFrom("item", tr("Item"), items, "studio:", tr("Anything"))}</div>;
    case "damage":
    case "heal":
      return <div className="studio-step-fields">{decimal("amount", tr("Health points (2 = one heart)"), 4, 0, 2048)}</div>;
    case "potion":
      return (
        <div className="studio-step-fields">
          {suggest("effect", tr("Effect"), EFFECTS, "speed")}
          {decimal("seconds", tr("Seconds"), 5, 0.05, 86400)}
          {number("level", tr("Level"), 1, 1)}
        </div>
      );
    case "launch":
      return (
        <div className="studio-step-fields">
          {decimal("up", tr("Up"), 0.8, -10, 10)}
          {decimal("forward", tr("Forward"), 0, -10, 10)}
          <p className="studio-placeholders">{tr("In a mob skill, forward is away from the mob; elsewhere it's where the player looks.")}</p>
        </div>
      );
    case "particle":
      return (
        <div className="studio-step-fields">
          {suggest("particle", tr("Particle"), PARTICLES, "flame")}
          {number("count", tr("Count"), 10, 1)}
        </div>
      );
    case "projectile":
      return (
        <div className="studio-step-fields">
          {choose("projectile", tr("Shoots"), PROJECTILES.map((one) => ({ value: one, label: one })), "arrow")}
          {decimal("speed", tr("Speed"), 1.5, 0.1, 10)}
        </div>
      );
    case "if":
      return (
        <div className="studio-step-fields column">
          <StepList conditions label={tr("If")} empty={tr("No conditions: Yes always runs.")} value={step.if} names={names} onChange={(next) => set("if", next)} />
          {nested && <StepList label={tr("Yes")} empty={tr("Nothing yet. Add an action.")} value={step.then} names={names} onChange={(next) => set("then", next)} />}
          {nested && <StepList label={tr("No")} empty={tr("Nothing happens when a condition fails.")} value={step.else} names={names} onChange={(next) => set("else", next)} />}
        </div>
      );
    case "repeat":
      return (
        <div className="studio-step-fields column">
          {number("times", tr("Times (at most 100)"), 3, 1)}
          {nested && <StepList label={tr("Each time")} empty={tr("Nothing yet. Add an action.")} value={step.actions} names={names} onChange={(next) => set("actions", next)} />}
          <p className="studio-placeholders">{tr("<index> is the round, from 1.")}</p>
        </div>
      );
    case "each_player":
      return (
        <div className="studio-step-fields column">
          {nested && <StepList label={tr("For each player")} empty={tr("Nothing yet. Add an action.")} value={step.actions} names={names} onChange={(next) => set("actions", next)} />}
          <p className="studio-placeholders">{tr("Runs once for every online player, as that player. Waits inside don't hold up the steps after it.")}</p>
        </div>
      );
    case "cancel":
      return <p className="studio-placeholders">{tr("Stops what the player did: the block isn't broken, the item isn't dropped... Only before the first Wait, and not for chat, joins or timers.")}</p>;
    case "stop":
      return <p className="studio-placeholders">{tr("Nothing after this runs.")}</p>;
    case "take":
      return (
        <div className="studio-step-fields">
          {pickFrom("item", tr("Item"), items, "studio:", "diamond")}
          {number("amount", tr("Amount"), 1, 1)}
        </div>
      );
    case "xp":
      return (
        <div className="studio-step-fields">
          {choose("op", tr("Do"), [{ value: "give", label: tr("Give") }, { value: "take", label: tr("Take") }, { value: "set", label: tr("Set to") }], "give")}
          {number("amount", tr("Amount"), 10, 0)}
          <label className="studio-toggle">
            <span>{tr("Levels instead of points")}</span>
            <Switch checked={step.levels === true} onChange={(event) => set("levels", event.target.checked)} />
          </label>
        </div>
      );
    case "gamemode":
      if (conditions) {
        return (
          <div className="studio-step-fields">
            <label className="field studio-field wide">
              <span>{tr("Game modes")}</span>
              <input
                value={Array.isArray(step.modes) ? (step.modes as string[]).join(", ") : ""}
                placeholder="survival, adventure"
                onChange={(event) => set("modes", event.target.value.split(",").map((one) => one.trim()).filter(Boolean))}
                spellCheck={false}
              />
            </label>
          </div>
        );
      }
      return <div className="studio-step-fields">{choose("mode", tr("Game mode"), GAMEMODES.map((one) => ({ value: one, label: one })), "survival")}</div>;
    case "kick":
      return <div className="studio-step-fields">{text("text", tr("Reason"), "<red>Bye!", true)}</div>;
    case "block":
      return (
        <div className="studio-step-fields">
          {text("block", tr("Block"), "stone")}
          {text("world", tr("World"), tr("Player's world"))}
          {text("x", "X", "~")}
          {text("y", "Y", "~")}
          {text("z", "Z", "~")}
          <p className="studio-placeholders">{tr("A game block (stone, oak_stairs[facing=east]) or studio:block/<id>. ~ is the player's spot, ~1 one block past it; values like <x> work too.")}</p>
        </div>
      );
    case "fill":
    case "region":
      return (
        <div className="studio-step-fields">
          {step.type === "region" && choose("op", tr("Do"), [{ value: "copy", label: tr("Copy an area") }, { value: "paste", label: tr("Paste it") }], "copy")}
          {step.type === "region" && text("name", tr("Name"), "arena")}
          {step.type === "fill" && text("block", tr("Block"), "air")}
          {text("world", tr("World"), tr("Player's world"))}
          {(step.type === "fill" || step.op !== "paste") && (
            <>
              {text("x1", tr("From X"), "~")}
              {text("y1", tr("From Y"), "~")}
              {text("z1", tr("From Z"), "~")}
              {text("x2", tr("To X"), "~")}
              {text("y2", tr("To Y"), "~")}
              {text("z2", tr("To Z"), "~")}
            </>
          )}
          {step.type === "region" && step.op === "paste" && (
            <>
              {text("x", "X", "~")}
              {text("y", "Y", "~")}
              {text("z", "Z", "~")}
              {choose("rotate", tr("Turned (degrees)"), ["0", "90", "180", "270"].map((value) => ({ value, label: value })), "0")}
              <label className="studio-toggle">
                <span>{tr("Paste air too")}</span>
                <Switch checked={step.air === true} onChange={(event) => set("air", event.target.checked)} />
              </label>
            </>
          )}
          <p className="studio-placeholders">
            {step.type === "fill"
              ? tr("At most 32768 blocks. ~ is the player's spot.")
              : tr("Copy keeps the area under its name (studio/data/regions) for Paste; pasting puts its lowest corner at the spot. At most 262144 blocks.")}
          </p>
        </div>
      );
    case "bossbar":
      return (
        <div className="studio-step-fields">
          {choose("op", tr("Do"), [{ value: "show", label: tr("Show") }, { value: "hide", label: tr("Hide") }], "show")}
          {step.op !== "hide" && (
            <>
              {text("text", tr("Text"), "<gold>Boss fight", true)}
              {choose("color", tr("Color"), ["white", "pink", "blue", "red", "green", "yellow", "purple"].map((one) => ({ value: one, label: one })), "white")}
              {decimal("progress", tr("Filled (0 to 1)"), 1, 0, 1)}
              {number("seconds", tr("Seconds (0 = until hidden)"), 10, 0)}
            </>
          )}
        </div>
      );
    case "food":
      return <div className="studio-step-fields">{number("amount", tr("Hunger (20 = full)"), 20, 0)}</div>;
    case "health":
      if (conditions) {
        return (
          <div className="studio-step-fields">
            {choose("op", tr("Is"), OPS.map((one) => ({ value: one, label: one })), ">=")}
            {decimal("value", tr("Health (20 = full)"), 10, 0, 2048)}
          </div>
        );
      }
      return <div className="studio-step-fields">{decimal("amount", tr("Health (20 = full)"), 20, 0, 2048)}</div>;
    case "level":
      return (
        <div className="studio-step-fields">
          {choose("op", tr("Is"), OPS.map((one) => ({ value: one, label: one })), ">=")}
          {number("value", tr("Level"), 10, 0)}
        </div>
      );
    case "has_item":
      return (
        <div className="studio-step-fields">
          {pickFrom("item", tr("Item"), items, "studio:", "diamond")}
          {number("amount", tr("At least"), 1, 1)}
        </div>
      );
    case "sneaking":
      return null;
    case "time":
      return <div className="studio-step-fields">{choose("is", tr("Is"), [{ value: "day", label: tr("Day") }, { value: "night", label: tr("Night") }], "day")}</div>;
    case "weather":
      return <div className="studio-step-fields">{choose("is", tr("Is"), [{ value: "clear", label: tr("Clear") }, { value: "rain", label: tr("Rain") }, { value: "thunder", label: tr("Thunder") }], "clear")}</div>;
    case "group":
      return (
        <div className="studio-step-fields">
          {text("group", tr("Group"), "vip")}
          <p className="studio-placeholders">{tr("A LuckPerms group (checks group.<name>). Paper only.")}</p>
        </div>
      );
    case "set_permission":
      return (
        <div className="studio-step-fields">
          {choose("op", tr("Do"), [{ value: "give", label: tr("Give") }, { value: "take", label: tr("Take") }], "give")}
          {text("permission", tr("Permission"), "server.vip", true)}
          {step.op !== "take" && number("seconds", tr("For seconds (0 = for good)"), 0, 0)}
          <p className="studio-placeholders">{tr("Saved with LuckPerms when it's installed; otherwise until the player leaves (Paper) or for good (Pumpkin).")}</p>
        </div>
      );
    case "set_group":
      return (
        <div className="studio-step-fields">
          {choose("op", tr("Do"), [{ value: "add", label: tr("Add to") }, { value: "remove", label: tr("Remove from") }, { value: "set", label: tr("Only this group") }], "add")}
          {text("group", tr("Group"), "vip")}
          <p className="studio-placeholders">{tr("Needs LuckPerms. Paper only.")}</p>
        </div>
      );
    case "manage_world": {
      const op = typeof step.op === "string" ? step.op : "time";
      return (
        <div className="studio-step-fields">
          {choose("op", tr("Do"), [
            { value: "time", label: tr("Set the time") },
            { value: "weather", label: tr("Set the weather") },
            { value: "rule", label: tr("Set a game rule") },
            { value: "difficulty", label: tr("Set the difficulty") },
            { value: "border", label: tr("Set the border size") },
            { value: "load", label: tr("Load or make a world") },
            { value: "unload", label: tr("Unload a world") },
            { value: "spawn", label: tr("Send the player to its spawn") },
          ], "time")}
          {text("world", tr("World"), tr("Player's world"))}
          {op === "time" && choose("value", tr("Time"), ["day", "noon", "night", "midnight"].map((one) => ({ value: one, label: one })), "day")}
          {op === "weather" && choose("value", tr("Weather"), ["clear", "rain", "thunder"].map((one) => ({ value: one, label: one })), "clear")}
          {op === "rule" && text("rule", tr("Game rule"), "keepInventory")}
          {op === "rule" && text("value", tr("Value"), "true")}
          {op === "difficulty" && choose("value", tr("Difficulty"), ["peaceful", "easy", "normal", "hard"].map((one) => ({ value: one, label: one })), "normal")}
          {op === "border" && text("value", tr("Size (blocks)"), "1000")}
          {op === "load" && choose("value", tr("Kind"), ["normal", "nether", "end"].map((one) => ({ value: one, label: one })), "normal")}
          {op === "load" && <p className="studio-placeholders">{tr("Makes the world when it doesn't exist yet. Not on Folia.")}</p>}
          {op === "unload" && <p className="studio-placeholders">{tr("Players in it go to the main world first. The main world can't be unloaded.")}</p>}
        </div>
      );
    }
    case "summon":
      return (
        <div className="studio-step-fields">
          {pickFrom("mob", tr("Mob"), mobNamesOf(names), "", "zombie")}
          {number("count", tr("How many"), 1, 1)}
          <p className="studio-placeholders">{tr("A studio mob's name, or a game mob like zombie.")}</p>
        </div>
      );
    default:
      return <ExpansionStepFields step={step} onChange={onChange} definition={expansionStep(expansions, conditions ? "conditions" : "actions", String(step.type ?? ""))} />;
  }
}

/** Studio mob names, once `StudioNames` carries them (the "mob" kind). */
function mobNamesOf(names: StudioNames): string[] {
  const mob = (names as unknown as Record<string, unknown>).mob;
  return Array.isArray(mob) ? mob.filter((one): one is string => typeof one === "string") : [];
}

/** A plugin's step drawn from its monolab-studio.json; with no description, its values as they are. */
function ExpansionStepFields({ step, onChange, definition }: { step: StudioData; onChange: (next: StudioData) => void; definition: ExpansionStep | undefined }) {
  const type = String(step.type ?? "");
  const set = (key: string, value: unknown) => onChange({ ...step, [key]: value });
  if (!type.includes(":")) return <p className="muted studio-step-unknown">{tr("Monolab doesn't know this step. It's kept as it is.")}</p>;
  if (!definition) {
    const entries = Object.entries(step).filter(([key]) => key !== "type" && key !== "negate");
    return (
      <div className="studio-step-fields">
        {entries.map(([key, value]) =>
          typeof value === "object" && value !== null ? (
            <label key={key} className="field studio-field wide">
              <span>{key}</span>
              <input value={JSON.stringify(value)} readOnly spellCheck={false} />
            </label>
          ) : (
            <label key={key} className="field studio-field">
              <span>{key}</span>
              <input
                value={String(value)}
                onChange={(event) => set(key, typeof value === "number" && event.target.value.trim() !== "" && Number.isFinite(Number(event.target.value)) ? Number(event.target.value) : typeof value === "boolean" ? event.target.value === "true" : event.target.value)}
                spellCheck={false}
              />
            </label>
          ),
        )}
        <p className="muted studio-step-unknown">{tr("No plugin on this server describes {0}. Its values are kept as they are; the add-on runs it if the plugin is there.", [type])}</p>
      </div>
    );
  }
  return (
    <div className="studio-step-fields">
      {definition.fields.length === 0 && <p className="muted studio-step-unknown">{tr("Nothing to fill in.")}</p>}
      {definition.fields.map((field) => {
        const value = step[field.key];
        switch (field.type) {
          case "number":
            return (
              <label key={field.key} className="field studio-field studio-number">
                <span>{field.label}</span>
                <input
                  type="number"
                  step="any"
                  value={typeof value === "number" ? value : typeof value === "string" && value.trim() !== "" && Number.isFinite(Number(value)) ? Number(value) : ""}
                  onChange={(event) => {
                    const next = Number(event.target.value);
                    if (event.target.value !== "" && Number.isFinite(next)) set(field.key, next);
                  }}
                />
              </label>
            );
          case "bool":
            return (
              <label key={field.key} className="studio-toggle">
                <span>{field.label}</span>
                <Switch checked={value === true || value === "true"} onChange={(event) => set(field.key, event.target.checked)} />
              </label>
            );
          case "choice": {
            // A step written without this field (by hand or by an AI) gets the plugin's default, so show that.
            const current = stringOf(value) || stringOf(fieldStart(field));
            const options = field.choices.map((one) => ({ value: one, label: one }));
            if (current && !field.choices.includes(current)) options.push({ value: current, label: current });
            return (
              <label key={field.key} className="field studio-field">
                <span>{field.label}</span>
                <Dropdown value={current} options={options} onChange={(next) => set(field.key, next)} />
              </label>
            );
          }
          default:
            return (
              <label key={field.key} className="field studio-field">
                <span>{field.label}</span>
                <input value={stringOf(value)} placeholder={field.type === "player" ? "<player>" : undefined} onChange={(event) => set(field.key, event.target.value)} spellCheck={false} />
              </label>
            );
        }
      })}
    </div>
  );
}

function move<T>(list: T[], from: number, to: number): T[] {
  if (to < 0 || to >= list.length) return list;
  const next = [...list];
  const [one] = next.splice(from, 1);
  next.splice(to, 0, one);
  return next;
}

/** An editable list of actions or conditions. */
export function StepList({
  value,
  onChange,
  conditions = false,
  label,
  empty,
  names,
}: {
  value: unknown;
  onChange: (next: StudioData[]) => void;
  conditions?: boolean;
  label: string;
  empty: string;
  names?: StudioNames;
}) {
  const expansions = useContext(StudioExpansions);
  const list = listOf(value);
  const types: readonly string[] = conditions ? CONDITION_TYPES : ACTION_TYPES;
  const nameOf = (type: string) => (conditions ? conditionLabel(type, expansions) : actionLabel(type, expansions));
  // Plugin steps after the built-in ones, under their plugin's name.
  const added = expansions.flatMap((one) =>
    (conditions ? one.conditions : one.actions).map((step, at) => ({
      label: step.label,
      hint: step.type,
      heading: at === 0 ? one.owner : undefined,
      onSelect: () => onChange([...list, newExpansionStep(step)]),
    })),
  );
  return (
    <div className="studio-steps">
      <div className="studio-steps-head">
        <span>{label}</span>
        <DropdownMenu
          align="end"
          trigger={
            <button type="button" className="button ghost small">
              <Icon name="plus" />
              {conditions ? tr("Add condition") : tr("Add action")}
            </button>
          }
          items={[...types.map((type) => ({ label: nameOf(type), onSelect: () => onChange([...list, conditions ? newCondition(type) : newAction(type)]) })), ...added]}
        />
      </div>
      {list.length === 0 && <p className="studio-steps-empty">{empty}</p>}
      {list.map((step, at) => (
        <div key={at} className="studio-step">
          <div className="studio-step-head">
            <span className="studio-step-number">{at + 1}</span>
            <strong>{nameOf(String(step.type ?? ""))}</strong>
            {conditions && (
              <label className="studio-step-not">
                <span>{tr("Not")}</span>
                <Switch checked={step.negate === true} onChange={(event) => onChange(list.map((one, index) => (index === at ? { ...one, negate: event.target.checked } : one)))} />
              </label>
            )}
            <span className="studio-step-tools">
              <button type="button" className="icon-button" aria-label={tr("Move up")} data-tip={tr("Move up")} disabled={at === 0} onClick={() => onChange(move(list, at, at - 1))}>
                <Icon name="arrow-up" />
              </button>
              <button type="button" className="icon-button" aria-label={tr("Move down")} data-tip={tr("Move down")} disabled={at === list.length - 1} onClick={() => onChange(move(list, at, at + 1))}>
                <Icon name="arrow-down" />
              </button>
              <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={() => onChange(list.filter((_, index) => index !== at))}>
                <Icon name="trash" />
              </button>
            </span>
          </div>
          <StepFields step={step} names={names} conditions={conditions} onChange={(next) => onChange(list.map((one, index) => (index === at ? next : one)))} />
        </div>
      ))}
    </div>
  );
}

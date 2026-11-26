import { tr } from "../../lib/i18n";
import type { StudioData } from "./StudioPreview";

/** What can start a rule. Matches StudioRules.EVENTS in the add-on. */
export const RULE_EVENTS = ["join", "first_join", "quit", "chat", "death", "respawn", "break", "place", "world_change", "command", "timer", "schedule", "click", "click_entity", "kill", "hit", "hurt", "eat", "craft", "fish", "sneak", "drop", "pickup", "level_up", "teleport", "gamemode", "server_start"] as const;
/** Where a recipe is made. Matches StudioItems.RECIPE_TYPES in the add-on. */
export const RECIPE_TYPES = ["shaped", "shapeless", "furnace", "blasting", "smoking", "campfire", "stonecutting"] as const;

/** Studio names other files can point at: menus to open, items to give, dialogs, loot tables, advancements. */
export type StudioNames = { menu: string[]; item: string[]; dialog: string[]; loot: string[]; advancement: string[]; model: string[]; mob: string[]; enchant: string[]; song: string[]; cosmetic: string[]; trade: string[]; function: string[]; quest: string[]; structure: string[]; biome: string[]; shop: string[] };
export const NO_NAMES: StudioNames = { menu: [], item: [], dialog: [], loot: [], advancement: [], model: [], mob: [], enchant: [], song: [], cosmetic: [], trade: [], function: [], quest: [], structure: [], biome: [], shop: [] };

/** Studio maker M2: glyph images (`studio/font/<id>.json`), used as `:name:` in text and as canvas pictures. */
export const FONT_KIND = "font";
export const FONT_ICON = "image";

export function fontKindLabel(): string {
  return tr("Glyph images");
}

export function fontKindHint(): string {
  return tr("Pictures used as :name: in any text and in canvas dialogs");
}

export function eventLabel(event: string): string {
  switch (event) {
    case "join":
      return tr("A player joins");
    case "first_join":
      return tr("A player joins for the first time");
    case "quit":
      return tr("A player leaves");
    case "chat":
      return tr("A player chats");
    case "death":
      return tr("A player dies");
    case "respawn":
      return tr("A player respawns");
    case "break":
      return tr("A player breaks a block");
    case "place":
      return tr("A player places a block");
    case "world_change":
      return tr("A player changes world");
    case "command":
      return tr("A player uses a command");
    case "timer":
      return tr("Every few seconds");
    case "schedule":
      return tr("At set times of day");
    case "click":
      return tr("A player clicks");
    case "click_entity":
      return tr("A player right-clicks a mob");
    case "kill":
      return tr("A player kills a mob");
    case "hit":
      return tr("A player hits a mob");
    case "hurt":
      return tr("A player is hurt");
    case "eat":
      return tr("A player eats or drinks");
    case "craft":
      return tr("A player crafts");
    case "fish":
      return tr("A player catches a fish");
    case "sneak":
      return tr("A player starts sneaking");
    case "drop":
      return tr("A player drops an item");
    case "pickup":
      return tr("A player picks up an item");
    case "level_up":
      return tr("A player levels up");
    case "teleport":
      return tr("A player teleports");
    case "gamemode":
      return tr("A player's game mode changes");
    case "server_start":
      return tr("The server starts");
    default:
      return event;
  }
}

export function recipeLabel(type: string): string {
  switch (type) {
    case "shaped":
      return tr("Crafting table (shaped)");
    case "shapeless":
      return tr("Crafting table (any order)");
    case "furnace":
      return tr("Furnace");
    case "blasting":
      return tr("Blast furnace");
    case "smoking":
      return tr("Smoker");
    case "campfire":
      return tr("Campfire");
    case "stonecutting":
      return tr("Stonecutter");
    default:
      return type;
  }
}

export function objectOf(value: unknown): StudioData {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as StudioData) : {};
}

/** A short "when" line for a rule: the event and its filter. */
export function whenSummary(when: StudioData): string {
  const event = typeof when.event === "string" ? when.event : "";
  const list = (key: string) => (Array.isArray(when[key]) ? (when[key] as unknown[]).filter((one) => typeof one === "string" && one).join(", ") : "");
  const text = (key: string) => (typeof when[key] === "string" ? (when[key] as string) : "");
  const detail =
    event === "break" || event === "place"
      ? list("blocks")
      : event === "click"
        ? [text("button"), list("blocks")].filter(Boolean).join(" · ")
        : event === "click_entity" || event === "kill" || event === "hit"
          ? list("entities")
          : ["eat", "craft", "fish", "drop", "pickup"].includes(event)
            ? list("items")
            : event === "gamemode"
              ? text("mode")
      : event === "chat"
        ? text("contains")
        : event === "world_change"
          ? text("world")
          : event === "command"
            ? text("command")
            : event === "timer"
              ? tr("every {0} s", [typeof when.everySeconds === "number" ? when.everySeconds : 60])
              : event === "schedule"
                ? list("at")
                : "";
  return detail ? `${eventLabel(event)} · ${detail}` : eventLabel(event);
}

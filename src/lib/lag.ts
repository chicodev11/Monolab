import type { LagFinding } from "./bindings";
import { tr } from "./i18n";

export type LagLine = {
  title: string;
  detail: string;
  action?: LagAction;
};

export type LagAction =
  | { kind: "overview" }
  | { kind: "properties" }
  | { kind: "copy"; command: string }
  | { kind: "console"; command: string; ask: string };

export function dimensionOf(world: string | null | undefined): string {
  if (!world) return "minecraft:overworld";
  if (world.endsWith("_nether")) return "minecraft:the_nether";
  if (world.endsWith("_the_end")) return "minecraft:the_end";
  if (world.includes(":")) return world;
  return "minecraft:overworld";
}

export function tpCommand(world: string | null | undefined, x: number, z: number): string {
  return `execute in ${dimensionOf(world)} run tp @s ${x} ~ ${z}`;
}

export function killInChunkCommand(world: string | null | undefined, type: string, x: number, z: number): string {
  return `execute in ${dimensionOf(world)} run kill @e[type=${type},x=${x - 8},y=-64,z=${z - 8},dx=16,dy=400,dz=16]`;
}

const gb = (mib: number) => `${(mib / 1024).toFixed(1)} GB`;

export function lagLine(one: LagFinding): LagLine {
  const value = one.value ?? 0;
  const limit = one.limit ?? 0;
  const count = value.toLocaleString();
  switch (one.kind) {
    case "hotspot": {
      const where = `${one.world ?? "world"} ${one.x ?? 0}, ${one.z ?? 0}`;
      const top = one.subject ?? "";
      const line: LagLine = {
        title: tr("{0, plural, one {# entity} other {# entities}} in one chunk", [count]),
        detail: tr("Near {0} · mostly {1}. This may be a mob farm or a pile of items. Clear the area or pause the farm.", [where, top]),
      };
      if (one.x != null && one.z != null) {
        line.action =
          top === "minecraft:item"
            ? {
                kind: "console",
                command: killInChunkCommand(one.world, top, one.x, one.z),
                ask: tr("Deletes all dropped items in one chunk near {0}, including items players just dropped.", [where]),
              }
            : { kind: "copy", command: tpCommand(one.world, one.x, one.z) };
      }
      return line;
    }
    case "items":
      return {
        title: tr("{0, plural, one {# dropped item} other {# dropped items}}", [count]),
        detail: tr("A farm outlet may be blocked, or items may be piling up. They despawn after 5 minutes, but if they keep appearing, find the source."),
        action: {
          kind: "console",
          command: "kill @e[type=minecraft:item]",
          ask: tr("Deletes all dropped items across all worlds, including items players just dropped."),
        },
      };
    case "one-kind":
      return {
        title: tr("{1} × {0}", [one.subject ?? "", count]),
        detail: tr("One entity type is unusually common. A farm may be too large, or breeding may still be active. Find where they gather and reduce their numbers."),
      };
    case "entities":
      return {
        title: tr("{0, plural, one {# entity} other {# entities}} in total", [count]),
        detail: tr("Entities are spread across several areas. Check the list by type and reduce unused farms and villagers."),
      };
    case "entity-growth":
      return {
        title: tr("Entities {0} → {1} in the last 30 minutes", [limit.toLocaleString(), count]),
        detail: tr("Something keeps increasing. Check breeding farms, spawners and areas where items pile up."),
      };
    case "chunks":
      return {
        title: tr("{0, plural, one {# loaded chunk} other {# loaded chunks}}", [count]),
        detail: tr("The player count and view distance should need about {0} chunks. There may be chunk loaders (mods) or force-loaded chunks.", [limit.toLocaleString()]),
        action: { kind: "copy", command: "forceload query" },
      };
    case "view-distance":
      return {
        title: tr("View distance {0}", [value.toFixed(0)]),
        detail: tr("Lowering this to {0} or less greatly reduces the chunks each player loads. This is view-distance in Server settings. Applies on the next start.", [limit.toFixed(0)]),
        action: { kind: "properties" },
      };
    case "simulation-distance":
      return {
        title: tr("Simulation distance {0}", [value.toFixed(0)]),
        detail: tr("Lowering it to {0} or less shrinks the area where mobs and crops tick. It's simulation-distance in server settings.", [limit.toFixed(0)]),
        action: { kind: "properties" },
      };
    case "heap":
      return {
        title: tr("Memory {0}% used", [value.toFixed(0)]),
        detail: tr("Memory use is close to the {0} limit, causing frequent GC pauses. Increase memory in Overview or reduce the entities and chunks listed above.", [gb(limit)]),
        action: { kind: "overview" },
      };
    case "machine-memory":
      return {
        title: tr("Server memory {0} is close to the computer's total RAM of {1}", [gb(value), gb(limit)]),
        detail: tr("If Windows runs short, the whole computer slows down. Leave 2 GB free."),
        action: { kind: "overview" },
      };
    case "cpu":
      return {
        title: tr("CPU {0}%", [value.toFixed(0)]),
        detail: tr("The server is using nearly all of this computer's CPU. Check whether the game or another server is running alongside it."),
      };
    case "aikar":
      return {
        title: tr("Try Aikar's flags"),
        detail: tr("At least 4 GB of memory is allocated, but no G1 tuning flags are set. Select the Aikar preset under JVM options in Overview to reduce GC stutter. Applies on the next start."),
        action: { kind: "overview" },
      };
    default:
      return { title: one.kind, detail: "" };
  }
}

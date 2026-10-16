import { describe, expect, test } from "vitest";
import type { LagFinding } from "./bindings";
import { dimensionOf, killInChunkCommand, lagLine, tpCommand } from "./lag";

const finding = (over: Partial<LagFinding>): LagFinding => ({
  kind: "entities",
  level: "medium",
  value: null,
  limit: null,
  world: null,
  x: null,
  z: null,
  subject: null,
  ...over,
});

describe("dimensionOf maps Paper folder names to dimension ids", () => {
  test.each([
    ["world", "minecraft:overworld"],
    ["world_nether", "minecraft:the_nether"],
    ["world_the_end", "minecraft:the_end"],
    ["survival_nether", "minecraft:the_nether"],
    ["twilightforest:twilight_forest", "twilightforest:twilight_forest"],
    [null, "minecraft:overworld"],
  ])("%s → %s", (world, id) => {
    expect(dimensionOf(world)).toBe(id);
  });
});

test("commands have no leading slash", () => {
  expect(tpCommand("world_nether", 200, -56)).toBe("execute in minecraft:the_nether run tp @s 200 ~ -56");
  expect(killInChunkCommand("world", "minecraft:item", 200, -56)).toBe(
    "execute in minecraft:overworld run kill @e[type=minecraft:item,x=192,y=-64,z=-64,dx=16,dy=400,dz=16]",
  );
});

describe("lagLine", () => {
  test("crowded chunks offer clear for items and teleport otherwise", () => {
    const zombies = lagLine(finding({ kind: "hotspot", level: "high", value: 1900, world: "world", x: 200, z: -56, subject: "minecraft:zombie" }));
    expect(zombies.title).toContain("1,900");
    expect(zombies.detail).toContain("world 200, -56");
    expect(zombies.detail).toContain("minecraft:zombie");
    expect(zombies.action).toEqual({ kind: "copy", command: "execute in minecraft:overworld run tp @s 200 ~ -56" });

    const items = lagLine(finding({ kind: "hotspot", level: "medium", value: 120, world: "world", x: 200, z: -56, subject: "minecraft:item" }));
    expect(items.action?.kind).toBe("console");
    if (items.action?.kind === "console") {
      expect(items.action.command).toContain("kill @e[type=minecraft:item");
      expect(items.action.ask.length).toBeGreaterThan(10);
    }
  });

  test("links to settings and overview", () => {
    expect(lagLine(finding({ kind: "view-distance", value: 16, limit: 10 })).action).toEqual({ kind: "properties" });
    expect(lagLine(finding({ kind: "heap", value: 95, limit: 4096 })).action).toEqual({ kind: "overview" });
    expect(lagLine(finding({ kind: "heap", value: 95, limit: 4096 })).detail).toContain("4.0 GB");
    expect(lagLine(finding({ kind: "aikar", level: "low" })).action).toEqual({ kind: "overview" });
  });

  test("keeps entity ids as they are", () => {
    const line = lagLine(finding({ kind: "one-kind", value: 640, subject: "minecraft:chicken" }));
    expect(line.title).toContain("minecraft:chicken");
    expect(line.title).toContain("640");
  });

  test("unknown kinds show their id", () => {
    expect(lagLine(finding({ kind: "something-new" })).title).toBe("something-new");
  });
});

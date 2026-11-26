import { describe, expect, it } from "vitest";
import { enchantLevel, maxDurability } from "./items";

describe("maxDurability", () => {
  it("knows tools, armor and the odd ones", () => {
    expect(maxDurability("minecraft:diamond_sword")).toBe(1561);
    expect(maxDurability("minecraft:netherite_pickaxe")).toBe(2031);
    expect(maxDurability("minecraft:diamond_chestplate")).toBe(528);
    expect(maxDurability("minecraft:leather_boots")).toBe(65);
    expect(maxDurability("minecraft:elytra")).toBe(432);
    expect(maxDurability("minecraft:turtle_helmet")).toBe(275);
  });

  it("says 0 when it does not know", () => {
    expect(maxDurability("minecraft:stone")).toBe(0);
    expect(maxDurability("minecraft:diamond")).toBe(0);
    expect(maxDurability("othermod:ruby_sword")).toBe(0);
  });
});

describe("enchantLevel", () => {
  it("writes roman numerals like the game", () => {
    expect(enchantLevel("minecraft:sharpness", 5)).toBe("V");
    expect(enchantLevel("minecraft:unbreaking", 1)).toBe("I");
    expect(enchantLevel("minecraft:efficiency", 12)).toBe("12");
  });

  it("leaves out the level of single-level enchantments", () => {
    expect(enchantLevel("minecraft:mending", 1)).toBe("");
    expect(enchantLevel("minecraft:mending", 2)).toBe("II");
  });
});

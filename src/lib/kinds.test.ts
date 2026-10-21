import { expect, test } from "vitest";
import { addonKind, coreLabel, gameInfo, has, isGame, isProxy, kindOf, minecraftCores } from "./kinds";

test("Minecraft cores keep their features", () => {
  expect(minecraftCores).toContain("Paper");
  expect(minecraftCores).not.toContain("BungeeCord");
  expect(has("Paper", "java")).toBe(true);
  expect(has("Paper", "pregen")).toBe(true);
  expect(has("Vanilla", "pregen")).toBe(false);
  expect(has("Pumpkin", "java")).toBe(false);
  expect(kindOf("Pumpkin").addons).toBe("wasm");
  expect(addonKind("Pumpkin")).toBe(null);
  expect(addonKind("Velocity")).toBe("plugin");
  expect(addonKind("NeoForge")).toBe("mod");
  expect(isProxy("Velocity")).toBe(true);
  expect(has("Velocity", "versions")).toBe(false);
});

test("an unknown core reads as a Java Minecraft server", () => {
  expect(isGame("Custom")).toBe(false);
  expect(has("Custom", "java")).toBe(true);
  expect(addonKind("Custom")).toBe(null);
});

test("games have their own entries and no Minecraft features", () => {
  expect(gameInfo.length).toBeGreaterThan(0);
  for (const game of gameInfo) {
    expect(isGame(game.id)).toBe(true);
    expect(kindOf(game.id).features.size).toBe(0);
    expect(game.logo.startsWith("https://")).toBe(true);
    expect(game.sizeBytes).toBeGreaterThan(0);
  }
  expect(coreLabel("Valheim", "latest")).toBe("Valheim");
  expect(coreLabel("Paper", "26.2")).toBe("Paper 26.2");
});

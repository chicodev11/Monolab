import { expect, test } from "vitest";
import { addonKind, coreLabel, has, isProxy, kindOf, minecraftCores } from "./kinds";

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
  expect(has("Custom", "java")).toBe(true);
  expect(addonKind("Custom")).toBe(null);
});

test("core labels include the version", () => {
  expect(coreLabel("Paper", "26.2")).toBe("Paper 26.2");
});

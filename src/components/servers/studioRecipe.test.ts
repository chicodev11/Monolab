import { describe, expect, it } from "vitest";
import { recipeGrid, withRecipeGrid } from "./studioRecipe";

describe("recipe grid", () => {
  it("reads a shaped pattern into 9 slots", () => {
    const cells = recipeGrid({ type: "shaped", shape: ["DDD", " S ", " S "], keys: { D: "diamond", S: "stick" } });
    expect(cells).toEqual(["diamond", "diamond", "diamond", "", "stick", "", "", "stick", ""]);
  });

  it("reads a small pattern into the top left", () => {
    expect(recipeGrid({ shape: ["AA", "AA"], keys: { A: "oak_planks" } })).toEqual(["oak_planks", "oak_planks", "", "oak_planks", "oak_planks", "", "", "", ""]);
  });

  it("writes a 3x3 shape and keeps the file's letters", () => {
    const data = { type: "shaped", shape: ["X"], keys: { X: "diamond" } };
    const next = withRecipeGrid(data, ["", "", "", "", "diamond", "", "", "stick", ""]);
    expect(next.shape).toEqual(["   ", " X ", " S "]);
    expect(next.keys).toEqual({ X: "diamond", S: "stick" });
    expect(recipeGrid(next)).toEqual(["", "", "", "", "diamond", "", "", "stick", ""]);
  });

  it("picks another letter when the first one is taken", () => {
    const next = withRecipeGrid({ type: "shaped" }, ["stone", "stick", "string", "", "", "", "", "", ""]);
    expect(next.keys).toEqual({ S: "stone", A: "stick", B: "string" });
    expect(next.shape).toEqual(["SAB", "   ", "   "]);
  });

  it("drops letters nothing uses and empties the shape when the grid is empty", () => {
    expect(withRecipeGrid({ shape: ["D"], keys: { D: "diamond", Q: "quartz" } }, Array(9).fill(""))).toMatchObject({ shape: [], keys: {} });
  });

  it("keeps shapeless ingredients in slot order", () => {
    const next = withRecipeGrid({ type: "shapeless" }, ["", "sugar", "", "egg", "", "", "", "", ""]);
    expect(next.ingredients).toEqual(["", "sugar", "", "egg"]);
    expect(recipeGrid(next)).toEqual(["", "sugar", "", "egg", "", "", "", "", ""]);
  });
});

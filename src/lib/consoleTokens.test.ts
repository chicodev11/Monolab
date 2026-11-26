import { describe, expect, it } from "vitest";
import { consoleTokens } from "./consoleTokens";

describe("consoleTokens", () => {
  it("keeps every character, so the shaded copy lines up with the input", () => {
    const typed = "tp  @a[distance=..5] ~ ^-3 12.5 spawn";
    expect(consoleTokens(typed).map((one) => one.text).join("")).toBe(typed);
  });

  it("tells the command, selectors and numbers apart", () => {
    const kinds = consoleTokens("give @p diamond 64 ~").filter((one) => one.kind !== "space").map((one) => one.kind);
    expect(kinds).toEqual(["command", "selector", "word", "number", "number"]);
    expect(consoleTokens("")).toEqual([]);
    expect(consoleTokens("  say").map((one) => one.kind)).toEqual(["space", "command"]);
  });
});


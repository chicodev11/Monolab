import { describe, expect, it } from "vitest";
import { formatPlayerCommands, parsePlayerCommands, playerCommandLines } from "../components/servers/PlayerCommands";
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

describe("player commands", () => {
  it("reads one set per line and fills in the player", () => {
    const list = parsePlayerCommands("Kit | /give {player} bread 16 | say hi {player}\n\nbroken line\nTo spawn | tp {uuid} 0 64 0");
    expect(list).toEqual([
      { label: "Kit", lines: ["/give {player} bread 16", "say hi {player}"] },
      { label: "To spawn", lines: ["tp {uuid} 0 64 0"] },
    ]);
    expect(playerCommandLines(list[0], { name: "Steve", uuid: "u-1" })).toEqual(["give Steve bread 16", "say hi Steve"]);
    expect(parsePlayerCommands(formatPlayerCommands(list))).toEqual(list);
  });
});

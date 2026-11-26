import { describe, expect, test } from "vitest";
import { entryScore, fuzzyScore, rankPalette, type PaletteEntry } from "./palette";

const entry = (id: string, label: string, extra: Partial<PaletteEntry> = {}): PaletteEntry => ({
  id,
  label,
  hint: "",
  icon: "dot",
  group: "action",
  ...extra,
});

describe("fuzzyScore", () => {
  test("matches letters in order, not out of order", () => {
    expect(fuzzyScore("bkp", "Backups")).not.toBeNull();
    expect(fuzzyScore("pkb", "Backups")).toBeNull();
  });
  test("a substring beats a scattered match, a word start beats the middle", () => {
    expect(fuzzyScore("back", "Backups")!).toBeGreaterThan(fuzzyScore("bcps", "Backups")!);
    expect(fuzzyScore("con", "Open console")!).toBeGreaterThan(fuzzyScore("con", "Reconnect")!);
  });
  test("empty needle matches everything", () => {
    expect(fuzzyScore("", "anything")).toBe(0);
  });
});

describe("entryScore", () => {
  test("every word has to match somewhere", () => {
    const one = entry("a", "Survival › Backups", { keywords: "restore" });
    expect(entryScore("surv back", one)).not.toBeNull();
    expect(entryScore("surv restore", one)).not.toBeNull();
    expect(entryScore("surv worlds", one)).toBeNull();
  });
});

describe("rankPalette", () => {
  const list: PaletteEntry[] = [
    entry("server-a", "Survival", { group: "server", scopes: "a", serverId: "a" }),
    entry("server-b", "Creative", { group: "server", scopes: "b", serverId: "b" }),
    entry("a-backups", "Survival › Backups", { group: "tab", serverId: "a" }),
    entry("b-backups", "Creative › Backups", { group: "tab", serverId: "b" }),
    entry("a-start", "Start Survival", { serverId: "a" }),
    entry("new", "New server", { group: "app" }),
  ];
  test("scoping keeps only that server's commands", () => {
    const ids = rankPalette(list, "", { scope: "a" }).map((one) => one.id);
    expect(ids).toEqual(["a-backups", "a-start"]);
  });
  test("no query: recent first, then servers and app actions", () => {
    const shown = rankPalette(list, "", { recent: ["b-backups"] });
    expect(shown.map((one) => one.id)).toEqual(["b-backups", "server-a", "server-b", "new"]);
    expect(shown[0].group).toBe("recent");
  });
  test("the server on screen wins a tie", () => {
    expect(rankPalette(list, "backups", { current: "b" })[0].id).toBe("b-backups");
    expect(rankPalette(list, "backups", { current: "a" })[0].id).toBe("a-backups");
  });
  test("drops entries that don't match", () => {
    expect(rankPalette(list, "zzz")).toEqual([]);
  });
});

import { describe, expect, it } from "vitest";
import { EMPTY_FILTER, isEmptyFilter, lineMatches, lineMinute, lineSource, sourcesIn, withSaved } from "./logFilter";

const paper = "[12:04:11 INFO]: [Essentials] Loaded 30 kits";
const paperPlain = "[12:05:00 WARN]: Can't keep up! Is the server overloaded?";
const fabric = "[22:10:00] [Server thread/INFO] [fabric/]: Loading 41 mods";
const vanilla = "[23:59:59] [Server thread/INFO]: Done (3.2s)!";

describe("lineMinute", () => {
  it("reads the clock at the start", () => {
    expect(lineMinute(paper)).toBe(12 * 60 + 4);
    expect(lineMinute(fabric)).toBe(22 * 60 + 10);
    expect(lineMinute("no time here")).toBeNull();
  });
});

describe("lineSource", () => {
  it("finds the plugin or logger", () => {
    expect(lineSource(paper)).toBe("Essentials");
    expect(lineSource(fabric)).toBe("fabric");
    expect(lineSource(paperPlain)).toBeNull();
    expect(lineSource(vanilla)).toBeNull();
    // Pumpkin's latest.log: the thread is not a source, at any level.
    expect(lineSource("[00:42:56] [tokio-rt-worker/DEBUG]: x")).toBeNull();
    expect(lineSource("[00:42:56] [main/TRACE]: [MyPlugin] hi")).toBe("MyPlugin");
    expect(lineSource("[00:42:56] [main/INFO]: [MyPlugin] hi")).toBe("MyPlugin");
  });
  it("lists the busiest first", () => {
    expect(sourcesIn([paper, paper, fabric, vanilla])).toEqual(["Essentials", "fabric"]);
  });
});

describe("lineMatches", () => {
  it("narrows by text, source and time", () => {
    expect(lineMatches(paper, { ...EMPTY_FILTER, query: "kits" })).toBe(true);
    expect(lineMatches(paper, { ...EMPTY_FILTER, source: "LuckPerms" })).toBe(false);
    expect(lineMatches(paper, { ...EMPTY_FILTER, from: "12:00", to: "12:30" })).toBe(true);
    expect(lineMatches(paper, { ...EMPTY_FILTER, from: "13:00" })).toBe(false);
    expect(lineMatches("no time", { ...EMPTY_FILTER, to: "13:00" })).toBe(false);
  });
  it("handles ranges past midnight", () => {
    expect(lineMatches(vanilla, { ...EMPTY_FILTER, from: "22:00", to: "02:00" })).toBe(true);
    expect(lineMatches(paper, { ...EMPTY_FILTER, from: "22:00", to: "02:00" })).toBe(false);
  });
});

describe("saved filters", () => {
  it("replaces one with the same name", () => {
    const first = withSaved([], { ...EMPTY_FILTER, query: "a" }, "Mine");
    const second = withSaved(first, { ...EMPTY_FILTER, query: "b" }, " Mine ");
    expect(second).toHaveLength(1);
    expect(second[0].query).toBe("b");
    expect(withSaved(second, EMPTY_FILTER, "  ")).toBe(second);
  });
  it("knows an empty filter", () => {
    expect(isEmptyFilter(EMPTY_FILTER)).toBe(true);
    expect(isEmptyFilter({ ...EMPTY_FILTER, levels: ["error"] })).toBe(false);
  });
});

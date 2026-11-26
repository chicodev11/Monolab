import { describe, expect, it } from "vitest";
import { fillPlaceholders, miniMessageLines, miniMessagePlain, parseMiniMessage } from "./miniMessage";

describe("miniMessage", () => {
  it("reads colors and decorations", () => {
    const [line] = parseMiniMessage("<red>Hi <b>there</b></red> you");
    expect(line).toEqual([
      { text: "Hi ", color: "#ff5555" },
      { text: "there", color: "#ff5555", bold: true },
      { text: " you" },
    ]);
  });

  it("takes hex, color: and negation", () => {
    const [line] = parseMiniMessage("<#12ab34>a<color:gold>b</color><b><!b>c</b>");
    expect(line.map((one) => [one.text, one.color, one.bold])).toEqual([
      ["a", "#12ab34", undefined],
      ["b", "#ffaa00", undefined],
      ["c", "#12ab34", false],
    ]);
  });

  it("splits lines and resets", () => {
    const lines = parseMiniMessage("<red>one<newline>two<reset>three");
    expect(lines).toHaveLength(2);
    expect(lines[1]).toEqual([{ text: "two", color: "#ff5555" }, { text: "three" }]);
  });

  it("paints a gradient across letters", () => {
    const [line] = parseMiniMessage("<gradient:#000000:#ffffff>abc</gradient>");
    expect(line.map((one) => one.text).join("")).toBe("abc");
    expect(line.map((one) => one.color)).toEqual(["#000000", "#808080", "#ffffff"]);
  });

  it("paints a gradient across nested styles", () => {
    const [line] = parseMiniMessage("<gradient:#000000:#ffffff>a<b>b</b></gradient>");
    expect(line).toEqual([
      { text: "a", color: "#000000" },
      { text: "b", color: "#ffffff", bold: true },
    ]);
  });

  it("keeps unknown tags and escapes as text", () => {
    expect(miniMessagePlain("a <nope> b \\<red> c", {})).toBe("a <nope> b <red> c");
    expect(miniMessagePlain("1 < 2", {})).toBe("1 < 2");
  });

  it("fills placeholders without letting values add tags", () => {
    expect(fillPlaceholders("Hi <player>", { player: "<red>Bob" })).toBe("Hi \\<red>Bob");
    const lines = miniMessageLines("<green><player></green> <unknown>", { player: "<red>Bob" });
    expect(lines[0][0]).toEqual({ text: "<red>Bob", color: "#55ff55" });
    expect(miniMessagePlain("Online: <online>/<max>", { online: "3", max: "10" })).toBe("Online: 3/10");
  });

  it("survives broken input", () => {
    expect(miniMessagePlain("<red", {})).toBe("<red");
    expect(miniMessagePlain("</b>x", {})).toBe("x");
  });
});

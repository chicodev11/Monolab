import { afterEach, describe, expect, it } from "vitest";
import widths from "../../../crates/studio-format/src/vanilla_widths.txt?raw";
import { afterOf, bodyOf, canvasProblems, hitBox, layoutCanvas, leavingOf, letterWidth, isBitmap, parseWidths, setWidths, textWidth } from "./canvasModel";
import type { AppGlyph } from "./studioGlyphs";

const glyph = (name: string, width: number, height: number): AppGlyph => ({ id: name, name, image: name, url: "", width, height, ascent: 7, advance: width + 1, pictureWidth: width, pictureHeight: height });
const glyphs = { bg: glyph("bg", 176, 166), buy: glyph("buy", 16, 16), down: glyph("down", 16, 17) };

afterEach(() => setWidths(null));

describe("canvas model", () => {
  it("reads the add-ons' width table", () => {
    setWidths(parseWidths(widths));
    expect(letterWidth("A".codePointAt(0) ?? 0)).toBe(6);
    expect(letterWidth("i".codePointAt(0) ?? 0)).toBe(2);
    expect(textWidth("Shop")).toBe(6 + 6 + 6 + 6);
    expect(isBitmap("상".codePointAt(0) ?? 0)).toBe(false);
    expect(isBitmap("S".codePointAt(0) ?? 0)).toBe(true);
  });

  it("lays text out like the add-ons", () => {
    const laid = layoutCanvas({ width: 100, height: 27, elements: [{ type: "text", text: "Shop", x: 50, y: 4, align: "center" }, { type: "text", text: "상점", x: 0, y: 13 }] }, glyphs);
    const shop = laid.find((one) => one.index === 0);
    // "Shop" is 24 across, 23 drawn: centered on 50 starts at 50 - 11.
    expect(shop?.x).toBe(39);
    expect(shop?.y).toBe(4);
    const korean = laid.find((one) => one.index === 1);
    expect(korean?.snapped).toBe(true);
    expect(korean?.y).toBe(9);
  });

  it("draws lower rows over higher ones", () => {
    const laid = layoutCanvas({ elements: [{ type: "image", glyph: "buy", x: 0, y: 20 }, { type: "image", glyph: "bg", x: 0, y: 0 }] }, glyphs);
    expect(laid.map((one) => one.index)).toEqual([1, 0]);
    const problems = canvasProblems({}, { width: 176, height: 166, elements: [{ type: "image", glyph: "bg", x: 0, y: 9 }, { type: "image", glyph: "buy", x: 0, y: 0 }] }, layoutCanvas({ elements: [{ type: "image", glyph: "bg", x: 0, y: 9 }, { type: "image", glyph: "buy", x: 0, y: 0 }] }, glyphs), true);
    expect(problems.some((one) => one.index === 1 && one.message.includes("#1"))).toBe(true);
  });

  it("grows click areas to whole rows", () => {
    const [area] = layoutCanvas({ elements: [{ type: "area", x: 10, y: 12, w: 30, h: 10 }] }, glyphs);
    expect(hitBox(area)).toEqual({ x: 10, y: 9, w: 30, h: 18 });
  });

  it("reads after, the old close and leaving actions", () => {
    expect(afterOf({})).toBe("close");
    expect(afterOf({ close: false })).toBe("stay");
    expect(afterOf({ close: false, after: "refresh" })).toBe("refresh");
    expect(leavingOf([{ type: "message" }, { type: "if", if: [{ type: "menu" }], then: [{ type: "dialog", dialog: "x" }] }])).toEqual(["dialog"]);
  });

  it("swaps a pressed button's picture", () => {
    const canvas = { elements: [{ type: "button", glyph: "buy", pressed: "down", x: 0, y: 0 }] };
    expect(layoutCanvas(canvas, glyphs, 0)[0].glyph?.name).toBe("down");
    expect(layoutCanvas(canvas, glyphs)[0].glyph?.name).toBe("buy");
  });

  it("finds what the game will do differently", () => {
    const canvas = { width: 176, height: 18, elements: [{ type: "button", glyph: "buy", pressed: "down", x: 0, y: 0, actions: [{ type: "dialog", dialog: "nope" }] }, { type: "image", glyph: "bg", x: 900, y: 0 }] };
    const laid = layoutCanvas(canvas, glyphs);
    const names = { menu: [], item: [], dialog: ["shop"], loot: [], advancement: [], model: [], mob: [], enchant: [], song: [], cosmetic: [], trade: [], function: [], quest: [], structure: [], biome: [], shop: [] };
    const problems = canvasProblems({ variables: { tab: "0" } }, canvas, laid, true, names);
    expect(problems.some((one) => one.index === 0 && one.message.includes("dialog nope"))).toBe(true);
    expect(problems.some((one) => one.index === 0 && one.message.includes("stays open"))).toBe(true);
    expect(bodyOf(canvas, laid).body).toBeGreaterThan(1024);
    expect(problems.some((one) => one.index === null && one.level === "error")).toBe(true);
  });
});

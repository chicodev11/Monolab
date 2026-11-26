import { describe, expect, it } from "vitest";
import { hasStyle, parseAnsi } from "./ansi";

const ESC = "\u001b";

describe("parseAnsi keeps console colors", () => {
  it("a line without color is one span", () => {
    const line = "[17:07:55 INFO]: Done (3.2s)!";
    expect(parseAnsi(line)).toEqual([{ text: line, style: {} }]);
    expect(hasStyle(parseAnsi(line))).toBe(false);
  });

  it("splits ANSI colors into spans without changing the text", () => {
    const pieces = parseAnsi(`${ESC}[2m2026-08-26 15:36:37${ESC}[0m ${ESC}[32m INFO${ESC}[0m hi`);
    expect(pieces.map((one) => one.text).join("")).toBe("2026-08-26 15:36:37  INFO hi");
    expect(pieces[0].style.dim).toBe(true);
    expect(pieces[2].style.color).toBeTruthy();
    expect(pieces[3].style).toEqual({});
  });

  it("256 colors and true color", () => {
    expect(parseAnsi(`${ESC}[38;5;196mred`)[0].style.color).toBe("rgb(255, 0, 0)");
    expect(parseAnsi(`${ESC}[38;2;1;2;3mx`)[0].style.color).toBe("rgb(1, 2, 3)");
  });

  it("Minecraft § codes", () => {
    const pieces = parseAnsi("§aGreen §lBold§r plain");
    expect(pieces.map((one) => one.text)).toEqual(["Green ", "Bold", " plain"]);
    expect(pieces[0].style.color).toBe("#55ff55");
    expect(pieces[1].style.bold).toBe(true);
    expect(pieces[2].style).toEqual({});
  });

  it("drops non-color escapes", () => {
    expect(parseAnsi(`${ESC}[2Kend`).map((one) => one.text).join("")).toBe("end");
  });
});

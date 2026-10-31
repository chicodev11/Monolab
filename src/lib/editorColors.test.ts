import { describe, expect, it } from "vitest";
import { FIND, colourOf } from "./editorColors";

const colours = (text: string) => [...text.matchAll(FIND)].map((one) => colourOf(one as RegExpExecArray));

describe("colourOf", () => {
  it("reads hex, legacy codes and MiniMessage names", () => {
    expect(colours("color: '#ff8800' &aHi §c <gold> <color:dark_red> 0x00FF00")).toEqual(["#ff8800", "#55FF55", "#FF5555", "#FFAA00", "#AA0000", "#00FF00"]);
  });
  it("skips words and unknown tags", () => {
    expect(colours("#comment #12345 <bold> issue#abcdef").filter(Boolean)).toEqual([]);
  });
});

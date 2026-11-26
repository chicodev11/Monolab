import { describe, expect, it } from "vitest";
import { type AppGlyph, glyphPieces } from "./studioGlyphs";

const ruby = { id: "ruby", name: "ruby", image: "ruby", url: "data:,", height: 8, ascent: 7, width: 8, advance: 9, pictureWidth: 8, pictureHeight: 8 } satisfies AppGlyph;

describe("glyphPieces", () => {
  it("cuts a line where a known glyph is", () => {
    const pieces = glyphPieces([{ text: "Look :ruby: nice", color: "#ff0000" }], { ruby });
    expect(pieces).toEqual([{ runs: [{ text: "Look ", color: "#ff0000" }] }, { glyph: ruby }, { runs: [{ text: " nice", color: "#ff0000" }] }]);
  });

  it("keeps unknown names as written, even next to a known one", () => {
    expect(glyphPieces([{ text: "a :nope:ruby: b" }], { ruby })).toEqual([{ runs: [{ text: "a :nope" }] }, { glyph: ruby }, { runs: [{ text: " b" }] }]);
    expect(glyphPieces([{ text: "12:30:45" }], { ruby })).toEqual([{ runs: [{ text: "12:30" }, { text: ":45" }] }]);
  });
});

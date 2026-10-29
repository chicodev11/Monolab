import { describe, expect, it } from "vitest";
import { foldDiff, lineDiff } from "./lineDiff";

describe("lineDiff", () => {
  it("marks changed lines and keeps the rest", () => {
    const diff = lineDiff("a\r\nb\r\nc\r\nd", "a\nB\nc\nd\ne");
    expect(diff).toEqual([
      { kind: "same", text: "a" },
      { kind: "remove", text: "b" },
      { kind: "add", text: "B" },
      { kind: "same", text: "c" },
      { kind: "same", text: "d" },
      { kind: "add", text: "e" },
    ]);
  });

  it("gives up on a huge changed middle", () => {
    const many = (tag: string) => Array.from({ length: 300 }, (_, at) => `${tag}${at}`).join("\n");
    expect(lineDiff(many("x"), many("y"), 1000)).toBeNull();
  });

  it("folds long unchanged runs", () => {
    const before = Array.from({ length: 20 }, (_, at) => `line ${at}`).join("\n");
    const after = before.replace("line 10", "line ten");
    const rows = foldDiff(lineDiff(before, after) ?? []);
    expect(rows[0]).toEqual({ kind: "gap", count: 8 });
    expect(rows.filter((one) => one.kind !== "gap" && one.kind !== "same")).toHaveLength(2);
    expect(rows[rows.length - 1]).toEqual({ kind: "gap", count: 7 });
  });
});

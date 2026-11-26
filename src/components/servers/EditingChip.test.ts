import { describe, expect, it } from "vitest";
import { editingLine } from "./EditingChip";

describe("editingLine", () => {
  it("leaves me out", () => {
    expect(editingLine(["host"], "host")).toBeNull();
    expect(editingLine(["@amy", "host"], "host")).toBe("@amy is editing");
  });
  it("counts the others", () => {
    expect(editingLine(["@amy", "@bob", "host"], "host")).toBe("@amy and 1 more are editing");
  });
  it("names the host", () => {
    expect(editingLine(["host"], "@amy")).toBe("the host is editing");
  });
});

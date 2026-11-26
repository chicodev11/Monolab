import { describe, expect, it } from "vitest";
import { playerless, varsOf } from "./StudioRuleTest";

describe("rule test tool", () => {
  it("reads name=value lines as placeholders", () => {
    expect(varsOf("item = diamond\nprice=20\nno equals\n=empty name")).toEqual({ item: "diamond", price: "20" });
  });

  it("knows which events need no player", () => {
    expect(playerless("timer")).toBe(true);
    expect(playerless("shop:buy")).toBe(true);
    expect(playerless("chat")).toBe(false);
  });
});

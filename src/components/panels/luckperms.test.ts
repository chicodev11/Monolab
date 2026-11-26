import { describe, expect, it } from "vitest";
import { lp } from "./luckperms";

describe("lp commands", () => {
  it("builds group and user changes", () => {
    expect(lp.setPermission("lp", { kind: "group", name: "VIP" }, "essentials.home", true)).toBe("lp group vip permission set essentials.home true");
    expect(lp.unsetPermission("lpv", { kind: "user", name: "Mina_01" }, "a.b")).toBe("lpv user Mina_01 permission unset a.b");
    expect(lp.addParent("lp", { kind: "user", name: "Mina" }, "admin")).toBe("lp user Mina parent add admin");
    expect(lp.setWeight("lp", "vip", 10.4)).toBe("lp group vip setweight 10");
    expect(lp.setPrefix("lp", "vip", '<gold>"VIP"', 100)).toBe("lp group vip meta setprefix 100 \"<gold>'VIP'\"");
    expect(lp.setPrefix("lp", "vip", " ", 100)).toBe("lp group vip meta removeprefix 100");
  });
  it("refuses names with spaces", () => {
    expect(() => lp.setPermission("lp", { kind: "group", name: "a b" }, "x", true)).toThrow();
    expect(() => lp.setPermission("lp", { kind: "group", name: "a" }, "x y", true)).toThrow();
    expect(() => lp.createGroup("lp", "x;op")).toThrow();
  });
});

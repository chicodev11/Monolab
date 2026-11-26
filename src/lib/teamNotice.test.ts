import { describe, expect, it } from "vitest";
import { DEFAULT_SWITCHES, wantsNotice } from "./teamNotice";

const notice = (kind: string, to = "") => ({ serverId: "s", kind, to, text: "x" });

describe("wantsNotice", () => {
  it("shows_only_mine_or_everyone", () => {
    expect(wantsNotice(notice("assigned", "@amy"), "@amy", DEFAULT_SWITCHES)).toBe(true);
    expect(wantsNotice(notice("assigned", "@amy"), "@bob", DEFAULT_SWITCHES)).toBe(false);
    expect(wantsNotice(notice("assetAdded"), "@bob", DEFAULT_SWITCHES)).toBe(true);
  });
  it("respects_switches", () => {
    expect(wantsNotice(notice("assetAdded"), "@bob", { ...DEFAULT_SWITCHES, assetAdded: false })).toBe(false);
    expect(wantsNotice(notice("proposalNew"), "@bob", { ...DEFAULT_SWITCHES, proposals: false })).toBe(false);
    expect(wantsNotice(notice("unknown"), "@bob", DEFAULT_SWITCHES)).toBe(false);
  });
});

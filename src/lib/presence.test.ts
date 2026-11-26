import { describe, expect, it } from "vitest";
import { discordPresence } from "./presence";

describe("discordPresence", () => {
  it("says when nothing runs", () => {
    expect(discordPresence([], 5)).toEqual({ details: "No servers running", state: "Monolab", startedAt: null });
  });
  it("shows core and version only", () => {
    const shown = discordPresence([{ core: "Paper", version: "1.21.4" }], 5);
    expect(shown).toEqual({ details: "Hosting 1 server", state: "Paper 1.21.4", startedAt: 5 });
  });
  it("sums up many servers", () => {
    const shown = discordPresence(
      [
        { core: "Paper", version: "1.21.4" },
        { core: "Fabric", version: "1.20.1" },
        { core: "Paper", version: "1.21.4" },
        { core: "Velocity", version: "3.4" },
      ],
      5,
    );
    expect(shown.details).toBe("Hosting 4 servers");
    expect(shown.state).toBe("Paper 1.21.4 · Fabric 1.20.1 …");
  });
});

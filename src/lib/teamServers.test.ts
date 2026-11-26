import { describe, expect, it } from "vitest";
import { askWords } from "../components/dialogs/TeamAgentAsks";
import { isSharedRef, sharedRef } from "./teamServers";

describe("teammates' agents", () => {
  it("names a shared server the way the app reads it back", () => {
    const ref = sharedRef("01JTEAM", "K7QX-M2PA", "srv-1");
    expect(ref).toBe("team~01JTEAM~K7QX-M2PA~srv-1");
    expect(isSharedRef(ref)).toBe(true);
    expect(isSharedRef("srv-1")).toBe(false);
  });

  it("says in words what an agent wants to do", () => {
    expect(askWords({ command: "save_editable_file", path: "server.properties", text: "" })).toBe("save server.properties");
    expect(askWords({ command: "send_console_command", path: "", text: "op bob" })).toBe("run the console command “op bob”");
    expect(askWords({ command: "save_world_settings", path: "", text: "" })).toBe("change the world settings");
    expect(askWords({ command: "something_new", path: "", text: "" })).toBe("use “something_new”");
  });
});

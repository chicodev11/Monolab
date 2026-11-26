import { describe, expect, it } from "vitest";
import { shownServerName, withShownNames } from "./serverName";

describe("default server names", () => {
  it("shows the default name in the app's language and leaves picked names alone", () => {
    expect(shownServerName("\uC11C\uBC84 3")).toBe("Server 3");
    expect(shownServerName("Server 2")).toBe("Server 2");
    expect(shownServerName("\uC11C\uBC84 3", true)).toBe("\uC11C\uBC84 3");
    expect(shownServerName("skyblock")).toBe("skyblock");
    expect(shownServerName("Server 2 copy")).toBe("Server 2 copy");
  });

  it("keeps the same object when nothing changes", () => {
    const list = [{ name: "skyblock" }, { name: "\uC11C\uBC84 1", named: false }];
    const shown = withShownNames(list);
    expect(shown[0]).toBe(list[0]);
    expect(shown[1].name).toBe("Server 1");
  });
});

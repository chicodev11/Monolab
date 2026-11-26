import { expect, it } from "vitest";
import { settingLabel, settingValue } from "./settingsImport";

it("reads setting values", () => {
  expect(settingValue("true")).toBe("on");
  expect(settingValue('""')).toBe("default");
  expect(settingValue('"D:\\\\Backups"')).toBe("D:\\Backups");
  expect(settingValue("8192")).toBe("8192");
  expect(settingLabel("defaultMemoryMib")).toBe("Default memory for new servers");
  expect(settingLabel("unknownKey")).toBe("unknownKey");
});

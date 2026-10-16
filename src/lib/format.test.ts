import ko from "../lang/ko.json";
import { afterAll, beforeAll, describe, expect, it, test } from "vitest";
import { setLanguage } from "./i18n";
import { bytes, clock, date, fileSize, isImage, isProxy, realVersion, relativeTime, statusInfo, today, uptime, used } from "./format";

describe("bytes converts MiB to GB", () => {
  test.each([
    [4096, "4 GB"],
    [1536, "1.5 GB"],
    [512, "0.5 GB"],
    [0, "0 GB"],
    [null, "—"],
    [undefined, "—"],
    [Number.NaN, "—"],
  ])("%s → %s", (mib, text) => {
    expect(bytes(mib)).toBe(text);
  });
});

test("used has no unit", () => {
  expect(used(4096)).toBe("4");
  expect(used(1536)).toBe("1.5");
  expect(used(null)).toBe("—");
});

describe("uptime", () => {
  beforeAll(() => setLanguage("en"));
  test.each([
    [undefined, "—"],
    [59, "0 min"],
    [60, "1 min"],
    [3600, "1 h 0 min"],
    [3661, "1 h 1 min"],
    [90000, "25 h 0 min"],
  ])("%s s → %s", (seconds, text) => {
    expect(uptime(seconds)).toBe(text);
  });
});

test("isProxy", () => {
  expect(isProxy("Velocity")).toBe(true);
  expect(isProxy("BungeeCord")).toBe(true);
  expect(isProxy("Paper")).toBe(false);
});

test("clock formats h:m:s (shape only, values depend on time zone)", () => {
  expect(clock(Date.UTC(2026, 0, 1, 12, 34, 56))).toMatch(/^\d{2}:\d{2}:\d{2}$/);
});

test("today returns local YYYY-MM-DD", () => {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  expect(today()).toBe(`${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`);
});

describe("fileSize", () => {
  test.each([
    [0, "0 B"],
    [512, "512 B"],
    [1536, "1.5 KB"],
    [482_800, "471.5 KB"],
    [5 * 1024 * 1024, "5.0 MB"],
    [3 * 1024 * 1024 * 1024, "3.00 GB"],
    [-1, "—"],
    [null, "—"],
    [undefined, "—"],
    [Number.NaN, "—"],
  ])("%s → %s", (bytes, text) => {
    expect(fileSize(bytes)).toBe(text);
  });
});

describe("statusInfo returns label, tone and shape", () => {
  beforeAll(() => setLanguage("en"));
  test("four states", () => {
    expect(statusInfo("running")).toEqual({ label: "Running", tone: "ok", shape: "circle" });
    expect(statusInfo("stopping")).toEqual({ label: "Stopping", tone: "warn", shape: "square" });
    expect(statusInfo("crashed")).toEqual({ label: "Crashed", tone: "danger", shape: "diamond" });
    expect(statusInfo("stopped")).toEqual({ label: "Stopped", tone: "muted", shape: "ring" });
  });
  test("unknown values look stopped", () => {
    expect(statusInfo("???")).toEqual(statusInfo("stopped"));
  });
});

describe("relativeTime turns seconds into a distance from now", () => {
  const now = Date.UTC(2026, 8, 15, 12, 0, 0);
  const at = (secondsAgo: number) => now / 1000 - secondsAgo;

  describe("English", () => {
    beforeAll(() => setLanguage("en"));
    test.each([
      [0, "just now"],
      [44, "just now"],
      [45, "1 min ago"],
      [90, "2 min ago"],
      [3600, "1 h ago"],
      [86_400 * 3, "3 d ago"],
      [86_400 * 45, "2 mo ago"],
      [86_400 * 400, "1 y ago"],
      [-3600, "in 1 h"],
      [-86_400 * 2, "in 2 d"],
    ])("%s s ago → %s", (secondsAgo, text) => {
      expect(relativeTime(at(secondsAgo), now)).toBe(text);
    });
  });

  describe("Korean", () => {
    beforeAll(() => setLanguage("ko"));
    afterAll(() => setLanguage("en"));
    test.each([
      [10, "just now", ""],
      [120, "{0} min ago", "2"],
      [7200, "{0} h ago", "2"],
      [86_400 * 3, "{0} d ago", "3"],
      [86_400 * 60, "{0} mo ago", "2"],
      [86_400 * 800, "{0} y ago", "2"],
      [-600, "in {0} min", "10"],
    ])("%s s ago → %s", (secondsAgo, key, value) => {
      expect(relativeTime(at(secondsAgo), now)).toBe((ko as Record<string, string>)[key].replace("{0}", value));
    });
  });
});

describe("date follows the UI language", () => {
  const at = Date.UTC(2026, 8, 15, 12, 0, 0) / 1000;
  test("English uses en-US", () => {
    setLanguage("en");
    expect(date(at)).toMatch(/^\d{1,2}\/\d{1,2}\/\d{2}, \d{1,2}:\d{2} [AP]M$/);
  });
  test("Korean uses ko-KR", () => {
    setLanguage("ko");
    expect(date(at)).toMatch(/^\d{2}\. \d{1,2}\. \d{1,2}\. \S+ \d{1,2}:\d{2}$/);
    setLanguage("en");
  });
});

describe("realVersion", () => {
  it("only accepts release-style versions", () => {
    expect(realVersion("1.21.8")).toBe(true);
    expect(realVersion("26.3")).toBe(true);
    expect(realVersion("25w14a")).toBe(true);
    expect(realVersion("1.21.4-pre1")).toBe(true);
  });
  it("rejects made-up labels", () => {
    expect(realVersion("Needs review")).toBe(false);
    expect(realVersion("")).toBe(false);
    expect(realVersion(null)).toBe(false);
  });
});

describe("isImage decides what opens as a picture", () => {
  it("is true only for picture extensions", () => {
    expect(isImage("server-icon.png")).toBe(true);
    expect(isImage("logs/a/b/pack.PNG")).toBe(true);
    expect(isImage("shot.jpeg")).toBe(true);
  });
  it("is false for text files and jars", () => {
    expect(isImage("server.properties")).toBe(false);
    expect(isImage("paper.jar")).toBe(false);
    expect(isImage("Dockerfile")).toBe(false);
  });
});

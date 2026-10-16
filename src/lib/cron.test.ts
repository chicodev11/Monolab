import ko from "../lang/ko.json";
import { beforeAll, describe, expect, it } from "vitest";
import { describeCron, fromCron, toCron } from "./cron";
import { setLanguage } from "./i18n";

const base = { minute: "0", hour: "0", day_of_month: "*", month: "*", day_of_week: "*" };

describe("schedule time between simple shapes and cron", () => {
  beforeAll(() => setLanguage("ko"));
  it("round-trips simple schedules through cron", () => {
    const daily = toCron({ kind: "daily", hour: 4, minute: 30 }, base);
    expect(daily).toEqual({ ...base, minute: "30", hour: "4" });
    expect(fromCron(daily)).toEqual({ kind: "daily", hour: 4, minute: 30 });
    expect(toCron({ kind: "hours", every: 6 }, base).hour).toBe("*/6");
    expect(fromCron({ ...base, minute: "*/15", hour: "*" })).toEqual({ kind: "minutes", every: 15 });
    expect(fromCron({ ...base, minute: "0", hour: "5", day_of_week: "1,5" })).toEqual({ kind: "weekly", days: [1, 5], hour: 5, minute: 0 });
  });
  it("unknown shapes stay as raw cron", () => {
    expect(fromCron({ ...base, day_of_month: "1" }).kind).toBe("custom");
    expect(describeCron({ ...base, day_of_month: "1" })).toBe("0 0 1 * *");
    expect(describeCron({ ...base, minute: "0", hour: "4" })).toBe(ko["Daily at {0}"].replace("{0}", "04:00"));
  });
});

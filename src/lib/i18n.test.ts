import { beforeAll, describe, expect, test } from "vitest";
import { ko } from "../test/lang";
import { rustKo } from "../test/lang";
import { format, language, setLanguage, t, tr, trBack } from "./i18n";

test("defaults to ko", () => {
  localStorage.removeItem("monolab-language");
  expect(language()).toBe("ko");
});

describe("English", () => {
  beforeAll(() => setLanguage("en"));
  test("tr returns the English key and fills placeholders", () => {
    expect(tr("Server imported.")).toBe("Server imported.");
    expect(tr("Deleted {0}.", ["x"])).toBe("Deleted x.");
  });
  test("trBack leaves Rust messages alone", () => {
    expect(trBack("Server not found.")).toBe("Server not found.");
  });
  test("t uses the legacy dictionary", () => {
    expect(t("status.running")).not.toBe("status.running");
    expect(t("no.such.key")).toBe("no.such.key");
  });
});

describe("after switching to Korean", () => {
  test("tr and trBack use ko.json and rust.ko.json", async () => {
    await setLanguage("ko");
    expect(language()).toBe("ko");
    expect(tr("Server imported.")).toBe(ko["Server imported."]);
    expect(tr("Deleted {0}.", ["x"])).toBe(ko["Deleted {0}."].replace("{0}", "x"));
    expect(tr("no such text")).toBe("no such text");
    expect(trBack("Server not found.")).toBe(rustKo["Server not found."]);
    expect(trBack("playit error: NotAllowed")).toBe(rustKo["playit error: {}"].replace("$1", "NotAllowed"));
    expect(trBack("nothing like this")).toBe("nothing like this");
    await setLanguage("en");
  });
});

describe("format handles the ICU plural subset", () => {
  beforeAll(() => setLanguage("en"));
  test("placeholders and number", () => {
    expect(format("{0} and {1}", { 0: "a", 1: 2 }, "en")).toBe("a and 2");
    expect(format("{0, number}", { 0: 1900 }, "en")).toBe("1,900");
    expect(format("{x} {0}", { 0: 1 }, "en")).toBe("{x} 1");
  });
  test("plural: English uses one/other, Korean always other, =0 first", () => {
    const msg = "{0, plural, =0 {no servers} one {# server} other {# servers}}";
    expect(format(msg, { 0: 0 }, "en")).toBe("no servers");
    expect(format(msg, { 0: 1 }, "en")).toBe("1 server");
    expect(format(msg, { 0: 1900 }, "en")).toBe("1,900 servers");
    expect(format(msg, { 0: 1 }, "ko")).toBe("1 servers");
    expect(format(msg, { 0: "1,900" }, "en")).toBe("1,900 servers");
  });
  test("select, nesting and quote escapes", () => {
    expect(format("{g, select, m {he} f {she} other {they}}", { g: "f" }, "en")).toBe("she");
    expect(format("{0, plural, one {# item for {1}} other {# items for {1}}}", { 0: 2, 1: "x" }, "en")).toBe("2 items for x");
    expect(format("'{'literal'}' {0}", { 0: 1 }, "en")).toBe("{literal} 1");
    expect(format("won't {0}", { 0: 1 }, "en")).toBe("won't 1");
  });
  test("malformed text is returned as is", () => {
    expect(format("{0, plural, one {# a}", { 0: 1 }, "en")).toBe("{0, plural, one {# a}");
    expect(format("{0, plural, one {# a}}", { 0: 2 }, "en")).toBe("{0, plural, one {# a}}");
  });
  test("tr and t with the real en.json", () => {
    const chunk = "{0, plural, one {# entity} other {# entities}} in one chunk";
    expect(tr(chunk, [1])).toBe("1 entity in one chunk");
    expect(tr(chunk, ["1,900"])).toBe("1,900 entities in one chunk");
    expect(t("top.subtitle", { total: 1, running: 0 })).toBe("1 server · 0 running");
  });
});

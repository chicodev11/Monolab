import { beforeEach, expect, test } from "vitest";
import { applyTheme, readTheme, resolvedTheme } from "./theme";

beforeEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.theme;
});

test("defaults to dark", () => {
  expect(readTheme()).toBe("dark");
});

test("applyTheme sets <html data-theme> and remembers it", () => {
  applyTheme("light");
  expect(document.documentElement.dataset.theme).toBe("light");
  expect(localStorage.getItem("monolab-theme")).toBe("light");
  expect(readTheme()).toBe("light");
});

test("system falls back to dark unless matchMedia says light", () => {
  expect(resolvedTheme("system")).toBe("dark");
  applyTheme("system");
  expect(document.documentElement.dataset.theme).toBe("dark");
  expect(readTheme()).toBe("system");
});

test("unknown values fall back to dark", () => {
  localStorage.setItem("monolab-theme", "neon");
  expect(readTheme()).toBe("dark");
});

test("oled theme is remembered too", () => {
  applyTheme("oled");
  expect(document.documentElement.dataset.theme).toBe("oled");
  expect(readTheme()).toBe("oled");
});

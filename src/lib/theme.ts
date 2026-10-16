export type Theme = "dark" | "light" | "oled" | "system";

const KEY = "monolab-theme";
const system = window.matchMedia("(prefers-color-scheme: light)");

export function readTheme(): Theme {
  try {
    const saved = localStorage.getItem(KEY);
    return saved === "light" || saved === "system" || saved === "oled" ? saved : "dark";
  } catch {
    return "dark";
  }
}

export function resolvedTheme(theme: Theme): "dark" | "light" | "oled" {
  if (theme === "system") return system.matches ? "light" : "dark";
  return theme;
}

export function applyTheme(theme: Theme) {
  document.documentElement.dataset.theme = resolvedTheme(theme);
  try {
    localStorage.setItem(KEY, theme);
  } catch {}
}

export function followSystem(get: () => Theme) {
  const onChange = () => {
    if (get() === "system") applyTheme("system");
  };
  system.addEventListener("change", onChange);
  return () => system.removeEventListener("change", onChange);
}

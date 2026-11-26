import { tr } from "./i18n";

export type LanguageKind = "json" | "yaml" | "toml" | "properties" | "shell" | null;

/** Which editor language a file uses, without loading the editor. */
export function languageKind(path: string): LanguageKind {
  switch (path.toLowerCase().split(".").pop() ?? "") {
    case "json":
    case "json5":
    case "mcmeta":
      return "json";
    case "yml":
    case "yaml":
      return "yaml";
    case "toml":
      return "toml";
    case "properties":
    case "cfg":
    case "conf":
    case "ini":
    case "lang":
      return "properties";
    case "sh":
    case "bash":
      return "shell";
    default:
      return null;
  }
}

const NAMES: Record<Exclude<LanguageKind, null>, string> = { json: "JSON", yaml: "YAML", toml: "TOML", properties: "Properties", shell: "Shell" };

export function languageName(path: string): string {
  const kind = languageKind(path);
  return kind ? NAMES[kind] : tr("Plain text");
}

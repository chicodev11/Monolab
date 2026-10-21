import type { PanelKind } from "./bindings";
import { tr } from "./i18n";

const NAMES: Record<PanelKind, string> = {
  hydrodactyl: "Hydrodactyl",
  pterodactyl: "Pterodactyl",
  pelican: "Pelican",
};

export function panelKindName(kind: PanelKind | undefined): string {
  return NAMES[kind ?? "hydrodactyl"];
}

export function panelsName(hosts: { kind?: PanelKind }[]): string {
  const kinds = new Set(hosts.map((one) => one.kind ?? "hydrodactyl"));
  return kinds.size === 1 ? panelKindName([...kinds][0]) : tr("Panels");
}

import type { DiscordPresence } from "./bindings";
import { tr } from "./i18n";

type Running = { core: string; version: string };

/** What Discord shows: how many servers run and their core and version. Never names, addresses or ports. */
export function discordPresence(running: readonly Running[], startedAt: number): DiscordPresence {
  if (!running.length) return { details: tr("No servers running"), state: tr("Monolab"), startedAt: null };
  const kinds = [...new Set(running.map((one) => [one.core, one.version].filter(Boolean).join(" ")))];
  return {
    details: running.length === 1 ? tr("Hosting 1 server") : tr("Hosting {0} servers", [running.length]),
    state: kinds.length === 1 ? kinds[0] : kinds.slice(0, 2).join(" · ") + (kinds.length > 2 ? " …" : ""),
    startedAt,
  };
}

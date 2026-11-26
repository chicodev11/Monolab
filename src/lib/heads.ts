import { useSyncExternalStore } from "react";
import { commands } from "./ipc";

const known = new Map<string, string | null>();
const asking = new Set<string>();
const listeners = new Set<() => void>();
let version = 0;

function tell() {
  version += 1;
  for (const one of listeners) one();
}

function ask(name: string) {
  if (known.has(name) || asking.has(name)) return;
  asking.add(name);
  commands
    .lookupMojangProfile(name)
    .then((profile) => known.set(name, profile.head ?? null))
    .catch(() => known.set(name, null))
    .finally(() => {
      asking.delete(name);
      tell();
    });
}

const NAME = /^[A-Za-z0-9_]{3,16}$/;

export function useHead(name: string): string | null {
  useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => version,
  );
  if (!NAME.test(name)) return null;
  if (!known.has(name)) ask(name);
  return known.get(name) ?? null;
}

import { useSyncExternalStore } from "react";
import type { ServerLogEvent } from "./bindings";
import type { Sent } from "./sent";

export type LogLine = Sent<ServerLogEvent>;

export const LIMIT = 10_000;

const EMPTY: readonly LogLine[] = Object.freeze([]);
const lines = new Map<string, readonly LogLine[]>();
const listeners = new Set<() => void>();

function notify() {
  for (const listener of listeners) listener();
}

function set(serverId: string, next: readonly LogLine[]) {
  lines.set(serverId, next.length > LIMIT ? next.slice(next.length - LIMIT) : next);
  notify();
}

export const logStore = {
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  },
  get(serverId: string): readonly LogLine[] {
    return lines.get(serverId) ?? EMPTY;
  },
  append(batch: readonly LogLine[]) {
    if (!batch.length) return;
    const grouped = new Map<string, LogLine[]>();
    for (const one of batch) {
      const list = grouped.get(one.serverId);
      if (list) list.push(one);
      else grouped.set(one.serverId, [one]);
    }
    for (const [serverId, fresh] of grouped) {
      const now = lines.get(serverId) ?? EMPTY;
      const next = now.concat(fresh);
      lines.set(serverId, next.length > LIMIT ? next.slice(next.length - LIMIT) : next);
    }
    notify();
  },
  replace(serverId: string, next: readonly LogLine[]) {
    set(serverId, next);
  },
  clear(serverId: string) {
    if (!lines.has(serverId)) return;
    lines.delete(serverId);
    notify();
  },
  reset() {
    lines.clear();
    notify();
  },
};

export function useServerLogs(serverId: string | null | undefined): readonly LogLine[] {
  return useSyncExternalStore(
    logStore.subscribe,
    () => (serverId ? logStore.get(serverId) : EMPTY),
    () => EMPTY,
  );
}

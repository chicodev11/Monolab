import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";
import { applyLine, emptyChat, settle, type ChatModel, type ChatUsage } from "./claudeChat";

type Line = { id: string; line?: string; exited?: boolean };
export type ChatEntry = { chatId: string | null; model: ChatModel; ended: boolean };

const EMPTY: ChatEntry = { chatId: null, model: emptyChat, ended: false };
const entries = new Map<string, ChatEntry>();
const listeners = new Set<() => void>();
const USAGE_KEY = "monolab-claude-usage";
const accountOf = new Map<string, string>();
export function setServerAccount(server: string, account: string) {
  accountOf.set(server, account);
}
function usageKey(server: string) {
  const account = accountOf.get(server) ?? "default";
  return account === "default" ? USAGE_KEY : `${USAGE_KEY}:${account}`;
}

function tell() {
  for (const one of listeners) one();
}

export function lastUsage(server: string): { usage: ChatUsage; at: number } | null {
  try {
    const raw = localStorage.getItem(usageKey(server));
    return raw ? (JSON.parse(raw) as { usage: ChatUsage; at: number }) : null;
  } catch {
    return null;
  }
}

function keepUsage(server: string, usage: ChatUsage | undefined) {
  if (!usage || (!usage.fiveHour && !usage.sevenDay)) return;
  try {
    localStorage.setItem(usageKey(server), JSON.stringify({ usage, at: Date.now() }));
  } catch {}
}

let listening = false;
export function startChatListener() {
  if (listening) return;
  listening = true;
  void listen<Line>("claude-chat", (event) => {
    for (const [server, entry] of entries) {
      if (entry.chatId !== event.payload.id) continue;
      let model = entry.model;
      let ended = entry.ended;
      if (event.payload.line) model = applyLine(model, event.payload.line);
      if (event.payload.exited) {
        ended = true;
        model = settle(model);
      }
      if (model.usage !== entry.model.usage) keepUsage(server, model.usage);
      entries.set(server, { ...entry, model, ended });
      tell();
      return;
    }
  }).catch(() => {
    listening = false;
  });
}

export function chatEntry(server: string): ChatEntry {
  return entries.get(server) ?? EMPTY;
}

export function setChatEntry(server: string, change: Partial<ChatEntry> | ((now: ChatEntry) => Partial<ChatEntry>)) {
  const now = chatEntry(server);
  const patch = typeof change === "function" ? change(now) : change;
  const next = { ...now, ...patch };
  if (next.model.usage !== now.model.usage) keepUsage(server, next.model.usage);
  entries.set(server, next);
  tell();
}

export function useChatEntry(server: string): ChatEntry {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => chatEntry(server),
  );
}

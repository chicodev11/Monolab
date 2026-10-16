
import { tr } from "./i18n";
export const onWeb = !(
  "__TAURI_INTERNALS__" in window || "__TAURI__" in window
);

const KEY = new URLSearchParams(location.search).get("key") ?? "";

let session = sessionStorage.getItem("se-session") ?? "";

export function webSession() {
  return session;
}

export function setWebSession(value: string) {
  session = value;
  if (value) sessionStorage.setItem("se-session", value);
  else sessionStorage.removeItem("se-session");
}

export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!onWeb) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(command, args);
  }
  const answer = await fetch(
    `/api/${command}?key=${encodeURIComponent(KEY)}`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-SE-Session": session,
      },
      body: JSON.stringify(args ?? {}),
    },
  );
  const data = await answer.json().catch(() => ({}));
  if (!answer.ok || data?.error) {
    if (data?.error) throw data.error;
    if (answer.status === 503) {
      throw tr("The app is busy with another task. Try again when it finishes.");
    }
    if (answer.status === 401) {
      throw tr("You were signed out. Please sign in again.");
    }
    if (answer.status === 403) {
      throw tr("You don't have permission to manage this server.");
    }
    throw tr("Couldn't call {0}. ({1})", [command, answer.status]);
  }
  return data?.value as T;
}

export async function listenTo<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (!onWeb) {
    const { listen } = await import("@tauri-apps/api/event");
    const stop = await listen<T>(event, (one) => handler(one.payload));
    return stop;
  }
  const list = webListeners.get(event) ?? [];
  list.push(handler as (payload: unknown) => void);
  webListeners.set(event, list);
  startPolling();
  return () => {
    const now = webListeners.get(event) ?? [];
    webListeners.set(
      event,
      now.filter((one) => one !== (handler as (payload: unknown) => void)),
    );
  };
}

const webListeners = new Map<string, ((payload: unknown) => void)[]>();
let polling = 0;

function startPolling() {
  if (polling || !onWeb) return;
  polling = 1;
  let after = 0;
  void (async () => {
    let quiet = 0;
    for (;;) {
      try {
        const answer = await fetch(`/api-events?after=${after}`, {
          headers: { "X-SE-Session": session },
          cache: "no-store",
        });
        if (!answer.ok) throw new Error(String(answer.status));
        const data = await answer.json();
        after = data.at ?? after;
        quiet = 0;
        for (const one of data.events ?? []) {
          for (const handler of webListeners.get(one.event) ?? []) {
            handler(one.payload);
          }
        }
      } catch {
        quiet = Math.min(quiet + 1, 10);
        await new Promise((done) => setTimeout(done, quiet * 500));
      }
    }
  })();
}

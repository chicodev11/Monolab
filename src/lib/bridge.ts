
import { tr } from "./i18n";
const inApp = "__TAURI_INTERNALS__" in window || "__TAURI__" in window;
/** This webview's label ("main", or "monolink-…" for a team view opened with `monolink_open` inside the Monolab window). */
const windowLabel: string =
  (window as unknown as { __TAURI_INTERNALS__?: { metadata?: { currentWebview?: { label?: string } } } }).__TAURI_INTERNALS__?.metadata?.currentWebview
    ?.label ?? "";
/**
 * A team view showing a teammate's servers through Monolink: the web panel's screens, carried by this app
 * in a webview of its own over the Monolab window. Only that webview (the app limits what it may call)
 * becomes one, never the main one.
 */
export const monolinkPeer = inApp && windowLabel.startsWith("monolink-") ? (new URLSearchParams(location.search).get("monolink") ?? "") : "";
export const onMonolink = monolinkPeer !== "";
/** The host's name, for the window title. */
export const monolinkHost = onMonolink ? (new URLSearchParams(location.search).get("name") ?? "") : "";
/** The team this window reaches the host through (its role grant). */
export const monolinkTeam = onMonolink ? (new URLSearchParams(location.search).get("team") ?? "") : "";
export const onWeb = !inApp || onMonolink;

async function viaMonolink<T>(command: string, body: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  const text = await invoke<string>("monolink_call", { peer: monolinkPeer, command, body: JSON.stringify(body), team: monolinkTeam });
  return JSON.parse(text) as T;
}

/** Who else is on a server right now: other teammates, and "host" while the host's own window shows it (team windows only). */
export function monolinkPresence(id: string) {
  return viaMonolink<string[]>("__presence", { id });
}

/** Who this window is signed in as on the host (Monolink windows only). */
export function monolinkWhoAmI() {
  return viaMonolink<{ name: string; role: string; teamRole?: string; node?: boolean }>("__whoami", {});
}

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
  if (onMonolink) return viaMonolink<T>(command, args ?? {});
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
        let data: { at?: number; events?: { event: string; payload: unknown }[] };
        if (onMonolink) {
          data = await viaMonolink("__events", { after });
        } else {
          const answer = await fetch(`/api-events?after=${after}`, {
            headers: { "X-SE-Session": session },
            cache: "no-store",
          });
          if (!answer.ok) throw new Error(String(answer.status));
          data = await answer.json();
        }
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

import { useEffect, useState } from "react";
import type { WebhookInfo } from "../../lib/bindings";
import { tr, trBack } from "../../lib/i18n";
import { commands } from "../../lib/ipc";

const EVENTS = ["crash", "lag", "join", "leave", "task", "server"] as const;

function eventLabel(event: string): { title: string; hint: string } {
  switch (event) {
    case "crash":
      return { title: tr("Crashes"), hint: tr("The server process stopped by itself, or auto-restart gave up.") };
    case "lag":
      return { title: tr("Lag"), hint: tr("TPS below 15 for a minute.") };
    case "join":
      return { title: tr("Players join"), hint: tr("Someone joined a server.") };
    case "leave":
      return { title: tr("Players leave"), hint: tr("Someone left a server.") };
    case "task":
      return { title: tr("Task alerts"), hint: tr("A scheduled task that sends an alert.") };
    default:
      return { title: tr("Server news"), hint: tr("Updates applied when a server started.") };
  }
}

/**
 * Alerts to a Discord channel through a webhook. The address stays in this PC's sealed secrets; this screen only
 * learns whether one is set.
 */
export function DiscordAlerts({ servers, onNotice, onError }: { servers: { id: string; name: string }[]; onNotice: (text: string) => void; onError: (text: string) => void }) {
  const [info, setInfo] = useState<WebhookInfo | null>(null);
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    void commands.webhookInfo().then(setInfo).catch(() => undefined);
  }, []);
  if (!info) return null;
  const save = async (change: { url?: string | null; events?: string[]; servers?: string[] }) => {
    setBusy(true);
    try {
      const next = await commands.setWebhook(change.url ?? null, change.events ?? info.events, change.servers ?? info.servers);
      setInfo(next);
      if (change.url !== undefined) {
        setUrl("");
        onNotice(change.url ? tr("Webhook saved.") : tr("Webhook removed."));
      }
    } catch (error) {
      onError(trBack(String(error)));
    } finally {
      setBusy(false);
    }
  };
  const toggle = (list: string[], one: string, on: boolean) => (on ? [...new Set([...list, one])] : list.filter((item) => item !== one));
  return (
    <div className="setting-row discord-alerts">
      <div>
        <strong>{tr("Discord alerts")}</strong>
        <small>
          {info.set
            ? tr("Sending to webhook {0}. Anyone with the address can post in that channel, so it stays on this PC only.", [info.hint])
            : tr("Paste a channel's webhook address (channel settings > Integrations > Webhooks) to get alerts there.")}
        </small>
      </div>
      <form
        className="discord-url"
        onSubmit={(event) => {
          event.preventDefault();
          if (url.trim()) void save({ url: url.trim(), events: info.events.length ? info.events : ["crash", "lag"] });
        }}
      >
        <input type="password" value={url} onChange={(event) => setUrl(event.target.value)} placeholder="https://discord.com/api/webhooks/…" autoComplete="off" spellCheck={false} aria-label={tr("Webhook address")} />
        <button type="submit" className="button small primary" disabled={busy || !url.trim()}>
          {info.set ? tr("Replace") : tr("Save")}
        </button>
        {info.set && (
          <>
            <button
              type="button"
              className="button small secondary"
              disabled={busy}
              onClick={() =>
                void commands
                  .testWebhook()
                  .then(() => onNotice(tr("Sent a test message.")))
                  .catch((error: unknown) => onError(trBack(String(error))))
              }
            >
              {tr("Send a test")}
            </button>
            <button type="button" className="button small ghost" disabled={busy} onClick={() => void save({ url: "" })}>
              {tr("Remove")}
            </button>
          </>
        )}
      </form>
      {info.set && (
        <>
          <div className="role-perms" role="group" aria-label={tr("What to send")}>
            {EVENTS.map((event) => {
              const on = info.events.includes(event);
              const label = eventLabel(event);
              return (
                <label key={event} className={`role-perm${on ? " on" : ""}`}>
                  <input type="checkbox" className="role-check" checked={on} disabled={busy} onChange={(change) => void save({ events: toggle(info.events, event, change.target.checked) })} />
                  <span>
                    <strong>{label.title}</strong>
                    <small className="muted">{label.hint}</small>
                  </span>
                </label>
              );
            })}
          </div>
          {servers.length > 1 && (
            <div className="role-perms" role="group" aria-label={tr("Servers")}>
              {servers.map((server) => {
                const on = info.servers.length === 0 || info.servers.includes(server.id);
                return (
                  <label key={server.id} className={`role-perm${on ? " on" : ""}`}>
                    <input
                      type="checkbox"
                      className="role-check"
                      checked={on}
                      disabled={busy}
                      onChange={(change) => {
                        const all = info.servers.length ? info.servers : servers.map((one) => one.id);
                        const next = toggle(all, server.id, change.target.checked);
                        if (next.length === 0) return;
                        // Every server ticked is the same as "all", which also covers servers added later.
                        void save({ servers: next.length === servers.length ? [] : next });
                      }}
                    />
                    <span>
                      <strong>{server.name}</strong>
                    </span>
                  </label>
                );
              })}
            </div>
          )}
        </>
      )}
    </div>
  );
}

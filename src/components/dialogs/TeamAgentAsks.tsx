import * as RadixDialog from "@radix-ui/react-dialog";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type { AgentAsk } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { shownServerName } from "../../lib/serverName";

/** What a teammate's agent wants to do, in words. */
export function askWords(ask: Pick<AgentAsk, "command" | "path" | "text">): string {
  const path = ask.path || tr("a file");
  switch (ask.command) {
    case "save_editable_file":
      return tr("save {0}", [path]);
    case "create_server_file":
      return tr("create the file {0}", [path]);
    case "create_server_folder":
      return tr("create the folder {0}", [path]);
    case "trash_server_path":
      return tr("move {0} to the server trash", [path]);
    case "rename_server_path":
      return tr("rename {0}", [path]);
    case "send_console_command":
    case "send_rcon_command":
      return tr("run the console command “{0}”", [ask.text]);
    case "install_addon":
    case "install_modrinth":
    case "install_addon_with_dependencies":
      return tr("install a plugin or mod");
    case "toggle_addon":
      return tr("turn {0} on or off", [path]);
    case "delete_addon":
      return tr("remove {0}", [path]);
    case "remove_addon":
      return tr("remove the Monolab add-on");
    case "save_server_properties":
      return tr("change server.properties");
    case "save_world_settings":
      return tr("change the world settings");
    case "start_instance":
      return tr("start the server");
    case "stop_instance":
      return tr("stop the server");
    case "restart_instance":
      return tr("restart the server");
    case "create_backup":
      return tr("make a backup");
    case "save_server_agents_md":
      return tr("change the server's AGENTS.md");
    default:
      return tr("use “{0}”", [ask.command]);
  }
}

/**
 * Asks the host, one at a time, whether a teammate's agent may make a change on one of this PC's
 * servers. Reads never come here; they pass straight away. Closing the question says no.
 */
export function TeamAgentAsks({ onError, onShow, onAway }: { onError: (message: string) => void; onShow?: () => void; onAway?: (ask: AgentAsk) => void }) {
  const [asks, setAsks] = useState<AgentAsk[]>([]);
  const [busy, setBusy] = useState(false);
  const [more, setMore] = useState(false);
  const [all, setAll] = useState(false);
  const shownId = asks[0]?.id;
  const newest = asks.at(-1);
  useEffect(() => {
    // A new question while the host is in another window (or it is minimized): tell them outside the app too.
    if (newest && (document.hidden || !document.hasFocus())) onAway?.(newest);
    // Only when a new request arrives.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [newest?.id]);
  useEffect(() => {
    if (asks.length < 2) setAll(false);
  }, [asks.length]);
  useEffect(() => {
    setMore(false);
    if (shownId) onShow?.();
    // Only when a new request comes to the front.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shownId]);

  useEffect(() => {
    void commands.teamAgentAsks().then(setAsks, () => {});
    const off = listen<AgentAsk[]>("team-agent-asks", (event) => setAsks(event.payload));
    return () => void off.then((stop) => stop());
  }, []);

  const first = asks[0];
  if (!first) return null;

  async function answer(choice: "allow" | "chat" | "deny") {
    setBusy(true);
    try {
      await commands.teamAgentAnswer(first.id, choice);
    } catch (cause) {
      onError(String(cause));
    }
    setAsks((was) => was.filter((one) => one.id !== first.id));
    setBusy(false);
  }

  /** The same answer to every question waiting now (the list as shown; new ones still come one by one). */
  async function answerAll(choice: "allow" | "deny") {
    setBusy(true);
    const ids = asks.map((one) => one.id);
    for (const id of ids) {
      try {
        await commands.teamAgentAnswer(id, choice);
      } catch (cause) {
        onError(String(cause));
      }
    }
    setAsks((was) => was.filter((one) => !ids.includes(one.id)));
    setBusy(false);
  }

  const server = first.serverName ? shownServerName(first.serverName) : tr("your server");
  return (
    <RadixDialog.Root open onOpenChange={(open) => !open && !busy && void answer("deny")}>
      <RadixDialog.Portal>
        <RadixDialog.Overlay className="ask-backdrop modal-backdrop" />
        <RadixDialog.Content className="ask-dialog dialog team-agent-ask">
          <RadixDialog.Title asChild>
            <h2>{tr("{0}'s agent wants to {1}", [first.who, askWords(first)])}</h2>
          </RadixDialog.Title>
          <RadixDialog.Description asChild>
            <p className="modal-copy">
              {tr("On {0}. Their agent runs on their own PC with their own account, as a {1} of your team.", [server, first.role])}
              {asks.length > 1 && <> {tr("{0} more waiting.", [asks.length - 1])}</>}
            </p>
          </RadixDialog.Description>
          {asks.length > 1 && (
            <button type="button" className="link-button" aria-expanded={all} onClick={() => setAll((was) => !was)}>
              {all ? tr("Hide the others") : tr("See all {0}", [asks.length])}
            </button>
          )}
          {all && (
            <>
              <ol className="team-agent-all">
                {asks.map((one) => (
                  <li key={one.id}>
                    <strong>{one.who}</strong> {askWords(one)}
                    <small>{one.serverName ? shownServerName(one.serverName) : tr("your server")}</small>
                  </li>
                ))}
              </ol>
              <div className="team-agent-all-actions">
                <button type="button" className="button secondary small" disabled={busy} onClick={() => void answerAll("deny")}>
                  {tr("Deny all {0}", [asks.length])}
                </button>
                <button type="button" className="button secondary small" disabled={busy} onClick={() => void answerAll("allow")}>
                  {tr("Allow all {0}", [asks.length])}
                </button>
              </div>
            </>
          )}
          <button type="button" className="link-button" aria-expanded={more} onClick={() => setMore((was) => !was)}>
            {more ? tr("Hide details") : tr("Show details")}
          </button>
          {more && <pre className="team-agent-detail">{first.detail}</pre>}
          <div className="modal-actions">
            <button type="button" className="button secondary" disabled={busy} onClick={() => void answer("deny")}>
              {tr("Deny")}
            </button>
            {first.canChat && (
              <button type="button" className="button secondary" disabled={busy} onClick={() => void answer("chat")} data-tip={tr("Changes from the rest of this chat go through without asking.")}>
                {tr("Allow for this chat")}
              </button>
            )}
            <button type="button" className="button primary" disabled={busy} onClick={() => void answer("allow")}>
              {tr("Allow")}
            </button>
          </div>
        </RadixDialog.Content>
      </RadixDialog.Portal>
    </RadixDialog.Root>
  );
}

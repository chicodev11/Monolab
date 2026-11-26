import { useCallback, useEffect, useMemo, useState } from "react";
import type { AcpChatMeta, ChatSession } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { isSharedRef, sharedServers, type SharedServer } from "../../lib/teamServers";
import { DropdownMenu, EmptyState, List, ListRow } from "../primitives";
import { Icon } from "../ui/Icon";
import { AgentPage } from "./AgentPage";
import { Splitter, usePaneWidth, widthVar } from "../ui/Splitter";

type Server = { id: string; name: string; detail: string };
type Open = { server: string; session: string; serial: number };

const LAST_KEY = "monolab-agent-last";

function readLast(): Open | null {
  try {
    const value = JSON.parse(localStorage.getItem(LAST_KEY) ?? "null") as { server?: unknown; session?: unknown } | null;
    if (value && typeof value.server === "string" && typeof value.session === "string") return { server: value.server, session: value.session, serial: 0 };
  } catch {
    /* nothing remembered */
  }
  return null;
}

/**
 * The agent on the main menu, laid out like a desktop chat app: conversations grouped by server on
 * the left, the open conversation on the right. Each server keeps its own workspace and history.
 */
export function AgentHome({
  servers: own,
  onError,
  confirm,
}: {
  servers: Server[];
  onError: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
}) {
  const sidePane = usePaneWidth("agent-side", 280, 220, 520);
  // Servers teammates share through Monolink: the agent runs here, with this user's own sign-in,
  // and its tools reach the host with this user's team role (the host approves changes).
  const [shared, setShared] = useState<SharedServer[]>([]);
  useEffect(() => {
    void sharedServers().then(setShared, () => setShared([]));
  }, []);
  const servers = useMemo<Server[]>(
    () => [...own, ...shared.map((one) => ({ id: one.ref, name: one.name, detail: tr("@{0}'s PC · {1}", [one.host, one.team]) }))],
    [own, shared],
  );
  const [chats, setChats] = useState<Record<string, ChatSession[]>>({});
  // Saved conversations with other (ACP) agents, kept by the app.
  const [acpChats, setAcpChats] = useState<Record<string, AcpChatMeta[]>>({});
  const [agentNames, setAgentNames] = useState<Record<string, string>>({});
  useEffect(() => {
    void commands.acpAgents(false).then(
      (list) => setAgentNames(Object.fromEntries(list.map((one) => [one.id, one.name]))),
      () => undefined,
    );
  }, []);
  const [folded, setFolded] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<Open | null>(() => readLast());
  const [query, setQuery] = useState("");

  const serverIds = useMemo(() => servers.map((one) => one.id).join(","), [servers]);
  const load = useCallback(() => {
    for (const id of serverIds.split(",").filter(Boolean)) {
      void commands.claudeChatSessions(id).then(
        (list) => setChats((was) => ({ ...was, [id]: list })),
        () => setChats((was) => ({ ...was, [id]: [] })),
      );
      void commands.acpChats(id).then(
        (list) => setAcpChats((was) => ({ ...was, [id]: list })),
        () => setAcpChats((was) => ({ ...was, [id]: [] })),
      );
    }
  }, [serverIds]);
  useEffect(load, [load]);
  const busy = Object.values(chats).some((list) => list.some((one) => one.busy));
  useEffect(() => {
    if (!busy) return;
    const timer = window.setInterval(load, 5000);
    return () => window.clearInterval(timer);
  }, [busy, load]);

  // A remembered conversation of a server that's gone falls back to the first server.
  const shown = open && servers.some((one) => one.id === open.server) ? open : servers[0] ? { server: servers[0].id, session: "", serial: 0 } : null;
  const go = (server: string, session: string) => {
    setOpen((was) => ({ server, session, serial: (was?.serial ?? 0) + 1 }));
    try {
      localStorage.setItem(LAST_KEY, JSON.stringify({ server, session }));
    } catch {
      /* remembered for this session only */
    }
  };
  const needle = query.trim().toLowerCase();
  const current = servers.find((one) => one.id === shown?.server);

  // Renaming and deleting a conversation: Claude chats through their own commands, ACP ones in the app's file.
  const [renaming, setRenaming] = useState<{ server: string; id: string; title: string } | null>(null);
  const saveName = async () => {
    if (!renaming) return;
    const { server, id, title } = renaming;
    setRenaming(null);
    const name = title.trim();
    if (!name) return;
    try {
      if (id.startsWith("acp:")) {
        const saved = await commands.acpChatRead(server, id.slice(4));
        await commands.acpChatSave(server, { ...saved, title: name });
      } else {
        await commands.claudeChatRename(id, name);
      }
      load();
    } catch (cause) {
      onError(String(cause));
    }
  };
  const remove = async (server: string, id: string, title: string) => {
    const ok = await confirm(
      tr("Delete this conversation?"),
      tr("This deletes the conversation history of \"{0}\". The notes in AGENTS.md stay.", [title || tr("New chat")]),
      tr("Delete"),
      true,
    );
    if (!ok) return;
    try {
      if (id.startsWith("acp:")) await commands.acpChatDelete(server, id.slice(4));
      else await commands.claudeChatDelete(server, id);
      if (shown?.server === server && shown.session === id) go(server, "");
      load();
    } catch (cause) {
      onError(String(cause));
    }
  };

  if (!servers.length) {
    return (
      <div className="agent-home empty">
        <EmptyState icon="sparkles" title={tr("No servers yet")} hint={tr("The agent works inside a server's folder. Make or import a server first.")} />
      </div>
    );
  }
  return (
    <div className="agent-home with-split" style={widthVar("--side-w", sidePane.width)}>
      <aside className="agent-side" aria-label={tr("Conversations")}>
        <div className="agent-side-top">
          <DropdownMenu
            trigger={
              <button type="button" className="agent-new">
                <Icon name="plus" />
                {tr("New chat")}
                <Icon name="chevron" />
              </button>
            }
            items={servers.map((one) => ({ label: one.name, icon: "server", onSelect: () => go(one.id, "") }))}
          />
          <label className="agent-search">
            <Icon name="search" />
            <input value={query} placeholder={tr("Search conversations")} onChange={(event) => setQuery(event.target.value)} />
          </label>
        </div>
        <div className="agent-side-scroll">
          {servers.map((server) => {
            // Claude chats and saved ACP ones together, newest first. ACP ids carry "acp:".
            const list = [
              ...(chats[server.id] ?? []).map((one) => ({ id: one.id, title: one.title, updated: one.updated ?? 0, busy: one.busy, agent: "" })),
              ...(acpChats[server.id] ?? []).map((one) => ({ id: `acp:${one.id}`, title: one.title, updated: (one.updated ?? 0) * 1000, busy: false, agent: one.agent })),
            ]
              .sort((a, b) => b.updated - a.updated)
              .filter((one) => !needle || (one.title || "").toLowerCase().includes(needle));
            if (needle && !list.length) return null;
            const closed = folded.has(server.id);
            return (
              <section key={server.id} className="agent-group">
                <div className="agent-group-head">
                  <button
                    type="button"
                    className="agent-group-name"
                    aria-expanded={!closed}
                    onClick={() =>
                      setFolded((was) => {
                        const next = new Set(was);
                        if (next.has(server.id)) next.delete(server.id);
                        else next.add(server.id);
                        return next;
                      })
                    }
                  >
                    <span className="agent-group-tile">
                      <Icon name={isSharedRef(server.id) ? "users" : "server"} />
                    </span>
                    <strong>{server.name}</strong>
                    <Icon name={closed ? "arrow" : "chevron"} />
                  </button>
                  <button type="button" className="wb-icon ghost" aria-label={tr("New chat in {0}", [server.name])} data-tip={tr("New chat")} onClick={() => go(server.id, "")}>
                    <Icon name="plus" />
                  </button>
                </div>
                {!closed && (
                  <div className="agent-group-list">
                    {list.length === 0 && <p className="agent-none">{tr("No conversations yet")}</p>}
                    {list.length > 0 && (
                    <List variant="plain" density="compact" className="side-rows agent-chat-rows">
                    {list.slice(0, 30).map((one) => {
                      const on = shown?.server === server.id && shown.session === one.id;
                      if (renaming && renaming.server === server.id && renaming.id === one.id) {
                        return (
                          <li key={one.id} className="list-row agent-chat-renaming">
                          <input
                            className="agent-chat-rename"
                            autoFocus
                            value={renaming.title}
                            aria-label={tr("Chat name")}
                            onChange={(event) => setRenaming({ ...renaming, title: event.target.value })}
                            onBlur={() => void saveName()}
                            onKeyDown={(event) => {
                              if (event.key === "Enter") void saveName();
                              if (event.key === "Escape") setRenaming(null);
                            }}
                          />
                          </li>
                        );
                      }
                      return (
                        <ListRow
                          key={one.id}
                          className="agent-chat-row"
                          selected={on}
                          onClick={() => go(server.id, one.id)}
                          mainProps={{ "aria-current": on || undefined, "data-tip": one.title || undefined }}
                          leading={one.busy ? <span className="chat-spin" aria-label={tr("Working")} /> : undefined}
                          title={one.title || tr("New chat")}
                          meta={one.agent ? <small className="agent-chat-tag">{agentNames[one.agent] ?? one.agent}</small> : undefined}
                          actions={
                          <>
                            <button
                              type="button"
                              className="wb-icon ghost"
                              aria-label={tr("Rename {0}", [one.title || tr("New chat")])}
                              data-tip={tr("Rename")}
                              onClick={() => setRenaming({ server: server.id, id: one.id, title: one.title })}
                            >
                              <Icon name="pencil" />
                            </button>
                            <button
                              type="button"
                              className="wb-icon ghost danger"
                              aria-label={tr("Delete {0}", [one.title || tr("New chat")])}
                              data-tip={tr("Delete")}
                              disabled={one.busy}
                              onClick={() => void remove(server.id, one.id, one.title)}
                            >
                              <Icon name="trash" />
                            </button>
                          </>
                          }
                        />
                      );
                    })}
                    </List>
                    )}
                  </div>
                )}
              </section>
            );
          })}
        </div>
      </aside>
      <Splitter pane={sidePane} side="left" label={tr("Resize the conversation list")} />
      <section className="agent-main">
        {shown && current && (
          <>
            <header className="agent-main-head">
              <Icon name={isSharedRef(current.id) ? "users" : "server"} />
              <strong>{current.name}</strong>
              <small>{current.detail}</small>
            </header>
            {isSharedRef(current.id) && (
              <p className="agent-shared-note muted">
                {tr("Your own agent account works on this teammate's server. It can do only what your team role allows, and the host approves every change.")}
              </p>
            )}
            <AgentPage
              key={`${shown.server}:${shown.session}:${shown.serial}`}
              server={{ id: current.id, name: current.name }}
              startSession={shown.session}
              listHidden
              onSessionsChanged={load}
              onError={onError}
              confirm={confirm}
            />
          </>
        )}
      </section>
    </div>
  );
}

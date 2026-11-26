import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AcpAgent, AcpOpened } from "../../lib/bindings";
import type { ChatTodo } from "../../lib/claudeChat";
import { listenTo } from "../../lib/bridge";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { EmptyState } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { ChatStarters, Markdown, TodoPanel } from "./ClaudeChat";

/** One line of the conversation, built from the agent's `session/update` notices. */
type Line =
  | { kind: "user"; text: string }
  | { kind: "agent"; text: string }
  | { kind: "thought"; text: string }
  | { kind: "tool"; id: string; title: string; status: string; tool: string; input?: string; output?: string }
  | { kind: "plan"; entries: { content: string; status: string }[] }
  | { kind: "note"; text: string };

type Ask = { requestId: string; title: string; detail: string; options: { optionId: string; name: string; kind: string }[] };
type Choice = { id: string; name: string; current: string; options: { value: string; name: string }[] };
type Install = { done: number; total: number; step: string };

export const AGENT_KEY = "monolab-acp-agent";

function parse<T>(text: string, fallback: T): T {
  try {
    return (JSON.parse(text) as T) ?? fallback;
  } catch {
    return fallback;
  }
}

function textOf(content: unknown): string {
  if (!content || typeof content !== "object") return "";
  const one = content as { type?: string; text?: string };
  return one.type === "text" && typeof one.text === "string" ? one.text : "";
}

/** What a tool call shows when opened: its text output, the files it changed, or its raw result. */
function outputOf(update: Record<string, unknown>): string | undefined {
  const parts: string[] = [];
  if (Array.isArray(update.content)) {
    for (const one of update.content as Record<string, unknown>[]) {
      if (one.type === "content") {
        const text = textOf(one.content);
        if (text) parts.push(text);
      } else if (one.type === "diff" && typeof one.path === "string") {
        const before = typeof one.oldText === "string" ? one.oldText.split("\n").length : 0;
        const after = typeof one.newText === "string" ? one.newText.split("\n").length : 0;
        parts.push(before ? tr("Changed {0} ({1} → {2} lines)", [one.path, before, after]) : tr("Created {0} ({1} lines)", [one.path, after]));
      }
    }
  }
  if (!parts.length && update.rawOutput !== undefined && update.rawOutput !== null) {
    parts.push(typeof update.rawOutput === "string" ? update.rawOutput : JSON.stringify(update.rawOutput, null, 2));
  }
  return parts.length ? parts.join("\n\n") : undefined;
}

/** Folds one `session/update` into the lines: chunks join the line before them, tool calls update in place. */
function fold(lines: Line[], update: Record<string, unknown>): Line[] {
  const kind = update.sessionUpdate;
  const last = lines[lines.length - 1];
  if (kind === "agent_message_chunk" || kind === "agent_thought_chunk") {
    const text = textOf(update.content);
    if (!text) return lines;
    const as = kind === "agent_message_chunk" ? "agent" : "thought";
    if (last && last.kind === as) return [...lines.slice(0, -1), { kind: as, text: last.text + text }];
    return [...lines, { kind: as, text }];
  }
  if (kind === "tool_call" || kind === "tool_call_update") {
    const id = String(update.toolCallId ?? "");
    const at = lines.findIndex((one) => one.kind === "tool" && one.id === id);
    const was = at >= 0 ? (lines[at] as Extract<Line, { kind: "tool" }>) : null;
    const next: Line = {
      kind: "tool",
      id,
      title: typeof update.title === "string" && update.title ? update.title : (was?.title ?? tr("Tool")),
      status: typeof update.status === "string" ? update.status : (was?.status ?? "pending"),
      tool: typeof update.kind === "string" ? update.kind : (was?.tool ?? "other"),
      input: update.rawInput !== undefined && update.rawInput !== null ? JSON.stringify(update.rawInput, null, 2) : was?.input,
      output: outputOf(update) ?? was?.output,
    };
    if (at < 0) return [...lines, next];
    return lines.map((one, index) => (index === at ? next : one));
  }
  if (kind === "plan" && Array.isArray(update.entries)) {
    const entries = (update.entries as { content?: string; status?: string }[]).map((one) => ({ content: String(one.content ?? ""), status: String(one.status ?? "pending") }));
    const at = lines.findIndex((one) => one.kind === "plan");
    if (at < 0) return [...lines, { kind: "plan", entries }];
    return lines.map((one, index) => (index === at ? { kind: "plan", entries } : one));
  }
  return lines;
}

function choicesOf(opened: AcpOpened): { modes: Choice | null; options: Choice[] } {
  const modes = parse<{ currentModeId?: string; availableModes?: { id: string; name: string }[] } | null>(opened.modes, null);
  const options = parse<{ id: string; name: string; type?: string; currentValue?: string; options?: { value: string; name: string }[] }[]>(opened.configOptions, []);
  // Newer agents send the mode as a config option too; show it once.
  const modeOption = (Array.isArray(options) ? options : []).some((one) => one.id === "mode" || (one as { category?: string }).category === "mode");
  return {
    modes:
      !modeOption && modes?.availableModes?.length
        ? { id: "mode", name: tr("Mode"), current: modes.currentModeId ?? "", options: modes.availableModes.map((one) => ({ value: one.id, name: one.name })) }
        : null,
    options: (Array.isArray(options) ? options : [])
      .filter((one) => one.type === "select" && one.options?.length)
      .map((one) => ({ id: one.id, name: one.name, current: one.currentValue ?? "", options: one.options ?? [] })),
  };
}

function toolState(status: string, live: boolean): "run" | "ok" | "bad" | "cut" {
  if (status === "completed") return "ok";
  if (status === "failed") return "bad";
  return live ? "run" : "cut";
}

function megabytes(bytes: number): string {
  return (bytes / 1024 / 1024).toFixed(bytes > 100 * 1024 * 1024 ? 0 : 1);
}

function newChatId(): string {
  return typeof crypto !== "undefined" && "randomUUID" in crypto ? crypto.randomUUID() : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

/**
 * Agents that speak ACP (Agent Client Protocol): pick one from the public registry, then chat with it
 * in this server's workspace. It looks like the Claude and Codex chats, uses the same Monolab tools
 * and asks here before risky steps.
 */
export function AcpPanel({
  server,
  onError,
  chatId = "",
  onChanged,
  wanted = "",
}: {
  server: { id: string; name: string };
  /** The agent picked in the page's own agent menu; its picker here then hides. */
  wanted?: string;
  onError: (message: string) => void;
  /** A saved conversation to open again; empty starts a new one. */
  chatId?: string;
  /** A conversation was saved, or an agent was downloaded, so lists can refresh. */
  onChanged?: () => void;
}) {
  const [agents, setAgents] = useState<AcpAgent[] | null>(null);
  const [agent, setAgent] = useState(() => {
    try {
      return localStorage.getItem(AGENT_KEY) ?? "";
    } catch {
      return "";
    }
  });
  const [opened, setOpened] = useState<AcpOpened | null>(null);
  const [starting, setStarting] = useState(false);
  const [lines, setLines] = useState<Line[]>([]);
  const [asks, setAsks] = useState<Ask[]>([]);
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState("");
  const [choices, setChoices] = useState<{ modes: Choice | null; options: Choice[] }>({ modes: null, options: [] });
  const [install, setInstall] = useState<Install | null>(null);
  const live = useRef<string>("");
  const scroller = useRef<HTMLDivElement | null>(null);
  const stick = useRef(true);
  // The conversation being shown, as saved: its file id and the agent's own session.
  const chat = useRef({ id: "", sessionId: "" });
  const linesNow = useRef<Line[]>([]);
  linesNow.current = lines;

  const loadAgents = useCallback(
    () =>
      commands.acpAgents(false).then(
        (list) => {
          setAgents(list);
          return list;
        },
        (cause) => {
          setAgents([]);
          onError(String(cause));
          return [] as AcpAgent[];
        },
      ),
    [onError],
  );
  useEffect(() => {
    void loadAgents();
  }, [loadAgents]);

  // Events of this conversation's agent process.
  useEffect(() => {
    const off = listenTo<{ id: string; kind: string; data: string }>("acp-event", (event) => {
      if (event.id !== live.current) return;
      const data = parse<Record<string, unknown>>(event.data, {});
      if (event.kind === "update") {
        const update = (data.update ?? {}) as Record<string, unknown>;
        if (update.sessionUpdate === "current_mode_update" && typeof update.currentModeId === "string") {
          const mode = update.currentModeId;
          setChoices((was) => (was.modes ? { ...was, modes: { ...was.modes, current: mode } } : was));
          return;
        }
        setLines((was) => fold(was, update));
      } else if (event.kind === "permission") {
        const request = (data.request ?? {}) as { toolCall?: { title?: string; rawInput?: unknown }; options?: Ask["options"] };
        const raw = request.toolCall?.rawInput;
        setAsks((was) => [
          ...was,
          {
            requestId: String(data.requestId ?? ""),
            title: request.toolCall?.title ?? tr("The agent wants to go on"),
            detail: raw === undefined || raw === null ? "" : typeof raw === "string" ? raw : JSON.stringify(raw, null, 2),
            options: request.options ?? [],
          },
        ]);
      } else if (event.kind === "done") {
        setBusy(false);
        setAsks([]);
        if (typeof data.error === "string") setLines((was) => [...was, { kind: "note", text: data.error as string }]);
        else if (data.stopReason === "cancelled") setLines((was) => [...was, { kind: "note", text: tr("Stopped.") }]);
        else if (data.stopReason === "refusal") setLines((was) => [...was, { kind: "note", text: tr("The agent declined.") }]);
      } else if (event.kind === "exited") {
        setBusy(false);
        setLines((was) => [...was, { kind: "note", text: tr("The agent stopped running.") }]);
        live.current = "";
        setOpened(null);
      }
    });
    return () => {
      void off.then((stop) => stop());
    };
  }, []);

  // Download progress of the agent being installed.
  useEffect(() => {
    const off = listenTo<{ agent: string; done: number; total: number; step: string }>("acp-install", (event) => {
      if (event.agent !== agent) return;
      setInstall({ done: event.done, total: event.total, step: event.step });
    });
    return () => {
      void off.then((stop) => stop());
    };
  }, [agent]);

  // The process closes with the page.
  useEffect(
    () => () => {
      if (live.current) void commands.acpClose(live.current);
    },
    [],
  );

  useEffect(() => {
    if (stick.current) scroller.current?.scrollTo({ top: scroller.current.scrollHeight });
  }, [lines, asks]);

  /** Starts the agent: a new conversation, or a saved one (shown again; picked up by the agent when it can). */
  const start = useCallback(
    async (id: string, saved?: { id: string; sessionId: string; lines: Line[] }) => {
      if (live.current) void commands.acpClose(live.current);
      live.current = "";
      chat.current = { id: saved?.id ?? "", sessionId: saved?.sessionId ?? "" };
      setOpened(null);
      setLines(saved?.lines ?? []);
      setAsks([]);
      setBusy(false);
      stick.current = true;
      // A downloadable agent that isn't here yet waits for the user's Download press.
      const known = agents?.find((one) => one.id === id);
      if (known && !known.ready && known.download) return;
      setStarting(true);
      try {
        const next = await commands.acpOpen(server.id, id, saved?.sessionId ?? "");
        live.current = next.id;
        chat.current.sessionId = next.sessionId || chat.current.sessionId;
        if (saved?.sessionId && next.sessionId && next.sessionId !== saved.sessionId) {
          setLines((was) => [...was, { kind: "note", text: tr("This agent can't pick up old conversations, so it starts fresh from here.") }]);
        }
        setOpened(next);
        setChoices(choicesOf(next));
      } catch (cause) {
        onError(String(cause));
      }
      setStarting(false);
    },
    [server.id, onError, agents],
  );

  // Open a saved conversation, or start the last used agent for a new one (once the list is in).
  useEffect(() => {
    if (agents === null) return;
    if (!chatId) {
      if (agent) void start(agent);
      return;
    }
    let gone = false;
    void commands.acpChatRead(server.id, chatId).then(
      (saved) => {
        if (gone) return;
        setAgent(saved.agent);
        void start(saved.agent, { id: saved.id, sessionId: saved.sessionId, lines: parse<Line[]>(saved.lines, []) });
      },
      (cause) => !gone && onError(String(cause)),
    );
    return () => {
      gone = true;
    };
    // Only when the conversation to show changes, or the agent list first arrives.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatId, server.id, agents === null]);

  // Save after each turn (and when one starts, so it shows in the list right away).
  useEffect(() => {
    const now = linesNow.current;
    const first = now.find((one) => one.kind === "user");
    if (!agent || !first || first.kind !== "user") return;
    if (!chat.current.id) chat.current.id = newChatId();
    void commands
      .acpChatSave(server.id, {
        id: chat.current.id,
        agent,
        title: first.text.slice(0, 80),
        updated: Date.now() / 1000,
        sessionId: chat.current.sessionId,
        lines: JSON.stringify(now),
      })
      .then(() => onChanged?.(), () => undefined);
    // Saved on turn edges only, not on every streamed piece.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [busy]);

  const pick = (id: string) => {
    setAgent(id);
    try {
      localStorage.setItem(AGENT_KEY, id);
    } catch {
      /* this session only */
    }
    void start(id);
  };

  // The page's agent menu picked a different agent.
  useEffect(() => {
    if (wanted && wanted !== agent) pick(wanted);
    // Only when the menu's choice changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wanted]);

  const download = async () => {
    setInstall({ done: 0, total: 0, step: "download" });
    try {
      await commands.acpInstall(agent);
      const list = await loadAgents();
      onChanged?.();
      setInstall(null);
      if (list.find((one) => one.id === agent)?.ready) {
        setStarting(true);
        try {
          const next = await commands.acpOpen(server.id, agent, "");
          live.current = next.id;
          setOpened(next);
          setChoices(choicesOf(next));
        } catch (cause) {
          onError(String(cause));
        }
        setStarting(false);
      }
    } catch (cause) {
      setInstall(null);
      onError(String(cause));
    }
  };

  const uninstall = async () => {
    if (live.current) void commands.acpClose(live.current);
    live.current = "";
    setOpened(null);
    try {
      await commands.acpUninstall(agent);
      await loadAgents();
      onChanged?.();
    } catch (cause) {
      onError(String(cause));
    }
  };

  const signIn = async (method: string) => {
    if (!opened) return;
    setStarting(true);
    try {
      const next = await commands.acpAuthenticate(opened.id, method);
      setOpened(next);
      setChoices(choicesOf(next));
    } catch (cause) {
      onError(String(cause));
    }
    setStarting(false);
  };

  const send = async (typed?: string) => {
    const text = (typed ?? draft).trim();
    if (!opened || !text || busy) return;
    if (typed === undefined) setDraft("");
    stick.current = true;
    setLines((was) => [...was, { kind: "user", text }]);
    setBusy(true);
    try {
      await commands.acpPrompt(opened.id, JSON.stringify([{ type: "text", text }]));
    } catch (cause) {
      setBusy(false);
      onError(String(cause));
    }
  };

  const answer = (ask: Ask, option: string | null) => {
    if (!opened) return;
    setAsks((was) => was.filter((one) => one.requestId !== ask.requestId));
    void commands.acpAnswer(opened.id, ask.requestId, option).catch((cause) => onError(String(cause)));
  };

  const setChoice = (choice: Choice, value: string) => {
    if (!opened) return;
    if (choice.id === "mode") {
      setChoices((was) => (was.modes ? { ...was, modes: { ...was.modes, current: value } } : was));
      void commands.acpSetMode(opened.id, value).catch((cause) => onError(String(cause)));
      return;
    }
    setChoices((was) => ({ ...was, options: was.options.map((one) => (one.id === choice.id ? { ...one, current: value } : one)) }));
    void commands.acpSetConfig(opened.id, choice.id, value).catch((cause) => onError(String(cause)));
  };

  const sorted = useMemo(
    () => [...(agents ?? [])].sort((a, b) => Number(b.ready || b.download) - Number(a.ready || a.download) || a.name.localeCompare(b.name)),
    [agents],
  );
  const current = agents?.find((one) => one.id === agent);
  const name = current?.name ?? agent;
  const authMethods = opened ? parse<{ id: string; name: string; description?: string }[]>(opened.authMethods, []) : [];
  const needsSignIn = !!opened && !opened.sessionId;
  const needsDownload = !!current && !current.ready && current.download;
  const plan = [...lines].reverse().find((one): one is Extract<Line, { kind: "plan" }> => one.kind === "plan");
  const todos: ChatTodo[] = (plan?.entries ?? []).map((one) => ({
    content: one.content,
    activeForm: one.content,
    status: one.status === "completed" || one.status === "in_progress" ? one.status : "pending",
  }));
  const picks = [choices.modes, ...choices.options].filter((one): one is Choice => !!one);

  return (
    <div className="acp-page">
      {(!wanted || (current && !starting && !needsDownload)) && (
        <div className="acp-bar">
          {!wanted && (
            <label className="acp-pick">
              <span>{tr("Agent")}</span>
              <select value={agent} onChange={(event) => pick(event.target.value)} disabled={agents === null}>
                <option value="" disabled>
                  {agents === null ? tr("Loading…") : tr("Pick an agent")}
                </option>
                {sorted.map((one) => (
                  <option key={one.id} value={one.id} disabled={!one.ready && !one.download}>
                    {one.name}
                    {one.ready ? "" : one.download ? ` (${tr("download")})` : ` (${tr("needs {0}", [one.launch])})`}
                  </option>
                ))}
              </select>
            </label>
          )}
          {current && !starting && !needsDownload && (
            <button type="button" className="button ghost small" onClick={() => void start(current.id)} data-tip={tr("Start over in a new conversation")}>
              <Icon name="plus" />
              {tr("New chat")}
            </button>
          )}
          {current?.launch === "binary" && current.ready && (
            <button type="button" className="button ghost small" onClick={() => void uninstall()} data-tip={tr("Remove the downloaded program of {0}", [name])}>
              <Icon name="trash" />
              {tr("Remove download")}
            </button>
          )}
        </div>
      )}
      {!agent ? (
        <EmptyState
          icon="sparkles"
          title={tr("Use another AI agent")}
          hint={tr("Agents that support ACP (Gemini CLI, GitHub Copilot, Claude and Codex adapters and more) work on this server with the same Monolab tools. Pick one above; some start with npx and others are downloaded once.")}
        />
      ) : needsDownload ? (
        <EmptyState
          icon="import"
          title={tr("Download {0}", [name])}
          hint={
            install
              ? install.step === "unpack"
                ? tr("Unpacking…")
                : install.total
                  ? tr("Downloading… {0} of {1} MB", [megabytes(install.done), megabytes(install.total)])
                  : tr("Downloading… {0} MB", [megabytes(install.done)])
              : tr("{0} comes as a program for this computer. Monolab downloads it from the address in the public agent list, checks it and keeps it in its own folder.", [name])
          }
          action={
            install ? (
              <span className="acp-install-bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={install.total ? Math.round((install.done / install.total) * 100) : undefined}>
                <span style={{ width: install.total ? `${Math.min(100, (install.done / install.total) * 100)}%` : "100%" }} className={install.total ? "" : "unknown"} />
              </span>
            ) : (
              <button type="button" className="button primary" onClick={() => void download()}>
                <Icon name="import" />
                {tr("Download")}
              </button>
            )
          }
        />
      ) : starting ? (
        <div className="acp-wait">
          <span className="chat-spin" aria-hidden />
          {current?.launch === "binary" ? tr("Starting {0}…", [name]) : tr("Starting {0}… The first time, npx downloads it first.", [name])}
        </div>
      ) : !opened ? (
        <EmptyState
          icon="alert"
          title={tr("{0} isn't running", [name])}
          action={
            <button type="button" className="button primary" onClick={() => void start(agent)}>
              {tr("Start")}
            </button>
          }
        />
      ) : needsSignIn ? (
        <div className="acp-signin">
          <strong>{tr("Sign in to {0}", [name])}</strong>
          <p className="muted">{tr("Pick how to sign in. A browser may open; come back here when it's done.")}</p>
          <div className="acp-signin-list">
            {authMethods.length === 0 && <p className="muted">{tr("This agent didn't say how to sign in. Sign in with its own program first, then start it again.")}</p>}
            {authMethods.map((one) => (
              <button key={one.id} type="button" className="button secondary" onClick={() => void signIn(one.id)}>
                {one.name}
                {one.description && <small>{one.description}</small>}
              </button>
            ))}
          </div>
        </div>
      ) : (
        <div className="chat">
          <div
            className="chat-list"
            ref={scroller}
            onScroll={(event) => {
              const box = event.currentTarget;
              stick.current = box.scrollHeight - box.scrollTop - box.clientHeight < 48;
            }}
          >
            {lines.length === 0 && (
              <div className="chat-hello">
                <Icon name="sparkles" />
                <strong>{tr("Chat with {0} about {1}", [name, server.name])}</strong>
                <p className="muted">{tr("It reads logs, edits files and runs tools through Monolab, and asks here before risky steps.")}</p>
                <ChatStarters onPick={(prompt) => void send(prompt)} />
              </div>
            )}
            {lines.map((line, at) =>
              line.kind === "user" ? (
                <div key={at} className="chat-user">
                  <div className="chat-bubble">
                    <span>{line.text}</span>
                  </div>
                </div>
              ) : line.kind === "agent" ? (
                <div key={at} className={`chat-assistant ${busy && at === lines.length - 1 ? "streaming" : ""}`}>
                  <Markdown text={line.text} />
                </div>
              ) : line.kind === "thought" ? (
                <details key={at} className="chat-summary">
                  <summary>
                    <Icon name="sparkles" />
                    <span>{tr("Thinking")}</span>
                    <Icon name="chevron" />
                  </summary>
                  <div className="chat-summary-body">
                    <Markdown text={line.text} />
                  </div>
                </details>
              ) : line.kind === "tool" ? (
                <details key={at} className={`chat-tool ${toolState(line.status, busy)}`}>
                  <summary>
                    <span className="chat-tool-mark">
                      {toolState(line.status, busy) === "run" ? <span className="chat-spin" /> : <Icon name={line.status === "failed" ? "alert" : line.status === "completed" ? "check" : "stop"} />}
                    </span>
                    <strong>{line.title}</strong>
                    <Icon name="chevron" />
                  </summary>
                  <div className="chat-tool-body">
                    {line.input && <pre>{line.input}</pre>}
                    <pre className="chat-tool-result">{line.output ? (line.output.length > 6000 ? `${line.output.slice(0, 6000)}\n…` : line.output) : tr("(no results)")}</pre>
                  </div>
                </details>
              ) : line.kind === "plan" ? null : (
                <div key={at} className="chat-note">
                  <span>{line.text}</span>
                </div>
              ),
            )}
            {asks.map((ask) => (
              <div key={ask.requestId} className="chat-ask" role="alertdialog" aria-label={ask.title}>
                <div className="chat-ask-head">
                  <Icon name="shield" />
                  <div>
                    <strong>{tr("Allow {0}?", [ask.title])}</strong>
                  </div>
                </div>
                {ask.detail && <pre className="chat-ask-what">{ask.detail.length > 2000 ? `${ask.detail.slice(0, 2000)}\n…` : ask.detail}</pre>}
                <div className="chat-ask-actions">
                  {ask.options.map((one) => (
                    <button
                      key={one.optionId}
                      type="button"
                      className={`button small ${one.kind === "allow_once" ? "primary" : one.kind.startsWith("allow") ? "secondary" : "ghost"}`}
                      onClick={() => answer(ask, one.optionId)}
                    >
                      {one.name}
                    </button>
                  ))}
                  {ask.options.length === 0 && (
                    <button type="button" className="button ghost small" onClick={() => answer(ask, null)}>
                      {tr("Deny")}
                    </button>
                  )}
                </div>
              </div>
            ))}
            {busy && asks.length === 0 && (
              <div className="chat-thinking">
                <span className="chat-spin" />
                {tr("Thinking…")}
              </div>
            )}
          </div>
          <div className="chat-compose">
            {todos.some((one) => one.status !== "completed") && <TodoPanel todos={todos} live={busy} />}
            <div className="chat-input">
              <textarea
                rows={1}
                value={draft}
                placeholder={tr("Message {0} (Enter to send, Shift+Enter for a new line)", [name])}
                aria-label={tr("Message")}
                onChange={(event) => setDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                    event.preventDefault();
                    void send();
                  }
                }}
              />
              <div className="chat-bar">
                {picks.map((choice) => (
                  <span key={choice.id} data-tip={choice.name}>
                    <Dropdown
                      value={choice.current}
                      onChange={(value) => setChoice(choice, value)}
                      className="chat-pick"
                      options={choice.options.map((one) => ({ value: one.value, label: one.name }))}
                    />
                  </span>
                ))}
                <span className="chat-usages" />
                {busy ? (
                  <button className="button secondary small chat-send" onClick={() => void commands.acpCancel(opened.id).catch((cause) => onError(String(cause)))} data-tip={tr("Stop the current reply")}>
                    <Icon name="stop" />
                    {tr("Stop")}
                  </button>
                ) : (
                  <button className="button primary small chat-send" disabled={!draft.trim()} onClick={() => void send()}>
                    <Icon name="arrow-up" />
                    {tr("Send")}
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

import { memo, useCallback, useEffect, useRef, useState, type ClipboardEvent, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { commands } from "../../lib/ipc";
import { tr } from "../../lib/i18n";
import { applyLine, emptyChat, markAnswered, settle, toolName, toolSummary, type ChatItem, type ChatModel, type ChatTodo } from "../../lib/claudeChat";
import { chatEntry, lastUsage, setChatEntry, startChatListener, useChatEntry } from "../../lib/claudeChatStore";
import { Icon } from "../ui/Icon";
import { Dropdown } from "../ui/Dropdown";

const MODE_KEY = "monolab-claude-permission";
const MODEL_KEY = "monolab-claude-model";
type Mode = "default" | "acceptEdits" | "bypassPermissions";
function remembered(key: string) {
  try {
    return localStorage.getItem(key) ?? "";
  } catch {
    return "";
  }
}
function remember(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {}
}
type Starter = { icon: "puzzle" | "alert" | "gauge" | "sliders" | "wrench" | "shield"; title: string; prompt: string };
// Prompts stay English for the agent; titles are translated on screen.
const MINECRAFT_STARTERS: Starter[] = [
  {
    icon: "wrench",
    title: "Set up the server",
    prompt: "Help me set up this Minecraft server. Ask what kind of server I want (survival, minigames, creative, modded, with friends or public), then suggest plugins or mods and server.properties settings, explain each one, and install and change them only after I agree.",
  },
  {
    icon: "puzzle",
    title: "Make a plugin",
    prompt: "I want a custom plugin (or mod) for this server. Ask me what it should do, then build it, add it to the server and tell me how to test it.",
  },
  {
    icon: "alert",
    title: "Find why it crashed",
    prompt: "Read the latest crash reports and logs for this server, find the cause and tell me how to fix it. Don't change anything until I agree.",
  },
  {
    icon: "gauge",
    title: "Find the lag",
    prompt: "Check the lag diagnosis, recent logs and settings for this server and tell me what is making it slow and what to change.",
  },
  {
    icon: "sliders",
    title: "Tune the settings",
    prompt: "Review server.properties, the start options and the installed mods for this server and suggest settings that make it run better. Explain each change before making it.",
  },
  {
    icon: "shield",
    title: "Check security",
    prompt: "Check this server's security: online-mode, whitelist, operators, exposed ports, RCON and query, and plugin permissions. List what is risky and how to fix it, and change nothing until I agree.",
  },
];
const GAME_STARTERS: Starter[] = [
  {
    icon: "wrench",
    title: "Set up the server",
    prompt: "Help me set up this game server. Read its config files, ask how I want to play (players, difficulty, world, password), then explain the settings to change and change them only after I agree.",
  },
  {
    icon: "alert",
    title: "Find why it stopped",
    prompt: "Read the recent logs and console output for this server, find why it crashed or stopped and tell me how to fix it. Don't change anything until I agree.",
  },
  {
    icon: "sliders",
    title: "Tune the settings",
    prompt: "Review this server's config files and start options and suggest changes that make it run better. Explain each change before making it.",
  },
  {
    icon: "shield",
    title: "Check security",
    prompt: "Check this server's security: passwords, admin lists, exposed ports and remote console settings. List what is risky and how to fix it, and change nothing until I agree.",
  },
];

const DRAFT_KEY = "monolab-claude-draft:";
type Draft = { text: string; attached: string[] };
const draftPictures = new Map<string, Picture[]>();
function savedDraft(server: string): Draft {
  try {
    const one = JSON.parse(remembered(DRAFT_KEY + server) || "null");
    if (one && typeof one.text === "string" && Array.isArray(one.attached)) return { text: one.text, attached: one.attached.filter((x: unknown) => typeof x === "string") };
  } catch {}
  return { text: "", attached: [] };
}
function saveDraft(server: string, draft: Draft) {
  try {
    if (!draft.text && !draft.attached.length) localStorage.removeItem(DRAFT_KEY + server);
    else localStorage.setItem(DRAFT_KEY + server, JSON.stringify(draft));
  } catch {}
}

export type Engine = "claude" | "codex";
const keyFor = (key: string, engine: Engine) => (engine === "codex" ? `${key}:codex` : key);
function savedMode(engine: Engine): Mode {
  const value = remembered(keyFor(MODE_KEY, engine));
  return value === "acceptEdits" || value === "bypassPermissions" ? value : "default";
}
const MODELS = ["default", "fable", "opus", "sonnet", "haiku"];
function savedModel(engine: Engine) {
  const value = remembered(keyFor(MODEL_KEY, engine));
  if (engine === "codex") return /^[\w.-]{1,63}$/.test(value) ? value : "default";
  return MODELS.includes(value) ? value : "default";
}

function untilText(at: number) {
  const left = Math.max(0, at * 1000 - Date.now());
  const minutes = Math.round(left / 60000);
  if (minutes < 60) return tr("in {0} min", [minutes]);
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return tr("in {0} h {1} min", [hours, minutes % 60]);
  return tr("in {0} d {1} h", [Math.floor(hours / 24), hours % 24]);
}

function freshUsage(server: string) {
  const last = lastUsage(server)?.usage;
  if (!last) return undefined;
  const now = Date.now() / 1000;
  const reset = (limit?: { used: number; resetsAt: number }) =>
    limit && limit.resetsAt && limit.resetsAt < now ? { used: 0, resetsAt: 0 } : limit;
  return { ...last, fiveHour: reset(last.fiveHour), sevenDay: reset(last.sevenDay) };
}

function UsageMeter({ label, limit }: { label: string; limit: { used: number; resetsAt: number } }) {
  const percent = Math.round(limit.used * 100);
  return (
    <span className={"chat-usage" + (percent >= 90 ? " danger" : percent >= 70 ? " warn" : "")} data-tip={limit.resetsAt ? tr("Resets {0}", [untilText(limit.resetsAt)]) : undefined}>
      <span className="chat-usage-label">{label}</span>
      <span className="chat-usage-bar">
        <span style={{ width: `${Math.min(100, percent)}%` }} />
      </span>
      <span className="chat-usage-value">{percent}%</span>
    </span>
  );
}

type Picture = { mediaType: string; data: string; url: string };

function inline(source: string, base: string): ReactNode[] {
  const out: ReactNode[] = [];
  const pattern = /(`[^`\n]+`)|(\*\*[^*\n]+\*\*)|(\[[^\]\n]+\]\((https?:\/\/[^)\s]+)\))/g;
  let last = 0;
  let match: RegExpExecArray | null;
  let n = 0;
  while ((match = pattern.exec(source))) {
    if (match.index > last) out.push(source.slice(last, match.index));
    const key = `${base}-${n++}`;
    if (match[1]) out.push(<code key={key}>{match[1].slice(1, -1)}</code>);
    else if (match[2]) out.push(<strong key={key}>{match[2].slice(2, -2)}</strong>);
    else if (match[3]) {
      const label = match[3].slice(1, match[3].indexOf("]"));
      const url = match[4];
      out.push(
        <a key={key} href={url} onClick={(event) => { event.preventDefault(); void openUrl(url).catch(() => undefined); }}>
          {label}
        </a>,
      );
    }
    last = pattern.lastIndex;
  }
  if (last < source.length) out.push(source.slice(last));
  return out;
}

function MarkdownBody({ text }: { text: string }) {
  const blocks: ReactNode[] = [];
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  let at = 0;
  let n = 0;
  while (at < lines.length) {
    const line = lines[at];
    const key = `b${n++}`;
    if (/^```/.test(line)) {
      const body: string[] = [];
      at += 1;
      while (at < lines.length && !/^```/.test(lines[at])) body.push(lines[at++]);
      at += 1;
      blocks.push(<pre key={key}><code>{body.join("\n")}</code></pre>);
      continue;
    }
    const heading = /^(#{1,4})\s+(.*)$/.exec(line);
    if (heading) {
      blocks.push(<p key={key} className="md-h">{inline(heading[2], key)}</p>);
      at += 1;
      continue;
    }
    if (/^\s*\|/.test(line)) {
      const rows: string[][] = [];
      while (at < lines.length && /^\s*\|/.test(lines[at])) {
        const cells = lines[at].trim().replace(/^\||\|$/g, "").split("|").map((one) => one.trim());
        if (!cells.every((one) => /^:?-{2,}:?$/.test(one))) rows.push(cells);
        at += 1;
      }
      const [first, ...rest] = rows;
      blocks.push(
        <div key={key} className="md-table">
          <table>
            {first && (
              <thead>
                <tr>{first.map((cell, index) => <th key={index}>{inline(cell, `${key}h${index}`)}</th>)}</tr>
              </thead>
            )}
            <tbody>
              {rest.map((row, y) => (
                <tr key={y}>{row.map((cell, x) => <td key={x}>{inline(cell, `${key}-${y}-${x}`)}</td>)}</tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }
    if (/^\s*([-*]|\d+\.)\s+/.test(line)) {
      const ordered = /^\s*\d+\./.test(line);
      const rows: ReactNode[] = [];
      while (at < lines.length && /^\s*([-*]|\d+\.)\s+/.test(lines[at])) {
        rows.push(<li key={`${key}-${at}`}>{inline(lines[at].replace(/^\s*([-*]|\d+\.)\s+/, ""), `${key}-${at}`)}</li>);
        at += 1;
      }
      blocks.push(ordered ? <ol key={key}>{rows}</ol> : <ul key={key}>{rows}</ul>);
      continue;
    }
    if (!line.trim()) {
      at += 1;
      continue;
    }
    const para: string[] = [];
    while (at < lines.length && lines[at].trim() && !/^```|^#{1,4}\s|^\s*([-*]|\d+\.)\s+|^\s*\|/.test(lines[at])) para.push(lines[at++]);
    blocks.push(<p key={key}>{inline(para.join("\n"), key)}</p>);
  }
  return <div className="md">{blocks}</div>;
}

// Streaming changes one message at a time; the others keep their props and skip re-parsing.
const Markdown = memo(MarkdownBody);

function ToolRowBody({ item }: { item: Extract<ChatItem, { kind: "tool" }> }) {
  const summary = toolSummary(item.name, item.input);
  const state = item.result === undefined ? "run" : item.failed ? "bad" : "ok";
  return (
    <details className={`chat-tool ${state}`}>
      <summary>
        <span className="chat-tool-mark">{state === "run" ? <span className="chat-spin" /> : <Icon name={state === "bad" ? "alert" : "check"} />}</span>
        <strong>{toolName(item.name)}</strong>
        {summary && <span className="chat-tool-what">{summary}</span>}
        <Icon name="chevron" />
      </summary>
      <div className="chat-tool-body">
        <pre>{JSON.stringify(item.input, null, 2)}</pre>
        {item.result !== undefined && <pre className="chat-tool-result">{item.result.length > 6000 ? `${item.result.slice(0, 6000)}\n…` : item.result || tr("(no results)")}</pre>}
      </div>
    </details>
  );
}

type Question = { question: string; header?: string; multiSelect?: boolean; options: { label: string; description?: string }[] };

const ToolRow = memo(ToolRowBody);

function PermissionCard({
  item,
  onAnswer,
}: {
  item: Extract<ChatItem, { kind: "permission" }>;
  onAnswer: (answer: "allow" | "always" | "deny", input?: Record<string, unknown>) => void;
}) {
  const questions = item.tool === "AskUserQuestion" && Array.isArray(item.input.questions) ? (item.input.questions as Question[]) : null;
  const [picked, setPicked] = useState<Record<string, string[]>>({});
  const [typed, setTyped] = useState<Record<string, string | undefined>>({});
  const done = Boolean(item.answer);

  if (questions) {
    const chosen = (question: string) => [...(picked[question] ?? []), ...(typed[question]?.trim() ? [typed[question]!.trim()] : [])];
    const ready = questions.every((one) => chosen(one.question).length > 0);
    return (
      <div className={`chat-ask ${done ? "done" : ""}`}>
        {questions.map((one) => (
          <div key={one.question} className="chat-question">
            <strong>{one.question}</strong>
            <div className="chat-options">
              {one.options.map((option) => {
                const on = (picked[one.question] ?? []).includes(option.label);
                return (
                  <button
                    key={option.label}
                    className={`chat-option ${on ? "on" : ""}`}
                    disabled={done}
                    onClick={() => {
                      setPicked((was) => {
                        const now = was[one.question] ?? [];
                        const next = one.multiSelect ? (on ? now.filter((label) => label !== option.label) : [...now, option.label]) : [option.label];
                        return { ...was, [one.question]: next };
                      });
                      if (!one.multiSelect) setTyped((was) => ({ ...was, [one.question]: undefined }));
                    }}
                  >
                    <span>{option.label}</span>
                    {option.description && <small>{option.description}</small>}
                  </button>
                );
              })}
              {typed[one.question] === undefined ? (
                <button
                  className="chat-option chat-option-other"
                  disabled={done}
                  onClick={() => {
                    setTyped((was) => ({ ...was, [one.question]: "" }));
                    if (!one.multiSelect) setPicked((was) => ({ ...was, [one.question]: [] }));
                  }}
                >
                  <span>{tr("Other")}</span>
                  <small>{tr("Type your own answer")}</small>
                </button>
              ) : (
                <input
                  className="chat-option-input"
                  autoFocus
                  disabled={done}
                  value={typed[one.question]}
                  placeholder={tr("Type your own answer")}
                  onChange={(event) => setTyped((was) => ({ ...was, [one.question]: event.target.value }))}
                />
              )}
            </div>
          </div>
        ))}
        {!done && (
          <div className="chat-ask-actions">
            <button className="button ghost small" onClick={() => onAnswer("deny")}>{tr("Skip")}</button>
            <button
              className="button primary small"
              disabled={!ready}
              onClick={() => {
                const answers = Object.fromEntries(questions.map((one) => [one.question, chosen(one.question).join(", ")]));
                onAnswer("allow", { ...item.input, answers });
              }}
            >
              {tr("Answer")}
            </button>
          </div>
        )}
      </div>
    );
  }

  if (done) {
    return (
      <div className={`chat-answered ${item.answer === "deny" ? "denied" : ""}`}>
        <Icon name={item.answer === "deny" ? "ban" : "shield-check"} />
        <span>
          {toolName(item.title || item.tool)} · {item.answer === "deny" ? tr("Denied") : item.answer === "always" ? tr("Allowed for this conversation") : tr("Allowed")}
        </span>
      </div>
    );
  }
  const summary = toolSummary(item.tool, item.input);
  return (
    <div className="chat-ask">
      <div className="chat-ask-head">
        <Icon name="shield" />
        <div>
          <strong>{tr("Allow {0}?", [toolName(item.title || item.tool)])}</strong>
          {item.description && <p>{item.description}</p>}
        </div>
      </div>
      {summary && <pre className="chat-ask-what">{summary}</pre>}
      <div className="chat-ask-actions">
        <button className="button ghost small" onClick={() => onAnswer("deny")}>{tr("Deny")}</button>
        {item.suggestions.length > 0 && (
          <button className="button secondary small" onClick={() => onAnswer("always")}>{tr("Allow for this conversation")}</button>
        )}
        <button className="button primary small" onClick={() => onAnswer("allow")}>{tr("Allow")}</button>
      </div>
    </div>
  );
}

function TodoPanel({ todos }: { todos: ChatTodo[] }) {
  const [open, setOpen] = useState(true);
  const done = todos.filter((one) => one.status === "completed").length;
  const now = todos.find((one) => one.status === "in_progress");
  return (
    <div className="chat-todos">
      <button className="chat-todos-head" aria-expanded={open} onClick={() => setOpen((was) => !was)}>
        <Icon name="list" />
        <strong>{now ? now.activeForm || now.content : tr("Todo list")}</strong>
        <span className="chat-todos-count">{done}/{todos.length}</span>
        <Icon name="chevron" />
      </button>
      {open && (
        <ul>
          {todos.map((one, index) => (
            <li key={index} className={one.status}>
              <span className="chat-todo-mark">{one.status === "in_progress" ? <span className="chat-spin" /> : one.status === "completed" ? <Icon name="check" /> : <i />}</span>
              <span>{one.content}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function ClaudeChat({
  server,
  engine,
  session,
  onError,
  onLogin,
}: {
  server: { id: string; name: string; minecraft: boolean };
  engine: Engine;
  /** "" starts a new chat, "last" continues the newest one, otherwise a Claude session id. */
  session: string;
  onError: (message: string) => void;
  onLogin: () => void;
}) {
  const { model, chatId: chat, ended } = useChatEntry(server.id);
  const setModel = useCallback(
    (change: ChatModel | ((was: ChatModel) => ChatModel)) =>
      setChatEntry(server.id, (now) => ({ model: typeof change === "function" ? change(now.model) : change })),
    [server.id],
  );
  const usage = engine === "codex" ? undefined : (model.usage ?? freshUsage(server.id));
  const who = engine === "codex" ? "Codex" : "Claude";
  const [opening, setOpening] = useState(true);
  const [draft, setDraft] = useState(() => savedDraft(server.id).text);
  const [pictures, setPictures] = useState<Picture[]>(() => draftPictures.get(server.id) ?? []);
  const [attached, setAttached] = useState<string[]>(() => savedDraft(server.id).attached);
  const draftOwner = useRef(server.id);
  useEffect(() => {
    if (draftOwner.current === server.id) return;
    draftOwner.current = server.id;
    const saved = savedDraft(server.id);
    setDraft(saved.text);
    setAttached(saved.attached);
    setPictures(draftPictures.get(server.id) ?? []);
  }, [server.id]);
  useEffect(() => {
    if (draftOwner.current !== server.id) return;
    saveDraft(server.id, { text: draft, attached });
  }, [server.id, draft, attached]);
  useEffect(() => {
    if (draftOwner.current !== server.id) return;
    if (pictures.length) draftPictures.set(server.id, pictures);
    else draftPictures.delete(server.id);
  }, [server.id, pictures]);
  const [dropping, setDropping] = useState(false);

  const attachPaths = useCallback(
    async (paths: string[]) => {
      if (!paths.length) return;
      try {
        const done = await commands.claudeAttachPaths(server.id, paths);
        setAttached((was) => [...was, ...done]);
      } catch (cause) {
        onError(String(cause));
      }
    },
    [onError, server.id],
  );

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let stop: (() => void) | undefined;
    let gone = false;
    void (async () => {
      const { getCurrentWebview } = await import("@tauri-apps/api/webview");
      const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === "over") setDropping(true);
        else if (event.payload.type === "drop") {
          setDropping(false);
          void attachPaths(event.payload.paths);
        } else setDropping(false);
      });
      if (gone) unlisten();
      else stop = unlisten;
    })();
    return () => {
      gone = true;
      stop?.();
    };
  }, [attachPaths]);

  const pickFiles = async () => {
    try {
      const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
      const chosen = await openDialog({ multiple: true, directory: false, title: tr("Files to send to {0}", [who]) });
      if (!chosen) return;
      await attachPaths(Array.isArray(chosen) ? chosen : [chosen]);
    } catch (cause) {
      onError(String(cause));
    }
  };
  const [mode, setMode] = useState<Mode>(() => savedMode(engine));
  const [pickedModel, setPickedModel] = useState(() => savedModel(engine));
  const [codexModels, setCodexModels] = useState<{ slug: string; name: string; description: string }[]>([]);
  useEffect(() => {
    if (engine !== "codex") return;
    void commands.codexModels().then(setCodexModels, () => setCodexModels([]));
  }, [engine]);
  const modeRef = useRef(mode);
  const modelRef = useRef(pickedModel);
  const chatRef = useRef<string | null>(null);
  const list = useRef<HTMLDivElement | null>(null);
  const stick = useRef(true);
  const input = useRef<HTMLTextAreaElement | null>(null);

  const open = useCallback(
    async (target: string) => {
      setOpening(true);
      try {
        chatRef.current = null;
        setChatEntry(server.id, { chatId: null });
        let start = emptyChat;
        if (target) for (const line of await commands.claudeChatHistory(server.id, engine, target === "last" ? "" : target)) start = applyLine(start, line);
        setModel(settle(start));
        const id = await commands.claudeChatOpen(server.id, target, modeRef.current, modelRef.current, engine);
        chatRef.current = id;
        setChatEntry(server.id, { chatId: id, ended: false });
        stick.current = true;
        input.current?.focus();
      } catch (cause) {
        onError(String(cause));
      } finally {
        setOpening(false);
      }
    },
    [engine, onError, server.id, setModel],
  );

  useEffect(() => {
    let gone = false;
    startChatListener();
    void (async () => {
      try {
        if (!session) {
          await open("");
          return;
        }
        const state = await commands.claudeChatState(server.id, engine, session === "last" ? "" : session);
        if (gone) return;
        if (state.id && chatEntry(server.id).chatId === state.id) {
          chatRef.current = state.id;
          setOpening(false);
          return;
        }
        if (state.id) {
          let start = emptyChat;
          for (const line of await commands.claudeChatHistory(server.id, engine, session === "last" ? "" : session)) start = applyLine(start, line);
          start = settle(start);
          for (const line of state.pending) start = applyLine(start, line);
          chatRef.current = state.id;
          setChatEntry(server.id, { chatId: state.id, model: start, ended: false });
          setOpening(false);
        } else {
          await open(session);
        }
      } catch (cause) {
        onError(String(cause));
        setOpening(false);
      }
    })();
    return () => {
      gone = true;
    };
  }, [engine, onError, open, server.id, session]);

  const changeMode = (next: string) => {
    const value = next as Mode;
    setMode(value);
    modeRef.current = value;
    remember(keyFor(MODE_KEY, engine), value);
    if (chatRef.current) void commands.claudeChatMode(chatRef.current, value).catch((cause) => onError(String(cause)));
  };
  const changeModel = (next: string) => {
    setPickedModel(next);
    modelRef.current = next;
    remember(keyFor(MODEL_KEY, engine), next);
    if (chatRef.current) void commands.claudeChatModel(chatRef.current, next).catch((cause) => onError(String(cause)));
  };

  useEffect(() => {
    const box = list.current;
    if (box && stick.current) box.scrollTop = box.scrollHeight;
  }, [model]);

  // Prompts go to the agent in English; it still replies in Korean.
  const send = async (text?: string) => {
    const typed = (text ?? draft).trim();
    if ((!typed && !pictures.length && !attached.length) || !chatRef.current) return;
    const words = attached.length
      ? `${typed}${typed ? "\n\n" : ""}Attached files (relative to the workspace):\n${attached.map((one) => `- ${one}`).join("\n")}`
      : typed;
    const content =
      pictures.length === 0
        ? words
        : [
            ...pictures.map((one) => ({ type: "image", source: { type: "base64", media_type: one.mediaType, data: one.data } })),
            ...(words ? [{ type: "text", text: words }] : []),
          ];
    try {
      await commands.claudeChatSend(chatRef.current, JSON.stringify(content));
      setDraft("");
      setPictures([]);
      setAttached([]);
      stick.current = true;
    } catch (cause) {
      onError(String(cause));
    }
  };

  const answer = async (item: Extract<ChatItem, { kind: "permission" }>, choice: "allow" | "always" | "deny", changed?: Record<string, unknown>) => {
    if (!chatRef.current) return;
    try {
      await commands.claudeChatAnswer(
        chatRef.current,
        item.requestId,
        choice !== "deny",
        choice === "deny" ? null : JSON.stringify(changed ?? item.input),
        choice === "always" ? JSON.stringify(item.suggestions) : null,
        choice === "deny" ? "The user declined this from the Monolab app." : null,
      );
      setModel((was) => markAnswered(was, item.requestId, choice));
    } catch (cause) {
      onError(String(cause));
    }
  };

  const autoAnswered = useRef(new Set<string>());
  useEffect(() => {
    if (mode !== "bypassPermissions") return;
    for (const one of model.items) {
      if (one.kind !== "permission" || one.answer || one.tool === "AskUserQuestion" || autoAnswered.current.has(one.requestId)) continue;
      autoAnswered.current.add(one.requestId);
      void answer(one, "allow");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, model.items]);

  const stopReply = () => {
    if (!chatRef.current) return;
    void commands.claudeChatInterrupt(chatRef.current).then(
      () => setModel((was) => settle(was)),
      (cause) => onError(String(cause)),
    );
  };

  const onPaste = (event: ClipboardEvent) => {
    const all = [...event.clipboardData.items].filter((one) => one.kind === "file");
    const files = all.filter((one) => one.type.startsWith("image/")).map((one) => one.getAsFile());
    const others = all.filter((one) => !one.type.startsWith("image/")).map((one) => one.getAsFile());
    if (!files.length && !others.length) return;
    event.preventDefault();
    for (const file of others) {
      if (!file) continue;
      const reader = new FileReader();
      reader.onload = () => {
        const url = String(reader.result);
        void commands
          .claudeAttachBytes(server.id, file.name || "file", url.slice(url.indexOf(",") + 1))
          .then((done) => setAttached((was) => [...was, done]), (cause) => onError(String(cause)));
      };
      reader.readAsDataURL(file);
    }
    for (const file of files) {
      if (!file) continue;
      const reader = new FileReader();
      reader.onload = () => {
        const url = String(reader.result);
        const data = url.slice(url.indexOf(",") + 1);
        setPictures((was) => [...was, { mediaType: file.type, data, url }]);
      };
      reader.readAsDataURL(file);
    }
  };

  const waiting = model.items.some((one) => one.kind === "permission" && !one.answer);
  const empty = model.items.length === 0;

  return (
    <div className="chat">
      <div
        className="chat-list"
        ref={list}
        onScroll={(event) => {
          const box = event.currentTarget;
          stick.current = box.scrollHeight - box.scrollTop - box.clientHeight < 48;
        }}
      >
        {empty && !opening && (
          <div className="chat-hello">
            <Icon name="sparkles" />
            <strong>{tr("Chat with the {0} agent", [server.name])}</strong>
            <p className="muted">{engine === "codex" ? tr("It can read and edit server files, check logs and build plugins for you. It works without asking, within the scope you pick below.") : server.minecraft ? tr("Reads and edits server files, checks logs and creates plugins. Asks here before changing files or running commands.") : tr("Reads and edits server files and checks logs. Asks here before changing files or running commands.")}</p>
            <div className="chat-starters">
              {(server.minecraft ? MINECRAFT_STARTERS : GAME_STARTERS).map((one) => (
                <button
                  key={one.title}
                  className="chat-starter"
                  onClick={() => {
                    void send(one.prompt);
                  }}
                >
                  <Icon name={one.icon} />
                  <span>{tr(one.title)}</span>
                </button>
              ))}
            </div>
          </div>
        )}
        {model.items.map((item) => {
          switch (item.kind) {
            case "user":
              return (
                <div key={item.key} className="chat-user">
                  <div className="chat-bubble">
                  {item.images > 0 && <span className="chat-pictures"><Icon name="image" />{tr("{0, plural, one {# image} other {# images}}", [item.images])}</span>}
                  {item.text && <span>{item.text}</span>}
                  </div>
                </div>
              );
            case "assistant":
              return (
                <div key={item.key} className={`chat-assistant ${item.streaming ? "streaming" : ""}`}>
                  <Markdown text={item.text} />
                </div>
              );
            case "tool":
              return item.name === "TodoWrite" ? null : <ToolRow key={item.key} item={item} />;
            case "summary":
              return (
                <details key={item.key} className="chat-summary">
                  <summary>
                    <Icon name="history" />
                    <span>{tr("Earlier conversation summarized")}</span>
                    <small>{tr("The chat got long, so Claude summarized the earlier part to keep going.")}</small>
                    <Icon name="chevron" />
                  </summary>
                  <div className="chat-summary-body">
                    <Markdown text={item.text} />
                  </div>
                </details>
              );
            case "permission":
              return <PermissionCard key={item.key} item={item} onAnswer={(choice, changed) => void answer(item, choice, changed)} />;
            case "note":
              return (
                <div key={item.key} className={`chat-note ${item.tone}`}>
                  <span>{item.text}</span>
                  {item.login && (
                    <button className="button secondary small" onClick={onLogin}>
                      <Icon name="users" />
                      {tr("Sign in")}
                    </button>
                  )}
                </div>
              );
          }
        })}
        {model.busy && !waiting && (
          <div className="chat-thinking">
            <span className="chat-spin" />
            {tr("Thinking…")}
          </div>
        )}
      </div>
      {ended ? (
        <div className="chat-ended">
          <span>{tr("This conversation has ended. Reopen it to pick up where you left off.")}</span>
          <button className="button primary small" onClick={() => void open(model.session || session || "last")}>{tr("Reopen")}</button>
        </div>
      ) : (
        <div className="chat-compose">
          {model.todos && model.todos.some((one) => one.status !== "completed") && <TodoPanel todos={model.todos} />}
          {dropping && <div className="chat-drop">{tr("Drop here to send files to {0}", [who])}</div>}
          {(pictures.length > 0 || attached.length > 0) && (
            <div className="chat-attached">
              {attached.map((one, index) => (
                <span key={one} className="chat-file" data-tip={one}>
                  <Icon name="file" />
                  <span>{one.replace(/^attachments\//, "")}</span>
                  <button aria-label={tr("Remove")} onClick={() => setAttached((was) => was.filter((_, at) => at !== index))}>
                    <Icon name="close" />
                  </button>
                </span>
              ))}
              {pictures.map((one, index) => (
                <span key={one.url.slice(-24) + index} className="chat-thumb">
                  <img src={one.url} alt="" />
                  <button aria-label={tr("Remove")} onClick={() => setPictures((was) => was.filter((_, at) => at !== index))}>
                    <Icon name="close" />
                  </button>
                </span>
              ))}
            </div>
          )}
          <div className="chat-input">
            <textarea
              ref={input}
              rows={1}
              value={draft}
              disabled={!chat || opening}
              placeholder={opening ? tr("Opening…") : tr("How can I help? (Enter to send · Shift+Enter for a new line · paste or drop images and files)")}
              onChange={(event) => setDraft(event.target.value)}
              onPaste={onPaste}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                  event.preventDefault();
                  void send();
                }
              }}
            />
            <div className="chat-bar">
              <button
                className="icon-button chat-clip"
                type="button"
                aria-label={tr("Attach files")}
                data-tip={tr("Attach files")}
                disabled={!chat}
                onClick={() => void pickFiles()}
              >
                <Icon name="paperclip" />
              </button>
              <Dropdown
                value={pickedModel}
                onChange={changeModel}
                className="chat-pick"
                options={
                  engine === "codex"
                    ? [
                        { value: "default", label: tr("Default model"), hint: tr("Your Codex setting, or the first model below if Codex can't use it here") },
                        ...codexModels.map((one) => ({ value: one.slug, label: one.name, hint: one.description })),
                      ]
                    : [
                        { value: "default", label: tr("Default model"), hint: model.model || tr("Chosen by Claude Code") },
                        { value: "fable", label: "Fable" },
                        { value: "opus", label: "Opus" },
                        { value: "sonnet", label: "Sonnet" },
                        { value: "haiku", label: "Haiku" },
                      ]
                }
              />
              <span className={`chat-pick-wrap ${mode === "bypassPermissions" ? "warn" : ""}`} data-tip={mode === "bypassPermissions" ? tr("Starts/stops servers, runs commands and deletes files without asking.") : undefined}>
                <Dropdown
                  value={mode}
                  onChange={changeMode}
                  className="chat-pick"
                  options={
                    engine === "codex"
                      ? [
                          { value: "default", label: tr("Read-only commands"), hint: tr("Shell commands can't change files. Server tools still work") },
                          { value: "acceptEdits", label: tr("Edit the workspace"), hint: tr("Can create files and build inside the workspace") },
                          { value: "bypassPermissions", label: tr("Allow all"), hint: tr("No limits anywhere on this computer") },
                        ]
                      : [
                          { value: "default", label: tr("Ask every time"), hint: tr("Asks before using each tool") },
                          { value: "acceptEdits", label: tr("Auto-accept edits"), hint: tr("Automatically edits workspace files; asks before running commands") },
                          { value: "bypassPermissions", label: tr("Allow all"), hint: tr("Doesn't ask for permission") },
                        ]
                  }
                />
              </span>
              <span className="chat-usages">
                {usage?.fiveHour && <UsageMeter label={tr("5 h")} limit={usage.fiveHour} />}
                {usage?.sevenDay && <UsageMeter label={tr("7 d")} limit={usage.sevenDay} />}
              </span>
              {model.busy ? (
                <button className="button secondary small chat-send" onClick={stopReply} data-tip={tr("Stop the current reply")}>
                  <Icon name="stop" />
                  {tr("Stop")}
                </button>
              ) : (
                <button className="button primary small chat-send" disabled={!chat || (!draft.trim() && !pictures.length && !attached.length)} onClick={() => void send()}>
                  <Icon name="arrow-up" />
                  {tr("Send")}
                </button>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

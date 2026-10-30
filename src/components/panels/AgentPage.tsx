import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { createPortal } from "react-dom";
import { commands } from "../../lib/ipc";
import type { ChatSession, ClaudeAccounts, ClaudeInfo } from "../../lib/bindings";
import { setServerAccount, useChatEntry } from "../../lib/claudeChatStore";
import { AskDialog, type AskRequest } from "../dialogs/AskDialog";
import { tr } from "../../lib/i18n";
import { EmptyState, SegmentedControl } from "../primitives";
import { ClaudeChat, type Engine } from "./ClaudeChat";
import { Icon } from "../ui/Icon";

type Login = { engine: Engine; account: string; url: string };

const INSTALL: Record<Engine, string> = {
  claude: "irm https://claude.ai/install.ps1 | iex",
  codex: "npm install -g @openai/codex",
};
const ENGINE_KEY = "monolab-agent-engine:";

function savedEngine(server: string): Engine | null {
  try {
    const value = localStorage.getItem(ENGINE_KEY + server);
    return value === "claude" || value === "codex" ? value : null;
  } catch {
    return null;
  }
}

function dayGroup(at: number) {
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  const today = start.getTime();
  if (at >= today) return tr("Today");
  if (at >= today - 86400000) return tr("Yesterday");
  if (at >= today - 7 * 86400000) return tr("Previous 7 days");
  if (at >= today - 30 * 86400000) return tr("Previous 30 days");
  return tr("Older");
}

function ChatList({
  server,
  list,
  current,
  onPick,
  onNew,
  onChanged,
  onError,
  folded,
  onFold,
}: {
  server: string;
  list: ChatSession[];
  current: string;
  folded: boolean;
  onFold: (folded: boolean) => void;
  onPick: (id: string) => void;
  onNew: () => void;
  onChanged: (deleted?: string) => void;
  onError: (message: string) => void;
}) {
  const [renaming, setRenaming] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [ask, setAsk] = useState<AskRequest | null>(null);
  const groups: { label: string; rows: ChatSession[] }[] = [];
  for (const one of list) {
    const label = dayGroup(one.updated ?? 0);
    const last = groups[groups.length - 1];
    if (last?.label === label) last.rows.push(one);
    else groups.push({ label, rows: [one] });
  }
  const saveName = (id: string) => {
    setRenaming(null);
    void commands.claudeChatRename(id, name).then(() => onChanged(), (cause) => onError(String(cause)));
  };
  const remove = (one: ChatSession) => {
    setAsk({
      title: tr("Delete this conversation?"),
      body: tr("This deletes the conversation history of \"{0}\". The notes in AGENTS.md stay.", [one.title || tr("New chat")]),
      confirmLabel: tr("Delete"),
      danger: true,
      resolve: (value) => {
        if (value === null) return;
        void commands.claudeChatDelete(server, one.id).then(() => onChanged(one.id), (cause) => onError(String(cause)));
      },
    });
  };
  if (folded) {
    return (
      <nav className="chat-sessions folded" aria-label={tr("Conversations")}>
        <button className="wb-icon ghost" aria-label={tr("Show conversations")} data-tip={tr("Show conversations")} onClick={() => onFold(false)}>
          <Icon name="panel-open" />
        </button>
        <button className="wb-icon ghost" aria-label={tr("New chat")} data-tip={tr("New chat")} onClick={onNew}>
          <Icon name="plus" />
        </button>
      </nav>
    );
  }
  return (
    <nav className="chat-sessions" aria-label={tr("Conversations")}>
      <div className="chat-session-top">
        <button className="chat-session-new" onClick={onNew}>
          <Icon name="plus" />
          {tr("New chat")}
        </button>
        <button className="wb-icon ghost" aria-label={tr("Hide conversations")} data-tip={tr("Hide conversations")} onClick={() => onFold(true)}>
          <Icon name="panel-close" />
        </button>
      </div>
      <div className="chat-session-scroll">
        {list.length === 0 && <p className="chat-session-empty">{tr("Conversations show up here.")}</p>}
        {groups.map((group) => (
          <section key={group.label}>
            <h3>{group.label}</h3>
            {group.rows.map((one) => {
              const on = one.id === current;
              return (
                <div key={one.id} className={`chat-session ${on ? "on" : ""}`}>
                  {renaming === one.id ? (
                    <input
                      autoFocus
                      value={name}
                      aria-label={tr("Chat name")}
                      onChange={(event) => setName(event.target.value)}
                      onBlur={() => saveName(one.id)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") saveName(one.id);
                        if (event.key === "Escape") setRenaming(null);
                      }}
                    />
                  ) : (
                    <button className="chat-session-pick" aria-current={on || undefined} onClick={() => !on && onPick(one.id)} data-tip={one.title}>
                      {one.busy && <span className="chat-spin" aria-label={tr("Working")} />}
                      <span>{one.title || tr("New chat")}</span>
                    </button>
                  )}
                  {on && renaming !== one.id && (
                    <span className="chat-session-tools">
                      <button
                        className="wb-icon ghost"
                        aria-label={tr("Rename")}
                        data-tip={tr("Rename")}
                        onClick={() => {
                          setName(one.title);
                          setRenaming(one.id);
                        }}
                      >
                        <Icon name="pencil" />
                      </button>
                      <button className="wb-icon ghost danger" aria-label={tr("Delete")} data-tip={tr("Delete")} onClick={() => remove(one)}>
                        <Icon name="trash" />
                      </button>
                    </span>
                  )}
                </div>
              );
            })}
          </section>
        ))}
      </div>
      {ask && <AskDialog request={ask} onClose={() => setAsk(null)} />}
    </nav>
  );
}

function AccountManager({
  list,
  onChanged,
  onPick,
  onLogin,
  onError,
  onClose,
}: {
  list: ClaudeAccounts;
  onChanged: () => void;
  onPick: (id: string) => void;
  onLogin: (id: string) => void;
  onError: (message: string) => void;
  onClose: () => void;
}) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const [draft, setDraft] = useState("");
  const [names, setNames] = useState<Record<string, string>>({});
  const run = async (work: () => Promise<unknown>) => {
    try {
      await work();
      onChanged();
    } catch (cause) {
      onError(String(cause));
    }
  };
  return createPortal(
    <div
      className="modal-backdrop ask-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
    <div className="ask-dialog claude-accounts" role="dialog" aria-modal="true">
      <div className="claude-accounts-head">
        <h2>{tr("Claude accounts")}</h2>
        <button className="close in-flow" aria-label={tr("Close")} data-tip={tr("Close")} onClick={onClose}>
          <Icon name="close" />
        </button>
      </div>
      <p className="modal-copy">{tr("Choose an account for this server. Each account only needs to sign in once. Switching accounts on the same server keeps the conversation going.")}</p>
      <div className="claude-account-list">
      {list.accounts.map((one) => {
        const current = one.id === list.current;
        const label = one.id === "default" ? tr("Default account") : names[one.id] ?? one.name;
        return (
        <div
          className={`claude-account-row ${current ? "on" : ""}`}
          key={one.id}
          role="radio"
          aria-checked={current}
          tabIndex={0}
          onClick={(event) => {
            if ((event.target as HTMLElement).closest("input, button")) return;
            if (!current) onPick(one.id);
          }}
          onKeyDown={(event) => {
            if ((event.key === "Enter" || event.key === " ") && event.target === event.currentTarget && !current) onPick(one.id);
          }}
        >
          <span className="claude-account-avatar" aria-hidden>
            {(one.email || label || "?").slice(0, 1).toUpperCase()}
          </span>
          <span className="claude-account-main">
            {one.id === "default" ? (
              <span className="claude-account-name">{label}</span>
            ) : (
              <input
                className="claude-account-name"
                value={label}
                aria-label={tr("Account name")}
                onChange={(event) => setNames({ ...names, [one.id]: event.target.value })}
                onBlur={() => {
                  const next = (names[one.id] ?? one.name).trim();
                  if (next && next !== one.name) void run(() => commands.claudeAccountRename(one.id, next));
                }}
              />
            )}
            <small className="claude-account-who" data-tip={[one.email, one.displayName, one.plan].filter(Boolean).join(" · ") || undefined}>
              {one.loggedIn ? [one.email || tr("Signed in"), one.plan].filter(Boolean).join(" · ") : tr("Not signed in")}
            </small>
          </span>
          {!one.loggedIn ? (
            <button className="button secondary small" onClick={() => onLogin(one.id)}>
              {tr("Sign in")}
            </button>
          ) : current ? (
            <span className="claude-account-current">
              <Icon name="check" />
              {tr("In use")}
            </span>
          ) : (
            <button className="button ghost small" onClick={() => onPick(one.id)}>
              {tr("Use")}
            </button>
          )}
          {one.loggedIn && (
            <button
              className="wb-icon ghost"
              aria-label={tr("Sign out")}
              data-tip={tr("Sign out")}
              onClick={() => void run(() => commands.claudeAccountLogout(one.id))}
            >
              <Icon name="exit" />
            </button>
          )}
          {one.id !== "default" && (
            <button
              className="wb-icon ghost danger"
              aria-label={tr("Remove account")}
              data-tip={tr("Remove account — removes sign-in details, keeps chat history")}
              onClick={() => void run(() => commands.claudeAccountRemove(one.id))}
            >
              <Icon name="trash" />
            </button>
          )}
        </div>
        );
      })}
      <form
        className="claude-account-add"
        onSubmit={(event) => {
          event.preventDefault();
          const name = draft.trim();
          if (!name) return;
          void run(async () => {
            await commands.claudeAccountAdd(name);
            setDraft("");
          });
        }}
      >
        <span className="claude-account-avatar add" aria-hidden>
          <Icon name="plus" />
        </span>
        <input value={draft} placeholder={tr("New account name (e.g. Friend)")} onChange={(event) => setDraft(event.target.value)} />
        <button className="button secondary small" type="submit" disabled={!draft.trim()}>
          {tr("Add")}
        </button>
      </form>
      </div>
    </div>
    </div>,
    document.body,
  );
}

export function AgentPage({ server, onError }: { server: { id: string; name: string; minecraft: boolean }; onError: (message: string) => void }) {
  const [info, setInfo] = useState<ClaudeInfo | null>(null);
  const [engine, setEngine] = useState<Engine>(() => savedEngine(server.id) ?? "claude");
  const [sessions, setSessions] = useState<ChatSession[] | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [pickSerial, setPickSerial] = useState(0);
  const [listFolded, setListFolded] = useState(() => {
    try {
      return localStorage.getItem("monolab-chat-list-folded") === "1";
    } catch {
      return false;
    }
  });
  const foldList = (next: boolean) => {
    setListFolded(next);
    try {
      localStorage.setItem("monolab-chat-list-folded", next ? "1" : "0");
    } catch {
      /* keep it for this session only */
    }
  };
  const entry = useChatEntry(server.id);
  const [copied, setCopied] = useState(false);
  const [accounts, setAccounts] = useState<ClaudeAccounts | null>(null);
  const [managing, setManaging] = useState(false);
  const [login, setLogin] = useState<Login | null>(null);
  const [code, setCode] = useState("");
  const [signedIn, setSignedIn] = useState(0);

  const loadInfo = useCallback(() => {
    void commands.claudeInfo(server.id).then((got) => {
      setInfo(got);
      if (!savedEngine(server.id) && !got.path && got.codex) setEngine("codex");
    }, (cause) => onError(String(cause)));
  }, [onError, server.id]);
  useEffect(loadInfo, [loadInfo]);

  const loadAccounts = useCallback(() => {
    void commands.claudeAccounts(server.id).then((list) => {
      setServerAccount(server.id, list.current);
      setAccounts(list);
    }, (cause) => onError(String(cause)));
  }, [onError, server.id]);
  useEffect(loadAccounts, [loadAccounts]);

  const loadSessions = useCallback(() => {
    void commands.claudeChatSessions(server.id).then(setSessions, () => setSessions([]));
  }, [server.id]);
  useEffect(loadSessions, [loadSessions, entry.model.busy, entry.model.session]);
  const anyBusy = sessions?.some((one) => one.busy) ?? false;
  useEffect(() => {
    if (!anyBusy) return;
    const timer = window.setInterval(loadSessions, 5000);
    return () => window.clearInterval(timer);
  }, [anyBusy, loadSessions]);
  useEffect(() => {
    if (picked !== null) return;
    if (engine === "codex") setPicked("last");
    else if (sessions) setPicked((sessions.find((one) => one.chat) ?? sessions[0])?.id ?? "");
  }, [engine, picked, sessions]);
  const pick = (id: string) => {
    setPicked(id);
    setPickSerial((was) => was + 1);
  };
  const currentSession = entry.chatId ? entry.model.session || picked || "" : picked || "";

  const pickEngine = (next: Engine) => {
    setPicked(null);
    setEngine(next);
    setManaging(false);
    try {
      localStorage.setItem(ENGINE_KEY + server.id, next);
    } catch {}
  };

  const waiting = login !== null;
  useEffect(() => {
    if (!waiting) return;
    let stop: (() => void) | undefined;
    let gone = false;
    void listen<string>("claude-login", (event) => {
      const found = event.payload.match(/https:\/\/\S+/);
      if (found) setLogin((was) => (was ? { ...was, url: found[0] } : was));
    }).then((off) => {
      if (gone) off();
      else stop = off;
    });
    return () => {
      gone = true;
      stop?.();
    };
  }, [waiting]);

  const startLogin = (which: Engine, account: string) => {
    setManaging(false);
    setCode("");
    setLogin({ engine: which, account, url: "" });
    const work = which === "codex" ? commands.codexLogin() : commands.claudeAccountLogin(account);
    work.then(
      () => {
        setLogin(null);
        loadAccounts();
        loadInfo();
        setSignedIn((was) => was + 1);
      },
      (cause) => {
        setLogin(null);
        onError(String(cause));
      },
    );
  };

  const currentOne = accounts?.accounts.find((one) => one.id === accounts.current);
  const currentName = currentOne ? (currentOne.id === "default" ? tr(currentOne.name) : currentOne.name) : "";
  const switchAccount = (id: string) => {
    void commands.claudeAccountUse(server.id, id).then(loadAccounts, (cause) => onError(String(cause)));
  };

  const copyInstall = () => {
    void navigator.clipboard.writeText(INSTALL[engine]).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    });
  };

  const installed = info === null ? null : engine === "codex" ? Boolean(info.codex) : Boolean(info.path);
  const codexOut = engine === "codex" && installed === true && info !== null && !info.codexLoggedIn;
  const name = engine === "codex" ? "Codex" : "Claude Code";
  const who =
    engine === "codex"
      ? info?.codexLoggedIn
        ? [info.codexEmail, info.codexPlan].filter(Boolean).join(" · ")
        : ""
      : currentOne?.loggedIn
        ? [currentOne.email, currentOne.plan].filter(Boolean).join(" · ")
        : "";

  return (
    <div className="claude-page">
      <div className="claude-bar">
        <SegmentedControl
          className="agent-engine"
          value={engine}
          onChange={pickEngine}
          options={[
            { value: "claude", label: "Claude" },
            { value: "codex", label: "Codex" },
          ]}
        />
        {installed && who && (
          <span className="claude-bar-who" data-tip={tr("Signed in as {0}", [who])}>
            <Icon name="user" />
            {who}
          </span>
        )}
        {engine === "claude" && installed && (
          <button
            className={`icon-button ${managing ? "on" : ""}`}
            aria-label={tr("Claude accounts")}
            data-tip={currentName ? tr("Claude account — {0}", [currentName]) : tr("Claude accounts")}
            onClick={() => setManaging((was) => !was)}
          >
            <Icon name="users" />
          </button>
        )}
        <button
          className="icon-button"
          aria-label={tr("Open workspace")}
          onClick={() => void commands.claudeOpenFolder(server.id).catch((cause) => onError(String(cause)))}
          data-tip={info?.workspace ? tr("Open workspace — {0}", [info.workspace]) : tr("Open workspace")}
        >
          <Icon name="folder" />
        </button>
        {installed && engine === "codex" && (
          <button className="button secondary small" onClick={() => pick("")} data-tip={tr("Starts over instead of continuing. The notes in AGENTS.md stay.")}>
            <Icon name="plus" />
            {tr("New chat")}
          </button>
        )}
      </div>
      {login && (
        <div className="claude-login">
          <div className="claude-login-text">
            <strong>{tr("Sign in using your browser")}</strong>
            <span className="muted">{tr("This window closes automatically once you're signed in.")}</span>
            {login.url && (
              <a href={login.url} target="_blank" rel="noreferrer">
                {tr("Browser didn't open? Click here")}
              </a>
            )}
          </div>
          <form
            className="claude-login-code"
            onSubmit={(event) => {
              event.preventDefault();
              if (!code.trim()) return;
              void commands.claudeAccountLoginCode(code).then(() => setCode(""), (cause) => onError(String(cause)));
            }}
          >
            <input value={code} placeholder={tr("Got a code? Paste it here")} onChange={(event) => setCode(event.target.value)} />
            <button className="button secondary small" type="submit" disabled={!code.trim()}>
              {tr("Send")}
            </button>
          </form>
          <button className="button ghost small" onClick={() => void commands.claudeAccountLoginCancel()}>
            {tr("Cancel")}
          </button>
        </div>
      )}
      {managing && accounts && engine === "claude" && (
        <AccountManager
          list={accounts}
          onChanged={loadAccounts}
          onPick={switchAccount}
          onLogin={(id) => startLogin("claude", id)}
          onError={onError}
          onClose={() => setManaging(false)}
        />
      )}
      {installed === false ? (
        <EmptyState
          icon="terminal"
          title={tr("{0} isn't installed on this computer", [name])}
          hint={
            engine === "codex"
              ? tr("Install the Codex app, or if you have Node.js, paste the line below into PowerShell. Then reopen this tab.")
              : tr("Open PowerShell and paste the line below to install. Then reopen this tab.")
          }
          action={
            <div className="claude-install">
              <code>{INSTALL[engine]}</code>
              <button className={`button secondary small copy-mark ${copied ? "done" : ""}`} onClick={copyInstall}>
                <Icon name={copied ? "check" : "copy"} />
                {copied ? tr("Copied") : tr("Copy")}
              </button>
            </div>
          }
        />
      ) : codexOut && !login ? (
        <EmptyState
          icon="users"
          title={tr("Sign in to Codex")}
          hint={tr("Sign in once with your ChatGPT account. Your browser will open.")}
          action={
            <button className="button primary" onClick={() => startLogin("codex", "default")}>
              {tr("Sign in")}
            </button>
          }
        />
      ) : installed ? (
        <>
        {engine === "claude" && currentOne && !currentOne.loggedIn && !login && (
          <div className="agent-signin">
            <Icon name="users" />
            <span>
              <strong>{tr("This Claude account isn't signed in")}</strong>
              <small className="muted">{tr("Sign in once and your browser will open. You can still try chatting if you signed in another way.")}</small>
            </span>
            <button className="button primary small" onClick={() => startLogin("claude", currentOne.id)}>
              {tr("Sign in")}
            </button>
          </div>
        )}
        <div className={`claude-main ${engine === "claude" ? "with-list" : ""} ${listFolded ? "list-folded" : ""}`}>
          {engine === "claude" && sessions && (
            <ChatList
              list={sessions}
              current={currentSession}
              onPick={pick}
              onNew={() => pick("")}
              onChanged={(deleted) => {
                if (deleted && deleted === currentSession) pick("");
                loadSessions();
              }}
              onError={onError}
              server={server.id}
              folded={listFolded}
              onFold={foldList}
            />
          )}
          {picked !== null && (
            <ClaudeChat
              key={`${engine}-${accounts?.current}-${signedIn}-${pickSerial}`}
              server={server}
              engine={engine}
              session={picked}
              onError={onError}
              onLogin={() => startLogin(engine, accounts?.current ?? "default")}
            />
          )}
        </div>
        </>
      ) : null}
    </div>
  );
}

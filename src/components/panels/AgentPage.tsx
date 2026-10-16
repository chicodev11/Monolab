import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { createPortal } from "react-dom";
import { commands } from "../../lib/ipc";
import type { ClaudeAccounts, ClaudeInfo } from "../../lib/bindings";
import { setServerAccount } from "../../lib/claudeChatStore";
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
      {list.accounts.map((one) => (
        <div
          className={`claude-account-row ${one.id === list.current ? "on" : ""}`}
          key={one.id}
          role="radio"
          aria-checked={one.id === list.current}
          tabIndex={0}
          onClick={(event) => {
            if ((event.target as HTMLElement).closest("input, button")) return;
            if (one.id !== list.current) onPick(one.id);
          }}
          onKeyDown={(event) => {
            if ((event.key === "Enter" || event.key === " ") && event.target === event.currentTarget && one.id !== list.current) onPick(one.id);
          }}
        >
          <span className="claude-account-radio" aria-hidden>
            {one.id === list.current && <Icon name="check" />}
          </span>
          {one.id === "default" ? (
            <span className="claude-account-name">{tr(one.name)}</span>
          ) : (
            <input
              className="claude-account-name"
              value={names[one.id] ?? one.name}
              onChange={(event) => setNames({ ...names, [one.id]: event.target.value })}
              onBlur={() => {
                const next = (names[one.id] ?? one.name).trim();
                if (next && next !== one.name) void run(() => commands.claudeAccountRename(one.id, next));
              }}
            />
          )}
          {one.loggedIn ? (
            <small className="claude-account-in">{tr("Signed in")}</small>
          ) : (
            <button className="button secondary small" onClick={() => onLogin(one.id)}>
              {tr("Sign in")}
            </button>
          )}
          {one.id !== "default" && (
            <button
              className="icon-button danger"
              aria-label={tr("Remove account")}
              data-tip={tr("Remove account — removes sign-in details, keeps chat history")}
              onClick={() => void run(() => commands.claudeAccountRemove(one.id))}
            >
              <Icon name="trash" />
            </button>
          )}
        </div>
      ))}
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
        <input value={draft} placeholder={tr("New account name (e.g. Friend)")} onChange={(event) => setDraft(event.target.value)} />
        <button className="button secondary small" type="submit" disabled={!draft.trim()}>
          <Icon name="plus" />
          {tr("Add")}
        </button>
      </form>
    </div>
    </div>,
    document.body,
  );
}

export function AgentPage({ server, onError }: { server: { id: string; name: string }; onError: (message: string) => void }) {
  const [info, setInfo] = useState<ClaudeInfo | null>(null);
  const [engine, setEngine] = useState<Engine>(() => savedEngine(server.id) ?? "claude");
  const [fresh, setFresh] = useState(0);
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

  const pickEngine = (next: Engine) => {
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
        <span className="claude-bar-note muted">{tr("An agent for this server only. Both share the notes in AGENTS.md.")}</span>
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
        {installed && (
          <button className="button secondary small" onClick={() => setFresh((was) => was + 1)} data-tip={tr("Starts over instead of continuing. The notes in AGENTS.md stay.")}>
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
        <ClaudeChat
          key={`${engine}-${accounts?.current}-${signedIn}`}
          server={server}
          engine={engine}
          fresh={fresh}
          onError={onError}
          onLogin={() => startLogin(engine, accounts?.current ?? "default")}
        />
      ) : null}
    </div>
  );
}

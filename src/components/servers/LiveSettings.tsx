import { useCallback, useEffect, useMemo, useState } from "react";
import type { MsmpRule, MsmpSetting, MsmpState } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { SearchField, Switch } from "../primitives";
import { Icon } from "../ui/Icon";

/** Names for the management server's settings; labels are made when drawn. */
function settingLabel(key: string): string {
  switch (key) {
    case "difficulty":
      return tr("Difficulty");
    case "game_mode":
      return tr("Default game mode");
    case "force_game_mode":
      return tr("Force game mode");
    case "max_players":
      return tr("Max players");
    case "motd":
      return tr("MOTD");
    case "view_distance":
      return tr("View distance");
    case "simulation_distance":
      return tr("Simulation distance");
    case "allow_flight":
      return tr("Allow flight");
    case "spawn_protection_radius":
      return tr("Spawn protection radius");
    case "player_idle_timeout":
      return tr("Idle kick (minutes)");
    case "pause_when_empty_seconds":
      return tr("Pause when empty (seconds)");
    case "autosave":
      return tr("Autosave");
    case "use_allowlist":
      return tr("Whitelist");
    case "enforce_allowlist":
      return tr("Enforce whitelist");
    case "accept_transfers":
      return tr("Accept transfers");
    case "hide_online_players":
      return tr("Hide online players");
    case "status_replies":
      return tr("Answer server list pings");
    case "status_heartbeat_interval":
      return tr("Status heartbeat (seconds)");
    case "entity_broadcast_range":
      return tr("Entity broadcast range (%)");
    case "operator_user_permission_level":
      return tr("Operator permission level");
    default:
      return key;
  }
}

const CHOICES: Record<string, string[]> = {
  difficulty: ["peaceful", "easy", "normal", "hard"],
  game_mode: ["survival", "creative", "adventure", "spectator"],
};

function parse(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

/** A text or number box that sends its value on Enter or when it loses focus. */
function Typed({ value, numeric, label, onSend }: { value: string; numeric: boolean; label: string; onSend: (next: string) => void }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const send = () => {
    if (draft === value) return;
    if (numeric && !/^-?\d+$/.test(draft.trim())) return setDraft(value);
    onSend(numeric ? draft.trim() : JSON.stringify(draft));
  };
  return (
    <input
      aria-label={label}
      className={numeric ? "msmp-number" : "msmp-text"}
      type={numeric ? "number" : "text"}
      value={draft}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={send}
      onKeyDown={(event) => {
        if (event.key === "Enter") send();
        if (event.key === "Escape") setDraft(value);
      }}
    />
  );
}

export function LiveSettings({
  serverId,
  status,
  ruleName,
  onError,
  onNotice,
  embedded = false,
}: {
  serverId: string;
  status: string;
  ruleName: (key: string) => string;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
  /** On the Settings page: always open, and says why when there's nothing to change. */
  embedded?: boolean;
}) {
  const [state, setState] = useState<MsmpState | null>(null);
  const [settings, setSettings] = useState<MsmpSetting[]>([]);
  const [rules, setRules] = useState<MsmpRule[]>([]);
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState(false);
  // Folded by default so the settings page starts with its own settings; the choice is remembered.
  const [openChoice, setOpen] = useState(() => {
    try {
      return localStorage.getItem("monolab.liveSettings") === "open";
    } catch {
      return false;
    }
  });
  const open = embedded || openChoice;
  const toggle = (next: boolean) => {
    setOpen(next);
    try {
      localStorage.setItem("monolab.liveSettings", next ? "open" : "closed");
    } catch {
      // Only remembers the choice.
    }
  };

  const load = useCallback(async () => {
    try {
      const next = await commands.msmpState(serverId);
      setState(next);
      if (next.connected) {
        const [list, ruleList] = await Promise.all([commands.msmpSettings(serverId), commands.msmpRules(serverId)]);
        setSettings(list);
        setRules(ruleList);
      }
    } catch (cause) {
      onError(String(cause));
    }
  }, [serverId, onError]);

  // Reconnect when the server starts or stops; the management server comes up with it.
  useEffect(() => {
    void load();
  }, [load, status]);

  const shownRules = useMemo(() => {
    const words = query.trim().toLowerCase();
    if (!words) return rules;
    return rules.filter((one) => one.key.toLowerCase().includes(words) || ruleName(one.key).toLowerCase().includes(words));
  }, [rules, query, ruleName]);

  async function setup() {
    setBusy(true);
    try {
      await commands.msmpSetup(serverId);
      onNotice(status === "stopped" ? tr("Set up. It works from the next start.") : tr("Set up. Restart the server to use it."));
      await load();
    } catch (cause) {
      onError(String(cause));
    }
    setBusy(false);
  }

  async function change(key: string, value: string) {
    try {
      const now = await commands.msmpSetSetting(serverId, key, value);
      setSettings((list) => list.map((one) => (one.key === key ? { ...one, value: now } : one)));
    } catch (cause) {
      onError(String(cause));
      void load();
    }
  }

  async function changeRule(key: string, value: string) {
    setRules((list) => list.map((one) => (one.key === key ? { ...one, value } : one)));
    try {
      await commands.msmpSetRule(serverId, key, value);
    } catch (cause) {
      onError(String(cause));
      void load();
    }
  }

  if (!state) return embedded ? <p className="muted msmp-note">{tr("Loading…")}</p> : null;
  if (!state.supported) {
    return embedded ? (
      <div className="msmp-bar">
        <Icon name="info" />
        <strong>{tr("Live settings")}</strong>
        <span className="muted">{tr("This server's version has no management connection, so settings change on the next start.")}</span>
      </div>
    ) : null;
  }
  // Ready but stopped: nothing to change live, so nothing to show.
  if (state.ready && !state.running) {
    return embedded ? (
      <div className="msmp-bar">
        <Icon name="sliders" />
        <strong>{tr("Live settings")}</strong>
        <span className="muted">{tr("Set up. Start the server to change settings and game rules while it runs.")}</span>
      </div>
    ) : null;
  }
  if (!state.ready) {
    return (
      <div className="msmp-bar">
        <Icon name="sliders" />
        <strong>{tr("Live settings")}</strong>
        <span className="muted" data-tip={tr("Change settings and game rules while the server runs, without a restart. This turns on the server's management connection, only on this computer.")}>
          {tr("Change settings while the server runs.")}
        </span>
        <button type="button" className="button small secondary" disabled={busy} onClick={() => void setup()}>
          {tr("Set up")}
        </button>
      </div>
    );
  }
  if (!state.connected) {
    return (
      <div className="msmp-bar">
        <Icon name="alert" />
        <strong>{tr("Live settings")}</strong>
        <span className="muted" data-tip={tr("If you just set it up, restart the server once.")}>
          {state.error || tr("Couldn't reach the server's management connection.")}
        </span>
        <button type="button" className="icon-button" aria-label={tr("Try again")} data-tip={tr("Try again")} onClick={() => void load()}>
          <Icon name="refresh" />
        </button>
      </div>
    );
  }

  return (
    <div className={`msmp-card ${open ? "open" : ""}`}>
      <div className="msmp-bar">
        <Icon name="sliders" />
        <strong>{tr("Live settings")}</strong>
        <span className="muted">{tr("{0} settings · {1} game rules · applied right away", [settings.length, rules.length])}</span>
        {open && (
          <button type="button" className="icon-button" aria-label={tr("Refresh")} data-tip={tr("Refresh")} onClick={() => void load()}>
            <Icon name="refresh" />
          </button>
        )}
        {!embedded && (
          <button type="button" className="button small secondary" aria-expanded={open} onClick={() => toggle(!open)}>
            {open ? tr("Hide") : tr("Show")}
          </button>
        )}
      </div>
      {open && (
        <div className="msmp-body">
      <div className="msmp-grid">
        {settings.map((one) => {
          const value = parse(one.value);
          const label = settingLabel(one.key);
          return (
            <label key={one.key} className="msmp-row">
              <span>
                {label}
                <code>{one.key}</code>
              </span>
              {one.kind === "boolean" ? (
                <Switch aria-label={label} checked={value === true} onChange={(event) => void change(one.key, String(event.target.checked))} />
              ) : CHOICES[one.kind] ? (
                <select aria-label={label} value={String(value)} onChange={(event) => void change(one.key, JSON.stringify(event.target.value))}>
                  {CHOICES[one.kind].map((choice) => (
                    <option key={choice} value={choice}>
                      {choice}
                    </option>
                  ))}
                </select>
              ) : (
                <Typed value={String(value)} numeric={one.kind === "integer"} label={label} onSend={(next) => void change(one.key, next)} />
              )}
            </label>
          );
        })}
      </div>
      <div className="msmp-rules-head">
        <span className="setting-title">{tr("Game rules")}</span>
        <SearchField value={query} onChange={setQuery} placeholder={tr("Search game rules (e.g. mob, keepInventory)")} />
      </div>
      <div className="msmp-grid">
        {shownRules.map((one) => {
          const label = ruleName(one.key);
          return (
            <label key={one.key} className="msmp-row">
              <span>
                {label}
                {label !== one.key && <code>{one.key}</code>}
              </span>
              {one.kind === "boolean" ? (
                <Switch aria-label={label} checked={one.value === "true"} onChange={(event) => void changeRule(one.key, String(event.target.checked))} />
              ) : (
                <Typed value={one.value} numeric label={label} onSend={(next) => void changeRule(one.key, next)} />
              )}
            </label>
          );
        })}
        {!shownRules.length && <p className="muted">{tr("No matching game rules.")}</p>}
      </div>
        </div>
      )}
    </div>
  );
}

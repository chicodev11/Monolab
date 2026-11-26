import { useEffect, useState } from "react";
import type { TestRun } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { linesOf, textOf, type StudioData } from "./StudioPreview";

/** The placeholder a test event can fill for each rule event, with the rule field it starts from. */
const FAKE_VALUE: Record<string, { name: string; from: string }> = {
  chat: { name: "message", from: "contains" },
  break: { name: "block", from: "blocks" },
  place: { name: "block", from: "blocks" },
  world_change: { name: "world", from: "world" },
  command: { name: "command", from: "command" },
  death: { name: "killer", from: "" },
};

/** Events with no player: timers, schedules and other plugins' events. */
export function playerless(event: string): boolean {
  return event === "timer" || event === "schedule" || event.includes(":");
}

/** "name=value" lines as placeholders; lines without "=" are skipped. */
export function varsOf(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const at = line.indexOf("=");
    const name = at > 0 ? line.slice(0, at).trim() : "";
    if (name) out[name] = line.slice(at + 1).trim();
  }
  return out;
}

function fakeLabel(name: string): string {
  switch (name) {
    case "message":
      return tr("Chat message");
    case "block":
      return tr("Block");
    case "world":
      return tr("World");
    case "command":
      return tr("Command");
    default:
      return tr("Killer");
  }
}

/**
 * In-game test tool: makes this rule's event happen on the server without anyone doing it, and shows what the rule
 * did. The server runs the saved file. Nothing is sent until the button is pressed.
 */
export function StudioRuleTest({ serverId, rule, data, dirty }: { serverId: string; rule: string; data: StudioData; dirty: boolean }) {
  const when = data.when && typeof data.when === "object" && !Array.isArray(data.when) ? (data.when as StudioData) : {};
  const event = textOf(when, "event") || "join";
  const fake = FAKE_VALUE[event];
  const [open, setOpen] = useState(false);
  const [players, setPlayers] = useState<string[]>([]);
  const [player, setPlayer] = useState("");
  const [value, setValue] = useState("");
  const [vars, setVars] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState("");
  const [runs, setRuns] = useState<TestRun[] | null>(null);

  useEffect(() => {
    if (!open) return;
    let live = true;
    commands.readLive(serverId).then(
      (snapshot) => {
        if (!live) return;
        const names = (snapshot && Date.now() - snapshot.at < 60_000 ? snapshot.players : []).map((one) => one.name);
        setPlayers(names);
        setPlayer((now) => now || (playerless(event) ? "" : names[0] || ""));
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [serverId, open, event]);

  // A fresh start value from the rule's own settings when the event changes.
  useEffect(() => {
    const from = FAKE_VALUE[event]?.from;
    setValue(from === "blocks" ? (linesOf(when, "blocks")[0] ?? "stone") : from ? textOf(when, from) : "");
    setRuns(null);
    setProblem("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [event, rule]);

  async function fire() {
    setBusy(true);
    setProblem("");
    setRuns(null);
    const placeholders = event.includes(":") ? varsOf(vars) : fake && value.trim() ? { [fake.name]: value.trim() } : {};
    try {
      setRuns(await commands.studioTestEvent(serverId, event, rule, player.trim(), placeholders));
    } catch (cause) {
      setProblem(String(cause));
    } finally {
      setBusy(false);
    }
  }

  const needsPlayer = !playerless(event);
  const options = [...(needsPlayer ? [] : [{ value: "", label: tr("No player (the server)") }]), ...players.map((one) => ({ value: one, label: one }))];

  return (
    <div className="studio-test">
      <button type="button" className="studio-test-head" aria-expanded={open} onClick={() => setOpen((now) => !now)}>
        <Icon name="play" />
        <span>{tr("Test this rule")}</span>
        <small>{tr("Make its event happen on the server, without anyone doing it")}</small>
        <Icon name="chevron" />
      </button>
      {open && (
        <div className="studio-test-body">
          <div className="studio-held-bar">
            <label className="field studio-field">
              <span>{tr("Player")}</span>
              {options.length > 0 ? (
                <Dropdown value={player} onChange={setPlayer} options={options} />
              ) : (
                <input value={player} onChange={(change) => setPlayer(change.target.value)} placeholder={needsPlayer ? tr("No one is online, or the Monolab add-on isn't running.") : tr("No player (the server)")} />
              )}
            </label>
            {fake && (
              <label className="field studio-field">
                <span>{fakeLabel(fake.name)}</span>
                <input value={value} onChange={(change) => setValue(change.target.value)} placeholder={fake.name === "killer" ? tr("Nobody") : ""} />
              </label>
            )}
            <button type="button" className="button small" disabled={busy || dirty || (needsPlayer && !player.trim())} onClick={() => void fire()}>
              <Icon name="play" />
              {busy ? tr("Waiting for the server…") : tr("Fire test event")}
            </button>
          </div>
          {event.includes(":") && (
            <label className="field studio-field">
              <span>{tr("Placeholders")}</span>
              <textarea className="studio-test-vars" spellCheck={false} value={vars} onChange={(change) => setVars(change.target.value)} placeholder={"item=diamond\nprice=20"} />
              <small>{tr("One name=value per line, the way the plugin would send them.")}</small>
            </label>
          )}
          {dirty && <p className="muted studio-test-note">{tr("Save first: the server runs the saved file.")}</p>}
          {problem && <p className="studio-held-problem">{problem}</p>}
          {runs && runs.length === 0 && <p className="muted studio-test-note">{tr("No rule ran. The add-on may not have read this file yet, or it has a problem.")}</p>}
          {runs?.map((one) => (
            <div key={one.rule} className="studio-test-run">
              <Icon name={!one.matched ? "minus" : one.passed ? "check" : "close"} />
              <span>
                <strong>{one.rule}</strong>
                {" · "}
                {!one.matched ? tr("its When settings didn't take this event") : one.passed ? tr("passed its conditions") : tr("went to Otherwise")}
                {one.matched && (one.ran.length ? ` (${one.ran.join(", ")})` : ` (${tr("nothing to run")})`)}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

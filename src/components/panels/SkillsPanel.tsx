import { useCallback, useEffect, useState } from "react";
import type { AgentSkill } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { EmptyState, Switch } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";

type Draft = { old: string; name: string; description: string; body: string };
const blank: Draft = { old: "", name: "", description: "", body: "" };

/** Lowercase letters, numbers and hyphens, as Claude Code wants skill names. */
export function skillName(text: string) {
  return text
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/-{2,}/g, "-")
    .replace(/^-+/, "")
    .slice(0, 64);
}

export function SkillsPanel({
  server,
  onError,
  confirm,
  onAskAgent,
}: {
  server: string;
  onError: (message: string) => void;
  confirm: (title: string, body: string, label: string, danger?: boolean) => Promise<boolean>;
  onAskAgent: (prompt: string) => void;
}) {
  const [skills, setSkills] = useState<AgentSkill[] | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const [others, setOthers] = useState<{ value: string; label: string }[]>([]);
  const [said, setSaid] = useState("");

  const load = useCallback(() => {
    void commands.agentSkills(server).then(setSkills, (cause) => onError(String(cause)));
  }, [server, onError]);
  useEffect(load, [load]);
  useEffect(() => {
    void commands.listInstances().then(
      (list) => setOthers(list.filter((one) => one.id !== server).map((one) => ({ value: one.id, label: one.name }))),
      () => setOthers([]),
    );
  }, [server]);

  const shown = draft?.old ? skills?.find((one) => one.name === draft.old) : undefined;
  const changed = !!draft && (!shown || shown.name !== draft.name || shown.description !== draft.description || shown.body.trimEnd() !== draft.body.trimEnd());
  const pick = (one: AgentSkill) => {
    setSaid("");
    setDraft({ old: one.name, name: one.name, description: one.description, body: one.body });
  };

  const run = async (work: () => Promise<unknown>, after?: () => void) => {
    setBusy(true);
    try {
      await work();
      after?.();
      load();
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy(false);
    }
  };
  const save = () => {
    if (!draft) return;
    const next = draft.name.replace(/-+$/, "");
    void run(() => commands.saveAgentSkill(server, draft.old, next, draft.description, draft.body), () => {
      setDraft({ ...draft, old: next, name: next });
      setSaid(tr("Saved"));
    });
  };
  const remove = async () => {
    if (!shown) return;
    if (!(await confirm(tr("Delete skill?"), tr("The agent stops using {0}.", [shown.name]), tr("Delete"), true))) return;
    void run(() => commands.deleteAgentSkill(server, shown.name), () => setDraft(null));
  };

  return (
    <div className="skills-page">
      <nav className="skills-list" aria-label={tr("Skills")}>
        <button className="chat-session-new" onClick={() => { setSaid(""); setDraft(blank); }}>
          <Icon name="plus" />
          {tr("New skill")}
        </button>
        <button
          className="button ghost small"
          data-tip={tr("The agent writes down what you did in the current chat as a skill.")}
          onClick={() => onAskAgent(tr("Turn what we did in this chat into a reusable skill: write .claude/skills/<short-name>/SKILL.md with name and description front matter and the steps in the body."))}
        >
          <Icon name="sparkles" />
          {tr("Make from chat")}
        </button>
        <div className="skills-scroll">
          {skills?.length === 0 && <p className="chat-session-empty">{tr("No skills yet.")}</p>}
          {skills?.map((one) => (
            <button key={one.name} className={`skills-item ${draft?.old === one.name ? "on" : ""}`} aria-current={draft?.old === one.name || undefined} onClick={() => pick(one)}>
              <span>{one.name}</span>
              {!one.enabled && <small>{tr("Off")}</small>}
            </button>
          ))}
        </div>
      </nav>
      <section className="skills-edit">
        {!draft ? (
          <EmptyState
            icon="library"
            title={tr("Skills")}
            hint={tr("Steps the agent follows when a task fits, like how to build a plugin or update the server safely.")}
          />
        ) : (
          <form
            className="skills-form"
            onSubmit={(event) => {
              event.preventDefault();
              save();
            }}
          >
            <div className="skills-row">
              <label className="field">
                <span>{tr("Name")}</span>
                <input value={draft.name} placeholder="build-plugin" onChange={(event) => setDraft({ ...draft, name: skillName(event.target.value) })} />
              </label>
              {shown && (
                <div className="skills-toggle">
                  <span>{tr("Use")}</span>
                  <Switch
                    checked={shown.enabled}
                    disabled={busy}
                    aria-label={tr("Use")}
                    onChange={(event) => void run(() => commands.setAgentSkillEnabled(server, shown.name, event.target.checked))}
                  />
                </div>
              )}
            </div>
            <label className="field">
              <span>{tr("Description")}</span>
              <input value={draft.description} placeholder={tr("When to use it, in one line")} onChange={(event) => setDraft({ ...draft, description: event.target.value })} />
            </label>
            <label className="field skills-body">
              <span>{tr("Content")}</span>
              <textarea value={draft.body} spellCheck={false} onChange={(event) => setDraft({ ...draft, body: event.target.value })} />
            </label>
            <div className="skills-actions">
              <button type="submit" className="button primary small" disabled={busy || !changed || !draft.name || !draft.description.trim()}>
                <Icon name="check" />
                {tr("Save")}
              </button>
              {shown && others.length > 0 && (
                <Dropdown
                  value=""
                  options={others}
                  placeholder={tr("Copy to another server")}
                  onChange={(to) => void run(() => commands.copyAgentSkill(server, shown.name, to), () => setSaid(tr("Copied to {0}.", [others.find((one) => one.value === to)?.label ?? to])))}
                />
              )}
              {shown && (
                <button type="button" className="button ghost small" disabled={busy} onClick={() => void remove()}>
                  <Icon name="trash" />
                  {tr("Delete")}
                </button>
              )}
              {said && !changed && <span className="muted skills-said">{said}</span>}
            </div>
          </form>
        )}
      </section>
    </div>
  );
}

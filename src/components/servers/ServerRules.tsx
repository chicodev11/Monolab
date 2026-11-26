import { useEffect, useState } from "react";
import { monolinkTeam, monolinkWhoAmI, onMonolink, onWeb } from "../../lib/bridge";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Card } from "../primitives";
import { Icon } from "../ui/Icon";

const STARTER = `# Rules for this server

Every agent that works on this server reads this file first.

## Don't touch

## Plugins and versions

## How we build and test
`;

/**
 * The server's shared AGENTS.md: the rules every agent on this server follows — the host's own and
 * teammates' agents. Everyone who can open the server reads it; the host, web panel admins and team
 * Admins or the Owner change it (the host's app checks, and records each change).
 */
export function ServerRules({ serverId, webAdmin, onError, onNotice }: { serverId: string; webAdmin: boolean; onError: (message: string) => void; onNotice: (message: string) => void }) {
  const [text, setText] = useState<string | null>(null);
  const [draft, setDraft] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // This PC's own servers, a web panel admin, or a team Admin or Owner through Monolink.
  const [canEdit, setCanEdit] = useState(!onWeb || (webAdmin && !onMonolink));

  useEffect(() => {
    let gone = false;
    setText(null);
    setDraft(null);
    void commands.readServerAgentsMd(serverId).then(
      (value) => !gone && setText(value),
      (cause) => {
        if (gone) return;
        setText("");
        onError(String(cause));
      },
    );
    return () => {
      gone = true;
    };
  }, [serverId, onError]);

  useEffect(() => {
    if (!onMonolink || !monolinkTeam) return;
    void monolinkWhoAmI().then(
      (me) => {
        const role = (me as { teamRole?: string }).teamRole ?? "";
        setCanEdit(role === "admin" || role === "owner");
      },
      () => setCanEdit(false),
    );
  }, []);

  async function save() {
    if (draft === null) return;
    setBusy(true);
    try {
      await commands.saveServerAgentsMd(serverId, draft);
      setText(draft);
      setDraft(null);
      onNotice(tr("Saved AGENTS.md. Agents read it when they start a new chat."));
    } catch (cause) {
      onError(String(cause));
    }
    setBusy(false);
  }

  const has = (text ?? "").trim() !== "";
  return (
    <Card
      title={tr("Team rules (AGENTS.md)")}
      className="server-rules"
      aside={
        canEdit && draft === null && text !== null ? (
          <button type="button" className="button secondary small" onClick={() => setDraft(has ? text : STARTER)}>
            <Icon name={has ? "pencil" : "plus"} />
            {has ? tr("Edit") : tr("Create")}
          </button>
        ) : undefined
      }
    >
      <p className="muted server-rules-hint">{tr("Every agent on this server reads this at the start of a chat: yours, and your teammates' own agents.")}</p>
      {draft !== null ? (
        <>
          <textarea
            className="server-rules-edit"
            value={draft}
            spellCheck={false}
            aria-label={tr("Team rules (AGENTS.md)")}
            onChange={(event) => setDraft(event.target.value)}
          />
          <div className="modal-actions">
            <button type="button" className="button secondary small" disabled={busy} onClick={() => setDraft(null)}>
              {tr("Cancel")}
            </button>
            <button type="button" className="button primary small" disabled={busy} onClick={() => void save()}>
              {tr("Save")}
            </button>
          </div>
        </>
      ) : text === null ? (
        <p className="muted">{tr("Loading…")}</p>
      ) : has ? (
        <pre className="server-rules-text">{text}</pre>
      ) : (
        <p className="muted">{canEdit ? tr("No rules yet. Create AGENTS.md to tell every agent what not to touch and how this server is built.") : tr("No rules yet.")}</p>
      )}
    </Card>
  );
}

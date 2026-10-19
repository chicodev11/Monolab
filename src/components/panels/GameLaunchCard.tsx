import { useEffect, useState } from "react";
import { launchFile } from "../../lib/games";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Icon } from "../ui/Icon";

export function GameLaunchCard({
  id,
  onError,
  onNotice,
}: {
  id: string;
  onError: (message: string) => void;
  onNotice: (message: string) => void;
}) {
  const [saved, setSaved] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [notes, setNotes] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setSaved(null);
    commands
      .readEditableFile(id, launchFile)
      .then((text) => {
        const lines = text.split(/\r?\n/);
        const comment = (line: string) => line.trimStart().startsWith("#");
        const args = lines.filter((line) => !comment(line)).join("\n").trim();
        setNotes(lines.filter(comment).map((line) => `${line}\n`).join(""));
        setSaved(args);
        setDraft(args);
      })
      .catch(() => {
        setNotes("");
        setSaved("");
        setDraft("");
      });
  }, [id]);

  async function save() {
    setBusy(true);
    try {
      await commands.saveEditableFile(id, launchFile, `${notes}${draft.trim()}\n`);
      setSaved(draft.trim());
      onNotice(tr("Saved the launch options. They apply on the next start."));
    } catch (cause) {
      onError(String(cause));
    } finally {
      setBusy(false);
    }
  }

  if (saved === null) {
    return <div className="setting-card"><div className="skeleton-row" /></div>;
  }
  return (
    <div className="setting-card command-card game-launch">
      <span className="setting-title">{tr("Launch options")}</span>
      <p className="memory-note">
        <Icon name="info" />
        {tr("Arguments passed to the game server when it starts. Ports, passwords and world names usually go here.")}
      </p>
      <textarea
        value={draft}
        spellCheck={false}
        rows={3}
        aria-label={tr("Launch options")}
        onChange={(event) => setDraft(event.target.value)}
      />
      <span className="title-actions">
        <button
          type="button"
          className="button secondary small"
          disabled={busy || draft.trim() === saved}
          onClick={() => void save()}
        >
          <Icon name="check" />
          {tr("Save")}
        </button>
      </span>
    </div>
  );
}

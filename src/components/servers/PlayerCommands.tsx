import { useState } from "react";
import { tr } from "../../lib/i18n";
import { Dialog } from "../primitives";

/** A named set of console commands to run for one player, from the player card's menus. */
export type PlayerCommand = { label: string; lines: string[] };

const KEY = "monolab-player-commands";

/** Saved on this computer only (they're a convenience, like console history). */
export function readPlayerCommands(): PlayerCommand[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((one): one is PlayerCommand => !!one && typeof one === "object" && typeof (one as PlayerCommand).label === "string" && Array.isArray((one as PlayerCommand).lines))
      .map((one) => ({ label: one.label, lines: one.lines.filter((line) => typeof line === "string") }));
  } catch {
    return [];
  }
}

export function savePlayerCommands(list: PlayerCommand[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    // Private mode or full storage: they last until the app closes.
  }
}

/** `{player}` and `{uuid}` filled in, the leading slash dropped. Empty lines are skipped. */
export function playerCommandLines(command: PlayerCommand, player: { name: string; uuid: string }): string[] {
  return command.lines
    .map((line) => line.trim().replace(/^\//, "").replaceAll("{player}", player.name).replaceAll("{uuid}", player.uuid))
    .filter(Boolean);
}

/** One set per line: `Label | first command | second command`. */
export function parsePlayerCommands(text: string): PlayerCommand[] {
  return text
    .split("\n")
    .map((row) => row.split("|").map((part) => part.trim()))
    .filter(([label, ...lines]) => label && lines.some(Boolean))
    .map(([label, ...lines]) => ({ label, lines: lines.filter(Boolean) }));
}

export function formatPlayerCommands(list: PlayerCommand[]): string {
  return list.map((one) => [one.label, ...one.lines].join(" | ")).join("\n");
}

export function PlayerCommandsDialog({ open, list, onClose, onSave }: { open: boolean; list: PlayerCommand[]; onClose: () => void; onSave: (list: PlayerCommand[]) => void }) {
  const [text, setText] = useState(() => formatPlayerCommands(list));
  const parsed = parsePlayerCommands(text);
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={tr("My player commands")}
      actions={
        <>
          <button type="button" className="button ghost" onClick={onClose}>
            {tr("Cancel")}
          </button>
          <button
            type="button"
            className="button primary"
            onClick={() => {
              onSave(parsed);
              onClose();
            }}
          >
            {tr("Save")}
          </button>
        </>
      }
    >
      <p className="muted">{tr("One set per line: a name, then commands, split by |. {player} and {uuid} become the player's. They show in the player's menu while the server runs.")}</p>
      <textarea
        className="player-commands-text"
        rows={Math.max(4, text.split("\n").length + 1)}
        value={text}
        placeholder={"Starter kit | give {player} stone_sword | give {player} bread 16\nTo spawn | tp {player} 0 64 0"}
        onChange={(event) => setText(event.target.value)}
        spellCheck={false}
      />
      <small className="muted">{tr("{0, plural, one {# set} other {# sets}}", [parsed.length])}</small>
    </Dialog>
  );
}

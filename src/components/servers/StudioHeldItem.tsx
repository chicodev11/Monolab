import { useEffect, useState } from "react";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";

/**
 * Item quick edit: reads the item in an online player's main hand (every component, as the game's item JSON) and
 * puts an edited one back. Nothing goes to the server until a button here is pressed.
 */
export function StudioHeldItem({ serverId, onNotice }: { serverId: string; onNotice: (message: string) => void }) {
  const [players, setPlayers] = useState<string[]>([]);
  const [player, setPlayer] = useState("");
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState("");

  useEffect(() => {
    let live = true;
    commands.readLive(serverId).then(
      (snapshot) => {
        if (!live) return;
        const names = (snapshot && Date.now() - snapshot.at < 60_000 ? snapshot.players : []).map((one) => one.name);
        setPlayers(names);
        setPlayer((now) => now || names[0] || "");
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [serverId]);

  let parsed = true;
  try {
    if (text.trim()) JSON.parse(text);
  } catch {
    parsed = false;
  }

  async function read() {
    setBusy(true);
    setProblem("");
    try {
      setText(await commands.studioHeldItem(serverId, player));
    } catch (cause) {
      setProblem(String(cause));
    } finally {
      setBusy(false);
    }
  }

  async function put() {
    setBusy(true);
    setProblem("");
    try {
      setText(await commands.studioSetHeldItem(serverId, player, text));
      onNotice(tr("The item is in {0}'s hand.", [player]));
    } catch (cause) {
      setProblem(String(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="studio-assets studio-held">
      <div className="studio-assets-head">
        <div>
          <h2>{tr("Held item")}</h2>
          <p className="muted">{tr("Read the item in a player's main hand with all its components, change it here, and put it back.")}</p>
        </div>
      </div>
      <div className="studio-held-bar">
        <label className="field studio-field">
          <span>{tr("Player")}</span>
          {players.length > 0 ? (
            <Dropdown value={player} onChange={setPlayer} options={players.map((one) => ({ value: one, label: one }))} />
          ) : (
            <input value={player} onChange={(event) => setPlayer(event.target.value)} placeholder={tr("No one is online, or the Monolab add-on isn't running.")} />
          )}
        </label>
        <button type="button" className="button secondary small" disabled={busy || !player.trim()} onClick={() => void read()}>
          <Icon name="download" />
          {tr("Read held item")}
        </button>
        <button type="button" className="button small" disabled={busy || !player.trim() || !text.trim() || !parsed} onClick={() => void put()}>
          <Icon name="upload" />
          {tr("Put it back")}
        </button>
      </div>
      {problem && <p className="studio-held-problem">{problem}</p>}
      {!parsed && <p className="studio-held-problem">{tr("That isn't valid JSON yet.")}</p>}
      <textarea
        className="studio-held-text"
        spellCheck={false}
        value={text}
        onChange={(event) => setText(event.target.value)}
        placeholder={tr("Press Read held item to load it here.")}
        aria-label={tr("Item JSON")}
      />
      <p className="muted studio-assets-hint">{tr("Components use the game's own names, like minecraft:custom_name or minecraft:enchantments. Putting it back replaces what they hold.")}</p>
    </div>
  );
}

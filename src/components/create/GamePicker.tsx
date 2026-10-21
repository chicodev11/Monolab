import { tr } from "../../lib/i18n";
import { fileSize, roughSize } from "../../lib/format";
import { customProgram, gameInfo, type GameInfo } from "../../lib/kinds";
import { CoreIcon } from "../ui/CoreIcon";

export function GamePicker({
  value,
  onPick,
  gamesLocked,
}: {
  value: string;
  onPick: (game: string) => void;
  gamesLocked: boolean;
}) {
  return (
    <div className="create-field">
      <div className="game-tiles">
        <button type="button" className={`game-tile ${value === "Minecraft" ? "on" : ""}`} onClick={() => onPick("Minecraft")}>
          <span className="game-mark"><CoreIcon core="Vanilla" /></span>
          <strong>Minecraft</strong>
        </button>
        {gameInfo.map((game) => (
          <button
            type="button"
            key={game.id}
            className={`game-tile ${value === game.id ? "on" : ""}`}
            disabled={gamesLocked}
            data-tip={gamesLocked ? tr("Game servers can only be created in the app") : undefined}
            onClick={() => onPick(game.id)}
          >
            <span className="game-mark"><CoreIcon core={game.id} /></span>
            <strong>{game.id}</strong>
          </button>
        ))}
      </div>
      <button
        type="button"
        className={`game-tile game-custom ${value === customProgram.id ? "on" : ""}`}
        disabled={gamesLocked}
        data-tip={gamesLocked ? tr("Game servers can only be created in the app") : undefined}
        onClick={() => onPick(customProgram.id)}
      >
        <span className="game-mark"><CoreIcon core={customProgram.id} /></span>
        <strong>{tr(customProgram.id)}</strong>
      </button>
    </div>
  );
}

export function GameFacts({ game, freeBytes }: { game: GameInfo; freeBytes: number | null }) {
  const tight = freeBytes !== null && freeBytes < game.sizeBytes * 1.2;
  return (
    <div className="game-facts">
      <div>
        <span>{tr("Server files")}</span>
        <strong>{game.source === "steam" ? tr("Steam (SteamCMD)") : tr("Official download")}</strong>
      </div>
      <div>
        <span>{tr("Default port")}</span>
        <strong>{game.port}</strong>
      </div>
      <div>
        <span>{tr("Install size")}</span>
        <strong>{tr("About {0}", [roughSize(game.sizeBytes)])}</strong>
      </div>
      <div
        className={tight ? "warn" : ""}
        data-tip={tight ? tr("There may not be enough space. Free some up or change the server folder in Settings.") : undefined}
      >
        <span>{tr("Free space")}</span>
        <strong>{freeBytes === null ? "—" : fileSize(freeBytes)}</strong>
      </div>
    </div>
  );
}

import { tr } from "../../lib/i18n";
import { fileSize, roughSize } from "../../lib/format";
import { customProgram, gameInfo, type GameInfo } from "../../lib/kinds";
import { CoreIcon, MinecraftLogo } from "../ui/CoreIcon";
import { Icon } from "../ui/Icon";

export function GamePicker({
  value,
  onPick,
  gamesLocked,
}: {
  value: string;
  onPick: (game: string) => void;
  gamesLocked: boolean;
}) {
  const lockTip = gamesLocked ? tr("Game servers can only be created in the app") : undefined;
  return (
    <div className="game-pick">
      <button
        type="button"
        className={`game-feature ${value === "Minecraft" ? "on" : ""}`}
        onClick={() => onPick("Minecraft")}
      >
        <span className="game-feature-mark"><MinecraftLogo /></span>
        <strong>Minecraft</strong>
        <span className="game-feature-cores">Paper · Fabric · NeoForge · Vanilla</span>
        <Icon name="arrow" />
      </button>
      <span className="game-pick-label">{tr("Other games")}</span>
      <div className="game-grid">
        {gameInfo.map((game) => (
          <button
            type="button"
            key={game.id}
            className={`game-card ${value === game.id ? "on" : ""}`}
            disabled={gamesLocked}
            data-tip={lockTip}
            onClick={() => onPick(game.id)}
          >
            <span className="game-card-mark"><CoreIcon core={game.id} /></span>
            <span className="game-card-text">
              <strong>{game.id}</strong>
              <small>{roughSize(game.sizeBytes)}</small>
            </span>
          </button>
        ))}
        <button
          type="button"
          className={`game-card game-card-custom ${value === customProgram.id ? "on" : ""}`}
          disabled={gamesLocked}
          data-tip={lockTip}
          onClick={() => onPick(customProgram.id)}
        >
          <span className="game-card-mark"><CoreIcon core={customProgram.id} /></span>
          <span className="game-card-text">
            <strong>{tr(customProgram.id)}</strong>
            <small>{tr("Existing folder")}</small>
          </span>
        </button>
      </div>
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

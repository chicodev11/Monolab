import { tr } from "../../lib/i18n";
import { customProgram, games } from "../../lib/kinds";
import { Dropdown } from "../ui/Dropdown";

export function GameFilter({
  value,
  counts,
  total,
  onChange,
}: {
  value: string;
  counts: Map<string, number>;
  total: number;
  onChange: (game: string) => void;
}) {
  if (counts.size < 2) return null;
  return (
    <Dropdown
      className="game-filter"
      value={value}
      onChange={onChange}
      options={[
        { value: "", label: tr("All games"), hint: String(total) },
        ...["Minecraft", ...games, customProgram.id]
          .filter((game) => counts.has(game))
          .map((game) => ({ value: game, label: game === customProgram.id ? tr(game) : game, hint: String(counts.get(game)) })),
      ]}
    />
  );
}

import { tr } from "../../lib/i18n";
import { customProgram, games } from "../../lib/kinds";
import { DropdownMenu } from "../primitives";
import { Icon } from "../ui/Icon";

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
  const label = (game: string) => (game === customProgram.id ? tr(game) : game);
  const pick = (game: string, name: string, count: number) => ({
    label: (
      <span className="game-filter-item">
        {name}
        <b>{count}</b>
      </span>
    ),
    icon: value === game ? "check" : undefined,
    onSelect: () => onChange(game),
  });
  return (
    <DropdownMenu
      trigger={
        <button
          className={`button secondary game-filter-button ${value ? "on" : ""}`}
          type="button"
          aria-label={tr("Filter by game")}
          data-tip={tr("Filter by game")}
        >
          <Icon name="filter" />
          {value && <span>{label(value)}</span>}
        </button>
      }
      items={[
        pick("", tr("All games"), total),
        ...["Minecraft", ...games, customProgram.id]
          .filter((game) => counts.has(game))
          .map((game) => pick(game, label(game), counts.get(game) ?? 0)),
      ]}
    />
  );
}

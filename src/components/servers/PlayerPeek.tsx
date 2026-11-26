import { useHead } from "../../lib/heads";
import { tr } from "../../lib/i18n";

function Face({ name }: { name: string }) {
  const head = useHead(name);
  return head ? <img src={head} alt="" /> : <span className="peek-face">{name.slice(0, 1).toUpperCase()}</span>;
}

export function PlayerPeek({ names = [], online }: { names?: string[]; online: number }) {
  if (online <= 0) return null;
  const rest = Math.max(0, online - names.length);
  return (
    <div className="player-peek" role="tooltip">
      {names.map((name) => (
        <div className="peek-row" key={name}>
          <Face name={name} />
          <span>{name}</span>
        </div>
      ))}
      {rest > 0 && <small>{names.length ? tr("+{0} more", [rest]) : tr("{0} online", [online])}</small>}
    </div>
  );
}

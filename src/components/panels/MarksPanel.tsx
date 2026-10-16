import { tr } from "../../lib/i18n";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";

export type Bookmark = {
  id: string;
  serverId: string;
  name: string;
  dimension: string;
  x: number;
  y: number;
  z: number;
  note: string;
  createdAt: number;
};

export type MarkForm = {
  name: string;
  dimension: string;
  x: string;
  y: string;
  z: string;
  note: string;
};

export function MarksPanel({
  bookmarks,
  markForm,
  setMarkForm,
  onSave,
  onRemove,
}: {
  bookmarks: Bookmark[];
  markForm: MarkForm;
  setMarkForm: (next: MarkForm) => void;
  onSave: () => void;
  onRemove: (mark: Bookmark) => void;
}) {
  return (

<div className="tool-content">
  <div className="tool-top">
    <p>
      {tr("These notes don't affect server files. Nether coordinates are also shown as Overworld coordinates divided by 8.")}</p>
  </div>
  <div className="mark-form">
    <input
      value={markForm.name}
      placeholder={tr("Name (e.g. Nether fortress)")}
      onChange={(event) =>
        setMarkForm({ ...markForm, name: event.target.value })
      }
    />
    <Dropdown
      value={markForm.dimension}
      onChange={(dimension) => setMarkForm({ ...markForm, dimension })}
      options={[
        { value: "overworld", label: tr("Overworld") },
        { value: "nether", label: tr("Nether") },
        { value: "end", label: tr("End") },
      ]}
    />
    {(["x", "y", "z"] as const).map((axis) => (
      <input
        key={axis}
        type="number"
        value={markForm[axis]}
        placeholder={axis.toUpperCase()}
        onChange={(event) =>
          setMarkForm({ ...markForm, [axis]: event.target.value })
        }
      />
    ))}
    <input
      value={markForm.note}
      placeholder={tr("Note (optional)")}
      onChange={(event) =>
        setMarkForm({ ...markForm, note: event.target.value })
      }
    />
    <button
      className="button primary"
      disabled={!markForm.name.trim()}
      onClick={() => onSave()}
    >
      {tr("Save")}</button>
  </div>
  {bookmarks.length ? (
    <div className="mark-list">
      {bookmarks.map((mark) => (
        <div key={mark.id}>
          <div>
            <strong>{mark.name}</strong>
            <span className="chip subtle">
              {mark.dimension === "overworld"
                ? tr("Overworld")
                : mark.dimension === "nether"
                  ? tr("Nether")
                  : tr("End")}
            </span>
            {mark.note && <small>{mark.note}</small>}
          </div>
          <code>
            {mark.x}, {mark.y}, {mark.z}
          </code>
          <small className="mark-convert">
            {mark.dimension === "nether"
              ? tr("Overworld {0}, {1}", [mark.x * 8, mark.z * 8])
              : mark.dimension === "overworld"
                ? tr("Nether {0}, {1}", [Math.round(mark.x / 8), Math.round(mark.z / 8)])
                : ""}
          </small>
          <button
            className="icon-button danger"
            title={tr("Delete")}
            onClick={() => onRemove(mark)}
          >
            <Icon name="trash" />
          </button>
        </div>
      ))}
    </div>
  ) : (
    <p className="muted tool-empty">
      {tr("No saved coordinates yet. Enter a name and coordinates above.")}</p>
  )}
</div>
  );
}

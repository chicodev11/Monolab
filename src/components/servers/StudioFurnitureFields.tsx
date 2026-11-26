import { useState } from "react";
import { tr } from "../../lib/i18n";
import { Dropdown } from "../ui/Dropdown";
import { StepList } from "./StudioActions";
import { PlacedWith } from "./StudioBlockFields";
import { AssetPick, NumberField, SwitchRow, put, useAssets } from "./StudioItemFields";
import { StudioPack3d } from "./StudioPack3d";
import { textOf, type StudioData } from "./StudioPreview";
import { objectOf, type StudioNames } from "./studioKinds";
import "./StudioBlocks.css";

/**
 * Studio maker M3: furniture. An item display showing the picked texture or model, a hitbox players click and hit,
 * and optional barrier or light blocks on its cells. The same field names as `crates/studio-format/src/furniture.rs`.
 */

export const FURNITURE_DISPLAYS = ["fixed", "none", "head", "ground"];
export const FURNITURE_TURNS = [1, 4, 8];
const MOST_CELLS = 27;
const MOST_REACH = 4;

const displayLabel = (one: string) => (({ fixed: tr("Like in an item frame"), none: tr("As the model is"), head: tr("Like a worn hat"), ground: tr("Like a dropped item") }) as Record<string, string>)[one] ?? one;
const turnsLabel = (one: number) => (({ 1: tr("Never turns"), 4: tr("Faces the player, 4 ways"), 8: tr("Faces the player, 8 ways") }) as Record<number, string>)[one] ?? String(one);

type Cell = { x: number; y: number; z: number };

function cellsOf(data: StudioData): Cell[] {
  const list = Array.isArray(data.cells) ? data.cells : [];
  return list.map((one) => objectOf(one)).map((one) => ({ x: Number(one.x) || 0, y: Number(one.y) || 0, z: Number(one.z) || 0 }));
}

/** Cells as rows of "x y z", one block each, from the block it is placed on. */
function CellsField({ data, onChange }: { data: StudioData; onChange: (next: StudioData) => void }) {
  const [draft, setDraft] = useState<string | null>(null);
  const shown = draft ?? cellsOf(data).map((one) => `${one.x} ${one.y} ${one.z}`).join("\n");
  const clamp = (one: number) => Math.max(-MOST_REACH, Math.min(MOST_REACH, Math.round(one) || 0));
  return (
    <label className="field studio-field">
      <span>{tr("Blocks it takes")}</span>
      <textarea
        rows={3}
        className="studio-furniture-cells"
        value={shown}
        placeholder="0 0 0"
        onChange={(event) => {
          setDraft(event.target.value);
          const cells = event.target.value
            .split("\n")
            .map((line) => line.trim().split(/[\s,]+/).map(Number))
            .filter((parts) => parts.length === 3 && parts.every(Number.isFinite))
            .slice(0, MOST_CELLS)
            .map(([x, y, z]) => ({ x: clamp(x), y: clamp(y), z: clamp(z) }));
          onChange(put(data, "cells", cells.length > 1 || (cells[0] && (cells[0].x || cells[0].y || cells[0].z)) ? cells : undefined));
        }}
        onBlur={() => setDraft(null)}
        spellCheck={false}
      />
      <small>{tr("One per row: x y z from the block it stands on, facing south. Empty = just that block. Up to {0} blocks, {1} away.", [String(MOST_CELLS), String(MOST_REACH)])}</small>
    </label>
  );
}

export function FurnitureFields({ data, onChange, names, serverId }: { data: StudioData; onChange: (next: StudioData) => void; names: StudioNames; serverId?: string }) {
  const assets = useAssets(serverId);
  const look = objectOf(data.look);
  const mode = textOf(look, "model") ? "model" : "texture";
  const box = objectOf(data.hitbox);
  const seat = typeof data.seat === "object" && data.seat !== null ? objectOf(data.seat) : null;
  const solid = data.solid === true;
  const turns = typeof data.turns === "number" && FURNITURE_TURNS.includes(data.turns) ? data.turns : 4;
  return (
    <>
      <div className="studio-fields-row">
        <label className="field studio-field">
          <span>{tr("Look")}</span>
          <Dropdown
            value={mode}
            onChange={(value) => onChange(put(data, "look", value === "model" ? { model: textOf(look, "model") } : { texture: textOf(look, "texture") }))}
            options={[
              { value: "texture", label: tr("A picture from the assets") },
              { value: "model", label: tr("A model from the assets") },
            ]}
          />
        </label>
        {mode === "model" ? (
          <AssetPick serverId={serverId} assets={assets} folder="models" value={textOf(look, "model")} label={tr("Model")} hint={tr("A Blockbench model exported as JSON.")} onChange={(model) => onChange(put(data, "look", { model }))} />
        ) : (
          <AssetPick serverId={serverId} assets={assets} folder="textures" value={textOf(look, "texture")} label={tr("Picture")} onChange={(texture) => onChange(put(data, "look", { texture }))} />
        )}
      </div>
      {!serverId && <p className="muted">{tr("Open the studio on a server to pick its assets.")}</p>}
      {mode === "model" && textOf(look, "model") && <StudioPack3d serverId={serverId} shape={{ kind: "model", model: textOf(look, "model") }} />}
      <div className="studio-fields-row three">
        <label className="field studio-field">
          <span>{tr("Drawn")}</span>
          <Dropdown value={textOf(data, "display") || "fixed"} onChange={(value) => onChange(put(data, "display", value === "fixed" ? undefined : value))} options={FURNITURE_DISPLAYS.map((one) => ({ value: one, label: displayLabel(one) }))} />
        </label>
        <NumberField value={data.scale} label={tr("Size")} step={0.05} least={0.05} most={8} placeholder="1" onChange={(next) => onChange(put(data, "scale", next))} />
        <NumberField value={data.y} label={tr("Height above the block")} step={0.05} least={-4} most={8} placeholder="0.5" onChange={(next) => onChange(put(data, "y", next))} />
      </div>
      <label className="field studio-field">
        <span>{tr("Turning")}</span>
        <Dropdown value={String(turns)} onChange={(value) => onChange(put(data, "turns", Number(value) === 4 ? undefined : Number(value)))} options={FURNITURE_TURNS.map((one) => ({ value: String(one), label: turnsLabel(one) }))} />
      </label>
      <CellsField data={data} onChange={onChange} />
      <div className="studio-fields-row">
        <NumberField value={box.width} label={tr("Hitbox width")} step={0.05} least={0.1} most={8} placeholder="1" onChange={(next) => onChange(put(data, "hitbox", put(box, "width", next)))} />
        <NumberField value={box.height} label={tr("Hitbox height")} step={0.05} least={0.1} most={8} placeholder="1" onChange={(next) => onChange(put(data, "hitbox", put(box, "height", next)))} />
      </div>
      <SwitchRow label={tr("Solid (players can't walk through)")} checked={solid} onChange={(on) => onChange(put(data, "solid", on || undefined))} />
      {!solid && <NumberField value={data.light} label={tr("Light")} least={0} most={15} placeholder="0" hint={tr("0 to 15, like a torch's 14.")} onChange={(next) => onChange(put(data, "light", next || undefined))} />}
      <div className={`studio-item-part ${seat ? "on" : ""}`}>
        <SwitchRow label={tr("Can be sat on")} checked={seat !== null} onChange={(on) => onChange(put(data, "seat", on ? {} : undefined))} />
        {seat && (
          <div className="studio-item-part-body">
            <NumberField value={seat.height} label={tr("Seat height")} step={0.05} least={-1} most={4} placeholder="0.5" onChange={(next) => onChange(put(data, "seat", put(seat, "height", next)))} />
          </div>
        )}
      </div>
      <PlacedWith data={data} onChange={onChange} names={names} kind="furniture" />
      <SwitchRow label={tr("Drops its item when broken")} checked={data.drops !== false} onChange={(on) => onChange(put(data, "drops", on ? undefined : false))} />
      <StepList label={tr("When right-clicked")} empty={tr("Right-clicking it does nothing more.")} value={data.useActions} names={names} onChange={(useActions) => onChange({ ...data, useActions })} />
      <StepList label={tr("When broken")} empty={tr("Breaking it does nothing more.")} value={data.breakActions} names={names} onChange={(breakActions) => onChange({ ...data, breakActions })} />
    </>
  );
}

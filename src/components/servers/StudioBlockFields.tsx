import { tr } from "../../lib/i18n";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { StepList } from "./StudioActions";
import { AssetPick, NumberField, SwitchRow, TOOL_KINDS, put, useAssets } from "./StudioItemFields";
import { StudioPack3d } from "./StudioPack3d";
import { linesOf, textOf, type StudioData } from "./StudioPreview";
import { objectOf, type StudioNames } from "./studioKinds";
import "./StudioBlocks.css";

/**
 * Studio maker M3: a custom block. It borrows a vanilla block state the app's pack draws as the picked texture or
 * model. The same field names as `crates/studio-format/src/block.rs` and the add-ons.
 */

export const BLOCK_SHAPES = ["cube", "plant"];
/** Vanilla blocks whose sounds a studio block can make. */
const SOUNDS = ["stone", "deepslate", "oak_planks", "glass", "white_wool", "sand", "gravel", "grass_block", "iron_block", "amethyst_block", "short_grass", "moss_block"];

const toolLabel = (one: string) => (({ "": tr("Any (or by hand)"), pickaxe: tr("Pickaxe"), axe: tr("Axe"), shovel: tr("Shovel"), hoe: tr("Hoe") }) as Record<string, string>)[one] ?? one;
const soundLabel = (one: string) => one.replace(/_/g, " ");

type Props = { data: StudioData; onChange: (next: StudioData) => void; names: StudioNames; serverId?: string };

/** What a crop is planted on: a few common choices (`growth.soil`); anything else shows as its own names. */
const SOILS: { value: string; blocks: string[] }[] = [
  { value: "farmland", blocks: ["farmland"] },
  { value: "dirt", blocks: ["grass_block", "dirt", "coarse_dirt", "podzol", "rooted_dirt", "farmland"] },
  { value: "sand", blocks: ["sand", "red_sand"] },
  { value: "soul_sand", blocks: ["soul_sand"] },
  { value: "any", blocks: [] },
];
const soilLabel = (one: string) => (({ farmland: tr("Farmland"), dirt: tr("Grass and dirt"), sand: tr("Sand"), soul_sand: tr("Soul sand"), any: tr("Any solid block") }) as Record<string, string>)[one] ?? one;
const MOST_STAGES = 15;

/** A plant that grows in stages (`growth`, `crates/studio-format/src/block.rs`). */
function GrowthFields({ data, onChange, names, serverId }: Props) {
  const assets = useAssets(serverId);
  const growth = objectOf(data.growth);
  const on = data.growth !== undefined;
  const model = !!textOf(objectOf(data.look), "model");
  const stages = linesOf(growth, "stages");
  const harvest = objectOf(growth.harvest);
  const soil = Array.isArray(growth.soil) ? linesOf(growth, "soil") : ["farmland"];
  const soilChoice = SOILS.find((one) => one.blocks.length === soil.length && one.blocks.every((name) => soil.includes(name)))?.value ?? "custom";
  const set = (key: string, value: unknown) => onChange(put(data, "growth", put(growth, key, value)));
  const setStages = (next: string[]) => set("stages", next);
  return (
    <div className="studio-steps studio-block-growth">
      <SwitchRow label={tr("Grows in stages, like wheat")} checked={on} onChange={(yes) => onChange(put(data, "growth", yes ? { stages: [""], minutes: 5 } : undefined))} />
      {on && (
        <>
          <div className="studio-steps-head">
            <span>{tr("Later stages")}</span>
            <button type="button" className="button ghost small" disabled={stages.length >= MOST_STAGES} onClick={() => setStages([...stages, ""])}>
              <Icon name="plus" />
              {tr("Add stage")}
            </button>
          </div>
          <p className="studio-placeholders">{tr("The look above is the first stage, just planted; the last one here is fully grown. Each stage takes a plant state of its own.")}</p>
          {stages.map((name, at) => (
            <div key={at} className="studio-block-stage">
              <AssetPick serverId={serverId} assets={assets} folder={model ? "models" : "textures"} value={name} label={tr("Stage {0}", [String(at + 1)])} onChange={(next) => setStages(stages.map((one, index) => (index === at ? next : one)))} />
              <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} disabled={stages.length === 1} onClick={() => setStages(stages.filter((_, index) => index !== at))}>
                <Icon name="trash" />
              </button>
            </div>
          ))}
          <div className="studio-fields-row three">
            <NumberField value={growth.minutes} label={tr("Minutes per stage")} step={0.5} least={0.05} most={1440} placeholder="5" hint={tr("On average; wheat takes about 4.")} onChange={(next) => set("minutes", next)} />
            <NumberField value={growth.light} label={tr("Least light")} least={0} most={15} placeholder="0" hint={tr("Wheat needs 9. Paper servers only: Pumpkin grows it in any light.")} onChange={(next) => set("light", next || undefined)} />
            <label className="field studio-field">
              <span>{tr("Planted on")}</span>
              <Dropdown
                value={soilChoice}
                onChange={(value) => set("soil", SOILS.find((one) => one.value === value)?.blocks ?? soil)}
                options={[...SOILS.map((one) => ({ value: one.value, label: soilLabel(one.value) })), ...(soilChoice === "custom" ? [{ value: "custom", label: soil.join(", ") }] : [])]}
              />
            </label>
          </div>
          {soil.includes("farmland") && <SwitchRow label={tr("Grows only on watered farmland")} checked={growth.water === true} onChange={(yes) => set("water", yes || undefined)} />}
          <SwitchRow label={tr("Bone meal grows it")} checked={growth.bonemeal !== false} onChange={(yes) => set("bonemeal", yes ? undefined : false)} />
          <div className="studio-fields-row">
            <label className="field studio-field">
              <span>{tr("Harvest when fully grown")}</span>
              <Dropdown value={textOf(harvest, "loot")} onChange={(value) => set("harvest", put(harvest, "loot", value || undefined))} options={[{ value: "", label: tr("None") }, ...names.loot.map((one) => ({ value: one, label: one }))]} />
              <small>{tr("Rolled on top of the drops below, only when it breaks fully grown.")}</small>
            </label>
            <NumberField value={harvest.exp} label={tr("Harvest experience")} least={0} most={1000} placeholder="0" onChange={(next) => set("harvest", put(harvest, "exp", next || undefined))} />
          </div>
        </>
      )}
    </div>
  );
}

/** A linked studio item, or the block's own item (name and lore). */
export function PlacedWith({ data, onChange, names, kind }: Omit<Props, "serverId"> & { kind: "block" | "furniture" }) {
  const link = textOf(data, "item").trim();
  const options = [{ value: "", label: tr("Its own item") }, ...names.item.map((one) => ({ value: one, label: tr("Studio item: {0}", [one]) }))];
  if (link && !names.item.includes(link)) options.push({ value: link, label: tr("{0} (no such studio item)", [link]) });
  return (
    <>
      <label className="field studio-field">
        <span>{tr("Placed with")}</span>
        <Dropdown value={link} options={options} search={options.length > 8 ? tr("Search items") : undefined} onChange={(value) => onChange(put(data, "item", value || undefined))} />
        <small>{link ? tr("Right-clicking with that item places it, and breaking it gives that item back.") : kind === "block" ? tr("Give it with the give action as studio:block/ and the file name.") : tr("Give it with the give action as studio:furniture/ and the file name.")}</small>
      </label>
      {!link && (
        <>
          <label className="field studio-field">
            <span>{tr("Item name")}</span>
            <input value={textOf(data, "name")} onChange={(event) => onChange(put(data, "name", event.target.value || undefined))} spellCheck={false} />
          </label>
          <label className="field studio-field">
            <span>{tr("Item description lines")}</span>
            <textarea rows={2} value={linesOf(data, "lore").join("\n")} onChange={(event) => onChange(put(data, "lore", event.target.value ? event.target.value.split("\n") : undefined))} spellCheck={false} />
          </label>
        </>
      )}
    </>
  );
}

/** The block's look: one picture on every side, a side picture with its own top and bottom, or a model. */
function BlockLook({ data, onChange, serverId }: Omit<Props, "names">) {
  const assets = useAssets(serverId);
  const look = objectOf(data.look);
  const plant = textOf(data, "shape") === "plant";
  const mode = textOf(look, "model") ? "model" : textOf(look, "top") || textOf(look, "bottom") ? "sides" : "texture";
  const set = (key: string, value: string) => onChange(put(data, "look", put(look, key, value || undefined)));
  return (
    <div className="studio-block-look">
      <label className="field studio-field">
        <span>{tr("Look")}</span>
        <Dropdown
          value={mode}
          onChange={(value) => {
            const texture = textOf(look, "texture");
            onChange(put(data, "look", value === "model" ? { model: textOf(look, "model") } : value === "sides" ? { texture, top: textOf(look, "top") || texture } : { texture }));
          }}
          options={[
            { value: "texture", label: plant ? tr("A picture, drawn crossed like a flower") : tr("One picture on every side") },
            ...(plant ? [] : [{ value: "sides", label: tr("Sides with their own top and bottom") }]),
            { value: "model", label: tr("A model from the assets") },
          ]}
        />
      </label>
      {!serverId && <p className="muted">{tr("Open the studio on a server to pick its assets.")}</p>}
      {mode === "model" ? (
        <AssetPick serverId={serverId} assets={assets} folder="models" value={textOf(look, "model")} label={tr("Model")} hint={tr("A Blockbench block model exported as JSON.")} onChange={(name) => set("model", name)} />
      ) : (
        <AssetPick serverId={serverId} assets={assets} folder="textures" value={textOf(look, "texture")} label={mode === "sides" ? tr("Side picture") : tr("Picture")} hint={tr("16×16, like the game's block pictures.")} onChange={(name) => set("texture", name)} />
      )}
      {mode === "sides" && (
        <div className="studio-fields-row">
          <AssetPick serverId={serverId} assets={assets} folder="textures" value={textOf(look, "top")} label={tr("Top")} onChange={(name) => set("top", name)} />
          <AssetPick serverId={serverId} assets={assets} folder="textures" value={textOf(look, "bottom")} label={tr("Bottom")} hint={tr("Empty = the top's picture.")} onChange={(name) => set("bottom", name)} />
        </div>
      )}
      <StudioPack3d
        serverId={serverId}
        shape={
          mode === "model"
            ? textOf(look, "model") ? { kind: "model", model: textOf(look, "model") } : null
            : !textOf(look, "texture") ? null
            : plant ? { kind: "plant", texture: textOf(look, "texture") }
            : { kind: "cube", side: textOf(look, "texture"), top: textOf(look, "top") || undefined, bottom: textOf(look, "bottom") || undefined }
        }
      />
    </div>
  );
}

export function BlockFields({ data, onChange, names, serverId }: Props) {
  const drops = objectOf(data.drops);
  const tool = textOf(data, "tool");
  const shape = textOf(data, "shape") || "cube";
  const setDrops = (key: string, value: unknown) => onChange(put(data, "drops", put(drops, key, value)));
  return (
    <>
      <label className="field studio-field">
        <span>{tr("Shape")}</span>
        <Dropdown
          value={shape}
          onChange={(value) => onChange(put(put(data, "shape", value === "cube" ? undefined : value), "growth", value === "plant" ? data.growth : undefined))}
          options={[
            { value: "cube", label: tr("A full block") },
            { value: "plant", label: tr("A plant you walk through") },
          ]}
        />
        <small>{shape === "plant" ? tr("Borrows tripwire states: up to 127 plants, and plain tripwire may show as one.") : tr("Borrows note block states (up to 799), then mushroom blocks. Plain note blocks only play their first note.")}</small>
      </label>
      <BlockLook data={data} onChange={onChange} serverId={serverId} />
      {shape === "plant" && <GrowthFields data={data} onChange={onChange} names={names} serverId={serverId} />}
      <PlacedWith data={data} onChange={onChange} names={names} kind="block" />
      <div className="studio-fields-row three">
        <NumberField value={data.hardness} label={tr("Hardness")} step={0.1} least={0} most={50} placeholder="1" hint={tr("Stone is 1.5, ores 3, obsidian 50. 0 breaks at once.")} onChange={(next) => onChange(put(data, "hardness", next))} />
        <label className="field studio-field">
          <span>{tr("Best tool")}</span>
          <Dropdown value={tool} onChange={(value) => onChange(put(put(data, "tool", value || undefined), "needsTool", value ? data.needsTool : undefined))} options={["", ...TOOL_KINDS].map((one) => ({ value: one, label: toolLabel(one) }))} />
        </label>
        <label className="field studio-field">
          <span>{tr("Sounds like")}</span>
          <Dropdown value={textOf(data, "sounds") || (shape === "plant" ? "short_grass" : "stone")} onChange={(value) => onChange(put(data, "sounds", value))} options={SOUNDS.map((one) => ({ value: one, label: soundLabel(one) }))} />
        </label>
      </div>
      <NumberField value={data.light} label={tr("Light")} least={0} most={15} placeholder="0" hint={tr("0 to 15, like a torch's 14. Pumpkin servers only: on Paper the block gives no light.")} onChange={(next) => onChange(put(data, "light", next || undefined))} />
      {tool && <SwitchRow label={tr("Drops nothing without that tool")} checked={data.needsTool === true} onChange={(on) => onChange(put(data, "needsTool", on || undefined))} />}
      <SwitchRow label={tr("Drops its item when broken")} checked={drops.self !== false} onChange={(on) => setDrops("self", on ? undefined : false)} />
      <div className="studio-fields-row">
        <label className="field studio-field">
          <span>{tr("Also rolls a loot table")}</span>
          <Dropdown value={textOf(drops, "loot")} onChange={(value) => setDrops("loot", value || undefined)} options={[{ value: "", label: tr("None") }, ...names.loot.map((one) => ({ value: one, label: one }))]} />
          <small>{tr("The roll goes to the player who broke it.")}</small>
        </label>
        <NumberField value={drops.exp} label={tr("Experience")} least={0} most={1000} placeholder="0" onChange={(next) => setDrops("exp", next || undefined)} />
      </div>
      <StepList label={tr("When placed")} empty={tr("Placing it does nothing more.")} value={data.placeActions} names={names} onChange={(placeActions) => onChange({ ...data, placeActions })} />
      <StepList label={tr("When right-clicked")} empty={tr("Right-clicking it does nothing.")} value={data.useActions} names={names} onChange={(useActions) => onChange({ ...data, useActions })} />
      <StepList label={tr("When broken")} empty={tr("Breaking it does nothing more.")} value={data.breakActions} names={names} onChange={(breakActions) => onChange({ ...data, breakActions })} />
    </>
  );
}

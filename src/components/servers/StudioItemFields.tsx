import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { commands, type StudioAsset } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { Switch } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { EFFECTS } from "./StudioActions";
import { linesOf, textOf, type StudioData } from "./StudioPreview";
import { objectOf } from "./studioKinds";
import { StudioPack3d } from "./StudioPack3d";
import "./StudioItems.css";

/**
 * Studio maker M1: an item's look (a studio texture or model asset) and its components (food, tool, attack, armor…).
 * The same field names as `crates/studio-format/src/item.rs` and the add-ons.
 */

export const RARITIES = ["common", "uncommon", "rare", "epic"];
export const EQUIP_SLOTS = ["head", "chest", "legs", "feet"];
export const TOOL_KINDS = ["pickaxe", "axe", "shovel", "hoe"];
/** Vanilla armor trims, as `crates/studio-format/src/item.rs` lists them. */
const TRIM_PATTERNS = ["sentry", "dune", "coast", "wild", "ward", "eye", "vex", "tide", "snout", "rib", "spire", "wayfinder", "shaper", "silence", "raiser", "host", "flow", "bolt"];
const TRIM_MATERIALS = ["quartz", "iron", "netherite", "redstone", "copper", "gold", "emerald", "diamond", "lapis", "amethyst", "resin"];
const words = (one: string) => one.replace(/_/g, " ");

/** A tool's own block rules: vanilla blocks by name, ours as `studio:<id>`; the first that matches counts. */
function ToolRules({ tool, set }: { tool: StudioData; set: (key: string, value: unknown) => void }) {
  const rules = Array.isArray(tool.rules) ? (tool.rules as StudioData[]) : [];
  const setRules = (next: StudioData[]) => set("rules", next.length ? next : undefined);
  const change = (at: number, key: string, value: unknown) => setRules(rules.map((was, index) => (index === at ? { ...was, [key]: value } : was)));
  return (
    <div className="studio-steps">
      <div className="studio-steps-head">
        <span>{tr("Block rules")}</span>
        <button type="button" className="button ghost small" disabled={rules.length >= 16} onClick={() => setRules([...rules, { blocks: [], speed: 8, drops: true }])}>
          <Icon name="plus" />
          {tr("Add rule")}
        </button>
      </div>
      {rules.length === 0 && <p className="studio-steps-empty">{tr("No own rules. Blocks of the kinds above dig at the speed above.")}</p>}
      {rules.map((one, at) => (
        <div key={at} className="studio-step studio-tool-rule">
          <div className="studio-fields-row">
            <label className="field studio-field">
              <span>{tr("Blocks")}</span>
              <input
                value={linesOf(one, "blocks").join(", ")}
                placeholder="obsidian, studio:ruby_ore"
                onChange={(event) => change(at, "blocks", event.target.value.split(",").map((name) => name.trim()).filter(Boolean))}
                spellCheck={false}
              />
            </label>
            <NumberField value={one.speed} label={tr("Mining speed")} step={0.5} least={0} most={1000} placeholder="6" onChange={(next) => change(at, "speed", next)} />
            <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={() => setRules(rules.filter((_, index) => index !== at))}>
              <Icon name="trash" />
            </button>
          </div>
          <SwitchRow label={tr("Drops their loot")} checked={one.drops !== false} onChange={(on) => change(at, "drops", on)} />
        </div>
      ))}
      <small className="studio-steps-hint">{tr("Comma-separated. Studio blocks are studio:<id>.")}</small>
    </div>
  );
}

/** Armor extras: the set it belongs to and what a full set gives, and a trim. */
function ArmorExtras({ equip, set }: { equip: StudioData; set: (key: string, value: unknown) => void }) {
  const effects = Array.isArray(equip.setEffects) ? (equip.setEffects as StudioData[]) : [];
  const trim = objectOf(equip.trim);
  const trimOn = equip.trim !== undefined;
  const setEffects = (next: StudioData[]) => set("setEffects", next.length ? next : undefined);
  const pick = (list: string[], value: string) => [...list.map((one) => ({ value: one, label: words(one) })), ...(value && !list.includes(value) ? [{ value, label: value }] : [])];
  return (
    <>
      <label className="field studio-field">
        <span>{tr("Armor set")}</span>
        <input value={textOf(equip, "set")} placeholder={tr("None")} onChange={(event) => set("set", event.target.value || undefined)} spellCheck={false} />
        <small>{tr("Items with the same set name make one set. Wearing every piece gives the effects of all of them.")}</small>
      </label>
      {textOf(equip, "set").trim() && (
        <div className="studio-steps">
          <div className="studio-steps-head">
            <span>{tr("Full set effects")}</span>
            <button type="button" className="button ghost small" onClick={() => setEffects([...effects, { effect: "speed", level: 1 }])}>
              <Icon name="plus" />
              {tr("Add effect")}
            </button>
          </div>
          {effects.length === 0 && <p className="studio-steps-empty">{tr("No effects from this piece.")}</p>}
          {effects.map((one, at) => (
            <div key={at} className="studio-fields-row studio-item-set-effect">
              <label className="field studio-field">
                <span>{tr("Effect")}</span>
                <Dropdown value={textOf(one, "effect") || "speed"} options={pick(EFFECTS, textOf(one, "effect"))} onChange={(value) => setEffects(effects.map((was, index) => (index === at ? { ...was, effect: value } : was)))} />
              </label>
              <NumberField value={one.level} label={tr("Level")} least={1} most={255} placeholder="1" onChange={(next) => setEffects(effects.map((was, index) => (index === at ? { ...was, level: next ?? 1 } : was)))} />
              <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={() => setEffects(effects.filter((_, index) => index !== at))}>
                <Icon name="trash" />
              </button>
            </div>
          ))}
        </div>
      )}
      <SwitchRow label={tr("Armor trim")} checked={trimOn} onChange={(on) => set("trim", on ? { pattern: "sentry", material: "gold" } : undefined)} />
      {trimOn && (
        <div className="studio-fields-row">
          <label className="field studio-field">
            <span>{tr("Pattern")}</span>
            <Dropdown value={textOf(trim, "pattern")} options={pick(TRIM_PATTERNS, textOf(trim, "pattern"))} onChange={(value) => set("trim", put(trim, "pattern", value))} />
          </label>
          <label className="field studio-field">
            <span>{tr("Material")}</span>
            <Dropdown value={textOf(trim, "material")} options={pick(TRIM_MATERIALS, textOf(trim, "material"))} onChange={(value) => set("trim", put(trim, "material", value))} />
          </label>
        </div>
      )}
    </>
  );
}
const MOST_FRAMES = 3;

/** Tools and weapons are held like one when the item doesn't say (same guess as the pack builder). */
function heldMaterial(material: string): boolean {
  const bare = material.trim().replace(/^minecraft:/, "");
  return /_(sword|axe|pickaxe|shovel|hoe|spear)$/.test(bare) || ["stick", "blaze_rod", "breeze_rod", "bone", "fishing_rod", "carrot_on_a_stick", "warped_fungus_on_a_stick", "mace", "trident"].includes(bare);
}

/** The server the studio is open on, so the item preview can draw the picked texture. */
export const StudioServer = createContext<string | undefined>(undefined);

// One asset list per server, shared by the form and the preview, read again when a few seconds old (the library
// may have added pictures since).
const lists = new Map<string, { at: number; list: Promise<StudioAsset[]> }>();
const pictures = new Map<string, Promise<string>>();

export function assetsOf(serverId: string): Promise<StudioAsset[]> {
  const known = lists.get(serverId);
  if (known && Date.now() - known.at < 4000) return known.list;
  const list = commands.studioAssetList(serverId).then(
    (one) => one.assets,
    () => [] as StudioAsset[],
  );
  lists.set(serverId, { at: Date.now(), list });
  return list;
}

export function pictureOf(serverId: string, asset: StudioAsset): Promise<string> {
  const key = `${serverId}/${asset.file}/${asset.modified}`;
  let known = pictures.get(key);
  if (!known) {
    known = commands.studioAssetImage(serverId, asset.file);
    known.catch(() => pictures.delete(key));
    pictures.set(key, known);
  }
  return known;
}

export function useAssets(serverId: string | undefined): StudioAsset[] | null {
  const [assets, setAssets] = useState<StudioAsset[] | null>(null);
  useEffect(() => {
    if (!serverId) return;
    let live = true;
    void assetsOf(serverId).then((list) => {
      if (live) setAssets(list);
    });
    return () => {
      live = false;
    };
  }, [serverId]);
  return assets;
}

/** A studio texture's picture as a data URL, once loaded. */
function usePicture(serverId: string | undefined, assets: StudioAsset[] | null, name: string): string | null {
  const [url, setUrl] = useState<string | null>(null);
  const asset = assets?.find((one) => one.folder === "textures" && one.name === name && one.file.endsWith(".png"));
  useEffect(() => {
    setUrl(null);
    if (!serverId || !asset) return;
    let live = true;
    pictureOf(serverId, asset).then(
      (next) => {
        if (live) setUrl(next);
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [serverId, asset]);
  return url;
}

/** The item's `look` fields. */
export function lookOf(data: StudioData): { texture: string; model: string; handheld?: boolean; pulling: string[] } {
  const look = objectOf(data.look);
  return { texture: textOf(look, "texture").trim(), model: textOf(look, "model").trim(), handheld: typeof look.handheld === "boolean" ? look.handheld : undefined, pulling: linesOf(look, "pulling") };
}

/** The item's studio texture for the preview, or null (no server, no texture, still loading). */
export function useItemPicture(data: StudioData): string | null {
  const serverId = useContext(StudioServer);
  const texture = lookOf(data).texture;
  const assets = useAssets(texture ? serverId : undefined);
  return usePicture(serverId, assets, texture);
}

function Picture({ url }: { url: string | null }) {
  return <span className="studio-item-pic">{url ? <img src={url} alt="" /> : <Icon name="image" />}</span>;
}

/** Picks one asset of a folder; empty = none. */
export function AssetPick({ serverId, assets, folder, value, label, hint, onChange }: { serverId?: string; assets: StudioAsset[] | null; folder: "textures" | "models"; value: string; label: string; hint?: string; onChange: (name: string) => void }) {
  const url = usePicture(serverId, assets, folder === "textures" ? value : "");
  const names = [...new Set((assets ?? []).filter((one) => one.folder === folder && (folder === "textures" ? one.file.endsWith(".png") : one.file.endsWith(".json"))).map((one) => one.name))];
  const options = [{ value: "", label: tr("None") }, ...names.map((name) => ({ value: name, label: name }))];
  if (value && !names.includes(value)) options.push({ value, label: tr("{0} (not in the assets)", [value]) });
  return (
    <label className="field studio-field">
      <span>{label}</span>
      <span className="studio-item-asset">
        {folder === "textures" && <Picture url={url} />}
        <Dropdown value={value} options={options} search={options.length > 8 ? tr("Search assets") : undefined} onChange={onChange} placeholder={assets === null ? tr("Loading…") : tr("Choose")} />
      </span>
      {hint && <small>{hint}</small>}
    </label>
  );
}

/** A whole or decimal number field; empty clears the key. */
export function NumberField({ value, label, hint, step = 1, least = 0, most, placeholder, onChange }: { value: unknown; label: string; hint?: string; step?: number; least?: number; most?: number; placeholder?: string; onChange: (next: number | undefined) => void }) {
  return (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input
        type="number"
        step={step}
        min={least}
        max={most}
        placeholder={placeholder}
        value={typeof value === "number" ? value : ""}
        onChange={(event) => {
          if (event.target.value === "") return onChange(undefined);
          const next = Number(event.target.value);
          if (!Number.isFinite(next)) return;
          const kept = Math.min(most ?? Number.MAX_SAFE_INTEGER, Math.max(least, step >= 1 ? Math.round(next) : next));
          onChange(kept);
        }}
      />
      {hint && <small>{hint}</small>}
    </label>
  );
}

export function SwitchRow({ label, checked, onChange }: { label: string; checked: boolean; onChange: (on: boolean) => void }) {
  return (
    <label className="studio-toggle">
      <span>{label}</span>
      <Switch checked={checked} onChange={(event) => onChange(event.target.checked)} />
    </label>
  );
}

/** Without `key` when `value` is undefined. */
export function put(data: StudioData, key: string, value: unknown): StudioData {
  const next = { ...data };
  if (value === undefined) delete next[key];
  else next[key] = value;
  return next;
}

/** A part (food, tool…) that is on when its object is there. */
function Part({ data, name, label, fresh, onChange, children }: { data: StudioData; name: string; label: string; fresh: StudioData; onChange: (next: StudioData) => void; children: (part: StudioData, set: (key: string, value: unknown) => void) => ReactNode }) {
  const on = typeof data[name] === "object" && data[name] !== null && !Array.isArray(data[name]);
  const part = objectOf(data[name]);
  return (
    <div className={`studio-item-part ${on ? "on" : ""}`}>
      <SwitchRow label={label} checked={on} onChange={(next) => onChange(put(data, name, next ? fresh : undefined))} />
      {on && <div className="studio-item-part-body">{children(part, (key, value) => onChange({ ...data, [name]: put(part, key, value) }))}</div>}
    </div>
  );
}

const rarityLabel = (one: string) => (({ common: tr("Common (white)"), uncommon: tr("Uncommon (yellow)"), rare: tr("Rare (aqua)"), epic: tr("Epic (pink)") }) as Record<string, string>)[one] ?? one;
const slotLabel = (one: string) => (({ head: tr("Head"), chest: tr("Chest"), legs: tr("Legs"), feet: tr("Feet") }) as Record<string, string>)[one] ?? one;
const toolLabel = (one: string) => (({ pickaxe: tr("Stone and ores (pickaxe)"), axe: tr("Wood (axe)"), shovel: tr("Dirt and sand (shovel)"), hoe: tr("Leaves and crops (hoe)") }) as Record<string, string>)[one] ?? one;

/** How the item looks: a studio texture (flat or held) or model, and pictures while it is drawn back. */
export function ItemLookFields({ data, onChange, serverId }: { data: StudioData; onChange: (next: StudioData) => void; serverId?: string }) {
  const assets = useAssets(serverId);
  const look = lookOf(data);
  const mode = look.model ? "model" : look.texture || data.look !== undefined ? "texture" : "base";
  const setLook = (next: StudioData | undefined) => onChange(put(data, "look", next));
  const raw = objectOf(data.look);
  return (
    <div className="studio-item-look">
      <label className="field studio-field">
        <span>{tr("Look")}</span>
        <Dropdown
          value={mode}
          onChange={(value) => setLook(value === "base" ? undefined : value === "model" ? { model: look.model } : { texture: look.texture })}
          options={[
            { value: "base", label: tr("The base item's") },
            { value: "texture", label: tr("A picture from the assets") },
            { value: "model", label: tr("A model from the assets") },
          ]}
        />
        {mode !== "base" && <small>{tr("Players see it once they take the studio resource pack. Before 1.21.4 the base item should be a flat one, like paper.")}</small>}
      </label>
      {!serverId && mode !== "base" && <p className="muted">{tr("Open the studio on a server to pick its assets.")}</p>}
      {mode === "texture" && (
        <>
          <AssetPick serverId={serverId} assets={assets} folder="textures" value={look.texture} label={tr("Picture")} onChange={(texture) => setLook({ ...raw, texture })} />
          <SwitchRow label={tr("Held like a tool")} checked={look.handheld ?? heldMaterial(textOf(data, "material"))} onChange={(handheld) => setLook({ ...raw, handheld })} />
        </>
      )}
      {mode === "model" && <AssetPick serverId={serverId} assets={assets} folder="models" value={look.model} label={tr("Model")} hint={tr("A Blockbench model exported as JSON.")} onChange={(model) => setLook({ ...raw, model })} />}
      {mode === "texture" && look.texture && <StudioPack3d serverId={serverId} shape={{ kind: "sprite", texture: look.texture }} />}
      {mode === "model" && look.model && <StudioPack3d serverId={serverId} shape={{ kind: "model", model: look.model }} />}
      {mode !== "base" && (
        <div className={`studio-item-part ${look.pulling.length ? "on" : ""}`}>
          <SwitchRow label={tr("Changes while used, like a bow")} checked={look.pulling.length > 0} onChange={(on) => setLook(put(raw, "pulling", on ? [""] : undefined))} />
          {look.pulling.length > 0 && (
            <div className="studio-item-part-body">
              {Array.from({ length: Math.min(MOST_FRAMES, look.pulling.length + 1) }, (_, at) => (
                <AssetPick
                  key={at}
                  serverId={serverId}
                  assets={assets}
                  folder="textures"
                  value={look.pulling[at] ?? ""}
                  label={tr("Picture {0}", [String(at + 1)])}
                  hint={at === 0 ? tr("Shown from the start of the pull; the next ones a little later.") : undefined}
                  onChange={(name) => {
                    const frames = [...look.pulling];
                    frames[at] = name;
                    const kept = frames.filter((one, place) => one || place === 0);
                    setLook({ ...raw, pulling: kept });
                  }}
                />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/** The item's components: stack size, rarity, durability, food, tool, attack, armor, cooldown, tooltip. */
export function ItemPartFields({ data, onChange, serverId, songs = [] }: { data: StudioData; onChange: (next: StudioData) => void; serverId?: string; songs?: string[] }) {
  const assets = useAssets(serverId);
  const durable = typeof data.durability === "number" && data.durability > 0;
  return (
    <>
      <div className="studio-fields-row">
        <label className="field studio-field">
          <span>{tr("Rarity")}</span>
          <Dropdown value={textOf(data, "rarity")} onChange={(value) => onChange(put(data, "rarity", value || undefined))} options={[{ value: "", label: tr("The base item's") }, ...RARITIES.map((one) => ({ value: one, label: rarityLabel(one) }))]} />
        </label>
        <NumberField value={durable ? undefined : data.maxStack} label={tr("Stacks up to")} least={1} most={99} placeholder={durable ? "1" : tr("Base item")} onChange={(next) => onChange(put(data, "maxStack", next))} />
      </div>
      <div className="studio-fields-row">
        <NumberField value={data.durability} label={tr("Durability")} least={0} hint={tr("Uses before it breaks. Empty = the base item's.")} onChange={(next) => onChange(put(data, "durability", next || undefined))} />
        <NumberField value={data.cooldown} label={tr("Cooldown (seconds)")} step={0.05} least={0} most={3600} hint={tr("Wait after each use.")} onChange={(next) => onChange(put(data, "cooldown", next || undefined))} />
      </div>
      <SwitchRow label={tr("Hide the tooltip")} checked={data.hideTooltip === true} onChange={(on) => onChange(put(data, "hideTooltip", on || undefined))} />
      <label className="field studio-field">
        <span>{tr("Music disc")}</span>
        <Dropdown
          value={textOf(data, "disc")}
          options={[{ value: "", label: tr("None") }, ...songs.map((one) => ({ value: one, label: one })), ...(textOf(data, "disc") && !songs.includes(textOf(data, "disc")) ? [{ value: textOf(data, "disc"), label: textOf(data, "disc") }] : [])]}
          onChange={(value) => onChange(put(data, "disc", value || undefined))}
        />
        <small>{tr("A studio song it plays in a jukebox. New songs work after the server restarts.")}</small>
      </label>
      <Part data={data} name="food" label={tr("Can be eaten")} fresh={{ nutrition: 4, saturation: 2.4 }} onChange={onChange}>
        {(food, set) => (
          <>
            <div className="studio-fields-row">
              <NumberField value={food.nutrition} label={tr("Hunger it fills")} least={0} most={1000} placeholder="4" onChange={(next) => set("nutrition", next)} />
              <NumberField value={food.saturation} label={tr("Saturation")} step={0.1} least={0} most={1000} placeholder="2.4" onChange={(next) => set("saturation", next)} />
              <NumberField value={food.seconds} label={tr("Seconds to eat")} step={0.05} least={0.05} most={60} placeholder="1.6" onChange={(next) => set("seconds", next)} />
            </div>
            <SwitchRow label={tr("Drink it")} checked={food.animation === "drink"} onChange={(on) => set("animation", on ? "drink" : undefined)} />
            <SwitchRow label={tr("Can eat when full")} checked={food.alwaysEat === true} onChange={(on) => set("alwaysEat", on || undefined)} />
            <label className="field studio-field">
              <span>{tr("Leaves behind")}</span>
              <input value={textOf(food, "leftover")} placeholder="bowl" onChange={(event) => set("leftover", event.target.value || undefined)} spellCheck={false} />
              <small>{tr("An item given back after eating. Empty = nothing.")}</small>
            </label>
          </>
        )}
      </Part>
      <Part data={data} name="tool" label={tr("Mines blocks fast")} fresh={{ mines: ["pickaxe"], speed: 8 }} onChange={onChange}>
        {(tool, set) => {
          const mines = linesOf(tool, "mines");
          return (
            <>
              <div className="studio-item-checks" role="group" aria-label={tr("Mines")}>
                {TOOL_KINDS.map((kind) => {
                  const on = mines.includes(kind);
                  return (
                    <button key={kind} type="button" className={`studio-item-check ${on ? "on" : ""}`} aria-pressed={on} onClick={() => set("mines", on ? mines.filter((one) => one !== kind) : [...mines, kind])}>
                      <span className="studio-item-check-box" aria-hidden>
                        {on && <Icon name="check" />}
                      </span>
                      {toolLabel(kind)}
                    </button>
                  );
                })}
              </div>
              <div className="studio-fields-row">
                <NumberField value={tool.speed} label={tr("Mining speed")} step={0.5} least={0} most={1000} placeholder="8" hint={tr("Diamond is 8, netherite 9.")} onChange={(next) => set("speed", next)} />
                <NumberField value={tool.damagePerBlock} label={tr("Wear per block")} least={0} most={1000} placeholder="1" onChange={(next) => set("damagePerBlock", next)} />
              </div>
              <ToolRules tool={tool} set={set} />
            </>
          );
        }}
      </Part>
      <Part data={data} name="attack" label={tr("Own attack damage and speed")} fresh={{ damage: 7, speed: 1.6 }} onChange={onChange}>
        {(attack, set) => (
          <div className="studio-fields-row">
            <NumberField value={attack.damage} label={tr("Damage")} step={0.5} least={0} most={2048} placeholder="7" hint={tr("A diamond sword does 7.")} onChange={(next) => set("damage", next)} />
            <NumberField value={attack.speed} label={tr("Attacks a second")} step={0.1} least={0} most={1024} placeholder="1.6" hint={tr("Swords 1.6, axes 0.8–1.")} onChange={(next) => set("speed", next)} />
          </div>
        )}
      </Part>
      <Part data={data} name="equip" label={tr("Can be worn")} fresh={{ slot: "head" }} onChange={onChange}>
        {(equip, set) => (
          <>
            <div className="studio-fields-row">
              <label className="field studio-field">
                <span>{tr("Worn on")}</span>
                <Dropdown value={textOf(equip, "slot") || "head"} onChange={(value) => set("slot", value)} options={EQUIP_SLOTS.map((one) => ({ value: one, label: slotLabel(one) }))} />
              </label>
              <NumberField value={equip.armor} label={tr("Armor")} step={0.5} least={0} most={30} placeholder="0" onChange={(next) => set("armor", next || undefined)} />
              <NumberField value={equip.toughness} label={tr("Toughness")} step={0.5} least={0} most={20} placeholder="0" onChange={(next) => set("toughness", next || undefined)} />
            </div>
            {textOf(equip, "slot") === "legs" ? (
              <AssetPick serverId={serverId} assets={assets} folder="textures" value={textOf(equip, "legsTexture")} label={tr("Armor picture")} hint={tr("The 64×32 leggings layer, like the game's iron leggings picture. 1.21.2 and later.")} onChange={(name) => set("legsTexture", name || undefined)} />
            ) : (
              <AssetPick serverId={serverId} assets={assets} folder="textures" value={textOf(equip, "texture")} label={tr("Armor picture")} hint={tr("The 64×32 armor layer, like the game's iron armor picture. 1.21.2 and later.")} onChange={(name) => set("texture", name || undefined)} />
            )}
            <StudioPack3d
              serverId={serverId}
              shape={(() => {
                const slot = textOf(equip, "slot") || "head";
                const texture = textOf(equip, slot === "legs" ? "legsTexture" : "texture");
                return texture ? { kind: "armor", slot, texture } : null;
              })()}
            />
            <ArmorExtras equip={equip} set={set} />
          </>
        )}
      </Part>
    </>
  );
}

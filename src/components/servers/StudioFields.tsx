import { useContext, useRef, useState } from "react";
import { commands, type LivePlayer } from "../../lib/bindings";
import { tr, trBack } from "../../lib/i18n";
import { onWeb } from "../../lib/bridge";
import { DropdownMenu, List as RowList, ListRow, Switch } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { ExpansionEventNote, expansionEventOptions, listOf, StepList, StudioExpansions } from "./StudioActions";
import { MobFields } from "./StudioMobFields";
import { AssetPick, ItemLookFields, ItemPartFields, lookOf, useAssets } from "./StudioItemFields";
import { StudioDialogEditor } from "./StudioDialogEditor";
import { useStudioGlyphs } from "./studioGlyphs";
import { itemsIn, linesOf, textOf, type StudioData } from "./StudioPreview";
import { FontFields } from "./StudioFontFields";
import { AnnouncementSchedule } from "./StudioAnnouncementFields";
import { ModelFields } from "./StudioModelFields";
import { BlockFields } from "./StudioBlockFields";
import { FurnitureFields } from "./StudioFurnitureFields";
import { defaultGenerator, WorldgenFields } from "./StudioWorldgenFields";
import { ItemField, ItemPickerSource, ItemSlot } from "./StudioItemPicker";
import { recipeGrid, withRecipeGrid } from "./studioRecipe";
import { eventLabel, NO_NAMES, objectOf, RECIPE_TYPES, recipeLabel, RULE_EVENTS, type StudioNames } from "./studioKinds";

const PLACEHOLDERS = ["<player>", "<online>", "<max>", "<tps>", "<world>", "<ping>"];

function numberOf(data: StudioData, key: string, fallback: number): number {
  const value = data[key];
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function showOf(data: StudioData): StudioData {
  const show = data.show;
  return show && typeof show === "object" && !Array.isArray(show) ? (show as StudioData) : {};
}

/** Rounds a position to two decimals, as people would type it. */
function near(value: number): number {
  return Math.round(value * 100) / 100;
}

/**
 * "Use a player's spot": the players online now (from the add-on's live snapshot, a couple of seconds old), and
 * picking one fills World, X, Y and Z with where they stand. Reading only; nothing is sent to the server.
 */
function PlayerSpot({ serverId, data, onChange }: { serverId?: string; data: StudioData; onChange: (next: StudioData) => void }) {
  const [players, setPlayers] = useState<LivePlayer[] | null>(null);
  const [problem, setProblem] = useState(false);
  if (!serverId) return null;
  const look = () => {
    setPlayers(null);
    setProblem(false);
    commands.readLive(serverId).then(
      // A snapshot older than a minute is from a server that stopped without clearing it.
      (live) => setPlayers((live && Date.now() - live.at < 60_000 ? live.players : []).filter((one) => typeof one.x === "number" && typeof one.y === "number" && typeof one.z === "number")),
      () => setProblem(true),
    );
  };
  const items = problem
    ? [{ label: tr("Couldn't read who is online."), disabled: true, onSelect: () => undefined }]
    : players === null
      ? [{ label: tr("Looking…"), disabled: true, onSelect: () => undefined }]
      : players.length === 0
        ? [{ label: tr("No one is online, or the Monolab add-on isn't running."), disabled: true, onSelect: () => undefined }]
        : players.map((one) => ({
            label: one.name,
            // Only players with a position are kept above.
            icon: "user",
            hint: `${one.world ? `${one.world} · ` : ""}${Math.round(one.x ?? 0)} ${Math.round(one.y ?? 0)} ${Math.round(one.z ?? 0)}`,
            onSelect: () => onChange({ ...data, ...(one.world ? { world: one.world } : {}), x: near(one.x ?? 0), y: near(one.y ?? 0), z: near(one.z ?? 0) }),
          }));
  return (
    <div className="studio-spot">
      <DropdownMenu
        align="start"
        trigger={
          <button type="button" className="button ghost small" onPointerDown={look} onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") look(); }}>
            <Icon name="compass" />
            {tr("Use a player's spot")}
          </button>
        }
        items={items}
      />
    </div>
  );
}

/** Kind-specific fields over the resource's JSON. Unknown keys are kept as they are. */
/** `art` is the client version for the game's pictures (null = the user hasn't said yes): item slots show icons with it. */
export function StudioFields({ art = null, ...props }: { kind: string; id?: string; data: StudioData; onChange: (next: StudioData) => void; names?: StudioNames; serverId?: string; art?: string | null }) {
  return (
    <ItemPickerSource version={art} studio={(props.names ?? NO_NAMES).item} items={itemsIn(props.kind, props.data)}>
      <KindFields {...props} />
    </ItemPickerSource>
  );
}

function KindFields({ kind, id, data, onChange, names = NO_NAMES, serverId }: { kind: string; id?: string; data: StudioData; onChange: (next: StudioData) => void; names?: StudioNames; serverId?: string }) {
  const set = (key: string, value: unknown) => onChange({ ...data, [key]: value });
  const setShow = (key: string, value: unknown) => onChange({ ...data, show: { ...showOf(data), [key]: value } });

  const text = (key: string, label: string, hint?: string) => (
    <label className="field studio-field">
      <span>{label}</span>
      <input value={textOf(data, key)} onChange={(event) => set(key, event.target.value)} spellCheck={false} />
      {hint && <small>{hint}</small>}
    </label>
  );
  const lines = (key: string, label: string, hint?: string, most?: number) => (
    <label className="field studio-field">
      <span>{label}</span>
      <textarea
        rows={Math.min(Math.max(linesOf(data, key).length, 2), 10)}
        value={linesOf(data, key).join("\n")}
        onChange={(event) => {
          const next = event.target.value.split("\n");
          set(key, most ? next.slice(0, most) : next);
        }}
        spellCheck={false}
      />
      <small>{hint ?? tr("One line per row.")}</small>
    </label>
  );
  const number = (key: string, label: string, fallback: number, least: number, hint?: string) => (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input
        type="number"
        min={least}
        value={numberOf(data, key, fallback)}
        onChange={(event) => {
          const value = Number(event.target.value);
          if (Number.isFinite(value)) set(key, Math.max(least, Math.round(value)));
        }}
      />
      {hint && <small>{hint}</small>}
    </label>
  );
  const decimal = (key: string, label: string, fallback: number) => (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input
        type="number"
        step="any"
        value={numberOf(data, key, fallback)}
        onChange={(event) => {
          const value = Number(event.target.value);
          if (event.target.value.trim() && Number.isFinite(value)) set(key, value);
        }}
      />
    </label>
  );
  const toggle = (key: string, label: string, fallback: boolean) => (
    <label className="studio-toggle">
      <span>{label}</span>
      <Switch checked={typeof data[key] === "boolean" ? (data[key] as boolean) : fallback} onChange={(event) => set(key, event.target.checked)} />
    </label>
  );
  const who = (
    <div className="studio-fields-row">
      <label className="field studio-field">
        <span>{tr("Worlds")}</span>
        <input
          value={linesOf(showOf(data), "worlds").join(", ")}
          placeholder={tr("Every world")}
          onChange={(event) =>
            setShow(
              "worlds",
              event.target.value
                .split(",")
                .map((one) => one.trim())
                .filter(Boolean),
            )
          }
          spellCheck={false}
        />
      </label>
      <label className="field studio-field">
        <span>{tr("Permission")}</span>
        <input value={textOf(showOf(data), "permission")} placeholder={tr("Everyone")} onChange={(event) => setShow("permission", event.target.value.trim())} spellCheck={false} />
      </label>
    </div>
  );
  const priority = number("priority", tr("Priority"), 0, -1000, tr("When several match a player, the highest one shows."));

  return (
    <div className="studio-fields">
      {toggle("enabled", tr("Enabled"), true)}
      {kind === "scoreboard" && (
        <>
          {text("title", tr("Title"))}
          {lines("lines", tr("Lines"), tr("One line per row, up to 15."), 15)}
          {number("refreshTicks", tr("Refresh every (ticks)"), 20, 10, tr("20 ticks = 1 second."))}
          {who}
          {priority}
        </>
      )}
      {kind === "tab" && (
        <>
          {lines("header", tr("Header"))}
          {lines("footer", tr("Footer"))}
          {text("playerName", tr("Player name in the list"), tr("Leave empty to keep the normal name."))}
          {number("refreshTicks", tr("Refresh every (ticks)"), 40, 10, tr("20 ticks = 1 second."))}
          {who}
          {priority}
        </>
      )}
      {kind === "motd" && (
        <>
          {lines("lines", tr("Lines"), tr("Two lines show in the server list."), 2)}
          {priority}
        </>
      )}
      {kind === "chat" && (
        <>
          {text("format", tr("Format"), tr("Use <message> where the chat message goes."))}
          {who}
          {priority}
        </>
      )}
      {kind === "message" && (
        <>
          {text("join", tr("Join"), tr("Leave empty to show no message."))}
          {text("firstJoin", tr("First join"), tr("For players who have never played here."))}
          {text("leave", tr("Leave"))}
          {who}
          {priority}
        </>
      )}
      {kind === "announcement" && (
        <>
          {lines("messages", tr("Messages"), tr("One message per row, sent in turn."))}
          <AnnouncementSchedule data={data} onChange={onChange} />
          {toggle("random", tr("Random order"), false)}
          {who}
        </>
      )}
      {kind === "bossbar" && (
        <>
          {text("title", tr("Title"))}
          <div className="studio-fields-row">
            <label className="field studio-field">
              <span>{tr("Color")}</span>
              <Dropdown value={choiceOf(data, "color", BOSSBAR_COLORS, "purple")} onChange={(value) => set("color", value)} options={BOSSBAR_COLORS.map((one) => ({ value: one, label: one }))} />
            </label>
            <label className="field studio-field">
              <span>{tr("Style")}</span>
              <Dropdown value={choiceOf(data, "style", BOSSBAR_STYLES, "progress")} onChange={(value) => set("style", value)} options={BOSSBAR_STYLES.map((one) => ({ value: one, label: one }))} />
            </label>
          </div>
          <label className="field studio-field studio-number">
            <span>{tr("Filled (%)")}</span>
            <input
              type="number"
              min={0}
              max={100}
              value={Math.round(Math.min(Math.max(numberOf(data, "progress", 1), 0), 1) * 100)}
              onChange={(event) => {
                const value = Number(event.target.value);
                if (Number.isFinite(value)) set("progress", Math.min(Math.max(value, 0), 100) / 100);
              }}
            />
            <small>{tr("Saved as progress from 0 to 1.")}</small>
          </label>
          {number("refreshTicks", tr("Refresh every (ticks)"), 20, 10, tr("20 ticks = 1 second."))}
          {who}
          {priority}
        </>
      )}
      {kind === "hologram" && (
        <>
          {lines("lines", tr("Lines"))}
          {text("world", tr("World"))}
          <div className="studio-fields-row three">
            {decimal("x", "X", 0.5)}
            {decimal("y", "Y", 80)}
            {decimal("z", "Z", 0.5)}
          </div>
          <PlayerSpot serverId={serverId} data={data} onChange={onChange} />
          <label className="field studio-field">
            <span>{tr("Turns")}</span>
            <Dropdown value={choiceOf(data, "billboard", HOLOGRAM_BILLBOARDS, "center")} onChange={(value) => set("billboard", value)} options={HOLOGRAM_BILLBOARDS.map((one) => ({ value: one, label: one }))} />
            <small>{tr("center faces the player; fixed doesn't turn.")}</small>
          </label>
          {decimal("scale", tr("Size"), 1)}
          {toggle("shadow", tr("Text shadow"), false)}
          {toggle("background", tr("Background"), true)}
          {toggle("seeThrough", tr("Seen through blocks"), false)}
          <p className="studio-placeholders">{tr("Everyone sees the same text, so only <online>, <max> and <tps> work here. It shows while its chunk is loaded.")}</p>
        </>
      )}
      {kind === "command" && <CommandFields data={data} onChange={onChange} names={names} />}
      {kind === "menu" && <MenuFields data={data} onChange={onChange} names={names} serverId={serverId} />}
      {kind === "item" && <ItemFields data={data} onChange={onChange} names={names} serverId={serverId} />}
      {kind === "recipe" && <RecipeFields data={data} onChange={onChange} names={names} />}
      {kind === "rule" && <RuleFields data={data} onChange={onChange} names={names} />}
      {kind === "npc" && <NpcFields data={data} onChange={onChange} names={names} serverId={serverId} />}
      {kind === "dialog" && <StudioDialogEditor data={data} onChange={onChange} names={names} serverId={serverId} />}
      {kind === "font" && <FontFields data={data} onChange={onChange} serverId={serverId} />}
      {kind === "model" && <ModelFields data={data} onChange={onChange} serverId={serverId} names={names} />}
      {kind === "block" && <BlockFields data={data} onChange={onChange} names={names} serverId={serverId} />}
      {kind === "furniture" && <FurnitureFields data={data} onChange={onChange} names={names} serverId={serverId} />}
      {kind === "mob" && <MobFields data={data} onChange={onChange} names={names} serverId={serverId} />}
      {kind === "enchant" && <EnchantFields data={data} onChange={onChange} names={names} />}
      {kind === "song" && <SongFields data={data} onChange={onChange} names={names} />}
      {kind === "painting" && <PaintingFields data={data} onChange={onChange} serverId={serverId} />}
      {kind === "biome" && <BiomeFields data={data} onChange={onChange} names={names} />}
      {kind === "cosmetic" && <CosmeticFields data={data} onChange={onChange} names={names} />}
      {kind === "trade" && <TradeFields data={data} onChange={onChange} names={names} />}
      {kind === "shop" && <ShopFields data={data} onChange={onChange} names={names} />}
      {kind === "function" && <FunctionFields data={data} onChange={onChange} names={names} />}
      {kind === "quest" && <QuestFields data={data} onChange={onChange} names={names} serverId={serverId} />}
      {kind === "structure" && <StructureFields data={data} onChange={onChange} id={id} serverId={serverId} />}
      {kind === "loot" && <LootFields data={data} onChange={onChange} names={names} />}
      {kind === "advancement" && <AdvancementFields data={data} onChange={onChange} names={names} />}
      {kind === "world" && <WorldFields data={data} onChange={onChange} names={names} />}
      {!NO_PLACEHOLDERS.includes(kind) && <p className="studio-placeholders">
        {tr("Placeholders")}: {[...PLACEHOLDERS, ...(EXTRA_PLACEHOLDERS[kind] ?? []), "<var:name>", "<pvar:name>"].join(" ")}
        {" · "}
        {tr("Text uses MiniMessage, like <red>, <b> and <gradient:#fff:#888>.")}
      </p>}
    </div>
  );
}

/** Boss bar colors and styles as Minecraft names them (the shared format crate has the same lists). */
export const BOSSBAR_COLORS = ["pink", "blue", "red", "green", "yellow", "purple", "white"];
export const BOSSBAR_STYLES = ["progress", "notched_6", "notched_10", "notched_12", "notched_20"];
export const HOLOGRAM_BILLBOARDS = ["center", "fixed", "vertical", "horizontal"];

function choiceOf(data: StudioData, key: string, allowed: string[], fallback: string): string {
  const value = textOf(data, key).trim().toLowerCase();
  return allowed.includes(value) ? value : fallback;
}

const EXTRA_PLACEHOLDERS: Record<string, string[]> = {
  command: ["<arg1>", "<args>", "<sender>"],
  rule: ["<message>", "<block>", "<killer>", "<command>"],
  npc: ["<npc>"],
  item: ["<item>", "<target>", "<block>"],
  dialog: ["<input:key>", "<dialog>"],
  advancement: ["<advancement>"],
  enchant: ["<enchant>", "<level>", "<target>", "<block>"],
};
/** Kinds with no text a player sees, so no placeholder line. */
const NO_PLACEHOLDERS = ["recipe", "loot", "world", "font", "song", "painting", "biome", "cosmetic", "trade"];
/** The shared format crate has the same lists. */
export const LOOT_SOURCES = ["none", "block", "mob", "chest", "fishing"];
export const ADVANCEMENT_FRAMES = ["task", "goal", "challenge"];
export const DIALOG_INPUTS = ["text", "bool", "number", "choice"];
export const WORLD_TYPES = ["normal", "flat", "amplified", "large_biomes", "void", "custom"];
export const WORLD_ENVIRONMENTS = ["normal", "nether", "end"];

type KindProps = { data: StudioData; onChange: (next: StudioData) => void; names: StudioNames };
type FieldProps = { data: StudioData; name: string; label: string; hint?: string; placeholder?: string; onChange: (next: StudioData) => void };

function Text({ data, name, label, hint, placeholder, onChange }: FieldProps) {
  return (
    <label className="field studio-field">
      <span>{label}</span>
      <input value={textOf(data, name)} placeholder={placeholder} onChange={(event) => onChange({ ...data, [name]: event.target.value })} spellCheck={false} />
      {hint && <small>{hint}</small>}
    </label>
  );
}

function Lines({ data, name, label, hint, onChange }: FieldProps) {
  return (
    <label className="field studio-field">
      <span>{label}</span>
      <textarea rows={Math.min(Math.max(linesOf(data, name).length, 2), 8)} value={linesOf(data, name).join("\n")} onChange={(event) => onChange({ ...data, [name]: event.target.value.split("\n") })} spellCheck={false} />
      <small>{hint ?? tr("One line per row.")}</small>
    </label>
  );
}

/** A comma-separated list kept as a JSON array. */
function List({ data, name, label, hint, placeholder, onChange }: FieldProps) {
  return (
    <label className="field studio-field">
      <span>{label}</span>
      <input
        value={linesOf(data, name).join(", ")}
        placeholder={placeholder}
        onChange={(event) =>
          onChange({
            ...data,
            [name]: event.target.value
              .split(",")
              .map((one) => one.trim())
              .filter(Boolean),
          })
        }
        spellCheck={false}
      />
      {hint && <small>{hint}</small>}
    </label>
  );
}

function Whole({ data, name, label, fallback, least, most, hint, onChange }: FieldProps & { fallback: number; least: number; most?: number }) {
  const value = typeof data[name] === "number" ? (data[name] as number) : fallback;
  return (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input
        type="number"
        min={least}
        max={most}
        value={value}
        onChange={(event) => {
          const next = Number(event.target.value);
          if (Number.isFinite(next)) onChange({ ...data, [name]: Math.min(most ?? Number.MAX_SAFE_INTEGER, Math.max(least, Math.round(next))) });
        }}
      />
      {hint && <small>{hint}</small>}
    </label>
  );
}

function Toggle({ data, name, label, onChange }: Omit<FieldProps, "hint" | "placeholder">) {
  return (
    <label className="studio-toggle">
      <span>{label}</span>
      <Switch checked={data[name] === true} onChange={(event) => onChange({ ...data, [name]: event.target.checked })} />
    </label>
  );
}

function CommandFields({ data, onChange, names }: KindProps) {
  return (
    <>
      <div className="studio-fields-row">
        <Text data={data} name="name" label={tr("Command")} placeholder="spawn" hint={tr("Players type it with a slash: /{0}", [textOf(data, "name") || "spawn"])} onChange={onChange} />
        <List data={data} name="aliases" label={tr("Other names")} placeholder="hub, lobby" onChange={onChange} />
      </div>
      <Text data={data} name="description" label={tr("Description")} onChange={onChange} />
      <div className="studio-fields-row">
        <Text data={data} name="permission" label={tr("Permission")} placeholder={tr("Everyone")} onChange={onChange} />
        <Whole data={data} name="cooldownSeconds" label={tr("Cooldown (seconds)")} fallback={0} least={0} onChange={onChange} />
      </div>
      <Toggle data={data} name="console" label={tr("The console can use it too")} onChange={onChange} />
      <StepList label={tr("What it does")} empty={tr("Nothing yet. Add an action.")} value={data.actions} names={names} onChange={(actions) => onChange({ ...data, actions })} />
    </>
  );
}

function MenuFields({ data, onChange, names, serverId }: KindProps & { serverId?: string }) {
  const rows = typeof data.rows === "number" ? Math.min(6, Math.max(1, data.rows)) : 3;
  const { glyphs } = useStudioGlyphs(serverId);
  const background = textOf(data, "background");
  const backgrounds = [
    { value: "", label: tr("None (the game's chest)") },
    ...glyphs.map((one) => ({ value: one.name, label: `:${one.name}:  ${one.width}×${one.height}` })),
    ...(background && !glyphs.some((one) => one.name === background) ? [{ value: background, label: `:${background}:` }] : []),
  ];
  const slots = listOf(data.slots);
  const [picked, setPicked] = useState<number | null>(null);
  const indexAt = (place: number) => slots.findIndex((one) => one.slot === place);
  const itemOf = (slot: StudioData) => (typeof slot.item === "string" ? (slot.item as string) : textOf(objectOf(slot.item), "material"));
  const setSlot = (at: number, next: StudioData) => onChange({ ...data, slots: slots.map((one, index) => (index === at ? next : one)) });
  /** Puts an item in a place: changes the slot there, or adds one. An empty pick removes it. */
  const putItem = (place: number, item: string) => {
    const at = indexAt(place);
    if (!item) {
      if (at >= 0) onChange({ ...data, slots: slots.filter((_, index) => index !== at) });
      return;
    }
    const value = item.startsWith("studio:") ? item : { ...(at >= 0 && typeof slots[at].item !== "string" ? objectOf(slots[at].item) : { name: "" }), material: item };
    if (at >= 0) setSlot(at, { ...slots[at], item: value });
    else onChange({ ...data, slots: [...slots, { slot: place, item: value, actions: [] }] });
    setPicked(place);
  };
  const shown = picked !== null && picked < rows * 9 ? indexAt(picked) : -1;
  const slot = shown >= 0 ? slots[shown] : null;
  return (
    <>
      <Text data={data} name="title" label={tr("Title")} onChange={onChange} />
      <div className="studio-fields-row">
        <Whole data={data} name="rows" label={tr("Rows")} fallback={3} least={1} most={6} onChange={onChange} />
        <Text data={data} name="command" label={tr("Opens with a command")} placeholder={tr("No command")} hint={tr("Leave empty and open it from another command or rule.")} onChange={onChange} />
      </div>
      <div className="studio-fields-row">
        <Text data={data} name="permission" label={tr("Permission")} placeholder={tr("Everyone")} onChange={onChange} />
        <ItemField label={tr("Empty slots")} value={textOf(data, "fill")} placeholder="gray_stained_glass_pane" onChange={(fill) => onChange({ ...data, fill })} />
      </div>
      <label className="field studio-field">
        <span>{tr("Background")}</span>
        <Dropdown
          value={background}
          options={backgrounds}
          search={tr("Find a glyph")}
          onChange={(next) => {
            const rest = { ...data };
            delete rest.background;
            onChange(next ? { ...rest, background: next } : rest);
          }}
        />
        <small>{tr("A glyph image drawn over the whole menu screen from its top-left corner (players need the studio pack). Its ascent in the glyph file sets how high it sits.")}</small>
      </label>
      <div className="studio-steps">
        <div className="studio-steps-head">
          <span>{tr("Slots")}</span>
        </div>
        <div className="studio-bench studio-chest" role="group" aria-label={tr("Slots")}>
          <div className="studio-chest-grid">
            {Array.from({ length: rows * 9 }, (_, place) => {
              const at = indexAt(place);
              const item = at >= 0 ? itemOf(slots[at]) : "";
              return (
                <div key={place} className={`studio-chest-cell ${picked === place ? "picked" : ""}`} onClickCapture={() => setPicked(place)}>
                  <ItemSlot value={item} onChange={(next) => putItem(place, next)} label={tr("Slot {0}", [place])} />
                </div>
              );
            })}
          </div>
        </div>
        <p className="studio-bench-hint">{tr("Click a slot to put an item in, right click to take it out. The last slot you clicked shows its settings below.")}</p>
        {slot && shown >= 0 && (
          <MenuSlotFields
            slot={slot}
            rows={rows}
            names={names}
            onChange={(next) => {
              setSlot(shown, next);
              if (typeof next.slot === "number") setPicked(next.slot);
            }}
            onRemove={() => {
              onChange({ ...data, slots: slots.filter((_, index) => index !== shown) });
            }}
          />
        )}
      </div>
    </>
  );
}

function MenuSlotFields({ slot, rows, names, onChange, onRemove }: { slot: StudioData; rows: number; names: StudioNames; onChange: (next: StudioData) => void; onRemove: () => void }) {
  const item = slot.item;
  const studioItem = typeof item === "string";
  const fields = objectOf(item);
  return (
    <div className="studio-step">
      <div className="studio-step-head">
        <span className="studio-step-number">{typeof slot.slot === "number" ? slot.slot : "?"}</span>
        <strong>{studioItem ? (item as string) : textOf(fields, "name").replace(/<[^>]+>/g, "") || textOf(fields, "material") || tr("Slot")}</strong>
        <span className="studio-step-tools">
          <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={onRemove}>
            <Icon name="trash" />
          </button>
        </span>
      </div>
      <div className="studio-step-fields">
        <label className="field studio-field studio-number">
          <span>{tr("Slot")}</span>
          <input
            type="number"
            min={0}
            max={rows * 9 - 1}
            value={typeof slot.slot === "number" ? slot.slot : 0}
            onChange={(event) => onChange({ ...slot, slot: Math.max(0, Math.min(rows * 9 - 1, Math.round(Number(event.target.value) || 0))) })}
          />
        </label>
        {!studioItem && (
          <>
            <label className="field studio-field">
              <span>{tr("Name")}</span>
              <input value={textOf(fields, "name")} onChange={(event) => onChange({ ...slot, item: { ...fields, name: event.target.value } })} spellCheck={false} />
            </label>
            <label className="field studio-field wide">
              <span>{tr("Description lines")}</span>
              <input
                value={linesOf(fields, "lore").join(" | ")}
                placeholder={tr("Split lines with |")}
                onChange={(event) => onChange({ ...slot, item: { ...fields, lore: event.target.value.split("|").map((one) => one.trim()) } })}
                spellCheck={false}
              />
            </label>
          </>
        )}
        <label className="studio-toggle">
          <span>{tr("Close the menu after")}</span>
          <Switch checked={slot.close === true} onChange={(event) => onChange({ ...slot, close: event.target.checked })} />
        </label>
      </div>
      <StepList label={tr("When clicked")} empty={tr("Clicking does nothing.")} value={slot.actions} names={names} onChange={(actions) => onChange({ ...slot, actions })} />
    </div>
  );
}

function ItemFields({ data, onChange, names, serverId }: KindProps & { serverId?: string }) {
  const enchantments = objectOf(data.enchantments);
  const look = lookOf(data);
  return (
    <>
      <div className="studio-fields-row">
        <ItemField label={tr("Base item")} value={textOf(data, "material")} placeholder="diamond_sword" empty={false} onChange={(material) => onChange({ ...data, material })} />
        {!look.texture && !look.model && (
          <Whole
            data={data}
            name="model"
            label={tr("Model number")}
            fallback={0}
            least={0}
            hint={tr("For your own resource pack. 0 = none.")}
            onChange={(next) => onChange(next.model === 0 ? Object.fromEntries(Object.entries(next).filter(([key]) => key !== "model")) : next)}
          />
        )}
      </div>
      <ItemLookFields data={data} onChange={onChange} serverId={serverId} />
      <Text data={data} name="name" label={tr("Name")} onChange={onChange} />
      <Lines data={data} name="lore" label={tr("Description lines")} onChange={onChange} />
      <label className="field studio-field">
        <span>{tr("Enchantments")}</span>
        <textarea
          rows={Math.max(2, Object.keys(enchantments).length)}
          value={Object.entries(enchantments)
            .map(([name, level]) => `${name} ${String(level)}`)
            .join("\n")}
          placeholder="sharpness 5"
          onChange={(event) =>
            onChange({
              ...data,
              enchantments: Object.fromEntries(
                event.target.value
                  .split("\n")
                  .map((line) => line.trim().split(/\s+/))
                  .filter((parts) => parts[0])
                  .map((parts) => [parts[0].toLowerCase(), Math.max(1, Math.round(Number(parts[1]) || 1))]),
              ),
            })
          }
          spellCheck={false}
        />
        <small>{tr("One per row: name and level.")}</small>
      </label>
      <Toggle data={data} name="unbreakable" label={tr("Unbreakable")} onChange={onChange} />
      <Toggle data={data} name="glow" label={tr("Glows")} onChange={onChange} />
      <ItemPartFields data={data} onChange={onChange} serverId={serverId} songs={names.song} />
      <StepList label={tr("When used (right click)")} empty={tr("Using it does what the base item does.")} value={data.useActions} names={names} onChange={(useActions) => onChange({ ...data, useActions })} />
      <StepList label={tr("When eaten")} empty={tr("Eating it does only what food does.")} value={data.eatActions} names={names} onChange={(eatActions) => onChange({ ...data, eatActions })} />
      <StepList label={tr("When it hits something")} empty={tr("Hitting with it only does damage.")} value={data.hitActions} names={names} onChange={(hitActions) => onChange({ ...data, hitActions })} />
      <StepList label={tr("When it breaks a block")} empty={tr("Breaking a block with it does nothing more.")} value={data.breakActions} names={names} onChange={(breakActions) => onChange({ ...data, breakActions })} />
    </>
  );
}

function RecipeFields({ data, onChange }: KindProps) {
  const type = textOf(data, "type") || "shaped";
  const result = objectOf(data.result);
  const amount = typeof result.amount === "number" ? result.amount : 1;
  const cooking = type === "furnace" || type === "blasting" || type === "smoking" || type === "campfire";
  const crafting = type === "shaped" || type === "shapeless";
  const cells = recipeGrid(data);
  const setCell = (at: number, item: string) => onChange(withRecipeGrid(data, cells.map((one, index) => (index === at ? item : one))));
  const setResult = (next: StudioData) => onChange({ ...data, result: { ...result, ...next } });
  const changeType = (next: string) => {
    // Switching between the two table kinds keeps what is on the table.
    const nextCrafting = next === "shaped" || next === "shapeless";
    onChange(crafting && nextCrafting && next !== type ? withRecipeGrid({ ...data, type: next }, cells) : { ...data, type: next });
  };
  return (
    <>
      <label className="field studio-field">
        <span>{tr("Made in")}</span>
        <Dropdown value={type} onChange={changeType} options={RECIPE_TYPES.map((one) => ({ value: one, label: recipeLabel(one) }))} />
      </label>
      <div className="studio-bench" role="group" aria-label={recipeLabel(type)}>
        {crafting ? (
          <div className="studio-bench-grid">
            {cells.map((item, at) => (
              <ItemSlot key={at} value={item} onChange={(next) => setCell(at, next)} label={tr("Slot {0}", [at + 1])} />
            ))}
          </div>
        ) : (
          <div className="studio-bench-input">
            <ItemSlot value={textOf(data, "input")} onChange={(input) => onChange({ ...data, input })} label={tr("Input")} />
            {cooking && (
              <span className="studio-bench-fire" aria-hidden>
                <Icon name="sparkles" />
              </span>
            )}
          </div>
        )}
        <span className="studio-bench-arrow" aria-hidden>
          <Icon name="arrow" />
        </span>
        <div className="studio-bench-result">
          <ItemSlot value={textOf(result, "item")} amount={amount} big empty={false} onChange={(item) => setResult({ item })} label={tr("Makes")} />
          <label className="studio-bench-amount">
            <span>×</span>
            <input
              type="number"
              min={1}
              max={64}
              aria-label={tr("Amount")}
              value={amount}
              onChange={(event) => setResult({ amount: Math.max(1, Math.min(64, Math.round(Number(event.target.value) || 1))) })}
            />
          </label>
        </div>
      </div>
      <p className="studio-bench-hint">
        {type === "shaped"
          ? tr("Click a slot to put an item in, right click to take it out. Players can make it anywhere on the table, as long as the shape matches.")
          : type === "shapeless"
            ? tr("Click a slot to put an item in, right click to take it out. Where they go doesn't matter.")
            : tr("Click a slot to pick the item.")}
      </p>
      {cooking && (
        <div className="studio-fields-row">
          <Whole data={data} name="cookTicks" label={tr("Cooking time (ticks)")} fallback={200} least={1} onChange={onChange} />
          <Whole data={data} name="experience" label={tr("Experience")} fallback={0} least={0} onChange={onChange} />
        </div>
      )}
    </>
  );
}

function RuleFields({ data, onChange, names }: KindProps) {
  return (
    <>
      <RuleWhen data={data} onChange={onChange} />
      <StepList conditions label={tr("If")} empty={tr("No conditions: it always runs.")} value={data.if} names={names} onChange={(next) => onChange({ ...data, if: next })} />
      <StepList label={tr("Then")} empty={tr("Nothing yet. Add an action.")} value={data.then} names={names} onChange={(next) => onChange({ ...data, then: next })} />
      <StepList label={tr("Otherwise")} empty={tr("Nothing happens when a condition fails.")} value={data.else} names={names} onChange={(next) => onChange({ ...data, else: next })} />
    </>
  );
}

/** A rule's trigger: the event, its filters and the cooldown. Also the node editor's trigger panel. */
export function RuleWhen({ data, onChange }: { data: StudioData; onChange: (next: StudioData) => void }) {
  const when = objectOf(data.when);
  const event = textOf(when, "event") || "join";
  const setWhen = (next: StudioData) => onChange({ ...data, when: next });
  const expansions = useContext(StudioExpansions);
  const eventOptions: { value: string; label: string; hint?: string }[] = [...RULE_EVENTS.map((one) => ({ value: one, label: eventLabel(one) })), ...expansionEventOptions(expansions)];
  if (!eventOptions.some((one) => one.value === event)) eventOptions.push({ value: event, label: event });
  return (
    <>
      <label className="field studio-field">
        <span>{tr("When")}</span>
        <Dropdown value={event} onChange={(value) => setWhen({ ...when, event: value })} options={eventOptions} />
      </label>
      {event.includes(":") && <ExpansionEventNote event={event} />}
      {(event === "break" || event === "place") && <List data={when} name="blocks" label={tr("Blocks")} placeholder={tr("Any block")} onChange={setWhen} />}
      {event === "chat" && <Text data={when} name="contains" label={tr("Message contains")} placeholder={tr("Any message")} onChange={setWhen} />}
      {event === "world_change" && <Text data={when} name="world" label={tr("To world")} placeholder={tr("Any world")} onChange={setWhen} />}
      {event === "command" && <Text data={when} name="command" label={tr("Command starts with")} placeholder="/spawn" onChange={setWhen} />}
      {event === "click" && <Pick data={when} name="button" label={tr("Button")} options={["right", "left", "any"]} fallback="right" onChange={setWhen} />}
      {event === "click" && <List data={when} name="blocks" label={tr("Blocks")} placeholder={tr("Any block, or air")} hint={tr("Write air for clicks on nothing.")} onChange={setWhen} />}
      {(event === "click_entity" || event === "kill" || event === "hit") && <List data={when} name="entities" label={tr("Mobs")} placeholder={tr("Any mob")} hint={tr("Game mobs like zombie, or a studio mob's id.")} onChange={setWhen} />}
      {["eat", "craft", "fish", "drop", "pickup"].includes(event) && <List data={when} name="items" label={tr("Items")} placeholder={tr("Any item")} hint={tr("Game items like bread, or studio:<id>.")} onChange={setWhen} />}
      {event === "gamemode" && <Pick data={when} name="mode" label={tr("To")} options={["any", "survival", "creative", "adventure", "spectator"]} fallback="any" onChange={setWhen} />}
      {event === "server_start" && <p className="studio-placeholders">{tr("Runs once, a few seconds after the server (or the add-on) starts. No player.")}</p>}
      {event === "timer" && <Whole data={when} name="everySeconds" label={tr("Every (seconds)")} fallback={60} least={1} onChange={setWhen} />}
      {event === "schedule" && <List data={when} name="at" label={tr("Times")} placeholder="08:00, 20:30" hint={tr("24-hour times on the server's clock.")} onChange={setWhen} />}
      {(event === "timer" || event === "schedule") && <Toggle data={when} name="perPlayer" label={tr("Run once for each online player")} onChange={setWhen} />}
      <Whole data={data} name="cooldownSeconds" label={tr("Cooldown (seconds)")} fallback={0} least={0} hint={tr("Per player. 0 = none.")} onChange={onChange} />
    </>
  );
}

function Decimal({ data, name, label, fallback, onChange }: Omit<FieldProps, "hint" | "placeholder"> & { fallback: number }) {
  return (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input
        type="number"
        step="any"
        value={numberOf(data, name, fallback)}
        onChange={(event) => {
          const value = Number(event.target.value);
          if (event.target.value.trim() && Number.isFinite(value)) onChange({ ...data, [name]: value });
        }}
      />
    </label>
  );
}

function Pick({ data, name, label, options, fallback, hint, onChange }: Omit<FieldProps, "placeholder"> & { options: string[]; fallback: string }) {
  return (
    <label className="field studio-field">
      <span>{label}</span>
      <Dropdown value={choiceOf(data, name, options, fallback)} onChange={(value) => onChange({ ...data, [name]: value })} options={options.map((one) => ({ value: one, label: one }))} />
      {hint && <small>{hint}</small>}
    </label>
  );
}

/** World and position, for things that stand at a spot. */
function Place({ data, onChange, yaw = false, serverId }: { data: StudioData; onChange: (next: StudioData) => void; yaw?: boolean; serverId?: string }) {
  return (
    <>
      <Text data={data} name="world" label={tr("World")} placeholder="world" onChange={onChange} />
      <div className={`studio-fields-row ${yaw ? "four" : "three"}`}>
        <Decimal data={data} name="x" label="X" fallback={0.5} onChange={onChange} />
        <Decimal data={data} name="y" label="Y" fallback={64} onChange={onChange} />
        <Decimal data={data} name="z" label="Z" fallback={0.5} onChange={onChange} />
        {yaw && <Decimal data={data} name="yaw" label={tr("Facing")} fallback={0} onChange={onChange} />}
      </div>
      <PlayerSpot serverId={serverId} data={data} onChange={onChange} />
    </>
  );
}

function NpcFields({ data, onChange, names, serverId }: KindProps & { serverId?: string }) {
  return (
    <>
      <Text data={data} name="name" label={tr("Name over its head")} hint={tr("Leave empty for no name.")} onChange={onChange} />
      <div className="studio-fields-row">
        <Text data={data} name="entity" label={tr("Mob")} placeholder="villager" hint={tr("Any mob name, like villager, zombie or armor_stand.")} onChange={onChange} />
        {(textOf(data, "entity") || "villager").replace(/^minecraft:/, "") === "villager" && <Text data={data} name="profession" label={tr("Profession")} placeholder="librarian" onChange={onChange} />}
      </div>
      <Place data={data} onChange={onChange} yaw serverId={serverId} />
      <label className="studio-toggle">
        <span>{tr("Turns to look at players")}</span>
        <Switch checked={data.lookAtPlayers !== false} onChange={(event) => onChange({ ...data, lookAtPlayers: event.target.checked })} />
      </label>
      <Toggle data={data} name="glow" label={tr("Glows")} onChange={onChange} />
      <Toggle data={data} name="baby" label={tr("Baby")} onChange={onChange} />
      <Whole data={data} name="cooldownSeconds" label={tr("Cooldown (seconds)")} fallback={0} least={0} onChange={onChange} />
      <StepList conditions label={tr("Only if")} empty={tr("No conditions: anyone can use it.")} value={data.if} names={names} onChange={(next) => onChange({ ...data, if: next })} />
      <StepList label={tr("When right-clicked")} empty={tr("Nothing yet. Add an action.")} value={data.actions} names={names} onChange={(actions) => onChange({ ...data, actions })} />
      <StepList label={tr("When a condition fails")} empty={tr("Nothing happens when a condition fails.")} value={data.deny} names={names} onChange={(deny) => onChange({ ...data, deny })} />
      <p className="studio-placeholders">{tr("It can't be hurt and doesn't move. It shows while its chunk is loaded.")}</p>
    </>
  );
}

function LootFields({ data, onChange }: KindProps) {
  const drops = objectOf(data.drops);
  const from = choiceOf(drops, "from", LOOT_SOURCES, "none");
  const entries = listOf(data.entries);
  const setDrops = (next: StudioData) => onChange({ ...data, drops: next });
  const setEntry = (at: number, next: StudioData) => onChange({ ...data, entries: entries.map((one, index) => (index === at ? next : one)) });
  const total = entries.reduce((sum, one) => sum + Math.max(0, numberOf(one, "weight", 1)), 0);
  return (
    <>
      <div className="studio-fields-row">
        <Pick data={drops} name="from" label={tr("Drops from")} options={LOOT_SOURCES} fallback="none" hint={tr("none: only the Give loot action gives it.")} onChange={setDrops} />
        {from !== "none" && from !== "fishing" && (
          <List
            data={drops}
            name="targets"
            label={from === "block" ? tr("Blocks") : from === "chest" ? tr("Chest loot tables") : tr("Mobs")}
            placeholder={from === "block" ? "diamond_ore, deepslate_diamond_ore" : from === "chest" ? "chests/simple_dungeon, chests/village/village_weaponsmith" : "zombie, skeleton"}
            hint={tr("Empty = every one.")}
            onChange={setDrops}
          />
        )}
      </div>
      {from !== "none" && <Toggle data={drops} name="replace" label={from === "fishing" ? tr("Replace the catch") : from === "chest" ? tr("Replace the chest's own loot") : tr("Replace the normal drops")} onChange={setDrops} />}
      {from === "fishing" && <p className="studio-placeholders">{tr("Each catch also gives a roll of this table, straight into the player's inventory.")}</p>}
      {from === "mob" && <Toggle data={drops} name="needsPlayer" label={tr("Only when a player kills it")} onChange={setDrops} />}
      <div className="studio-fields-row">
        <Whole data={data} name="rolls" label={tr("Rolls")} fallback={1} least={0} most={64} onChange={onChange} />
        <Whole data={data} name="rollsMax" label={tr("Up to (rolls)")} fallback={numberOf(data, "rolls", 1)} least={0} most={64} hint={tr("A random count between the two.")} onChange={onChange} />
      </div>
      <div className="studio-steps">
        <div className="studio-steps-head">
          <span>{tr("Entries")}</span>
          <button type="button" className="button ghost small" onClick={() => onChange({ ...data, entries: [...entries, { item: "diamond", min: 1, max: 1, weight: 1 }] })}>
            <Icon name="plus" />
            {tr("Add entry")}
          </button>
        </div>
        {entries.length === 0 && <p className="studio-steps-empty">{tr("Add at least one entry.")}</p>}
        {entries.map((entry, at) => {
          const weight = Math.max(0, numberOf(entry, "weight", 1));
          return (
            <div key={at} className="studio-step">
              <div className="studio-step-head">
                <span className="studio-step-number">{at + 1}</span>
                <strong>{textOf(entry, "item") || tr("Nothing")}</strong>
                <span className="muted studio-loot-chance">{total > 0 ? `${Math.round((weight / total) * 1000) / 10}%` : "0%"}</span>
                <span className="studio-step-tools">
                  <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={() => onChange({ ...data, entries: entries.filter((_, index) => index !== at) })}>
                    <Icon name="trash" />
                  </button>
                </span>
              </div>
              <div className="studio-step-fields">
                <ItemField label={tr("Item")} value={textOf(entry, "item")} placeholder={tr("Empty = nothing")} onChange={(item) => setEntry(at, { ...entry, item })} />
                <Whole data={entry} name="min" label={tr("At least")} fallback={1} least={1} onChange={(next) => setEntry(at, next)} />
                <Whole data={entry} name="max" label={tr("At most")} fallback={numberOf(entry, "min", 1)} least={1} onChange={(next) => setEntry(at, next)} />
                <Whole data={entry} name="weight" label={tr("Weight")} fallback={1} least={0} onChange={(next) => setEntry(at, next)} />
              </div>
            </div>
          );
        })}
      </div>
      {from !== "none" && <ShowFields data={data} onChange={onChange} />}
      <p className="studio-placeholders">{tr("Items can be plain (diamond) or studio items (studio:name). Each roll picks one entry by weight.")}</p>
    </>
  );
}

/** What a studio enchantment may go on (`crates/studio-format/src/enchant.rs`). */
const ENCHANT_GROUPS = ["any", "weapon", "tool", "armor", "sword", "axe", "pickaxe", "shovel", "hoe", "bow", "crossbow", "trident", "mace", "fishing_rod", "helmet", "chestplate", "leggings", "boots", "elytra"];
const enchantGroupLabel = (one: string) =>
  (({ any: tr("Any item"), weapon: tr("Weapons"), tool: tr("Tools"), armor: tr("Armor") }) as Record<string, string>)[one] ?? one.replace(/_/g, " ");

function EnchantFields({ data, onChange, names }: KindProps) {
  const groups = linesOf(data, "items");
  const flip = (group: string, on: boolean) => onChange({ ...data, items: on ? [...groups.filter((one) => one !== group), group] : groups.filter((one) => one !== group) });
  return (
    <>
      <Text data={data} name="name" label={tr("Name")} hint={tr("Shown first in the item's description, with the level.")} onChange={onChange} />
      <div className="studio-fields-row">
        <Whole data={data} name="maxLevel" label={tr("Highest level")} fallback={1} least={1} most={255} onChange={onChange} />
        <Whole data={data} name="chance" label={tr("Chance (%)")} fallback={100} least={0} most={100} hint={tr("Each time it would run.")} onChange={onChange} />
      </div>
      <div className="field studio-field">
        <span>{tr("Goes on")}</span>
        <RowList variant="box" density="compact" className="check-list studio-enchant-groups">
          {ENCHANT_GROUPS.map((group) => (
            <ListRow
              key={group}
              leading={<input type="checkbox" checked={groups.includes(group)} aria-label={enchantGroupLabel(group)} onChange={(event) => flip(group, event.target.checked)} />}
              onClick={() => flip(group, !groups.includes(group))}
              mainProps={{ tabIndex: -1 }}
              title={enchantGroupLabel(group)}
            />
          ))}
        </RowList>
        <small>{tr("Nothing picked = any item.")}</small>
      </div>
      <StepList label={tr("When it hits")} empty={tr("Hitting does nothing more.")} value={data.hitActions} names={names} onChange={(hitActions) => onChange({ ...data, hitActions })} />
      <StepList label={tr("When it breaks a block")} empty={tr("Mining does nothing more.")} value={data.mineActions} names={names} onChange={(mineActions) => onChange({ ...data, mineActions })} />
      <StepList label={tr("When the wearer is hurt")} empty={tr("Getting hurt does nothing more.")} value={data.hurtActions} names={names} onChange={(hurtActions) => onChange({ ...data, hurtActions })} />
      <p className="studio-placeholders">{tr("Items get it from the Enchant the held item action, or as studio:{0} in an item's enchantments. <level> is its level, so actions can scale with it.", [textOf(data, "id") || "name"])}</p>
    </>
  );
}

function SongFields({ data, onChange }: KindProps) {
  return (
    <>
      <Text data={data} name="name" label={tr("Shown as")} placeholder="C418 - cat" hint={tr("Under the disc's name and in the Now playing line. MiniMessage works.")} onChange={onChange} />
      <Text data={data} name="sound" label={tr("Sound")} placeholder="boss_theme" hint={tr("A studio sound's file name (Assets → Sounds), or a sound id like minecraft:music_disc.cat.")} onChange={onChange} />
      <div className="studio-fields-row">
        <Whole data={data} name="seconds" label={tr("Length (seconds)")} fallback={60} least={1} most={3600} hint={tr("How long the jukebox plays it.")} onChange={onChange} />
        <Whole data={data} name="comparator" label={tr("Comparator signal")} fallback={1} least={0} most={15} onChange={onChange} />
      </div>
      <p className="studio-placeholders">{tr("Items play it with Music disc in their fields. The server takes new and changed songs when it starts.")}</p>
    </>
  );
}

function TradeFields({ data, onChange }: KindProps) {
  const offers = Array.isArray(data.offers) ? (data.offers as StudioData[]) : [];
  const setOffers = (next: StudioData[]) => onChange({ ...data, offers: next });
  const change = (at: number, key: string, value: unknown) => setOffers(offers.map((was, index) => (index === at ? { ...was, [key]: value } : was)));
  const amount = (one: StudioData, at: number, key: string, label: string) => (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input type="number" min={1} max={64} value={numberOf(one, key, 1)} onChange={(event) => change(at, key, Math.min(64, Math.max(1, Math.round(Number(event.target.value) || 1))))} />
    </label>
  );
  return (
    <>
      <Text data={data} name="title" label={tr("Screen title")} placeholder={tr("Trades")} onChange={onChange} />
      <Whole data={data} name="restockMinutes" label={tr("Restock after (minutes)")} fallback={0} least={0} most={43200} hint={tr("0 = limited offers never come back (until a restart).")} onChange={onChange} />
      <div className="studio-steps">
        <div className="studio-steps-head">
          <span>{tr("Offers")}</span>
          <button type="button" className="button ghost small" disabled={offers.length >= 64} onClick={() => setOffers([...offers, { buy: "emerald", buyAmount: 1, sell: "", sellAmount: 1, uses: 0 }])}>
            <Icon name="plus" />
            {tr("Add offer")}
          </button>
        </div>
        {offers.length === 0 && <p className="studio-steps-empty">{tr("No offers yet.")}</p>}
        {offers.map((one, at) => (
          <div key={at} className="studio-step">
            <div className="studio-step-head">
              <strong>{tr("Offer {0}", [String(at + 1)])}</strong>
              <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={() => setOffers(offers.filter((_, index) => index !== at))}>
                <Icon name="trash" />
              </button>
            </div>
            <div className="studio-fields-row studio-trade-row">
              <ItemField label={tr("Player gives")} value={textOf(one, "buy")} placeholder="emerald" empty={false} onChange={(item) => change(at, "buy", item)} />
              {amount(one, at, "buyAmount", tr("Amount"))}
            </div>
            <div className="studio-fields-row studio-trade-row">
              <ItemField label={tr("And also (optional)")} value={textOf(one, "buy2")} placeholder={tr("Nothing")} onChange={(item) => change(at, "buy2", item || undefined)} />
              {amount(one, at, "buy2Amount", tr("Amount"))}
            </div>
            <div className="studio-fields-row studio-trade-row">
              <ItemField label={tr("Player gets")} value={textOf(one, "sell")} placeholder="studio:ruby" empty={false} onChange={(item) => change(at, "sell", item)} />
              {amount(one, at, "sellAmount", tr("Amount"))}
            </div>
            <label className="field studio-field studio-number">
              <span>{tr("Times each player may take it")}</span>
              <input type="number" min={0} max={100000} value={numberOf(one, "uses", 0)} onChange={(event) => change(at, "uses", Math.max(0, Math.round(Number(event.target.value) || 0)))} />
              <small>{tr("0 = no limit.")}</small>
            </label>
          </div>
        ))}
      </div>
      <p className="studio-placeholders">{tr("Open it with the Open trades action (from an NPC, a command or a menu button). Paper only for now: Pumpkin has no trade screen for plugins yet.")}</p>
    </>
  );
}

function ShopFields({ data, onChange }: KindProps) {
  const goods = Array.isArray(data.goods) ? (data.goods as StudioData[]) : [];
  const setGoods = (next: StudioData[]) => onChange({ ...data, goods: next });
  const change = (at: number, key: string, value: unknown) => setGoods(goods.map((was, index) => (index === at ? { ...was, [key]: value } : was)));
  const vault = textOf(data, "currency") === "vault";
  const price = (one: StudioData, at: number, key: string, label: string) => (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input type="number" min={0} step="any" value={numberOf(one, key, 0)} onChange={(event) => change(at, key, Math.max(0, Number(event.target.value) || 0))} />
    </label>
  );
  return (
    <>
      <Text data={data} name="title" label={tr("Screen title")} placeholder={tr("Shop")} onChange={onChange} />
      <label className="field studio-field">
        <span>{tr("Money")}</span>
        <Dropdown
          value={vault ? "vault" : "variable"}
          onChange={(value) => onChange({ ...data, currency: value })}
          options={[
            { value: "variable", label: tr("A player variable"), hint: "variable" },
            { value: "vault", label: tr("Vault (an economy plugin, Paper only)"), hint: "vault" },
          ]}
        />
      </label>
      <div className="studio-fields-row">
        {!vault && <Text data={data} name="variable" label={tr("Variable")} placeholder="coins" hint={tr("Change it with the Change a variable action, or show it with <pvar:coins>.")} onChange={onChange} />}
        <Text data={data} name="moneyName" label={tr("Money is called")} placeholder="coins" onChange={onChange} />
      </div>
      <Whole data={data} name="rows" label={tr("Rows")} fallback={0} least={0} most={6} hint={tr("0 = just enough for the goods.")} onChange={onChange} />
      <div className="studio-steps">
        <div className="studio-steps-head">
          <span>{tr("Goods")}</span>
          <button type="button" className="button ghost small" disabled={goods.length >= 54} onClick={() => setGoods([...goods, { item: "", amount: 1, buy: 10, sell: 0 }])}>
            <Icon name="plus" />
            {tr("Add good")}
          </button>
        </div>
        {goods.length === 0 && <p className="studio-steps-empty">{tr("No goods yet.")}</p>}
        {goods.map((one, at) => (
          <div key={at} className="studio-step">
            <div className="studio-step-head">
              <strong>{tr("Slot {0}", [String(at + 1)])}</strong>
              <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={() => setGoods(goods.filter((_, index) => index !== at))}>
                <Icon name="trash" />
              </button>
            </div>
            <div className="studio-fields-row studio-trade-row">
              <ItemField label={tr("Item")} value={textOf(one, "item")} placeholder="diamond" empty={false} onChange={(item) => change(at, "item", item)} />
              <label className="field studio-field studio-number">
                <span>{tr("Amount")}</span>
                <input type="number" min={1} max={64} value={numberOf(one, "amount", 1)} onChange={(event) => change(at, "amount", Math.min(64, Math.max(1, Math.round(Number(event.target.value) || 1))))} />
              </label>
            </div>
            <div className="studio-fields-row">
              {price(one, at, "buy", tr("Players buy it for"))}
              {price(one, at, "sell", tr("Players sell it for"))}
            </div>
          </div>
        ))}
      </div>
      <p className="studio-placeholders">{tr("0 = can't be bought (or sold). In the game: left click buys, right click sells, shift + right click sells all. Open it with the Open a shop action.")}</p>
    </>
  );
}

/** Quest step types (`crates/studio-format/src/quest.rs`). */
const QUEST_STEPS = ["kill", "collect", "break", "place", "craft", "fish", "talk", "visit"];

function questStepLabel(type: string): string {
  switch (type) {
    case "collect":
      return tr("Collect items");
    case "break":
      return tr("Break blocks");
    case "place":
      return tr("Place blocks");
    case "craft":
      return tr("Craft items");
    case "fish":
      return tr("Catch fish");
    case "talk":
      return tr("Talk to an NPC");
    case "visit":
      return tr("Go to a spot");
    default:
      return tr("Kill mobs");
  }
}

/** A quest: steps in order, then rewards (`crates/studio-format/src/quest.rs`). */
function QuestFields({ data, onChange, names, serverId }: KindProps & { serverId?: string }) {
  const steps = listOf(data.steps);
  const setSteps = (next: StudioData[]) => onChange({ ...data, steps: next });
  const change = (at: number, next: StudioData) => setSteps(steps.map((was, index) => (index === at ? next : was)));
  const move = (at: number, to: number) => {
    if (to < 0 || to >= steps.length) return;
    const list = [...steps];
    const [one] = list.splice(at, 1);
    list.splice(to, 0, one);
    setSteps(list);
  };
  return (
    <>
      <Text data={data} name="name" label={tr("Quest name")} placeholder={tr("Lost sheep")} onChange={onChange} />
      <Text data={data} name="description" label={tr("What it is about")} onChange={onChange} />
      <Toggle data={data} name="repeatable" label={tr("Can be done again")} onChange={onChange} />
      <label className="field studio-field">
        <span>{tr("Advancement when finished (optional)")}</span>
        <input list="studio-quest-advancements" value={textOf(data, "advancement")} placeholder="first_steps" spellCheck={false} onChange={(event) => onChange({ ...data, advancement: event.target.value })} />
        <datalist id="studio-quest-advancements">
          {names.advancement.map((one) => (
            <option key={one} value={one} />
          ))}
        </datalist>
      </label>
      <div className="studio-steps">
        <div className="studio-steps-head">
          <span>{tr("Steps, in order")}</span>
          <button type="button" className="button ghost small" disabled={steps.length >= 32} onClick={() => setSteps([...steps, { type: "kill", target: "zombie", amount: 5, text: "" }])}>
            <Icon name="plus" />
            {tr("Add step")}
          </button>
        </div>
        {steps.length === 0 && <p className="studio-steps-empty">{tr("No steps yet.")}</p>}
        {steps.map((step, at) => {
          const type = QUEST_STEPS.includes(textOf(step, "type")) ? textOf(step, "type") : "kill";
          return (
            <div key={at} className="studio-step">
              <div className="studio-step-head">
                <strong>{tr("Step {0}", [String(at + 1)])}</strong>
                <span className="studio-step-tools">
                  <button type="button" className="icon-button" aria-label={tr("Move up")} data-tip={tr("Move up")} disabled={at === 0} onClick={() => move(at, at - 1)}>
                    <Icon name="arrow-up" />
                  </button>
                  <button type="button" className="icon-button" aria-label={tr("Move down")} data-tip={tr("Move down")} disabled={at === steps.length - 1} onClick={() => move(at, at + 1)}>
                    <Icon name="arrow-down" />
                  </button>
                  <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={() => setSteps(steps.filter((_, index) => index !== at))}>
                    <Icon name="trash" />
                  </button>
                </span>
              </div>
              <div className="studio-fields-row">
                <label className="field studio-field">
                  <span>{tr("Players have to")}</span>
                  <Dropdown value={type} onChange={(value) => change(at, { ...step, type: value })} options={QUEST_STEPS.map((one) => ({ value: one, label: questStepLabel(one) }))} />
                </label>
                {type !== "visit" && (
                  <Text
                    data={step}
                    name="target"
                    label={type === "talk" ? tr("NPC (its file name)") : tr("Which (empty = any)")}
                    placeholder={type === "talk" ? "elder" : type === "kill" ? "zombie, studio:goblin" : type === "fish" ? "cod" : "wheat"}
                    hint={type === "talk" ? undefined : tr("Game names; several split by commas. A studio mob is studio:<id>.")}
                    onChange={(next) => change(at, next)}
                  />
                )}
                {type !== "visit" && type !== "talk" && <Whole data={step} name="amount" label={tr("How many")} fallback={1} least={1} most={100000} onChange={(next) => change(at, next)} />}
              </div>
              {type === "visit" && (
                <>
                  <Place data={step} onChange={(next) => change(at, next)} serverId={serverId} />
                  <Whole data={step} name="radius" label={tr("Within (blocks)")} fallback={5} least={1} most={512} onChange={(next) => change(at, next)} />
                </>
              )}
              <Text data={step} name="text" label={tr("What players read (optional)")} placeholder={tr("Made from the step when empty")} onChange={(next) => change(at, next)} />
            </div>
          );
        })}
      </div>
      <StepList label={tr("When it starts")} empty={tr("Starting shows only the first step.")} value={data.startActions} names={names} onChange={(startActions) => onChange({ ...data, startActions })} />
      <StepList label={tr("Rewards")} empty={tr("No rewards yet. Add an action, like Give an item.")} value={data.rewards} names={names} onChange={(rewards) => onChange({ ...data, rewards })} />
      <p className="studio-placeholders">{tr("Start it with the Start or end a quest action (from an NPC, a command or a menu button), and check it with the Quest state condition. Progress shows in the action bar; <quest_name> is its name in rewards.")}</p>
    </>
  );
}

/** A function: a named action list other lists run with the Run function action (`crates/studio-format/src/function.rs`). */
function FunctionFields({ data, onChange, names }: { data: StudioData; onChange: (next: StudioData) => void; names: StudioNames }) {
  return (
    <>
      <Text data={data} name="description" label={tr("What it is for")} placeholder={tr("Welcome a player")} onChange={onChange} />
      <StepList label={tr("Steps")} empty={tr("Nothing yet. Add an action.")} value={data.actions} names={names} onChange={(actions) => onChange({ ...data, actions })} />
      <p className="studio-placeholders">{tr("Run it from any action list with the Run function action. It runs for the same player, with the same placeholders, as the list that runs it. A wait inside it holds back the rest of that list too.")}</p>
    </>
  );
}

/** Where a cosmetic goes (`crates/studio-format/src/cosmetic.rs`). */
const COSMETIC_SLOTS = ["hat", "back", "trail"];
const cosmeticSlotLabel = (slot: string) => (({ hat: tr("Hat"), back: tr("Back"), trail: tr("Walking trail") }) as Record<string, string>)[slot] ?? slot;

function CosmeticFields({ data, onChange }: KindProps) {
  const slot = choiceOf(data, "slot", COSMETIC_SLOTS, "hat");
  return (
    <>
      <label className="field studio-field">
        <span>{tr("Slot")}</span>
        <Dropdown value={slot} options={COSMETIC_SLOTS.map((one) => ({ value: one, label: cosmeticSlotLabel(one) }))} onChange={(next) => onChange({ ...data, slot: next })} />
        <small>{tr("A player wears one of each.")}</small>
      </label>
      {slot === "trail" ? (
        <div className="studio-fields-row">
          <Text data={data} name="particle" label={tr("Particle")} placeholder="heart" hint={tr("One that needs no colour or block: flame, heart, end_rod, note...")} onChange={onChange} />
          <Whole data={data} name="count" label={tr("Particles a step")} fallback={2} least={1} most={20} onChange={onChange} />
        </div>
      ) : (
        <>
          <ItemField label={tr("Shown item")} value={textOf(data, "item")} placeholder="studio:crown" empty={false} onChange={(item) => onChange({ ...data, item })} />
          <div className="studio-fields-row">
            <Decimal data={data} name="scale" label={tr("Size")} fallback={1} onChange={onChange} />
            <Decimal data={data} name="up" label={tr("Up (blocks)")} fallback={0} onChange={onChange} />
            <Decimal data={data} name="back" label={tr("Back (blocks)")} fallback={0} onChange={onChange} />
          </div>
        </>
      )}
      <Text data={data} name="permission" label={tr("Permission")} placeholder="cosmetics.crown" hint={tr("Empty = anyone may wear it.")} onChange={onChange} />
      <p className="studio-placeholders">{tr("Players put it on with the Wear a cosmetic action (from a menu button, a command or an NPC). It stays on after they rejoin.")}</p>
    </>
  );
}

function PaintingFields({ data, onChange, serverId }: { data: StudioData; onChange: (next: StudioData) => void; serverId?: string }) {
  const assets = useAssets(serverId);
  return (
    <>
      <AssetPick serverId={serverId} assets={assets} folder="textures" value={textOf(data, "texture")} label={tr("Picture")} hint={tr("16 pixels per block looks like the game's own paintings.")} onChange={(texture) => onChange({ ...data, texture: texture || undefined })} />
      <div className="studio-fields-row">
        <Whole data={data} name="width" label={tr("Width (blocks)")} fallback={1} least={1} most={16} onChange={onChange} />
        <Whole data={data} name="height" label={tr("Height (blocks)")} fallback={1} least={1} most={16} onChange={onChange} />
      </div>
      <Text data={data} name="title" label={tr("Title")} hint={tr("Shown on the painting item. MiniMessage works.")} onChange={onChange} />
      <Text data={data} name="author" label={tr("Author")} onChange={onChange} />
      <p className="studio-placeholders">{tr("Give it with studio:painting/{0}. The server takes new paintings and new sizes when it starts.", [textOf(data, "id") || "name"])}</p>
    </>
  );
}

function BiomeFields({ data, onChange }: KindProps) {
  return (
    <>
      <div className="studio-fields-row three">
        <Text data={data} name="sky" label={tr("Sky colour")} placeholder="#78a7ff" onChange={onChange} />
        <Text data={data} name="fog" label={tr("Fog colour")} placeholder="#c0d8ff" onChange={onChange} />
        <Text data={data} name="water" label={tr("Water colour")} placeholder="#3f76e4" onChange={onChange} />
      </div>
      <div className="studio-fields-row three">
        <Text data={data} name="waterFog" label={tr("Underwater fog colour")} placeholder="#050533" onChange={onChange} />
        <Text data={data} name="grass" label={tr("Grass colour")} placeholder={tr("From warmth and wetness")} onChange={onChange} />
        <Text data={data} name="foliage" label={tr("Leaf colour")} placeholder={tr("From warmth and wetness")} onChange={onChange} />
      </div>
      <div className="studio-fields-row">
        <Text data={data} name="particle" label={tr("Floating particles")} placeholder="white_ash" hint={tr("A particle name, like white_ash, crimson_spore or cherry_leaves.")} onChange={onChange} />
        <Decimal data={data} name="particleChance" label={tr("How many (0 to 1)")} fallback={0.01} onChange={onChange} />
      </div>
      <div className="studio-fields-row">
        <Text data={data} name="music" label={tr("Music")} placeholder="calm_theme" hint={tr("A studio sound's file name, or a sound id like minecraft:music.overworld.cherry_grove.")} onChange={onChange} />
        <Text data={data} name="ambient" label={tr("Ambient sound loop")} placeholder="minecraft:ambient.basalt_deltas.loop" onChange={onChange} />
      </div>
      <div className="studio-fields-row three">
        <Decimal data={data} name="temperature" label={tr("Warmth")} fallback={0.8} onChange={onChange} />
        <Decimal data={data} name="downfall" label={tr("Wetness")} fallback={0.4} onChange={onChange} />
        <Text data={data} name="like" label={tr("Shown on Pumpkin as")} placeholder="plains" hint={tr("A vanilla biome, until the Pumpkin fork can place your own.")} onChange={onChange} />
      </div>
      <label className="studio-toggle">
        <span>{tr("Rain and snow")}</span>
        <Switch checked={data.rain !== false} onChange={(event) => onChange({ ...data, rain: event.target.checked })} />
      </label>
      <p className="studio-placeholders">{tr("Use it in a world generator's biome rules as studio:{0}. Its caves, ores, plants and mobs are like plains. The server takes new and changed biomes when it starts.", [textOf(data, "id") || "name"])}</p>
    </>
  );
}

function AdvancementFields({ data, onChange, names }: KindProps) {
  const root = !textOf(data, "parent").trim();
  return (
    <>
      <Text data={data} name="title" label={tr("Title")} onChange={onChange} />
      <Text data={data} name="description" label={tr("Description")} onChange={onChange} />
      <div className="studio-fields-row">
        <ItemField label={tr("Icon item")} value={textOf(data, "icon")} placeholder="diamond" onChange={(icon) => onChange({ ...data, icon })} />
        <Pick data={data} name="frame" label={tr("Frame")} options={ADVANCEMENT_FRAMES} fallback="task" onChange={onChange} />
      </div>
      <label className="field studio-field">
        <span>{tr("Comes after")}</span>
        <input list="studio-advancement-parents" value={textOf(data, "parent")} placeholder={tr("Nothing: starts its own tab")} onChange={(event) => onChange({ ...data, parent: event.target.value.trim() })} spellCheck={false} />
        <datalist id="studio-advancement-parents">
          {names.advancement.map((name) => (
            <option key={name} value={name} />
          ))}
        </datalist>
      </label>
      {root && <Text data={data} name="background" label={tr("Tab background")} placeholder="stone" hint={tr("A block name, like stone or dirt.")} onChange={onChange} />}
      <label className="studio-toggle">
        <span>{tr("Pop-up when earned")}</span>
        <Switch checked={data.toast !== false} onChange={(event) => onChange({ ...data, toast: event.target.checked })} />
      </label>
      <label className="studio-toggle">
        <span>{tr("Say it in chat")}</span>
        <Switch checked={data.announce !== false} onChange={(event) => onChange({ ...data, announce: event.target.checked })} />
      </label>
      <Toggle data={data} name="hidden" label={tr("Hidden until earned")} onChange={onChange} />
      <StepList label={tr("Rewards")} empty={tr("No rewards.")} value={data.rewards} names={names} onChange={(rewards) => onChange({ ...data, rewards })} />
      <p className="studio-placeholders">{tr("Players get it from the Grant an advancement action, in a rule, command, NPC or dialog.")}</p>
    </>
  );
}

/** "name value" rows to a JSON object of game rules (true/false and numbers become those). */
function rulesFrom(text: string): StudioData {
  return Object.fromEntries(
    text
      .split("\n")
      .map((line) => line.trim().split(/\s+/))
      .filter(([name]) => name)
      .map(([name, value = ""]) => [name, value === "true" ? true : value === "false" ? false : value !== "" && Number.isFinite(Number(value)) ? Number(value) : value]),
  );
}

function WorldFields({ data, onChange, names }: KindProps) {
  const type = choiceOf(data, "type", WORLD_TYPES, "normal");
  const layers = listOf(data.layers);
  const rules = objectOf(data.gamerules);
  return (
    <>
      <Text data={data} name="name" label={tr("World folder")} hint={tr("Teleport players here with the world name {0}.", [textOf(data, "name") || "…"])} onChange={onChange} />
      <div className="studio-fields-row">
        <Pick data={data} name="type" label={tr("Kind")} options={WORLD_TYPES} fallback="normal" onChange={(next) => onChange(next.type === "custom" && !next.generator ? { ...next, generator: defaultGenerator(), structures: undefined } : next)} />
        <Pick data={data} name="environment" label={tr("Dimension")} options={WORLD_ENVIRONMENTS} fallback="normal" onChange={onChange} />
      </div>
      <Text data={data} name="seed" label={tr("Seed")} placeholder={tr("Random")} onChange={onChange} />
      {type === "custom" && (
        <>
          <label className="studio-toggle">
            <span>{tr("Use for the main world")}</span>
            <Switch checked={data.main === true} onChange={(event) => onChange({ ...data, main: event.target.checked ? true : undefined })} />
          </label>
          {data.main === true && (
            <p className="studio-placeholders">
              {tr("The server's own world makes new land with this from its next start, instead of a world of its own. Land it already made stays as it is, so use it on a new server or after deleting the world folder.")}
            </p>
          )}
          <WorldgenFields data={data} onChange={onChange} structures={names.structure} biomes={names.biome} />
        </>
      )}
      {type !== "void" && type !== "custom" && (
        <label className="studio-toggle">
          <span>{tr("Villages and other structures")}</span>
          <Switch checked={data.structures !== false} onChange={(event) => onChange({ ...data, structures: event.target.checked })} />
        </label>
      )}
      {type === "flat" && (
        <>
          <label className="field studio-field">
            <span>{tr("Layers (bottom first)")}</span>
            <textarea
              rows={Math.max(3, layers.length)}
              value={layers.map((one) => `${textOf(one, "block")} ${numberOf(one, "height", 1)}`).join("\n")}
              placeholder={"bedrock 1\ndirt 2\ngrass_block 1"}
              onChange={(event) =>
                onChange({
                  ...data,
                  layers: event.target.value
                    .split("\n")
                    .map((line) => line.trim().split(/\s+/))
                    .filter(([block]) => block)
                    .map(([block, height]) => ({ block, height: Math.max(1, Math.round(Number(height) || 1)) })),
                })
              }
              spellCheck={false}
            />
            <small>{tr("One block and its height per row.")}</small>
          </label>
          <Text data={data} name="biome" label={tr("Biome")} placeholder="plains" onChange={onChange} />
        </>
      )}
      <label className="studio-toggle">
        <span>{tr("PvP")}</span>
        <Switch checked={data.pvp !== false} onChange={(event) => onChange({ ...data, pvp: event.target.checked })} />
      </label>
      <label className="field studio-field">
        <span>{tr("Game rules")}</span>
        <textarea
          rows={Math.max(2, Object.keys(rules).length)}
          value={Object.entries(rules)
            .map(([name, value]) => `${name} ${String(value)}`)
            .join("\n")}
          placeholder="doDaylightCycle false"
          onChange={(event) => onChange({ ...data, gamerules: rulesFrom(event.target.value) })}
          spellCheck={false}
        />
        <small>{tr("One rule and its value per row.")}</small>
      </label>
      <p className="studio-placeholders">{tr("The kind, dimension, seed and layers only count when the world is first made. Removing this file doesn't delete the world.")}</p>
    </>
  );
}

/** Worlds and permission (the shared "show" block), for kinds whose main form doesn't have it. */
function ShowFields({ data, onChange }: { data: StudioData; onChange: (next: StudioData) => void }) {
  const show = objectOf(data.show);
  const set = (key: string, value: unknown) => onChange({ ...data, show: { ...show, [key]: value } });
  return (
    <div className="studio-fields-row">
      <label className="field studio-field">
        <span>{tr("Worlds")}</span>
        <input
          value={linesOf(show, "worlds").join(", ")}
          placeholder={tr("Every world")}
          onChange={(event) =>
            set(
              "worlds",
              event.target.value
                .split(",")
                .map((one) => one.trim())
                .filter(Boolean),
            )
          }
          spellCheck={false}
        />
      </label>
      <label className="field studio-field">
        <span>{tr("Permission")}</span>
        <input value={textOf(show, "permission")} placeholder={tr("Everyone")} onChange={(event) => set("permission", event.target.value.trim())} spellCheck={false} />
      </label>
    </div>
  );
}

/** Rotations a structure can be put down at (`crates/studio-format/src/structure.rs`). */
export const STRUCTURE_ROTATIONS = ["0", "90", "180", "270", "random"];

async function fileBase64(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let text = "";
  for (let at = 0; at < bytes.length; at += 0x8000) text += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  return btoa(text);
}

/** A structure: a build brought in from a .schem or .nbt file (`crates/studio-format/src/structure.rs`). */
function StructureFields({ data, onChange, id, serverId }: { data: StudioData; onChange: (next: StudioData) => void; id?: string; serverId?: string }) {
  const picker = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState("");
  const size = Array.isArray(data.size) ? data.size.map((one) => Number(one) || 0) : [0, 0, 0];
  const blocks = typeof data.blocks === "number" ? data.blocks : 0;
  const bring = async (file: File | undefined) => {
    if (!file || !serverId || !id) return;
    setBusy(true);
    setProblem("");
    try {
      const made = await commands.studioStructureImport(serverId, id, file.name, await fileBase64(file));
      onChange({ ...data, source: file.name, size: made.size, blocks: made.blocks });
    } catch (error) {
      setProblem(trBack(String(error)));
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <div className="studio-structure-file">
        <div>
          <strong>{textOf(data, "source") || tr("No file yet")}</strong>
          <small>{blocks > 0 ? tr("{0} × {1} × {2}, {3} blocks", [String(size[0]), String(size[1]), String(size[2]), blocks.toLocaleString()]) : tr("Bring in a WorldEdit .schem or a structure block .nbt file.")}</small>
        </div>
        {!onWeb && (
          <>
            <input ref={picker} type="file" hidden accept=".schem,.schematic,.nbt" onChange={(event) => void bring(event.target.files?.[0]).finally(() => (event.target.value = ""))} />
            <button type="button" className="button ghost small" disabled={busy || !serverId || !id} onClick={() => picker.current?.click()}>
              <Icon name="upload" />
              {busy ? tr("Reading…") : blocks > 0 ? tr("Replace file") : tr("Import file")}
            </button>
          </>
        )}
      </div>
      {problem && <p className="studio-structure-problem">{problem}</p>}
      <Pick data={data} name="rotation" label={tr("Turned (degrees)")} options={STRUCTURE_ROTATIONS} fallback="0" hint={tr("The action can turn it another way.")} onChange={onChange} />
      <label className="studio-toggle">
        <span>{tr("Middle on the spot (otherwise its corner)")}</span>
        <Switch checked={data.centered !== false} onChange={(event) => onChange({ ...data, centered: event.target.checked })} />
      </label>
      <Whole data={data} name="lift" label={tr("Raise by (blocks)")} fallback={0} least={-64} most={64} hint={tr("Negative sinks it into the ground.")} onChange={onChange} />
      <p className="studio-placeholders">{tr("Put it down with the Place a structure action. Air is left out, so it doesn't clear what's already there; chest contents and sign text aren't brought along.")}</p>
    </>
  );
}

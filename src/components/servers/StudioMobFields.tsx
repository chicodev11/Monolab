import { useEffect, useState, type ReactNode } from "react";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { listOf, StepList } from "./StudioActions";
import { NumberField, put, SwitchRow } from "./StudioItemFields";
import { textOf, type StudioData } from "./StudioPreview";
import { objectOf, type StudioNames } from "./studioKinds";
import "./StudioItems.css";
import "./StudioModels.css";
import "./StudioMobs.css";
import { InSide, Studio3dStage } from "./StudioPack3d";

/**
 * Studio maker M5: a mob (`studio/mob/<id>.json`). A vanilla mob with its own name, stats, gear, model, drops,
 * simple AI switches, natural spawning and skills (trigger → targets → conditions → actions). The same field names
 * as `crates/studio-format/src/mob.rs` and the add-ons.
 */

/** Living mobs to start from; any other game mob name works too. */
export const MOB_BASES = [
  "zombie", "husk", "drowned", "zombie_villager", "skeleton", "stray", "bogged", "wither_skeleton", "creeper", "spider", "cave_spider",
  "enderman", "witch", "slime", "magma_cube", "blaze", "ghast", "phantom", "piglin", "piglin_brute", "zombified_piglin", "hoglin", "zoglin",
  "vindicator", "pillager", "evoker", "ravager", "vex", "guardian", "silverfish", "endermite", "shulker", "breeze", "warden", "iron_golem",
  "snow_golem", "wolf", "polar_bear", "bee", "llama", "panda", "fox", "goat", "cow", "pig", "sheep", "chicken", "rabbit", "horse", "donkey",
  "cat", "ocelot", "parrot", "frog", "axolotl", "turtle", "villager", "wandering_trader", "allay", "armadillo", "camel", "sniffer",
];
/** `studio_format::mob::TRIGGERS` / `TARGETS` / `SLOTS` / `AI`. */
export const MOB_TRIGGERS = ["spawn", "attack", "damaged", "death", "timer", "interact", "target"] as const;
export const MOB_TARGETS = ["self", "target", "trigger", "nearby"] as const;
export const MOB_SLOTS = ["head", "chest", "legs", "feet", "hand", "offhand"] as const;
export const MOB_AI = ["attack", "wander", "look", "burn", "pickup"] as const;
/** What the AI switches are when the file doesn't say. */
const AI_START: Record<string, boolean> = { attack: true, wander: true, look: true, burn: false, pickup: false };

function triggerLabel(one: string): string {
  switch (one) {
    case "spawn":
      return tr("When it spawns");
    case "attack":
      return tr("When it hits something");
    case "damaged":
      return tr("When it's hurt");
    case "death":
      return tr("When it dies");
    case "timer":
      return tr("Every few seconds");
    case "interact":
      return tr("When a player right-clicks it");
    case "target":
      return tr("When it picks a target");
    default:
      return one;
  }
}

function targetsLabel(one: string): string {
  switch (one) {
    case "self":
      return tr("The mob itself");
    case "target":
      return tr("Its current target");
    case "trigger":
      return tr("Who set it off");
    case "nearby":
      return tr("Players nearby");
    default:
      return one;
  }
}

function slotLabel(one: string): string {
  switch (one) {
    case "head":
      return tr("Head");
    case "chest":
      return tr("Chest");
    case "legs":
      return tr("Legs");
    case "feet":
      return tr("Feet");
    case "hand":
      return tr("Main hand");
    case "offhand":
      return tr("Off hand");
    default:
      return one;
  }
}

function aiLabel(one: string): string {
  switch (one) {
    case "attack":
      return tr("Attacks players");
    case "wander":
      return tr("Wanders around");
    case "look":
      return tr("Looks at players");
    case "burn":
      return tr("Burns in daylight");
    case "pickup":
      return tr("Picks up items");
    default:
      return one;
  }
}

/** Who "Who set it off" is for each trigger, as a hint under the targets. */
function triggerWho(trigger: string): string {
  switch (trigger) {
    case "attack":
      return tr("Who set it off: what it hit.");
    case "damaged":
      return tr("Who set it off: who hurt it.");
    case "death":
      return tr("Who set it off: who killed it.");
    case "interact":
      return tr("Who set it off: the player who clicked.");
    case "target":
      return tr("Who set it off: its new target.");
    default:
      return tr("Who set it off: nobody for this trigger, so the mob itself.");
  }
}

function move<T>(list: T[], from: number, to: number): T[] {
  if (to < 0 || to >= list.length) return list;
  const next = [...list];
  const [one] = next.splice(from, 1);
  next.splice(to, 0, one);
  return next;
}

const listText = (data: StudioData, key: string) => (Array.isArray(data[key]) ? (data[key] as unknown[]).filter((one) => typeof one === "string").join(", ") : "");
const listFrom = (value: string) => value.split(",").map((one) => one.trim()).filter(Boolean);

function prefersReducedMotion() {
  return typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
}

/** The picked model's .bbmodel and its idle animation, read the way the model form does. */
function useModelSource(serverId: string | undefined, model: string) {
  const [root, setRoot] = useState<string | null>(null);
  const [shown, setShown] = useState<{ source: string; idle: string | null } | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  useEffect(() => {
    if (!serverId) return;
    let live = true;
    commands.studioOverview(serverId).then(
      (overview) => live && setRoot(overview.root),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [serverId]);
  useEffect(() => {
    setShown(null);
    setProblem(null);
    if (!serverId || !root || !model) return;
    let live = true;
    void (async () => {
      try {
        const file = JSON.parse(await commands.readEditableFile(serverId, `${root}/model/${model}.json`)) as unknown;
        const read = objectOf(file);
        const asset = textOf(read, "model").trim().replace(/\.bbmodel$/, "");
        if (!asset) {
          if (live) setProblem(tr("The model {0} has no Blockbench model picked yet.", [model]));
          return;
        }
        const source = await commands.readEditableFile(serverId, `${root}/assets/models/${asset}.bbmodel`);
        const idle = textOf(objectOf(read.animations), "idle") || null;
        if (live) setShown({ source, idle });
      } catch (cause) {
        if (live) setProblem(String(cause));
      }
    })();
    return () => {
      live = false;
    };
  }, [serverId, root, model]);
  return { shown, problem };
}

function ModelPreview({ serverId, model }: { serverId?: string; model: string }) {
  const { shown, problem } = useModelSource(serverId, model);
  const still = prefersReducedMotion();
  if (!serverId) return <p className="muted">{tr("Open the studio on a server to see the model.")}</p>;
  if (problem) return <p className="studio-model-note">{problem}</p>;
  if (!shown) return <div className="studio-model-stage studio-mob-stage" />;
  return (
    <Studio3dStage source={shown.source} animation={shown.idle} time={still ? 0 : null} className="studio-mob-stage" />
  );
}

/** A titled group of fields. */
function Section({ title, hint, children }: { title: string; hint?: string; children: ReactNode }) {
  return (
    <section className="studio-mob-section">
      <div className="studio-mob-section-head">
        <strong>{title}</strong>
        {hint && <small>{hint}</small>}
      </div>
      {children}
    </section>
  );
}

function SkillCard({ skill, at, count, names, onChange, onMove, onRemove }: { skill: StudioData; at: number; count: number; names: StudioNames; onChange: (next: StudioData) => void; onMove: (to: number) => void; onRemove: () => void }) {
  const trigger = textOf(skill, "trigger") || "attack";
  const targets = textOf(skill, "targets") || "trigger";
  return (
    <div className="studio-step studio-mob-skill">
      <div className="studio-step-head">
        <span className="studio-step-number">{at + 1}</span>
        <strong>{triggerLabel(trigger)}</strong>
        <span className="studio-step-tools">
          <button type="button" className="icon-button" aria-label={tr("Move up")} data-tip={tr("Move up")} disabled={at === 0} onClick={() => onMove(at - 1)}>
            <Icon name="arrow-up" />
          </button>
          <button type="button" className="icon-button" aria-label={tr("Move down")} data-tip={tr("Move down")} disabled={at === count - 1} onClick={() => onMove(at + 1)}>
            <Icon name="arrow-down" />
          </button>
          <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={onRemove}>
            <Icon name="trash" />
          </button>
        </span>
      </div>
      <div className="studio-fields-row">
        <label className="field studio-field">
          <span>{tr("Trigger")}</span>
          <Dropdown value={trigger} onChange={(value) => onChange({ ...skill, trigger: value, every: value === "timer" ? (typeof skill.every === "number" && skill.every >= 1 ? skill.every : 5) : 0 })} options={MOB_TRIGGERS.map((one) => ({ value: one, label: triggerLabel(one), hint: one }))} />
        </label>
        {trigger === "timer" && <NumberField value={skill.every} label={tr("Every (seconds)")} step={0.5} least={1} most={86400} placeholder="5" onChange={(next) => onChange(put(skill, "every", next ?? 1))} />}
      </div>
      <div className="studio-fields-row">
        <NumberField value={skill.cooldown} label={tr("Cooldown (seconds)")} step={0.5} least={0} most={86400} placeholder="0" onChange={(next) => onChange(put(skill, "cooldown", next ?? 0))} />
        <NumberField value={skill.chance} label={tr("Chance (%)")} least={0} most={100} placeholder="100" onChange={(next) => onChange(put(skill, "chance", next ?? 100))} />
      </div>
      <div className="studio-fields-row">
        <label className="field studio-field">
          <span>{tr("Acts on")}</span>
          <Dropdown value={targets} onChange={(value) => onChange({ ...skill, targets: value })} options={MOB_TARGETS.map((one) => ({ value: one, label: targetsLabel(one), hint: one }))} />
          {targets === "trigger" && <small>{triggerWho(trigger)}</small>}
        </label>
        {targets === "nearby" && <NumberField value={skill.radius} label={tr("Radius (blocks)")} least={1} most={64} placeholder="8" onChange={(next) => onChange(put(skill, "radius", next ?? 8))} />}
      </div>
      <StepList conditions label={tr("Only if")} empty={tr("No conditions: it always runs.")} value={skill.conditions} names={names} onChange={(conditions) => onChange({ ...skill, conditions })} />
      <StepList label={tr("Then, for each one")} empty={tr("Nothing yet. Add an action.")} value={skill.actions} names={names} onChange={(actions) => onChange({ ...skill, actions })} />
    </div>
  );
}

export function MobFields({ data, onChange, names, serverId }: { data: StudioData; onChange: (next: StudioData) => void; names: StudioNames; serverId?: string }) {
  const equipment = objectOf(data.equipment);
  const ai = objectOf(data.ai);
  const spawn = objectOf(data.spawn);
  const spawning = spawn.enabled === true;
  const model = textOf(data, "model").trim();
  const loot = textOf(data, "loot");
  const skills = listOf(data.skills);
  const setSkills = (next: StudioData[]) => onChange({ ...data, skills: next });
  const setSlot = (slot: string, value: string) => {
    const next = put(equipment, slot, value.trim() || undefined);
    onChange(put(data, "equipment", Object.keys(next).length ? next : undefined));
  };
  const setSpawn = (key: string, value: unknown) => onChange({ ...data, spawn: put(spawn, key, value) });
  const lootOptions = [{ value: "", label: tr("None") }, ...names.loot.map((one) => ({ value: one, label: one }))];
  if (loot && !names.loot.includes(loot)) lootOptions.push({ value: loot, label: tr("{0} (not found)", [loot]) });
  const modelOptions = [{ value: "", label: tr("None: the base mob's own look") }, ...names.model.map((one) => ({ value: one, label: one }))];
  if (model && !names.model.includes(model)) modelOptions.push({ value: model, label: tr("{0} (not found)", [model]) });

  return (
    <>
      <div className="studio-fields-row">
        <label className="field studio-field">
          <span>{tr("Base mob")}</span>
          <input list="studio-mob-bases" value={textOf(data, "base")} placeholder="zombie" onChange={(event) => onChange({ ...data, base: event.target.value.trim().toLowerCase() })} spellCheck={false} />
          <datalist id="studio-mob-bases">
            {MOB_BASES.map((one) => (
              <option key={one} value={one} />
            ))}
          </datalist>
          <small>{tr("The game mob it starts from. Its own AI and sounds stay unless you turn them off.")}</small>
        </label>
        <label className="field studio-field">
          <span>{tr("Name")}</span>
          <input value={textOf(data, "name")} placeholder="<red>Ruby Golem" onChange={(event) => onChange(put(data, "name", event.target.value))} spellCheck={false} />
          <small>{tr("MiniMessage. Empty = no name.")}</small>
        </label>
      </div>
      <SwitchRow label={tr("Name always shows")} checked={data.showName === true} onChange={(on) => onChange(put(data, "showName", on || undefined))} />

      <Section title={tr("Stats")} hint={tr("Empty = the base mob's own.")}>
        <div className="studio-fields-row four">
          <NumberField value={data.health} label={tr("Health")} step={0.5} least={1} most={1024} placeholder="20" onChange={(next) => onChange(put(data, "health", next))} />
          <NumberField value={data.damage} label={tr("Damage")} step={0.5} least={0} most={2048} placeholder="3" onChange={(next) => onChange(put(data, "damage", next))} />
          <NumberField value={data.speed} label={tr("Speed")} step={0.01} least={0} most={1} placeholder="0.23" onChange={(next) => onChange(put(data, "speed", next))} />
          <NumberField value={data.armor} label={tr("Armor")} step={0.5} least={0} most={30} placeholder="0" onChange={(next) => onChange(put(data, "armor", next))} />
        </div>
        <div className="studio-fields-row four">
          <NumberField value={data.toughness} label={tr("Toughness")} step={0.5} least={0} most={20} placeholder="0" onChange={(next) => onChange(put(data, "toughness", next))} />
          <NumberField value={data.knockbackResistance} label={tr("Knockback resistance")} step={0.05} least={0} most={1} placeholder="0" onChange={(next) => onChange(put(data, "knockbackResistance", next))} />
          <NumberField value={data.followRange} label={tr("Follow range")} least={0} most={2048} placeholder="35" onChange={(next) => onChange(put(data, "followRange", next))} />
          <NumberField value={data.size} label={tr("Size")} step={0.05} least={0.0625} most={16} placeholder="1" onChange={(next) => onChange(put(data, "size", next))} />
        </div>
        <p className="studio-placeholders">{tr("Health 2 = one heart. Size needs 1.20.5 or later.")}</p>
      </Section>

      <Section title={tr("Gear")} hint={tr("Item names like diamond_helmet, or studio:<name> for a studio item.")}>
        <div className="studio-fields-row three">
          {MOB_SLOTS.map((slot) => (
            <label key={slot} className="field studio-field">
              <span>{slotLabel(slot)}</span>
              <input list="studio-mob-items" value={textOf(equipment, slot)} placeholder={tr("Nothing")} onChange={(event) => setSlot(slot, event.target.value)} spellCheck={false} />
            </label>
          ))}
        </div>
        <datalist id="studio-mob-items">
          {names.item.map((one) => (
            <option key={one} value={`studio:${one}`} />
          ))}
        </datalist>
        <SwitchRow label={tr("Drops its gear when it dies")} checked={data.dropEquipment === true} onChange={(on) => onChange(put(data, "dropEquipment", on || undefined))} />
      </Section>

      <Section title={tr("Model")}>
        <label className="field studio-field">
          <span>{tr("Model")}</span>
          <Dropdown value={model} options={modelOptions} search={modelOptions.length > 8 ? tr("Find a model") : undefined} onChange={(value) => onChange(put(data, "model", value || undefined))} />
          <small>{model ? tr("The base mob turns invisible and the model rides it, playing its idle, walk, attack, hurt, death and spawn animations.") : tr("A studio model (Models) to show instead of the base mob.")}</small>
        </label>
        {model && (
          <InSide title={tr("3D preview")}>
            <ModelPreview serverId={serverId} model={model} />
          </InSide>
        )}
      </Section>

      <Section title={tr("Drops")}>
        <div className="studio-fields-row">
          <label className="field studio-field">
            <span>{tr("Loot table")}</span>
            <Dropdown value={loot} options={lootOptions} onChange={(value) => onChange(put(data, "loot", value || undefined))} />
          </label>
          <NumberField value={data.exp} label={tr("Experience")} least={0} most={100000} placeholder="5" onChange={(next) => onChange(put(data, "exp", next))} />
        </div>
        <SwitchRow label={tr("Also drops what the base mob drops")} checked={data.vanillaDrops === true} onChange={(on) => onChange(put(data, "vanillaDrops", on || undefined))} />
      </Section>

      <Section title={tr("Behaviour")}>
        {MOB_AI.map((goal) => (
          <SwitchRow key={goal} label={aiLabel(goal)} checked={typeof ai[goal] === "boolean" ? (ai[goal] as boolean) : AI_START[goal]} onChange={(on) => onChange({ ...data, ai: { ...AI_START, ...ai, [goal]: on } })} />
        ))}
        <SwitchRow label={tr("Silent")} checked={data.silent === true} onChange={(on) => onChange(put(data, "silent", on || undefined))} />
        <SwitchRow label={tr("Glowing")} checked={data.glowing === true} onChange={(on) => onChange(put(data, "glowing", on || undefined))} />
        <SwitchRow label={tr("Never despawns")} checked={data.persistent === true} onChange={(on) => onChange(put(data, "persistent", on || undefined))} />
      </Section>

      <div className={`studio-item-part ${spawning ? "on" : ""}`}>
        <SwitchRow label={tr("Spawns by itself")} checked={spawning} onChange={(on) => setSpawn("enabled", on)} />
        {spawning && (
          <div className="studio-item-part-body">
            <div className="studio-fields-row">
              <label className="field studio-field">
                <span>{tr("Worlds")}</span>
                <input value={listText(spawn, "worlds")} placeholder={tr("Every world")} onChange={(event) => setSpawn("worlds", listFrom(event.target.value))} spellCheck={false} />
              </label>
              <label className="field studio-field">
                <span>{tr("Biomes")}</span>
                <input value={listText(spawn, "biomes")} placeholder={tr("Every biome")} onChange={(event) => setSpawn("biomes", listFrom(event.target.value))} spellCheck={false} />
              </label>
            </div>
            <div className="studio-fields-row four">
              <NumberField value={spawn.every} label={tr("Every (seconds)")} least={1} most={86400} placeholder="30" onChange={(next) => setSpawn("every", next ?? 30)} />
              <NumberField value={spawn.chance} label={tr("Chance (%)")} least={0} most={100} placeholder="50" onChange={(next) => setSpawn("chance", next ?? 50)} />
              <NumberField value={spawn.max} label={tr("At most alive")} least={1} most={1000} placeholder="10" onChange={(next) => setSpawn("max", next ?? 10)} />
              <NumberField value={spawn.radius} label={tr("Up to (blocks away)")} least={16} most={128} placeholder="32" onChange={(next) => setSpawn("radius", next ?? 32)} />
            </div>
            <div className="studio-fields-row">
              <NumberField value={spawn.lightMin} label={tr("Light from")} least={0} most={15} placeholder="0" onChange={(next) => setSpawn("lightMin", next ?? 0)} />
              <NumberField value={spawn.lightMax} label={tr("Light up to")} least={0} most={15} placeholder="15" onChange={(next) => setSpawn("lightMax", next ?? 15)} />
            </div>
            <p className="studio-placeholders">{tr("Every few seconds, near each online player in those worlds, it may try one spot on the ground 16 or more blocks away. Comma-separated names; empty = any.")}</p>
          </div>
        )}
      </div>

      <div className="studio-steps studio-mob-skills">
        <div className="studio-steps-head">
          <span>{tr("Skills")}</span>
          <button type="button" className="button ghost small" onClick={() => setSkills([...skills, { trigger: "attack", every: 0, cooldown: 2, chance: 100, targets: "trigger", radius: 8, conditions: [], actions: [] }])}>
            <Icon name="plus" />
            {tr("Add skill")}
          </button>
        </div>
        {skills.length === 0 && <p className="studio-steps-empty">{tr("No skills: it fights like the base mob.")}</p>}
        {skills.map((skill, at) => (
          <SkillCard
            key={at}
            skill={skill}
            at={at}
            count={skills.length}
            names={names}
            onChange={(next) => setSkills(skills.map((one, index) => (index === at ? next : one)))}
            onMove={(to) => setSkills(move(skills, at, to))}
            onRemove={() => setSkills(skills.filter((_, index) => index !== at))}
          />
        ))}
      </div>
      <p className="studio-placeholders">{tr("In skills: <mob> is its name, <mob_id> its id, <target> who it acts on. Actions run once for each one it acts on; a model animation with no model plays on this mob's model.")}</p>
    </>
  );
}

import { useEffect, useMemo, useState } from "react";
import type { StudioAsset } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Switch } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { StepList } from "./StudioActions";
import type { StudioNames } from "./studioKinds";
import type { StudioData } from "./StudioPreview";
import { InSide, Studio3dStage } from "./StudioPack3d";
import "./StudioModels.css";

/** The roles other kinds ask a model for (`studio_format::model::ROLES`). */
const ROLES = ["idle", "walk", "run", "attack", "hurt", "death", "spawn", "interact"] as const;
const VISIBILITY = ["everyone", "permission"] as const;

const object = (value: unknown): StudioData => (value && typeof value === "object" && !Array.isArray(value) ? (value as StudioData) : {});
const text = (data: StudioData, key: string) => (typeof data[key] === "string" ? (data[key] as string) : "");

type BbGroup = { uuid?: string; name?: string; children?: (string | BbGroup)[] };
type BbAnimation = { name?: string; loop?: string; length?: number; animators?: Record<string, { keyframes?: { time?: number }[] }> };
type ModelInfo = { bones: string[]; animations: { name: string; length: number; loop: string }[] };

/** Bone and animation names in a `.bbmodel`, read without three.js (the 3D view loads only when shown). */
function modelInfo(source: string): ModelInfo | null {
  let model: { outliner?: (string | BbGroup)[]; groups?: BbGroup[]; animations?: BbAnimation[] };
  try {
    model = JSON.parse(source) as typeof model;
  } catch {
    return null;
  }
  const bones: string[] = [];
  const byId = new Map((model.groups ?? []).filter((one) => one.uuid).map((one) => [one.uuid as string, one]));
  const walk = (items: (string | BbGroup)[]) => {
    for (const item of items) {
      if (typeof item === "string") continue;
      const full = item.uuid ? byId.get(item.uuid) : undefined;
      const name = full?.name ?? item.name;
      if (name) bones.push(name);
      walk(item.children ?? full?.children ?? []);
    }
  };
  walk(model.outliner ?? []);
  const lengthOf = (one: BbAnimation) => {
    if ((one.length ?? 0) > 0) return one.length as number;
    let most = 0;
    for (const animator of Object.values(one.animators ?? {})) for (const key of animator.keyframes ?? []) most = Math.max(most, key.time ?? 0);
    return most;
  };
  return { bones, animations: (model.animations ?? []).map((one) => ({ name: one.name ?? "", length: lengthOf(one), loop: one.loop ?? "once" })) };
}

function prefersReducedMotion() {
  return typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
}

function Decimal({ data, name, label, fallback, least, most, step = 0.1, hint, onChange }: { data: StudioData; name: string; label: string; fallback: number; least: number; most: number; step?: number; hint?: string; onChange: (next: StudioData) => void }) {
  return (
    <label className="field studio-field studio-number">
      <span>{label}</span>
      <input
        type="number"
        min={least}
        max={most}
        step={step}
        value={typeof data[name] === "number" ? (data[name] as number) : fallback}
        onChange={(event) => {
          const value = Number(event.target.value);
          if (event.target.value !== "" && Number.isFinite(value)) onChange({ ...data, [name]: Math.min(most, Math.max(least, value)) });
        }}
      />
      {hint && <small>{hint}</small>}
    </label>
  );
}

/** The model's rig problems from the last pack build (`studio/pack/models/<id>.json`). */
function useRigProblems(serverId: string | undefined, root: string | null, id: string) {
  const [problems, setProblems] = useState<string[] | null>(null);
  useEffect(() => {
    if (!serverId || !root || !id) return;
    let live = true;
    commands.readEditableFile(serverId, `${root}/pack/models/${id}.json`).then(
      (raw) => {
        try {
          const read = JSON.parse(raw) as { problems?: unknown };
          if (live) setProblems(Array.isArray(read.problems) ? read.problems.filter((one): one is string => typeof one === "string") : []);
        } catch {
          if (live) setProblems(null);
        }
      },
      () => live && setProblems(null),
    );
    return () => {
      live = false;
    };
  }, [serverId, root, id]);
  return problems;
}

/**
 * Studio maker M4: a Blockbench model (`model` file) shown in the game as one item display per bone. The pack build
 * bakes its animations; this form picks the model, names which animation plays for each role, and where (if
 * anywhere) it stands by itself.
 */
export function ModelFields({ id: given, data, onChange, serverId, names }: { id?: string; data: StudioData; onChange: (next: StudioData) => void; serverId?: string; names?: StudioNames }) {
  const id = given ?? text(data, "id");
  const [assets, setAssets] = useState<StudioAsset[]>([]);
  const [root, setRoot] = useState<string | null>(null);
  const [source, setSource] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [watching, setWatching] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const still = prefersReducedMotion();
  const model = text(data, "model").trim().replace(/\.bbmodel$/, "");
  const rigProblems = useRigProblems(serverId, root, id);

  useEffect(() => {
    if (!serverId) return;
    let live = true;
    commands.studioAssetList(serverId).then(
      (list) => live && setAssets(list.assets.filter((one) => one.folder === "models" && one.file.endsWith(".bbmodel"))),
      () => undefined,
    );
    commands.studioOverview(serverId).then(
      (overview) => live && setRoot(overview.root),
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [serverId]);

  useEffect(() => {
    setSource(null);
    setProblem(null);
    setWatching(null);
    if (!serverId || !root || !model) return;
    let live = true;
    commands.readEditableFile(serverId, `${root}/assets/models/${model}.bbmodel`).then(
      (raw) => live && setSource(raw),
      (cause) => live && setProblem(String(cause)),
    );
    return () => {
      live = false;
    };
  }, [serverId, root, model]);

  const info = useMemo(() => (source ? modelInfo(source) : null), [source]);
  const animations = info?.animations ?? [];
  const shown = animations.find((one) => one.name === watching) ?? null;
  const roles = object(data.animations);
  const hitbox = object(data.hitbox);
  const place = data.place && typeof data.place === "object" ? object(data.place) : null;
  const visibility = VISIBILITY.includes(text(data, "visibility") as (typeof VISIBILITY)[number]) ? text(data, "visibility") : "everyone";

  const set = (key: string, value: unknown) => {
    const next = { ...data };
    if (value === undefined || value === "" || (typeof value === "object" && value !== null && !Array.isArray(value) && Object.keys(value).length === 0)) delete next[key];
    else next[key] = value;
    onChange(next);
  };
  const setRole = (role: string, name: string) => {
    const next = { ...roles };
    if (name) next[role] = name;
    else delete next[role];
    set("animations", next);
  };
  const animationOptions = [{ value: "", label: tr("None") }, ...animations.map((one) => ({ value: one.name, label: one.name }))];

  return (
    <>
      <label className="field studio-field">
        <span>{tr("Model")}</span>
        <Dropdown
          value={model}
          options={assets.map((one) => ({ value: one.name, label: one.file }))}
          placeholder={tr("Pick a Blockbench model")}
          search={tr("Find a model")}
          onChange={(value) => set("model", value)}
        />
        <small>{tr("A .bbmodel from the studio's models (Assets). Each bone becomes one display entity.")}</small>
      </label>

      {problem && <p className="studio-model-note">{problem}</p>}
      {source && (
        <InSide title={tr("3D preview")}>
        <div className="studio-model-preview">
          <Studio3dStage source={source} animation={watching} time={still || !playing ? time : null} />
          {animations.length > 0 && (
            <div className="studio-model-player">
              <Dropdown
                value={watching ?? ""}
                options={[{ value: "", label: tr("Rest pose") }, ...animations.map((one) => ({ value: one.name, label: one.name }))]}
                onChange={(value) => {
                  setWatching(value || null);
                  setTime(0);
                  setPlaying(!still && !!value);
                }}
              />
              {shown && !still && (
                <button type="button" className="button ghost small" onClick={() => setPlaying(!playing)}>
                  {playing ? tr("Pause") : tr("Play")}
                </button>
              )}
              {shown && (!playing || still) && (
                <input
                  type="range"
                  className="studio-model-time"
                  min={0}
                  max={Math.max(shown.length, 0.05)}
                  step={0.05}
                  value={time}
                  aria-label={tr("Time")}
                  onChange={(event) => setTime(Number(event.target.value))}
                />
              )}
              {shown && <small>{tr("{0} s · {1}", [shown.length.toFixed(2), shown.loop])}</small>}
            </div>
          )}
          {info && (
            <div className="studio-model-bones">
              <small>{tr("{0, plural, one {# bone} other {# bones}}", [info.bones.length])}</small>
              <div className="studio-model-chips">
                {info.bones.map((bone, at) => (
                  <code key={`${bone}-${at}`}>{bone}</code>
                ))}
              </div>
            </div>
          )}
        </div>
        </InSide>
      )}

      <Decimal data={data} name="scale" label={tr("Scale")} fallback={1} least={0.05} most={16} hint={tr("1 = the size it has in Blockbench (16 pixels a block).")} onChange={onChange} />

      <div className="studio-model-roles">
        <span className="studio-model-label">{tr("Animations")}</span>
        <small>{tr("Which animation plays for each role. NPCs, mobs and furniture ask for these; idle plays by itself.")}</small>
        <div className="studio-fields-row">
          {ROLES.map((role) => (
            <label key={role} className="field studio-field">
              <span>{role}</span>
              <Dropdown value={text(roles, role)} options={animationOptions} onChange={(value) => setRole(role, value)} disabled={animations.length === 0} />
            </label>
          ))}
        </div>
      </div>

      <div className="studio-fields-row">
        <Decimal data={hitbox} name="width" label={tr("Hitbox width")} fallback={0} least={0} most={16} hint={tr("Blocks. 0 = no hitbox (it can't be clicked).")} onChange={(next) => set("hitbox", next)} />
        <Decimal data={hitbox} name="height" label={tr("Hitbox height")} fallback={0} least={0} most={16} onChange={(next) => set("hitbox", next)} />
      </div>

      <label className="field studio-field">
        <span>{tr("Seat")}</span>
        <Dropdown
          value={text(data, "seat")}
          options={[{ value: "", label: tr("None") }, ...(info?.bones ?? []).map((one) => ({ value: one, label: one }))]}
          onChange={(value) => set("seat", value)}
          disabled={!info}
        />
        <small>{tr("A bone players sit on when they right-click the hitbox.")}</small>
      </label>

      <div className="studio-fields-row">
        <label className="field studio-field">
          <span>{tr("Who sees it")}</span>
          <Dropdown value={visibility} options={VISIBILITY.map((one) => ({ value: one, label: one }))} onChange={(value) => set("visibility", value === "everyone" ? undefined : value)} />
        </label>
        {visibility === "permission" && (
          <label className="field studio-field">
            <span>{tr("Permission")}</span>
            <input value={text(data, "permission")} placeholder="server.vip" onChange={(event) => set("permission", event.target.value.trim())} spellCheck={false} />
          </label>
        )}
      </div>

      <label className="studio-toggle">
        <span>{tr("Stands at a spot by itself")}</span>
        <Switch checked={!!place} onChange={(event) => set("place", event.target.checked ? { world: "world", x: 0.5, y: 64, z: 0.5, yaw: 0 } : undefined)} />
      </label>
      {place && (
        <>
          <label className="field studio-field">
            <span>{tr("World")}</span>
            <input value={text(place, "world")} placeholder="world" onChange={(event) => set("place", { ...place, world: event.target.value.trim() })} spellCheck={false} />
          </label>
          <div className="studio-fields-row four">
            <Decimal data={place} name="x" label="X" fallback={0.5} least={-30000000} most={30000000} onChange={(next) => set("place", next)} />
            <Decimal data={place} name="y" label="Y" fallback={64} least={-2048} most={2048} onChange={(next) => set("place", next)} />
            <Decimal data={place} name="z" label="Z" fallback={0.5} least={-30000000} most={30000000} onChange={(next) => set("place", next)} />
            <Decimal data={place} name="yaw" label={tr("Facing")} fallback={0} least={-180} most={180} step={1} onChange={(next) => set("place", next)} />
          </div>
        </>
      )}

      <StepList label={tr("When right-clicked")} empty={tr("Nothing yet. Add an action.")} value={data.clickActions} names={names} onChange={(next) => set("clickActions", next)} />

      {rigProblems && rigProblems.length > 0 && (
        <ul className="studio-model-problems">
          {rigProblems.map((one) => (
            <li key={one}>{one}</li>
          ))}
        </ul>
      )}
      <p className="studio-placeholders">{tr("Models need Minecraft 1.21.4 or later. Saving rebuilds the studio pack; players see the model once the pack is sent (Assets).")}</p>
    </>
  );
}

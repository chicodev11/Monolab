import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import type { WorldgenPreview } from "../../lib/bindings";
import { tr } from "../../lib/i18n";
import { commands } from "../../lib/ipc";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { NumberField, SwitchRow, put } from "./StudioItemFields";
import { textOf, type StudioData } from "./StudioPreview";
import { objectOf } from "./studioKinds";
import { shapeFromLayers, WorldgenNodes } from "./StudioWorldgenNodes";
import "./StudioWorldgen.css";

/**
 * A custom world's generator (`"type": "custom"`): the `"generator"` block of a world file, the same field names
 * as `crates/studio-format/src/worldgen.rs`. The map on the right is drawn by that same code, so it shows what the
 * server makes.
 */

export const WORLDGEN_NOISES = ["perlin", "ridged", "billow", "cellular"];
export const WORLDGEN_WARPS = ["none", "spiral", "domain"];
export const WORLDGEN_CAVES = ["spaghetti", "cheese"];
export const WORLDGEN_FEATURES = ["ore", "tree", "boulder", "patch", "structure"];
export const WORLDGEN_MODES = ["custom", "vanilla"];
export const WORLDGEN_TERRAINS = ["normal", "amplified", "large_biomes"];

const noiseLabel = (one: string) => (({ perlin: tr("Rolling hills"), ridged: tr("Sharp ridges"), billow: tr("Round bumps"), cellular: tr("Cells and craters") }) as Record<string, string>)[one] ?? one;
const warpLabel = (one: string) => (({ none: tr("No twist"), spiral: tr("Spiral around a point"), domain: tr("Wavy, like stirred paint") }) as Record<string, string>)[one] ?? one;
const caveLabel = (one: string) => (({ spaghetti: tr("Long tunnels"), cheese: tr("Big open caves") }) as Record<string, string>)[one] ?? one;
const terrainLabel = (one: string) => (({ normal: tr("Normal"), amplified: tr("Amplified"), large_biomes: tr("Large biomes") }) as Record<string, string>)[one] ?? one;
const featureLabel = (one: string) => (({ ore: tr("Ore"), tree: tr("Tree"), boulder: tr("Boulder"), patch: tr("Plants on the ground"), structure: tr("Structure") }) as Record<string, string>)[one] ?? one;

function listIn(data: StudioData, key: string): StudioData[] {
  return Array.isArray(data[key]) ? (data[key] as unknown[]).map((one) => objectOf(one)) : [];
}

function pairOf(data: StudioData, key: string, fallback: [number, number]): [number, number] {
  const value = data[key];
  return Array.isArray(value) && value.length === 2 && value.every((one) => typeof one === "number") ? [value[0] as number, value[1] as number] : fallback;
}

function wordsOf(data: StudioData, key: string): string {
  return Array.isArray(data[key]) ? (data[key] as unknown[]).filter((one) => typeof one === "string").join(", ") : "";
}

function words(text: string): string[] | undefined {
  const list = text
    .split(",")
    .map((one) => one.trim())
    .filter(Boolean);
  return list.length ? list : undefined;
}

/** Ready-made generators to start from. */
function presets(): { id: string; label: string; generator: StudioData }[] {
  const common = {
    biomes: [
      { biome: "desert", temperature: [0.62, 1], humidity: [0, 0.45] },
      { biome: "snowy_plains", temperature: [0, 0.3] },
      { biome: "forest", humidity: [0.55, 1] },
    ],
    surface: [
      { biome: "desert", top: "sand", under: "sandstone", depth: 4, underwater: "sand" },
      { biome: "snowy_plains", top: "snow_block", under: "dirt", depth: 3 },
      { top: "grass_block", under: "dirt", depth: 3, underwater: "gravel" },
    ],
    caves: [{ type: "spaghetti", scale: 64, width: 0.07 }],
    features: [
      { type: "ore", block: "coal_ore", y: [0, 120], per_chunk: 10, size: 8 },
      { type: "ore", block: "iron_ore", y: [-48, 64], per_chunk: 8, size: 6 },
      { type: "ore", block: "diamond_ore", y: [-64, -40], per_chunk: 1, size: 4 },
      { type: "tree", trunk: "oak_log", leaves: "oak_leaves", per_chunk: 3, height: [4, 6], biomes: ["forest", "plains"] },
      { type: "patch", block: "short_grass", per_chunk: 10, biomes: ["forest", "plains"] },
    ],
  };
  return [
    {
      id: "spiral",
      label: tr("Spiral"),
      generator: { ...common, warp: { type: "spiral", strength: 2.5, x: 0, z: 0 }, sea: 63, base: 66, height: 34, layers: [{ noise: "ridged", scale: 220, octaves: 4, amount: 0.8 }, { noise: "perlin", scale: 60, octaves: 3, amount: 0.25 }] },
    },
    {
      id: "islands",
      label: tr("Islands"),
      generator: { ...common, warp: { type: "domain", scale: 200, amount: 60 }, sea: 63, base: 52, height: 30, layers: [{ noise: "cellular", scale: 260, octaves: 2, amount: 1 }, { noise: "perlin", scale: 48, octaves: 3, amount: 0.25 }] },
    },
    {
      id: "continents",
      label: tr("Continents and seas"),
      generator: { ...common, warp: { type: "none" }, sea: 63, base: 64, height: 46, layers: [{ noise: "perlin", scale: 900, octaves: 6, amount: 1 }, { noise: "ridged", scale: 300, octaves: 4, amount: 0.35 }] },
    },
    {
      id: "canyons",
      label: tr("Mesas and canyons"),
      generator: {
        ...common,
        warp: { type: "none" },
        sea: 40,
        base: 90,
        height: 40,
        overhangs: 0.6,
        layers: [{ noise: "billow", scale: 240, octaves: 4, amount: 1 }],
        biomes: [{ biome: "badlands" }],
        surface: [{ top: "red_sand", under: "terracotta", depth: 6, underwater: "red_sand" }],
      },
    },
  ];
}

export function defaultGenerator(): StudioData {
  return presets()[0].generator;
}

/** Map colors for the usual top blocks, close to how Minecraft's maps draw them; others get a grey. */
const BLOCK_COLORS: Record<string, [number, number, number]> = {
  grass_block: [127, 178, 56],
  water: [64, 64, 255],
  lava: [255, 0, 0],
  sand: [247, 233, 163],
  red_sand: [216, 127, 51],
  sandstone: [247, 233, 163],
  snow_block: [255, 255, 255],
  ice: [160, 160, 255],
  stone: [112, 112, 112],
  gravel: [130, 124, 120],
  dirt: [151, 109, 77],
  coarse_dirt: [151, 109, 77],
  podzol: [129, 86, 49],
  mycelium: [127, 63, 178],
  terracotta: [209, 177, 161],
  netherrack: [112, 2, 0],
  end_stone: [229, 229, 153],
  moss_block: [102, 127, 51],
  clay: [164, 168, 184],
};

function colorOf(block: string): [number, number, number] {
  const short = block.replace(/^minecraft:/, "");
  return BLOCK_COLORS[short] ?? [128, 128, 128];
}

/** A steady colour per biome name for the biome map; the server's own biomes (empty name) are dark grey. */
export function biomeColor(name: string): [number, number, number] {
  if (!name) return [58, 58, 58];
  let hash = 0;
  for (const letter of name) hash = (hash * 31 + letter.charCodeAt(0)) >>> 0;
  const hue = (hash % 360) / 360;
  const [s, l] = [0.45, 0.55];
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const channel = (t: number) => {
    const u = t < 0 ? t + 1 : t > 1 ? t - 1 : t;
    const v = u < 1 / 6 ? p + (q - p) * 6 * u : u < 1 / 2 ? q : u < 2 / 3 ? p + (q - p) * (2 / 3 - u) * 6 : p;
    return Math.round(v * 255);
  };
  return [channel(hue + 1 / 3), channel(hue), channel(hue - 1 / 3)];
}

function draw(canvas: HTMLCanvasElement, map: WorldgenPreview, byBiome: boolean) {
  const { size, heights, tops, blocks, sea, biomes, biomeNames } = map;
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext("2d");
  if (!context) return;
  const image = context.createImageData(size, size);
  for (let row = 0; row < size; row += 1) {
    for (let col = 0; col < size; col += 1) {
      const at = row * size + col;
      const block = blocks[tops[at]] ?? "";
      let [r, g, b] = byBiome ? biomeColor(biomeNames[biomes[at]] ?? "") : colorOf(block);
      const height = heights[at];
      if (byBiome && map.vanilla) {
        // Only biomes are known on the server's own ground: no shading.
      } else if (height < sea && /water|lava/.test(block)) {
        const deep = Math.min(1, (sea - height) / 24);
        const shade = 1 - deep * 0.45;
        r *= shade;
        g *= shade;
        b *= shade;
      } else {
        const north = row > 0 ? heights[at - size] : height;
        const shade = height > north ? 1.08 : height < north ? 0.82 : 0.95;
        r *= shade;
        g *= shade;
        b *= shade;
      }
      const pixel = at * 4;
      image.data[pixel] = Math.min(255, r);
      image.data[pixel + 1] = Math.min(255, g);
      image.data[pixel + 2] = Math.min(255, b);
      image.data[pixel + 3] = 255;
    }
  }
  context.putImageData(image, 0, 0);
}

const STEPS = [1, 2, 4, 8, 16];

/** The top-down map, drawn by the app from the generator text (no server needed). */
function WorldgenMap({ generator, seed, world }: { generator: StudioData; seed: string; world: string }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [center, setCenter] = useState<[number, number]>([0, 0]);
  const [step, setStep] = useState(4);
  const [map, setMap] = useState<WorldgenPreview | null>(null);
  const [failed, setFailed] = useState("");
  const [biomeView, setBiomeView] = useState(false);
  const byBiome = biomeView || map?.vanilla === true;
  const text = JSON.stringify(generator);
  const size = 160;
  useEffect(() => {
    let gone = false;
    const timer = window.setTimeout(() => {
      const half = (size * step) / 2;
      commands
        .studioWorldgenPreview(text, seed, world, Math.round(center[0] - half), Math.round(center[1] - half), size, step)
        .then((next) => {
          if (gone) return;
          setMap(next);
          setFailed("");
        })
        .catch((error: unknown) => !gone && setFailed(String(error)));
    }, 250);
    return () => {
      gone = true;
      window.clearTimeout(timer);
    };
  }, [text, seed, world, center, step]);
  useEffect(() => {
    if (map && canvas.current) draw(canvas.current, map, byBiome);
  }, [map, byBiome]);
  const legend = byBiome && map ? map.biomeNames.slice(0, 10) : [];
  const move = (dx: number, dz: number) => setCenter(([x, z]) => [x + dx * size * step * 0.25, z + dz * size * step * 0.25]);
  const across = size * step;
  return (
    <div className="studio-worldgen-map">
      <div className="studio-worldgen-canvas">
        <canvas ref={canvas} aria-label={tr("Map of the generated world from above")} />
        <span className="studio-worldgen-mark" aria-hidden="true" />
      </div>
      <div className="studio-worldgen-tools">
        <button type="button" className="icon-button" aria-label={tr("Move west")} data-tip={tr("Move west")} onClick={() => move(-1, 0)}>
          <Icon name="arrow-left" />
        </button>
        <button type="button" className="icon-button" aria-label={tr("Move north")} data-tip={tr("Move north")} onClick={() => move(0, -1)}>
          <Icon name="arrow-up" />
        </button>
        <button type="button" className="icon-button" aria-label={tr("Move south")} data-tip={tr("Move south")} onClick={() => move(0, 1)}>
          <Icon name="arrow-down" />
        </button>
        <button type="button" className="icon-button" aria-label={tr("Move east")} data-tip={tr("Move east")} onClick={() => move(1, 0)}>
          <Icon name="arrow" />
        </button>
        <button type="button" className="icon-button" aria-label={tr("Zoom in")} data-tip={tr("Zoom in")} disabled={step === STEPS[0]} onClick={() => setStep(STEPS[Math.max(0, STEPS.indexOf(step) - 1)])}>
          <Icon name="plus" />
        </button>
        <button type="button" className="icon-button" aria-label={tr("Zoom out")} data-tip={tr("Zoom out")} disabled={step === STEPS[STEPS.length - 1]} onClick={() => setStep(STEPS[Math.min(STEPS.length - 1, STEPS.indexOf(step) + 1)])}>
          <Icon name="minus" />
        </button>
        <button type="button" className="button ghost small" onClick={() => setCenter([0, 0])}>
          {tr("Back to 0, 0")}
        </button>
      </div>
      {!map?.vanilla && (
        <div className="wb-seg slide studio-worldgen-view" role="tablist" aria-label={tr("Map shows")} style={{ "--at": biomeView ? 1 : 0 } as CSSProperties}>
          <button type="button" role="tab" aria-selected={!biomeView} className={biomeView ? "" : "on"} onClick={() => setBiomeView(false)}>
            {tr("Ground")}
          </button>
          <button type="button" role="tab" aria-selected={biomeView} className={biomeView ? "on" : ""} onClick={() => setBiomeView(true)}>
            {tr("Biomes")}
          </button>
        </div>
      )}
      {legend.length > 0 && (
        <ul className="studio-worldgen-legend">
          {legend.map((name) => {
            const [r, g, b] = biomeColor(name);
            return (
              <li key={name || "own"}>
                <span className="studio-worldgen-swatch" style={{ background: `rgb(${r}, ${g}, ${b})` }} aria-hidden="true" />
                {name || tr("The server's own")}
              </li>
            );
          })}
        </ul>
      )}
      <small className="muted">
        {map?.vanilla
          ? tr("{0} blocks across, centered on {1}, {2}. Where your biome rules hold; the ground is the server's own.", [String(across), String(Math.round(center[0])), String(Math.round(center[1]))])
          : tr("{0} blocks across, centered on {1}, {2}. The ground before caves and trees.", [String(across), String(Math.round(center[0])), String(Math.round(center[1]))])}
      </small>
      {failed && <p className="studio-worldgen-problem">{failed}</p>}
      {map?.problems.map((one) => (
        <p key={one} className="studio-worldgen-problem">
          {one}
        </p>
      ))}
    </div>
  );
}

function RowCard({ title, at, count, onMove, onRemove, children }: { title: string; at: number; count: number; onMove: (to: number) => void; onRemove: () => void; children: ReactNode }) {
  return (
    <div className="studio-step">
      <div className="studio-step-head">
        <span className="studio-step-number">{at + 1}</span>
        <strong>{title}</strong>
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
      <div className="studio-step-fields">{children}</div>
    </div>
  );
}

function moved<T>(list: T[], from: number, to: number): T[] {
  const next = [...list];
  const [one] = next.splice(from, 1);
  next.splice(to, 0, one);
  return next;
}

/** A list of rows in the generator (layers, biomes, …) with add, move and remove. */
function Rows({ generator, name, label, empty, fresh, title, onChange, children }: { generator: StudioData; name: string; label: string; empty: string; fresh: StudioData; title: (row: StudioData) => string; onChange: (next: StudioData) => void; children: (row: StudioData, set: (key: string, value: unknown) => void) => ReactNode }) {
  const rows = listIn(generator, name);
  const setRows = (next: StudioData[]) => onChange({ ...generator, [name]: next });
  return (
    <div className="studio-steps">
      <div className="studio-steps-head">
        <span>{label}</span>
        <button type="button" className="button ghost small" onClick={() => setRows([...rows, fresh])}>
          <Icon name="plus" />
          {tr("Add")}
        </button>
      </div>
      {rows.length === 0 && <p className="studio-steps-empty">{empty}</p>}
      {rows.map((row, at) => (
        <RowCard key={at} title={title(row)} at={at} count={rows.length} onMove={(to) => setRows(moved(rows, at, to))} onRemove={() => setRows(rows.filter((_, index) => index !== at))}>
          {children(row, (key, value) => setRows(rows.map((one, index) => (index === at ? put(one, key, value) : one))))}
        </RowCard>
      ))}
    </div>
  );
}

function Word({ row, name, label, placeholder, set }: { row: StudioData; name: string; label: string; placeholder?: string; set: (key: string, value: unknown) => void }) {
  return (
    <label className="field studio-field">
      <span>{label}</span>
      <input value={textOf(row, name)} placeholder={placeholder} onChange={(event) => set(name, event.target.value.trim() || undefined)} spellCheck={false} />
    </label>
  );
}

function Pair({ row, name, label, fallback, step = 1, least, most, set }: { row: StudioData; name: string; label: string; fallback: [number, number]; step?: number; least: number; most: number; set: (key: string, value: unknown) => void }) {
  const [from, to] = pairOf(row, name, fallback);
  return (
    <>
      <NumberField value={from} label={tr("{0} from", [label])} step={step} least={least} most={most} onChange={(next) => set(name, [next ?? fallback[0], to])} />
      <NumberField value={to} label={tr("{0} to", [label])} step={step} least={least} most={most} onChange={(next) => set(name, [from, next ?? fallback[1]])} />
    </>
  );
}

function Choice({ row, name, label, options, fallback, labelOf, set }: { row: StudioData; name: string; label: string; options: string[]; fallback: string; labelOf: (one: string) => string; set: (key: string, value: unknown) => void }) {
  const value = options.includes(textOf(row, name)) ? textOf(row, name) : fallback;
  return (
    <label className="field studio-field">
      <span>{label}</span>
      <Dropdown value={value} onChange={(next) => set(name, next)} options={options.map((one) => ({ value: one, label: labelOf(one), hint: one }))} />
    </label>
  );
}

export function WorldgenFields({ data, onChange, structures = [], biomes = [] }: { data: StudioData; onChange: (next: StudioData) => void; structures?: string[]; biomes?: string[] }) {
  const generator = objectOf(data.generator);
  const setGenerator = (next: StudioData) => onChange({ ...data, generator: next });
  const set = (key: string, value: unknown) => setGenerator(put(generator, key, value));
  const warp = objectOf(generator.warp);
  const warpType = WORLDGEN_WARPS.includes(textOf(warp, "type")) ? textOf(warp, "type") : "none";
  const setWarp = (key: string, value: unknown) => set("warp", put(warp, key, value));
  const keep = objectOf(generator.keep);
  const setKeep = (key: string, on: boolean) => set("keep", put(keep, key, on));
  const vanilla = textOf(generator, "mode") === "vanilla";
  const shape = generator.shape && typeof generator.shape === "object" ? objectOf(generator.shape) : null;
  const biomeList = (
    <datalist id="studio-worldgen-biomes">
      {biomes.map((one) => (
        <option key={one} value={`studio:${one}`} />
      ))}
    </datalist>
  );
  const biomeRules = (
    <Rows
      generator={generator}
      name="biomes"
      label={tr("Biomes (the first that fits wins)")}
      empty={vanilla ? tr("No rules: the server's own biomes everywhere.") : tr("No rules: everywhere is plains.")}
      fresh={{ biome: "plains" }}
      title={(row) => textOf(row, "biome") || "plains"}
      onChange={setGenerator}
    >
      {(row, setRow) => (
        <>
          <label className="field studio-field">
            <span>{tr("Biome")}</span>
            <input list="studio-worldgen-biomes" value={textOf(row, "biome")} placeholder="plains" onChange={(event) => setRow("biome", event.target.value.trim() || undefined)} spellCheck={false} />
          </label>
          <Pair row={row} name="temperature" label={tr("Warmth")} fallback={[0, 1]} step={0.05} least={0} most={1} set={setRow} />
          <Pair row={row} name="humidity" label={tr("Wetness")} fallback={[0, 1]} step={0.05} least={0} most={1} set={setRow} />
          {!vanilla && (
            <>
              <Pair row={row} name="height" label={tr("Height")} fallback={[-64, 320]} least={-64} most={320} set={setRow} />
              <NumberField value={row.offset} label={tr("Lift the ground by")} least={-256} most={256} placeholder="0" onChange={(next) => setRow("offset", next || undefined)} />
              <NumberField value={row.stretch} label={tr("Hills here (× of all)")} step={0.1} least={0} most={8} placeholder="1" onChange={(next) => setRow("stretch", next === 1 ? undefined : next)} />
            </>
          )}
        </>
      )}
    </Rows>
  );
  return (
    <div className="studio-worldgen">
      {biomeList}
      <div className="studio-worldgen-form">
        <div className="studio-fields-row">
          <label className="field studio-field">
            <span>{tr("Ground")}</span>
            <Dropdown
              value={vanilla ? "vanilla" : "custom"}
              onChange={(value) => set("mode", value === "vanilla" ? "vanilla" : undefined)}
              options={[
                { value: "custom", label: tr("Made by this generator"), hint: "custom" },
                { value: "vanilla", label: tr("The server's own (vanilla) ground"), hint: "vanilla" },
              ]}
            />
          </label>
          {vanilla && (
            <label className="field studio-field">
              <span>{tr("Vanilla ground")}</span>
              <Dropdown value={WORLDGEN_TERRAINS.includes(textOf(generator, "terrain")) ? textOf(generator, "terrain") : "normal"} onChange={(value) => set("terrain", value === "normal" ? undefined : value)} options={WORLDGEN_TERRAINS.map((one) => ({ value: one, label: terrainLabel(one), hint: one }))} />
            </label>
          )}
        </div>
        {vanilla ? (
          <>
            <p className="studio-placeholders">{tr("Paper puts your biome rules on the server's own ground by warmth and wetness; where no rule fits, its own biome stays. Pumpkin twists that ground with a spiral instead and keeps its own biomes. Layers, surface, caves and features are not used.")}</p>
            <h4 className="studio-worldgen-head">{tr("Twist (Pumpkin only)")}</h4>
            <SwitchRow label={tr("Spiral around a point")} checked={warpType === "spiral"} onChange={(on) => set("warp", on ? { type: "spiral", strength: 1 } : undefined)} />
            {warpType === "spiral" && (
              <div className="studio-fields-row three">
                <NumberField value={warp.strength} label={tr("Turns")} step={0.1} least={-20} most={20} placeholder="1" onChange={(next) => setWarp("strength", next)} />
                <NumberField value={warp.x} label={tr("Middle X")} least={-30000000} most={30000000} placeholder="0" onChange={(next) => setWarp("x", next)} />
                <NumberField value={warp.z} label={tr("Middle Z")} least={-30000000} most={30000000} placeholder="0" onChange={(next) => setWarp("z", next)} />
              </div>
            )}
            <h4 className="studio-worldgen-head">{tr("Biomes (Paper only)")}</h4>
            <NumberField value={generator.climate} label={tr("Climate size (blocks)")} least={16} most={100000} placeholder="512" hint={tr("How big each warm, cold, dry or wet area is.")} onChange={(next) => set("climate", next)} />
            {biomeRules}
          </>
        ) : (
          <>
        <div className="studio-worldgen-presets">
          <span>{tr("Start from")}</span>
          {presets().map((one) => (
            <button key={one.id} type="button" className="button ghost small" onClick={() => setGenerator(one.generator)}>
              {one.label}
            </button>
          ))}
        </div>

        <h4 className="studio-worldgen-head">{tr("Ground")}</h4>
        <div className="studio-fields-row three">
          <NumberField value={generator.base} label={tr("Middle height")} least={-64} most={320} placeholder="68" onChange={(next) => set("base", next)} />
          <NumberField value={generator.height} label={tr("Hills up and down")} least={0} most={256} placeholder="40" onChange={(next) => set("height", next)} />
          <NumberField value={generator.sea} label={tr("Sea level")} least={-64} most={320} placeholder="63" onChange={(next) => set("sea", next)} />
        </div>
        <div className="studio-fields-row three">
          <NumberField value={generator.overhangs} label={tr("Overhangs and arches")} step={0.1} least={0} most={2} placeholder="0" hint={tr("0 keeps the ground solid from below.")} onChange={(next) => set("overhangs", next || undefined)} />
          <label className="field studio-field">
            <span>{tr("Stone")}</span>
            <input value={textOf(generator, "stone")} placeholder="stone" onChange={(event) => set("stone", event.target.value.trim() || undefined)} spellCheck={false} />
          </label>
          <label className="field studio-field">
            <span>{tr("Sea fluid")}</span>
            <input value={textOf(generator, "fluid")} placeholder="water" onChange={(event) => set("fluid", event.target.value.trim() || undefined)} spellCheck={false} />
          </label>
        </div>
        <SwitchRow label={tr("Bedrock at the bottom")} checked={generator.bedrock !== false} onChange={(on) => set("bedrock", on ? undefined : false)} />
        <SwitchRow label={tr("Shape the ground with nodes")} checked={shape !== null} onChange={(on) => set("shape", on ? shapeFromLayers(listIn(generator, "layers")) : undefined)} />
        {shape ? (
          <WorldgenNodes shape={shape} onChange={(next) => set("shape", next)} noises={WORLDGEN_NOISES} noiseLabel={noiseLabel} />
        ) : (
        <Rows
          generator={generator}
          name="layers"
          label={tr("Noise layers (added together)")}
          empty={tr("No layers: the ground is flat at the middle height.")}
          fresh={{ noise: "perlin", scale: 256, octaves: 4, amount: 0.5 }}
          title={(row) => noiseLabel(WORLDGEN_NOISES.includes(textOf(row, "noise")) ? textOf(row, "noise") : "perlin")}
          onChange={setGenerator}
        >
          {(row, setRow) => (
            <>
              <Choice row={row} name="noise" label={tr("Shape")} options={WORLDGEN_NOISES} fallback="perlin" labelOf={noiseLabel} set={setRow} />
              <NumberField value={row.scale} label={tr("Size (blocks)")} least={1} most={100000} placeholder="256" onChange={(next) => setRow("scale", next)} />
              <NumberField value={row.octaves} label={tr("Detail")} least={1} most={8} placeholder="4" onChange={(next) => setRow("octaves", next)} />
              <NumberField value={row.amount} label={tr("Strength")} step={0.05} least={-4} most={4} placeholder="1" onChange={(next) => setRow("amount", next)} />
            </>
          )}
        </Rows>
        )}

        <h4 className="studio-worldgen-head">{tr("Twist")}</h4>
        <div className="studio-fields-row">
          <label className="field studio-field">
            <span>{tr("Twist")}</span>
            <Dropdown value={warpType} onChange={(value) => set("warp", { type: value })} options={WORLDGEN_WARPS.map((one) => ({ value: one, label: warpLabel(one), hint: one }))} />
          </label>
          {warpType === "spiral" && <NumberField value={warp.strength} label={tr("Turns")} step={0.1} least={-20} most={20} placeholder="1" onChange={(next) => setWarp("strength", next)} />}
          {warpType === "domain" && <NumberField value={warp.amount} label={tr("How far (blocks)")} least={0} most={2000} placeholder="64" onChange={(next) => setWarp("amount", next)} />}
        </div>
        {warpType === "spiral" && (
          <div className="studio-fields-row">
            <NumberField value={warp.x} label={tr("Middle X")} least={-30000000} most={30000000} placeholder="0" onChange={(next) => setWarp("x", next)} />
            <NumberField value={warp.z} label={tr("Middle Z")} least={-30000000} most={30000000} placeholder="0" onChange={(next) => setWarp("z", next)} />
          </div>
        )}
        {warpType === "domain" && <NumberField value={warp.scale} label={tr("Wave size (blocks)")} least={1} most={100000} placeholder="256" onChange={(next) => setWarp("scale", next)} />}

        <h4 className="studio-worldgen-head">{tr("Biomes and surface")}</h4>
        <NumberField value={generator.climate} label={tr("Climate size (blocks)")} least={16} most={100000} placeholder="512" hint={tr("How big each warm, cold, dry or wet area is.")} onChange={(next) => set("climate", next)} />
        <NumberField value={generator.blend} label={tr("Biome edge softness")} step={0.01} least={0} most={0.5} placeholder="0.05" hint={tr("How far a biome's lift and hills fade into the next (in warmth and wetness, 0 to 1). 0 makes cliffs at the edges.")} onChange={(next) => set("blend", next)} />
        {biomeRules}
        <Rows
          generator={generator}
          name="surface"
          label={tr("Surface by biome")}
          empty={tr("Grass on dirt everywhere.")}
          fresh={{ biome: "", top: "grass_block", under: "dirt", depth: 3 }}
          title={(row) => textOf(row, "biome") || tr("Every other biome")}
          onChange={setGenerator}
        >
          {(row, setRow) => (
            <>
              <Word row={row} name="biome" label={tr("Biome")} placeholder={tr("Every other biome")} set={setRow} />
              <Word row={row} name="top" label={tr("Top")} placeholder="grass_block" set={setRow} />
              <Word row={row} name="under" label={tr("Under it")} placeholder="dirt" set={setRow} />
              <NumberField value={row.depth} label={tr("How deep")} least={0} most={32} placeholder="3" onChange={(next) => setRow("depth", next)} />
              <Word row={row} name="underwater" label={tr("Under water")} placeholder="gravel" set={setRow} />
            </>
          )}
        </Rows>

        <h4 className="studio-worldgen-head">{tr("Caves and things on the ground")}</h4>
        <Rows generator={generator} name="caves" label={tr("Caves")} empty={tr("No caves.")} fresh={{ type: "spaghetti", scale: 64, width: 0.07 }} title={(row) => caveLabel(textOf(row, "type") || "spaghetti")} onChange={setGenerator}>
          {(row, setRow) => (
            <>
              <Choice row={row} name="type" label={tr("Kind")} options={WORLDGEN_CAVES} fallback="spaghetti" labelOf={caveLabel} set={setRow} />
              <NumberField value={row.scale} label={tr("Size (blocks)")} least={4} most={2000} placeholder={textOf(row, "type") === "cheese" ? "96" : "64"} onChange={(next) => setRow("scale", next)} />
              {textOf(row, "type") === "cheese" ? (
                <NumberField value={row.threshold} label={tr("Rarity")} step={0.05} least={-1} most={1} placeholder="0.55" onChange={(next) => setRow("threshold", next)} />
              ) : (
                <NumberField value={row.width} label={tr("Width")} step={0.01} least={0} most={1} placeholder="0.07" onChange={(next) => setRow("width", next)} />
              )}
            </>
          )}
        </Rows>
        <Rows
          generator={generator}
          name="features"
          label={tr("Ores, trees and more")}
          empty={tr("Nothing on the ground.")}
          fresh={{ type: "tree", trunk: "oak_log", leaves: "oak_leaves", per_chunk: 2 }}
          title={(row) => featureLabel(textOf(row, "type") || "tree")}
          onChange={setGenerator}
        >
          {(row, setRow) => {
            const type = WORLDGEN_FEATURES.includes(textOf(row, "type")) ? textOf(row, "type") : "tree";
            return (
              <>
                <Choice row={row} name="type" label={tr("Kind")} options={WORLDGEN_FEATURES} fallback="tree" labelOf={featureLabel} set={setRow} />
                {type === "tree" ? (
                  <>
                    <Word row={row} name="trunk" label={tr("Trunk")} placeholder="oak_log" set={setRow} />
                    <Word row={row} name="leaves" label={tr("Leaves")} placeholder="oak_leaves" set={setRow} />
                    <Pair row={row} name="height" label={tr("Height")} fallback={[4, 6]} least={1} most={32} set={setRow} />
                  </>
                ) : type === "structure" ? (
                  <>
                    <label className="field studio-field">
                      <span>{tr("Structure")}</span>
                      <input list="studio-worldgen-structures" value={textOf(row, "structure")} placeholder="ruin" spellCheck={false} onChange={(event) => setRow("structure", event.target.value.trim())} />
                      <datalist id="studio-worldgen-structures">
                        {structures.map((one) => (
                          <option key={one} value={one} />
                        ))}
                      </datalist>
                      <small>{tr("Up to 32 blocks across. Its middle goes on the ground.")}</small>
                    </label>
                    <Choice row={row} name="rotation" label={tr("Turned (degrees)")} options={["random", "0", "90", "180", "270"]} fallback="random" labelOf={(one) => one} set={setRow} />
                    <NumberField value={row.lift} label={tr("Raise by (blocks)")} least={-64} most={64} placeholder="0" onChange={(next) => setRow("lift", next)} />
                  </>
                ) : (
                  <Word row={row} name="block" label={tr("Block")} placeholder={type === "ore" ? "coal_ore" : type === "patch" ? "short_grass" : "mossy_cobblestone"} set={setRow} />
                )}
                {type === "ore" && <Pair row={row} name="y" label={tr("Height")} fallback={[-48, 64]} least={-64} most={320} set={setRow} />}
                {(type === "ore" || type === "boulder") && <NumberField value={row.size} label={tr("Size")} least={1} most={type === "ore" ? 16 : 6} placeholder="3" onChange={(next) => setRow("size", next)} />}
                <NumberField value={row.per_chunk} label={tr("Per chunk")} step={0.05} least={0} most={64} placeholder="1" onChange={(next) => setRow("per_chunk", next)} />
                <label className="field studio-field wide">
                  <span>{tr("Only in biomes")}</span>
                  <input value={wordsOf(row, "biomes")} placeholder={tr("Every biome")} onChange={(event) => setRow("biomes", words(event.target.value))} spellCheck={false} />
                </label>
              </>
            );
          }}
        </Rows>
        <p className="studio-placeholders">{tr("Block names as in Minecraft (sand, oak_log) or your own studio blocks (studio:ruby_ore).")}</p>

        <h4 className="studio-worldgen-head">{tr("From the server's own generation")}</h4>
        <SwitchRow label={tr("Villages and other structures")} checked={keep.structures === true} onChange={(on) => setKeep("structures", on)} />
        <SwitchRow label={tr("Mobs spawn when chunks are made")} checked={keep.mobs !== false} onChange={(on) => setKeep("mobs", on)} />
        <SwitchRow label={tr("Vanilla caves and ravines")} checked={keep.carvers === true} onChange={(on) => setKeep("carvers", on)} />
        <SwitchRow label={tr("Vanilla trees, flowers and ores")} checked={keep.decorations === true} onChange={(on) => setKeep("decorations", on)} />
        <p className="studio-placeholders">{tr("On Pumpkin, structures go where the seed would put them in a normal world, not by your biomes, and the mobs switch does nothing.")}</p>
          </>
        )}
      </div>
      <WorldgenMap generator={generator} seed={textOf(data, "seed")} world={textOf(data, "name") || textOf(data, "id")} />
    </div>
  );
}

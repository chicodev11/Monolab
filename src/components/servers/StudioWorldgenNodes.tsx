import { useState } from "react";
import { tr } from "../../lib/i18n";
import { DropdownMenu } from "../primitives";
import { Dropdown } from "../ui/Dropdown";
import { Icon } from "../ui/Icon";
import { NumberField, put } from "./StudioItemFields";
import { textOf, type StudioData } from "./StudioPreview";
import { objectOf } from "./studioKinds";

/**
 * A custom world's ground as a tree of nodes (`"shape"` in the generator, `Shape` in
 * `crates/studio-format/src/worldgen.rs`): the top node's value makes the height (middle height + hills × value).
 * Nodes that take several inputs (`inputs`) or one (`input`) show them in a row underneath.
 */

export const WORLDGEN_SHAPES = ["noise", "constant", "add", "multiply", "min", "max", "clamp", "abs", "power", "terrace", "remap", "climate", "distance"];
const MANY = ["add", "multiply", "min", "max"];
const ONE = ["clamp", "abs", "power", "terrace", "remap"];

export function shapeLabel(type: string): string {
  return (
    (
      {
        noise: tr("Noise"),
        constant: tr("Number"),
        add: tr("Add together"),
        multiply: tr("Multiply"),
        min: tr("Lowest of"),
        max: tr("Highest of"),
        clamp: tr("Keep between"),
        abs: tr("Fold below zero up"),
        power: tr("Sharpen"),
        terrace: tr("Terraces"),
        remap: tr("Rescale"),
        climate: tr("Climate"),
        distance: tr("Distance from a point"),
      } as Record<string, string>
    )[type] ?? type
  );
}

function typeOf(node: StudioData): string {
  const type = textOf(node, "type");
  return WORLDGEN_SHAPES.includes(type) ? type : "constant";
}

function num(node: StudioData, key: string, fallback: number): number {
  return typeof node[key] === "number" ? (node[key] as number) : fallback;
}

function summary(node: StudioData, noiseLabel: (one: string) => string): string {
  const type = typeOf(node);
  switch (type) {
    case "noise":
      return `${noiseLabel(textOf(node, "noise") || "perlin")} · ${num(node, "scale", 256)} · ×${num(node, "amount", 1)}`;
    case "constant":
      return String(num(node, "value", 0));
    case "clamp":
      return `${num(node, "min", -1)} … ${num(node, "max", 1)}`;
    case "power":
      return `^${num(node, "exponent", 2)}`;
    case "terrace":
      return tr("{0} steps", [String(num(node, "steps", 4))]);
    case "climate":
      return textOf(node, "which") === "wetness" ? tr("Wetness") : tr("Warmth");
    case "distance":
      return `${num(node, "x", 0)}, ${num(node, "z", 0)} · r ${num(node, "radius", 512)}`;
    default:
      return "";
  }
}

/** The nodes feeding one: `inputs` for nodes that take many, `input` for those that take one. */
export function inputsOf(node: StudioData): StudioData[] {
  const type = typeOf(node);
  if (MANY.includes(type)) return Array.isArray(node.inputs) ? (node.inputs as unknown[]).map(objectOf) : [];
  if (ONE.includes(type)) return node.input && typeof node.input === "object" ? [objectOf(node.input)] : [];
  return [];
}

function withInputs(node: StudioData, list: StudioData[]): StudioData {
  const type = typeOf(node);
  if (MANY.includes(type)) return { ...node, inputs: list };
  if (ONE.includes(type)) return put(node, "input", list[0]);
  return node;
}

function at(root: StudioData, path: number[]): StudioData | undefined {
  let node: StudioData | undefined = root;
  for (const step of path) node = node ? inputsOf(node)[step] : undefined;
  return node;
}

function replaced(root: StudioData, path: number[], next: StudioData | undefined): StudioData | undefined {
  if (path.length === 0) return next;
  const [head, ...rest] = path;
  const list = inputsOf(root);
  const child = replaced(list[head], rest, next);
  const changed = child === undefined ? list.filter((_, index) => index !== head) : list.map((one, index) => (index === head ? child : one));
  return withInputs(root, changed);
}

/** A fresh node of a type (with a noise under nodes that take inputs, so they do something at once). */
export function freshNode(type: string): StudioData {
  switch (type) {
    case "noise":
      return { type, noise: "perlin", scale: 256, octaves: 4, amount: 1 };
    case "constant":
      return { type, value: 0 };
    case "clamp":
      return { type, min: -0.5, max: 0.5, input: freshNode("noise") };
    case "power":
      return { type, exponent: 2, input: freshNode("noise") };
    case "terrace":
      return { type, steps: 4, input: freshNode("noise") };
    case "remap":
      return { type, from: [-1, 1], to: [0, 1], input: freshNode("noise") };
    case "abs":
      return { type, input: freshNode("noise") };
    case "climate":
      return { type, which: "warmth" };
    case "distance":
      return { type, x: 0, z: 0, radius: 512 };
    default:
      return { type, inputs: [freshNode("noise")] };
  }
}

/** Noise layers as the same ground in nodes: their sum. */
export function shapeFromLayers(layers: StudioData[]): StudioData {
  return { type: "add", inputs: layers.map((one) => ({ type: "noise", noise: textOf(one, "noise") || "perlin", scale: num(one, "scale", 256), octaves: num(one, "octaves", 4), amount: num(one, "amount", 1) })) };
}

const W = 196;
const H = 56;
const GAP_X = 18;
const GAP_Y = 46;

type Placed = { path: number[]; node: StudioData; x: number; y: number };

/** Lays the tree out top-down: each node above the middle of what feeds it. */
function layout(root: StudioData): { boxes: Placed[]; edges: [number, number, number, number][]; width: number; height: number } {
  const boxes: Placed[] = [];
  const edges: [number, number, number, number][] = [];
  let next = 0;
  let deepest = 0;
  const place = (node: StudioData, path: number[], depth: number): number => {
    deepest = Math.max(deepest, depth);
    const list = inputsOf(node);
    let x: number;
    if (list.length === 0 || path.length > 12) {
      x = next * (W + GAP_X);
      next += 1;
    } else {
      const xs = list.map((child, index) => place(child, [...path, index], depth + 1));
      x = (xs[0] + xs[xs.length - 1]) / 2;
      xs.forEach((childX) => edges.push([x + W / 2, depth * (H + GAP_Y) + H, childX + W / 2, (depth + 1) * (H + GAP_Y)]));
    }
    boxes.push({ path, node, x, y: depth * (H + GAP_Y) });
    return x;
  };
  place(root, [], 0);
  return { boxes, edges, width: Math.max(1, next) * (W + GAP_X), height: (deepest + 1) * (H + GAP_Y) };
}

export function WorldgenNodes({ shape, onChange, noises, noiseLabel }: { shape: StudioData; onChange: (next: StudioData) => void; noises: string[]; noiseLabel: (one: string) => string }) {
  const [picked, setPicked] = useState<number[]>([]);
  const { boxes, edges, width, height } = layout(shape);
  const node = at(shape, picked) ?? shape;
  const path = at(shape, picked) ? picked : [];
  const type = typeOf(node);
  const set = (next: StudioData) => onChange(replaced(shape, path, next) ?? freshNode("constant"));
  const setKey = (key: string, value: unknown) => set(put(node, key, value));
  const takes = MANY.includes(type) || (ONE.includes(type) && inputsOf(node).length === 0);
  const same = (a: number[], b: number[]) => a.length === b.length && a.every((one, index) => one === b[index]);
  const pair = (key: string, fallback: [number, number]): [number, number] => {
    const value = node[key];
    return Array.isArray(value) && value.length === 2 && value.every((one) => typeof one === "number") ? [value[0] as number, value[1] as number] : fallback;
  };
  return (
    <div className="studio-nodes studio-worldgen-nodes">
      <div className="studio-nodes-canvas" role="group" aria-label={tr("Ground as nodes")}>
        <div className="studio-nodes-board" style={{ width, height }}>
          <svg className="studio-nodes-edges" width={width} height={height} aria-hidden>
            {edges.map(([x1, y1, x2, y2], index) => {
              const mid = (y1 + y2) / 2;
              return <path key={index} d={`M ${x1} ${y1} C ${x1} ${mid}, ${x2} ${mid}, ${x2} ${y2}`} />;
            })}
          </svg>
          {boxes.map((box) => (
            <button
              key={box.path.join(".") || "top"}
              type="button"
              className={`studio-node ${box.path.length === 0 ? "when" : ""} ${same(box.path, path) ? "on" : ""}`}
              style={{ left: box.x, top: box.y, width: W, height: H }}
              aria-pressed={same(box.path, path)}
              onClick={() => setPicked(box.path)}
            >
              <strong>{shapeLabel(typeOf(box.node))}</strong>
              <small>{summary(box.node, noiseLabel) || " "}</small>
            </button>
          ))}
        </div>
      </div>
      <aside className="studio-nodes-panel" aria-label={tr("Selected node")}>
        <div className="studio-nodes-panel-head">
          <strong>{path.length === 0 ? tr("Top node: makes the height") : shapeLabel(type)}</strong>
          <span className="studio-step-tools">
            {takes && (
              <DropdownMenu
                trigger={
                  <button type="button" className="icon-button" aria-label={tr("Add input")} data-tip={tr("Add input")}>
                    <Icon name="plus" />
                  </button>
                }
                items={WORLDGEN_SHAPES.map((one) => ({
                  label: shapeLabel(one),
                  onSelect: () => {
                    const list = [...inputsOf(node), freshNode(one)];
                    set(withInputs(node, list));
                    setPicked([...path, list.length - 1]);
                  },
                }))}
              />
            )}
            {path.length > 0 && (
              <button
                type="button"
                className="icon-button"
                aria-label={tr("Remove")}
                data-tip={tr("Remove")}
                onClick={() => {
                  onChange(replaced(shape, path, undefined) ?? freshNode("constant"));
                  setPicked(path.slice(0, -1));
                }}
              >
                <Icon name="trash" />
              </button>
            )}
          </span>
        </div>
        <label className="field studio-field">
          <span>{tr("Kind")}</span>
          <Dropdown
            value={type}
            onChange={(next) => {
              // Keep what fed it when the new kind takes inputs too.
              const fed = inputsOf(node);
              const made = freshNode(next);
              set(fed.length && (MANY.includes(next) || ONE.includes(next)) ? withInputs(made, MANY.includes(next) ? fed : fed.slice(0, 1)) : made);
            }}
            options={WORLDGEN_SHAPES.map((one) => ({ value: one, label: shapeLabel(one), hint: one }))}
          />
        </label>
        {type === "noise" && (
          <>
            <label className="field studio-field">
              <span>{tr("Shape")}</span>
              <Dropdown value={noises.includes(textOf(node, "noise")) ? textOf(node, "noise") : "perlin"} onChange={(next) => setKey("noise", next)} options={noises.map((one) => ({ value: one, label: noiseLabel(one), hint: one }))} />
            </label>
            <NumberField value={node.scale} label={tr("Size (blocks)")} least={1} most={100000} placeholder="256" onChange={(next) => setKey("scale", next)} />
            <NumberField value={node.octaves} label={tr("Detail")} least={1} most={8} placeholder="4" onChange={(next) => setKey("octaves", next)} />
            <NumberField value={node.amount} label={tr("Strength")} step={0.05} least={-4} most={4} placeholder="1" onChange={(next) => setKey("amount", next)} />
          </>
        )}
        {type === "constant" && <NumberField value={node.value} label={tr("Value")} step={0.05} least={-100} most={100} placeholder="0" onChange={(next) => setKey("value", next)} />}
        {type === "clamp" && (
          <>
            <NumberField value={node.min} label={tr("Lowest")} step={0.05} least={-100} most={100} placeholder="-1" onChange={(next) => setKey("min", next)} />
            <NumberField value={node.max} label={tr("Highest")} step={0.05} least={-100} most={100} placeholder="1" onChange={(next) => setKey("max", next)} />
          </>
        )}
        {type === "power" && <NumberField value={node.exponent} label={tr("Power")} step={0.1} least={0.1} most={8} placeholder="2" hint={tr("Above 1 flattens low ground and sharpens peaks.")} onChange={(next) => setKey("exponent", next)} />}
        {type === "terrace" && <NumberField value={node.steps} label={tr("Steps between -1 and 1")} least={1} most={64} placeholder="4" onChange={(next) => setKey("steps", next)} />}
        {type === "remap" && (
          <>
            <NumberField value={pair("from", [-1, 1])[0]} label={tr("From low")} step={0.05} least={-100} most={100} onChange={(next) => setKey("from", [next ?? -1, pair("from", [-1, 1])[1]])} />
            <NumberField value={pair("from", [-1, 1])[1]} label={tr("From high")} step={0.05} least={-100} most={100} onChange={(next) => setKey("from", [pair("from", [-1, 1])[0], next ?? 1])} />
            <NumberField value={pair("to", [0, 1])[0]} label={tr("To low")} step={0.05} least={-100} most={100} onChange={(next) => setKey("to", [next ?? 0, pair("to", [0, 1])[1]])} />
            <NumberField value={pair("to", [0, 1])[1]} label={tr("To high")} step={0.05} least={-100} most={100} onChange={(next) => setKey("to", [pair("to", [0, 1])[0], next ?? 1])} />
          </>
        )}
        {type === "climate" && (
          <label className="field studio-field">
            <span>{tr("Which")}</span>
            <Dropdown
              value={textOf(node, "which") === "wetness" ? "wetness" : "warmth"}
              onChange={(next) => setKey("which", next)}
              options={[
                { value: "warmth", label: tr("Warmth") },
                { value: "wetness", label: tr("Wetness") },
              ]}
            />
            <small>{tr("-1 where it is coldest or driest, 1 where it is warmest or wettest.")}</small>
          </label>
        )}
        {type === "distance" && (
          <>
            <NumberField value={node.x} label={tr("Middle X")} least={-30000000} most={30000000} placeholder="0" onChange={(next) => setKey("x", next)} />
            <NumberField value={node.z} label={tr("Middle Z")} least={-30000000} most={30000000} placeholder="0" onChange={(next) => setKey("z", next)} />
            <NumberField value={node.radius} label={tr("Radius (blocks)")} least={1} most={100000} placeholder="512" hint={tr("1 in the middle, 0 at the radius, below 0 further out.")} onChange={(next) => setKey("radius", next)} />
          </>
        )}
        {(MANY.includes(type) || ONE.includes(type)) && inputsOf(node).length === 0 && <p className="muted studio-nodes-hint">{tr("Nothing feeds this node yet. Add an input with +.")}</p>}
      </aside>
    </div>
  );
}

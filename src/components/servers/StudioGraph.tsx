import { useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import { tr } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import { ACTION_TYPES, actionLabel, actionSummary, conditionSummary, expansionTrigger, listOf, newAction, newExpansionStep, StepFields, StudioExpansions, type Expansion } from "./StudioActions";
import { RuleWhen } from "./StudioFields";
import type { StudioData } from "./StudioPreview";
import { objectOf, whenSummary, type StudioNames } from "./studioKinds";
import {
  allNodes,
  connect,
  disconnect,
  edgesOf,
  ends,
  eventValues,
  find,
  FLOW_TYPES,
  insert,
  moveTo,
  newNode,
  NODE_H,
  NODE_W,
  pinsOf,
  putList,
  readGraph,
  remove,
  rootOf,
  setStep,
  textKeys,
  writeGraph,
  type From,
  type Graph,
  type RootDef,
} from "./studioGraphModel";

/** The lists a file of this kind starts from; none = no node editor for it. */
export function rootsOf(kind: string, data: StudioData): RootDef[] {
  const list = (key: string, label: string): RootDef => ({
    key,
    label,
    conditions: false,
    otherwise: false,
    event: false,
    read: (value) => ({ steps: listOf(value[key]) }),
    write: (value, next) => putList(value, key, next.steps),
  });
  switch (kind) {
    case "rule":
      return [
        {
          key: "rule",
          label: tr("When"),
          conditions: true,
          otherwise: true,
          event: true,
          read: (value) => ({ steps: listOf(value.then), conditions: listOf(value.if), otherwise: listOf(value.else) }),
          write: (value, next) => putList(putList(putList(value, "if", next.conditions ?? []), "then", next.steps), "else", next.otherwise ?? []),
        },
      ];
    case "command":
      return [list("actions", tr("What it does"))];
    case "function":
      return [list("actions", tr("Steps"))];
    case "quest":
      return [list("startActions", tr("When it starts")), list("rewards", tr("Rewards"))];
    case "item":
      return [list("useActions", tr("When used")), list("eatActions", tr("When eaten")), list("hitActions", tr("When it hits")), list("breakActions", tr("When it breaks a block"))];
    case "block":
      return [list("placeActions", tr("When placed")), list("useActions", tr("When right-clicked")), list("breakActions", tr("When broken"))];
    case "furniture":
      return [list("useActions", tr("When right-clicked")), list("breakActions", tr("When broken"))];
    case "enchant":
      return [list("hitActions", tr("When it hits")), list("mineActions", tr("When it breaks a block")), list("hurtActions", tr("When the wearer is hurt"))];
    case "npc":
      return [
        {
          key: "npc",
          label: tr("When right-clicked"),
          conditions: true,
          otherwise: true,
          event: false,
          read: (value) => ({ steps: listOf(value.actions), conditions: listOf(value.if), otherwise: listOf(value.deny) }),
          write: (value, next) => putList(putList(putList(value, "if", next.conditions ?? []), "actions", next.steps), "deny", next.otherwise ?? []),
        },
      ];
    case "mob":
      return listOf(data.skills).map((skill, at) => {
        const put = (value: StudioData, next: StudioData) => ({ ...value, skills: listOf(value.skills).map((was, index) => (index === at ? next : was)) });
        return {
          key: `skill${at}`,
          label: tr("Skill {0} · {1}", [String(at + 1), typeof skill.trigger === "string" ? skill.trigger : "attack"]),
          conditions: true,
          otherwise: false,
          event: false,
          read: (value) => {
            const one = listOf(value.skills)[at] ?? {};
            return { steps: listOf(one.actions), conditions: listOf(one.conditions) };
          },
          write: (value, next) => {
            const one = listOf(value.skills)[at] ?? {};
            return put(value, putList(putList(one, "conditions", next.conditions ?? []), "actions", next.steps));
          },
        };
      });
    default:
      return [];
  }
}


function pinLabel(type: string, pin: string): string {
  if (pin === "then") return tr("Yes");
  if (pin === "else") return tr("No");
  if (pin === "actions") return type === "repeat" ? tr("Each time") : tr("Each player");
  return tr("Next");
}

function nodeTitle(step: StudioData): string {
  return actionLabel(String(step.type ?? ""));
}

function nodeDetail(step: StudioData): string {
  switch (step.type) {
    case "if": {
      const list = listOf(step.if);
      return list.length ? list.map(conditionSummary).filter(Boolean).join(" · ") || tr("{0} conditions", [String(list.length)]) : tr("No conditions yet");
    }
    case "repeat":
      return tr("{0} times", [String(typeof step.times === "number" ? step.times : 2)]);
    case "each_player":
      return tr("Everyone online");
    case "cancel":
      return tr("Stops what the player did");
    case "stop":
      return tr("Nothing after runs");
    default:
      return actionSummary(step);
  }
}

type Pick = { kind: "node"; id: string } | { kind: "root"; key: string } | { kind: "edge"; from: From } | null;
type Menu = { x: number; y: number; at: [number, number]; from: From | null };
type ValueMenu = { x: number; y: number; id: string; value: string };
type Drag = { id: string; startX: number; startY: number; fromX: number; fromY: number; moved: boolean };
type Wire = { from: From; x: number; y: number } | { value: string; root: string; x: number; y: number };

const MIN_ZOOM = 0.35;
const MAX_ZOOM = 1.6;
/** Spot of an output pin, relative to its node. "next" is under the middle; a flow step's lists are on its right. */
function pinSpot(pins: string[], pin: string): [number, number] {
  if (pin === "next") return [NODE_W / 2, NODE_H];
  const index = pins.indexOf(pin);
  return [NODE_W, ((index + 1) * NODE_H) / (pins.length + 1)];
}

/**
 * The node editor: every list a file starts from as a node on a canvas you can pan and zoom, its steps as nodes
 * wired "next" and into a flow step's own lists. Drag nodes to move them, drag from an output dot to wire it (drop on
 * nothing to add a node there), right-click or double-click the canvas to add a loose one. Same JSON as the fields
 * view; spots and loose nodes go in "graph".
 */
export function StudioGraph({ kind, data, onChange, names }: { kind: string; data: StudioData; onChange: (next: StudioData) => void; names: StudioNames }) {
  const defs = useMemo(() => rootsOf(kind, data), [kind, data]);
  const graph = useMemo(() => readGraph(data, defs), [data, defs]);
  const expansions = useContext(StudioExpansions);
  const [view, setView] = useState({ x: 32, y: 72, zoom: 1 });
  const [pick, setPick] = useState<Pick>(null);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [valueMenu, setValueMenu] = useState<ValueMenu | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [wire, setWire] = useState<Wire | null>(null);
  const [pan, setPan] = useState<{ startX: number; startY: number; fromX: number; fromY: number; moved: boolean } | null>(null);
  const canvas = useRef<HTMLDivElement>(null);

  const save = useCallback((next: Graph) => onChange(writeGraph(data, next, defs)), [data, defs, onChange]);
  const when = objectOf(data.when);
  const event = typeof when.event === "string" ? when.event : "join";
  const trigger = event.includes(":") ? expansionTrigger(expansions, event) : undefined;
  const values = trigger ? ["player", ...trigger.vars] : eventValues(event);

  // Wheel zooms around the pointer (dragging the board pans). Not passive, so the page doesn't scroll too.
  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    const onWheel = (wheel: WheelEvent) => {
      wheel.preventDefault();
      if (wheel.deltaY === 0) return;
      const rect = element.getBoundingClientRect();
      const px = wheel.clientX - rect.left;
      const py = wheel.clientY - rect.top;
      setView((was) => {
        const zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, was.zoom * (wheel.deltaY < 0 ? 1.1 : 1 / 1.1)));
        return { zoom, x: px - ((px - was.x) * zoom) / was.zoom, y: py - ((py - was.y) * zoom) / was.zoom };
      });
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, []);

  const toBoard = (clientX: number, clientY: number): [number, number] => {
    const rect = canvas.current?.getBoundingClientRect();
    if (!rect) return [0, 0];
    return [(clientX - rect.left - view.x) / view.zoom, (clientY - rect.top - view.y) / view.zoom];
  };
  const toCanvas = (clientX: number, clientY: number): [number, number] => {
    const rect = canvas.current?.getBoundingClientRect();
    return rect ? [clientX - rect.left, clientY - rect.top] : [0, 0];
  };

  // A dragged node's spot while the drag is on, so the file isn't rewritten on every move.
  const [live, setLive] = useState<{ id: string; x: number; y: number } | null>(null);
  const at = (id: string, x: number, y: number): [number, number] => (live && live.id === id ? [live.x, live.y] : [x, y]);

  const nodes = allNodes(graph);
  const edges = edgesOf(graph);
  const box = new Map<string, { x: number; y: number; pins: string[]; step: StudioData | null }>();
  for (const root of graph.roots) {
    const [x, y] = at(`@${root.key}`, root.x, root.y);
    box.set(`@${root.key}`, { x, y, pins: [], step: null });
  }
  for (const node of nodes) {
    const [x, y] = at(node.id, node.x, node.y);
    box.set(node.id, { x, y, pins: pinsOf(node.step), step: node.step });
  }

  const startDrag = (id: string, x: number, y: number) => (pointer: ReactPointerEvent) => {
    if (pointer.button !== 0) return;
    pointer.stopPropagation();
    canvas.current?.setPointerCapture(pointer.pointerId);
    setDrag({ id, startX: pointer.clientX, startY: pointer.clientY, fromX: x, fromY: y, moved: false });
    setPick(id.startsWith("@") ? { kind: "root", key: id.slice(1) } : { kind: "node", id });
    setMenu(null);
    setValueMenu(null);
  };
  const startWire = (from: From) => (pointer: ReactPointerEvent) => {
    if (pointer.button !== 0) return;
    pointer.stopPropagation();
    canvas.current?.setPointerCapture(pointer.pointerId);
    const [x, y] = toBoard(pointer.clientX, pointer.clientY);
    setWire({ from, x, y });
    setMenu(null);
  };
  const startValue = (value: string, rootKey: string) => (pointer: ReactPointerEvent) => {
    if (pointer.button !== 0) return;
    pointer.stopPropagation();
    canvas.current?.setPointerCapture(pointer.pointerId);
    const [x, y] = toBoard(pointer.clientX, pointer.clientY);
    setWire({ value, root: rootKey, x, y });
  };

  const onPointerDown = (pointer: ReactPointerEvent) => {
    if (pointer.button !== 0 && pointer.button !== 1) return;
    canvas.current?.setPointerCapture(pointer.pointerId);
    setPan({ startX: pointer.clientX, startY: pointer.clientY, fromX: view.x, fromY: view.y, moved: false });
  };
  const onPointerMove = (pointer: ReactPointerEvent) => {
    if (drag) {
      const dx = (pointer.clientX - drag.startX) / view.zoom;
      const dy = (pointer.clientY - drag.startY) / view.zoom;
      if (!drag.moved && Math.hypot(dx, dy) < 3) return;
      if (!drag.moved) setDrag({ ...drag, moved: true });
      setLive({ id: drag.id, x: Math.round(drag.fromX + dx), y: Math.round(drag.fromY + dy) });
    } else if (wire) {
      const [x, y] = toBoard(pointer.clientX, pointer.clientY);
      setWire({ ...wire, x, y });
    } else if (pan) {
      const dx = pointer.clientX - pan.startX;
      const dy = pointer.clientY - pan.startY;
      if (!pan.moved && Math.hypot(dx, dy) < 3) return;
      setPan({ ...pan, moved: true });
      setView((was) => ({ ...was, x: pan.fromX + dx, y: pan.fromY + dy }));
    }
  };
  const nodeUnder = (clientX: number, clientY: number): string | null => {
    const element = document.elementFromPoint(clientX, clientY);
    const holder = element instanceof Element ? element.closest("[data-node]") : null;
    return holder?.getAttribute("data-node") ?? null;
  };
  const onPointerUp = (pointer: ReactPointerEvent) => {
    if (drag) {
      if (drag.moved && live) save(moveTo(graph, drag.id, live.x, live.y));
      setDrag(null);
      setLive(null);
    } else if (wire) {
      const target = nodeUnder(pointer.clientX, pointer.clientY);
      if ("from" in wire) {
        if (target && !target.startsWith("@") && target !== wire.from.id) {
          save(connect(graph, wire.from, target));
        } else if (!target) {
          const [x, y] = toCanvas(pointer.clientX, pointer.clientY);
          setMenu({ x, y, at: [wire.x - NODE_W / 2, wire.y], from: wire.from });
        }
      } else if (target && !target.startsWith("@")) {
        const [x, y] = toCanvas(pointer.clientX, pointer.clientY);
        setValueMenu({ x, y, id: target, value: wire.value });
      }
      setWire(null);
    } else if (pan) {
      if (!pan.moved) {
        setPick(null);
        setMenu(null);
        setValueMenu(null);
      }
      setPan(null);
    }
  };
  const openMenuAt = (clientX: number, clientY: number) => {
    const [x, y] = toCanvas(clientX, clientY);
    const [bx, by] = toBoard(clientX, clientY);
    setMenu({ x, y, at: [bx, by], from: null });
    setValueMenu(null);
  };

  const add = (step: StudioData) => {
    if (!menu) return;
    const x = Math.round(menu.at[0]);
    let y = Math.round(menu.at[1]);
    if (!menu.from) {
      // A loose node goes down until it covers no other node (nor a trigger's value dots beside it).
      const taken = [...box.entries()].map(([id, one]) => ({ x: one.x, y: one.y, w: NODE_W + (id.startsWith("@") ? 90 : 0) }));
      const covers = () => taken.some((one) => x < one.x + one.w + 16 && x + NODE_W + 16 > one.x && y < one.y + NODE_H + 16 && y + NODE_H + 16 > one.y);
      for (let tries = 0; tries < 50 && covers(); tries += 1) y += NODE_H + 24;
    }
    const node = newNode(step, x, y);
    save(insert(graph, menu.from, node));
    setPick({ kind: "node", id: node.id });
    setMenu(null);
  };
  const removePicked = () => {
    if (pick?.kind === "node") {
      save(remove(graph, pick.id));
      setPick(null);
    } else if (pick?.kind === "edge") {
      save(disconnect(graph, pick.from));
      setPick(null);
    }
  };
  const nudge = (dx: number, dy: number) => {
    if (pick?.kind !== "node" && pick?.kind !== "root") return;
    const id = pick.kind === "node" ? pick.id : `@${pick.key}`;
    const spot = box.get(id);
    if (spot) save(moveTo(graph, id, spot.x + dx, spot.y + dy));
  };
  const onKeyDown = (key: ReactKeyboardEvent) => {
    if (key.target instanceof HTMLInputElement || key.target instanceof HTMLTextAreaElement) return;
    if (key.key === "Delete" || key.key === "Backspace") {
      key.preventDefault();
      removePicked();
    } else if (key.key === "Escape") {
      setMenu(null);
      setValueMenu(null);
      setWire(null);
    } else if (key.key.startsWith("Arrow") && pick && pick.kind !== "edge") {
      key.preventDefault();
      const step = key.shiftKey ? 64 : 16;
      nudge(key.key === "ArrowLeft" ? -step : key.key === "ArrowRight" ? step : 0, key.key === "ArrowUp" ? -step : key.key === "ArrowDown" ? step : 0);
    }
  };
  const zoomBy = (factor: number) => {
    const rect = canvas.current?.getBoundingClientRect();
    const px = rect ? rect.width / 2 : 0;
    const py = rect ? rect.height / 2 : 0;
    setView((was) => {
      const zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, was.zoom * factor));
      return { zoom, x: px - ((px - was.x) * zoom) / was.zoom, y: py - ((py - was.y) * zoom) / was.zoom };
    });
  };
  const fit = () => {
    const rect = canvas.current?.getBoundingClientRect();
    const spots = [...box.values()];
    if (!rect || spots.length === 0) return;
    const left = Math.min(...spots.map((one) => one.x));
    const top = Math.min(...spots.map((one) => one.y));
    const right = Math.max(...spots.map((one) => one.x + NODE_W));
    const bottom = Math.max(...spots.map((one) => one.y + NODE_H));
    const zoom = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, Math.min((rect.width - 64) / (right - left), (rect.height - 104) / (bottom - top), 1)));
    // The tool bar sits over the top left, so the nodes start under it.
    setView({ zoom, x: 32 - left * zoom, y: 72 - top * zoom });
  };

  if (defs.length === 0) {
    return <p className="muted studio-nodes-hint">{tr("Add a skill in the Fields view first.")}</p>;
  }

  const wirePath = (x1: number, y1: number, x2: number, y2: number, side: boolean) => {
    if (side) {
      const bend = Math.max(40, Math.abs(x2 - x1) / 2);
      return `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2} ${y2 - bend}, ${x2} ${y2}`;
    }
    const bend = Math.max(30, Math.abs(y2 - y1) / 2);
    return `M ${x1} ${y1} C ${x1} ${y1 + bend}, ${x2} ${y2 - bend}, ${x2} ${y2}`;
  };
  const outSpot = (from: From): [number, number] | null => {
    const one = box.get(from.id);
    if (!one) return null;
    const [dx, dy] = pinSpot(one.pins, from.pin);
    return [one.x + dx, one.y + dy];
  };
  const valueSpot = (rootKey: string, value: string): [number, number] | null => {
    const one = box.get(`@${rootKey}`);
    if (!one) return null;
    const index = values.indexOf(value);
    return [one.x + NODE_W + 10, one.y + 14 + index * 18];
  };

  // Values a node uses: a dotted wire from the trigger's value dot for each <value> in its text.
  const valueWires: { key: string; root: string; value: string; to: string }[] = [];
  for (const def of defs) {
    if (!def.event) continue;
    for (const node of nodes) {
      if (rootOf(graph, node.id) !== def.key) continue;
      const text = JSON.stringify(node.step);
      for (const value of values) if (text.includes(`<${value}>`)) valueWires.push({ key: `${def.key}-${value}-${node.id}`, root: def.key, value, to: node.id });
    }
  }

  const picked = pick?.kind === "node" ? find(graph, pick.id) : null;
  const pickedDef = pick?.kind === "root" ? defs.find((one) => one.key === pick.key) : undefined;

  return (
    <div className="studio-graph">
      <div
        ref={canvas}
        className={`studio-graph-canvas ${pan?.moved ? "panning" : ""}`}
        role="application"
        aria-label={tr("Node editor")}
        tabIndex={0}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={() => {
          setDrag(null);
          setLive(null);
          setWire(null);
          setPan(null);
        }}
        onContextMenu={(mouse) => {
          mouse.preventDefault();
          if (!nodeUnder(mouse.clientX, mouse.clientY)) openMenuAt(mouse.clientX, mouse.clientY);
        }}
        onDoubleClick={(mouse) => {
          if (!nodeUnder(mouse.clientX, mouse.clientY)) openMenuAt(mouse.clientX, mouse.clientY);
        }}
        onKeyDown={onKeyDown}
      >
        <div className="studio-graph-board" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.zoom})` }}>
          <svg className="studio-graph-wires" aria-hidden>
            {edges.map((edge) => {
              const from = outSpot({ id: edge.from, pin: edge.pin });
              const to = box.get(edge.to);
              if (!from || !to) return null;
              const on = pick?.kind === "edge" && pick.from.id === edge.from && pick.from.pin === edge.pin;
              const d = wirePath(from[0], from[1], to.x + NODE_W / 2, to.y, edge.pin !== "next");
              return (
                <g key={`${edge.from}-${edge.pin}`} className={on ? "on" : ""}>
                  <path className="studio-graph-wire" d={d} />
                  <path
                    className="studio-graph-wire-hit"
                    d={d}
                    onPointerDown={(pointer) => {
                      pointer.stopPropagation();
                      setPick({ kind: "edge", from: { id: edge.from, pin: edge.pin } });
                    }}
                  />
                </g>
              );
            })}
            {valueWires.map((one) => {
              const from = valueSpot(one.root, one.value);
              const to = box.get(one.to);
              if (!from || !to) return null;
              return <path key={one.key} className="studio-graph-value-wire" d={wirePath(from[0], from[1], to.x, to.y + NODE_H / 2, true)} />;
            })}
            {wire && "from" in wire && (() => {
              const from = outSpot(wire.from);
              return from ? <path className="studio-graph-wire live" d={wirePath(from[0], from[1], wire.x, wire.y, wire.from.pin !== "next")} /> : null;
            })()}
            {wire && "value" in wire && (() => {
              const from = valueSpot(wire.root, wire.value);
              return from ? <path className="studio-graph-value-wire live" d={wirePath(from[0], from[1], wire.x, wire.y, true)} /> : null;
            })()}
          </svg>
          {graph.roots.map((root) => {
            const def = defs.find((one) => one.key === root.key);
            const spot = box.get(`@${root.key}`);
            if (!def || !spot) return null;
            const on = pick?.kind === "root" && pick.key === root.key;
            return (
              <div
                key={root.key}
                data-node={`@${root.key}`}
                className={`studio-graph-node root ${on ? "on" : ""}`}
                style={{ left: spot.x, top: spot.y, width: NODE_W, height: NODE_H }}
                role="button"
                tabIndex={0}
                aria-pressed={on}
                onPointerDown={startDrag(`@${root.key}`, spot.x, spot.y)}
                onKeyDown={(key) => {
                  if (key.key === "Enter" || key.key === " ") setPick({ kind: "root", key: root.key });
                }}
              >
                <strong>{def.label}</strong>
                <small>{def.event ? whenSummary(when) : tr("{0} steps", [String(root.chain.length)])}</small>
                <span className="studio-graph-pin out" style={{ left: NODE_W / 2, top: NODE_H }} onPointerDown={startWire({ id: `@${root.key}`, pin: "next" })} aria-hidden />
                {def.event &&
                  values.map((value, index) => (
                    <span key={value} className="studio-graph-value" style={{ left: NODE_W + 4, top: 8 + index * 18 }} onPointerDown={startValue(value, root.key)} data-tip={tr("Drag onto a node to use it in a field")}>
                      <i aria-hidden />
                      {value}
                    </span>
                  ))}
              </div>
            );
          })}
          {nodes.map((node) => {
            const spot = box.get(node.id);
            if (!spot) return null;
            const type = String(node.step.type ?? "");
            const on = pick?.kind === "node" && pick.id === node.id;
            const pins = pinsOf(node.step);
            const loose = rootOf(graph, node.id) === null;
            return (
              <div
                key={node.id}
                data-node={node.id}
                className={`studio-graph-node ${FLOW_TYPES.includes(type as (typeof FLOW_TYPES)[number]) ? "flow" : ""} ${on ? "on" : ""} ${loose ? "loose" : ""}`}
                style={{ left: spot.x, top: spot.y, width: NODE_W, height: NODE_H }}
                role="button"
                tabIndex={0}
                aria-pressed={on}
                onPointerDown={startDrag(node.id, spot.x, spot.y)}
                onKeyDown={(key) => {
                  if (key.key === "Enter" || key.key === " ") setPick({ kind: "node", id: node.id });
                }}
              >
                <span className="studio-graph-pin in" style={{ left: NODE_W / 2, top: 0 }} aria-hidden />
                <strong>{nodeTitle(node.step)}</strong>
                <small>{nodeDetail(node.step) || " "}</small>
                {!ends(node.step) && <span className="studio-graph-pin out" style={{ left: NODE_W / 2, top: NODE_H }} onPointerDown={startWire({ id: node.id, pin: "next" })} aria-hidden />}
                {pins.map((pin) => {
                  const [x, y] = pinSpot(pins, pin);
                  return (
                    <span key={pin} className="studio-graph-pin out side" style={{ left: x, top: y }} onPointerDown={startWire({ id: node.id, pin })}>
                      <em>{pinLabel(type, pin)}</em>
                    </span>
                  );
                })}
              </div>
            );
          })}
        </div>
        <div className="studio-graph-tools" onPointerDown={(pointer) => pointer.stopPropagation()}>
          <button
            type="button"
            className="button ghost small"
            onClick={() => {
              const rect = canvas.current?.getBoundingClientRect();
              if (rect) openMenuAt(rect.left + rect.width / 2 - 120, rect.top + 60);
            }}
          >
            <Icon name="plus" />
            {tr("Add node")}
          </button>
          <button type="button" className="icon-button" aria-label={tr("Zoom out")} data-tip={tr("Zoom out")} onClick={() => zoomBy(1 / 1.2)}>
            <Icon name="minus" />
          </button>
          <span className="studio-graph-zoom">{Math.round(view.zoom * 100)}%</span>
          <button type="button" className="icon-button" aria-label={tr("Zoom in")} data-tip={tr("Zoom in")} onClick={() => zoomBy(1.2)}>
            <Icon name="plus" />
          </button>
          <button type="button" className="icon-button" aria-label={tr("Show everything")} data-tip={tr("Show everything")} onClick={fit}>
            <Icon name="maximize" />
          </button>
        </div>
        {menu && <AddMenu x={menu.x} y={menu.y} expansions={expansions} onPick={add} onClose={() => setMenu(null)} />}
        {valueMenu && (() => {
          const spot = find(graph, valueMenu.id);
          const keys = spot ? textKeys(spot.node.step) : [];
          return (
            <div className="studio-graph-menu small" style={{ left: valueMenu.x, top: valueMenu.y }} onPointerDown={(pointer) => pointer.stopPropagation()} role="menu">
              <p className="studio-graph-menu-head">{tr("Put <{0}> into", [valueMenu.value])}</p>
              {keys.length === 0 && <p className="muted studio-graph-menu-empty">{tr("This node has no text to put it in.")}</p>}
              {keys.map((key) => (
                <button
                  key={key}
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    if (spot) save(setStep(graph, valueMenu.id, { ...spot.node.step, [key]: `${String(spot.node.step[key] ?? "")}<${valueMenu.value}>` }));
                    setValueMenu(null);
                  }}
                >
                  {key}
                </button>
              ))}
            </div>
          );
        })()}
      </div>
      <aside className="studio-nodes-panel" aria-label={tr("Selected node")}>
        {pickedDef ? (
          <>
            <strong>{pickedDef.label}</strong>
            {pickedDef.event && <RuleWhen data={data} onChange={onChange} />}
            {pickedDef.event && values.length > 0 && (
              <p className="studio-placeholders">
                {tr("Values: {0}. Drag one from the node onto another node to put it in a field.", [values.map((one) => `<${one}>`).join(" ")])}
              </p>
            )}
            {!pickedDef.event && <p className="muted studio-nodes-hint">{tr("Wire nodes under it. They run top to bottom.")}</p>}
          </>
        ) : picked ? (
          <>
            <div className="studio-nodes-panel-head">
              <strong>{nodeTitle(picked.node.step)}</strong>
              <span className="studio-step-tools">
                <button type="button" className="icon-button" aria-label={tr("Remove")} data-tip={tr("Remove")} onClick={removePicked}>
                  <Icon name="trash" />
                </button>
              </span>
            </div>
            {rootOf(graph, picked.node.id) === null && <p className="studio-placeholders">{tr("Not wired to anything, so it never runs.")}</p>}
            <StepFields step={picked.node.step} names={names} nested={false} onChange={(next) => save(setStep(graph, picked.node.id, next))} />
          </>
        ) : pick?.kind === "edge" ? (
          <>
            <strong>{tr("Wire")}</strong>
            <button type="button" className="button ghost small" onClick={removePicked}>
              <Icon name="scissors" />
              {tr("Cut this wire")}
            </button>
          </>
        ) : (
          <div className="studio-graph-help">
            <strong>{tr("Node editor")}</strong>
            <p className="muted">{tr("Drag a node to move it. Drag from a dot under a node to wire the next step; drop on empty space to add one there.")}</p>
            <p className="muted">{tr("Right-click or double-click the canvas to add a node. Delete removes the picked node or wire.")}</p>
            <p className="muted">{tr("Scroll to zoom; drag empty space to move around.")}</p>
          </div>
        )}
      </aside>
    </div>
  );
}


/** The searchable list of nodes to add. */
function AddMenu({ x, y, expansions, onPick, onClose }: { x: number; y: number; expansions: Expansion[]; onPick: (step: StudioData) => void; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const shell = useRef<HTMLDivElement>(null);
  const [spot, setSpot] = useState({ x, y });
  useEffect(() => input.current?.focus(), []);
  // Keeps the whole menu inside the canvas (a double-click near an edge would cut it off).
  useLayoutEffect(() => {
    const own = shell.current;
    const room = own?.offsetParent;
    if (!own || !(room instanceof HTMLElement)) return;
    setSpot({ x: Math.max(8, Math.min(x, room.clientWidth - own.offsetWidth - 8)), y: Math.max(8, Math.min(y, room.clientHeight - own.offsetHeight - 8)) });
  }, [x, y]);
  const all: { group: string; label: string; hint: string; make: () => StudioData }[] = [
    ...FLOW_TYPES.map((type) => ({ group: tr("Flow"), label: actionLabel(type), hint: type, make: () => newAction(type) })),
    ...ACTION_TYPES.filter((type) => !(FLOW_TYPES as readonly string[]).includes(type)).map((type) => ({ group: tr("Actions"), label: actionLabel(type, expansions), hint: type, make: () => newAction(type) })),
    ...expansions.flatMap((one) => one.actions.map((step) => ({ group: one.owner, label: step.label, hint: step.type, make: () => newExpansionStep(step) }))),
  ];
  const words = query.trim().toLowerCase();
  const shown = words ? all.filter((one) => one.label.toLowerCase().includes(words) || one.hint.toLowerCase().includes(words)) : all;
  const choose = (index: number) => {
    const one = shown[index];
    if (one) onPick(one.make());
  };
  return (
    <div ref={shell} className="studio-graph-menu" style={{ left: spot.x, top: spot.y }} onPointerDown={(pointer) => pointer.stopPropagation()} onDoubleClick={(mouse) => mouse.stopPropagation()}>
      <input
        ref={input}
        value={query}
        placeholder={tr("Search nodes")}
        aria-label={tr("Search nodes")}
        onChange={(change) => {
          setQuery(change.target.value);
          setActive(0);
        }}
        onKeyDown={(key) => {
          key.stopPropagation();
          if (key.key === "ArrowDown") {
            key.preventDefault();
            setActive((was) => Math.min(shown.length - 1, was + 1));
          } else if (key.key === "ArrowUp") {
            key.preventDefault();
            setActive((was) => Math.max(0, was - 1));
          } else if (key.key === "Enter") {
            key.preventDefault();
            choose(active);
          } else if (key.key === "Escape") {
            onClose();
          }
        }}
      />
      <div className="studio-graph-menu-list" role="listbox" aria-label={tr("Nodes")}>
        {shown.length === 0 && <p className="muted studio-graph-menu-empty">{tr("Nothing matches.")}</p>}
        {shown.map((one, index) => (
          <button
            key={`${one.group}-${one.hint}`}
            type="button"
            role="option"
            aria-selected={index === active}
            className={index === active ? "on" : ""}
            onMouseEnter={() => setActive(index)}
            onClick={() => choose(index)}
          >
            {(index === 0 || shown[index - 1].group !== one.group) && <span className="studio-graph-menu-group">{one.group}</span>}
            <span>{one.label}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

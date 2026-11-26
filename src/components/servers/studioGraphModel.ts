import type { StudioData } from "./StudioPreview";

/*
 * The node editor's model (StudioGraph.tsx). A file's action lists are trees: each step runs after the one before
 * it, and flow steps own lists of their own ("if" runs then/else, a loop its body). The graph shows each step as a
 * node; wires are "next" (the step after) and a flow step's own lists. It saves back to the same JSON, so the add-ons
 * run it as before; only node spots and loose nodes go in "graph", which the add-ons ignore.
 */

export type RootValue = { steps: StudioData[]; conditions?: StudioData[]; otherwise?: StudioData[] };

/** One list that starts something: a rule's actions, an item's "when used", one mob skill... */
export type RootDef = {
  key: string;
  label: string;
  /** Has conditions in front (a rule's "if", an NPC's "only if") that fold into one "if" node. */
  conditions: boolean;
  /** Has a list for when those fail (a rule's "else"). */
  otherwise: boolean;
  /** A rule's trigger: shows the event's values. */
  event: boolean;
  read: (data: StudioData) => RootValue;
  write: (data: StudioData, value: RootValue) => StudioData;
};

export type GNode = { id: string; step: StudioData; kids: Record<string, GNode[]>; x: number; y: number };
export type GRoot = { key: string; chain: GNode[]; x: number; y: number };
export type Graph = { roots: GRoot[]; spares: GNode[][] };
export type Edge = { from: string; pin: string; to: string };
/** Where a wire starts: a root (`@key`) or a node, and which of its outputs. */
export type From = { id: string; pin: string };

export const NODE_W = 216;
export const NODE_H = 60;
const ROW = 92;
const COL = 252;

export const FLOW_TYPES = ["if", "repeat", "each_player", "wait", "cancel", "stop"] as const;

/** The lists a step owns, by output pin. */
export function pinsOf(step: StudioData): string[] {
  switch (step.type) {
    case "if":
      return ["then", "else"];
    case "repeat":
    case "each_player":
      return ["actions"];
    default:
      return [];
  }
}

/** A step nothing runs after. */
export function ends(step: StudioData): boolean {
  return step.type === "stop";
}

function listOf(value: unknown): StudioData[] {
  return Array.isArray(value) ? value.filter((one): one is StudioData => Boolean(one) && typeof one === "object" && !Array.isArray(one)) : [];
}

function objectOf(value: unknown): StudioData {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as StudioData) : {};
}

const rootId = (key: string) => `@${key}`;

let made = 0;
/** A fresh id for a node added in the editor (saved spots use paths, so this only lives until the next save). */
export function freshId(): string {
  made += 1;
  return `new${made}`;
}

function nodeOf(step: StudioData, id: string, at: Record<string, [number, number]>): GNode {
  const own: StudioData = { ...step };
  const kids: Record<string, GNode[]> = {};
  for (const pin of pinsOf(step)) {
    kids[pin] = listOf(step[pin]).map((one, index) => nodeOf(one, `${id}/${pin}:${index}`, at));
    delete own[pin];
  }
  const spot = at[id];
  return { id, step: own, kids, x: spot ? spot[0] : Number.NaN, y: spot ? spot[1] : Number.NaN };
}

/** Lays out nodes that have no saved spot: down the chain, a flow step's lists to its right. */
function place(chain: GNode[], x: number, y: number): { right: number; bottom: number } {
  let right = x + NODE_W;
  let at = y;
  for (const node of chain) {
    if (!Number.isFinite(node.x) || !Number.isFinite(node.y)) {
      node.x = x;
      node.y = at;
    }
    let bodyX = node.x + COL;
    let bottom = node.y;
    for (const pin of Object.keys(node.kids)) {
      if (node.kids[pin].length === 0) continue;
      const made = place(node.kids[pin], bodyX, node.y + ROW);
      bodyX = made.right + 36;
      bottom = Math.max(bottom, made.bottom);
      right = Math.max(right, made.right);
    }
    at = Math.max(node.y + ROW, bottom + ROW);
  }
  return { right, bottom: at - ROW };
}

/** The graph for a file, from its JSON and its saved spots. */
export function readGraph(data: StudioData, defs: RootDef[]): Graph {
  const saved = objectOf(data.graph);
  const at: Record<string, [number, number]> = {};
  for (const [key, value] of Object.entries(objectOf(saved.at))) {
    if (Array.isArray(value) && value.length === 2 && value.every((one) => typeof one === "number" && Number.isFinite(one))) at[key] = [value[0], value[1]];
  }
  let x = 0;
  let bottom = 0;
  const roots = defs.map((def) => {
    const value = def.read(data);
    const conditions = value.conditions ?? [];
    const otherwise = value.otherwise ?? [];
    // Conditions in front fold into one "if" node; its "next" is free to use.
    const steps = def.conditions && (conditions.length > 0 || otherwise.length > 0) ? [{ type: "if", if: conditions, then: value.steps, else: otherwise }] : value.steps;
    const chain = steps.map((one, index) => nodeOf(one, `${def.key}:${index}`, at));
    const spot = at[rootId(def.key)];
    const root: GRoot = { key: def.key, chain, x: spot ? spot[0] : x, y: spot ? spot[1] : 0 };
    const made = place(chain, root.x, root.y + ROW);
    x = Math.max(x, made.right) + 72;
    bottom = Math.max(bottom, made.bottom);
    return root;
  });
  const spares = (Array.isArray(saved.spare) ? saved.spare : []).map((chain, s) => listOf(chain).map((one, index) => nodeOf(one, `~${s}:${index}`, at))).filter((chain) => chain.length > 0);
  let spareX = 0;
  for (const chain of spares) {
    const made = place(chain, spareX, bottom + ROW * 2);
    spareX = made.right + 72;
  }
  return { roots, spares };
}

function json(node: GNode): StudioData {
  const out: StudioData = { ...node.step };
  for (const pin of pinsOf(node.step)) out[pin] = (node.kids[pin] ?? []).map(json);
  return out;
}

function spots(chain: GNode[], path: string, at: Record<string, [number, number]>) {
  chain.forEach((node, index) => {
    const id = `${path}:${index}`;
    at[id] = [Math.round(node.x), Math.round(node.y)];
    for (const pin of pinsOf(node.step)) spots(node.kids[pin] ?? [], `${id}/${pin}`, at);
  });
}

/** The file's JSON for a graph: the lists back in place, spots and loose nodes in "graph". */
export function writeGraph(data: StudioData, graph: Graph, defs: RootDef[]): StudioData {
  let next: StudioData = { ...data };
  const at: Record<string, [number, number]> = {};
  graph.roots.forEach((root) => {
    const def = defs.find((one) => one.key === root.key);
    if (!def) return;
    const steps = root.chain.map(json);
    const only = steps.length === 1 && steps[0].type === "if" ? steps[0] : null;
    const folds = def.conditions && only !== null && (def.otherwise || listOf(only.else).length === 0);
    next = folds && only ? def.write(next, { conditions: listOf(only.if), steps: listOf(only.then), otherwise: listOf(only.else) }) : def.write(next, { conditions: [], steps, otherwise: [] });
    at[rootId(root.key)] = [Math.round(root.x), Math.round(root.y)];
    spots(root.chain, root.key, at);
  });
  const spare = graph.spares.filter((chain) => chain.length > 0);
  spare.forEach((chain, s) => spots(chain, `~${s}`, at));
  next.graph = spare.length ? { at, spare: spare.map((chain) => chain.map(json)) } : { at };
  return next;
}

/** Puts a list under a key, leaving the key out when it was never there and stays empty. */
export function putList(data: StudioData, key: string, list: StudioData[]): StudioData {
  const next = { ...data };
  if (list.length === 0 && !(key in data)) return next;
  next[key] = list;
  return next;
}

// ---------- finding and changing ----------

type Spot = { chain: GNode[]; index: number; node: GNode };

function search(chain: GNode[], id: string): Spot | null {
  for (let index = 0; index < chain.length; index++) {
    const node = chain[index];
    if (node.id === id) return { chain, index, node };
    for (const pin of Object.keys(node.kids)) {
      const found = search(node.kids[pin], id);
      if (found) return found;
    }
  }
  return null;
}

export function find(graph: Graph, id: string): Spot | null {
  for (const root of graph.roots) {
    const found = search(root.chain, id);
    if (found) return found;
  }
  for (const chain of graph.spares) {
    const found = search(chain, id);
    if (found) return found;
  }
  return null;
}

function clone(graph: Graph): Graph {
  const copy = (node: GNode): GNode => ({ ...node, step: { ...node.step }, kids: Object.fromEntries(Object.entries(node.kids).map(([pin, chain]) => [pin, chain.map(copy)])) });
  return { roots: graph.roots.map((root) => ({ ...root, chain: root.chain.map(copy) })), spares: graph.spares.map((chain) => chain.map(copy)) };
}

function ids(chain: GNode[], out: Set<string>) {
  for (const node of chain) {
    out.add(node.id);
    for (const pin of Object.keys(node.kids)) ids(node.kids[pin], out);
  }
}

function tidy(graph: Graph) {
  graph.spares = graph.spares.filter((chain) => chain.length > 0);
}

/** The list a wire's start feeds, and how to replace it. */
function slot(graph: Graph, from: From): { get: () => GNode[]; set: (chain: GNode[]) => void } | null {
  if (from.id.startsWith("@")) {
    const root = graph.roots.find((one) => rootId(one.key) === from.id);
    if (!root || from.pin !== "next") return null;
    return { get: () => root.chain, set: (chain) => (root.chain = chain) };
  }
  const spot = find(graph, from.id);
  if (!spot) return null;
  if (from.pin === "next") {
    if (ends(spot.node.step)) return null;
    return { get: () => spot.chain.slice(spot.index + 1), set: (chain) => spot.chain.splice(spot.index + 1, spot.chain.length, ...chain) };
  }
  if (!pinsOf(spot.node.step).includes(from.pin)) return null;
  return { get: () => spot.node.kids[from.pin] ?? [], set: (chain) => (spot.node.kids[from.pin] = chain) };
}

/**
 * Wires `from` to the node `to`: that node and the ones after it move there; what was there before is left loose.
 * Refused (the graph unchanged) when it would wire a node into itself.
 */
export function connect(graph: Graph, from: From, to: string): Graph {
  const next = clone(graph);
  const target = find(next, to);
  if (!target) return graph;
  const moving = new Set<string>();
  ids(target.chain.slice(target.index), moving);
  if (moving.has(from.id)) return graph;
  const detached = target.chain.splice(target.index);
  const place = slot(next, from);
  if (!place) return graph;
  const before = place.get();
  if (before.length) next.spares.push(before);
  place.set(detached);
  tidy(next);
  return next;
}

/** A new node right after `from` (what came after now follows it), or loose when `from` is null. */
export function insert(graph: Graph, from: From | null, node: GNode): Graph {
  const next = clone(graph);
  if (!from) {
    next.spares.push([node]);
    return next;
  }
  const place = slot(next, from);
  if (!place) {
    next.spares.push([node]);
    return next;
  }
  const after = place.get();
  place.set(ends(node.step) ? [node] : [node, ...after]);
  if (ends(node.step) && after.length) next.spares.push(after);
  return next;
}

/** Cuts the wire out of `from`; what it fed is left loose. */
export function disconnect(graph: Graph, from: From): Graph {
  const next = clone(graph);
  const place = slot(next, from);
  if (!place) return graph;
  const before = place.get();
  if (before.length === 0) return graph;
  place.set([]);
  next.spares.push(before);
  return next;
}

/** Removes a node and its own lists; the nodes after it close up. */
export function remove(graph: Graph, id: string): Graph {
  const next = clone(graph);
  const spot = find(next, id);
  if (!spot) return graph;
  spot.chain.splice(spot.index, 1);
  tidy(next);
  return next;
}

export function setStep(graph: Graph, id: string, step: StudioData): Graph {
  const next = clone(graph);
  const spot = find(next, id);
  if (!spot) return graph;
  const own = { ...step };
  for (const pin of pinsOf(step)) {
    if (!spot.node.kids[pin]) spot.node.kids[pin] = [];
    delete own[pin];
  }
  spot.node.step = own;
  return next;
}

/** Moves a node or a root (`@key`) to a spot. */
export function moveTo(graph: Graph, id: string, x: number, y: number): Graph {
  const next = clone(graph);
  const root = next.roots.find((one) => rootId(one.key) === id);
  if (root) {
    root.x = x;
    root.y = y;
    return next;
  }
  const spot = find(next, id);
  if (!spot) return graph;
  spot.node.x = x;
  spot.node.y = y;
  return next;
}

/** A node for a new step at a spot; a flow step starts with empty lists. */
export function newNode(step: StudioData, x: number, y: number): GNode {
  const kids: Record<string, GNode[]> = {};
  const own = { ...step };
  for (const pin of pinsOf(step)) {
    kids[pin] = [];
    delete own[pin];
  }
  return { id: freshId(), step: own, kids, x, y };
}

/** Every node, roots first then loose ones. */
export function allNodes(graph: Graph): GNode[] {
  const out: GNode[] = [];
  const walk = (chain: GNode[]) => {
    for (const node of chain) {
      out.push(node);
      for (const pin of Object.keys(node.kids)) walk(node.kids[pin]);
    }
  };
  graph.roots.forEach((root) => walk(root.chain));
  graph.spares.forEach(walk);
  return out;
}

export function edgesOf(graph: Graph): Edge[] {
  const out: Edge[] = [];
  const walk = (chain: GNode[], from: From | null) => {
    chain.forEach((node, index) => {
      const before: From | null = index === 0 ? from : { id: chain[index - 1].id, pin: "next" };
      if (before) out.push({ from: before.id, pin: before.pin, to: node.id });
      for (const pin of Object.keys(node.kids)) walk(node.kids[pin], { id: node.id, pin });
    });
  };
  graph.roots.forEach((root) => walk(root.chain, { id: rootId(root.key), pin: "next" }));
  graph.spares.forEach((chain) => walk(chain, null));
  return out;
}

/** Which root a node runs under (null when loose). */
export function rootOf(graph: Graph, id: string): string | null {
  for (const root of graph.roots) if (search(root.chain, id)) return root.key;
  return null;
}

/** The values (placeholders) a rule's event gives its actions, without the angle brackets. */
export function eventValues(event: string): string[] {
  switch (event) {
    case "chat":
      return ["player", "message"];
    case "death":
      return ["player", "killer"];
    case "break":
    case "place":
      return ["player", "block", "x", "y", "z"];
    case "world_change":
      return ["player", "world"];
    case "command":
      return ["player", "command"];
    case "timer":
    case "schedule":
    case "server_start":
      return [];
    case "click":
      return ["player", "button", "block", "x", "y", "z"];
    case "click_entity":
      return ["player", "entity", "entity_name"];
    case "kill":
      return ["player", "entity"];
    case "hit":
      return ["player", "entity", "amount"];
    case "hurt":
      return ["player", "cause", "amount", "attacker"];
    case "eat":
    case "craft":
    case "fish":
    case "drop":
    case "pickup":
      return ["player", "item", "amount"];
    case "level_up":
      return ["player", "level"];
    case "teleport":
      return ["player", "world", "x", "y", "z"];
    case "gamemode":
      return ["player", "gamemode"];
    default:
      return ["player"];
  }
}

/** The text fields of a step a value can go into. */
export function textKeys(step: StudioData): string[] {
  return Object.keys(step).filter((key) => key !== "type" && typeof step[key] === "string");
}

import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { OrbitControls } from "three/examples/jsm/controls/OrbitControls.js";
import { tr } from "../../lib/i18n";

type Face = { uv?: number[]; texture?: number | null; rotation?: number };
/** Mesh faces list their corner vertices and give each one its own UV. */
type MeshFace = { uv?: Record<string, number[]>; vertices?: string[]; texture?: number | null };
type Element = {
  type?: string;
  uuid: string;
  from: number[];
  to: number[];
  vertices?: Record<string, number[]>;
  origin?: number[];
  rotation?: number[];
  inflate?: number;
  visibility?: boolean;
  box_uv?: boolean;
  uv_offset?: number[];
  faces?: Record<string, Face & MeshFace>;
};
type Group = { uuid?: string; name?: string; origin?: number[]; rotation?: number[]; visibility?: boolean; children?: (string | Group)[] };
type Keyframe = { channel?: string; time?: number; interpolation?: string; data_points?: Record<string, number | string>[] };
type Animation = { name?: string; loop?: string; length?: number; animators?: Record<string, { name?: string; keyframes?: Keyframe[] }> };
type Model = {
  meta?: { box_uv?: boolean };
  resolution?: { width?: number; height?: number };
  textures?: { source?: string; uv_width?: number; uv_height?: number; width?: number; height?: number }[];
  elements?: Element[];
  outliner?: (string | Group)[];
  /** Newer files keep groups here and only name them (uuid + children) in the outliner. */
  groups?: Group[];
  animations?: Animation[];
};

/** A bone as built: its node and rest pose, so animations can move it and put it back. */
type Bone = { name: string; node: THREE.Group; position: THREE.Vector3; rotation: number[] };

const SIDES = ["north", "south", "east", "west", "up", "down"] as const;

/** Corners of a face as seen from outside: top-left, top-right, bottom-right, bottom-left (Minecraft UV order). */
function corners(side: (typeof SIDES)[number], [x1, y1, z1]: number[], [x2, y2, z2]: number[]): number[][] {
  switch (side) {
    case "north": return [[x2, y2, z1], [x1, y2, z1], [x1, y1, z1], [x2, y1, z1]];
    case "south": return [[x1, y2, z2], [x2, y2, z2], [x2, y1, z2], [x1, y1, z2]];
    case "east": return [[x2, y2, z2], [x2, y2, z1], [x2, y1, z1], [x2, y1, z2]];
    case "west": return [[x1, y2, z1], [x1, y2, z2], [x1, y1, z2], [x1, y1, z1]];
    case "up": return [[x1, y2, z1], [x2, y2, z1], [x2, y2, z2], [x1, y2, z2]];
    case "down": return [[x1, y1, z2], [x2, y1, z2], [x2, y1, z1], [x1, y1, z1]];
  }
}

/** Box UV layout (Blockbench "box_uv"): faces unfold around uv_offset. */
function boxUv(side: (typeof SIDES)[number], [u, v]: number[], from: number[], to: number[]): number[] {
  const w = Math.abs(to[0] - from[0]), h = Math.abs(to[1] - from[1]), d = Math.abs(to[2] - from[2]);
  switch (side) {
    case "east": return [u, v + d, u + d, v + d + h];
    case "north": return [u + d, v + d, u + d + w, v + d + h];
    case "west": return [u + d + w, v + d, u + d + w + d, v + d + h];
    case "south": return [u + d + w + d, v + d, u + d + w + d + w, v + d + h];
    case "up": return [u + d + w, v + d, u + d, v];
    case "down": return [u + d + w + w, v, u + d + w, v + d];
  }
}

/** Puts a quad's four corners in order around their middle, since mesh faces store them in any order. */
function aroundQuad(points: number[][]): number[] {
  const mid = [0, 1, 2].map((axis) => points.reduce((sum, one) => sum + one[axis], 0) / points.length);
  const sub = (a: number[], b: number[]) => a.map((value, axis) => value - b[axis]);
  const cross = (a: number[], b: number[]) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
  const dot = (a: number[], b: number[]) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
  const normal = cross(sub(points[1], points[0]), sub(points[2], points[0]));
  const across = sub(points[0], mid);
  const side = cross(normal, across);
  return points
    .map((point, at) => ({ at, angle: Math.atan2(dot(sub(point, mid), side), dot(sub(point, mid), across)) }))
    .sort((a, b) => a.angle - b.angle)
    .map((one) => one.at);
}

const radians = (rotation?: number[]) => (rotation ?? [0, 0, 0]).map((one) => (one * Math.PI) / 180);

function build(model: Model, textures: THREE.Texture[]): { root: THREE.Group; skipped: number; bones: Map<string, Bone> } {
  const root = new THREE.Group();
  const width = model.resolution?.width ?? 16;
  const height = model.resolution?.height ?? 16;
  const plain = new THREE.MeshLambertMaterial({ color: 0x9a9a9a, side: THREE.DoubleSide });
  const materials = textures.map(
    (map) => new THREE.MeshLambertMaterial({ map, side: THREE.DoubleSide, alphaTest: 0.05, transparent: true }),
  );
  const byId = new Map((model.elements ?? []).map((one) => [one.uuid, one]));
  const placed = new Set<string>();
  const bones = new Map<string, Bone>();
  const groupsById = new Map((model.groups ?? []).filter((one) => one.uuid).map((one) => [one.uuid as string, one]));
  let skipped = 0;

  const pivot = (parentOrigin: number[], origin: number[] | undefined, rotation: number[] | undefined) => {
    const at = origin ?? [0, 0, 0];
    const node = new THREE.Group();
    node.position.set(at[0] - parentOrigin[0], at[1] - parentOrigin[1], at[2] - parentOrigin[2]);
    const [x, y, z] = radians(rotation);
    node.rotation.set(x, y, z, "ZYX");
    return { node, at };
  };

  const emit = (node: THREE.Object3D, groups: Map<number, { positions: number[]; uvs: number[] }>) => {
    for (const [slot, { positions, uvs }] of groups) {
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
      geometry.setAttribute("uv", new THREE.Float32BufferAttribute(uvs, 2));
      geometry.computeVertexNormals();
      node.add(new THREE.Mesh(geometry, slot >= 0 ? materials[slot] : plain));
    }
  };

  // Mesh vertices are relative to the element's origin, which is also its pivot.
  const addMesh = (parent: THREE.Object3D, parentOrigin: number[], element: Element, vertices: Record<string, number[]>) => {
    const { node } = pivot(parentOrigin, element.origin, element.rotation);
    const groups = new Map<number, { positions: number[]; uvs: number[] }>();
    for (const face of Object.values(element.faces ?? {})) {
      const keys = (face.vertices ?? []).filter((key) => vertices[key]);
      if (keys.length < 3 || keys.length > 4 || face.texture === null) continue;
      const slot = typeof face.texture === "number" && textures[face.texture] ? face.texture : -1;
      const info = model.textures?.[slot];
      const uw = info?.uv_width ?? width;
      const vh = info?.uv_height ?? height;
      const order = keys.length === 4 ? aroundQuad(keys.map((key) => vertices[key])).map((at) => keys[at]) : keys;
      const into = groups.get(slot) ?? { positions: [], uvs: [] };
      for (const at of order.length === 4 ? [0, 1, 2, 0, 2, 3] : [0, 1, 2]) {
        const key = order[at];
        const uv = (face.uv as Record<string, number[]> | undefined)?.[key] ?? [0, 0];
        into.positions.push(...vertices[key]);
        into.uvs.push(uv[0] / uw, 1 - uv[1] / vh);
      }
      groups.set(slot, into);
    }
    emit(node, groups);
    parent.add(node);
  };

  const addElement = (parent: THREE.Object3D, parentOrigin: number[], element: Element) => {
    placed.add(element.uuid);
    if (element.visibility === false) return;
    if (element.type === "mesh" && element.vertices) {
      addMesh(parent, parentOrigin, element, element.vertices);
      return;
    }
    if ((element.type ?? "cube") !== "cube" || !element.from || !element.to) {
      skipped += 1;
      return;
    }
    const grow = element.inflate ?? 0;
    const from = element.from.map((one) => one - grow);
    const to = element.to.map((one) => one + grow);
    const { node, at } = pivot(parentOrigin, element.origin, element.rotation);
    const boxed = element.box_uv ?? model.meta?.box_uv ?? false;
    const groups = new Map<number, { positions: number[]; uvs: number[] }>();
    for (const side of SIDES) {
      const face = element.faces?.[side];
      if (!face || face.texture === null) continue;
      const uv = (Array.isArray(face.uv) ? face.uv : null) ?? (boxed && element.uv_offset ? boxUv(side, element.uv_offset, element.from, element.to) : null);
      if (!uv) continue;
      const slot = typeof face.texture === "number" && textures[face.texture] ? face.texture : -1;
      const info = model.textures?.[slot];
      const uw = info?.uv_width ?? width;
      const vh = info?.uv_height ?? height;
      const points = corners(side, from, to).map(([x, y, z]) => [x - at[0], y - at[1], z - at[2]]);
      const coords = [[uv[0], uv[1]], [uv[2], uv[1]], [uv[2], uv[3]], [uv[0], uv[3]]].map(([u, v]) => [u / uw, 1 - v / vh]);
      const turn = (((face.rotation ?? 0) / 90) % 4 + 4) % 4;
      const turned = coords.map((_, at) => coords[(at - turn + 4) % 4]);
      const into = groups.get(slot) ?? { positions: [], uvs: [] };
      for (const index of [0, 3, 2, 0, 2, 1]) {
        into.positions.push(...points[index]);
        into.uvs.push(...turned[index]);
      }
      groups.set(slot, into);
    }
    emit(node, groups);
    parent.add(node);
  };

  const walk = (parent: THREE.Object3D, parentOrigin: number[], items: (string | Group)[]) => {
    for (let item of items) {
      if (typeof item === "string") {
        const element = byId.get(item);
        if (element) addElement(parent, parentOrigin, element);
        continue;
      }
      const full = item.uuid ? groupsById.get(item.uuid) : undefined;
      if (full) item = { ...full, children: item.children ?? full.children };
      if (item.visibility === false) {
        const hide = (list: (string | Group)[]) => list.forEach((one) => (typeof one === "string" ? placed.add(one) : hide(one.children ?? [])));
        hide(item.children ?? []);
        continue;
      }
      const { node, at } = pivot(parentOrigin, item.origin, item.rotation);
      parent.add(node);
      if (item.uuid) bones.set(item.uuid, { name: item.name ?? "", node, position: node.position.clone(), rotation: radians(item.rotation) });
      walk(node, at, item.children ?? []);
    }
  };
  walk(root, [0, 0, 0], model.outliner ?? []);
  for (const element of model.elements ?? []) if (!placed.has(element.uuid)) addElement(root, [0, 0, 0], element);
  return { root, skipped, bones };
}

const num = (value: number | string | undefined, fallback: number) => {
  const read = typeof value === "number" ? value : Number.parseFloat(value ?? "");
  return Number.isFinite(read) ? read : fallback;
};

/**
 * A channel's value at `time`: held outside the keys, step / linear / Catmull-Rom between. Same rules as
 * `studio_format::bbmodel::sample`, which bakes what the game plays.
 */
function sample(keys: Keyframe[], time: number, fallback: number): number[] | null {
  if (keys.length === 0) return null;
  const value = (key: Keyframe) => ["x", "y", "z"].map((axis) => num(key.data_points?.[0]?.[axis], fallback));
  const at = (key: Keyframe) => key.time ?? 0;
  if (time <= at(keys[0]) || keys.length === 1) return value(keys[0]);
  const next = keys.findIndex((key) => at(key) > time);
  if (next < 0) return value(keys[keys.length - 1]);
  const [a, b] = [keys[next - 1], keys[next]];
  if (a.interpolation === "step") return value(a);
  const span = at(b) - at(a);
  const t = span <= 0 ? 1 : (time - at(a)) / span;
  const [va, vb] = [value(a), value(b)];
  if (a.interpolation === "catmullrom" || b.interpolation === "catmullrom") {
    const p0 = next >= 2 ? value(keys[next - 2]) : va;
    const p3 = next + 1 < keys.length ? value(keys[next + 1]) : vb;
    return va.map((p1, i) => {
      const [q0, p2, q3] = [p0[i], vb[i], p3[i]];
      return 0.5 * (2 * p1 + (p2 - q0) * t + (2 * q0 - 5 * p1 + 4 * p2 - q3) * t * t + (3 * p1 - q0 - 3 * p2 + q3) * t * t * t);
    });
  }
  return va.map((one, i) => one + (vb[i] - one) * t);
}

/** Puts every bone at rest, then moves the animated ones (rotation X/Y and position X mirrored, like Blockbench). */
function poseAt(bones: Map<string, Bone>, animation: Animation | null, time: number) {
  for (const bone of bones.values()) {
    bone.node.position.copy(bone.position);
    bone.node.rotation.set(bone.rotation[0], bone.rotation[1], bone.rotation[2], "ZYX");
    bone.node.scale.set(1, 1, 1);
  }
  if (!animation) return;
  for (const [uuid, animator] of Object.entries(animation.animators ?? {})) {
    const bone = bones.get(uuid) ?? [...bones.values()].find((one) => one.name === animator.name);
    if (!bone) continue;
    const keys = (channel: string) => (animator.keyframes ?? []).filter((one) => one.channel === channel).sort((a, b) => (a.time ?? 0) - (b.time ?? 0));
    const turn = sample(keys("rotation"), time, 0);
    if (turn) bone.node.rotation.set(bone.rotation[0] - (turn[0] * Math.PI) / 180, bone.rotation[1] - (turn[1] * Math.PI) / 180, bone.rotation[2] + (turn[2] * Math.PI) / 180, "ZYX");
    const move = sample(keys("position"), time, 0);
    if (move) bone.node.position.set(bone.position.x - move[0], bone.position.y + move[1], bone.position.z + move[2]);
    const size = sample(keys("scale"), time, 1);
    if (size) bone.node.scale.set(size[0], size[1], size[2]);
  }
}

/** How long an animation runs: its own length, or up to its last key. */
function lengthOf(animation: Animation): number {
  if ((animation.length ?? 0) > 0) return animation.length as number;
  let most = 0;
  for (const animator of Object.values(animation.animators ?? {})) for (const key of animator.keyframes ?? []) most = Math.max(most, key.time ?? 0);
  return most;
}

const reducedMotion = () => typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;

/**
 * Read-only 3D preview of a Blockbench .bbmodel (cubes; mesh elements are skipped). With `animation` it plays that
 * animation; with `time` too it holds that moment instead (also what reduced motion gets: no playback by itself).
 */
export function ModelView({ source, animation = null, time = null }: { source: string; animation?: string | null; time?: number | null }) {
  const host = useRef<HTMLDivElement>(null);
  const live = useRef<{ bones: Map<string, Bone>; model: Model; draw: () => void } | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [skipped, setSkipped] = useState(0);

  useEffect(() => {
    const box = host.current;
    if (!box) return;
    let model: Model;
    try {
      model = JSON.parse(source) as Model;
    } catch {
      setProblem(tr("This model file can't be read as JSON."));
      return;
    }
    setProblem(null);
    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, preserveDrawingBuffer: true });
    renderer.setPixelRatio(window.devicePixelRatio);
    box.appendChild(renderer.domElement);
    const scene = new THREE.Scene();
    scene.add(new THREE.AmbientLight(0xffffff, 1.6));
    const sun = new THREE.DirectionalLight(0xffffff, 1.4);
    sun.position.set(0.4, 1, 0.7);
    scene.add(sun);
    const camera = new THREE.PerspectiveCamera(40, 1, 0.1, 5000);
    const controls = new OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;

    const loader = new THREE.TextureLoader();
    const textures = (model.textures ?? []).map((one) => {
      const texture = one.source?.startsWith("data:image/") ? loader.load(one.source, () => draw()) : new THREE.Texture();
      texture.magFilter = THREE.NearestFilter;
      texture.minFilter = THREE.NearestFilter;
      texture.colorSpace = THREE.SRGBColorSpace;
      return texture;
    });
    const { root, skipped, bones } = build(model, textures);
    setSkipped(skipped);
    scene.add(root);
    root.updateMatrixWorld(true);
    const bounds = new THREE.Box3().setFromObject(root);
    const center = bounds.isEmpty() ? new THREE.Vector3() : bounds.getCenter(new THREE.Vector3());
    const size = bounds.isEmpty() ? 16 : Math.max(...bounds.getSize(new THREE.Vector3()).toArray(), 1);
    controls.target.copy(center);
    camera.position.copy(center).add(new THREE.Vector3(-0.9, 0.7, -1.2).multiplyScalar(size * 1.8));
    controls.update();

    let frame = 0;
    const draw = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        if (controls.update()) draw();
        renderer.render(scene, camera);
      });
    };
    controls.addEventListener("change", draw);
    live.current = { bones, model, draw };
    const fit = () => {
      const width = box.clientWidth || 1;
      const height = box.clientHeight || 1;
      renderer.setSize(width, height);
      camera.aspect = width / height;
      camera.updateProjectionMatrix();
      draw();
    };
    const watch = new ResizeObserver(fit);
    watch.observe(box);
    fit();
    return () => {
      live.current = null;
      watch.disconnect();
      cancelAnimationFrame(frame);
      controls.dispose();
      scene.traverse((one) => {
        if (one instanceof THREE.Mesh) one.geometry.dispose();
      });
      textures.forEach((one) => one.dispose());
      renderer.dispose();
      renderer.domElement.remove();
    };
  }, [source]);

  useEffect(() => {
    const now = live.current;
    if (!now) return;
    const playing = animation ? ((now.model.animations ?? []).find((one) => one.name === animation) ?? null) : null;
    if (!playing || time !== null || reducedMotion()) {
      poseAt(now.bones, playing, time ?? 0);
      now.draw();
      return;
    }
    const length = lengthOf(playing);
    const loop = playing.loop ?? "once";
    const start = performance.now();
    let frame = 0;
    const step = () => {
      const elapsed = (performance.now() - start) / 1000;
      const at = length <= 0 ? 0 : loop === "loop" ? elapsed % length : Math.min(elapsed, length);
      poseAt(now.bones, loop === "once" && elapsed > length ? null : playing, at);
      now.draw();
      if (loop !== "once" || elapsed <= length) frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => {
      cancelAnimationFrame(frame);
      poseAt(now.bones, null, 0);
      now.draw();
    };
  }, [animation, time, source]);

  return (
    <div className="model-view">
      <div className="model-view-stage" ref={host} />
      {(problem || skipped > 0) && (
        <p className="model-view-note">
          {problem ?? tr("{0, plural, one {# mesh element isn't shown} other {# mesh elements aren't shown}}", [skipped])}
        </p>
      )}
    </div>
  );
}

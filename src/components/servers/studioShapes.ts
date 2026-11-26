/**
 * Pack 3D preview: studio blocks, furniture and armor as a Blockbench model, so the files viewer's ModelView can
 * draw and turn them. Pictures are data URLs; a face with no picture is drawn plain grey.
 */

type Face = { uv: number[]; texture?: number; rotation?: number };
type Element = { uuid: string; type: "cube"; from: number[]; to: number[]; origin?: number[]; rotation?: number[]; inflate?: number; faces: Record<string, Face> };
type Texture = { source: string; uv_width: number; uv_height: number };
const SIDES = ["north", "south", "east", "west", "up", "down"] as const;

function model(elements: Element[], textures: Texture[], width = 16, height = 16): string {
  return JSON.stringify({ meta: { box_uv: false }, resolution: { width, height }, textures, elements, outliner: elements.map((one) => one.uuid) });
}

/** The UV a face gets when the model file doesn't say (Minecraft's own rule). */
export function autoUv(side: (typeof SIDES)[number], from: number[], to: number[]): number[] {
  const [x1, y1, z1] = from;
  const [x2, y2, z2] = to;
  switch (side) {
    case "north":
    case "south":
      return [x1, 16 - y2, x2, 16 - y1];
    case "east":
    case "west":
      return [z1, 16 - y2, z2, 16 - y1];
    default:
      return [x1, z1, x2, z2];
  }
}

/** A full block: one picture on the sides, maybe its own top and bottom. */
export function cubeModel(side: string | null, top: string | null = side, bottom: string | null = top): string {
  const textures: Texture[] = [];
  const index = (url: string | null) => {
    if (!url) return undefined;
    const found = textures.findIndex((one) => one.source === url);
    if (found >= 0) return found;
    textures.push({ source: url, uv_width: 16, uv_height: 16 });
    return textures.length - 1;
  };
  const faces: Record<string, Face> = {};
  for (const one of SIDES) faces[one] = { uv: [0, 0, 16, 16], texture: index(one === "up" ? top : one === "down" ? bottom : side) };
  return model([{ uuid: "block", type: "cube", from: [0, 0, 0], to: [16, 16, 16], faces }], textures);
}

/**
 * A flat item as the game draws it in hand: the picture extruded one pixel deep. Each row's runs of solid pixels
 * become one thin box (front and back show the picture, the edges the edge pixel's color). `solid` says which
 * pixels are not see-through; a picture too big to extrude (over 128×128) is shown as a flat card.
 */
export function spriteModel(picture: string | null, width = 16, height = 16, solid?: (x: number, y: number) => boolean): string {
  if (!picture) return model([{ uuid: "card", type: "cube", from: [0, 0, 7.5], to: [16, 16, 8.5], faces: Object.fromEntries(SIDES.map((one) => [one, { uv: [0, 0, 16, 16] }])) }], []);
  const textures: Texture[] = [{ source: picture, uv_width: width, uv_height: height }];
  const scale = 16 / Math.max(width, height, 1);
  const top = 16 - (16 - height * scale) / 2;
  const left = (16 - width * scale) / 2;
  if (!solid || width * height > 128 * 128) {
    const from = [left, top - height * scale, 7.5];
    const to = [left + width * scale, top, 8.5];
    return model([{ uuid: "card", type: "cube", from, to, faces: { south: { uv: [0, 0, width, height], texture: 0 }, north: { uv: [width, 0, 0, height], texture: 0 } } }], textures, width, height);
  }
  const elements: Element[] = [];
  for (let y = 0; y < height; y++) {
    let x = 0;
    while (x < width) {
      if (!solid(x, y)) {
        x++;
        continue;
      }
      const start = x;
      while (x < width && solid(x, y)) x++;
      const strip = [start, y, x, y + 1];
      elements.push({
        uuid: `r${y}-${start}`,
        type: "cube",
        from: [left + start * scale, top - (y + 1) * scale, 8 - scale / 2],
        to: [left + x * scale, top - y * scale, 8 + scale / 2],
        faces: {
          south: { uv: strip, texture: 0 },
          north: { uv: [x, y, start, y + 1], texture: 0 },
          up: { uv: strip, texture: 0 },
          down: { uv: strip, texture: 0 },
          east: { uv: [x - 1, y, x, y + 1], texture: 0 },
          west: { uv: [start, y, start + 1, y + 1], texture: 0 },
        },
      });
    }
  }
  return model(elements, textures, width, height);
}

/** A plant: the picture on two crossed planes, like a flower. */
export function plantModel(picture: string | null): string {
  const textures: Texture[] = picture ? [{ source: picture, uv_width: 16, uv_height: 16 }] : [];
  const texture = picture ? 0 : undefined;
  const plane = (uuid: string, turn: number): Element => ({
    uuid,
    type: "cube",
    from: [0.8, 0, 8],
    to: [15.2, 16, 8],
    origin: [8, 8, 8],
    rotation: [0, turn, 0],
    faces: { north: { uv: [0, 0, 16, 16], texture }, south: { uv: [0, 0, 16, 16], texture } },
  });
  return model([plane("a", 45), plane("b", -45)], textures);
}

type JsonFace = { uv?: number[]; texture?: string; rotation?: number };
type JsonElement = { from?: number[]; to?: number[]; rotation?: { angle?: number; axis?: string; origin?: number[] }; faces?: Record<string, JsonFace> };

/** The studio texture name a model's texture path means (`monolab:studio/ruby` → `ruby`), or null for a game one. */
export function textureName(path: string): string | null {
  const bare = path.replace(/^monolab:/, "");
  if (path.startsWith("minecraft:") || (!path.includes(":") && /^(block|item)\//.test(bare))) return null;
  const name = bare.split("/").pop() ?? "";
  return name || null;
}

/**
 * A block model (Blockbench's Java JSON export): its elements, with `pictures` for the studio textures it names.
 * Texture references (`#0`) are followed to the `textures` map.
 */
export function jsonModel(source: string, pictures: Record<string, string>): string {
  const read = JSON.parse(source) as { textures?: Record<string, string>; elements?: JsonElement[] };
  const map = read.textures ?? {};
  const textures: Texture[] = [];
  const slots = new Map<string, number>();
  const resolve = (ref: string | undefined): number | undefined => {
    let key = ref ?? "";
    for (let hops = 0; key.startsWith("#") && hops < 8; hops += 1) key = map[key.slice(1)] ?? "";
    const name = key ? textureName(key) : null;
    const url = name ? pictures[name] : undefined;
    if (!url) return undefined;
    if (!slots.has(url)) {
      textures.push({ source: url, uv_width: 16, uv_height: 16 });
      slots.set(url, textures.length - 1);
    }
    return slots.get(url);
  };
  const elements: Element[] = (read.elements ?? []).filter((one) => one.from && one.to).map((one, at) => {
    const from = one.from as number[];
    const to = one.to as number[];
    const faces: Record<string, Face> = {};
    for (const side of SIDES) {
      const face = one.faces?.[side];
      if (face) faces[side] = { uv: face.uv ?? autoUv(side, from, to), texture: resolve(face.texture), rotation: face.rotation };
    }
    const turn = one.rotation;
    const angle = turn?.angle ?? 0;
    const rotation = turn ? [turn.axis === "x" ? angle : 0, turn.axis === "y" ? angle : 0, turn.axis === "z" ? angle : 0] : undefined;
    return { uuid: `e${at}`, type: "cube", from, to, origin: turn?.origin ?? [8, 8, 8], rotation, faces };
  });
  return model(elements, textures);
}

/** One box of the game's humanoid armor model: box UV from `at` on the 64×32 armor picture. */
function box(uuid: string, from: number[], to: number[], at: number[], inflate: number, texture: number | undefined): Element {
  const [u, v] = at;
  const [w, h, d] = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
  const uv: Record<string, number[]> = {
    east: [u, v + d, u + d, v + d + h],
    north: [u + d, v + d, u + d + w, v + d + h],
    west: [u + d + w, v + d, u + d + w + d, v + d + h],
    south: [u + d + w + d, v + d, u + d + w + d + w, v + d + h],
    up: [u + d + w, v + d, u + d, v],
    down: [u + d + w + w, v, u + d + w, v + d],
  };
  const faces: Record<string, Face> = {};
  for (const side of SIDES) faces[side] = { uv: uv[side], texture };
  return { uuid, type: "cube", from: from.map((one) => one - inflate), to: to.map((one) => one + inflate), faces };
}

/** A grey stand-in player wearing one armor piece (`head`, `chest`, `legs` or `feet`) drawn from its picture. */
export function armorModel(slot: string, picture: string | null): string {
  const textures: Texture[] = picture ? [{ source: picture, uv_width: 64, uv_height: 32 }] : [];
  const t = picture ? 0 : undefined;
  const parts = {
    head: [[-4, 24, -4], [4, 32, 4], [0, 0]],
    body: [[-4, 12, -2], [4, 24, 2], [16, 16]],
    rightArm: [[-8, 12, -2], [-4, 24, 2], [40, 16]],
    leftArm: [[4, 12, -2], [8, 24, 2], [40, 16]],
    rightLeg: [[-4, 0, -2], [0, 12, 2], [0, 16]],
    leftLeg: [[0, 0, -2], [4, 12, 2], [0, 16]],
  } as const;
  const stand: Element[] = Object.entries(parts).map(([name, [from, to]]) => box(`stand-${name}`, [...from], [...to], [0, 0], 0, undefined));
  const worn: Record<string, (keyof typeof parts)[]> = { head: ["head"], chest: ["body", "rightArm", "leftArm"], legs: ["body", "rightLeg", "leftLeg"], feet: ["rightLeg", "leftLeg"] };
  const grow = slot === "legs" ? 0.5 : 1;
  const armor = (worn[slot] ?? worn.head).map((name) => {
    const [from, to, at] = parts[name];
    return box(`armor-${name}`, [...from], [...to], [...at], grow, t);
  });
  return model([...stand, ...armor], textures, 64, 32);
}

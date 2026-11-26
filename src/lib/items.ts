const TOOL_MATERIALS: Record<string, number> = {
  wooden: 59,
  stone: 131,
  copper: 190,
  iron: 250,
  golden: 32,
  diamond: 1561,
  netherite: 2031,
};
const TOOLS = ["sword", "pickaxe", "axe", "shovel", "hoe"];
const ARMOR_MATERIALS: Record<string, number> = {
  leather: 5,
  copper: 11,
  chainmail: 15,
  iron: 15,
  golden: 7,
  diamond: 33,
  netherite: 37,
};
const ARMOR_PIECES: Record<string, number> = { helmet: 11, chestplate: 16, leggings: 15, boots: 13 };
const OTHERS: Record<string, number> = {
  turtle_helmet: 275,
  bow: 384,
  crossbow: 465,
  trident: 250,
  shield: 336,
  elytra: 432,
  fishing_rod: 64,
  flint_and_steel: 64,
  shears: 238,
  carrot_on_a_stick: 25,
  warped_fungus_on_a_stick: 100,
  brush: 64,
  mace: 500,
  wolf_armor: 64,
};
const SINGLE_LEVEL = new Set([
  "mending",
  "silk_touch",
  "infinity",
  "flame",
  "channeling",
  "multishot",
  "aqua_affinity",
  "binding_curse",
  "vanishing_curse",
]);

export function maxDurability(id: string): number {
  const bare = id.replace(/^minecraft:/, "");
  if (bare in OTHERS) return OTHERS[bare];
  const cut = bare.lastIndexOf("_");
  if (cut < 0) return 0;
  const material = bare.slice(0, cut);
  const kind = bare.slice(cut + 1);
  if (TOOLS.includes(kind) && material in TOOL_MATERIALS) return TOOL_MATERIALS[material];
  if (kind in ARMOR_PIECES && material in ARMOR_MATERIALS) {
    return ARMOR_PIECES[kind] * ARMOR_MATERIALS[material];
  }
  return 0;
}

export function enchantLevel(id: string, level: number): string {
  if (level === 1 && SINGLE_LEVEL.has(id.replace(/^minecraft:/, ""))) return "";
  const numerals = ["", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];
  return numerals[level] ?? String(level);
}

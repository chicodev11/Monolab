import type { StudioData } from "./StudioPreview";

/** The 9 crafting slots of a shaped or shapeless recipe, row by row ("" = empty). */
export function recipeGrid(data: StudioData): string[] {
  const cells = Array.from({ length: 9 }, () => "");
  if ((typeof data.type === "string" ? data.type : "shaped") === "shapeless") {
    const list = Array.isArray(data.ingredients) ? data.ingredients : typeof data.ingredients === "string" ? [data.ingredients] : [];
    list.slice(0, 9).forEach((one, at) => {
      cells[at] = typeof one === "string" ? one.trim() : "";
    });
    return cells;
  }
  const shape = Array.isArray(data.shape) ? data.shape : [];
  const keys = data.keys && typeof data.keys === "object" && !Array.isArray(data.keys) ? (data.keys as StudioData) : {};
  shape.slice(0, 3).forEach((row, y) => {
    const text = typeof row === "string" ? row : "";
    for (let x = 0; x < 3; x += 1) {
      const letter = text[x] ?? " ";
      if (letter === " ") continue;
      const item = keys[letter];
      cells[y * 3 + x] = typeof item === "string" ? item : "";
    }
  });
  return cells;
}

/**
 * The recipe with these 9 slots. Shaped: a 3×3 `shape` (the game trims empty rows and columns, so it is still made
 * anywhere on the table) and one letter per item, keeping the letters the file already had. Shapeless: the
 * ingredients in slot order, without the empty ones at the end.
 */
export function withRecipeGrid(data: StudioData, cells: string[]): StudioData {
  const type = typeof data.type === "string" ? data.type : "shaped";
  if (type === "shapeless") {
    const list = cells.slice(0, 9).map((one) => one.trim());
    while (list.length && !list[list.length - 1]) list.pop();
    return { ...data, ingredients: list };
  }
  const old = data.keys && typeof data.keys === "object" && !Array.isArray(data.keys) ? (data.keys as StudioData) : {};
  const letterOf = new Map<string, string>();
  for (const [letter, item] of Object.entries(old)) {
    if (typeof item === "string" && item && /^[A-Za-z0-9#]$/.test(letter) && !letterOf.has(item)) letterOf.set(item, letter);
  }
  const used = new Set<string>();
  const keys: StudioData = {};
  const letterFor = (item: string): string => {
    let letter = letterOf.get(item);
    if (!letter || (used.has(letter) && keys[letter] !== item)) {
      const bare = item.replace(/^(minecraft|studio):/, "");
      const first = (bare.match(/[a-z]/i)?.[0] ?? "A").toUpperCase();
      const taken = (one: string) => used.has(one) || [...letterOf.values()].includes(one);
      letter = !taken(first) ? first : ("ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("").find((one) => !taken(one)) ?? "#");
      letterOf.set(item, letter);
    }
    used.add(letter);
    keys[letter] = item;
    return letter;
  };
  const filled = cells.some((one) => one.trim());
  const shape = filled
    ? [0, 1, 2].map((y) =>
        [0, 1, 2]
          .map((x) => {
            const item = (cells[y * 3 + x] ?? "").trim();
            return item ? letterFor(item) : " ";
          })
          .join(""),
      )
    : [];
  return { ...data, shape, keys };
}

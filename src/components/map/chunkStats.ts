import { tr } from "../../lib/i18n";
import { fileSize, relativeTime } from "../../lib/format";

export type StatGrid = (number | null)[];
export type StatFetch = (metric: string, rx: number, rz: number) => Promise<StatGrid | null>;

export const METRICS = ["inhabited", "updated", "entities", "blockEntities", "size", "status", "version"] as const;
export type Metric = (typeof METRICS)[number];

export const STATUSES = [
  "empty",
  "structure_starts",
  "structure_references",
  "biomes",
  "noise",
  "surface",
  "carvers",
  "liquid_carvers",
  "features",
  "initialize_light",
  "light",
  "spawn",
  "heightmaps",
  "full",
];

export function metricLabel(metric: Metric) {
  return {
    inhabited: tr("Time players stayed"),
    updated: tr("Last saved"),
    entities: tr("Entities"),
    blockEntities: tr("Block entities"),
    size: tr("Size on disk"),
    status: tr("Generation status"),
    version: tr("Data version"),
  }[metric];
}

export function metricHint(metric: Metric) {
  return {
    inhabited: tr("How long players have been near each chunk, added up (InhabitedTime)."),
    updated: tr("When each chunk was last saved to disk."),
    entities: tr("Mobs, items and other entities saved in each chunk."),
    blockEntities: tr("Chests, furnaces, signs and other block entities in each chunk."),
    size: tr("How much space each chunk takes in the region file."),
    status: tr("How far each chunk got in world generation. full means finished."),
    version: tr("The DataVersion each chunk was last saved with. Older numbers are from older Minecraft versions."),
  }[metric];
}

export function formatValue(metric: Metric, value: number) {
  switch (metric) {
    case "inhabited":
      return value < 60 ? tr("{0} min", [Math.round(value)]) : tr("{0} h", [(value / 60).toFixed(1)]);
    case "updated":
      return relativeTime(value);
    case "size":
      return fileSize(value);
    case "status":
      return STATUSES[value] ?? String(value);
    default:
      return Math.round(value).toLocaleString();
  }
}

export type Scale = { metric: Metric; min: number; max: number; values: number[] };

export function makeScale(metric: Metric, grids: StatGrid[]): Scale | null {
  let min = Infinity;
  let max = -Infinity;
  const seen = new Set<number>();
  for (const grid of grids) {
    for (const value of grid) {
      if (value === null) continue;
      if (value < min) min = value;
      if (value > max) max = value;
      if (metric === "version" && seen.size < 64) seen.add(value);
    }
  }
  if (!Number.isFinite(min)) return null;
  if (metric === "inhabited" || metric === "entities" || metric === "blockEntities" || metric === "size") min = 0;
  if (metric === "status") {
    min = 0;
    max = STATUSES.length - 1;
  }
  return { metric, min, max, values: [...seen].sort((a, b) => a - b) };
}

function ratio(scale: Scale, value: number) {
  const { metric, min, max } = scale;
  if (metric === "version") {
    const at = scale.values.indexOf(value);
    return scale.values.length > 1 ? Math.max(0, at) / (scale.values.length - 1) : 1;
  }
  if (max <= min) return 1;
  if (metric === "inhabited" || metric === "entities" || metric === "blockEntities" || metric === "size") {
    return Math.log1p(value - min) / Math.log1p(max - min);
  }
  return (value - min) / (max - min);
}

export function colorOf(scale: Scale, value: number, alpha = 0.62) {
  const t = Math.min(1, Math.max(0, ratio(scale, value)));
  return `hsla(${Math.round(240 - 240 * t)}, 85%, 55%, ${alpha})`;
}

export function gradient() {
  return `linear-gradient(90deg, ${[0, 0.25, 0.5, 0.75, 1].map((t) => `hsl(${Math.round(240 - 240 * t)}, 85%, 55%)`).join(", ")})`;
}

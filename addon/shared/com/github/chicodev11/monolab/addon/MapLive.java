package com.github.chicodev11.monolab.addon;

import java.util.ArrayDeque;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Keeps the map up to date between saves: on the server thread it samples loaded chunks near players a few at a
 * time, and hands the ones whose surface changed to {@link MapRender}, which patches them into the tiles.
 */
final class MapLive {
    interface Reader {
        int minY();

        /** The y of the highest non-air block in the column, or {@link Integer#MIN_VALUE} if there is none. */
        int top(int x, int z);

        String block(int x, int y, int z);

        /** True when water is in this block (a water block or a waterlogged one). */
        boolean water(int x, int y, int z);

        String biome(int x, int y, int z);

        int light(int x, int y, int z);
    }

    interface Source {
        /** Players as {dimension id, block x, block z}. */
        List<Object[]> players();

        /** A loaded chunk, or null; must never load or generate one. */
        Reader chunk(String dimension, int cx, int cz);
    }

    static final class Columns {
        final String[] top = new String[256];
        final String[] floor = new String[256];
        final String[] biome = new String[256];
        final short[] height = new short[256];
        final int[] depth = new int[256];
        final int[] light = new int[256];
        final boolean[] empty = new boolean[256];
        long hash = 1125899906842597L;
    }

    static final int RADIUS = 6;
    private static final long BUDGET_NANOS = 600_000L;
    private static final long REFILL_MS = 2_000L;

    private final MapRender map;
    private final Map<String, Long> seen = new ConcurrentHashMap<>();
    private final ArrayDeque<Object[]> queue = new ArrayDeque<>();
    private long filledAt;
    private boolean broken;

    MapLive(MapRender map) {
        this.map = map;
    }

    /** Called every tick on the server thread. */
    void tick(Source source) {
        if (broken || !AddonConfig.on("map") || !AddonConfig.on("live-map")) {
            return;
        }
        try {
            long start = System.nanoTime();
            if (queue.isEmpty()) {
                long now = System.currentTimeMillis();
                if (now - filledAt < REFILL_MS) {
                    return;
                }
                filledAt = now;
                fill(source.players());
            }
            while (!queue.isEmpty() && System.nanoTime() - start < BUDGET_NANOS) {
                Object[] job = queue.poll();
                String layer = (String) job[0];
                if (!map.knows(layer)) {
                    continue;
                }
                int cx = (Integer) job[2];
                int cz = (Integer) job[3];
                Reader reader = source.chunk((String) job[1], cx, cz);
                if (reader == null) {
                    continue;
                }
                Columns columns = sample(reader, layer.equals("nether") ? 127 : Integer.MAX_VALUE);
                String key = layer + "|" + cx + "|" + cz;
                Long before = seen.put(key, columns.hash);
                if (before == null || before != columns.hash) {
                    map.live(layer, cx, cz, columns);
                }
            }
        } catch (RuntimeException | LinkageError trouble) {
            broken = true;
            map.trouble("Live map updates aren't available on this server; the map updates after saves instead.", trouble);
        }
    }

    /** The disk pass redrew this region from saved chunks, so the next sample must patch it again if it differs. */
    void forget(String layer, int rx, int rz) {
        String prefix = layer + "|";
        seen.keySet().removeIf(key -> {
            if (!key.startsWith(prefix)) {
                return false;
            }
            String[] bits = key.substring(prefix.length()).split("\\|");
            return bits.length == 2 && Math.floorDiv(Integer.parseInt(bits[0]), 32) == rx && Math.floorDiv(Integer.parseInt(bits[1]), 32) == rz;
        });
    }

    static String layerOf(String dimension) {
        if (dimension == null || dimension.isEmpty()) {
            return null;
        }
        return switch (dimension) {
            case "minecraft:overworld" -> "overworld";
            case "minecraft:the_nether" -> "nether";
            case "minecraft:the_end" -> "end";
            default -> {
                String key = dimension.toLowerCase(java.util.Locale.ROOT).replace(':', '.').replace('/', '.').replaceAll("[^a-z0-9_.-]", "_");
                yield key.length() <= 96 ? key : null;
            }
        };
    }

    private void fill(List<Object[]> players) {
        Set<String> added = new LinkedHashSet<>();
        for (int ring = 0; ring <= RADIUS; ring++) {
            for (Object[] player : players) {
                String dimension = (String) player[0];
                String layer = layerOf(dimension);
                if (layer == null) {
                    continue;
                }
                int pcx = Math.floorDiv((Integer) player[1], 16);
                int pcz = Math.floorDiv((Integer) player[2], 16);
                for (int dx = -ring; dx <= ring; dx++) {
                    for (int dz = -ring; dz <= ring; dz++) {
                        if (Math.max(Math.abs(dx), Math.abs(dz)) != ring) {
                            continue;
                        }
                        int cx = pcx + dx;
                        int cz = pcz + dz;
                        if (added.add(layer + "|" + cx + "|" + cz)) {
                            queue.add(new Object[] {layer, dimension, cx, cz});
                        }
                    }
                }
            }
        }
    }

    private Columns sample(Reader reader, int cutoff) {
        boolean ceiling = cutoff != Integer.MAX_VALUE;
        int floorY = reader.minY();
        Columns out = new Columns();
        for (int z = 0; z < 16; z++) {
            for (int x = 0; x < 16; x++) {
                int at = z * 16 + x;
                int start = reader.top(x, z);
                if (start == Integer.MIN_VALUE) {
                    out.empty[at] = !ceiling;
                    out.hash = 31 * out.hash + (ceiling ? 1 : 2);
                    continue;
                }
                boolean open = !ceiling;
                int y = start;
                if (ceiling) {
                    if (start < cutoff - 1) {
                        open = true;
                    } else {
                        y = cutoff - 1;
                    }
                }
                String top = null;
                String floor = null;
                int topY = Integer.MIN_VALUE;
                int water = 0;
                for (; y >= floorY; y--) {
                    String name = reader.block(x, y, z);
                    boolean fluid = reader.water(x, y, z) || MapRender.alwaysWet(name);
                    boolean visible = map.visible(name);
                    if (!open) {
                        if (!visible && !fluid) {
                            open = true;
                        }
                        continue;
                    }
                    if (topY == Integer.MIN_VALUE) {
                        if (fluid) {
                            topY = y;
                            top = "minecraft:water";
                            water = 1;
                        } else if (visible) {
                            topY = y;
                            top = name;
                            break;
                        }
                    } else if (fluid) {
                        water++;
                    } else if (visible) {
                        floor = name;
                        break;
                    }
                }
                if (top == null) {
                    out.empty[at] = !ceiling;
                    out.hash = 31 * out.hash + (ceiling ? 3 : 4);
                    continue;
                }
                out.top[at] = top;
                out.floor[at] = floor;
                out.biome[at] = reader.biome(x, topY, z);
                out.height[at] = (short) topY;
                out.depth[at] = Math.min(water, 254);
                out.light[at] = reader.light(x, topY + 1, z);
                out.hash = 31 * out.hash + top.hashCode();
                out.hash = 31 * out.hash + (floor == null ? 0 : floor.hashCode());
                out.hash = 31 * out.hash + out.biome[at].hashCode();
                out.hash = 31 * out.hash + topY;
                out.hash = 31 * out.hash + out.depth[at];
                out.hash = 31 * out.hash + out.light[at];
            }
        }
        return out;
    }
}

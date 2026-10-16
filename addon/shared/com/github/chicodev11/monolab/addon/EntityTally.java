package com.github.chicodev11.monolab.addon;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

final class EntityTally {
    static final int MOST_KINDS = 1000;

    static final int MOST_HOTSPOTS = 6;

    static final int HOTSPOT_FLOOR = 10;

    private record WorldRow(String name, long chunks, long entities) {}

    private final Map<String, Long> kinds = new HashMap<>();
    private final Map<String, Map<String, Integer>> chunks = new HashMap<>();
    private final List<WorldRow> worlds = new ArrayList<>();
    private long total;

    void add(String world, String type, int chunkX, int chunkZ) {
        total++;
        String kind = type == null || type.isBlank() ? "unknown" : type;
        kinds.merge(kind, 1L, Long::sum);
        String key = (world == null ? "" : world) + '|' + chunkX + '|' + chunkZ;
        chunks.computeIfAbsent(key, ignored -> new HashMap<>()).merge(kind, 1, Integer::sum);
    }

    void world(String name, long chunks, long entities) {
        worlds.add(new WorldRow(name, chunks, entities));
    }

    long total() {
        return total;
    }

    String kindsJson() {
        List<Map.Entry<String, Long>> rows = new ArrayList<>(kinds.entrySet());
        rows.sort((a, b) -> Long.compare(b.getValue(), a.getValue()));
        StringBuilder out = new StringBuilder("[");
        int shown = 0;
        for (Map.Entry<String, Long> row : rows) {
            if (shown++ >= MOST_KINDS) {
                break;
            }
            if (out.length() > 1) {
                out.append(',');
            }
            out.append(new Json().put("type", row.getKey()).put("count", row.getValue()).end());
        }
        return out.append(']').toString();
    }

    String hotspotsJson() {
        List<Map.Entry<String, Map<String, Integer>>> rows = new ArrayList<>(chunks.entrySet());
        rows.sort((a, b) -> Integer.compare(sum(b.getValue()), sum(a.getValue())));
        StringBuilder out = new StringBuilder("[");
        int shown = 0;
        for (Map.Entry<String, Map<String, Integer>> row : rows) {
            int count = sum(row.getValue());
            if (count < HOTSPOT_FLOOR || shown++ >= MOST_HOTSPOTS) {
                break;
            }
            String[] key = row.getKey().split("\\|", -1);
            String top = null;
            int most = 0;
            for (Map.Entry<String, Integer> kind : row.getValue().entrySet()) {
                if (kind.getValue() > most) {
                    most = kind.getValue();
                    top = kind.getKey();
                }
            }
            if (out.length() > 1) {
                out.append(',');
            }
            out.append(new Json()
                    .put("world", key[0].isEmpty() ? null : key[0])
                    .put("x", Long.parseLong(key[1]))
                    .put("z", Long.parseLong(key[2]))
                    .put("count", count)
                    .put("top", top)
                    .end());
        }
        return out.append(']').toString();
    }

    String worldsJson() {
        StringBuilder out = new StringBuilder("[");
        for (WorldRow row : worlds) {
            if (out.length() > 1) {
                out.append(',');
            }
            out.append(new Json()
                    .put("name", row.name())
                    .put("chunks", row.chunks())
                    .put("entities", row.entities())
                    .end());
        }
        return out.append(']').toString();
    }

    private static int sum(Map<String, Integer> counts) {
        int total = 0;
        for (int one : counts.values()) {
            total += one;
        }
        return total;
    }
}

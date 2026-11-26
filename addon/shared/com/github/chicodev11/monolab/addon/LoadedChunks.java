package com.github.chicodev11.monolab.addon;

import java.util.LinkedHashMap;
import java.util.Map;

final class LoadedChunks {
    static final int EVERY_SECONDS = 5;

    private final Map<String, StringBuilder[]> worlds = new LinkedHashMap<>();

    void add(String world, int x, int z, boolean forced) {
        StringBuilder[] lists = worlds.computeIfAbsent(world == null ? "" : world, ignored -> new StringBuilder[] {new StringBuilder(), new StringBuilder()});
        StringBuilder list = forced ? lists[1] : lists[0];
        if (list.length() > 0) {
            list.append(',');
        }
        list.append(x).append(',').append(z);
    }

    String json() {
        StringBuilder out = new StringBuilder("{\"at\":").append(System.currentTimeMillis()).append(",\"worlds\":[");
        boolean first = true;
        for (Map.Entry<String, StringBuilder[]> one : worlds.entrySet()) {
            if (!first) {
                out.append(',');
            }
            first = false;
            String quoted = new Json().put("w", one.getKey()).end();
            out.append("{\"world\":").append(quoted, 5, quoted.length() - 1)
                    .append(",\"chunks\":[").append(one.getValue()[0]).append("],\"forced\":[").append(one.getValue()[1]).append("]}");
        }
        return out.append("]}").toString();
    }
}

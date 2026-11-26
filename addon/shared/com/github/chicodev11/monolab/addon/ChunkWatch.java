package com.github.chicodev11.monolab.addon;

import java.util.Collection;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Function;

final class ChunkWatch {
    private final Map<String, Set<Long>> worlds = new ConcurrentHashMap<>();
    private int beat;

    static long pack(int x, int z) {
        return (x & 0xffffffffL) | ((z & 0xffffffffL) << 32);
    }

    void load(String world, int x, int z) {
        worlds.computeIfAbsent(world, ignored -> ConcurrentHashMap.newKeySet()).add(pack(x, z));
    }

    void unload(String world, int x, int z) {
        Set<Long> one = worlds.get(world);
        if (one != null) {
            one.remove(pack(x, z));
        }
    }

    boolean due() {
        return beat++ % LoadedChunks.EVERY_SECONDS == 0;
    }

    String json(Function<String, Collection<Long>> forcedOf) {
        LoadedChunks list = new LoadedChunks();
        for (Map.Entry<String, Set<Long>> world : worlds.entrySet()) {
            Collection<Long> forced = forcedOf.apply(world.getKey());
            if (forced == null) {
                forced = List.of();
            }
            for (long at : world.getValue()) {
                list.add(world.getKey(), (int) at, (int) (at >>> 32), forced.contains(at));
            }
        }
        return list.json();
    }
}

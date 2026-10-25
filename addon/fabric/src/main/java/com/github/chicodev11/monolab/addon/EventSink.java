package com.github.chicodev11.monolab.addon;

import java.nio.file.Path;
import java.util.List;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;
import java.util.function.BiConsumer;

final class EventSink {
    static final int LIVE_TICKS = 20;

    static final int PERF_TICKS = 20 * 60;

    record Spot(
            String name,
            String world,
            double x,
            double y,
            double z,
            int health,
            int food,
            int ping) {}

    private EventLog log;

    private ScheduledExecutorService flusher;

    private long ticks;

    boolean off() {
        return log == null;
    }

    long tick() {
        return ++ticks;
    }

    void start(Path folder, BiConsumer<String, Throwable> warn) {
        log = new EventLog(folder, warn);
        flusher = Executors.newSingleThreadScheduledExecutor(runnable -> {
            Thread thread = new Thread(runnable, "Monolab-flush");
            thread.setDaemon(true);
            return thread;
        });
        flusher.scheduleAtFixedRate(this::flushQuietly, 1, 1, TimeUnit.SECONDS);
    }

    void stop() {
        if (log == null) {
            return;
        }
        log.setLive(new Json()
                .put("at", System.currentTimeMillis())
                .raw("players", "[]")
                .end());
        log.flush();
        if (flusher != null) {
            flusher.shutdownNow();
        }
    }

    private void flushQuietly() {
        if (log != null) {
            log.flush();
        }
    }

    void live(double mspt, long chunks, EntityTally tally, List<Spot> people) {
        if (log == null) {
            return;
        }
        Runtime runtime = Runtime.getRuntime();
        StringBuilder out = new StringBuilder("[");
        boolean head = true;
        for (Spot one : people) {
            if (!head) {
                out.append(',');
            }
            head = false;
            out.append(new Json()
                    .put("name", one.name())
                    .put("world", one.world())
                    .put("x", one.x())
                    .put("y", one.y())
                    .put("z", one.z())
                    .put("health", one.health())
                    .put("food", one.food())
                    .put("ping", one.ping())
                    .end());
        }
        out.append(']');
        log.setLive(new Json()
                .put("at", System.currentTimeMillis())
                .put("tps", round(tps(mspt)))
                .put("mspt", round(mspt))
                .put("memoryMib", usedMib(runtime))
                .put("maxMemoryMib", runtime.maxMemory() / (1024 * 1024))
                .put("chunks", chunks)
                .put("entities", tally.total())
                .raw("entityKinds", tally.kindsJson())
                .raw("hotspots", tally.hotspotsJson())
                .raw("worlds", tally.worldsJson())
                .raw("players", out.toString())
                .end());
    }

    void loaded(String json) {
        if (log != null) {
            log.setLoaded(json);
        }
    }

    void inventories(String json) {
        if (log != null) {
            log.setInventories(json);
        }
    }

    void perf(double mspt, int players, long chunks, long entities) {
        add(new Json()
                .put("at", System.currentTimeMillis())
                .put("kind", "perf")
                .put("tps", round(tps(mspt)))
                .put("mspt", round(mspt))
                .put("memoryMib", usedMib(Runtime.getRuntime()))
                .put("players", players)
                .put("chunks", chunks)
                .put("entities", entities));
    }

    static String worldName(String levelName, String dimension) {
        if (levelName == null || !dimension.startsWith("minecraft:")) {
            return dimension;
        }
        return switch (dimension.substring("minecraft:".length())) {
            case "overworld" -> levelName;
            case "the_nether" -> levelName + "_nether";
            case "the_end" -> levelName + "_the_end";
            default -> dimension;
        };
    }

    private void add(Json line) {
        if (log != null) {
            log.add(line.end());
        }
    }

    private static double tps(double mspt) {
        return mspt <= 0 ? 20.0 : Math.min(20.0, 1000.0 / mspt);
    }

    private static double round(double value) {
        return Math.round(value * 100.0) / 100.0;
    }

    private static long usedMib(Runtime runtime) {
        return (runtime.totalMemory() - runtime.freeMemory()) / (1024 * 1024);
    }
}

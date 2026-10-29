package com.github.chicodev11.monolab.addon;

import java.lang.management.GarbageCollectorMXBean;
import java.lang.management.ManagementFactory;
import java.lang.management.MemoryPoolMXBean;
import java.lang.management.MemoryType;
import java.lang.management.MemoryUsage;
import java.lang.management.ThreadMXBean;

/** JVM details for the app's advanced monitor: collectors, memory pools, threads, CPU, tick spread. */
final class JvmStats {
    private static final int TICKS = 1200;
    private static final long[] ticks = new long[TICKS];
    private static int tickCount;
    private static long tickStart;

    private JvmStats() {}

    static synchronized void tickStart() {
        tickStart = System.nanoTime();
    }

    static synchronized void tickEnd() {
        if (tickStart != 0) {
            tick(System.nanoTime() - tickStart);
            tickStart = 0;
        }
    }

    static synchronized void tick(long nanos) {
        ticks[tickCount++ % TICKS] = nanos;
    }

    static String json() {
        if (!AddonConfig.on("jvm-stats")) {
            return "null";
        }
        Json out = new Json().put("uptimeMs", ManagementFactory.getRuntimeMXBean().getUptime());
        out.put("vm", System.getProperty("java.vm.name", "") + " " + System.getProperty("java.version", ""));
        StringBuilder gcs = new StringBuilder("[");
        for (GarbageCollectorMXBean gc : ManagementFactory.getGarbageCollectorMXBeans()) {
            if (gcs.length() > 1) {
                gcs.append(',');
            }
            gcs.append(new Json()
                    .put("name", gc.getName())
                    .put("count", Math.max(0, gc.getCollectionCount()))
                    .put("ms", Math.max(0, gc.getCollectionTime()))
                    .end());
        }
        out.raw("gc", gcs.append(']').toString());
        StringBuilder pools = new StringBuilder("[");
        for (MemoryPoolMXBean pool : ManagementFactory.getMemoryPoolMXBeans()) {
            MemoryUsage usage = pool.getUsage();
            if (usage == null) {
                continue;
            }
            if (pools.length() > 1) {
                pools.append(',');
            }
            pools.append(new Json()
                    .put("name", pool.getName())
                    .put("heap", pool.getType() == MemoryType.HEAP)
                    .put("usedMib", usage.getUsed() / (1024 * 1024))
                    .put("maxMib", usage.getMax() < 0 ? -1 : usage.getMax() / (1024 * 1024))
                    .end());
        }
        out.raw("pools", pools.append(']').toString());
        ThreadMXBean threads = ManagementFactory.getThreadMXBean();
        out.put("threads", threads.getThreadCount())
                .put("daemonThreads", threads.getDaemonThreadCount())
                .put("peakThreads", threads.getPeakThreadCount())
                .put("classes", ManagementFactory.getClassLoadingMXBean().getLoadedClassCount());
        try {
            if (ManagementFactory.getOperatingSystemMXBean() instanceof com.sun.management.OperatingSystemMXBean os) {
                double process = os.getProcessCpuLoad();
                if (process >= 0) {
                    out.put("processCpu", process * 100.0);
                }
            }
        } catch (LinkageError | RuntimeException ignored) {
            // Not a HotSpot-style JVM; CPU comes from the app instead.
        }
        String spread = tickSpread();
        if (spread != null) {
            out.raw("ticks", spread);
        }
        return out.end();
    }

    private static synchronized String tickSpread() {
        int size = Math.min(tickCount, TICKS);
        if (size == 0) {
            return null;
        }
        long[] sorted = java.util.Arrays.copyOf(ticks, size);
        java.util.Arrays.sort(sorted);
        return new Json()
                .put("p50", sorted[size / 2] / 1_000_000.0)
                .put("p95", sorted[Math.min(size - 1, (int) Math.ceil(size * 0.95) - 1)] / 1_000_000.0)
                .put("max", sorted[size - 1] / 1_000_000.0)
                .put("samples", size)
                .end();
    }
}

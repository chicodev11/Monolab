package com.github.chicodev11.monolab.addon;

import java.nio.file.Path;
import java.util.logging.Level;
import org.bukkit.Location;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.server.ServerLoadEvent;
import org.bukkit.command.Command;
import org.bukkit.command.SimpleCommandMap;
import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import java.util.concurrent.TimeUnit;
import org.bukkit.plugin.java.JavaPlugin;

public final class MonolabPlugin extends JavaPlugin implements Listener {
    private static final int MOST_ARGS = 40;

    private static final long LIVE_TICKS = 20L;

    private static final long PERF_TICKS = 20L * 60L;

    private EventLog log;
    private ScheduledTask flusher;

    private ScheduledTask liveTask;
    private ScheduledTask perfTask;
    private MapRender map;
    private int liveCount;

    @Override
    public void onEnable() {
        log = new EventLog(
                eventFolder(),
                (message, error) -> getLogger().log(Level.WARNING, message, error));
        getServer().getPluginManager().registerEvents(this, this);
        flusher =
                getServer()
                        .getAsyncScheduler()
                        .runAtFixedRate(this, task -> log.flush(), 1, 1, TimeUnit.SECONDS);
        liveTask =
                getServer()
                        .getGlobalRegionScheduler()
                        .runAtFixedRate(this, task -> snapshot(), LIVE_TICKS, LIVE_TICKS);
        perfTask =
                getServer()
                        .getGlobalRegionScheduler()
                        .runAtFixedRate(this, task -> logPerf(), PERF_TICKS, PERF_TICKS);
        startMap();
        getLogger().info("Starting the Monolab monitor: " + eventFolder());
    }

    @Override
    public void onDisable() {
        if (flusher != null) {
            flusher.cancel();
        }
        if (liveTask != null) {
            liveTask.cancel();
        }
        if (perfTask != null) {
            perfTask.cancel();
        }
        if (map != null) {
            map.stop();
        }
        if (log == null) {
            return;
        }
        log.setLive(new Json()
                .put("at", System.currentTimeMillis())
                .raw("players", "[]")
                .end());
        log.flush();
    }

    private void startMap() {
        map = new MapRender(
                getServer().getWorldContainer().toPath().toAbsolutePath(),
                eventFolder().resolve("map"),
                (message, error) -> getLogger().log(Level.WARNING, message, error));
        for (org.bukkit.Material material : org.bukkit.Material.values()) {
            try {
                if (!material.isBlock()) {
                    continue;
                }
                org.bukkit.block.data.BlockData data = material.createBlockData();
                String key = material.getKey().toString();
                map.color(key, data.getMapColor().asRGB());
                if (data instanceof org.bukkit.block.data.Orientable orientable) {
                    for (org.bukkit.Axis axis : orientable.getAxes()) {
                        org.bukkit.block.data.Orientable turned = (org.bukkit.block.data.Orientable) data.clone();
                        turned.setAxis(axis);
                        map.color("Block{" + key + "}[axis=" + axis.name().toLowerCase(java.util.Locale.ROOT) + "]",
                                turned.getMapColor().asRGB());
                    }
                }
            } catch (RuntimeException skipped) {
                // Legacy materials with no block data.
            }
        }
        map.game(getServer().getMinecraftVersion());
        map.start();
    }

    private Path eventFolder() {
        return getDataFolder().toPath();
    }

    @EventHandler
    public void onLoaded(ServerLoadEvent event) {
        try {
            dumpCommands();
        } catch (Throwable trouble) {
            getLogger().log(Level.WARNING, "Couldn't read the command list", trouble);
        }
    }

    private void dumpCommands() {
        if (!(getServer().getCommandMap() instanceof SimpleCommandMap map)) {
            return;
        }
        var console = getServer().getConsoleSender();
        var out = new java.util.ArrayList<String>();
        for (var entry : map.getKnownCommands().entrySet()) {
            String key = entry.getKey();
            if (key.indexOf(':') >= 0) {
                continue;
            }
            Command command = entry.getValue();
            java.util.List<String> args = java.util.List.of();
            try {
                var found = map.tabComplete(console, key + " ");
                if (found != null) {
                    args = found.size() > MOST_ARGS ? found.subList(0, MOST_ARGS) : found;
                }
            } catch (Throwable ignored) {
                // Some commands complain when asked from the console. Then only the name is passed.
            }
            String usage = command.getUsage();
            if (usage != null && usage.startsWith("/")) {
                usage = usage.substring(1);
            }
            out.add(new Json()
                    .put("name", key)
                    .put("usage", usage == null || usage.isBlank() ? null : usage)
                    .put("from", sourceOf(command))
                    .put("args", args)
                    .end());
        }
        CommandDump.write(
                eventFolder(),
                "commands",
                out,
                (message, error) -> getLogger().log(Level.WARNING, message, error));
    }

    private String sourceOf(Command command) {
        if (command instanceof org.bukkit.command.PluginCommand plugin) {
            return plugin.getPlugin().getName();
        }
        return null;
    }

    private void logPerf() {
        Runtime runtime = Runtime.getRuntime();
        long usedMib = (runtime.totalMemory() - runtime.freeMemory()) / (1024 * 1024);
        log.add(new Json()
                .put("at", System.currentTimeMillis())
                .put("kind", "perf")
                .put("tps", Math.round(getServer().getTPS()[0] * 100.0) / 100.0)
                .put("mspt", Math.round(getServer().getAverageTickTime() * 100.0) / 100.0)
                .put("memoryMib", usedMib)
                .put("players", getServer().getOnlinePlayers().size())
                .put("chunks", loadedChunks())
                .put("entities", loadedEntities())
                .end());
    }

    private void snapshot() {
        Runtime runtime = Runtime.getRuntime();
        long usedMib = (runtime.totalMemory() - runtime.freeMemory()) / (1024 * 1024);
        long maxMib = runtime.maxMemory() / (1024 * 1024);
        StringBuilder people = new StringBuilder("[");
        boolean head = true;
        for (Player player : getServer().getOnlinePlayers()) {
            if (!head) {
                people.append(',');
            }
            head = false;
            Location spot = player.getLocation();
            Json one = new Json().put("name", player.getName());
            if (spot.getWorld() != null) {
                one.put("world", spot.getWorld().getName());
            }
            people.append(one.put("x", spot.getX())
                    .put("y", spot.getY())
                    .put("z", spot.getZ())
                    .put("health", Math.round(player.getHealth()))
                    .put("food", player.getFoodLevel())
                    .put("ping", player.getPing())
                    .end());
        }
        people.append(']');
        EntityTally tally = tallyEntities();
        if (liveCount++ % LoadedChunks.EVERY_SECONDS == 0) {
            log.setLoaded(loadedChunkList());
        }
        log.setLive(new Json()
                .put("at", System.currentTimeMillis())
                .put("tps", Math.round(getServer().getTPS()[0] * 100.0) / 100.0)
                .put("mspt", Math.round(getServer().getAverageTickTime() * 100.0) / 100.0)
                .put("memoryMib", usedMib)
                .put("maxMemoryMib", maxMib)
                .put("chunks", loadedChunks())
                .put("entities", tally.total())
                .raw("entityKinds", tally.kindsJson())
                .raw("hotspots", tally.hotspotsJson())
                .raw("worlds", tally.worldsJson())
                .raw("players", people.toString())
                .end());
    }

    private EntityTally tallyEntities() {
        EntityTally tally = new EntityTally();
        for (org.bukkit.World world : getServer().getWorlds()) {
            java.util.List<org.bukkit.entity.Entity> entities = world.getEntities();
            tally.world(world.getName(), world.getLoadedChunks().length, entities.size());
            for (org.bukkit.entity.Entity entity : entities) {
                Location spot = entity.getLocation();
                tally.add(world.getName(), typeId(entity), spot.getBlockX() >> 4, spot.getBlockZ() >> 4);
            }
        }
        return tally;
    }

    private static String typeId(org.bukkit.entity.Entity entity) {
        org.bukkit.entity.EntityType type = entity.getType();
        try {
            return type.getKey().toString();
        } catch (RuntimeException noKey) {
            return type.name().toLowerCase(java.util.Locale.ROOT);
        }
    }

    private String loadedChunkList() {
        LoadedChunks list = new LoadedChunks();
        for (org.bukkit.World world : getServer().getWorlds()) {
            for (org.bukkit.Chunk chunk : world.getLoadedChunks()) {
                list.add(world.getName(), chunk.getX(), chunk.getZ(), chunk.isForceLoaded());
            }
        }
        return list.json();
    }

    private long loadedChunks() {
        long total = 0;
        for (org.bukkit.World world : getServer().getWorlds()) {
            total += world.getLoadedChunks().length;
        }
        return total;
    }

    private long loadedEntities() {
        long total = 0;
        for (org.bukkit.World world : getServer().getWorlds()) {
            total += world.getEntities().size();
        }
        return total;
    }
}

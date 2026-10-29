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
    private ScheduledTask mapTask;
    private MapRender map;
    private int liveCount;
    private boolean chunkListBroken;
    private boolean inventoriesBroken;

    @Override
    public void onEnable() {
        log = new EventLog(
                eventFolder(),
                (message, error) -> getLogger().log(Level.WARNING, message, error));
        getServer().getPluginManager().registerEvents(this, this);
        MonolabApi.serverThread(task -> getServer().getGlobalRegionScheduler().execute(this, task));
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
        if (mapTask != null) {
            mapTask.cancel();
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
        // The block registry, not Material.values(): touching LEGACY_* materials makes Paper load legacy support.
        for (org.bukkit.block.BlockType type : io.papermc.paper.registry.RegistryAccess.registryAccess()
                .getRegistry(io.papermc.paper.registry.RegistryKey.BLOCK)) {
            try {
                org.bukkit.block.data.BlockData data = type.createBlockData();
                String key = type.getKey().toString();
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
                // A block this server can't make data for.
            }
        }
        map.game(getServer().getMinecraftVersion());
        map.start();
        if (!folia()) {
            PaperChunks source = new PaperChunks();
            MapRender running = map;
            mapTask = getServer().getGlobalRegionScheduler().runAtFixedRate(this, task -> running.live().tick(source), 1L, 1L);
        }
    }

    private static boolean folia() {
        try {
            Class.forName("io.papermc.paper.threadedregions.RegionizedServer");
            return true;
        } catch (ClassNotFoundException paper) {
            return false;
        }
    }

    /** Live map samples; Folia has no single thread that may touch every chunk, so it only gets the saved-file map. */
    private final class PaperChunks implements MapLive.Source {
        private final java.util.Map<org.bukkit.Material, Object[]> materials = new java.util.EnumMap<>(org.bukkit.Material.class);
        private final java.util.Map<org.bukkit.block.Biome, String> biomes = new java.util.IdentityHashMap<>();

        @Override
        public java.util.List<Object[]> players() {
            java.util.List<Object[]> out = new java.util.ArrayList<>();
            for (org.bukkit.World world : getServer().getWorlds()) {
                String dimension = world.getKey().toString();
                for (Player player : world.getPlayers()) {
                    Location spot = player.getLocation();
                    out.add(new Object[] {dimension, spot.getBlockX(), spot.getBlockZ()});
                }
            }
            return out;
        }

        @Override
        public MapLive.Reader chunk(String dimension, int cx, int cz) {
            for (org.bukkit.World world : getServer().getWorlds()) {
                if (!dimension.equals(world.getKey().toString())) {
                    continue;
                }
                if (!world.isChunkLoaded(cx, cz)) {
                    return null;
                }
                org.bukkit.ChunkSnapshot snap = world.getChunkAt(cx, cz).getChunkSnapshot(true, true, false);
                int minY = world.getMinHeight();
                int maxY = world.getMaxHeight();
                return new MapLive.Reader() {
                    @Override
                    public int minY() {
                        return minY;
                    }

                    @Override
                    public int top(int x, int z) {
                        int y = Math.min(snap.getHighestBlockYAt(x, z), maxY - 1);
                        return y < minY ? Integer.MIN_VALUE : y;
                    }

                    private Object[] kind(int x, int y, int z) {
                        return materials.computeIfAbsent(snap.getBlockType(x, y, z), material -> {
                            boolean wet = false;
                            try {
                                wet = material.isBlock() && material.createBlockData() instanceof org.bukkit.block.data.Waterlogged;
                            } catch (RuntimeException legacy) {
                                // No block data for this material.
                            }
                            return new Object[] {material.getKey().toString(), wet};
                        });
                    }

                    @Override
                    public String block(int x, int y, int z) {
                        return (String) kind(x, y, z)[0];
                    }

                    @Override
                    public boolean water(int x, int y, int z) {
                        return (Boolean) kind(x, y, z)[1]
                                && snap.getBlockData(x, y, z) instanceof org.bukkit.block.data.Waterlogged logged
                                && logged.isWaterlogged();
                    }

                    @Override
                    public String biome(int x, int y, int z) {
                        return biomes.computeIfAbsent(snap.getBiome(x, y, z), biome -> biome.getKey().toString());
                    }

                    @Override
                    public int light(int x, int y, int z) {
                        return y < minY || y >= maxY ? 0 : snap.getBlockEmittedLight(x, y, z);
                    }
                };
            }
            return null;
        }
    }

    private Path eventFolder() {
        return getDataFolder().toPath();
    }

    @EventHandler
    public void onTickEnd(com.destroystokyo.paper.event.server.ServerTickEndEvent event) {
        JvmStats.tick((long) (event.getTickDuration() * 1_000_000.0));
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
        if (!AddonConfig.on("performance-log")) {
            return;
        }
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
        if (!inventoriesBroken && liveCount % LoadedChunks.EVERY_SECONDS == 0 && AddonConfig.on("inventories")) {
            try {
                log.setInventories(inventories());
            } catch (RuntimeException | LinkageError trouble) {
                inventoriesBroken = true;
                getLogger().log(Level.INFO, "Player inventories aren't available on this server; skipping them.", trouble);
            }
        }
        if (!chunkListBroken && liveCount++ % LoadedChunks.EVERY_SECONDS == 0) {
            try {
                log.setLoaded(loadedChunkList());
            } catch (RuntimeException unsupported) {
                chunkListBroken = true;
                getLogger().log(Level.INFO, "Loaded chunk list isn't available on this server; skipping it.", unsupported);
            }
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
                .raw("jvm", JvmStats.json())
                .end());
    }

private String inventories() {
        Inventories all = new Inventories();
        for (Player player : getServer().getOnlinePlayers()) {
            all.player(player.getName(), player.getUniqueId().toString());
            org.bukkit.inventory.PlayerInventory bag = player.getInventory();
            for (int at = 0; at < bag.getSize(); at++) {
                org.bukkit.inventory.ItemStack stack = bag.getItem(at);
                if (stack != null && !stack.getType().isAir()) {
                    all.bag(at, stack.getType().getKey().toString(), stack.getAmount(), damageOf(stack));
                }
            }
            org.bukkit.inventory.Inventory ender = player.getEnderChest();
            for (int at = 0; at < ender.getSize(); at++) {
                org.bukkit.inventory.ItemStack stack = ender.getItem(at);
                if (stack != null && !stack.getType().isAir()) {
                    all.ender(at, stack.getType().getKey().toString(), stack.getAmount(), damageOf(stack));
                }
            }
        }
        return all.json();
    }

    private static int damageOf(org.bukkit.inventory.ItemStack stack) {
        if (stack.hasItemMeta() && stack.getItemMeta() instanceof org.bukkit.inventory.meta.Damageable meta) {
            return meta.getDamage();
        }
        return 0;
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

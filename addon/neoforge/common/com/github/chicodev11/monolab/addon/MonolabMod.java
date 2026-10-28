package com.github.chicodev11.monolab.addon;

import java.nio.file.Path;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.Entity;
import net.neoforged.api.distmarker.Dist;
import net.neoforged.bus.api.SubscribeEvent;
import net.neoforged.fml.common.Mod;
import net.neoforged.fml.loading.FMLPaths;
import net.neoforged.neoforge.common.NeoForge;
import net.neoforged.neoforge.event.server.ServerStartedEvent;
import net.neoforged.neoforge.event.server.ServerStoppingEvent;
import net.neoforged.neoforge.event.tick.ServerTickEvent;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

@Mod(value = MonolabMod.ID, dist = {Dist.DEDICATED_SERVER})
public final class MonolabMod {
    // Must match {@code modId} in {@code neoforge.mods.toml}.
    public static final String ID = "monolab";

    private static final int LIVE_TICKS = 20;

    private static final int PERF_TICKS = 20 * 60;

    private static final Logger LOGGER = LoggerFactory.getLogger("Monolab");
    private boolean inventoriesBroken;

    public MonolabMod() {
        NeoForge.EVENT_BUS.register(this);
    }

    private EventLog log;
    private ScheduledExecutorService flusher;
    private int ticks;
    private MapRender map;
    private MojangChunks liveChunks;
    private final ChunkWatch chunks = new ChunkWatch();

    @SubscribeEvent
    public void onChunkLoad(net.neoforged.neoforge.event.level.ChunkEvent.Load event) {
        if (event.getLevel() instanceof ServerLevel level) {
            net.minecraft.world.level.ChunkPos pos = event.getChunk().getPos();
            chunks.load(worldName(level.getServer(), Bridge.dimensionId(level)), pos.getMinBlockX() >> 4, pos.getMinBlockZ() >> 4);
        }
    }

    @SubscribeEvent
    public void onChunkUnload(net.neoforged.neoforge.event.level.ChunkEvent.Unload event) {
        if (event.getLevel() instanceof ServerLevel level) {
            net.minecraft.world.level.ChunkPos pos = event.getChunk().getPos();
            chunks.unload(worldName(level.getServer(), Bridge.dimensionId(level)), pos.getMinBlockX() >> 4, pos.getMinBlockZ() >> 4);
        }
    }

    @SuppressWarnings("unchecked")
    private static java.util.Collection<Long> forcedOf(ServerLevel level) {
        for (String name : new String[] {"getForceLoadedChunks", "getForcedChunks"}) {
            try {
                Object got = ServerLevel.class.getMethod(name).invoke(level);
                if (got instanceof java.util.Collection<?> set) {
                    return (java.util.Collection<Long>) set;
                }
            } catch (ReflectiveOperationException | RuntimeException missing) {
                // Older and newer versions name this differently; try the other name.
            }
        }
        return java.util.List.of();
    }

    @SubscribeEvent
    public void onStarted(ServerStartedEvent event) {
        Path folder = FMLPaths.CONFIGDIR.get().resolve("monolab");
        log = new EventLog(folder, (message, error) -> LOGGER.warn(message, error));
        flusher = Executors.newSingleThreadScheduledExecutor(runnable -> {
            Thread thread = new Thread(runnable, "Monolab-flush");
            thread.setDaemon(true);
            return thread;
        });
        flusher.scheduleAtFixedRate(this::flushQuietly, 1, 1, TimeUnit.SECONDS);
        ticks = 0;
        LOGGER.info("Starting the Monolab monitor: {}", folder);
        dumpCommands(event.getServer(), folder);
        try {
            map = startMap(folder);
            liveChunks = new MojangChunks(event.getServer(), Bridge::dimensionId);
        } catch (Throwable trouble) {
            LOGGER.warn("Couldn't start the map", trouble);
        }
    }

    private MapRender startMap(Path folder) {
        MapRender map = new MapRender(
                Path.of("").toAbsolutePath(),
                folder.resolve("map"),
                (message, error) -> LOGGER.warn(message, error));
        for (net.minecraft.world.level.block.Block block : net.minecraft.core.registries.BuiltInRegistries.BLOCK) {
            for (net.minecraft.world.level.block.state.BlockState state : block.getStateDefinition().getPossibleStates()) {
                try {
                    map.color(state.toString(),
                            state.getMapColor(net.minecraft.world.level.EmptyBlockGetter.INSTANCE, net.minecraft.core.BlockPos.ZERO).col);
                } catch (Throwable skipped) {
                    // A modded block that needs a world to know its color — it's simply left off the map.
                }
            }
        }
        map.game(net.neoforged.fml.ModList.get().getModContainerById("minecraft")
                .map(one -> one.getModInfo().getVersion().toString()).orElse(""));
        map.start();
        return map;
    }

    private void dumpCommands(MinecraftServer server, Path folder) {
        try {
            CommandDump.write(
                    folder,
                    "nodes",
                    CommandTree.read(
                            server.getCommands().getDispatcher(),
                            server.createCommandSourceStack()),
                    (message, error) -> LOGGER.warn(message, error));
        } catch (Throwable trouble) {
            LOGGER.warn("Couldn't read the command list", trouble);
        }
    }

    @SubscribeEvent
    public void onStopping(ServerStoppingEvent event) {
        if (map != null) {
            map.stop();
            map = null;
            liveChunks = null;
        }
        if (log == null) {
            return;
        }
        if (flusher != null) {
            flusher.shutdownNow();
            flusher = null;
        }
        log.setLive(new Json()
                .put("at", System.currentTimeMillis())
                .raw("players", "[]")
                .end());
        log.flush();
        log = null;
    }

    @SubscribeEvent
    public void onTick(ServerTickEvent.Post event) {
        if (log == null) {
            return;
        }
        ticks++;
        if (map != null && liveChunks != null) {
            map.live().tick(liveChunks);
        }
        if (ticks % LIVE_TICKS == 0) {
            snapshot(event.getServer());
        }
        if (ticks % PERF_TICKS == 0) {
            logPerf(event.getServer());
        }
    }

    private void logPerf(MinecraftServer server) {
        Runtime runtime = Runtime.getRuntime();
        long usedMib = (runtime.totalMemory() - runtime.freeMemory()) / (1024 * 1024);
        double mspt = server.getAverageTickTimeNanos() / 1_000_000.0;
        double tps = mspt <= 0 ? 20.0 : Math.min(20.0, 1000.0 / mspt);
        log.add(new Json()
                .put("at", System.currentTimeMillis())
                .put("kind", "perf")
                .put("tps", Math.round(tps * 100.0) / 100.0)
                .put("mspt", Math.round(mspt * 100.0) / 100.0)
                .put("memoryMib", usedMib)
                .put("players", server.getPlayerList().getPlayers().size())
                .put("chunks", loadedChunks(server))
                .put("entities", tallyEntities(server).total())
                .end());
    }

    private static long loadedChunks(MinecraftServer server) {
        long chunks = 0;
        for (ServerLevel level : server.getAllLevels()) {
            chunks += level.getChunkSource().getLoadedChunksCount();
        }
        return chunks;
    }

    private static EntityTally tallyEntities(MinecraftServer server) {
        EntityTally tally = new EntityTally();
        for (ServerLevel level : server.getAllLevels()) {
            String world = worldName(server, Bridge.dimensionId(level));
            long before = tally.total();
            for (Entity entity : level.getAllEntities()) {
                tally.add(
                        world,
                        typeId(entity),
                        ((int) Math.floor(entity.getX())) >> 4,
                        ((int) Math.floor(entity.getZ())) >> 4);
            }
            tally.world(world, level.getChunkSource().getLoadedChunksCount(), tally.total() - before);
        }
        return tally;
    }

private static String itemId(net.minecraft.world.item.ItemStack stack) {
        Object key = net.minecraft.core.registries.BuiltInRegistries.ITEM.getKey(stack.getItem());
        return key == null ? null : key.toString();
    }

    private static String inventories(MinecraftServer server) {
        Inventories all = new Inventories();
        for (ServerPlayer player : server.getPlayerList().getPlayers()) {
            all.player(player.getScoreboardName(), player.getUUID().toString());
            var bag = player.getInventory();
            for (int at = 0; at < bag.getContainerSize(); at++) {
                var stack = bag.getItem(at);
                if (!stack.isEmpty()) {
                    all.bag(at, itemId(stack), stack.getCount(), stack.getDamageValue());
                }
            }
            var ender = player.getEnderChestInventory();
            for (int at = 0; at < ender.getContainerSize(); at++) {
                var stack = ender.getItem(at);
                if (!stack.isEmpty()) {
                    all.ender(at, itemId(stack), stack.getCount(), stack.getDamageValue());
                }
            }
        }
        return all.json();
    }

    private static String typeId(Entity entity) {
        Object key = net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getKey(entity.getType());
        return key == null ? null : key.toString();
    }

    private void snapshot(MinecraftServer server) {
        Runtime runtime = Runtime.getRuntime();
        long usedMib = (runtime.totalMemory() - runtime.freeMemory()) / (1024 * 1024);
        long maxMib = runtime.maxMemory() / (1024 * 1024);

        EntityTally tally = tallyEntities(server);
        if (chunks.due()) {
            java.util.Map<String, ServerLevel> byName = new java.util.HashMap<>();
            for (ServerLevel level : server.getAllLevels()) {
                byName.put(worldName(server, Bridge.dimensionId(level)), level);
            }
            log.setLoaded(chunks.json(name -> byName.containsKey(name) ? forcedOf(byName.get(name)) : null));
            if (!inventoriesBroken) {
                try {
                    log.setInventories(inventories(server));
                } catch (Throwable trouble) {
                    inventoriesBroken = true;
                    LOGGER.warn("Couldn't read player inventories; skipping them.", trouble);
                }
            }
        }

        StringBuilder people = new StringBuilder("[");
        boolean head = true;
        for (ServerPlayer player : server.getPlayerList().getPlayers()) {
            if (!head) {
                people.append(',');
            }
            head = false;
            people.append(new Json()
                    .put("name", player.getScoreboardName())
                    .put("world", worldName(server, Bridge.dimensionId(Bridge.levelOf(player))))
                    .put("x", player.getX())
                    .put("y", player.getY())
                    .put("z", player.getZ())
                    .put("health", Math.round(player.getHealth()))
                    .put("food", player.getFoodData().getFoodLevel())
                    .put("ping", player.connection.latency())
                    .end());
        }
        people.append(']');

        double mspt = server.getAverageTickTimeNanos() / 1_000_000.0;
        double tps = mspt <= 0 ? 20.0 : Math.min(20.0, 1000.0 / mspt);

        log.setLive(new Json()
                .put("at", System.currentTimeMillis())
                .put("tps", Math.round(tps * 100.0) / 100.0)
                .put("mspt", Math.round(mspt * 100.0) / 100.0)
                .put("memoryMib", usedMib)
                .put("maxMemoryMib", maxMib)
                .put("chunks", loadedChunks(server))
                .put("entities", tally.total())
                .raw("entityKinds", tally.kindsJson())
                .raw("hotspots", tally.hotspotsJson())
                .raw("worlds", tally.worldsJson())
                .raw("players", people.toString())
                .end());
    }

    private void flushQuietly() {
        EventLog current = log;
        if (current == null) {
            return;
        }
        try {
            current.flush();
        } catch (RuntimeException error) {
            LOGGER.warn("Something went wrong while flushing records.", error);
        }
    }

    private static String worldName(MinecraftServer server, String dimension) {
        if (server == null || !dimension.startsWith("minecraft:")) {
            return dimension;
        }
        String level = server.getWorldData().getLevelName();
        return switch (dimension.substring("minecraft:".length())) {
            case "overworld" -> level;
            case "the_nether" -> level + "_nether";
            case "the_end" -> level + "_the_end";
            default -> dimension;
        };
    }
}

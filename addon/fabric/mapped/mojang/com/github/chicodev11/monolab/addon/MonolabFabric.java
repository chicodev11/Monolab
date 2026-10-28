package com.github.chicodev11.monolab.addon;

import java.util.ArrayList;
import java.util.List;
import net.fabricmc.api.DedicatedServerModInitializer;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerLifecycleEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.Entity;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

public final class MonolabFabric implements DedicatedServerModInitializer {
    private static final Logger LOGGER = LoggerFactory.getLogger("Monolab");

    private final EventSink sink = new EventSink();
    private MapRender map;
    private MojangChunks liveChunks;
    private final ChunkWatch chunks = new ChunkWatch();
    private boolean inventoriesBroken;

    @Override
    public void onInitializeServer() {
        ServerLifecycleEvents.SERVER_STARTED.register(this::onStarted);
        ServerLifecycleEvents.SERVER_STOPPING.register(server -> {
            sink.stop();
            if (map != null) {
                map.stop();
                map = null;
                liveChunks = null;
            }
        });
        ServerTickEvents.END_SERVER_TICK.register(this::onTick);
        net.fabricmc.fabric.api.event.lifecycle.v1.ServerChunkEvents.CHUNK_LOAD.register((world, chunk, generated) ->
                chunks.load(Bridge.worldIdOf(world), chunk.getPos().getMinBlockX() >> 4, chunk.getPos().getMinBlockZ() >> 4));
        net.fabricmc.fabric.api.event.lifecycle.v1.ServerChunkEvents.CHUNK_UNLOAD.register((world, chunk) ->
                chunks.unload(Bridge.worldIdOf(world), chunk.getPos().getMinBlockX() >> 4, chunk.getPos().getMinBlockZ() >> 4));
    }

    private void onStarted(MinecraftServer server) {
        var folder = FabricLoader.getInstance().getConfigDir().resolve("monolab");
        sink.start(folder, LOGGER::warn);
        LOGGER.info("Starting the Monolab monitor: {}", folder);
        dumpCommands(server, folder);
        try {
            map = startMap(folder);
            liveChunks = new MojangChunks(server, level -> level.dimension().identifier().toString());
        } catch (Throwable trouble) {
            LOGGER.warn("Couldn't start the map", trouble);
        }
    }

    private MapRender startMap(java.nio.file.Path folder) {
        MapRender map = new MapRender(
                java.nio.file.Path.of("").toAbsolutePath(),
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
        map.game(net.fabricmc.loader.api.FabricLoader.getInstance().getModContainer("minecraft")
                .map(one -> one.getMetadata().getVersion().getFriendlyString()).orElse(""));
        map.start();
        return map;
    }

    private void dumpCommands(MinecraftServer server, java.nio.file.Path folder) {
        try {
            CommandDump.write(
                    folder,
                    "nodes",
                    CommandTree.read(
                            server.getCommands().getDispatcher(),
                            server.createCommandSourceStack()),
                    (message, trouble) -> LOGGER.warn(message, trouble));
        } catch (Throwable trouble) {
            LOGGER.warn("Couldn't read the command list", trouble);
        }
    }

    private void onTick(MinecraftServer server) {
        if (map != null && liveChunks != null) {
            map.live().tick(liveChunks);
        }
        if (sink.off()) {
            return;
        }
        long ticks = sink.tick();
        if (ticks % EventSink.LIVE_TICKS == 0) {
            snapshot(server);
        }
        if (ticks % EventSink.PERF_TICKS == 0) {
            EntityTally tally = tallyEntities(server);
            sink.perf(mspt(server), server.getPlayerList().getPlayers().size(), loadedChunks(server), tally.total());
        }
    }

    private void snapshot(MinecraftServer server) {
        if (chunks.due()) {
            sink.loaded(chunks.json(name -> null));
            if (!inventoriesBroken) {
                try {
                    sink.inventories(inventories(server));
                } catch (Throwable trouble) {
                    inventoriesBroken = true;
                    LOGGER.warn("Couldn't read player inventories; skipping them.", trouble);
                }
            }
        }
        EntityTally tally = tallyEntities(server);
        List<EventSink.Spot> people = new ArrayList<>();
        for (ServerPlayer player : server.getPlayerList().getPlayers()) {
            people.add(new EventSink.Spot(
                    Bridge.nameOf(player),
                    Bridge.worldIdOf(player),
                    player.getX(),
                    player.getY(),
                    player.getZ(),
                    Math.round(player.getHealth()),
                    player.getFoodData().getFoodLevel(),
                    player.connection.latency()));
        }
        sink.live(mspt(server), loadedChunks(server), tally, people);
    }

private static String itemId(net.minecraft.world.item.ItemStack stack) {
        Object key = net.minecraft.core.registries.BuiltInRegistries.ITEM.getKey(stack.getItem());
        return key == null ? null : key.toString();
    }

    private static String inventories(MinecraftServer server) {
        Inventories all = new Inventories();
        for (ServerPlayer player : server.getPlayerList().getPlayers()) {
            all.player(Bridge.nameOf(player), player.getUUID().toString());
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
            String world = Bridge.worldIdOf(level);
            long before = tally.total();
            for (Entity entity : level.getAllEntities()) {
                Object key = net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getKey(entity.getType());
                tally.add(
                        world,
                        key == null ? null : key.toString(),
                        ((int) Math.floor(entity.getX())) >> 4,
                        ((int) Math.floor(entity.getZ())) >> 4);
            }
            tally.world(world, level.getChunkSource().getLoadedChunksCount(), tally.total() - before);
        }
        return tally;
    }

    private static double mspt(MinecraftServer server) {
        return server.getAverageTickTimeNanos() / 1_000_000.0;
    }
}

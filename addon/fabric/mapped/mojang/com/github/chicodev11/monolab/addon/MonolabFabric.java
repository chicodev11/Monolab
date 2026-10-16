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

    @Override
    public void onInitializeServer() {
        ServerLifecycleEvents.SERVER_STARTED.register(this::onStarted);
        ServerLifecycleEvents.SERVER_STOPPING.register(server -> {
            sink.stop();
            if (map != null) {
                map.stop();
                map = null;
            }
        });
        ServerTickEvents.END_SERVER_TICK.register(this::onTick);
    }

    private void onStarted(MinecraftServer server) {
        var folder = FabricLoader.getInstance().getConfigDir().resolve("monolab");
        sink.start(folder, LOGGER::warn);
        LOGGER.info("Starting the Monolab monitor: {}", folder);
        dumpCommands(server, folder);
        try {
            map = startMap(folder);
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

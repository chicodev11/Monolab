package com.github.chicodev11.monolab.addon;

import java.util.ArrayList;
import java.util.List;
import net.fabricmc.api.DedicatedServerModInitializer;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerLifecycleEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.entity.Entity;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.network.ServerPlayerEntity;
import net.minecraft.server.world.ServerWorld;
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
        for (net.minecraft.block.Block block : net.minecraft.registry.Registries.BLOCK) {
            for (net.minecraft.block.BlockState state : block.getStateManager().getStates()) {
                try {
                    map.color(state.toString(),
                            state.getMapColor(net.minecraft.world.EmptyBlockView.INSTANCE, net.minecraft.util.math.BlockPos.ORIGIN).color);
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
                            server.getCommandManager().getDispatcher(),
                            server.getCommandSource()),
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
            sink.perf(
                    server.getAverageTickTime(),
                    server.getPlayerManager().getPlayerList().size(),
                    loadedChunks(server),
                    tally.total());
        }
    }

    private void snapshot(MinecraftServer server) {
        EntityTally tally = tallyEntities(server);
        List<EventSink.Spot> people = new ArrayList<>();
        for (ServerPlayerEntity player : server.getPlayerManager().getPlayerList()) {
            people.add(new EventSink.Spot(
                    Bridge.nameOf(player),
                    Bridge.worldIdOf(player),
                    player.getX(),
                    player.getY(),
                    player.getZ(),
                    Math.round(player.getHealth()),
                    player.getHungerManager().getFoodLevel(),
                    player.networkHandler.getLatency()));
        }
        sink.live(server.getAverageTickTime(), loadedChunks(server), tally, people);
    }

    private static long loadedChunks(MinecraftServer server) {
        long chunks = 0;
        for (ServerWorld world : server.getWorlds()) {
            chunks += world.getChunkManager().getLoadedChunkCount();
        }
        return chunks;
    }

    private static EntityTally tallyEntities(MinecraftServer server) {
        EntityTally tally = new EntityTally();
        for (ServerWorld world : server.getWorlds()) {
            String name = Bridge.worldIdOf(world);
            long before = tally.total();
            for (Entity entity : world.iterateEntities()) {
                Object key = net.minecraft.registry.Registries.ENTITY_TYPE.getId(entity.getType());
                tally.add(
                        name,
                        key == null ? null : key.toString(),
                        ((int) Math.floor(entity.getX())) >> 4,
                        ((int) Math.floor(entity.getZ())) >> 4);
            }
            tally.world(name, world.getChunkManager().getLoadedChunkCount(), tally.total() - before);
        }
        return tally;
    }
}

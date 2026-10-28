package com.github.chicodev11.monolab.addon;

import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import net.minecraft.block.Block;
import net.minecraft.block.BlockState;
import net.minecraft.registry.Registries;
import net.minecraft.registry.tag.FluidTags;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.network.ServerPlayerEntity;
import net.minecraft.server.world.ServerWorld;
import net.minecraft.util.math.BlockPos;
import net.minecraft.world.Heightmap;
import net.minecraft.world.LightType;
import net.minecraft.world.chunk.WorldChunk;

/** Live map samples for Fabric on Yarn names (1.21 – 1.21.11). Server thread only. */
final class YarnChunks implements MapLive.Source {
    private final MinecraftServer server;
    private final Map<Block, String> names = new IdentityHashMap<>();

    YarnChunks(MinecraftServer server) {
        this.server = server;
    }

    private static String dimensionOf(ServerWorld world) {
        return world.getRegistryKey().getValue().toString();
    }

    @Override
    public List<Object[]> players() {
        List<Object[]> out = new ArrayList<>();
        for (ServerWorld world : server.getWorlds()) {
            String dimension = dimensionOf(world);
            for (ServerPlayerEntity player : world.getPlayers()) {
                BlockPos at = player.getBlockPos();
                out.add(new Object[] {dimension, at.getX(), at.getZ()});
            }
        }
        return out;
    }

    @Override
    public MapLive.Reader chunk(String dimension, int cx, int cz) {
        for (ServerWorld world : server.getWorlds()) {
            if (dimension.equals(dimensionOf(world))) {
                WorldChunk chunk = world.getChunkManager().getWorldChunk(cx, cz);
                return chunk == null ? null : new Reader(world, chunk, cx * 16, cz * 16);
            }
        }
        return null;
    }

    private final class Reader implements MapLive.Reader {
        private final ServerWorld world;
        private final WorldChunk chunk;
        private final int baseX;
        private final int baseZ;
        private final BlockPos.Mutable pos = new BlockPos.Mutable();

        Reader(ServerWorld world, WorldChunk chunk, int baseX, int baseZ) {
            this.world = world;
            this.chunk = chunk;
            this.baseX = baseX;
            this.baseZ = baseZ;
        }

        private BlockState state(int x, int y, int z) {
            return chunk.getBlockState(pos.set(baseX + x, y, baseZ + z));
        }

        @Override
        public int minY() {
            return world.getDimension().minY();
        }

        @Override
        public int top(int x, int z) {
            int y = chunk.sampleHeightmap(Heightmap.Type.WORLD_SURFACE, x, z);
            return y < minY() ? Integer.MIN_VALUE : y;
        }

        @Override
        public String block(int x, int y, int z) {
            Block block = state(x, y, z).getBlock();
            return names.computeIfAbsent(block, one -> Registries.BLOCK.getId(one).toString());
        }

        @Override
        public boolean water(int x, int y, int z) {
            return state(x, y, z).getFluidState().isIn(FluidTags.WATER);
        }

        @Override
        public String biome(int x, int y, int z) {
            return chunk.getBiomeForNoiseGen((baseX + x) >> 2, y >> 2, (baseZ + z) >> 2).getIdAsString();
        }

        @Override
        public int light(int x, int y, int z) {
            return world.getLightLevel(LightType.BLOCK, pos.set(baseX + x, y, baseZ + z));
        }
    }
}

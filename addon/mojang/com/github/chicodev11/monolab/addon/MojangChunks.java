package com.github.chicodev11.monolab.addon;

import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.function.Function;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.tags.FluidTags;
import net.minecraft.world.level.LightLayer;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.LevelChunk;
import net.minecraft.world.level.levelgen.Heightmap;

/** Live map samples for loaders that use Mojang's names (NeoForge, Fabric 26.x). Server thread only. */
final class MojangChunks implements MapLive.Source {
    private final MinecraftServer server;
    private final Function<ServerLevel, String> dimensionOf;
    private final Map<Block, String> names = new IdentityHashMap<>();

    MojangChunks(MinecraftServer server, Function<ServerLevel, String> dimensionOf) {
        this.server = server;
        this.dimensionOf = dimensionOf;
    }

    @Override
    public List<Object[]> players() {
        List<Object[]> out = new ArrayList<>();
        for (ServerLevel level : server.getAllLevels()) {
            String dimension = dimensionOf.apply(level);
            for (ServerPlayer player : level.players()) {
                BlockPos at = player.blockPosition();
                out.add(new Object[] {dimension, at.getX(), at.getZ()});
            }
        }
        return out;
    }

    @Override
    public MapLive.Reader chunk(String dimension, int cx, int cz) {
        for (ServerLevel level : server.getAllLevels()) {
            if (dimension.equals(dimensionOf.apply(level))) {
                LevelChunk chunk = level.getChunkSource().getChunkNow(cx, cz);
                return chunk == null ? null : new Reader(level, chunk, cx * 16, cz * 16);
            }
        }
        return null;
    }

    private final class Reader implements MapLive.Reader {
        private final ServerLevel level;
        private final LevelChunk chunk;
        private final int baseX;
        private final int baseZ;
        private final BlockPos.MutableBlockPos pos = new BlockPos.MutableBlockPos();

        Reader(ServerLevel level, LevelChunk chunk, int baseX, int baseZ) {
            this.level = level;
            this.chunk = chunk;
            this.baseX = baseX;
            this.baseZ = baseZ;
        }

        private BlockState state(int x, int y, int z) {
            return chunk.getBlockState(pos.set(baseX + x, y, baseZ + z));
        }

        @Override
        public int minY() {
            return level.dimensionType().minY();
        }

        @Override
        public int top(int x, int z) {
            int y = chunk.getHeight(Heightmap.Types.WORLD_SURFACE, x, z);
            return y < minY() ? Integer.MIN_VALUE : y;
        }

        @Override
        public String block(int x, int y, int z) {
            Block block = state(x, y, z).getBlock();
            return names.computeIfAbsent(block, one -> BuiltInRegistries.BLOCK.getKey(one).toString());
        }

        @Override
        public boolean water(int x, int y, int z) {
            return state(x, y, z).getFluidState().is(FluidTags.WATER);
        }

        @Override
        public String biome(int x, int y, int z) {
            return chunk.getNoiseBiome((baseX + x) >> 2, y >> 2, (baseZ + z) >> 2).getRegisteredName();
        }

        @Override
        public int light(int x, int y, int z) {
            return level.getBrightness(LightLayer.BLOCK, pos.set(baseX + x, y, baseZ + z));
        }
    }
}

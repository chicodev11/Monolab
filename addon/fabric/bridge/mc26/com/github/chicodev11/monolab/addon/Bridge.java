package com.github.chicodev11.monolab.addon;

import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.item.context.BlockPlaceContext;
import net.minecraft.world.level.block.Block;

public final class Bridge {
    private Bridge() {}

    public static String nameOf(Player player) {
        return player.getScoreboardName();
    }

    public static String worldIdOf(ServerPlayer player) {
        return worldIdOf(player.level());
    }

    public static String worldIdOf(ServerLevel level) {
        MinecraftServer server = level.getServer();
        String name = server == null ? null : server.getWorldData().getLevelName();
        return EventSink.worldName(name, level.dimension().identifier().toString());
    }

    public static boolean placedNow(BlockPlaceContext context, Block block) {
        return context.getLevel().getBlockState(context.getClickedPos()).is(block);
    }
}

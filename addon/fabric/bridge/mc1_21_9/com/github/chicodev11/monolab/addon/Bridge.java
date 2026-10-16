package com.github.chicodev11.monolab.addon;

import net.minecraft.block.Block;
import net.minecraft.entity.player.PlayerEntity;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.world.ServerWorld;
import net.minecraft.item.ItemPlacementContext;
import net.minecraft.server.network.ServerPlayerEntity;

public final class Bridge {
    private Bridge() {}

    public static String nameOf(PlayerEntity player) {
        return player.getGameProfile().name();
    }

    public static String worldIdOf(ServerPlayerEntity player) {
        return worldIdOf(player.getEntityWorld());
    }

    public static String worldIdOf(ServerWorld world) {
        MinecraftServer server = world.getServer();
        String level = server == null ? null : server.getSaveProperties().getLevelInfo().getLevelName();
        return EventSink.worldName(level, world.getRegistryKey().getValue().toString());
    }

    public static boolean placedNow(ItemPlacementContext context, Block block) {
        return context.getWorld().getBlockState(context.getBlockPos()).isOf(block);
    }
}

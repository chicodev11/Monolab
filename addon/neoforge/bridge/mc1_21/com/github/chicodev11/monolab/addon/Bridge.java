package com.github.chicodev11.monolab.addon;

import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;

final class Bridge {
    private Bridge() {}

    static ServerLevel levelOf(ServerPlayer player) {
        return player.serverLevel();
    }

    static String dimensionId(ServerLevel level) {
        return level.dimension().location().toString();
    }
}

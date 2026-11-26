package com.github.chicodev11.monolab.addon;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import org.bukkit.Bukkit;
import org.bukkit.entity.Player;
import org.bukkit.inventory.ItemStack;
import org.bukkit.plugin.Plugin;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.concurrent.TimeUnit;
import java.util.function.BiConsumer;

/**
 * Item quick edit: the app writes {@code live/item-ask.json} ({@code player}, {@code nonce}) to read a player's
 * main-hand item, or {@code live/item-set.json} (plus {@code json}, the game's item JSON) to put one there. The
 * answer goes to {@code live/item.json} with the same nonce: {@code json} (the item as the game reads it back) or
 * {@code error}. Only the app writes these files, and only when the user presses its buttons.
 */
final class StudioHeldItems {
    private final Plugin plugin;
    private final Path live;
    private final BiConsumer<String, Throwable> warn;
    private ScheduledTask task;

    StudioHeldItems(Plugin plugin, Path studioRoot, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.live = studioRoot.resolve("live");
        this.warn = warn;
    }

    void start() {
        task = Bukkit.getAsyncScheduler().runAtFixedRate(plugin, ignored -> check(), 500, 500, TimeUnit.MILLISECONDS);
    }

    void stop() {
        if (task != null) {
            task.cancel();
        }
    }

    private void check() {
        handle(live.resolve("item-ask.json"), false);
        handle(live.resolve("item-set.json"), true);
    }

    private void handle(Path file, boolean set) {
        if (!Files.isRegularFile(file)) {
            return;
        }
        JsonObject request;
        try {
            request = JsonParser.parseString(Files.readString(file, StandardCharsets.UTF_8)).getAsJsonObject();
            Files.deleteIfExists(file);
        } catch (Exception error) {
            try {
                Files.deleteIfExists(file);
            } catch (IOException ignored) {
                // Tried again next check.
            }
            return;
        }
        long nonce = request.has("nonce") ? request.get("nonce").getAsLong() : 0;
        String name = request.has("player") ? request.get("player").getAsString() : "";
        Player player = Bukkit.getPlayerExact(name);
        if (player == null) {
            answer(nonce, null, name + " isn't online.");
            return;
        }
        JsonObject item = set && request.has("json") && request.get("json").isJsonObject() ? request.getAsJsonObject("json") : null;
        if (set && item == null) {
            answer(nonce, null, "No item was sent.");
            return;
        }
        player.getScheduler().run(plugin, ignored -> {
            try {
                if (set) {
                    player.getInventory().setItemInMainHand(read(item));
                }
                ItemStack held = player.getInventory().getItemInMainHand();
                if (held.isEmpty()) {
                    answer(nonce, null, "Their main hand is empty.");
                } else {
                    answer(nonce, write(held), null);
                }
            } catch (RuntimeException error) {
                answer(nonce, null, "The game didn't take that item: " + error.getMessage());
            }
        }, () -> answer(nonce, null, name + " left."));
    }

    @SuppressWarnings("deprecation")
    private static ItemStack read(JsonObject item) {
        return Bukkit.getUnsafe().deserializeItemFromJson(item);
    }

    @SuppressWarnings("deprecation")
    private static JsonObject write(ItemStack item) {
        return Bukkit.getUnsafe().serializeItemAsJson(item);
    }

    private void answer(long nonce, JsonObject item, String error) {
        JsonObject out = new JsonObject();
        out.addProperty("nonce", nonce);
        if (item != null) {
            out.add("json", item);
        } else {
            out.addProperty("error", error);
        }
        try {
            Files.createDirectories(live);
            Path temp = live.resolve("item.json.tmp");
            Files.writeString(temp, out.toString(), StandardCharsets.UTF_8);
            Files.move(temp, live.resolve("item.json"), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
        } catch (IOException failed) {
            warn.accept("Could not answer the app's item request", failed);
        }
    }
}

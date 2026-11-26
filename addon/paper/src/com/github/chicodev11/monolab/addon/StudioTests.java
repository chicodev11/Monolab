package com.github.chicodev11.monolab.addon;

import com.google.gson.Gson;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import org.bukkit.Bukkit;
import org.bukkit.entity.Player;
import org.bukkit.plugin.Plugin;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.TimeUnit;
import java.util.function.BiConsumer;

/**
 * In-game test tool: the app writes {@code live/fire.json} ({@code nonce}, {@code event}, {@code rule}, {@code player},
 * {@code vars}) to make a rule event happen without anyone doing it. The rules run as for the real event (cooldowns
 * skipped) and the answer goes to {@code live/fired.json} with the same nonce: {@code runs} (one per rule for that event:
 * {@code rule}, {@code matched}, {@code passed}, {@code ran}) or {@code error}. Only the app writes the request, and only
 * when the user presses its button.
 */
final class StudioTests {
    private final Plugin plugin;
    private final Path live;
    private final StudioRules rules;
    private final BiConsumer<String, Throwable> warn;
    private ScheduledTask task;

    StudioTests(Plugin plugin, Path studioRoot, StudioRules rules, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.live = studioRoot.resolve("live");
        this.rules = rules;
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
        Path file = live.resolve("fire.json");
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
        String event = request.has("event") ? request.get("event").getAsString().trim() : "";
        String rule = request.has("rule") ? request.get("rule").getAsString().trim() : "";
        String name = request.has("player") ? request.get("player").getAsString().trim() : "";
        Map<String, String> vars = new HashMap<>();
        if (request.has("vars") && request.get("vars").isJsonObject()) {
            request.getAsJsonObject("vars").entrySet().forEach(entry -> vars.put(entry.getKey(),
                    entry.getValue().isJsonPrimitive() ? entry.getValue().getAsString() : entry.getValue().toString()));
        }
        // The real break and place events give plain block names.
        vars.computeIfPresent("block", (key, block) -> block.trim().toLowerCase(Locale.ROOT).replaceFirst("^minecraft:", ""));
        if (!java.util.Arrays.asList(StudioRules.EVENTS).contains(event) && !MonolabApi.studioName(event)) {
            answer(nonce, null, "There's no event called " + event + ".");
            return;
        }
        if (name.isEmpty()) {
            if (!StudioRules.playerless(event)) {
                answer(nonce, null, "This event happens to a player. Pick one who is online.");
                return;
            }
            Bukkit.getGlobalRegionScheduler().execute(plugin, () -> run(nonce, event, rule, null, vars));
            return;
        }
        Player player = Bukkit.getPlayerExact(name);
        if (player == null) {
            answer(nonce, null, name + " isn't online.");
            return;
        }
        player.getScheduler().run(plugin, ignored -> run(nonce, event, rule, player, vars), () -> answer(nonce, null, name + " left."));
    }

    private void run(long nonce, String event, String rule, Player player, Map<String, String> vars) {
        try {
            answer(nonce, rules.test(event, rule, player, vars), null);
        } catch (RuntimeException error) {
            answer(nonce, null, "The test failed: " + error.getMessage());
        }
    }

    private void answer(long nonce, List<Map<String, Object>> runs, String error) {
        JsonObject out = new JsonObject();
        out.addProperty("nonce", nonce);
        if (runs != null) {
            out.add("runs", new Gson().toJsonTree(runs));
        } else {
            out.addProperty("error", error);
        }
        try {
            Files.createDirectories(live);
            Path temp = live.resolve("fired.json.tmp");
            Files.writeString(temp, out.toString(), StandardCharsets.UTF_8);
            Files.move(temp, live.resolve("fired.json"), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
        } catch (IOException failed) {
            warn.accept("Could not answer the app's test event", failed);
        }
    }
}

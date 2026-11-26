package com.github.chicodev11.monolab.addon;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.TreeMap;
import java.util.function.BiConsumer;
import net.kyori.adventure.text.serializer.gson.GsonComponentSerializer;
import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.advancement.Advancement;
import org.bukkit.advancement.AdvancementProgress;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerAdvancementDoneEvent;
import org.bukkit.plugin.Plugin;

/**
 * Studio advancements: your own tab in the advancements screen. Each one has a single criterion that only the
 * {@code advancement} action grants (from a rule, a command, an NPC...), and may run reward actions when a player
 * gets it. The server keeps loaded advancements in its own data pack across restarts, so this only loads what
 * changed; changing or removing one reloads the server's data once (Folia can't: it says to restart).
 */
final class StudioAdvancements implements Listener {
    static final String[] FRAMES = {"task", "goal", "challenge"};
    static final String CRITERION = "granted";

    private final Plugin plugin;
    private final Studio studio;
    private final StudioActions actions;
    private final BiConsumer<String, Throwable> warn;
    private final Path record;
    /** What this add-on loaded, by id: the JSON it gave the server (the server keeps it across restarts). */
    private final Map<String, String> loaded = new TreeMap<>();
    private volatile Map<String, Studio.Resource> current = Map.of();

    StudioAdvancements(Plugin plugin, Studio studio, StudioActions actions, Path studioRoot, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.studio = studio;
        this.actions = actions;
        this.warn = warn;
        this.record = studioRoot.resolve("data").resolve("advancements.json");
        readRecord();
    }

    NamespacedKey key(String id) {
        return new NamespacedKey(plugin, id);
    }

    void register() {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "advancement";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                sync(resources);
            }
        });
        actions.action("advancement", (step, run) -> {
            Player player = run.player();
            Advancement advancement = Bukkit.getAdvancement(key(StudioText.text(step, "advancement", "")));
            if (player == null || advancement == null) {
                return;
            }
            boolean revoke = Boolean.TRUE.equals(step.get("revoke"));
            actions.onPlayer(player, () -> {
                AdvancementProgress progress = player.getAdvancementProgress(advancement);
                if (revoke) {
                    progress.revokeCriteria(CRITERION);
                } else {
                    progress.awardCriteria(CRITERION);
                }
            });
        });
        actions.condition("advancement", (step, run) -> {
            Advancement advancement = Bukkit.getAdvancement(key(StudioText.text(step, "advancement", "")));
            return run.player() != null && advancement != null && run.player().getAdvancementProgress(advancement).isDone();
        });
    }

    /** On the server thread: removes what changed or went, reloads the server's data if it had to, loads the rest. */
    @SuppressWarnings("deprecation") // Bukkit.getUnsafe(): the only way to add an advancement while running.
    private void sync(List<Studio.Resource> resources) {
        Map<String, Studio.Resource> byId = new HashMap<>();
        for (Studio.Resource one : resources) {
            byId.put(one.id(), one);
        }
        current = Map.copyOf(byId);
        Map<String, String> wanted = new LinkedHashMap<>();
        for (String id : order(byId)) {
            wanted.put(id, json(byId.get(id).data(), id));
        }
        List<String> drop = new ArrayList<>();
        for (Map.Entry<String, String> was : loaded.entrySet()) {
            if (!was.getValue().equals(wanted.get(was.getKey()))) {
                drop.add(was.getKey());
            }
        }
        boolean folia = folia();
        if (!drop.isEmpty()) {
            if (folia) {
                for (String id : wanted.keySet()) {
                    if (drop.contains(id)) {
                        studio.problem("advancement", id, "Folia can't reload advancements while it runs. Restart the server to apply this change.");
                    }
                }
                return;
            }
            for (String id : drop) {
                Bukkit.getUnsafe().removeAdvancement(key(id));
                loaded.remove(id);
            }
            try {
                Bukkit.reloadData();
            } catch (RuntimeException error) {
                warn.accept("Could not reload the server's data for studio advancements", error);
            }
        }
        for (Map.Entry<String, String> one : wanted.entrySet()) {
            String id = one.getKey();
            if (one.getValue().equals(loaded.get(id)) && Bukkit.getAdvancement(key(id)) != null) {
                continue;
            }
            try {
                if (Bukkit.getAdvancement(key(id)) != null) {
                    // Left in the server's data pack by an older copy of this file; it takes a reload to replace.
                    Bukkit.getUnsafe().removeAdvancement(key(id));
                    studio.problem("advancement", id, "An older copy is still loaded. Save again or restart the server.");
                    continue;
                }
                if (Bukkit.getUnsafe().loadAdvancement(key(id), one.getValue()) == null) {
                    studio.problem("advancement", id, "The server didn't accept this advancement.");
                    continue;
                }
                loaded.put(id, one.getValue());
            } catch (RuntimeException error) {
                studio.problem("advancement", id, "The server didn't accept this advancement: " + error.getMessage());
            }
        }
        writeRecord();
    }

    /** Parents first; one whose parent is missing (or loops) is reported and left out. */
    private List<String> order(Map<String, Studio.Resource> byId) {
        List<String> out = new ArrayList<>();
        for (String id : new TreeMap<>(byId).keySet()) {
            visit(id, byId, out, new ArrayList<>());
        }
        return out;
    }

    private boolean visit(String id, Map<String, Studio.Resource> byId, List<String> out, List<String> path) {
        if (out.contains(id)) {
            return true;
        }
        if (path.contains(id)) {
            studio.problem("advancement", id, "Its parents go round in a loop.");
            return false;
        }
        String parent = StudioText.text(byId.get(id).data(), "parent", "").trim();
        if (!parent.isEmpty()) {
            if (!byId.containsKey(parent)) {
                studio.problem("advancement", id, "There is no studio advancement called " + parent + " to come after.");
                return false;
            }
            path.add(id);
            boolean ok = visit(parent, byId, out, path);
            path.remove(id);
            if (!ok) {
                return false;
            }
        }
        out.add(id);
        return true;
    }

    /** The Minecraft advancement JSON for one file. */
    String json(Map<String, Object> data, String id) {
        GsonComponentSerializer gson = GsonComponentSerializer.gson();
        Material icon = Material.matchMaterial(StudioText.text(data, "icon", "diamond"));
        if (icon == null || !icon.isItem() || icon.isAir()) {
            icon = Material.DIAMOND;
        }
        String frame = StudioText.text(data, "frame", "task").trim().toLowerCase(Locale.ROOT);
        if (!List.of(FRAMES).contains(frame)) {
            frame = "task";
        }
        String parent = StudioText.text(data, "parent", "").trim();
        Json display = new Json()
                .raw("icon", new Json().put("id", icon.getKey().toString()).end())
                .raw("title", gson.serialize(StudioText.render(StudioText.text(data, "title", id), null)))
                .raw("description", gson.serialize(StudioText.render(StudioText.text(data, "description", ""), null)))
                .put("frame", frame)
                .put("show_toast", !Boolean.FALSE.equals(data.get("toast")))
                .put("announce_to_chat", !Boolean.FALSE.equals(data.get("announce")))
                .put("hidden", Boolean.TRUE.equals(data.get("hidden")));
        if (parent.isEmpty()) {
            display.put("background", background(StudioText.text(data, "background", "stone")));
        }
        Json out = new Json().raw("display", display.end());
        if (!parent.isEmpty()) {
            out.put("parent", key(parent).toString());
        }
        out.raw("criteria", new Json().raw(CRITERION, new Json().put("trigger", "minecraft:impossible").end()).end());
        return out.end();
    }

    /**
     * The tab's background: a texture path as it is, or a block name. Minecraft 1.21.5 changed the format from
     * {@code minecraft:textures/block/stone.png} to {@code minecraft:block/stone}.
     */
    static String background(String given) {
        String name = given.trim().toLowerCase(Locale.ROOT);
        if (name.contains("/")) {
            return name;
        }
        String block = name.startsWith("minecraft:") ? name.substring(10) : name.isEmpty() ? "stone" : name;
        return newBackgrounds(Bukkit.getMinecraftVersion()) ? "minecraft:block/" + block : "minecraft:textures/block/" + block + ".png";
    }

    /** 1.21.5 and later, and the 26.x numbering. */
    static boolean newBackgrounds(String version) {
        String[] parts = version.split("[.\\-]");
        try {
            int major = Integer.parseInt(parts[0]);
            int minor = parts.length > 1 ? Integer.parseInt(parts[1]) : 0;
            int patch = parts.length > 2 ? Integer.parseInt(parts[2]) : 0;
            return major > 1 || minor > 21 || (minor == 21 && patch >= 5);
        } catch (NumberFormatException unknown) {
            return true;
        }
    }

    @EventHandler
    public void onDone(PlayerAdvancementDoneEvent event) {
        NamespacedKey done = event.getAdvancement().getKey();
        if (!done.getNamespace().equals(plugin.getName().toLowerCase(Locale.ROOT))) {
            return;
        }
        Studio.Resource one = current.get(done.getKey());
        if (one != null) {
            actions.run(one.data().get("rewards"), new StudioActions.Run(event.getPlayer(), Map.of("advancement", one.id())));
        }
    }

    private static boolean folia() {
        try {
            Class.forName("io.papermc.paper.threadedregions.RegionizedServer");
            return true;
        } catch (ClassNotFoundException paper) {
            return false;
        }
    }

    @SuppressWarnings("unchecked")
    private void readRecord() {
        try {
            if (Files.isRegularFile(record) && MiniJson.parse(Files.readString(record, StandardCharsets.UTF_8)) instanceof Map<?, ?> all) {
                ((Map<String, Object>) all).forEach((id, text) -> {
                    if (text instanceof String one) {
                        loaded.put(id, one);
                    }
                });
            }
        } catch (IOException | RuntimeException unreadable) {
            loaded.clear();
        }
    }

    private void writeRecord() {
        Json out = new Json();
        loaded.forEach(out::put);
        try {
            Files.createDirectories(record.getParent());
            Files.writeString(record, out.end(), StandardCharsets.UTF_8);
        } catch (IOException error) {
            warn.accept("Could not write studio/data/advancements.json", error);
        }
    }
}

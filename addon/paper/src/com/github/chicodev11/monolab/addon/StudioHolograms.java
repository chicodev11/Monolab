package com.github.chicodev11.monolab.addon;

import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.BiConsumer;
import net.kyori.adventure.text.Component;
import org.bukkit.Bukkit;
import org.bukkit.Color;
import org.bukkit.Location;
import org.bukkit.NamespacedKey;
import org.bukkit.World;
import org.bukkit.entity.Display;
import org.bukkit.entity.TextDisplay;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.Plugin;
import org.bukkit.util.Transformation;
import org.joml.AxisAngle4f;
import org.joml.Vector3f;

/**
 * Studio holograms: floating text at a spot, one text display each. The entity is never saved into the world
 * (it goes when its chunk unloads), so a crash or a removed file leaves nothing behind; {@link #tick} puts it back
 * while the chunk is loaded and keeps {@code <online>}, {@code <max>} and {@code <tps>} fresh. Player placeholders
 * don't apply: everyone sees the same text.
 */
final class StudioHolograms {
    static final long TICK_EVERY = 20L;

    private final Plugin plugin;
    private final Studio studio;
    private final BiConsumer<String, Throwable> warn;
    private final NamespacedKey key;
    private volatile List<Studio.Resource> holograms = List.of();
    /** Per hologram id: the entity showing it, and the problem already reported (status.json is written once). */
    private final Map<String, TextDisplay> shown = new ConcurrentHashMap<>();
    private final Map<String, String> reported = new ConcurrentHashMap<>();

    StudioHolograms(Plugin plugin, Studio studio, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.studio = studio;
        this.warn = warn;
        this.key = new NamespacedKey(plugin, "hologram");
    }

    void register() {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "hologram";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                holograms = resources;
                reported.clear();
                // Rebuilt from scratch: the next tick spawns what is there now.
                for (String id : List.copyOf(shown.keySet())) {
                    take(id);
                }
                tick();
            }
        });
    }

    /** Every second on the global region thread: spawn what is missing in loaded chunks, refresh the text. */
    void tick() {
        for (Studio.Resource one : holograms) {
            Map<String, Object> data = one.data();
            World world = Bukkit.getWorld(StudioText.text(data, "world", "world"));
            if (world == null) {
                report(one.id(), "There is no world named \"" + StudioText.text(data, "world", "world") + "\".");
                continue;
            }
            if (!(data.get("x") instanceof Double x) || !(data.get("y") instanceof Double y) || !(data.get("z") instanceof Double z)) {
                report(one.id(), "\"x\", \"y\" and \"z\" are needed.");
                continue;
            }
            Location at = new Location(world, x, y, z);
            if (!world.isChunkLoaded(at.getBlockX() >> 4, at.getBlockZ() >> 4)) {
                continue;
            }
            String text = String.join("<newline>", StudioText.lines(data, "lines"));
            Bukkit.getRegionScheduler().execute(plugin, at, () -> {
                try {
                    show(one.id(), data, at, text);
                } catch (RuntimeException error) {
                    warn.accept("A studio hologram failed: " + one.id(), error);
                }
            });
        }
    }

    /** On the region thread of {@code at}. */
    private void show(String id, Map<String, Object> data, Location at, String text) {
        TextDisplay entity = shown.get(id);
        Component rendered = StudioText.render(text, null);
        if (entity == null || !entity.isValid()) {
            entity = at.getWorld().spawn(at, TextDisplay.class, made -> {
                made.setPersistent(false);
                made.getPersistentDataContainer().set(key, PersistentDataType.STRING, id);
                made.text(rendered);
                style(made, data);
            });
            shown.put(id, entity);
            return;
        }
        // Placeholders change; the rest only with the file, which respawns it.
        entity.text(rendered);
    }

    private static void style(TextDisplay entity, Map<String, Object> data) {
        entity.setBillboard(billboard(StudioText.text(data, "billboard", "center")));
        entity.setShadowed(Boolean.TRUE.equals(data.get("shadow")));
        entity.setSeeThrough(Boolean.TRUE.equals(data.get("seeThrough")));
        if (Boolean.FALSE.equals(data.get("background"))) {
            entity.setDefaultBackground(false);
            entity.setBackgroundColor(Color.fromARGB(0, 0, 0, 0));
        }
        float scale = (float) Math.max(0.1, Math.min(10.0, data.get("scale") instanceof Double one ? one : 1.0));
        if (scale != 1f) {
            entity.setTransformation(new Transformation(new Vector3f(), new AxisAngle4f(), new Vector3f(scale, scale, scale), new AxisAngle4f()));
        }
    }

    /** {@code center}, {@code fixed}, {@code vertical} or {@code horizontal}; anything else faces the player. */
    static Display.Billboard billboard(String name) {
        return switch (name.trim().toLowerCase(Locale.ROOT)) {
            case "fixed" -> Display.Billboard.FIXED;
            case "vertical" -> Display.Billboard.VERTICAL;
            case "horizontal" -> Display.Billboard.HORIZONTAL;
            default -> Display.Billboard.CENTER;
        };
    }

    private void report(String id, String problem) {
        if (!problem.equals(reported.put(id, problem))) {
            studio.problem("hologram", id, problem);
        }
    }

    private void take(String id) {
        TextDisplay entity = shown.remove(id);
        if (entity != null && entity.isValid()) {
            entity.getScheduler().run(plugin, task -> entity.remove(), null);
        }
    }

    /** When the plugin turns off: nothing can be scheduled any more, so this runs right here. */
    void stop() {
        holograms = List.of();
        for (TextDisplay entity : shown.values()) {
            try {
                if (entity.isValid()) {
                    entity.remove();
                }
            } catch (RuntimeException otherThread) {
                // Folia: not this region's entity. It isn't saved, so it goes with its chunk.
            }
        }
        shown.clear();
        reported.clear();
    }
}

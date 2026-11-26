package com.github.chicodev11.monolab.addon;

import io.papermc.paper.plugin.bootstrap.BootstrapContext;
import io.papermc.paper.plugin.bootstrap.PluginBootstrap;
import io.papermc.paper.registry.RegistryKey;
import io.papermc.paper.registry.TypedKey;
import io.papermc.paper.registry.event.RegistryEvents;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.concurrent.ConcurrentHashMap;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.minimessage.MiniMessage;

/**
 * Runs before the server builds its registries: the studio's {@code song} files become jukebox songs and its
 * {@code painting} files painting variants ({@code monolab:<id>}, {@code crates/studio-format/src/song.rs} and
 * {@code painting.rs}). Paper only takes registry entries now, so a song
 * made or changed later is reported in its file's status until the next restart.
 */
public final class StudioBootstrap implements PluginBootstrap {
    /** Songs registered at this start, with what they were registered as ({@link #fingerprint}). */
    static final Map<String, String> SONGS = new ConcurrentHashMap<>();
    /** Paintings registered at this start, the same way ({@link #paintingPrint}). */
    static final Map<String, String> PAINTINGS = new ConcurrentHashMap<>();

    @Override
    public void bootstrap(BootstrapContext context) {
        biomes(context);
        Map<String, Map<String, Object>> paintings = read(context.getDataDirectory().resolve("studio").resolve("painting"));
        if (!paintings.isEmpty()) {
            context.getLifecycleManager().registerEventHandler(RegistryEvents.PAINTING_VARIANT.compose().newHandler(event -> {
                for (Map.Entry<String, Map<String, Object>> one : paintings.entrySet()) {
                    String id = one.getKey();
                    Map<String, Object> data = one.getValue();
                    if (StudioText.text(data, "texture", "").isBlank()) {
                        continue;
                    }
                    try {
                        event.registry().register(TypedKey.create(RegistryKey.PAINTING_VARIANT, Key.key("monolab", id)), art -> {
                            art.assetId(Key.key("monolab", id)).width(blocks(data, "width")).height(blocks(data, "height"))
                                    .title(MiniMessage.miniMessage().deserialize(title(data, id)));
                            String author = StudioText.text(data, "author", "").trim();
                            if (!author.isEmpty()) {
                                art.author(MiniMessage.miniMessage().deserialize(author));
                            }
                        });
                        PAINTINGS.put(id, paintingPrint(data, id));
                    } catch (RuntimeException error) {
                        context.getLogger().warn("Could not add studio painting " + id + ": " + error.getMessage());
                    }
                }
            }));
        }
        Map<String, Map<String, Object>> songs = read(context.getDataDirectory().resolve("studio").resolve("song"));
        if (songs.isEmpty()) {
            return;
        }
        context.getLifecycleManager().registerEventHandler(RegistryEvents.JUKEBOX_SONG.compose().newHandler(event -> {
            for (Map.Entry<String, Map<String, Object>> one : songs.entrySet()) {
                String id = one.getKey();
                Map<String, Object> data = one.getValue();
                String sound = sound(data);
                if (sound.isEmpty() || !(data.get("seconds") instanceof Double)) {
                    continue;
                }
                try {
                    event.registry().register(TypedKey.create(RegistryKey.JUKEBOX_SONG, Key.key("monolab", id)), song -> song
                            .soundEvent(factory -> factory.empty().location(Key.key(sound)))
                            .description(MiniMessage.miniMessage().deserialize(description(data, id)))
                            .lengthInSeconds((float) seconds(data))
                            .comparatorOutput(comparator(data)));
                    SONGS.put(id, fingerprint(data, id));
                } catch (RuntimeException error) {
                    context.getLogger().warn("Could not add studio song " + id + ": " + error.getMessage());
                }
            }
        }));
    }

    /**
     * Studio biomes: Paper only takes new biomes from data packs, so they are written into one in the add-on's folder
     * ({@link StudioBiomes#writePack}) that is handed over while the server finds its packs.
     */
    private static void biomes(BootstrapContext context) {
        Map<String, Map<String, Object>> biomes = read(context.getDataDirectory().resolve("studio").resolve("biome"));
        if (biomes.isEmpty()) {
            return;
        }
        Path pack = context.getDataDirectory().resolve("studio-datapack");
        Map<String, String> written;
        try {
            written = StudioBiomes.writePack(pack, biomes);
        } catch (IOException | RuntimeException error) {
            context.getLogger().warn("Could not write the studio biome data pack: " + error.getMessage());
            return;
        }
        context.getLifecycleManager().registerEventHandler(io.papermc.paper.plugin.lifecycle.event.types.LifecycleEvents.DATAPACK_DISCOVERY, event -> {
            try {
                event.registrar().discoverPack(pack, "studio", configurer -> configurer.autoEnableOnServerStart(true));
                StudioBiomes.BIOMES.putAll(written);
            } catch (IOException | RuntimeException error) {
                context.getLogger().warn("Could not add the studio biome data pack: " + error.getMessage());
            }
        });
    }

    private static Map<String, Map<String, Object>> read(Path folder) {
        Map<String, Map<String, Object>> out = new TreeMap<>();
        if (!Files.isDirectory(folder)) {
            return out;
        }
        try (DirectoryStream<Path> files = Files.newDirectoryStream(folder, "*.json")) {
            for (Path file : files) {
                String name = file.getFileName().toString();
                Studio.Resource one = Studio.check(folder.getFileName().toString(), name.substring(0, name.length() - 5), Files.readString(file, StandardCharsets.UTF_8));
                if (one.enabled()) {
                    out.put(one.id(), one.data());
                }
            }
        } catch (IOException | RuntimeException ignored) {
            // Unreadable folder: no songs this start; the studio reports the files later.
        }
        return out;
    }

    /** {@code song::sound}. */
    static String sound(Map<String, Object> data) {
        String raw = StudioText.text(data, "sound", "").trim().toLowerCase(java.util.Locale.ROOT);
        return raw.isEmpty() || raw.contains(":") ? raw : "monolab:studio." + raw;
    }

    static double seconds(Map<String, Object> data) {
        return StudioItemParts.number(data, "seconds", 60, 1, 3600);
    }

    static int comparator(Map<String, Object> data) {
        return StudioText.number(data, "comparator", 1, 0, 15);
    }

    static String description(Map<String, Object> data, String id) {
        String name = StudioText.text(data, "name", "").trim();
        return name.isEmpty() ? id.replace('_', ' ').replace('-', ' ') : name;
    }

    /** {@code song::fingerprint}: the same text the Rust side makes, so a changed song is told apart. */
    static String fingerprint(Map<String, Object> data, String id) {
        double seconds = seconds(data);
        String shown = seconds == Math.floor(seconds) ? String.valueOf((long) seconds) : String.valueOf(seconds);
        return sound(data) + "|" + shown + "|" + comparator(data) + "|" + description(data, id);
    }

    static int blocks(Map<String, Object> data, String key) {
        return StudioText.number(data, key, 1, 1, 16);
    }

    static String title(Map<String, Object> data, String id) {
        String name = StudioText.text(data, "title", "").trim();
        return name.isEmpty() ? id.replace('_', ' ').replace('-', ' ') : name;
    }

    /** {@code painting::fingerprint}. */
    static String paintingPrint(Map<String, Object> data, String id) {
        return blocks(data, "width") + "|" + blocks(data, "height") + "|" + title(data, id) + "|" + StudioText.text(data, "author", "").trim();
    }

    /** {@code studio:painting/<id>}: a painting item that hangs ours (registered when the server started). */
    static org.bukkit.inventory.ItemStack painting(String ref) {
        if (!ref.startsWith("painting/")) {
            return null;
        }
        String id = ref.substring(9);
        if (!PAINTINGS.containsKey(id)) {
            return null;
        }
        org.bukkit.Art art = io.papermc.paper.registry.RegistryAccess.registryAccess().getRegistry(RegistryKey.PAINTING_VARIANT).get(Key.key("monolab", id));
        if (art == null) {
            return null;
        }
        org.bukkit.inventory.ItemStack stack = new org.bukkit.inventory.ItemStack(org.bukkit.Material.PAINTING);
        stack.setData(io.papermc.paper.datacomponent.DataComponentTypes.PAINTING_VARIANT, art);
        return stack;
    }

    /** The song kind's status: wrong fields, and songs this start didn't take. */
    static void feature(Studio studio) {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "song";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                for (Studio.Resource one : resources) {
                    String problem = null;
                    if (sound(one.data()).isEmpty()) {
                        problem = "\"sound\" is needed: a studio sound's name or a sound id.";
                    } else if (!(one.data().get("seconds") instanceof Double)) {
                        problem = "\"seconds\" is needed: how long the song plays.";
                    } else if (!SONGS.containsKey(one.id())) {
                        problem = "Restart the server to add this song: Paper only takes new jukebox songs while it starts.";
                    } else if (!SONGS.get(one.id()).equals(fingerprint(one.data(), one.id()))) {
                        problem = "Changed since the server started: jukeboxes play it as it was until the next restart.";
                    }
                    if (problem != null) {
                        studio.problem("song", one.id(), problem);
                    }
                }
            }
        });
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "painting";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                for (Studio.Resource one : resources) {
                    String problem = null;
                    if (StudioText.text(one.data(), "texture", "").isBlank()) {
                        problem = "\"texture\" is needed: a studio picture (Assets → Textures).";
                    } else if (!PAINTINGS.containsKey(one.id())) {
                        problem = "Restart the server to add this painting: Paper only takes new paintings while it starts.";
                    } else if (!PAINTINGS.get(one.id()).equals(paintingPrint(one.data(), one.id()))) {
                        problem = "Changed since the server started: its size and title stay as they were until the next restart (the picture comes from the pack).";
                    }
                    if (problem != null) {
                        studio.problem("painting", one.id(), problem);
                    }
                }
            }
        });
    }
}

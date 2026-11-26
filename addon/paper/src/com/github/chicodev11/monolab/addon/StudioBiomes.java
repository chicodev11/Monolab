package com.github.chicodev11.monolab.addon;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Studio biomes ({@code biome} files, {@code crates/studio-format/src/biome.rs}): {@code monolab:<id>} biomes with
 * their own sky, fog, water, grass and leaf colours, particles, music and ambient sound. Paper takes new biomes only
 * from data packs while it starts, so the bootstrap writes them into {@code studio-datapack} in the add-on's folder
 * and hands that pack over ({@link StudioBootstrap}). Every biome copies plains' caves, ores, plants and mobs.
 */
final class StudioBiomes {
    /** Biomes the data pack had at this start, with the JSON they were written as. */
    static final Map<String, String> BIOMES = new ConcurrentHashMap<>();
    static final String[][] COLORS = {{"sky", "sky_color"}, {"fog", "fog_color"}, {"water", "water_color"}, {"waterFog", "water_fog_color"}, {"grass", "grass_color"}, {"foliage", "foliage_color"}};
    static final String WATER = "#3f76e4";
    private static final String CARVERS = "[\"minecraft:cave\",\"minecraft:cave_extra_underground\",\"minecraft:canyon\"]";
    private static final String FEATURES = "[[],[\"minecraft:lake_lava_underground\",\"minecraft:lake_lava_surface\"],[\"minecraft:amethyst_geode\"],[\"minecraft:monster_room\",\"minecraft:monster_room_deep\"],[],[],[\"minecraft:ore_dirt\",\"minecraft:ore_gravel\",\"minecraft:ore_granite_upper\",\"minecraft:ore_granite_lower\",\"minecraft:ore_diorite_upper\",\"minecraft:ore_diorite_lower\",\"minecraft:ore_andesite_upper\",\"minecraft:ore_andesite_lower\",\"minecraft:ore_tuff\",\"minecraft:ore_coal_upper\",\"minecraft:ore_coal_lower\",\"minecraft:ore_iron_upper\",\"minecraft:ore_iron_middle\",\"minecraft:ore_iron_small\",\"minecraft:ore_gold\",\"minecraft:ore_gold_lower\",\"minecraft:ore_redstone\",\"minecraft:ore_redstone_lower\",\"minecraft:ore_diamond\",\"minecraft:ore_diamond_medium\",\"minecraft:ore_diamond_large\",\"minecraft:ore_diamond_buried\",\"minecraft:ore_lapis\",\"minecraft:ore_lapis_buried\",\"minecraft:ore_copper\",\"minecraft:underwater_magma\",\"minecraft:disk_sand\",\"minecraft:disk_clay\",\"minecraft:disk_gravel\"],[],[\"minecraft:spring_water\",\"minecraft:spring_lava\"],[\"minecraft:glow_lichen\",\"minecraft:patch_tall_grass_2\",\"minecraft:patch_bush\",\"minecraft:trees_plains\",\"minecraft:flower_plains\",\"minecraft:patch_grass_plain\",\"minecraft:brown_mushroom_normal\",\"minecraft:red_mushroom_normal\",\"minecraft:patch_pumpkin\",\"minecraft:patch_sugar_cane\",\"minecraft:patch_firefly_bush_near_water\"],[\"minecraft:freeze_top_layer\"]]";
    private static final String SPAWNS = "{\"argument\":{\"spawn_costs\":{},\"spawns_by_category\":{\"ambient\":[{\"type\":\"minecraft:bat\",\"count\":8,\"weight\":10}],\"creature\":[{\"type\":\"minecraft:sheep\",\"count\":4,\"weight\":12},{\"type\":\"minecraft:pig\",\"count\":4,\"weight\":10},{\"type\":\"minecraft:chicken\",\"count\":4,\"weight\":10},{\"type\":\"minecraft:cow\",\"count\":4,\"weight\":8},{\"type\":\"minecraft:horse\",\"count\":{\"type\":\"minecraft:uniform\",\"max_inclusive\":6,\"min_inclusive\":2},\"weight\":5},{\"type\":\"minecraft:donkey\",\"count\":{\"type\":\"minecraft:uniform\",\"max_inclusive\":3,\"min_inclusive\":1},\"weight\":1}],\"monster\":[{\"type\":\"minecraft:spider\",\"count\":4,\"weight\":100},{\"type\":\"minecraft:zombie\",\"count\":4,\"weight\":90},{\"type\":\"minecraft:zombie_villager\",\"count\":1,\"weight\":5},{\"type\":\"minecraft:zombie_horse\",\"count\":1,\"weight\":5},{\"type\":\"minecraft:skeleton\",\"count\":4,\"weight\":100},{\"type\":\"minecraft:creeper\",\"count\":4,\"weight\":100},{\"type\":\"minecraft:slime\",\"count\":4,\"weight\":100},{\"type\":\"minecraft:enderman\",\"count\":{\"type\":\"minecraft:uniform\",\"max_inclusive\":4,\"min_inclusive\":1},\"weight\":10},{\"type\":\"minecraft:witch\",\"count\":1,\"weight\":5}],\"underground_water_creature\":[{\"type\":\"minecraft:glow_squid\",\"count\":{\"type\":\"minecraft:uniform\",\"max_inclusive\":6,\"min_inclusive\":4},\"weight\":10}]}},\"modifier\":\"overlay\"}";

    private StudioBiomes() {}

    /** {@code #rrggbb} in lower case, or null. */
    static String color(Map<String, Object> data, String key) {
        String raw = StudioText.text(data, key, "").trim().toLowerCase(Locale.ROOT);
        String hex = raw.startsWith("#") ? raw.substring(1) : raw;
        return hex.matches("[0-9a-f]{6}") ? "#" + hex : null;
    }

    static String sound(Map<String, Object> data, String key) {
        String raw = StudioText.text(data, key, "").trim().toLowerCase(Locale.ROOT);
        return raw.isEmpty() || raw.contains(":") ? raw : "monolab:studio." + raw;
    }

    static String particle(Map<String, Object> data) {
        String raw = StudioText.text(data, "particle", "").trim().toLowerCase(Locale.ROOT);
        return raw.isEmpty() || raw.contains(":") ? raw : "minecraft:" + raw;
    }

    /** {@code biome::registry_json}: the vanilla data pack JSON (Minecraft 26.x environment attributes). */
    static String json(Map<String, Object> data) {
        Json attributes = new Json().raw("minecraft:gameplay/natural_mob_spawns", SPAWNS)
                .put("minecraft:visual/sky_color", color(data, "sky"))
                .put("minecraft:visual/fog_color", color(data, "fog"))
                .put("minecraft:visual/water_fog_color", color(data, "waterFog"));
        String particle = particle(data);
        if (!particle.isEmpty()) {
            double chance = StudioItemParts.number(data, "particleChance", 0.01, 0, 1);
            attributes.raw("minecraft:visual/ambient_particles", "{\"argument\":[" + new Json().raw("particle", new Json().put("type", particle).end()).raw("probability", String.valueOf(chance)).end() + "],\"modifier\":\"append\"}");
        }
        String music = sound(data, "music");
        if (!music.isEmpty()) {
            attributes.raw("minecraft:audio/background_music", new Json().raw("default", new Json().put("sound", music).put("min_delay", 12000).put("max_delay", 24000).end()).end());
        }
        String ambient = sound(data, "ambient");
        if (!ambient.isEmpty()) {
            attributes.raw("minecraft:audio/ambient_sounds", new Json().put("loop", ambient).end());
        }
        String water = color(data, "water");
        Json effects = new Json().put("water_color", water != null ? water : WATER).put("grass_color", color(data, "grass")).put("foliage_color", color(data, "foliage"));
        return new Json().raw("attributes", attributes.end())
                .raw("carvers", CARVERS)
                .raw("downfall", String.valueOf(StudioItemParts.number(data, "downfall", 0.4, 0, 1)))
                .raw("effects", effects.end())
                .raw("features", FEATURES)
                .put("has_precipitation", !Boolean.FALSE.equals(data.get("rain")))
                .raw("temperature", String.valueOf(StudioItemParts.number(data, "temperature", 0.8, -1, 2)))
                .end();
    }

    /** {@code biome::problems}. */
    static String problem(Map<String, Object> data) {
        for (String[] one : COLORS) {
            if (!StudioText.text(data, one[0], "").isBlank() && color(data, one[0]) == null) {
                return "\"" + one[0] + "\" isn't a colour like #78a7ff.";
            }
        }
        for (String key : new String[] {"music", "ambient"}) {
            String one = sound(data, key);
            if (!one.isEmpty() && !valid(one)) {
                return "\"" + key + "\" isn't a sound name: " + one + ".";
            }
        }
        String particle = particle(data);
        if (!particle.isEmpty() && !valid(particle)) {
            return "\"particle\" isn't a particle name: " + particle + ".";
        }
        return null;
    }

    private static boolean valid(String name) {
        int colon = name.indexOf(':');
        return colon > 0 && colon < name.length() - 1 && name.matches("[a-z0-9_.:/-]+");
    }

    /**
     * Writes the data pack the bootstrap hands to Paper: {@code pack.mcmeta} and one file per biome (old ones go).
     * Returns the biomes written, by id, with their JSON.
     */
    static Map<String, String> writePack(Path folder, Map<String, Map<String, Object>> biomes) throws IOException {
        Path files = folder.resolve("data").resolve("monolab").resolve("worldgen").resolve("biome");
        Files.createDirectories(files);
        Files.writeString(folder.resolve("pack.mcmeta"), "{\"pack\":{\"description\":\"Monolab studio biomes\",\"pack_format\":81,\"supported_formats\":[81,999],\"min_format\":81,\"max_format\":999}}", StandardCharsets.UTF_8);
        try (DirectoryStream<Path> old = Files.newDirectoryStream(files, "*.json")) {
            for (Path one : old) {
                String name = one.getFileName().toString();
                if (!biomes.containsKey(name.substring(0, name.length() - 5))) {
                    Files.deleteIfExists(one);
                }
            }
        }
        Map<String, String> written = new java.util.TreeMap<>();
        for (Map.Entry<String, Map<String, Object>> one : biomes.entrySet()) {
            String text = json(one.getValue());
            Files.writeString(files.resolve(one.getKey() + ".json"), text, StandardCharsets.UTF_8);
            written.put(one.getKey(), text);
        }
        return written;
    }

    /** The biome kind's status: wrong fields, and biomes this start didn't take. */
    static void feature(Studio studio) {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "biome";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                for (Studio.Resource one : resources) {
                    String problem = problem(one.data());
                    if (problem == null && !BIOMES.containsKey(one.id())) {
                        problem = "Restart the server to add this biome: Paper only takes new biomes while it starts.";
                    } else if (problem == null && !BIOMES.get(one.id()).equals(json(one.data()))) {
                        problem = "Changed since the server started: it looks as it was until the next restart.";
                    }
                    if (problem != null) {
                        studio.problem("biome", one.id(), problem);
                    }
                }
            }
        });
    }
}

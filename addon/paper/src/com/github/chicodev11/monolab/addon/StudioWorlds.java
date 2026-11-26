package com.github.chicodev11.monolab.addon;

import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Random;
import java.util.function.BiConsumer;
import org.bukkit.Bukkit;
import org.bukkit.GameRule;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.World;
import org.bukkit.WorldCreator;
import org.bukkit.WorldType;
import org.bukkit.block.Block;
import org.bukkit.generator.ChunkGenerator;
import org.bukkit.generator.WorldInfo;

/**
 * Studio worlds: extra worlds the server loads (and makes the first time): normal, flat with your own layers,
 * amplified, large biomes or empty (void). Game rules and PvP apply every time. The way a world generates only
 * matters when it is first made; removing the file never unloads or deletes a world. Folia can't add worlds
 * while it runs.
 */
final class StudioWorlds {
    static final String[] TYPES = {"normal", "flat", "amplified", "large_biomes", "void", "custom"};
    static final String[] ENVIRONMENTS = {"normal", "nether", "end"};

    private final Studio studio;
    private final BiConsumer<String, Throwable> warn;
    /** Our blocks by id, for custom generators that place {@code studio:<id>}. */
    private final java.util.function.Function<String, org.bukkit.block.data.BlockData> studioBlocks;

    StudioWorlds(Studio studio, BiConsumer<String, Throwable> warn, java.util.function.Function<String, org.bukkit.block.data.BlockData> studioBlocks) {
        this.studio = studio;
        this.warn = warn;
        this.studioBlocks = studioBlocks;
    }

    void register() {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "world";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                for (Studio.Resource one : resources) {
                    try {
                        load(one);
                    } catch (RuntimeException error) {
                        warn.accept("A studio world failed: " + one.id(), error);
                        studio.problem("world", one.id(), "The world couldn't be loaded: " + error.getMessage());
                    }
                }
            }
        });
    }

    /** World folder names: letters, digits, {@code _} and {@code -}. */
    static String name(Studio.Resource one) {
        String name = StudioText.text(one.data(), "name", one.id()).trim();
        return name.isEmpty() ? one.id() : name;
    }

    private void load(Studio.Resource one) {
        Map<String, Object> data = one.data();
        String name = name(one);
        if (!name.matches("[A-Za-z0-9_-]{1,48}")) {
            studio.problem("world", one.id(), "\"name\" may only use letters, digits, _ and - (up to 48).");
            return;
        }
        if ("custom".equals(type(data))) {
            for (String problem : StudioWorldGen.problems(generatorOf(data))) {
                studio.problem("world", one.id(), problem);
            }
        }
        // The main world took this generator when the server made it (MonolabPlugin.getDefaultWorldGenerator).
        if (isMain(data)) {
            return;
        }
        World main = Bukkit.getWorlds().isEmpty() ? null : Bukkit.getWorlds().get(0);
        if (main != null && List.of(main.getName(), main.getName() + "_nether", main.getName() + "_the_end").contains(name)) {
            studio.problem("world", one.id(), "This is the name of one of the server's own worlds. To use this generator there, turn on \"Use for the main world\".");
            return;
        }
        World world = Bukkit.getWorld(name);
        if (world == null) {
            if (folia()) {
                studio.problem("world", one.id(), "Folia can't add worlds while it runs.");
                return;
            }
            world = Bukkit.createWorld(creator(name, data, studioBlocks, studio.root().resolve("structures")));
            if (world == null) {
                studio.problem("world", one.id(), "The server didn't make the world.");
                return;
            }
            if ("void".equals(type(data))) {
                Location spawn = world.getSpawnLocation();
                Block floor = world.getBlockAt(spawn.getBlockX(), spawn.getBlockY() - 1, spawn.getBlockZ());
                if (floor.getType().isAir()) {
                    floor.setType(Material.BEDROCK);
                }
            }
        }
        if (world.getGenerator() instanceof StudioWorldGen generator) {
            for (String problem : generator.structureProblems) {
                studio.problem("world", one.id(), problem);
            }
            if (generator.vanilla != null && generator.spiral) {
                studio.problem("world", one.id(), "Paper can't twist the server's own ground (Pumpkin only): it stays untwisted.");
            }
        }
        settings(one, world, data);
    }

    static String type(Map<String, Object> data) {
        String type = StudioText.text(data, "type", "normal").trim().toLowerCase(Locale.ROOT);
        return List.of(TYPES).contains(type) ? type : "normal";
    }

    /** A custom world file that gives the server's main world its generator instead of making a world of its own. */
    static boolean isMain(Map<String, Object> data) {
        return "custom".equals(type(data)) && Boolean.TRUE.equals(data.get("main"));
    }

    /** The generator for the main world from {@code studio/world/<id>.json}, or null (the server's own) when it can't be used. */
    @SuppressWarnings("unchecked")
    static ChunkGenerator mainGenerator(java.nio.file.Path studioRoot, String id, java.util.function.Function<String, org.bukkit.block.data.BlockData> studioBlocks, java.util.function.Consumer<String> warn) {
        if (id == null || !id.matches("[a-z0-9_-]{1,64}")) {
            warn.accept("bukkit.yml asks for the studio world generator \"" + id + "\", which isn't a studio file name.");
            return null;
        }
        java.nio.file.Path file = studioRoot.resolve("world").resolve(id + ".json");
        try {
            Object parsed = MiniJson.parse(java.nio.file.Files.readString(file, java.nio.charset.StandardCharsets.UTF_8));
            if (!(parsed instanceof Map<?, ?> map)) {
                warn.accept("studio/world/" + id + ".json isn't a JSON object, so the main world uses the server's own generator.");
                return null;
            }
            Map<String, Object> data = (Map<String, Object>) map;
            if (Boolean.FALSE.equals(data.get("enabled")) || !isMain(data)) {
                warn.accept("studio/world/" + id + ".json isn't a custom world marked for the main world, so the server's own generator is used.");
                return null;
            }
            String name = StudioText.text(data, "name", id).trim();
            return new StudioWorldGen(generatorOf(data), StudioWorldGen.seedOf(StudioText.text(data, "seed", "").trim(), name.isEmpty() ? id : name), studioBlocks, studioRoot.resolve("structures"));
        } catch (java.io.IOException | RuntimeException error) {
            warn.accept("studio/world/" + id + ".json couldn't be read (" + error.getMessage() + "), so the main world uses the server's own generator.");
            return null;
        }
    }

    @SuppressWarnings("unchecked")
    static Map<String, Object> generatorOf(Map<String, Object> data) {
        return data.get("generator") instanceof Map<?, ?> map ? (Map<String, Object>) map : Map.of();
    }

    private static WorldCreator creator(String name, Map<String, Object> data, java.util.function.Function<String, org.bukkit.block.data.BlockData> studioBlocks, java.nio.file.Path structureFolder) {
        WorldCreator creator = new WorldCreator(name);
        creator.environment(switch (StudioText.text(data, "environment", "normal").trim().toLowerCase(Locale.ROOT)) {
            case "nether" -> World.Environment.NETHER;
            case "end" -> World.Environment.THE_END;
            default -> World.Environment.NORMAL;
        });
        String seed = StudioText.text(data, "seed", "").trim();
        if (!seed.isEmpty()) {
            creator.seed(seedOf(seed));
        }
        creator.generateStructures(!Boolean.FALSE.equals(data.get("structures")));
        switch (type(data)) {
            case "flat" -> {
                creator.type(WorldType.FLAT);
                creator.generatorSettings(flatSettings(data));
            }
            case "amplified" -> creator.type(WorldType.AMPLIFIED);
            case "large_biomes" -> creator.type(WorldType.LARGE_BIOMES);
            case "void" -> {
                creator.generator(new Empty());
                creator.generateStructures(false);
            }
            case "custom" -> {
                // Chosen again on every start: Bukkit doesn't save a plugin's generator with the world.
                StudioWorldGen generator = new StudioWorldGen(generatorOf(data), StudioWorldGen.seedOf(seed, name), studioBlocks, structureFolder);
                creator.generator(generator);
                creator.biomeProvider(generator.getDefaultBiomeProvider(null));
                if (generator.vanilla != null) {
                    // The server's own ground of this preset, with the biome rules on it.
                    creator.type(switch (generator.vanilla) {
                        case "amplified" -> WorldType.AMPLIFIED;
                        case "large_biomes" -> WorldType.LARGE_BIOMES;
                        default -> WorldType.NORMAL;
                    });
                    creator.generateStructures(!Boolean.FALSE.equals(data.get("structures")));
                } else {
                    creator.generateStructures(generator.keepStructures);
                }
            }
            default -> creator.type(WorldType.NORMAL);
        }
        return creator;
    }

    /** A number as it is; any other text the way Minecraft turns a typed seed into one. */
    static long seedOf(String seed) {
        try {
            return Long.parseLong(seed);
        } catch (NumberFormatException text) {
            return seed.hashCode();
        }
    }

    /** Flat layers, bottom first: {@code [{"block": "bedrock", "height": 1}, ...]}; none gives the usual grass. */
    static String flatSettings(Map<String, Object> data) {
        StringBuilder layers = new StringBuilder("[");
        if (data.get("layers") instanceof List<?> list) {
            for (Object raw : list) {
                if (!(raw instanceof Map<?, ?> one) || !(one.get("block") instanceof String block)) {
                    continue;
                }
                Material material = Material.matchMaterial(block);
                if (material == null || !material.isBlock()) {
                    continue;
                }
                int height = one.get("height") instanceof Double count ? (int) Math.max(1, Math.min(384, Math.round(count))) : 1;
                if (layers.length() > 1) {
                    layers.append(',');
                }
                layers.append(new Json().put("block", material.getKey().toString()).put("height", height).end());
            }
        }
        if (layers.length() == 1) {
            layers.append("{\"block\":\"minecraft:bedrock\",\"height\":1},{\"block\":\"minecraft:dirt\",\"height\":2},{\"block\":\"minecraft:grass_block\",\"height\":1}");
        }
        layers.append(']');
        String biome = StudioText.text(data, "biome", "plains").trim().toLowerCase(Locale.ROOT);
        return new Json().raw("layers", layers.toString()).put("biome", biome.contains(":") ? biome : "minecraft:" + biome).raw("structure_overrides", "[]").end();
    }

    /**
     * By its old name ({@code doDaylightCycle}) or, on 26.x, its registry name ({@code advance_time}). 26.x renamed
     * the rules but kept the old constants ({@code DO_DAYLIGHT_CYCLE}), so the old names are looked up there.
     */
    @SuppressWarnings("removal")
    static GameRule<?> rule(String name) {
        try {
            GameRule<?> found = GameRule.getByName(name);
            if (found != null) {
                return found;
            }
        } catch (IllegalArgumentException notAKey) {
            // An old camelCase name on 26.x: tried below.
        }
        try {
            java.lang.reflect.Field constant = GameRule.class.getField(name.replaceAll("([a-z])([A-Z])", "$1_$2").toUpperCase(Locale.ROOT));
            return constant.get(null) instanceof GameRule<?> found ? found : null;
        } catch (ReflectiveOperationException | RuntimeException unknown) {
            return null;
        }
    }

    @SuppressWarnings({"unchecked", "deprecation"})
    private void settings(Studio.Resource one, World world, Map<String, Object> data) {
        if (data.get("pvp") instanceof Boolean pvp) {
            world.setPVP(pvp);
        }
        if (data.get("gamerules") instanceof Map<?, ?> raw) {
            for (Map.Entry<String, Object> rule : ((Map<String, Object>) raw).entrySet()) {
                GameRule<?> found = rule(rule.getKey());
                if (found == null) {
                    studio.problem("world", one.id(), "There is no game rule called " + rule.getKey() + ".");
                    continue;
                }
                if (found.getType() == Boolean.class && rule.getValue() instanceof Boolean on) {
                    world.setGameRule((GameRule<Boolean>) found, on);
                } else if (found.getType() == Integer.class && rule.getValue() instanceof Double count) {
                    world.setGameRule((GameRule<Integer>) found, (int) Math.round(count));
                } else {
                    studio.problem("world", one.id(), "The game rule " + rule.getKey() + " needs a " + (found.getType() == Boolean.class ? "switch" : "number") + ".");
                }
            }
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

    /** Generates nothing: an empty world. */
    static final class Empty extends ChunkGenerator {
        @Override
        public boolean shouldGenerateNoise(WorldInfo info, Random random, int x, int z) {
            return false;
        }

        @Override
        public boolean shouldGenerateSurface(WorldInfo info, Random random, int x, int z) {
            return false;
        }

        @Override
        public boolean shouldGenerateCaves(WorldInfo info, Random random, int x, int z) {
            return false;
        }

        @Override
        public boolean shouldGenerateDecorations(WorldInfo info, Random random, int x, int z) {
            return false;
        }

        @Override
        public boolean shouldGenerateMobs(WorldInfo info, Random random, int x, int z) {
            return false;
        }

        @Override
        public boolean shouldGenerateStructures(WorldInfo info, Random random, int x, int z) {
            return false;
        }

        @Override
        public Location getFixedSpawnLocation(World world, Random random) {
            return new Location(world, 0.5, 64, 0.5);
        }
    }
}

package com.github.chicodev11.monolab.addon;

import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Random;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Function;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.World;
import org.bukkit.block.Biome;
import org.bukkit.block.data.BlockData;
import org.bukkit.generator.BiomeProvider;
import org.bukkit.generator.ChunkGenerator;
import org.bukkit.generator.WorldInfo;

/**
 * A studio world's own generator ({@code "type": "custom"}). The same steps as
 * {@code crates/studio-format/src/worldgen.rs}, number for number (same hash, same noise, same order), so the app's
 * preview shows what this makes. Keep the two in step.
 */
final class StudioWorldGen extends ChunkGenerator {
    static final List<String> NOISES = List.of("perlin", "ridged", "billow", "cellular");
    static final List<String> WARPS = List.of("none", "spiral", "domain");
    static final List<String> CAVES = List.of("spaghetti", "cheese");
    static final List<String> FEATURES = List.of("ore", "tree", "boulder", "patch", "structure");
    /** {@code worldgen::SHAPES}, in the same order (a node's type is its place here). */
    static final List<String> SHAPES = List.of("noise", "constant", "add", "multiply", "min", "max", "clamp", "abs", "power", "terrace", "remap", "climate", "distance");
    static final List<String> MODES = List.of("custom", "vanilla");
    static final List<String> TERRAINS = List.of("normal", "amplified", "large_biomes");
    static final int MOST_SHAPE_DEPTH = 12;
    static final int MOST_SHAPE_NODES = 64;
    /** A world feature's structure may be at most this many blocks across ({@code structure::MOST_FEATURE_SIZE}). */
    static final int MOST_FEATURE_SIZE = 32;

    // ---------- hash, random, noise ----------

    static long mix(long h) {
        h ^= h >>> 33;
        h *= 0xff51afd7ed558ccdL;
        h ^= h >>> 33;
        h *= 0xc4ceb9fe1a85ec53L;
        return h ^ (h >>> 33);
    }

    static long hash(long seed, int x, int y, int z) {
        return mix(seed ^ ((long) x * 0x9e3779b97f4a7c15L) ^ ((long) y * 0xc2b2ae3d27d4eb4fL) ^ ((long) z * 0x165667b19e3779f9L));
    }

    static final class Rng {
        private long state;

        Rng(long seed) {
            state = seed;
        }

        long next() {
            state += 0x9e3779b97f4a7c15L;
            return mix(state);
        }

        double unit() {
            return (next() >>> 11) * 0x1.0p-53;
        }

        int between(int from, int to) {
            if (to <= from) {
                return from;
            }
            return from + (int) Math.floor(unit() * (to - from + 1));
        }
    }

    private static double fade(double t) {
        return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    }

    private static double lerp(double t, double a, double b) {
        return a + t * (b - a);
    }

    private static double grad2(long h, double x, double y) {
        return switch ((int) (h & 7)) {
            case 0 -> x + y;
            case 1 -> -x + y;
            case 2 -> x - y;
            case 3 -> -x - y;
            case 4 -> x;
            case 5 -> -x;
            case 6 -> y;
            default -> -y;
        };
    }

    private static double grad3(long hash, double x, double y, double z) {
        int h = (int) (hash & 15);
        double u = h < 8 ? x : y;
        double v = h < 4 ? y : (h == 12 || h == 14 ? x : z);
        return ((h & 1) == 0 ? u : -u) + ((h & 2) == 0 ? v : -v);
    }

    private static double clamp(double value, double least, double most) {
        return Math.max(least, Math.min(most, value));
    }

    static double perlin2(long seed, double x, double z) {
        double x0 = Math.floor(x);
        double z0 = Math.floor(z);
        double fx = x - x0;
        double fz = z - z0;
        int ix = (int) x0;
        int iz = (int) z0;
        double u = fade(fx);
        double v = fade(fz);
        double a = lerp(u, grad2(hash(seed, ix, 0, iz), fx, fz), grad2(hash(seed, ix + 1, 0, iz), fx - 1.0, fz));
        double b = lerp(u, grad2(hash(seed, ix, 0, iz + 1), fx, fz - 1.0), grad2(hash(seed, ix + 1, 0, iz + 1), fx - 1.0, fz - 1.0));
        return clamp(lerp(v, a, b) * 0.9, -1.0, 1.0);
    }

    static double perlin3(long seed, double x, double y, double z) {
        double x0 = Math.floor(x);
        double y0 = Math.floor(y);
        double z0 = Math.floor(z);
        double fx = x - x0;
        double fy = y - y0;
        double fz = z - z0;
        int ix = (int) x0;
        int iy = (int) y0;
        int iz = (int) z0;
        double u = fade(fx);
        double v = fade(fy);
        double w = fade(fz);
        double x00 = lerp(u, g3(seed, ix, iy, iz, fx, fy, fz, 0, 0, 0), g3(seed, ix, iy, iz, fx, fy, fz, 1, 0, 0));
        double x10 = lerp(u, g3(seed, ix, iy, iz, fx, fy, fz, 0, 1, 0), g3(seed, ix, iy, iz, fx, fy, fz, 1, 1, 0));
        double x01 = lerp(u, g3(seed, ix, iy, iz, fx, fy, fz, 0, 0, 1), g3(seed, ix, iy, iz, fx, fy, fz, 1, 0, 1));
        double x11 = lerp(u, g3(seed, ix, iy, iz, fx, fy, fz, 0, 1, 1), g3(seed, ix, iy, iz, fx, fy, fz, 1, 1, 1));
        return clamp(lerp(w, lerp(v, x00, x10), lerp(v, x01, x11)) * 0.9, -1.0, 1.0);
    }

    private static double g3(long seed, int ix, int iy, int iz, double fx, double fy, double fz, int dx, int dy, int dz) {
        return grad3(hash(seed, ix + dx, iy + dy, iz + dz), fx - dx, fy - dy, fz - dz);
    }

    static double cellular2(long seed, double x, double z) {
        int cx = (int) Math.floor(x);
        int cz = (int) Math.floor(z);
        double nearest = 2.0;
        for (int dx = -1; dx <= 1; dx++) {
            for (int dz = -1; dz <= 1; dz++) {
                int px = cx + dx;
                int pz = cz + dz;
                Rng rng = new Rng(hash(seed, px, 7, pz));
                double fx = px + rng.unit() - x;
                double fz = pz + rng.unit() - z;
                nearest = Math.min(nearest, Math.sqrt(fx * fx + fz * fz));
            }
        }
        return 1.0 - 2.0 * Math.min(nearest, 1.0);
    }

    private static double oneNoise(int kind, long seed, double x, double z) {
        return switch (kind) {
            case 1 -> {
                double r = 1.0 - Math.abs(perlin2(seed, x, z));
                yield r * r * 2.0 - 1.0;
            }
            case 2 -> Math.abs(perlin2(seed, x, z)) * 2.0 - 1.0;
            case 3 -> cellular2(seed, x, z);
            default -> perlin2(seed, x, z);
        };
    }

    static double fbm(int kind, long seed, double x, double z, int octaves) {
        double sum = 0;
        double total = 0;
        double amp = 1;
        double freq = 1;
        for (int octave = 0; octave < Math.max(1, octaves); octave++) {
            sum += amp * oneNoise(kind, seed + octave * 131L, x * freq, z * freq);
            total += amp;
            amp *= 0.5;
            freq *= 2.0;
        }
        return sum / total;
    }

    /** Rust's {@code f64::round}: halves away from zero. */
    private static long round(double value) {
        return value < 0 ? -(long) Math.floor(-value + 0.5) : (long) Math.floor(value + 0.5);
    }

    // ---------- reading the file ----------

    private static double num(Map<String, Object> data, String key, double fallback) {
        return data.get(key) instanceof Number one && Double.isFinite(one.doubleValue()) ? one.doubleValue() : fallback;
    }

    private static String text(Map<String, Object> data, String key, String fallback) {
        return data.get(key) instanceof String one && !one.isBlank() ? one.trim() : fallback;
    }

    /** Two numbers as given (not sorted), for {@code remap}. */
    private static double[] pair(Map<String, Object> data, String key, double from, double to) {
        if (data.get(key) instanceof List<?> list && list.size() == 2 && list.get(0) instanceof Number a && list.get(1) instanceof Number b) {
            return new double[] {a.doubleValue(), b.doubleValue()};
        }
        return new double[] {from, to};
    }

    private static double[] range(Map<String, Object> data, String key, double from, double to) {
        if (data.get(key) instanceof List<?> list && list.size() == 2 && list.get(0) instanceof Number a && list.get(1) instanceof Number b) {
            return new double[] {Math.min(a.doubleValue(), b.doubleValue()), Math.max(a.doubleValue(), b.doubleValue())};
        }
        return new double[] {from, to};
    }

    @SuppressWarnings("unchecked")
    private static List<Map<String, Object>> objects(Map<String, Object> data, String key) {
        List<Map<String, Object>> out = new ArrayList<>();
        if (data.get(key) instanceof List<?> list) {
            for (Object one : list) {
                if (one instanceof Map<?, ?> map) {
                    out.add((Map<String, Object>) map);
                }
            }
        }
        return out;
    }

    @SuppressWarnings("unchecked")
    private static Map<String, Object> object(Map<String, Object> data, String key) {
        return data.get(key) instanceof Map<?, ?> map ? (Map<String, Object>) map : Map.of();
    }

    static String shortName(String name) {
        String lower = name.trim().toLowerCase(Locale.ROOT);
        return lower.startsWith("minecraft:") ? lower.substring("minecraft:".length()) : lower;
    }

    /** The seed a custom world generates from: a number, other text hashed, or the world's name. */
    static long seedOf(String seed, String world) {
        String given = seed.trim();
        if (given.isEmpty()) {
            return world.hashCode();
        }
        try {
            return Long.parseLong(given);
        } catch (NumberFormatException text) {
            return given.hashCode();
        }
    }

    /** What is wrong in a {@code "generator"} block. */
    static List<String> problems(Map<String, Object> data) {
        List<String> out = new ArrayList<>();
        known(out, object(data, "warp").get("type"), WARPS, "warp");
        for (Map<String, Object> one : objects(data, "layers")) {
            known(out, one.get("noise"), NOISES, "noise");
        }
        known(out, data.get("mode"), MODES, "mode");
        known(out, data.get("terrain"), TERRAINS, "terrain");
        int[] shapeSize = null;
        if (data.get("shape") instanceof Map<?, ?>) {
            List<Map<String, Object>> nodes = new ArrayList<>();
            List<Integer> depths = new ArrayList<>();
            shapeNodes(object(data, "shape"), 0, nodes, depths);
            for (Map<String, Object> node : nodes) {
                known(out, node.get("type"), SHAPES, "node");
                known(out, node.get("noise"), NOISES, "noise");
            }
            shapeSize = new int[] {nodes.size(), depths.stream().mapToInt(Integer::intValue).max().orElse(0)};
        }
        for (Map<String, Object> one : objects(data, "caves")) {
            known(out, one.get("type"), CAVES, "cave");
        }
        for (Map<String, Object> one : objects(data, "features")) {
            known(out, one.get("type"), FEATURES, "feature");
            if ("structure".equals(one.get("type")) && text(one, "structure", "").isEmpty()) {
                out.add("A structure feature needs a structure.");
            }
        }
        if (shapeSize != null) {
            if (shapeSize[0] > MOST_SHAPE_NODES) {
                out.add("The shape has " + shapeSize[0] + " nodes; only the first " + MOST_SHAPE_NODES + " are used.");
            }
            if (shapeSize[1] > MOST_SHAPE_DEPTH) {
                out.add("The shape nests deeper than " + MOST_SHAPE_DEPTH + "; deeper nodes count as 0.");
            }
        }
        boolean vanilla = "vanilla".equals(data.get("mode"));
        if (vanilla && "domain".equals(object(data, "warp").get("type"))) {
            out.add("On the server's own ground only the spiral twist works (Pumpkin).");
        }
        if (!vanilla && !data.containsKey("shape") && num(data, "sea", 63) > num(data, "base", 68) + num(data, "height", 40)) {
            out.add("The sea is above the highest ground: the whole world is under water.");
        }
        return out;
    }

    private static void shapeNodes(Map<String, Object> data, int depth, List<Map<String, Object>> out, List<Integer> depths) {
        out.add(data);
        depths.add(depth);
        if (out.size() > MOST_SHAPE_NODES * 4) {
            return;
        }
        if (data.get("input") instanceof Map<?, ?>) {
            shapeNodes(object(data, "input"), depth + 1, out, depths);
        }
        for (Map<String, Object> one : objects(data, "inputs")) {
            shapeNodes(one, depth + 1, out, depths);
        }
    }

    private static void known(List<String> out, Object value, List<String> list, String what) {
        if (value instanceof String one && !list.contains(one)) {
            out.add("Unknown " + what + " \"" + one + "\" (use " + String.join(", ", list) + ").");
        }
    }

    // ---------- the generator ----------

    private record Layer(int noise, double scale, int octaves, double amount) {}

    private record BiomeRule(String biome, double[] temperature, double[] humidity, double[] height, double offset, double stretch) {}

    /**
     * One node of a {@code shape} tree ({@code Shape} in worldgen.rs): {@code type} is its place in {@link #SHAPES};
     * {@code a}..{@code d} are its numbers (noise: scale, amount; clamp: min, max; power: exponent; terrace: steps;
     * remap: from a..b to c..d; climate: 1 for wetness; distance: x, z, radius).
     */
    private record Shape(int type, List<Shape> inputs, int noise, int octaves, long seed, double a, double b, double c, double d) {
        static Shape constant(double value) {
            return new Shape(1, List.of(), 0, 0, 0, value, 0, 0, 0);
        }

        static Shape read(Map<String, Object> data, long seed, int depth, int[] count) {
            count[0]++;
            if (depth > MOST_SHAPE_DEPTH || count[0] > MOST_SHAPE_NODES) {
                return constant(0);
            }
            int type = SHAPES.indexOf(text(data, "type", "constant"));
            return switch (type) {
                case 0 -> new Shape(0, List.of(), Math.max(0, NOISES.indexOf(text(data, "noise", "perlin"))), (int) clamp(num(data, "octaves", 4.0), 1, 8),
                        seed + 20000 + 1000L * count[0], Math.max(1.0, num(data, "scale", 256.0)), num(data, "amount", 1.0), 0, 0);
                case 2, 3, 4, 5 -> {
                    List<Shape> inputs = new ArrayList<>();
                    for (Map<String, Object> one : objects(data, "inputs")) {
                        inputs.add(read(one, seed, depth + 1, count));
                    }
                    yield new Shape(type, inputs, 0, 0, 0, 0, 0, 0, 0);
                }
                case 6 -> {
                    double least = num(data, "min", -1.0);
                    double most = num(data, "max", 1.0);
                    yield new Shape(6, List.of(input(data, seed, depth, count)), 0, 0, 0, Math.min(least, most), Math.max(least, most), 0, 0);
                }
                case 7 -> new Shape(7, List.of(input(data, seed, depth, count)), 0, 0, 0, 0, 0, 0, 0);
                case 8 -> new Shape(8, List.of(input(data, seed, depth, count)), 0, 0, 0, clamp(num(data, "exponent", 2.0), 0.1, 8), 0, 0, 0);
                case 9 -> new Shape(9, List.of(input(data, seed, depth, count)), 0, 0, 0, (double) round(clamp(num(data, "steps", 4.0), 1, 64)), 0, 0, 0);
                case 10 -> {
                    double[] from = pair(data, "from", -1, 1);
                    double[] to = pair(data, "to", 0, 1);
                    yield new Shape(10, List.of(input(data, seed, depth, count)), 0, 0, 0, from[0], from[1], to[0], to[1]);
                }
                case 11 -> new Shape(11, List.of(), 0, 0, 0, "wetness".equals(text(data, "which", "warmth")) ? 1 : 0, 0, 0, 0);
                case 12 -> new Shape(12, List.of(), 0, 0, 0, num(data, "x", 0.0), num(data, "z", 0.0), Math.max(1.0, num(data, "radius", 512.0)), 0);
                default -> constant(num(data, "value", 0.0));
            };
        }

        private static Shape input(Map<String, Object> data, long seed, int depth, int[] count) {
            return data.get("input") instanceof Map<?, ?> ? read(object(data, "input"), seed, depth + 1, count) : constant(0);
        }

        double value(double x, double z, double temperature, double humidity) {
            switch (type) {
                case 0:
                    return b * fbm(noise, seed, x / a, z / a, octaves);
                case 2: {
                    double sum = 0;
                    for (Shape one : inputs) {
                        sum += one.value(x, z, temperature, humidity);
                    }
                    return sum;
                }
                case 3: {
                    double product = 1;
                    for (Shape one : inputs) {
                        product *= one.value(x, z, temperature, humidity);
                    }
                    return product;
                }
                case 4:
                case 5: {
                    if (inputs.isEmpty()) {
                        return 0;
                    }
                    double out = inputs.get(0).value(x, z, temperature, humidity);
                    for (int i = 1; i < inputs.size(); i++) {
                        double one = inputs.get(i).value(x, z, temperature, humidity);
                        out = type == 4 ? Math.min(out, one) : Math.max(out, one);
                    }
                    return out;
                }
                case 6:
                    return clamp(inputs.get(0).value(x, z, temperature, humidity), a, b);
                case 7:
                    return Math.abs(inputs.get(0).value(x, z, temperature, humidity));
                case 8: {
                    double value = inputs.get(0).value(x, z, temperature, humidity);
                    return (value < 0 ? -1.0 : 1.0) * Math.pow(Math.abs(value), a);
                }
                case 9: {
                    double scaled = inputs.get(0).value(x, z, temperature, humidity) * a;
                    double floor = Math.floor(scaled);
                    return (floor + fade(scaled - floor)) / a;
                }
                case 10: {
                    double value = inputs.get(0).value(x, z, temperature, humidity);
                    return Math.abs(b - a) < 1e-12 ? c : c + (value - a) / (b - a) * (d - c);
                }
                case 11:
                    return (a == 1 ? humidity : temperature) * 2.0 - 1.0;
                case 12: {
                    double dx = x - a;
                    double dz = z - b;
                    return 1.0 - Math.sqrt(dx * dx + dz * dz) / c;
                }
                default:
                    return a;
            }
        }
    }

    /** {@code worldgen::inside}: how much a climate value is in a range, with soft edges {@code blend} wide. */
    private static double inside(double value, double[] range, double blend) {
        if (blend <= 0) {
            return value >= range[0] && value <= range[1] ? 1 : 0;
        }
        double low = range[0] <= 0 ? 1 : clamp((value - range[0]) / blend + 0.5, 0, 1);
        double high = range[1] >= 1 ? 1 : clamp((range[1] - value) / blend + 0.5, 0, 1);
        return Math.min(low, high);
    }

    record Surface(String biome, String top, String under, int depth, String underwater) {}

    /** 0 spaghetti, 1 cheese; {@code a} is the scale, {@code b} the width or threshold. */
    private record Cave(int type, double a, double b) {}

    /** 0 ore, 1 tree, 2 boulder, 3 patch, 4 structure (block = its id, leaves = rotation, size = lift). */
    private record Feature(int type, String block, String leaves, int from, int to, int size, double perChunk, List<String> biomes) {}

    record Column(int height, double temperature, double humidity, int biome) {}

    record Place(int x, int y, int z, String block, int only) {}

    static final int ANY = 0;
    static final int AIR = 1;
    static final int STONE = 2;

    private final long seed;
    /** 0 none, 1 spiral, 2 domain. */
    private final int warp;
    private final double warpA;
    private final double warpB;
    private final double warpC;
    final int sea;
    private final double base;
    private final double height;
    private final double overhangs;
    private final double climate;
    private final List<Layer> layers = new ArrayList<>();
    final String stone;
    final String fluid;
    final boolean bedrock;
    private final List<BiomeRule> biomes = new ArrayList<>();
    private final List<Surface> surface = new ArrayList<>();
    private final Surface fallbackSurface;
    private final List<Cave> caves = new ArrayList<>();
    private final List<Feature> features = new ArrayList<>();
    final boolean keepStructures;
    final boolean keepMobs;
    final boolean keepCarvers;
    final boolean keepDecorations;
    private final Function<String, BlockData> studioBlocks;
    private final Map<String, BlockData> states = new ConcurrentHashMap<>();
    private final Map<String, Biome> biomeTypes = new ConcurrentHashMap<>();
    /** Structures the features use, by id, each turned 0–3 quarters. */
    private final Map<String, Turned[]> structures = new HashMap<>();
    /** What went wrong reading them, for the world's status. */
    final List<String> structureProblems = new ArrayList<>();
    /** The node tree that makes the ground instead of the layers, or null. */
    private final Shape shape;
    private final double blend;
    /** Whether any biome rule lifts or stretches the ground. */
    private final boolean shaped;
    /** {@code "mode": "vanilla"}: the server's own ground of this preset (normal, amplified, large_biomes), or null. */
    final String vanilla;
    /** Whether the twist is a spiral (on vanilla ground only Pumpkin can do it). */
    final boolean spiral;

    private record Turned(int width, int depth, int[] blocks, String[] states) {}

    StudioWorldGen(Map<String, Object> data, long seed, Function<String, BlockData> studioBlocks, java.nio.file.Path structureFolder) {
        this.seed = seed;
        this.studioBlocks = studioBlocks;
        Map<String, Object> warpData = object(data, "warp");
        switch (text(warpData, "type", "none")) {
            case "spiral" -> {
                warp = 1;
                warpA = num(warpData, "strength", 1.0);
                warpB = num(warpData, "x", 0.0);
                warpC = num(warpData, "z", 0.0);
            }
            case "domain" -> {
                warp = 2;
                warpA = Math.max(1.0, num(warpData, "scale", 256.0));
                warpB = num(warpData, "amount", 64.0);
                warpC = 0;
            }
            default -> {
                warp = 0;
                warpA = 0;
                warpB = 0;
                warpC = 0;
            }
        }
        for (Map<String, Object> one : objects(data, "layers")) {
            layers.add(new Layer(Math.max(0, NOISES.indexOf(text(one, "noise", "perlin"))), Math.max(1.0, num(one, "scale", 256.0)), (int) clamp(num(one, "octaves", 4.0), 1, 8), num(one, "amount", 1.0)));
        }
        if (layers.isEmpty() && !data.containsKey("layers")) {
            layers.add(new Layer(0, 384.0, 5, 1.0));
        }
        for (Map<String, Object> one : objects(data, "biomes")) {
            biomes.add(new BiomeRule(shortName(text(one, "biome", "plains")), range(one, "temperature", 0, 1), range(one, "humidity", 0, 1), range(one, "height", -4096, 4096),
                    clamp(num(one, "offset", 0.0), -256, 256), clamp(num(one, "stretch", 1.0), 0, 8)));
        }
        boolean anyShaped = false;
        for (BiomeRule rule : biomes) {
            anyShaped |= rule.offset() != 0.0 || rule.stretch() != 1.0;
        }
        shaped = anyShaped;
        shape = data.get("shape") instanceof Map<?, ?> ? Shape.read(object(data, "shape"), seed, 0, new int[] {0}) : null;
        blend = clamp(num(data, "blend", 0.05), 0, 0.5);
        if ("vanilla".equals(text(data, "mode", "custom"))) {
            String terrain = text(data, "terrain", "normal").trim();
            vanilla = TERRAINS.contains(terrain) ? terrain : "normal";
        } else {
            vanilla = null;
        }
        spiral = warp == 1;
        Surface fallback = null;
        for (Map<String, Object> one : objects(data, "surface")) {
            Surface made = surfaceOf(one);
            if (made.biome().isEmpty() && fallback == null) {
                fallback = made;
            } else {
                surface.add(made);
            }
        }
        fallbackSurface = fallback != null ? fallback : surfaceOf(Map.of());
        for (Map<String, Object> one : objects(data, "caves")) {
            switch (text(one, "type", "")) {
                case "spaghetti" -> caves.add(new Cave(0, Math.max(4.0, num(one, "scale", 64.0)), clamp(num(one, "width", 0.07), 0, 1)));
                case "cheese" -> caves.add(new Cave(1, Math.max(4.0, num(one, "scale", 96.0)), clamp(num(one, "threshold", 0.55), -1, 1)));
                default -> { }
            }
        }
        for (Map<String, Object> one : objects(data, "features")) {
            int size = (int) clamp(num(one, "size", 3.0), 1, 16);
            List<String> only = new ArrayList<>();
            if (one.get("biomes") instanceof List<?> list) {
                for (Object name : list) {
                    if (name instanceof String biome && !shortName(biome).isEmpty()) {
                        only.add(shortName(biome));
                    }
                }
            }
            double per = clamp(num(one, "per_chunk", 1.0), 0, 64);
            switch (text(one, "type", "")) {
                case "ore" -> {
                    double[] y = range(one, "y", -48, 64);
                    features.add(new Feature(0, shortName(text(one, "block", "coal_ore")), "", (int) y[0], (int) y[1], size, per, only));
                }
                case "tree" -> {
                    double[] tall = range(one, "height", 4, 6);
                    features.add(new Feature(1, shortName(text(one, "trunk", "oak_log")), shortName(text(one, "leaves", "oak_leaves")), (int) Math.max(1, tall[0]), (int) clamp(tall[1], 1, 32), size, per, only));
                }
                case "boulder" -> features.add(new Feature(2, shortName(text(one, "block", "mossy_cobblestone")), "", 0, 0, Math.min(size, 6), per, only));
                case "patch" -> features.add(new Feature(3, shortName(text(one, "block", "short_grass")), "", 0, 0, size, per, only));
                case "structure" -> {
                    String rotation = text(one, "rotation", "random").trim();
                    features.add(new Feature(4, text(one, "structure", "").trim(), List.of(StudioStructures.ROTATIONS).contains(rotation) ? rotation : "random", 0, 0, (int) clamp(num(one, "lift", 0.0), -64, 64), per, only));
                }
                default -> { }
            }
        }
        Map<String, Object> keep = object(data, "keep");
        keepStructures = Boolean.TRUE.equals(keep.get("structures"));
        keepMobs = !Boolean.FALSE.equals(keep.get("mobs"));
        keepCarvers = Boolean.TRUE.equals(keep.get("carvers"));
        keepDecorations = Boolean.TRUE.equals(keep.get("decorations"));
        sea = (int) clamp(num(data, "sea", 63.0), -64, 320);
        base = clamp(num(data, "base", 68.0), -64, 320);
        height = clamp(num(data, "height", 40.0), 0, 256);
        overhangs = clamp(num(data, "overhangs", 0.0), 0, 2);
        climate = Math.max(16.0, num(data, "climate", 512.0));
        stone = shortName(text(data, "stone", "stone"));
        fluid = shortName(text(data, "fluid", "water"));
        bedrock = !Boolean.FALSE.equals(data.get("bedrock"));
        for (Feature feature : features) {
            if (feature.type() != 4 || structures.containsKey(feature.block()) || structureFolder == null || !feature.block().matches("[a-z0-9_-]{1,48}")) {
                continue;
            }
            try {
                StudioStructures.Built built = StudioStructures.load(structureFolder.resolve(feature.block() + ".json"), 0);
                if (built.sx() > MOST_FEATURE_SIZE || built.sz() > MOST_FEATURE_SIZE) {
                    structureProblems.add("The structure " + feature.block() + " is over " + MOST_FEATURE_SIZE + " blocks across, too big for a world feature.");
                    continue;
                }
                Turned[] turned = new Turned[4];
                for (int quarter = 0; quarter < 4; quarter++) {
                    String[] names = new String[built.palette().size()];
                    for (int i = 0; i < names.length; i++) {
                        names[i] = shortName(StudioStructures.rotateState(built.palette().get(i), quarter));
                    }
                    boolean odd = quarter % 2 == 1;
                    turned[quarter] = new Turned(odd ? built.sz() : built.sx(), odd ? built.sx() : built.sz(), StudioStructures.turned(built, quarter), names);
                }
                structures.put(feature.block(), turned);
            } catch (java.io.IOException | RuntimeException failed) {
                structureProblems.add("There is no structure " + feature.block() + " (or its file wasn't brought in yet).");
            }
        }
    }

    private static Surface surfaceOf(Map<String, Object> one) {
        return new Surface(shortName(text(one, "biome", "")), shortName(text(one, "top", "grass_block")), shortName(text(one, "under", "dirt")), (int) clamp(num(one, "depth", 3.0), 0, 32), shortName(text(one, "underwater", "gravel")));
    }

    private double[] warped(double x, double z) {
        return switch (warp) {
            case 1 -> {
                double dx = x - warpB;
                double dz = z - warpC;
                double r = Math.sqrt(dx * dx + dz * dz);
                double theta = warpA * 2.0 * Math.log(1.0 + r / 96.0);
                double sin = Math.sin(theta);
                double cos = Math.cos(theta);
                yield new double[] {warpB + dx * cos - dz * sin, warpC + dx * sin + dz * cos};
            }
            case 2 -> new double[] {x + warpB * perlin2(seed + 77, x / warpA, z / warpA), z + warpB * perlin2(seed + 78, x / warpA, z / warpA)};
            default -> new double[] {x, z};
        };
    }

    /**
     * On vanilla ground: the biome the rules give a spot by warmth and wetness only, or null where none holds and
     * the server's own biome stays ({@code Generator::vanilla_biome}; not warped).
     */
    String vanillaBiome(int x, int z) {
        double temperature = (fbm(0, seed + 11, x / climate, z / climate, 3) + 1.0) / 2.0;
        double humidity = (fbm(0, seed + 12, x / climate, z / climate, 3) + 1.0) / 2.0;
        for (BiomeRule rule : biomes) {
            if (temperature >= rule.temperature()[0] && temperature <= rule.temperature()[1] && humidity >= rule.humidity()[0] && humidity <= rule.humidity()[1]) {
                return rule.biome();
            }
        }
        return null;
    }

    Column column(int x, int z) {
        double[] w = warped(x, z);
        double temperature = (fbm(0, seed + 11, w[0] / climate, w[1] / climate, 3) + 1.0) / 2.0;
        double humidity = (fbm(0, seed + 12, w[0] / climate, w[1] / climate, 3) + 1.0) / 2.0;
        double sum = 0;
        if (shape != null) {
            sum = shape.value(w[0], w[1], temperature, humidity);
        } else {
            for (int index = 0; index < layers.size(); index++) {
                Layer layer = layers.get(index);
                sum += layer.amount() * fbm(layer.noise(), seed + 1000L * (index + 1), w[0] / layer.scale(), w[1] / layer.scale(), layer.octaves());
            }
        }
        int ground;
        if (shaped) {
            double offset = 0;
            double stretch = 0;
            double left = 1;
            for (BiomeRule rule : biomes) {
                double take = left * inside(temperature, rule.temperature(), blend) * inside(humidity, rule.humidity(), blend);
                offset += take * rule.offset();
                stretch += take * rule.stretch();
                left -= take;
            }
            ground = (int) round(base + offset + height * (stretch + left) * sum);
        } else {
            ground = (int) round(base + height * sum);
        }
        int biome = -1;
        for (int index = 0; index < biomes.size(); index++) {
            BiomeRule rule = biomes.get(index);
            if (temperature >= rule.temperature()[0] && temperature <= rule.temperature()[1] && humidity >= rule.humidity()[0] && humidity <= rule.humidity()[1] && ground >= rule.height()[0] && ground <= rule.height()[1]) {
                biome = index;
                break;
            }
        }
        return new Column(ground, temperature, humidity, biome);
    }

    String biome(Column column) {
        return column.biome() >= 0 ? biomes.get(column.biome()).biome() : "plains";
    }

    Surface surface(Column column) {
        String biome = biome(column);
        for (Surface one : surface) {
            if (one.biome().equals(biome)) {
                return one;
            }
        }
        return fallbackSurface;
    }

    String topBlock(Column column) {
        return column.height() < sea ? fluid : surface(column).top();
    }

    private boolean ground(int x, int y, int z, Column column) {
        if (overhangs <= 0) {
            return y <= column.height();
        }
        double[] w = warped(x, z);
        double density = (column.height() - y) / 16.0 + overhangs * perlin3(seed + 21, w[0] / 48.0, y / 32.0, w[1] / 48.0);
        return density > 0;
    }

    private boolean cave(int x, int y, int z) {
        for (int index = 0; index < caves.size(); index++) {
            Cave cave = caves.get(index);
            long caveSeed = seed + 300 + 10L * index;
            if (cave.type() == 0) {
                double s = cave.a();
                if (Math.abs(perlin3(caveSeed, x / s, y / (s * 0.5), z / s)) < cave.b() && Math.abs(perlin3(caveSeed + 1, x / s, y / (s * 0.5), z / s)) < cave.b()) {
                    return true;
                }
            } else if (perlin3(caveSeed, x / cave.a(), y / (cave.a() * 0.6), z / cave.a()) > cave.b()) {
                return true;
            }
        }
        return false;
    }

    private int reach(Column column) {
        return column.height() + (int) Math.ceil(16.0 * overhangs) + 1;
    }

    /** Fills one column of the chunk, top down, the way {@code Generator::runs} does. */
    private void fillColumn(ChunkData chunk, int lx, int lz, int x, int z, int minY, int maxY) {
        Column column = column(x, z);
        Surface here = surface(column);
        int top = Math.min(Math.max(reach(column), sea), maxY);
        int depth = -1;
        boolean wet = false;
        for (int y = top; y >= minY; y--) {
            if (bedrock && y == minY) {
                chunk.setBlock(lx, y, lz, state("bedrock"));
                continue;
            }
            if (!ground(x, y, z, column)) {
                depth = -1;
                if (y < sea) {
                    chunk.setBlock(lx, y, lz, state(fluid));
                    wet = true;
                } else {
                    wet = false;
                }
                continue;
            }
            depth++;
            if (!caves.isEmpty() && y > minY + 4 && y < column.height() - 4 && cave(x, y, z)) {
                continue;
            }
            String block;
            if (depth == 0) {
                block = wet || y < sea - 1 ? here.underwater() : here.top();
            } else if (depth <= here.depth()) {
                block = wet ? here.underwater() : here.under();
            } else {
                block = stone;
            }
            chunk.setBlock(lx, y, lz, state(block));
        }
    }

    private int topY(int x, int z, Column column) {
        if (overhangs <= 0) {
            return column.height();
        }
        int y = reach(column);
        while (y > column.height() - 64 && !ground(x, y, z, column)) {
            y--;
        }
        return y;
    }

    List<Place> features(int cx, int cz, int minY, int maxY) {
        List<Place> out = new ArrayList<>();
        for (int index = 0; index < features.size(); index++) {
            Feature feature = features.get(index);
            // A structure reaches up to 16 blocks from its middle, so it may start two chunks away.
            int reach = feature.type() == 4 ? 2 : 1;
            for (int nx = cx - reach; nx <= cx + reach; nx++) {
                for (int nz = cz - reach; nz <= cz + reach; nz++) {
                    Rng rng = new Rng(hash(seed + 5000 + index, nx, 0, nz));
                    int count = (int) Math.floor(feature.perChunk());
                    if (rng.unit() < feature.perChunk() - Math.floor(feature.perChunk())) {
                        count++;
                    }
                    for (int i = 0; i < count; i++) {
                        int x = nx * 16 + rng.between(0, 15);
                        int z = nz * 16 + rng.between(0, 15);
                        Column column = column(x, z);
                        if (!feature.biomes().isEmpty() && !feature.biomes().contains(biome(column))) {
                            rng.next();
                            continue;
                        }
                        place(feature, rng, x, z, column, out);
                    }
                }
            }
        }
        out.removeIf(one -> Math.floorDiv(one.x(), 16) != cx || Math.floorDiv(one.z(), 16) != cz || one.y() < minY || one.y() > maxY);
        return out;
    }

    private void place(Feature feature, Rng rng, int x, int z, Column column, List<Place> out) {
        boolean dry = column.height() >= sea;
        switch (feature.type()) {
            case 0 -> {
                int px = x;
                int py = rng.between(feature.from(), feature.to());
                int pz = z;
                for (int i = 0; i < feature.size(); i++) {
                    out.add(new Place(px, py, pz, feature.block(), STONE));
                    switch (rng.between(0, 5)) {
                        case 0 -> px++;
                        case 1 -> px--;
                        case 2 -> py++;
                        case 3 -> py--;
                        case 4 -> pz++;
                        default -> pz--;
                    }
                }
            }
            case 1 -> {
                int tall = rng.between(feature.from(), feature.to());
                if (!dry) {
                    return;
                }
                int ground = topY(x, z, column);
                if (topBlock(column).equals(fluid)) {
                    return;
                }
                int crown = ground + tall;
                for (int dy = -2; dy <= 1; dy++) {
                    int radius = dy >= 0 ? 1 : 2;
                    for (int dx = -radius; dx <= radius; dx++) {
                        for (int dz = -radius; dz <= radius; dz++) {
                            boolean corner = Math.abs(dx) == radius && Math.abs(dz) == radius;
                            if (corner && (dy == 1 || rng.unit() < 0.5)) {
                                continue;
                            }
                            out.add(new Place(x + dx, crown + dy, z + dz, feature.leaves(), AIR));
                        }
                    }
                }
                for (int y = ground + 1; y < crown; y++) {
                    out.add(new Place(x, y, z, feature.block(), ANY));
                }
            }
            case 2 -> {
                if (!dry) {
                    return;
                }
                int ground = topY(x, z, column);
                int r = feature.size();
                for (int dx = -r; dx <= r; dx++) {
                    for (int dy = -r; dy <= r; dy++) {
                        for (int dz = -r; dz <= r; dz++) {
                            if (dx * dx + dy * dy + dz * dz <= r * r) {
                                out.add(new Place(x + dx, ground + dy, z + dz, feature.block(), ANY));
                            }
                        }
                    }
                }
            }
            case 4 -> {
                int quarter = StudioStructures.quarterOf(feature.leaves(), rng.between(0, 3));
                Turned[] turned = structures.get(feature.block());
                if (turned == null || !dry || topBlock(column).equals(fluid)) {
                    return;
                }
                Turned one = turned[quarter];
                int sx = x - one.width() / 2;
                int sy = topY(x, z, column) + 1 + feature.size();
                int sz = z - one.depth() / 2;
                for (int i = 0; i < one.blocks().length; i += 4) {
                    out.add(new Place(sx + one.blocks()[i], sy + one.blocks()[i + 1], sz + one.blocks()[i + 2], one.states()[one.blocks()[i + 3]], ANY));
                }
            }
            default -> {
                if (dry && !topBlock(column).equals(fluid)) {
                    out.add(new Place(x, topY(x, z, column) + 1, z, feature.block(), AIR));
                }
            }
        }
    }

    // ---------- Bukkit ----------

    /** A block by its short name, or one of ours as {@code studio:<id>}; unknown names give stone. */
    private BlockData state(String name) {
        if (name.startsWith("studio:")) {
            // Not kept while unknown: the main world starts before the add-on knows its blocks.
            BlockData known = states.get(name);
            if (known != null) {
                return known;
            }
            BlockData ours = studioBlocks.apply(name.substring("studio:".length()));
            if (ours == null) {
                return Material.STONE.createBlockData();
            }
            states.put(name, ours);
            return ours;
        }
        return states.computeIfAbsent(name, key -> {
            try {
                return Bukkit.createBlockData(key.contains(":") ? key : "minecraft:" + key);
            } catch (IllegalArgumentException unknown) {
                return Material.STONE.createBlockData();
            }
        });
    }

    @Override
    public void generateNoise(WorldInfo info, Random random, int chunkX, int chunkZ, ChunkData chunk) {
        if (vanilla != null) {
            return;
        }
        int minY = chunk.getMinHeight();
        int maxY = chunk.getMaxHeight() - 1;
        for (int lx = 0; lx < 16; lx++) {
            for (int lz = 0; lz < 16; lz++) {
                fillColumn(chunk, lx, lz, chunkX * 16 + lx, chunkZ * 16 + lz, minY, maxY);
            }
        }
    }

    @Override
    public void generateSurface(WorldInfo info, Random random, int chunkX, int chunkZ, ChunkData chunk) {
        if (vanilla != null) {
            return;
        }
        BlockData stoneState = state(stone);
        for (Place one : features(chunkX, chunkZ, chunk.getMinHeight(), chunk.getMaxHeight() - 1)) {
            int lx = one.x() - chunkX * 16;
            int lz = one.z() - chunkZ * 16;
            if (one.only() == AIR && !chunk.getType(lx, one.y(), lz).isAir()) {
                continue;
            }
            if (one.only() == STONE && !chunk.getBlockData(lx, one.y(), lz).equals(stoneState)) {
                continue;
            }
            chunk.setBlock(lx, one.y(), lz, state(one.block()));
        }
    }

    @Override
    public BiomeProvider getDefaultBiomeProvider(WorldInfo info) {
        if (vanilla != null) {
            // The server's own ground and biomes, with ours where a rule holds.
            return new BiomeProvider() {
                @Override
                public Biome getBiome(WorldInfo world, int x, int y, int z) {
                    String ours = vanillaBiome(x, z);
                    return ours != null ? biomeType(ours) : world.vanillaBiomeProvider().getBiome(world, x, y, z);
                }

                @Override
                public Biome getBiome(WorldInfo world, int x, int y, int z, org.bukkit.generator.BiomeParameterPoint point) {
                    String ours = vanillaBiome(x, z);
                    return ours != null ? biomeType(ours) : world.vanillaBiomeProvider().getBiome(world, x, y, z, point);
                }

                @Override
                public List<Biome> getBiomes(WorldInfo world) {
                    List<Biome> out = new ArrayList<>(world.vanillaBiomeProvider().getBiomes(world));
                    for (BiomeRule rule : biomes) {
                        Biome one = biomeType(rule.biome());
                        if (!out.contains(one)) {
                            out.add(one);
                        }
                    }
                    return out;
                }
            };
        }
        return new BiomeProvider() {
            @Override
            public Biome getBiome(WorldInfo world, int x, int y, int z) {
                return biomeType(biome(column(x, z)));
            }

            @Override
            public List<Biome> getBiomes(WorldInfo world) {
                List<Biome> out = new ArrayList<>();
                for (BiomeRule rule : biomes) {
                    Biome one = biomeType(rule.biome());
                    if (!out.contains(one)) {
                        out.add(one);
                    }
                }
                Biome plains = biomeType("plains");
                if (!out.contains(plains)) {
                    out.add(plains);
                }
                return out;
            }
        };
    }

    /** A biome by short name, or one of ours ({@code studio:<id>} → {@code monolab:<id>}, from the studio data pack). */
    private Biome biomeType(String name) {
        return biomeTypes.computeIfAbsent(name, key -> {
            NamespacedKey id = key.startsWith("studio:") ? new NamespacedKey("monolab", key.substring("studio:".length())) : key.contains(":") ? NamespacedKey.fromString(key) : NamespacedKey.minecraft(key);
            Biome found = id == null ? null : RegistryAccess.registryAccess().getRegistry(RegistryKey.BIOME).get(id);
            return found != null ? found : Biome.PLAINS;
        });
    }

    @Override
    public boolean shouldGenerateNoise() {
        return vanilla != null;
    }

    @Override
    public boolean shouldGenerateSurface() {
        return vanilla != null;
    }

    @Override
    public boolean shouldGenerateCaves() {
        return vanilla != null || keepCarvers;
    }

    @Override
    public boolean shouldGenerateDecorations() {
        return vanilla != null || keepDecorations;
    }

    @Override
    public boolean shouldGenerateMobs() {
        return vanilla != null || keepMobs;
    }

    @Override
    public boolean shouldGenerateStructures() {
        return vanilla != null || keepStructures;
    }

    @Override
    public Location getFixedSpawnLocation(World world, Random random) {
        if (vanilla != null) {
            return null;
        }
        Column column = column(0, 0);
        return new Location(world, 0.5, Math.max(topY(0, 0, column), sea) + 1, 0.5);
    }
}

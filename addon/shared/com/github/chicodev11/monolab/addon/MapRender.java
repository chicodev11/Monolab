package com.github.chicodev11.monolab.addon;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Properties;
import java.util.TreeMap;
import java.util.function.BiConsumer;
import java.util.zip.GZIPInputStream;
import java.util.zip.GZIPOutputStream;
import java.util.zip.InflaterInputStream;

final class MapRender {
    static final int SPAN = 512;
    private static final byte[] MAGIC = "MNM2".getBytes(StandardCharsets.US_ASCII);
    private static final long PAUSE_MS = 60_000L;
    private static final String RULES = "3";
    static final int CAVE_Y = 32;
    private static final int OPEN = Integer.MAX_VALUE;
    private static final int VOID = -2;
    private static final java.util.Set<String> DRAWN_STATUS = java.util.Set.of(
            "full", "spawn", "light", "initialize_light", "features", "carvers", "surface");

    private final Path server;
    private final Path out;
    private final BiConsumer<String, Throwable> onTrouble;
    private final Map<String, Integer> colors = new HashMap<>();
    private String game = "";
    private volatile boolean running;
    private Thread thread;

    MapRender(Path server, Path out, BiConsumer<String, Throwable> onTrouble) {
        this.server = server;
        this.out = out;
        this.onTrouble = onTrouble;
    }

    void color(String state, int rgb) {
        String name = state;
        String props = "";
        if (state.startsWith("Block{")) {
            int close = state.indexOf('}');
            name = state.substring(6, close);
            int open = state.indexOf('[', close);
            if (open > 0 && state.endsWith("]")) {
                props = state.substring(open + 1, state.length() - 1);
            }
        }
        rgb &= 0xFFFFFF;
        if (!props.isEmpty()) {
            colors.put(name + "|" + sortProps(props), rgb);
        }
        colors.putIfAbsent(name, rgb);
    }

    private static String sortProps(String props) {
        TreeMap<String, String> sorted = new TreeMap<>();
        for (String pair : props.split(",")) {
            int eq = pair.indexOf('=');
            if (eq > 0) {
                sorted.put(pair.substring(0, eq).trim(), pair.substring(eq + 1).trim());
            }
        }
        StringBuilder text = new StringBuilder();
        for (Map.Entry<String, String> one : sorted.entrySet()) {
            if (text.length() > 0) {
                text.append(',');
            }
            text.append(one.getKey()).append('=').append(one.getValue());
        }
        return text.toString();
    }

    void game(String version) {
        this.game = version == null ? "" : version;
    }

    void start() {
        running = true;
        thread = new Thread(this::loop, "Monolab map");
        thread.setDaemon(true);
        thread.setPriority(Thread.MIN_PRIORITY);
        thread.start();
    }

    void stop() {
        running = false;
        if (thread != null) {
            thread.interrupt();
        }
    }

    private void loop() {
        try {
            Thread.sleep(20_000L);
        } catch (InterruptedException ignored) {
            return;
        }
        try {
            MapColors.build(out, server, game, colors, onTrouble, true);
        } catch (Throwable error) {
            onTrouble.accept("Couldn't build the map palette", error);
        }
        while (running) {
            try {
                MapColors.build(out, server, game, colors, onTrouble, false);
            } catch (Throwable error) {
                onTrouble.accept("Couldn't build the map palette", error);
            }
            try {
                pass();
            } catch (Throwable error) {
                onTrouble.accept("Couldn't draw the map", error);
            }
            try {
                Thread.sleep(PAUSE_MS);
            } catch (InterruptedException ignored) {
                return;
            }
        }
    }

    private void pass() throws IOException {
        Map<String, Path> realms = realms();
        List<Path[]> todo = new ArrayList<>();
        int total = 0;
        Path rules = out.resolve("rules.txt");
        boolean redo = !Files.isRegularFile(rules) || !RULES.equals(Files.readString(rules).trim());
        List<Integer> cutoffs = new ArrayList<>();
        for (Map.Entry<String, Path> realm : realms.entrySet()) {
            List<String> layers = realm.getKey().equals("overworld") ? List.of("overworld", "overworld-cave") : List.of(realm.getKey());
            for (String layer : layers) {
                int cutoff = layer.equals("nether") ? 127 : layer.endsWith("-cave") ? CAVE_Y : OPEN;
                Path dir = out.resolve(layer);
                try (var files = Files.list(realm.getValue())) {
                    for (Path mca : (Iterable<Path>) files::iterator) {
                        String name = mca.getFileName().toString();
                        if (!name.startsWith("r.") || !name.endsWith(".mca")) {
                            continue;
                        }
                        total++;
                        Path tile = dir.resolve(name.substring(0, name.length() - 4) + ".mnm");
                        if (!redo && Files.isRegularFile(tile) && !changedSince(mca, Files.getLastModifiedTime(tile).toMillis())) {
                            continue;
                        }
                        todo.add(new Path[] {mca, tile});
                        cutoffs.add(cutoff);
                    }
                }
            }
        }
        int done = total - todo.size();
        status(done, total);
        for (int i = 0; i < todo.size(); i++) {
            Path[] one = todo.get(i);
            if (!running) {
                return;
            }
            try {
                renderRegion(one[0], one[1], cutoffs.get(i));
            } catch (IOException error) {
                onTrouble.accept("Couldn't draw region " + one[0].getFileName(), error);
            }
            done++;
            status(done, total);
            Thread.yield();
        }
        if (redo) {
            Files.createDirectories(out);
            Files.writeString(rules, RULES);
        }
    }

    private static boolean changedSince(Path mca, long tileMillis) throws IOException {
        if (Files.getLastModifiedTime(mca).toMillis() > tileMillis) {
            return true;
        }
        long newest = 0;
        try (InputStream in = Files.newInputStream(mca)) {
            byte[] head = in.readNBytes(8192);
            if (head.length < 8192) {
                return false;
            }
            for (int at = 4096; at < 8192; at += 4) {
                long stamp = ((head[at] & 0xFFL) << 24) | ((head[at + 1] & 0xFFL) << 16) | ((head[at + 2] & 0xFFL) << 8) | (head[at + 3] & 0xFFL);
                newest = Math.max(newest, stamp);
            }
        }
        return (newest + 1) * 1000 > tileMillis;
    }

    private void status(int done, int total) {
        try {
            Files.createDirectories(out);
            write(out.resolve("status.json"), new Json()
                    .put("at", System.currentTimeMillis())
                    .put("done", done)
                    .put("total", total)
                    .end()
                    .getBytes(StandardCharsets.UTF_8));
        } catch (IOException ignored) {
            // Only a progress indicator.
        }
    }

    private Map<String, Path> realms() {
        String level = "world";
        Properties props = new Properties();
        try (InputStream in = Files.newInputStream(server.resolve("server.properties"))) {
            props.load(in);
            level = props.getProperty("level-name", "world").trim();
        } catch (IOException ignored) {
            // Default name if missing.
        }
        Path world = server.resolve(level);
        Map<String, Path> found = new LinkedHashMap<>();
        pick(found, "overworld", world.resolve("dimensions/minecraft/overworld/region"), world.resolve("region"));
        pick(found, "nether",
                world.resolve("dimensions/minecraft/the_nether/region"),
                world.resolve("DIM-1/region"),
                server.resolve(level + "_nether").resolve("DIM-1/region"));
        pick(found, "end",
                world.resolve("dimensions/minecraft/the_end/region"),
                world.resolve("DIM1/region"),
                server.resolve(level + "_the_end").resolve("DIM1/region"));
        Path dims = world.resolve("dimensions");
        if (Files.isDirectory(dims)) {
            try (var walk = Files.walk(dims, 5)) {
                walk.filter(one -> one.getFileName() != null && one.getFileName().toString().equals("region") && Files.isDirectory(one))
                        .forEach(region -> {
                            String rel = dims.relativize(region.getParent()).toString().replace(java.io.File.separatorChar, '/');
                            if (rel.equals("minecraft/overworld") || rel.equals("minecraft/the_nether") || rel.equals("minecraft/the_end")) {
                                return;
                            }
                            String key = rel.toLowerCase(java.util.Locale.ROOT).replace('/', '.').replaceAll("[^a-z0-9_.-]", "_");
                            if (!key.isEmpty() && key.length() <= 96) {
                                found.putIfAbsent(key, region);
                            }
                        });
            } catch (IOException ignored) {
                // If scanning fails, just the three dimensions.
            }
        }
        return found;
    }

    private static void pick(Map<String, Path> into, String key, Path... candidates) {
        for (Path one : candidates) {
            if (Files.isDirectory(one)) {
                into.put(key, one);
                return;
            }
        }
    }

    private static final class Region {
        final List<String> blocks = new ArrayList<>();
        final Map<String, Integer> blockAt = new HashMap<>();
        final List<String> biomes = new ArrayList<>();
        final Map<String, Integer> biomeAt = new HashMap<>();
        final int[] block = new int[SPAN * SPAN];
        final int[] floor = new int[SPAN * SPAN];
        final int[] biome = new int[SPAN * SPAN];
        final short[] height = new short[SPAN * SPAN];
        final int[] depth = new int[SPAN * SPAN];
        final int[] light = new int[SPAN * SPAN];

        Region() {
            java.util.Arrays.fill(block, -1);
            java.util.Arrays.fill(floor, -1);
        }

        int block(String name) {
            return blockAt.computeIfAbsent(name, key -> {
                blocks.add(key);
                return blocks.size() - 1;
            });
        }

        int biome(String name) {
            return biomeAt.computeIfAbsent(name, key -> {
                biomes.add(key);
                return biomes.size() - 1;
            });
        }
    }

    private void renderRegion(Path mca, Path tile, int cutoff) throws IOException {
        Region region = new Region();
        readTile(tile, region);
        byte[] raw = Files.readAllBytes(mca);
        if (raw.length < 8192) {
            return;
        }
        for (int index = 0; index < 1024; index++) {
            Map<String, Object> chunk;
            try {
                chunk = readChunk(raw, index);
            } catch (IOException | RuntimeException broken) {
                continue;
            }
            if (chunk == null) {
                continue;
            }
            Object status = chunk.get("Status");
            if (status instanceof String text && !DRAWN_STATUS.contains(text.substring(text.indexOf(':') + 1))) {
                continue;
            }
            Object sections = chunk.get("sections");
            if (!(sections instanceof List<?> list)) {
                continue;
            }
            surface(list, cutoff, (index % 32) * 16, (index / 32) * 16, region);
        }
        Files.createDirectories(tile.getParent());
        ByteArrayOutputStream bytes = new ByteArrayOutputStream(SPAN * SPAN * 2);
        try (java.io.DataOutputStream gz = new java.io.DataOutputStream(new GZIPOutputStream(bytes))) {
            gz.write(MAGIC);
            gz.writeShort(region.blocks.size());
            for (String one : region.blocks) {
                gz.writeUTF(one);
            }
            gz.writeShort(region.biomes.size());
            for (String one : region.biomes) {
                gz.writeUTF(one);
            }
            byte[] row = new byte[SPAN * 10];
            for (int z = 0; z < SPAN; z++) {
                for (int x = 0; x < SPAN; x++) {
                    int at = z * SPAN + x;
                    int o = x * 10;
                    int b = region.block[at] == VOID ? 0xFFFE : region.block[at] < 0 ? 0xFFFF : region.block[at];
                    int f = region.floor[at] < 0 ? 0xFFFF : region.floor[at];
                    row[o] = (byte) (b >> 8);
                    row[o + 1] = (byte) b;
                    row[o + 2] = (byte) (f >> 8);
                    row[o + 3] = (byte) f;
                    row[o + 4] = (byte) (region.biome[at] >> 8);
                    row[o + 5] = (byte) region.biome[at];
                    row[o + 6] = (byte) (region.height[at] >> 8);
                    row[o + 7] = (byte) region.height[at];
                    row[o + 8] = (byte) region.depth[at];
                    row[o + 9] = (byte) region.light[at];
                }
                gz.write(row);
            }
        }
        write(tile, bytes.toByteArray());
    }

    private static void readTile(Path tile, Region region) {
        if (!Files.isRegularFile(tile)) {
            return;
        }
        try (DataInputStream in = new DataInputStream(new java.io.BufferedInputStream(new GZIPInputStream(Files.newInputStream(tile))))) {
            byte[] magic = new byte[4];
            in.readFully(magic);
            if (!java.util.Arrays.equals(magic, MAGIC)) {
                return;
            }
            int[] blockMap = new int[in.readUnsignedShort()];
            for (int i = 0; i < blockMap.length; i++) {
                blockMap[i] = region.block(in.readUTF());
            }
            int[] biomeMap = new int[in.readUnsignedShort()];
            for (int i = 0; i < biomeMap.length; i++) {
                biomeMap[i] = region.biome(in.readUTF());
            }
            byte[] row = new byte[SPAN * 10];
            for (int z = 0; z < SPAN; z++) {
                in.readFully(row);
                for (int x = 0; x < SPAN; x++) {
                    int at = z * SPAN + x;
                    int o = x * 10;
                    int b = ((row[o] & 0xFF) << 8) | (row[o + 1] & 0xFF);
                    int f = ((row[o + 2] & 0xFF) << 8) | (row[o + 3] & 0xFF);
                    int bio = ((row[o + 4] & 0xFF) << 8) | (row[o + 5] & 0xFF);
                    region.block[at] = b == 0xFFFE ? VOID : b == 0xFFFF || b >= blockMap.length ? -1 : blockMap[b];
                    region.floor[at] = f == 0xFFFF || f >= blockMap.length ? -1 : blockMap[f];
                    region.biome[at] = bio < biomeMap.length ? biomeMap[bio] : 0;
                    region.height[at] = (short) (((row[o + 6] & 0xFF) << 8) | (row[o + 7] & 0xFF));
                    region.depth[at] = row[o + 8] & 0xFF;
                    region.light[at] = row[o + 9] & 0xFF;
                }
            }
        } catch (IOException | RuntimeException broken) {
            java.util.Arrays.fill(region.block, -1);
        }
    }

    private void surface(List<?> sections, int cutoff, int baseX, int baseZ, Region region) {
        boolean ceiling = cutoff != OPEN;
        List<Section> parsed = new ArrayList<>();
        for (Object one : sections) {
            if (one instanceof Map<?, ?> map) {
                Section section = Section.of(map, this);
                if (section != null) {
                    parsed.add(section);
                }
            }
        }
        parsed.sort((a, b) -> Integer.compare(b.y, a.y));
        if (parsed.isEmpty()) {
            if (!ceiling) {
                for (int z = 0; z < 16; z++) {
                    for (int x = 0; x < 16; x++) {
                        region.block[(baseZ + z) * SPAN + baseX + x] = VOID;
                    }
                }
            }
            return;
        }
        for (int z = 0; z < 16; z++) {
            for (int x = 0; x < 16; x++) {
                String top = null;
                String floor = null;
                int topY = Integer.MIN_VALUE;
                int water = 0;
                Section topSection = null;
                int topLocalY = 0;
                boolean open = !ceiling;
                found:
                for (Section section : parsed) {
                    for (int y = 15; y >= 0; y--) {
                        int worldY = section.y * 16 + y;
                        if (ceiling && worldY >= cutoff) {
                            continue;
                        }
                        int state = section.state(y * 256 + z * 16 + x);
                        boolean fluid = section.fluid[state];
                        boolean seen = section.seen[state];
                        if (!open) {
                            if (!seen && !fluid) {
                                open = true;
                            }
                            continue;
                        }
                        if (topY == Integer.MIN_VALUE) {
                            if (fluid) {
                                topY = worldY;
                                top = "minecraft:water";
                                water = 1;
                                topSection = section;
                                topLocalY = y;
                            } else if (seen) {
                                topY = worldY;
                                top = section.names[state];
                                topSection = section;
                                topLocalY = y;
                                break found;
                            }
                        } else if (fluid) {
                            water++;
                        } else if (seen) {
                            floor = section.names[state];
                            break found;
                        }
                    }
                }
                int at = (baseZ + z) * SPAN + baseX + x;
                if (top == null) {
                    if (!ceiling) {
                        region.block[at] = VOID;
                    }
                    continue;
                }
                region.block[at] = region.block(top);
                region.floor[at] = floor == null ? -1 : region.block(floor);
                region.biome[at] = region.biome(topSection.biome(x, topLocalY, z));
                region.depth[at] = Math.min(water, 254);
                region.height[at] = (short) topY;
                region.light[at] = lightAbove(parsed, topSection, x, topLocalY, z);
            }
        }
    }

    private static int lightAbove(List<Section> parsed, Section section, int x, int y, int z) {
        Section at = section;
        int ly = y + 1;
        if (ly > 15) {
            at = null;
            for (Section one : parsed) {
                if (one.y == section.y + 1) {
                    at = one;
                }
            }
            ly = 0;
        }
        if (at == null || at.light == null || at.light.length < 2048) {
            return 0;
        }
        int index = ly * 256 + z * 16 + x;
        int pair = at.light[index >> 1] & 0xFF;
        return (index & 1) == 0 ? pair & 0x0F : pair >> 4;
    }

    private static final java.util.Set<String> ALWAYS_WET = java.util.Set.of(
            "minecraft:water", "minecraft:kelp", "minecraft:kelp_plant", "minecraft:seagrass",
            "minecraft:tall_seagrass", "minecraft:bubble_column");

    private static final class Section {
        int y;
        String[] names;
        boolean[] seen;
        boolean[] fluid;
        long[] data;
        int bits;
        byte[] light;
        List<String> biomeNames;
        long[] biomeData;
        int biomeBits;

        static Section of(Map<?, ?> map, MapRender owner) {
            Object y = map.get("Y");
            Object states = map.get("block_states");
            if (!(y instanceof Number number) || !(states instanceof Map<?, ?> blocks)) {
                return null;
            }
            Object palette = blocks.get("palette");
            if (!(palette instanceof List<?> list) || list.isEmpty()) {
                return null;
            }
            Section section = new Section();
            section.y = number.intValue();
            section.names = new String[list.size()];
            section.seen = new boolean[list.size()];
            section.fluid = new boolean[list.size()];
            for (int i = 0; i < list.size(); i++) {
                Object item = list.get(i);
                Map<?, ?> entry = item instanceof Map<?, ?> wrapped ? wrapped : Map.of();
                Object rawName = item instanceof String text ? text
                        : entry.get("Name") != null ? entry.get("Name")
                        : entry.get("id") != null ? entry.get("id")
                        : entry.get("");
                String name = rawName instanceof String text ? text : "minecraft:air";
                section.names[i] = name;
                Object rawProps = entry.get("Properties") != null ? entry.get("Properties") : entry.get("properties");
                if (rawProps instanceof Map<?, ?> properties) {
                    section.fluid[i] = "true".equals(String.valueOf(properties.get("waterlogged")));
                }
                if (ALWAYS_WET.contains(name)) {
                    section.fluid[i] = true;
                }
                Integer mapColor = owner.colors.get(name);
                section.seen[i] = mapColor == null ? !name.endsWith(":air") && !name.endsWith("_air") : mapColor != 0;
            }
            if (list.size() > 1 && blocks.get("data") instanceof long[] longs) {
                section.data = longs;
                section.bits = Math.max(4, 32 - Integer.numberOfLeadingZeros(list.size() - 1));
            }
            if (map.get("BlockLight") instanceof byte[] light) {
                section.light = light;
            }
            if (map.get("biomes") instanceof Map<?, ?> biomes && biomes.get("palette") instanceof List<?> names && !names.isEmpty()) {
                section.biomeNames = new ArrayList<>();
                for (Object one : names) {
                    if (one instanceof Map<?, ?> wrapped) {
                        one = wrapped.get("") != null ? wrapped.get("") : wrapped.get("id");
                    }
                    section.biomeNames.add(String.valueOf(one));
                }
                if (names.size() > 1 && biomes.get("data") instanceof long[] longs) {
                    section.biomeData = longs;
                    section.biomeBits = 32 - Integer.numberOfLeadingZeros(names.size() - 1);
                }
            }
            return section;
        }

        int state(int index) {
            if (data == null) {
                return 0;
            }
            int perLong = 64 / bits;
            int word = index / perLong;
            if (word >= data.length) {
                return 0;
            }
            int value = (int) ((data[word] >>> ((index % perLong) * bits)) & ((1L << bits) - 1));
            return value < names.length ? value : 0;
        }

        String biome(int x, int y, int z) {
            if (biomeNames == null) {
                return "minecraft:plains";
            }
            if (biomeData == null) {
                return biomeNames.get(0);
            }
            int index = (y / 4) * 16 + (z / 4) * 4 + (x / 4);
            int perLong = 64 / biomeBits;
            int word = index / perLong;
            if (word >= biomeData.length) {
                return biomeNames.get(0);
            }
            int value = (int) ((biomeData[word] >>> ((index % perLong) * biomeBits)) & ((1L << biomeBits) - 1));
            return biomeNames.get(Math.min(value, biomeNames.size() - 1));
        }
    }

    private static Map<String, Object> readChunk(byte[] raw, int index) throws IOException {
        int at = index * 4;
        int offset = ((raw[at] & 0xFF) << 16) | ((raw[at + 1] & 0xFF) << 8) | (raw[at + 2] & 0xFF);
        if (offset == 0) {
            return null;
        }
        int start = offset * 4096;
        if (start + 5 > raw.length) {
            return null;
        }
        int length = ((raw[start] & 0xFF) << 24) | ((raw[start + 1] & 0xFF) << 16)
                | ((raw[start + 2] & 0xFF) << 8) | (raw[start + 3] & 0xFF);
        int kind = raw[start + 4];
        if (length <= 1 || start + 4 + length > raw.length) {
            return null;
        }
        InputStream body = new ByteArrayInputStream(raw, start + 5, length - 1);
        InputStream plain = switch (kind) {
            case 1 -> new GZIPInputStream(body);
            case 2 -> new InflaterInputStream(body);
            case 3 -> body;
            default -> null;
        };
        if (plain == null) {
            return null;
        }
        try (DataInputStream in = new DataInputStream(new java.io.BufferedInputStream(plain))) {
            if (in.readByte() != 10) {
                return null;
            }
            in.readUTF();
            return compound(in);
        }
    }

    private static Map<String, Object> compound(DataInputStream in) throws IOException {
        Map<String, Object> map = new HashMap<>();
        while (true) {
            byte type = in.readByte();
            if (type == 0) {
                return map;
            }
            String name = in.readUTF();
            map.put(name, tag(type, in));
        }
    }

    private static Object tag(byte type, DataInputStream in) throws IOException {
        switch (type) {
            case 1: return in.readByte();
            case 2: return in.readShort();
            case 3: return in.readInt();
            case 4: return in.readLong();
            case 5: return in.readFloat();
            case 6: return in.readDouble();
            case 7: {
                int n = in.readInt();
                byte[] bytes = new byte[n];
                in.readFully(bytes);
                return bytes;
            }
            case 8: return in.readUTF();
            case 9: {
                byte inner = in.readByte();
                int n = in.readInt();
                List<Object> list = new ArrayList<>(Math.max(0, Math.min(n, 4096)));
                for (int i = 0; i < n; i++) {
                    list.add(tag(inner, in));
                }
                return list;
            }
            case 10: return compound(in);
            case 11: {
                int n = in.readInt();
                in.skipNBytes(n * 4L);
                return null;
            }
            case 12: {
                int n = in.readInt();
                long[] longs = new long[n];
                for (int i = 0; i < n; i++) {
                    longs[i] = in.readLong();
                }
                return longs;
            }
            default:
                throw new IOException("Unknown NBT type " + type);
        }
    }

    private static void write(Path target, byte[] bytes) throws IOException {
        Path temp = target.resolveSibling(target.getFileName() + ".tmp");
        Files.write(temp, bytes);
        try {
            Files.move(temp, target, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
        } catch (IOException atomicFailed) {
            Files.move(temp, target, StandardCopyOption.REPLACE_EXISTING);
        }
    }
}

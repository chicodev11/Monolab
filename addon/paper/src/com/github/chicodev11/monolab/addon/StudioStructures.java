package com.github.chicodev11.monolab.addon;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ThreadLocalRandom;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.block.data.BlockData;
import org.bukkit.entity.Player;
import org.bukkit.plugin.Plugin;

/**
 * Structures ({@code structure} files, same rules as {@code studio_format::structure} on Pumpkin): the block lists
 * the app made from {@code .schem}/{@code .nbt} files ({@code studio/structures/<id>.json}). The {@code structure}
 * action puts one down, a slice each tick, at the player or at a spot.
 */
final class StudioStructures {
    static final String[] ROTATIONS = {"0", "90", "180", "270", "random"};
    private static final int PER_TICK = 8192;
    private static final int MOST_BLOCKS = 1_000_000;
    private static final String[] SIDES = {"north", "east", "south", "west"};

    private final Plugin plugin;
    private final StudioActions actions;
    private final Studio studio;
    private final Path folder;
    private volatile Map<String, Map<String, Object>> structures = Map.of();
    private final Map<String, Built> cache = new ConcurrentHashMap<>();
    private final ArrayDeque<Job> queue = new ArrayDeque<>();
    private ScheduledTask task;

    record Built(long stamp, int sx, int sy, int sz, List<String> palette, int[] blocks) {}

    private record Job(World world, int[] at, List<BlockData> states, int[] blocks, int[] cursor) {}

    StudioStructures(Plugin plugin, StudioActions actions, Studio studio, Path studioRoot) {
        this.plugin = plugin;
        this.actions = actions;
        this.studio = studio;
        this.folder = studioRoot.resolve("structures");
    }

    void register() {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "structure";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, Map<String, Object>> next = new HashMap<>();
                for (Studio.Resource one : resources) {
                    if (!Files.isRegularFile(folder.resolve(one.id() + ".json"))) {
                        studio.problem("structure", one.id(), "Bring a .schem or .nbt file in from the app first (Import file).");
                    }
                    next.put(one.id(), one.data());
                }
                structures = Map.copyOf(next);
            }
        });
        actions.action("structure", (step, run) -> {
            String id = StudioText.text(step, "structure", "").trim();
            Map<String, Object> data = structures.get(id);
            if (data == null) {
                return;
            }
            String rotation = StudioText.text(step, "rotation", "").trim();
            if (rotation.isEmpty()) {
                rotation = StudioText.text(data, "rotation", "0");
            }
            int quarter = quarterOf(rotation, ThreadLocalRandom.current().nextInt(4));
            if (StudioText.text(step, "at", "player").equals("spot")) {
                World world = Bukkit.getWorld(StudioText.text(step, "world", "world"));
                if (world != null) {
                    place(id, data, world, new int[] {(int) Math.floor(decimal(step, "x", 0)), (int) Math.floor(decimal(step, "y", 64)), (int) Math.floor(decimal(step, "z", 0))}, quarter);
                }
            } else if (run.player() != null) {
                Location spot = run.player().getLocation();
                place(id, data, spot.getWorld(), new int[] {spot.getBlockX(), spot.getBlockY(), spot.getBlockZ()}, quarter);
            }
        });
        task = Bukkit.getGlobalRegionScheduler().runAtFixedRate(plugin, ignored -> tick(), 1L, 1L);
    }

    void stop() {
        if (task != null) {
            task.cancel();
        }
        synchronized (queue) {
            queue.clear();
        }
    }

    // ---------- rules (studio_format::structure) ----------

    private static double decimal(Map<String, Object> data, String name, double fallback) {
        Object value = data.get(name);
        if (value instanceof Number number) {
            return number.doubleValue();
        }
        if (value instanceof String text) {
            try {
                return Double.parseDouble(text.trim());
            } catch (NumberFormatException ignored) {
                return fallback;
            }
        }
        return fallback;
    }

    static int quarterOf(String rotation, int roll) {
        return switch (rotation.trim()) {
            case "90" -> 1;
            case "180" -> 2;
            case "270" -> 3;
            case "random" -> Math.floorMod(roll, 4);
            default -> 0;
        };
    }

    private static String turnSide(String side, int quarter) {
        for (int i = 0; i < 4; i++) {
            if (SIDES[i].equals(side)) {
                return SIDES[(i + quarter) % 4];
            }
        }
        return null;
    }

    /** The state turned {@code quarter} times clockwise (facing, axis, rotation, fence/wall/pane sides). */
    static String rotateState(String state, int quarter) {
        quarter = Math.floorMod(quarter, 4);
        int open = state.indexOf('[');
        if (quarter == 0 || open < 0) {
            return state;
        }
        String name = state.substring(0, open);
        String rest = state.substring(open + 1).replaceAll("]+$", "");
        List<String> turned = new ArrayList<>();
        for (String one : rest.split(",")) {
            int eq = one.indexOf('=');
            if (eq < 0) {
                continue;
            }
            String key = one.substring(0, eq).trim();
            String value = one.substring(eq + 1).trim();
            String side = turnSide(key, quarter);
            if (key.equals("facing") && turnSide(value, quarter) != null) {
                turned.add("facing=" + turnSide(value, quarter));
            } else if (key.equals("axis") && quarter % 2 == 1 && (value.equals("x") || value.equals("z"))) {
                turned.add("axis=" + (value.equals("x") ? "z" : "x"));
            } else if (key.equals("rotation") && value.matches("\\d{1,2}")) {
                turned.add("rotation=" + (Integer.parseInt(value) + quarter * 4) % 16);
            } else if (side != null) {
                turned.add(side + "=" + value);
            } else {
                turned.add(key + "=" + value);
            }
        }
        return name + "[" + String.join(",", turned) + "]";
    }

    private Built read(String id) {
        Path file = folder.resolve(id + ".json");
        try {
            long stamp = Files.getLastModifiedTime(file).toMillis();
            Built known = cache.get(id);
            if (known != null && known.stamp() == stamp) {
                return known;
            }
            Built built = load(file, stamp);
            cache.put(id, built);
            return built;
        } catch (Exception failed) {
            return null;
        }
    }

    /** Reads a block list file ({@code structures/<id>.json}). */
    static Built load(Path file, long stamp) throws java.io.IOException {
        {
            JsonObject root = JsonParser.parseString(Files.readString(file, StandardCharsets.UTF_8)).getAsJsonObject();
            JsonArray size = root.getAsJsonArray("size");
            List<String> palette = new ArrayList<>();
            for (JsonElement one : root.getAsJsonArray("palette")) {
                palette.add(one.getAsString());
            }
            JsonArray flat = root.getAsJsonArray("blocks");
            int count = Math.min(flat.size() / 4, MOST_BLOCKS);
            int[] blocks = new int[count * 4];
            for (int i = 0; i < count * 4; i++) {
                blocks[i] = flat.get(i).getAsInt();
            }
            return new Built(stamp, size.get(0).getAsInt(), size.get(1).getAsInt(), size.get(2).getAsInt(), palette, blocks);
        }
    }

    /** The blocks turned {@code quarter} times clockwise around the low corner, still starting at (0, 0, 0). */
    static int[] turned(Built built, int quarter) {
        int[] blocks = built.blocks().clone();
        for (int i = 0; i < blocks.length; i += 4) {
            int x = blocks[i];
            int z = blocks[i + 2];
            int nx;
            int nz;
            switch (quarter) {
                case 1 -> { nx = built.sz() - 1 - z; nz = x; }
                case 2 -> { nx = built.sx() - 1 - x; nz = built.sz() - 1 - z; }
                case 3 -> { nx = z; nz = built.sx() - 1 - x; }
                default -> { nx = x; nz = z; }
            }
            blocks[i] = nx;
            blocks[i + 2] = nz;
        }
        return blocks;
    }

    /** Queues a structure with its middle (or corner) at {@code at}. */
    void place(String id, Map<String, Object> data, World world, int[] at, int quarter) {
        Built built = read(id);
        if (built == null || world == null) {
            return;
        }
        Map<String, BlockData> made = new LinkedHashMap<>();
        List<BlockData> states = new ArrayList<>();
        for (String state : built.palette()) {
            String turned = rotateState(state, quarter);
            BlockData one = made.computeIfAbsent(turned, StudioStructures::blockData);
            states.add(one);
        }
        boolean odd = quarter % 2 == 1;
        int width = odd ? built.sz() : built.sx();
        int depth = odd ? built.sx() : built.sz();
        boolean centered = !Boolean.FALSE.equals(data.get("centered"));
        int lift = (int) Math.max(-64, Math.min(64, decimal(data, "lift", 0)));
        int[] corner = {centered ? at[0] - width / 2 : at[0], at[1] + lift, centered ? at[2] - depth / 2 : at[2]};
        int[] blocks = turned(built, quarter);
        synchronized (queue) {
            queue.add(new Job(world, corner, states, blocks, new int[] {0}));
        }
    }

    private static BlockData blockData(String state) {
        String full = state.contains(":") ? state : "minecraft:" + state;
        try {
            return Bukkit.createBlockData(full);
        } catch (IllegalArgumentException wrong) {
            int open = full.indexOf('[');
            try {
                return open < 0 ? null : Bukkit.createBlockData(full.substring(0, open));
            } catch (IllegalArgumentException alsoWrong) {
                return null;
            }
        }
    }

    /** Hands the next slice of queued blocks to the regions that own them. */
    private void tick() {
        int left = PER_TICK;
        while (left > 0) {
            Job job;
            synchronized (queue) {
                job = queue.peek();
            }
            if (job == null) {
                return;
            }
            int start = job.cursor()[0];
            int end = Math.min(job.blocks().length, start + left * 4);
            Map<Long, List<Integer>> byChunk = new HashMap<>();
            for (int i = start; i < end; i += 4) {
                int x = job.at()[0] + job.blocks()[i];
                int z = job.at()[2] + job.blocks()[i + 2];
                byChunk.computeIfAbsent(((long) (x >> 4) << 32) ^ ((z >> 4) & 0xffffffffL), ignored -> new ArrayList<>()).add(i);
            }
            for (List<Integer> slice : byChunk.values()) {
                int first = slice.get(0);
                Location spot = new Location(job.world(), job.at()[0] + job.blocks()[first], 0, job.at()[2] + job.blocks()[first + 2]);
                Bukkit.getRegionScheduler().execute(plugin, spot, () -> {
                    for (int i : slice) {
                        BlockData state = job.states().get(job.blocks()[i + 3]);
                        if (state != null) {
                            job.world().getBlockAt(job.at()[0] + job.blocks()[i], job.at()[1] + job.blocks()[i + 1], job.at()[2] + job.blocks()[i + 2]).setBlockData(state, false);
                        }
                    }
                });
            }
            left -= (end - start) / 4;
            job.cursor()[0] = end;
            if (end >= job.blocks().length) {
                synchronized (queue) {
                    queue.poll();
                }
            }
        }
    }
}

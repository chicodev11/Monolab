package com.github.chicodev11.monolab.addon;

import java.io.IOException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.function.Consumer;
import java.util.function.Predicate;
import java.util.regex.Pattern;
import net.kyori.adventure.bossbar.BossBar;
import org.bukkit.Bukkit;
import org.bukkit.Difficulty;
import org.bukkit.GameMode;
import org.bukkit.GameRule;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.World;
import org.bukkit.WorldCreator;
import org.bukkit.attribute.AttributeInstance;
import org.bukkit.block.data.BlockData;
import org.bukkit.block.structure.StructureRotation;
import org.bukkit.entity.Player;
import org.bukkit.inventory.ItemStack;
import org.bukkit.permissions.PermissionAttachment;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.RegisteredServiceProvider;

/**
 * The rule steps added with the node editor: take items, experience, game mode, kick, blocks and areas (set, fill,
 * copy and paste), boss bars, hunger and health, permissions and groups (LuckPerms when it's installed), and world
 * management; and the conditions has items, game mode, health, level, sneaking, time, weather and group.
 */
final class StudioSteps {
    /** A fill may change this many blocks at most; a copied area may hold this many. */
    static final int MOST_FILL = 32768;
    static final int MOST_REGION = 262144;
    private static final Pattern NAME = Pattern.compile("[a-z0-9_-]{1,64}");
    private static final Pattern WORLD = Pattern.compile("[A-Za-z0-9_.-]{1,64}");
    private static final Pattern NODE = Pattern.compile("[A-Za-z0-9_.*-]{1,128}");

    private final Plugin plugin;
    private final StudioActions actions;
    private final Path regions;
    private final java.util.function.BiConsumer<String, Throwable> warn;
    private final Map<UUID, BossBar> bars = new ConcurrentHashMap<>();
    private final Map<UUID, PermissionAttachment> attachments = new ConcurrentHashMap<>();

    StudioSteps(Plugin plugin, StudioActions actions, Path studioRoot, java.util.function.BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.actions = actions;
        this.regions = studioRoot.resolve("data").resolve("regions");
        this.warn = warn;
    }

    void register() {
        actions.action("take", (one, run) -> actions.onPlayer(run.player(), () -> take(run.player(), StudioText.text(one, "item", ""), StudioText.number(one, "amount", 1, 1, 64 * 36))));
        actions.action("xp", (one, run) -> actions.onPlayer(run.player(), () -> xp(run.player(), one)));
        actions.action("gamemode", (one, run) -> actions.onPlayer(run.player(), () -> {
            GameMode mode = gameMode(StudioText.text(one, "mode", "survival"));
            if (mode != null) {
                run.player().setGameMode(mode);
            }
        }));
        actions.action("kick", (one, run) -> actions.onPlayer(run.player(), () -> run.player().kick(actions.render(StudioText.text(one, "text", ""), run))));
        actions.action("food", (one, run) -> actions.onPlayer(run.player(), () -> run.player().setFoodLevel(StudioText.number(one, "amount", 20, 0, 20))));
        actions.action("health", (one, run) -> actions.onPlayer(run.player(), () -> {
            Player player = run.player();
            AttributeInstance max = StudioMobs.attributeOf(player, "health");
            double most = max == null ? 20 : max.getValue();
            player.setHealth(Math.max(0, Math.min(most, decimal(one.get("amount"), 20))));
        }));
        actions.action("bossbar", this::bossbar);
        actions.action("block", this::block);
        actions.action("fill", this::fill);
        actions.action("region", this::region);
        actions.action("set_permission", this::permission);
        actions.action("set_group", this::group);
        actions.action("manage_world", this::world);

        actions.condition("has_item", (one, run) -> run.player() != null && count(run.player(), StudioText.text(one, "item", "")) >= StudioText.number(one, "amount", 1, 1, 64 * 36));
        actions.condition("gamemode", (one, run) -> {
            List<String> modes = StudioText.lines(one, "modes");
            modes.removeIf(String::isBlank);
            return run.player() != null && (modes.isEmpty() || modes.contains(run.player().getGameMode().name().toLowerCase(Locale.ROOT)));
        });
        actions.condition("health", (one, run) -> run.player() != null && compare(run.player().getHealth(), StudioText.text(one, "op", ">="), decimal(one.get("value"), 10)));
        actions.condition("level", (one, run) -> run.player() != null && compare(run.player().getLevel(), StudioText.text(one, "op", ">="), decimal(one.get("value"), 10)));
        actions.condition("sneaking", (one, run) -> run.player() != null && run.player().isSneaking());
        actions.condition("time", (one, run) -> {
            World world = run.player() != null ? run.player().getWorld() : Bukkit.getWorlds().get(0);
            long time = world.getTime();
            boolean day = time < 12300 || time >= 23850;
            return "night".equals(StudioText.text(one, "is", "day")) != day;
        });
        actions.condition("weather", (one, run) -> {
            World world = run.player() != null ? run.player().getWorld() : Bukkit.getWorlds().get(0);
            String now = world.isThundering() ? "thunder" : world.hasStorm() ? "rain" : "clear";
            return now.equals(StudioText.text(one, "is", "clear"));
        });
        actions.condition("group", (one, run) -> {
            String group = StudioText.text(one, "group", "").trim().toLowerCase(Locale.ROOT);
            return run.player() != null && !group.isEmpty() && run.player().hasPermission("group." + group);
        });
    }

    void stop() {
        for (Map.Entry<UUID, BossBar> one : bars.entrySet()) {
            Player player = Bukkit.getPlayer(one.getKey());
            if (player != null) {
                player.hideBossBar(one.getValue());
            }
        }
        bars.clear();
        attachments.clear();
    }

    // ---------- items, experience ----------

    private boolean matches(ItemStack stack, String wanted) {
        if (stack == null || stack.getType().isAir()) {
            return false;
        }
        if (wanted.startsWith("studio:")) {
            String id = wanted.substring(7);
            return id.startsWith("item/") ? id.substring(5).equals(StudioItems.idOf(plugin, stack)) : id.equals(StudioItems.idOf(plugin, stack));
        }
        Material material = Material.matchMaterial(wanted);
        return material != null && stack.getType() == material && StudioItems.idOf(plugin, stack) == null;
    }

    private int count(Player player, String wanted) {
        int count = 0;
        for (ItemStack stack : player.getInventory().getContents()) {
            if (matches(stack, wanted)) {
                count += stack.getAmount();
            }
        }
        return count;
    }

    private void take(Player player, String wanted, int amount) {
        int left = amount;
        ItemStack[] contents = player.getInventory().getContents();
        for (int slot = 0; slot < contents.length && left > 0; slot++) {
            ItemStack stack = contents[slot];
            if (!matches(stack, wanted)) {
                continue;
            }
            int taken = Math.min(left, stack.getAmount());
            left -= taken;
            if (taken == stack.getAmount()) {
                player.getInventory().setItem(slot, null);
            } else {
                stack.setAmount(stack.getAmount() - taken);
            }
        }
    }

    private static void xp(Player player, Map<String, Object> one) {
        int amount = StudioText.number(one, "amount", 10, 0, 1_000_000);
        boolean levels = Boolean.TRUE.equals(one.get("levels"));
        switch (StudioText.text(one, "op", "give")) {
            case "take" -> {
                if (levels) {
                    player.setLevel(Math.max(0, player.getLevel() - amount));
                } else {
                    player.giveExp(-Math.min(amount, player.calculateTotalExperiencePoints()));
                }
            }
            case "set" -> {
                if (levels) {
                    player.setLevel(amount);
                } else {
                    player.setLevel(0);
                    player.setExp(0);
                    player.giveExp(amount);
                }
            }
            default -> {
                if (levels) {
                    player.giveExpLevels(amount);
                } else {
                    player.giveExp(amount);
                }
            }
        }
    }

    static GameMode gameMode(String name) {
        try {
            return GameMode.valueOf(name.trim().toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException unknown) {
            return null;
        }
    }

    // ---------- boss bar ----------

    private void bossbar(Map<String, Object> one, StudioActions.Run run) {
        Player player = run.player();
        actions.onPlayer(player, () -> {
            BossBar was = bars.remove(player.getUniqueId());
            if (was != null) {
                player.hideBossBar(was);
            }
            if ("hide".equals(StudioText.text(one, "op", "show"))) {
                return;
            }
            BossBar.Color color;
            try {
                color = BossBar.Color.valueOf(StudioText.text(one, "color", "white").toUpperCase(Locale.ROOT));
            } catch (IllegalArgumentException unknown) {
                color = BossBar.Color.WHITE;
            }
            float progress = (float) Math.max(0, Math.min(1, decimal(one.get("progress"), 1)));
            BossBar bar = BossBar.bossBar(actions.render(StudioText.text(one, "text", ""), run), progress, color, BossBar.Overlay.PROGRESS);
            bars.put(player.getUniqueId(), bar);
            player.showBossBar(bar);
            int seconds = StudioText.number(one, "seconds", 10, 0, 24 * 60 * 60);
            if (seconds > 0) {
                player.getScheduler().runDelayed(plugin, task -> {
                    if (bars.remove(player.getUniqueId(), bar)) {
                        player.hideBossBar(bar);
                    }
                }, null, seconds * 20L);
            }
        });
    }

    // ---------- blocks and areas ----------

    /** The world a step names, else the player's, else the main one. */
    private World worldOf(Map<String, Object> one, StudioActions.Run run) {
        String name = actions.plain(StudioText.text(one, "world", ""), run).trim();
        if (!name.isEmpty()) {
            return Bukkit.getWorld(name);
        }
        return run.player() != null ? run.player().getWorld() : Bukkit.getWorlds().get(0);
    }

    /** One axis of a spot: a number, {@code ~} (the player's) or {@code ~3} (three past it); placeholders work. */
    private int axis(Map<String, Object> one, String key, int base, StudioActions.Run run) {
        String text = actions.plain(StudioText.scalar(one, key, "~"), run).trim();
        try {
            if (text.startsWith("~")) {
                String rest = text.substring(1).trim();
                return base + (rest.isEmpty() ? 0 : (int) Math.floor(Double.parseDouble(rest)));
            }
            return (int) Math.floor(Double.parseDouble(text));
        } catch (NumberFormatException bad) {
            return base;
        }
    }

    private int[] spot(Map<String, Object> one, String suffix, StudioActions.Run run) {
        Location base = run.player() != null ? run.player().getLocation() : new Location(null, 0, 64, 0);
        return new int[] {
            axis(one, "x" + suffix, base.getBlockX(), run),
            axis(one, "y" + suffix, base.getBlockY(), run),
            axis(one, "z" + suffix, base.getBlockZ(), run)
        };
    }

    /** A game block ({@code oak_stairs[facing=east]}) or one of ours ({@code studio:block/<id>}); null when unknown. */
    static BlockData blockData(String text) {
        String name = text.trim();
        if (name.startsWith("studio:")) {
            String id = name.substring(7);
            return StudioBlocks.stateFor(id.startsWith("block/") ? id.substring(6) : id);
        }
        try {
            return Bukkit.createBlockData(name.toLowerCase(Locale.ROOT).startsWith("minecraft:") || name.contains(":") ? name : "minecraft:" + name);
        } catch (IllegalArgumentException unknown) {
            return null;
        }
    }

    /** Runs {@code work} for every chunk the box touches, on that chunk's region thread, then {@code done}. */
    private void perChunk(World world, int[] low, int[] high, Consumer<int[]> work, Runnable done) {
        List<int[]> chunks = new ArrayList<>();
        for (int cx = low[0] >> 4; cx <= high[0] >> 4; cx++) {
            for (int cz = low[2] >> 4; cz <= high[2] >> 4; cz++) {
                chunks.add(new int[] {cx, cz});
            }
        }
        AtomicInteger left = new AtomicInteger(chunks.size());
        for (int[] chunk : chunks) {
            Bukkit.getRegionScheduler().execute(plugin, world, chunk[0], chunk[1], () -> {
                try {
                    work.accept(chunk);
                } catch (RuntimeException error) {
                    warn.accept("A studio area step failed in one chunk", error);
                }
                if (left.decrementAndGet() == 0 && done != null) {
                    done.run();
                }
            });
        }
    }

    private static int[][] box(int[] a, int[] b, World world) {
        int[] low = {Math.min(a[0], b[0]), Math.max(world.getMinHeight(), Math.min(a[1], b[1])), Math.min(a[2], b[2])};
        int[] high = {Math.max(a[0], b[0]), Math.min(world.getMaxHeight() - 1, Math.max(a[1], b[1])), Math.max(a[2], b[2])};
        return new int[][] {low, high};
    }

    private static long volume(int[] low, int[] high) {
        return (long) (high[0] - low[0] + 1) * Math.max(0, high[1] - low[1] + 1) * (high[2] - low[2] + 1);
    }

    private void block(Map<String, Object> one, StudioActions.Run run) {
        World world = worldOf(one, run);
        BlockData data = blockData(actions.plain(StudioText.text(one, "block", "stone"), run));
        if (world == null || data == null) {
            return;
        }
        int[] at = spot(one, "", run);
        if (at[1] < world.getMinHeight() || at[1] >= world.getMaxHeight()) {
            return;
        }
        Bukkit.getRegionScheduler().execute(plugin, world, at[0] >> 4, at[2] >> 4, () -> world.getBlockAt(at[0], at[1], at[2]).setBlockData(data, true));
    }

    private void fill(Map<String, Object> one, StudioActions.Run run) {
        World world = worldOf(one, run);
        BlockData data = blockData(actions.plain(StudioText.text(one, "block", "air"), run));
        if (world == null || data == null) {
            return;
        }
        int[][] box = box(spot(one, "1", run), spot(one, "2", run), world);
        int[] low = box[0];
        int[] high = box[1];
        if (volume(low, high) <= 0 || volume(low, high) > MOST_FILL) {
            return;
        }
        perChunk(world, low, high, chunk -> {
            for (int x = Math.max(low[0], chunk[0] << 4); x <= Math.min(high[0], (chunk[0] << 4) + 15); x++) {
                for (int z = Math.max(low[2], chunk[1] << 4); z <= Math.min(high[2], (chunk[1] << 4) + 15); z++) {
                    for (int y = low[1]; y <= high[1]; y++) {
                        world.getBlockAt(x, y, z).setBlockData(data, false);
                    }
                }
            }
        }, null);
    }

    private void region(Map<String, Object> one, StudioActions.Run run) {
        String name = actions.plain(StudioText.text(one, "name", ""), run).trim().toLowerCase(Locale.ROOT);
        World world = worldOf(one, run);
        if (!NAME.matcher(name).matches() || world == null) {
            return;
        }
        if ("paste".equals(StudioText.text(one, "op", "copy"))) {
            int[] at = spot(one, "", run);
            int quarter = StudioText.number(one, "rotate", 0, 0, 270) / 90;
            boolean air = Boolean.TRUE.equals(one.get("air"));
            Bukkit.getAsyncScheduler().runNow(plugin, task -> paste(name, world, at, quarter, air));
            return;
        }
        int[][] box = box(spot(one, "1", run), spot(one, "2", run), world);
        int[] low = box[0];
        int[] high = box[1];
        long volume = volume(low, high);
        if (volume <= 0 || volume > MOST_REGION) {
            return;
        }
        int width = high[0] - low[0] + 1;
        int height = high[1] - low[1] + 1;
        int depth = high[2] - low[2] + 1;
        String[] states = new String[(int) volume];
        perChunk(world, low, high, chunk -> {
            for (int x = Math.max(low[0], chunk[0] << 4); x <= Math.min(high[0], (chunk[0] << 4) + 15); x++) {
                for (int z = Math.max(low[2], chunk[1] << 4); z <= Math.min(high[2], (chunk[1] << 4) + 15); z++) {
                    for (int y = low[1]; y <= high[1]; y++) {
                        states[((y - low[1]) * depth + (z - low[2])) * width + (x - low[0])] = world.getBlockAt(x, y, z).getBlockData().getAsString();
                    }
                }
            }
        }, () -> Bukkit.getAsyncScheduler().runNow(plugin, task -> save(name, width, height, depth, states)));
    }

    /** {@code {"size":[w,h,d],"palette":[...],"blocks":[...]}}: blocks go x, then z, then y; numbers into the palette. */
    private void save(String name, int width, int height, int depth, String[] states) {
        Map<String, Integer> palette = new HashMap<>();
        List<String> order = new ArrayList<>();
        StringBuilder blocks = new StringBuilder("[");
        for (int at = 0; at < states.length; at++) {
            String state = states[at] == null ? "minecraft:air" : states[at];
            Integer index = palette.get(state);
            if (index == null) {
                index = order.size();
                palette.put(state, index);
                order.add(state);
            }
            if (at > 0) {
                blocks.append(',');
            }
            blocks.append(index);
        }
        blocks.append(']');
        String text = new Json().put("schema", Studio.SCHEMA).raw("size", "[" + width + "," + height + "," + depth + "]").put("palette", order).raw("blocks", blocks.toString()).end();
        try {
            Files.createDirectories(regions);
            Path temp = regions.resolve(name + ".json.tmp");
            Files.writeString(temp, text, StandardCharsets.UTF_8);
            Files.move(temp, regions.resolve(name + ".json"), StandardCopyOption.REPLACE_EXISTING);
        } catch (IOException error) {
            warn.accept("Could not save the studio area " + name, error);
        }
    }

    private void paste(String name, World world, int[] at, int quarter, boolean air) {
        Object parsed;
        try {
            Path file = regions.resolve(name + ".json");
            if (!Files.isRegularFile(file)) {
                return;
            }
            parsed = MiniJson.parse(Files.readString(file, StandardCharsets.UTF_8));
        } catch (IOException | RuntimeException error) {
            warn.accept("Could not read the studio area " + name, error);
            return;
        }
        if (!(parsed instanceof Map<?, ?> map) || !(map.get("size") instanceof List<?> size) || size.size() != 3
                || !(map.get("palette") instanceof List<?> paletteList) || !(map.get("blocks") instanceof List<?> blockList)) {
            return;
        }
        int width = (int) decimal(size.get(0), 0);
        int height = (int) decimal(size.get(1), 0);
        int depth = (int) decimal(size.get(2), 0);
        if (width <= 0 || height <= 0 || depth <= 0 || (long) width * height * depth != blockList.size() || blockList.size() > MOST_REGION) {
            return;
        }
        StructureRotation rotation = switch (quarter) {
            case 1 -> StructureRotation.CLOCKWISE_90;
            case 2 -> StructureRotation.CLOCKWISE_180;
            case 3 -> StructureRotation.COUNTERCLOCKWISE_90;
            default -> StructureRotation.NONE;
        };
        List<BlockData> palette = new ArrayList<>();
        for (Object state : paletteList) {
            BlockData data = null;
            try {
                data = Bukkit.createBlockData(String.valueOf(state));
                if (rotation != StructureRotation.NONE) {
                    data.rotate(rotation);
                }
            } catch (IllegalArgumentException | NoSuchMethodError unknown) {
                // An unknown block (another version) or no rotate: left out or unturned.
            }
            palette.add(data);
        }
        int turnedWidth = quarter % 2 == 0 ? width : depth;
        int turnedDepth = quarter % 2 == 0 ? depth : width;
        int[] low = {at[0], Math.max(world.getMinHeight(), at[1]), at[2]};
        int[] high = {at[0] + turnedWidth - 1, Math.min(world.getMaxHeight() - 1, at[1] + height - 1), at[2] + turnedDepth - 1};
        perChunk(world, low, high, chunk -> {
            for (int y = 0; y < height; y++) {
                for (int z = 0; z < depth; z++) {
                    for (int x = 0; x < width; x++) {
                        int[] turned = turn(x, z, width, depth, quarter);
                        int wx = at[0] + turned[0];
                        int wz = at[2] + turned[1];
                        int wy = at[1] + y;
                        if (wx >> 4 != chunk[0] || wz >> 4 != chunk[1] || wy < low[1] || wy > high[1]) {
                            continue;
                        }
                        int index = (int) decimal(blockList.get((y * depth + z) * width + x), -1);
                        BlockData data = index >= 0 && index < palette.size() ? palette.get(index) : null;
                        if (data == null || (!air && data.getMaterial().isAir())) {
                            continue;
                        }
                        world.getBlockAt(wx, wy, wz).setBlockData(data, false);
                    }
                }
            }
        }, null);
    }

    /** Where (x, z) inside a w × d area lands after turning it a quarter clockwise {@code quarter} times. */
    static int[] turn(int x, int z, int width, int depth, int quarter) {
        return switch (quarter) {
            case 1 -> new int[] {depth - 1 - z, x};
            case 2 -> new int[] {width - 1 - x, depth - 1 - z};
            case 3 -> new int[] {z, width - 1 - x};
            default -> new int[] {x, z};
        };
    }

    // ---------- permissions (LuckPerms when installed) ----------

    private static Object luckPerms() {
        for (Class<?> service : Bukkit.getServicesManager().getKnownServices()) {
            if ("net.luckperms.api.LuckPerms".equals(service.getName())) {
                RegisteredServiceProvider<?> registration = Bukkit.getServicesManager().getRegistration(service);
                return registration == null ? null : registration.getProvider();
            }
        }
        return null;
    }

    /** Changes a player's LuckPerms data and saves it; false when LuckPerms isn't there or something failed. */
    private boolean modify(Object luckPerms, UUID player, Consumer<Object> change) {
        try {
            Object users = luckPerms.getClass().getMethod("getUserManager").invoke(luckPerms);
            Method modify = null;
            for (Method method : users.getClass().getMethods()) {
                if (method.getName().equals("modifyUser") && method.getParameterCount() == 2) {
                    modify = method;
                }
            }
            if (modify == null) {
                return false;
            }
            modify.invoke(users, player, change);
            return true;
        } catch (ReflectiveOperationException | RuntimeException error) {
            warn.accept("LuckPerms didn't take a studio change", error);
            return false;
        }
    }

    private static Object node(Object luckPerms, String kind, String value, long seconds) throws ReflectiveOperationException {
        ClassLoader loader = luckPerms.getClass().getClassLoader();
        Class<?> type = Class.forName("group".equals(kind) ? "net.luckperms.api.node.types.InheritanceNode" : "net.luckperms.api.node.Node", true, loader);
        Object builder = type.getMethod("builder", String.class).invoke(null, value);
        if (seconds > 0) {
            Method expiry = builder.getClass().getMethod("expiry", long.class, TimeUnit.class);
            expiry.setAccessible(true);
            builder = expiry.invoke(builder, seconds, TimeUnit.SECONDS);
        }
        Method build = builder.getClass().getMethod("build");
        build.setAccessible(true);
        return build.invoke(builder);
    }

    private static void call(Object target, String name, Object argument) throws ReflectiveOperationException {
        for (Method method : target.getClass().getMethods()) {
            if (method.getName().equals(name) && method.getParameterCount() == 1 && method.getParameterTypes()[0].isInstance(argument)) {
                method.setAccessible(true);
                method.invoke(target, argument);
                return;
            }
        }
    }

    private void permission(Map<String, Object> one, StudioActions.Run run) {
        Player player = run.player();
        String node = actions.plain(StudioText.text(one, "permission", ""), run).trim();
        if (player == null || !NODE.matcher(node).matches()) {
            return;
        }
        boolean give = !"take".equals(StudioText.text(one, "op", "give"));
        long seconds = give ? StudioText.number(one, "seconds", 0, 0, Integer.MAX_VALUE) : 0;
        Object luckPerms = luckPerms();
        if (luckPerms != null) {
            modify(luckPerms, player.getUniqueId(), user -> {
                try {
                    Object data = user.getClass().getMethod("data").invoke(user);
                    call(data, give ? "add" : "remove", node(luckPerms, "permission", node, seconds));
                } catch (ReflectiveOperationException | RuntimeException error) {
                    warn.accept("LuckPerms didn't take a studio permission", error);
                }
            });
            return;
        }
        // No LuckPerms: until the player leaves.
        actions.onPlayer(player, () -> {
            PermissionAttachment attachment = attachments.compute(player.getUniqueId(), (id, was) -> was != null && was.getPermissible() == player ? was : player.addAttachment(plugin));
            if (give) {
                attachment.setPermission(node, true);
                if (seconds > 0) {
                    player.getScheduler().runDelayed(plugin, task -> attachment.unsetPermission(node), null, seconds * 20);
                }
            } else {
                attachment.unsetPermission(node);
            }
        });
    }

    private void group(Map<String, Object> one, StudioActions.Run run) {
        Player player = run.player();
        String group = actions.plain(StudioText.text(one, "group", ""), run).trim().toLowerCase(Locale.ROOT);
        Object luckPerms = luckPerms();
        if (player == null || luckPerms == null || !NODE.matcher(group).matches()) {
            return;
        }
        String op = StudioText.text(one, "op", "add");
        modify(luckPerms, player.getUniqueId(), user -> {
            try {
                Object data = user.getClass().getMethod("data").invoke(user);
                Object node = node(luckPerms, "group", group, 0);
                if ("set".equals(op)) {
                    ClassLoader loader = luckPerms.getClass().getClassLoader();
                    Class<?> nodeType = Class.forName("net.luckperms.api.node.NodeType", true, loader);
                    Object inheritance = nodeType.getField("INHERITANCE").get(null);
                    Method matches = nodeType.getMethod("matches", Class.forName("net.luckperms.api.node.Node", true, loader));
                    Predicate<Object> isGroup = each -> {
                        try {
                            return Boolean.TRUE.equals(matches.invoke(inheritance, each));
                        } catch (ReflectiveOperationException error) {
                            return false;
                        }
                    };
                    call(data, "clear", isGroup);
                    call(data, "add", node);
                    user.getClass().getMethod("setPrimaryGroup", String.class).invoke(user, group);
                } else {
                    call(data, "remove".equals(op) ? "remove" : "add", node);
                }
            } catch (ReflectiveOperationException | RuntimeException error) {
                warn.accept("LuckPerms didn't take a studio group change", error);
            }
        });
    }

    // ---------- worlds ----------

    private void world(Map<String, Object> one, StudioActions.Run run) {
        String op = StudioText.text(one, "op", "time");
        String name = actions.plain(StudioText.text(one, "world", ""), run).trim();
        String value = actions.plain(StudioText.scalar(one, "value", ""), run).trim().toLowerCase(Locale.ROOT);
        if (!name.isEmpty() && !WORLD.matcher(name).matches()) {
            return;
        }
        if ("load".equals(op)) {
            if (name.isEmpty() || Bukkit.getWorld(name) != null) {
                return;
            }
            World.Environment environment = switch (value) {
                case "nether" -> World.Environment.NETHER;
                case "end" -> World.Environment.THE_END;
                default -> World.Environment.NORMAL;
            };
            Bukkit.getGlobalRegionScheduler().execute(plugin, () -> {
                try {
                    new WorldCreator(name).environment(environment).createWorld();
                } catch (UnsupportedOperationException | IllegalStateException folia) {
                    warn.accept("This server can't load worlds while it runs (Folia): " + name, folia);
                }
            });
            return;
        }
        World world = worldOf(one, run);
        if (world == null) {
            return;
        }
        switch (op) {
            case "spawn" -> {
                Player player = run.player();
                actions.onPlayer(player, () -> player.teleportAsync(world.getSpawnLocation()));
            }
            case "unload" -> {
                World main = Bukkit.getWorlds().get(0);
                if (world.equals(main)) {
                    return;
                }
                for (Player player : world.getPlayers()) {
                    actions.onPlayer(player, () -> player.teleportAsync(main.getSpawnLocation()));
                }
                Bukkit.getGlobalRegionScheduler().runDelayed(plugin, task -> {
                    try {
                        Bukkit.unloadWorld(world, true);
                    } catch (UnsupportedOperationException folia) {
                        warn.accept("This server can't unload worlds while it runs (Folia): " + world.getName(), folia);
                    }
                }, 40L);
            }
            default -> Bukkit.getGlobalRegionScheduler().execute(plugin, () -> setWorld(world, op, value, StudioText.text(one, "rule", "")));
        }
    }

    @SuppressWarnings("unchecked")
    private void setWorld(World world, String op, String value, String ruleName) {
        switch (op) {
            case "time" -> world.setTime(switch (value) {
                case "noon" -> 6000;
                case "night" -> 13000;
                case "midnight" -> 18000;
                default -> 1000;
            });
            case "weather" -> {
                world.setStorm(!"clear".equals(value));
                world.setThundering("thunder".equals(value));
                if ("clear".equals(value)) {
                    world.setClearWeatherDuration(20 * 60 * 10);
                }
            }
            case "difficulty" -> {
                try {
                    world.setDifficulty(Difficulty.valueOf(value.toUpperCase(Locale.ROOT)));
                } catch (IllegalArgumentException unknown) {
                    // Not a difficulty.
                }
            }
            case "border" -> {
                double size = decimal(value, 0);
                if (size >= 1 && size <= 59_999_968) {
                    world.getWorldBorder().setSize(size);
                }
            }
            case "rule" -> {
                GameRule<?> rule = StudioWorlds.rule(ruleName.trim());
                if (rule == null) {
                    return;
                }
                if (rule.getType() == Boolean.class) {
                    world.setGameRule((GameRule<Boolean>) rule, "true".equals(value));
                } else if (rule.getType() == Integer.class) {
                    try {
                        world.setGameRule((GameRule<Integer>) rule, Integer.parseInt(value));
                    } catch (NumberFormatException bad) {
                        // Not a number.
                    }
                }
            }
            default -> {
                // Unknown: skipped.
            }
        }
    }

    // ---------- helpers ----------

    private static double decimal(Object value, double fallback) {
        if (value instanceof Number number) {
            return number.doubleValue();
        }
        try {
            return value == null ? fallback : Double.parseDouble(String.valueOf(value).trim());
        } catch (NumberFormatException bad) {
            return fallback;
        }
    }

    private static boolean compare(double left, String op, double right) {
        int order = Double.compare(left, right);
        return switch (op) {
            case "!=" -> order != 0;
            case ">" -> order > 0;
            case "<" -> order < 0;
            case "<=" -> order <= 0;
            case "=" -> order == 0;
            default -> order >= 0;
        };
    }
}

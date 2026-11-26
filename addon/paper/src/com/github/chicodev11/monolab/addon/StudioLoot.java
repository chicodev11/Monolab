package com.github.chicodev11.monolab.addon;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Random;
import org.bukkit.Location;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.block.BlockDropItemEvent;
import org.bukkit.event.entity.EntityDeathEvent;
import org.bukkit.inventory.ItemStack;

/**
 * Studio loot tables: a weighted list of items rolled a few times. A table can add to (or replace) what a block
 * or mob drops, and the {@code loot} action gives a player one roll — a crate, a reward, a daily gift.
 */
final class StudioLoot implements Listener {
    static final String[] SOURCES = {"none", "block", "mob", "chest", "fishing"};

    private final StudioActions actions;
    private final Studio studio;
    private final Random random = new Random();
    private volatile Map<String, Studio.Resource> tables = Map.of();
    /** Tables that drop from blocks / mobs, by the block or mob name they drop from ({@code *} = any). */
    private volatile Map<String, List<Studio.Resource>> fromBlocks = Map.of();
    private volatile Map<String, List<Studio.Resource>> fromMobs = Map.of();
    /** By vanilla loot table path ({@code chests/simple_dungeon}). */
    private volatile Map<String, List<Studio.Resource>> fromChests = Map.of();
    private volatile List<Studio.Resource> fromFishing = List.of();
    private static volatile StudioLoot current;

    StudioLoot(StudioActions actions, Studio studio) {
        this.actions = actions;
        this.studio = studio;
    }

    /** The running feature, for mobs' drops. Null before the add-on starts. */
    static StudioLoot get() {
        return current;
    }

    void register() {
        current = this;
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "loot";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, Studio.Resource> all = new HashMap<>();
                Map<String, List<Studio.Resource>> blocks = new HashMap<>();
                Map<String, List<Studio.Resource>> mobs = new HashMap<>();
                Map<String, List<Studio.Resource>> chests = new HashMap<>();
                List<Studio.Resource> fishing = new ArrayList<>();
                for (Studio.Resource one : resources) {
                    String problem = problem(one.data());
                    if (problem != null) {
                        studio.problem("loot", one.id(), problem);
                        continue;
                    }
                    all.put(one.id(), one);
                    Map<String, Object> drops = StudioText.object(one.data(), "drops");
                    String from = StudioText.text(drops, "from", "none");
                    if ("fishing".equals(from)) {
                        fishing.add(one);
                        continue;
                    }
                    Map<String, List<Studio.Resource>> into = "block".equals(from) ? blocks : "mob".equals(from) ? mobs : "chest".equals(from) ? chests : null;
                    if (into == null) {
                        continue;
                    }
                    List<String> targets = StudioText.lines(drops, "targets");
                    targets.removeIf(String::isBlank);
                    if (targets.isEmpty()) {
                        targets = List.of("*");
                    }
                    for (String target : targets) {
                        into.computeIfAbsent(clean(target), key -> new ArrayList<>()).add(one);
                    }
                }
                tables = Map.copyOf(all);
                fromBlocks = Map.copyOf(blocks);
                fromMobs = Map.copyOf(mobs);
                fromChests = Map.copyOf(chests);
                fromFishing = List.copyOf(fishing);
            }
        });
        actions.action("loot", (step, run) -> {
            Player player = run.player();
            Studio.Resource table = tables.get(StudioText.text(step, "loot", ""));
            if (player == null || table == null) {
                return;
            }
            List<ItemStack> rolled = roll(table.data());
            actions.onPlayer(player, () -> {
                for (ItemStack stack : rolled) {
                    for (ItemStack left : player.getInventory().addItem(stack).values()) {
                        player.getWorld().dropItemNaturally(player.getLocation(), left);
                    }
                }
            });
        });
    }

    /** {@code minecraft:diamond_ore} and {@code DIAMOND_ORE} both become {@code diamond_ore}. */
    static String clean(String name) {
        String lower = name.trim().toLowerCase(Locale.ROOT);
        return lower.startsWith("minecraft:") ? lower.substring(10) : lower;
    }

    private static String problem(Map<String, Object> data) {
        if (!(data.get("entries") instanceof List<?> entries) || entries.isEmpty()) {
            return "Add at least one entry to \"entries\".";
        }
        return null;
    }

    /** Rolls the table: {@code rolls} times (or a random count up to {@code rollsMax}) one entry by weight. */
    List<ItemStack> roll(Map<String, Object> data) {
        int least = StudioText.number(data, "rolls", 1, 0, 64);
        int most = Math.max(least, StudioText.number(data, "rollsMax", least, 0, 64));
        int times = least + (most > least ? random.nextInt(most - least + 1) : 0);
        List<Map<String, Object>> entries = new ArrayList<>();
        int total = 0;
        if (data.get("entries") instanceof List<?> list) {
            for (Object raw : list) {
                if (raw instanceof Map<?, ?> one) {
                    @SuppressWarnings("unchecked")
                    Map<String, Object> entry = (Map<String, Object>) one;
                    int weight = StudioText.number(entry, "weight", 1, 0, 1_000_000);
                    if (weight > 0) {
                        entries.add(entry);
                        total += weight;
                    }
                }
            }
        }
        List<ItemStack> out = new ArrayList<>();
        for (int at = 0; at < times && total > 0; at++) {
            int pick = random.nextInt(total);
            for (Map<String, Object> entry : entries) {
                pick -= StudioText.number(entry, "weight", 1, 0, 1_000_000);
                if (pick < 0) {
                    String item = StudioText.text(entry, "item", "").trim();
                    if (!item.isEmpty()) {
                        int low = StudioText.number(entry, "min", 1, 1, 64 * 36);
                        int high = Math.max(low, StudioText.number(entry, "max", low, 1, 64 * 36));
                        ItemStack stack = actions.stack(item, low + (high > low ? random.nextInt(high - low + 1) : 0));
                        if (stack != null) {
                            out.add(stack);
                        }
                    }
                    break;
                }
            }
        }
        return out;
    }

    /** One roll of a table by id (a studio mob's {@code loot}); empty when there is no such table. Any thread. */
    List<ItemStack> roll(String id) {
        Studio.Resource table = tables.get(id);
        return table == null ? List.of() : roll(table.data());
    }

    private List<Studio.Resource> matching(Map<String, List<Studio.Resource>> by, String name) {
        List<Studio.Resource> out = new ArrayList<>(by.getOrDefault(name, List.of()));
        out.addAll(by.getOrDefault("*", List.of()));
        return out;
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onBlockDrop(BlockDropItemEvent event) {
        List<Studio.Resource> found = matching(fromBlocks, clean(event.getBlockState().getType().getKey().getKey()));
        if (found.isEmpty()) {
            return;
        }
        Player player = event.getPlayer();
        Location at = event.getBlock().getLocation().add(0.5, 0.5, 0.5);
        for (Studio.Resource one : found) {
            if (!StudioText.shows(one.data(), player)) {
                continue;
            }
            if (Boolean.TRUE.equals(StudioText.object(one.data(), "drops").get("replace"))) {
                event.getItems().clear();
            }
            for (ItemStack stack : roll(one.data())) {
                at.getWorld().dropItemNaturally(at, stack);
            }
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onMobDeath(EntityDeathEvent event) {
        if (event.getEntity() instanceof Player) {
            return;
        }
        if (StudioMobs.ownDropsOnly(event.getEntity())) {
            return; // A studio mob with vanillaDrops off drops only what its file says.
        }
        List<Studio.Resource> found = matching(fromMobs, clean(event.getEntityType().getKey().getKey()));
        if (found.isEmpty()) {
            return;
        }
        Player killer = event.getEntity().getKiller();
        for (Studio.Resource one : found) {
            if (Boolean.TRUE.equals(StudioText.object(one.data(), "drops").get("needsPlayer")) && killer == null) {
                continue;
            }
            if (killer != null ? !StudioText.shows(one.data(), killer) : !worldOnly(one, event.getEntity().getWorld().getName())) {
                continue;
            }
            if (Boolean.TRUE.equals(StudioText.object(one.data(), "drops").get("replace"))) {
                event.getDrops().clear();
            }
            event.getDrops().addAll(roll(one.data()));
        }
    }

    /** A container filling from a vanilla loot table: ours add to (or replace) what it rolled. */
    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onChestLoot(org.bukkit.event.world.LootGenerateEvent event) {
        List<Studio.Resource> found = matching(fromChests, clean(event.getLootTable().getKey().getKey()));
        if (found.isEmpty()) {
            return;
        }
        List<ItemStack> loot = new ArrayList<>(event.getLoot());
        Player player = event.getEntity() instanceof Player one ? one : null;
        for (Studio.Resource one : found) {
            if (player != null && !StudioText.shows(one.data(), player)) {
                continue;
            }
            if (Boolean.TRUE.equals(StudioText.object(one.data(), "drops").get("replace"))) {
                loot.clear();
            }
            loot.addAll(roll(one.data()));
        }
        event.setLoot(loot);
    }

    /** A catch: ours go into the player's inventory; one that replaces the catch takes the caught item away. */
    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onFish(org.bukkit.event.player.PlayerFishEvent event) {
        if (event.getState() != org.bukkit.event.player.PlayerFishEvent.State.CAUGHT_FISH || fromFishing.isEmpty()) {
            return;
        }
        Player player = event.getPlayer();
        boolean replace = false;
        for (Studio.Resource one : fromFishing) {
            if (!StudioText.shows(one.data(), player)) {
                continue;
            }
            replace |= Boolean.TRUE.equals(StudioText.object(one.data(), "drops").get("replace"));
            for (ItemStack stack : roll(one.data())) {
                for (ItemStack left : player.getInventory().addItem(stack).values()) {
                    player.getWorld().dropItemNaturally(player.getLocation(), left);
                }
            }
        }
        if (replace && event.getCaught() != null) {
            event.getCaught().remove();
        }
    }

    /** With no player to ask, only the world part of {@code show} can be checked; a permission means "players only". */
    private static boolean worldOnly(Studio.Resource one, String world) {
        Map<String, Object> show = StudioText.object(one.data(), "show");
        if (!StudioText.text(show, "permission", "").isBlank()) {
            return false;
        }
        List<String> worlds = StudioText.lines(show, "worlds");
        worlds.removeIf(String::isBlank);
        return worlds.isEmpty() || worlds.contains(world);
    }
}

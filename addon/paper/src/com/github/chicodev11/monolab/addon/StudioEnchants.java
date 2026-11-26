package com.github.chicodev11.monolab.addon;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ThreadLocalRandom;
import net.kyori.adventure.text.Component;
import org.bukkit.NamespacedKey;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.entity.EntityDamageByEntityEvent;
import org.bukkit.event.entity.EntityDamageEvent;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.Plugin;

/**
 * Studio custom enchantments ({@code enchant}). Paper can't add enchantments to the game's registry while it runs, so
 * ours live on the item: {@code monolab:enchants} holds {@code id=level;...} and the lore starts with one line each
 * (how many in {@code monolab:enchant_lines}). Their effects are action lists run on hits, block breaks and hurts.
 * Same rules as {@code crates/studio-format/src/enchant.rs}.
 */
final class StudioEnchants implements Listener {
    static final String[] ITEM_GROUPS = {"sword", "axe", "pickaxe", "shovel", "hoe", "bow", "crossbow", "trident", "mace", "fishing_rod", "helmet", "chestplate", "leggings", "boots", "elytra"};
    static final String[] WIDE_GROUPS = {"weapon", "tool", "armor", "any"};
    static final String PREFIX = "studio:";
    private static final int MOST_LEVEL = 255;
    private static volatile StudioEnchants current;

    private final Plugin plugin;
    private final StudioActions actions;
    private final Studio studio;
    private final NamespacedKey enchantsKey;
    private final NamespacedKey linesKey;
    private volatile Map<String, Map<String, Object>> enchants = Map.of();

    StudioEnchants(Plugin plugin, StudioActions actions, Studio studio) {
        this.plugin = plugin;
        this.actions = actions;
        this.studio = studio;
        this.enchantsKey = new NamespacedKey(plugin, "enchants");
        this.linesKey = new NamespacedKey(plugin, "enchant_lines");
    }

    /** The running feature (items and the {@code enchant} action use it). Null before the add-on starts. */
    static StudioEnchants get() {
        return current;
    }

    void register() {
        current = this;
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "enchant";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, Map<String, Object>> next = new HashMap<>();
                for (Studio.Resource one : resources) {
                    String problem = problem(one.data());
                    if (problem != null) {
                        studio.problem("enchant", one.id(), problem);
                        if (problem.startsWith("\"items\"")) {
                            continue;
                        }
                    }
                    next.put(one.id(), one.data());
                }
                enchants = Map.copyOf(next);
            }
        });
    }

    boolean has(String id) {
        return enchants.containsKey(id);
    }

    // ---------- the shared rules ----------

    static String problem(Map<String, Object> data) {
        for (String group : StudioText.lines(data, "items")) {
            String clean = group.trim().toLowerCase(Locale.ROOT);
            if (!clean.isEmpty() && !List.of(ITEM_GROUPS).contains(clean) && !List.of(WIDE_GROUPS).contains(clean)) {
                return "\"items\" takes: " + String.join(", ", ITEM_GROUPS) + ", " + String.join(", ", WIDE_GROUPS) + ".";
            }
        }
        for (String list : new String[] {"hitActions", "mineActions", "hurtActions"}) {
            String wrong = StudioActions.problem(data.get(list), (kind, id) -> true);
            if (wrong != null) {
                return wrong;
            }
        }
        boolean any = false;
        for (String list : new String[] {"hitActions", "mineActions", "hurtActions"}) {
            any |= data.get(list) instanceof List<?> found && !found.isEmpty();
        }
        return any ? null : "Add actions for when it hits, mines or is hurt, or it does nothing.";
    }

    static int maxLevel(Map<String, Object> data) {
        return StudioText.number(data, "maxLevel", 1, 1, MOST_LEVEL);
    }

    static boolean fits(Map<String, Object> data, String material) {
        List<String> groups = new ArrayList<>();
        for (String one : StudioText.lines(data, "items")) {
            if (!one.isBlank()) {
                groups.add(one.trim().toLowerCase(Locale.ROOT));
            }
        }
        if (groups.isEmpty()) {
            return true;
        }
        String bare = material.toLowerCase(Locale.ROOT).replaceFirst("^minecraft:", "");
        for (String group : groups) {
            if (inGroup(group, bare)) {
                return true;
            }
        }
        return false;
    }

    private static boolean inGroup(String group, String bare) {
        return switch (group) {
            case "any" -> true;
            case "weapon" -> List.of("sword", "axe", "bow", "crossbow", "trident", "mace").stream().anyMatch(one -> inGroup(one, bare));
            case "tool" -> List.of("pickaxe", "axe", "shovel", "hoe", "fishing_rod", "shears").stream().anyMatch(one -> inGroup(one, bare));
            case "armor" -> List.of("helmet", "chestplate", "leggings", "boots").stream().anyMatch(one -> inGroup(one, bare)) || bare.equals("turtle_helmet");
            case "axe" -> bare.endsWith("_axe");
            case "bow" -> bare.equals("bow");
            case "shears" -> bare.equals("shears");
            default -> bare.equals(group) || bare.endsWith("_" + group);
        };
    }

    static String numeral(int level) {
        String[] roman = {"I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"};
        return level >= 1 && level <= 10 ? roman[level - 1] : String.valueOf(level);
    }

    static String loreLine(Map<String, Object> data, String id, int level) {
        String name = StudioText.text(data, "name", id);
        String shown = maxLevel(data) == 1 && level == 1 ? name : name + " " + numeral(level);
        return name.stripLeading().startsWith("<") ? shown : "<gray>" + shown;
    }

    static List<Map.Entry<String, Integer>> parse(String text) {
        List<Map.Entry<String, Integer>> out = new ArrayList<>();
        if (text == null) {
            return out;
        }
        for (String one : text.split(";")) {
            int at = one.indexOf('=');
            if (at <= 0) {
                continue;
            }
            String id = one.substring(0, at).trim();
            try {
                int level = Math.max(1, Math.min(MOST_LEVEL, Integer.parseInt(one.substring(at + 1).trim())));
                if (id.matches("[a-z0-9_-]{1,48}")) {
                    out.add(Map.entry(id, level));
                }
            } catch (NumberFormatException skipped) {
                // Not ours.
            }
        }
        return out;
    }

    // ---------- items ----------

    /** The studio enchantments on a stack, in order. */
    List<Map.Entry<String, Integer>> on(ItemStack stack) {
        if (stack == null || !stack.hasItemMeta()) {
            return List.of();
        }
        return parse(stack.getItemMeta().getPersistentDataContainer().get(enchantsKey, PersistentDataType.STRING));
    }

    /** Sets one enchantment on a stack (0 takes it off), with its lore line. False when it doesn't fit the item. */
    boolean set(ItemStack stack, String id, int level, Player viewer) {
        Map<String, Object> data = enchants.get(id);
        if (stack == null || stack.getType().isAir() || data == null || !fits(data, stack.getType().getKey().toString())) {
            return false;
        }
        List<Map.Entry<String, Integer>> list = new ArrayList<>(on(stack));
        int at = -1;
        for (int index = 0; index < list.size(); index++) {
            if (list.get(index).getKey().equals(id)) {
                at = index;
            }
        }
        int capped = Math.min(level, maxLevel(data));
        if (at >= 0) {
            list.remove(at);
        }
        if (capped > 0) {
            list.add(at >= 0 ? at : list.size(), Map.entry(id, capped));
        }
        write(stack, list, viewer);
        return true;
    }

    /** Writes the list to the stack and puts its lore lines first (in place of the old ones). */
    void write(ItemStack stack, List<Map.Entry<String, Integer>> list, Player viewer) {
        ItemMeta meta = stack.getItemMeta();
        if (meta == null) {
            return;
        }
        Integer had = meta.getPersistentDataContainer().get(linesKey, PersistentDataType.INTEGER);
        List<Component> lore = meta.lore() == null ? new ArrayList<>() : new ArrayList<>(meta.lore());
        for (int left = had == null ? 0 : had; left > 0 && !lore.isEmpty(); left--) {
            lore.remove(0);
        }
        List<Component> lines = new ArrayList<>();
        StringBuilder text = new StringBuilder();
        for (Map.Entry<String, Integer> one : list) {
            Map<String, Object> data = enchants.get(one.getKey());
            text.append(text.isEmpty() ? "" : ";").append(one.getKey()).append('=').append(one.getValue());
            if (data != null) {
                lines.add(StudioBlocks.plain(StudioText.render(loreLine(data, one.getKey(), one.getValue()), viewer)));
            }
        }
        lore.addAll(0, lines);
        meta.lore(lore.isEmpty() ? null : lore);
        if (list.isEmpty()) {
            meta.getPersistentDataContainer().remove(enchantsKey);
            meta.getPersistentDataContainer().remove(linesKey);
        } else {
            meta.getPersistentDataContainer().set(enchantsKey, PersistentDataType.STRING, text.toString());
            meta.getPersistentDataContainer().set(linesKey, PersistentDataType.INTEGER, lines.size());
        }
        stack.setItemMeta(meta);
    }

    /** A studio item's own {@code studio:<id>} enchantments, put on a stack just built from it. */
    void fromItem(ItemStack stack, Map<String, Object> item, Player viewer) {
        if (!(item.get("enchantments") instanceof Map<?, ?> map)) {
            return;
        }
        List<Map.Entry<String, Integer>> list = new ArrayList<>();
        for (Map.Entry<?, ?> one : map.entrySet()) {
            String key = String.valueOf(one.getKey()).trim().toLowerCase(Locale.ROOT);
            if (key.startsWith(PREFIX) && enchants.containsKey(key.substring(PREFIX.length())) && one.getValue() instanceof Double level) {
                list.add(Map.entry(key.substring(PREFIX.length()), Math.max(1, Math.min(MOST_LEVEL, level.intValue()))));
            }
        }
        if (!list.isEmpty()) {
            write(stack, list, viewer);
        }
    }

    // ---------- effects ----------

    private void fire(ItemStack stack, String key, Player player, Entity target, Map<String, String> extra) {
        if (enchants.isEmpty()) {
            return;
        }
        for (Map.Entry<String, Integer> one : on(stack)) {
            Map<String, Object> data = enchants.get(one.getKey());
            if (data == null || !(data.get(key) instanceof List<?> list) || list.isEmpty()) {
                continue;
            }
            if (ThreadLocalRandom.current().nextInt(100) >= StudioText.number(data, "chance", 100, 0, 100)) {
                continue;
            }
            Map<String, String> more = new HashMap<>(extra);
            more.put("enchant", one.getKey());
            more.put("level", String.valueOf(one.getValue()));
            actions.run(list, new StudioActions.Run(player, Map.copyOf(more), null, target));
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onHit(EntityDamageByEntityEvent event) {
        if (event.getDamager() instanceof Player player) {
            String target = event.getEntity() instanceof Player hit ? hit.getName() : event.getEntity().getType().getKey().getKey();
            fire(player.getInventory().getItemInMainHand(), "hitActions", player, event.getEntity(), Map.of("target", target));
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onBreak(BlockBreakEvent event) {
        fire(event.getPlayer().getInventory().getItemInMainHand(), "mineActions", event.getPlayer(), null, Map.of("block", event.getBlock().getType().getKey().getKey()));
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onHurt(EntityDamageEvent event) {
        if (!(event.getEntity() instanceof Player player)) {
            return;
        }
        String by = event instanceof EntityDamageByEntityEvent hit ? (hit.getDamager() instanceof Player who ? who.getName() : hit.getDamager().getType().getKey().getKey()) : "";
        for (ItemStack piece : player.getInventory().getArmorContents()) {
            fire(piece, "hurtActions", player, null, Map.of("target", by));
        }
    }

    void stop() {
        current = null;
        enchants = Map.of();
    }
}

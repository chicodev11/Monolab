package com.github.chicodev11.monolab.addon;

import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.TextDecoration;
import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.block.Action;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.entity.EntityDamageByEntityEvent;
import org.bukkit.event.player.PlayerInteractEvent;
import org.bukkit.event.player.PlayerItemConsumeEvent;
import org.bukkit.inventory.BlastingRecipe;
import org.bukkit.inventory.CampfireRecipe;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.FurnaceRecipe;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.Recipe;
import org.bukkit.inventory.RecipeChoice;
import org.bukkit.inventory.ShapedRecipe;
import org.bukkit.inventory.ShapelessRecipe;
import org.bukkit.inventory.SmokingRecipe;
import org.bukkit.inventory.StonecuttingRecipe;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.Plugin;
import org.bukkit.potion.PotionEffect;
import org.bukkit.potion.PotionEffectType;

/**
 * Studio S4: custom items ({@code item}) and crafting recipes ({@code recipe}). Items carry a
 * {@code monolab:item} tag with their id, so rules and recipes can tell them apart from plain ones,
 * and may run actions when a player uses them (right click), eats them, hits with them or breaks a block
 * with them. M1: their look and components ({@link StudioItemParts}).
 */
final class StudioItems implements Listener {
    /** What starts each action list: right click, eating it, hitting with it, breaking a block with it. */
    static final String[] ACTION_LISTS = {"useActions", "eatActions", "hitActions", "breakActions"};
    static final String[] RECIPE_TYPES = {"shaped", "shapeless", "furnace", "blasting", "smoking", "campfire", "stonecutting"};

    private final Plugin plugin;
    private final StudioActions actions;
    private final Studio studio;
    private volatile Map<String, Studio.Resource> items = Map.of();
    private final Set<NamespacedKey> added = new HashSet<>();
    private final StudioItemParts parts;
    /** Armor sets (`equip.set`): set name → its pieces and the effects a full set gives. */
    private volatile Map<String, ArmorSet> sets = Map.of();
    /** Set effects each player has from us, to take away when the set comes off. */
    private final Map<java.util.UUID, Set<PotionEffectType>> setGiven = new java.util.concurrent.ConcurrentHashMap<>();
    private io.papermc.paper.threadedregions.scheduler.ScheduledTask setTask;
    /** Seconds a set effect lasts each time (given again every second); the same as {@code item::SET_EFFECT_SECONDS}. */
    private static final int SET_EFFECT_SECONDS = 15;

    /** The same as {@code item::ArmorSet}: effects are (type, level 1..). */
    private record ArmorSet(Set<String> pieces, Map<PotionEffectType, Integer> effects) {}

    StudioItems(Plugin plugin, StudioActions actions, Studio studio) {
        this.plugin = plugin;
        this.actions = actions;
        this.studio = studio;
        this.parts = new StudioItemParts(plugin);
    }

    static NamespacedKey tag(Plugin plugin) {
        return new NamespacedKey(plugin, "item");
    }

    /** The studio id an item stack was made from, or null. */
    static String idOf(Plugin plugin, ItemStack stack) {
        if (stack == null || !stack.hasItemMeta()) {
            return null;
        }
        return stack.getItemMeta().getPersistentDataContainer().get(tag(plugin), PersistentDataType.STRING);
    }

    void register() {
        studio.add(feature("item", list -> {
            Map<String, Studio.Resource> next = new HashMap<>();
            for (Studio.Resource one : list) {
                String problem = itemProblem(one.data());
                if (problem != null) {
                    studio.problem("item", one.id(), problem);
                    continue;
                }
                next.put(one.id(), one);
                // Built once here so a server too old for some parts says which.
                List<String> skipped = parts.apply(one.id(), one.data(), build(one.id(), one.data(), null, false));
                if (!skipped.isEmpty()) {
                    studio.problem("item", one.id(), "This server can't do: " + String.join(", ", skipped) + ". The rest works.");
                }
            }
            items = next;
            sets = armorSets(next);
        }));
        setTask = Bukkit.getGlobalRegionScheduler().runAtFixedRate(plugin, task -> {
            if (sets.isEmpty() && setGiven.isEmpty()) {
                return;
            }
            for (Player player : Bukkit.getOnlinePlayers()) {
                player.getScheduler().run(plugin, one -> wearSets(player), null);
            }
        }, 20L, 20L);
        studio.add(feature("recipe", this::applyRecipes));
    }

    private static Studio.Feature feature(String kind, java.util.function.Consumer<List<Studio.Resource>> apply) {
        return new Studio.Feature() {
            @Override
            public String kind() {
                return kind;
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                apply.accept(resources);
            }
        };
    }

    private static Map<String, ArmorSet> armorSets(Map<String, Studio.Resource> items) {
        Map<String, ArmorSet> out = new HashMap<>();
        for (Studio.Resource one : items.values()) {
            if (!(one.data().get("equip") instanceof Map<?, ?> raw)) {
                continue;
            }
            Map<String, Object> equip = StudioActions.map(raw);
            String name = StudioText.text(equip, "set", "").trim();
            if (name.isEmpty()) {
                continue;
            }
            ArmorSet set = out.computeIfAbsent(name, key -> new ArmorSet(new HashSet<>(), new HashMap<>()));
            set.pieces().add(one.id());
            if (equip.get("setEffects") instanceof List<?> list) {
                for (Object entry : list) {
                    if (!(entry instanceof Map<?, ?> rawEffect)) {
                        continue;
                    }
                    Map<String, Object> effect = StudioActions.map(rawEffect);
                    PotionEffectType type = StudioActions.effect(StudioText.text(effect, "effect", ""));
                    if (type != null) {
                        set.effects().merge(type, StudioText.number(effect, "level", 1, 1, 255), Math::max);
                    }
                }
            }
        }
        return Map.copyOf(out);
    }

    /** Once a second for each player: full sets give their effects; ones we gave that no set gives now go. */
    private void wearSets(Player player) {
        List<String> worn = new ArrayList<>();
        for (ItemStack piece : player.getInventory().getArmorContents()) {
            String id = idOf(plugin, piece);
            if (id != null) {
                worn.add(id);
            }
        }
        Map<PotionEffectType, Integer> wanted = new HashMap<>();
        for (ArmorSet set : sets.values()) {
            if (!set.pieces().isEmpty() && worn.containsAll(set.pieces())) {
                set.effects().forEach((type, level) -> wanted.merge(type, level, Math::max));
            }
        }
        Set<PotionEffectType> had = setGiven.getOrDefault(player.getUniqueId(), Set.of());
        for (PotionEffectType type : had) {
            PotionEffect now = player.getPotionEffect(type);
            if (!wanted.containsKey(type) && now != null && now.isAmbient() && now.getDuration() <= SET_EFFECT_SECONDS * 20) {
                player.removePotionEffect(type);
            }
        }
        wanted.forEach((type, level) -> player.addPotionEffect(new PotionEffect(type, SET_EFFECT_SECONDS * 20, level - 1, true, false, true)));
        if (wanted.isEmpty()) {
            setGiven.remove(player.getUniqueId());
        } else {
            setGiven.put(player.getUniqueId(), Set.copyOf(wanted.keySet()));
        }
    }

    /** The file of the studio item a stack was made from, or null. */
    Map<String, Object> dataOf(ItemStack stack) {
        String id = idOf(plugin, stack);
        Studio.Resource one = id == null ? null : items.get(id);
        return one == null ? null : one.data();
    }

    boolean has(String id) {
        return items.containsKey(id);
    }

    // ---------- items ----------

    private static String itemProblem(Map<String, Object> data) {
        Material material = Material.matchMaterial(StudioText.text(data, "material", ""));
        if (material == null || !material.isItem() || material.isAir()) {
            return "\"material\" must be an item, like diamond_sword.";
        }
        String wrong = StudioItemParts.problem(data);
        if (wrong != null) {
            return wrong;
        }
        for (String list : ACTION_LISTS) {
            String problem = StudioActions.problem(data.get(list), (kind, id) -> true);
            if (problem != null) {
                return problem;
            }
        }
        return null;
    }

    /** A fresh stack of a studio item, or null when there is none by that id. */
    ItemStack stack(String id) {
        Studio.Resource one = items.get(id);
        return one == null ? null : build(id, one.data(), null);
    }

    /** Builds an item from its fields. Text is rendered for {@code player} (may be null). */
    ItemStack build(String id, Map<String, Object> data, Player player) {
        return build(id, data, player, true);
    }

    /** {@code withParts} false leaves the M1 components off (the caller sets them and wants to know what failed). */
    private ItemStack build(String id, Map<String, Object> data, Player player, boolean withParts) {
        Material material = Material.matchMaterial(StudioText.text(data, "material", "stone"));
        if (material == null || !material.isItem() || material.isAir()) {
            material = Material.STONE;
        }
        ItemStack stack = new ItemStack(material, 1);
        ItemMeta meta = stack.getItemMeta();
        if (meta == null) {
            return stack;
        }
        if (StudioText.has(data, "name")) {
            meta.displayName(plainStyle(StudioText.render(StudioText.text(data, "name", ""), player)));
        }
        List<String> lore = StudioText.lines(data, "lore");
        if (!lore.isEmpty()) {
            List<Component> lines = new ArrayList<>();
            for (String line : lore) {
                lines.add(plainStyle(StudioText.render(line, player)));
            }
            meta.lore(lines);
        }
        int oldModel = id == null || !StudioItemParts.hasLook(data) ? -1 : parts.oldModel(id);
        if (oldModel >= 0) {
            // Before 1.21.4 the look is a custom model data override in the app's pack.
            setModel(meta, oldModel);
        } else if (data.get("model") instanceof Double model) {
            setModel(meta, model.intValue());
        }
        if (data.get("enchantments") instanceof Map<?, ?> enchantments) {
            for (Map.Entry<?, ?> one : enchantments.entrySet()) {
                Enchantment enchantment = enchantment(String.valueOf(one.getKey()));
                if (enchantment != null && one.getValue() instanceof Double level) {
                    meta.addEnchant(enchantment, Math.max(1, Math.min(255, level.intValue())), true);
                }
            }
        }
        if (Boolean.TRUE.equals(data.get("unbreakable"))) {
            meta.setUnbreakable(true);
        }
        if (Boolean.TRUE.equals(data.get("glow"))) {
            meta.setEnchantmentGlintOverride(true);
        }
        if (id != null) {
            meta.getPersistentDataContainer().set(tag(plugin), PersistentDataType.STRING, id);
        }
        stack.setItemMeta(meta);
        if (withParts) {
            parts.apply(id, data, stack);
        }
        StudioEnchants enchants = StudioEnchants.get();
        if (enchants != null) {
            enchants.fromItem(stack, data, player);
        }
        stack.setAmount(StudioText.number(data, "amount", 1, 1, Math.max(1, stack.getMaxStackSize())));
        return stack;
    }

    @SuppressWarnings("deprecation")
    private static void setModel(ItemMeta meta, int model) {
        // The number form still works on 1.21.4+, where it became the first float of the model data.
        meta.setCustomModelData(model);
    }

    /** Item names show as written, not in the game's default italic. */
    private static Component plainStyle(Component text) {
        return text.decorationIfAbsent(TextDecoration.ITALIC, TextDecoration.State.FALSE);
    }

    private static Enchantment enchantment(String name) {
        NamespacedKey key = NamespacedKey.fromString(name.toLowerCase(Locale.ROOT));
        if (key == null) {
            return null;
        }
        try {
            return RegistryAccess.registryAccess().getRegistry(RegistryKey.ENCHANTMENT).get(key);
        } catch (RuntimeException unknown) {
            return null;
        }
    }

    @EventHandler(priority = EventPriority.HIGH)
    public void onUse(PlayerInteractEvent event) {
        if (event.getHand() != EquipmentSlot.HAND || (event.getAction() != Action.RIGHT_CLICK_AIR && event.getAction() != Action.RIGHT_CLICK_BLOCK)) {
            return;
        }
        // A studio block or furniture piece placed with this item already took the click.
        if (event.useItemInHand() == org.bukkit.event.Event.Result.DENY) {
            return;
        }
        String id = idOf(plugin, event.getItem());
        Studio.Resource one = id == null ? null : items.get(id);
        if (one == null || !(one.data().get("useActions") instanceof List<?> list) || list.isEmpty()) {
            return;
        }
        event.setCancelled(true);
        actions.run(list, new StudioActions.Run(event.getPlayer(), Map.of("item", id)));
    }

    /** The item's action list {@code key}, or null when it has none. */
    private List<?> listOf(String id, String key) {
        Studio.Resource one = id == null ? null : items.get(id);
        return one != null && one.data().get(key) instanceof List<?> list && !list.isEmpty() ? list : null;
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onEat(PlayerItemConsumeEvent event) {
        String id = idOf(plugin, event.getItem());
        List<?> list = listOf(id, "eatActions");
        if (list != null) {
            actions.run(list, new StudioActions.Run(event.getPlayer(), Map.of("item", id)));
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onHit(EntityDamageByEntityEvent event) {
        if (!(event.getDamager() instanceof Player player)) {
            return;
        }
        String id = idOf(plugin, player.getInventory().getItemInMainHand());
        List<?> list = listOf(id, "hitActions");
        if (list != null) {
            String target = event.getEntity() instanceof Player hit ? hit.getName() : event.getEntity().getType().getKey().getKey();
            actions.run(list, new StudioActions.Run(player, Map.of("item", id, "target", target)));
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onBreak(BlockBreakEvent event) {
        String id = idOf(plugin, event.getPlayer().getInventory().getItemInMainHand());
        List<?> list = listOf(id, "breakActions");
        if (list != null) {
            actions.run(list, new StudioActions.Run(event.getPlayer(), Map.of("item", id, "block", event.getBlock().getType().getKey().getKey())));
        }
    }

    // ---------- recipes ----------

    private void applyRecipes(List<Studio.Resource> list) {
        for (NamespacedKey key : added) {
            Bukkit.removeRecipe(key);
        }
        added.clear();
        for (Studio.Resource one : list) {
            try {
                Recipe recipe = recipe(one.id(), one.data());
                NamespacedKey key = new NamespacedKey(plugin, "recipe_" + one.id().replace('-', '_'));
                Bukkit.removeRecipe(key);
                if (Bukkit.addRecipe(recipe)) {
                    added.add(key);
                } else {
                    studio.problem("recipe", one.id(), "The server didn't take this recipe.");
                }
            } catch (IllegalArgumentException | IllegalStateException error) {
                studio.problem("recipe", one.id(), error.getMessage());
            }
        }
    }

    private Recipe recipe(String id, Map<String, Object> data) {
        NamespacedKey key = new NamespacedKey(plugin, "recipe_" + id.replace('-', '_'));
        ItemStack result = result(data.get("result"));
        String type = StudioText.text(data, "type", "shaped");
        float experience = (float) Math.max(0, StudioText.number(data, "experience", 0, 0, 1000));
        int cook = StudioText.number(data, "cookTicks", 200, 1, 20 * 60 * 10);
        return switch (type) {
            case "shaped" -> {
                List<String> shape = StudioText.lines(data, "shape");
                if (shape.isEmpty() || shape.size() > 3) {
                    throw new IllegalArgumentException("\"shape\" needs 1 to 3 rows.");
                }
                int width = shape.get(0).length();
                for (String row : shape) {
                    if (row.isEmpty() || row.length() > 3 || row.length() != width) {
                        throw new IllegalArgumentException("Every row of \"shape\" must be 1 to 3 letters, all the same length.");
                    }
                }
                ShapedRecipe shaped = new ShapedRecipe(key, result).shape(shape.toArray(String[]::new));
                if (!(data.get("keys") instanceof Map<?, ?> keys)) {
                    throw new IllegalArgumentException("\"keys\" says what each letter is.");
                }
                Set<Character> used = new HashSet<>();
                for (String row : shape) {
                    for (char letter : row.toCharArray()) {
                        if (letter != ' ') {
                            used.add(letter);
                        }
                    }
                }
                for (char letter : used) {
                    Object what = keys.get(String.valueOf(letter));
                    if (!(what instanceof String name)) {
                        throw new IllegalArgumentException("\"keys\" has nothing for " + letter + ".");
                    }
                    shaped.setIngredient(letter, choice(name));
                }
                yield shaped;
            }
            case "shapeless" -> {
                List<String> ingredients = StudioText.lines(data, "ingredients");
                ingredients.removeIf(String::isBlank);
                if (ingredients.isEmpty() || ingredients.size() > 9) {
                    throw new IllegalArgumentException("\"ingredients\" needs 1 to 9 items.");
                }
                ShapelessRecipe shapeless = new ShapelessRecipe(key, result);
                for (String name : ingredients) {
                    shapeless.addIngredient(choice(name));
                }
                yield shapeless;
            }
            case "furnace" -> new FurnaceRecipe(key, result, choice(StudioText.text(data, "input", "")), experience, cook);
            case "blasting" -> new BlastingRecipe(key, result, choice(StudioText.text(data, "input", "")), experience, cook);
            case "smoking" -> new SmokingRecipe(key, result, choice(StudioText.text(data, "input", "")), experience, cook);
            case "campfire" -> new CampfireRecipe(key, result, choice(StudioText.text(data, "input", "")), experience, cook);
            case "stonecutting" -> new StonecuttingRecipe(key, result, choice(StudioText.text(data, "input", "")));
            default -> throw new IllegalArgumentException("Unknown recipe type " + type + ".");
        };
    }

    private ItemStack result(Object raw) {
        if (!(raw instanceof Map<?, ?> map)) {
            throw new IllegalArgumentException("\"result\" says what the recipe makes.");
        }
        Map<String, Object> result = StudioActions.map(map);
        int amount = StudioText.number(result, "amount", 1, 1, 64);
        ItemStack stack = actions.stack(StudioText.text(result, "item", ""), amount);
        if (stack == null) {
            throw new IllegalArgumentException("The result " + StudioText.text(result, "item", "") + " isn't an item or a studio item.");
        }
        return stack;
    }

    /** {@code diamond} matches the material; {@code studio:<id>} matches only that studio item. */
    private RecipeChoice choice(String name) {
        if (name.startsWith("studio:")) {
            ItemStack exact = stack(name.substring(7));
            if (exact == null) {
                throw new IllegalArgumentException("There is no studio item called " + name.substring(7) + ".");
            }
            exact.setAmount(1);
            return new RecipeChoice.ExactChoice(exact);
        }
        Material material = Material.matchMaterial(name);
        if (material == null || !material.isItem() || material.isAir()) {
            throw new IllegalArgumentException(name + " isn't an item.");
        }
        return new RecipeChoice.MaterialChoice(material);
    }

    void stop() {
        for (NamespacedKey key : added) {
            try {
                Bukkit.removeRecipe(key);
            } catch (RuntimeException ignored) {
                // Shutting down.
            }
        }
        added.clear();
        items = Map.of();
        if (setTask != null) {
            setTask.cancel();
            setTask = null;
        }
    }
}

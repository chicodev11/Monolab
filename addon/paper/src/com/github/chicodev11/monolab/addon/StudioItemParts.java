package com.github.chicodev11.monolab.addon;

import io.papermc.paper.datacomponent.DataComponentTypes;
import io.papermc.paper.datacomponent.item.Consumable;
import io.papermc.paper.datacomponent.item.Equippable;
import io.papermc.paper.datacomponent.item.FoodProperties;
import io.papermc.paper.datacomponent.item.ItemAttributeModifiers;
import io.papermc.paper.datacomponent.item.Tool;
import io.papermc.paper.datacomponent.item.TooltipDisplay;
import io.papermc.paper.datacomponent.item.UseCooldown;
import io.papermc.paper.datacomponent.item.UseRemainder;
import io.papermc.paper.datacomponent.item.consumable.ItemUseAnimation;
import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import io.papermc.paper.registry.tag.TagKey;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.function.Function;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.util.TriState;
import org.bukkit.Bukkit;
import org.bukkit.NamespacedKey;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeModifier;
import org.bukkit.block.BlockType;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.EquipmentSlotGroup;
import org.bukkit.inventory.ItemRarity;
import org.bukkit.inventory.ItemStack;
import org.bukkit.plugin.Plugin;

/**
 * Studio maker M1: the parts of a custom item past name and lore, set with Paper's data component API. The look is
 * the {@code item_model} {@code monolab:<id>} on 1.21.4 and later (the app's pack has {@code items/<id>.json}), and
 * custom model data from {@code studio/pack/ids.json} before. Each part is tried on its own: a server too old for one
 * skips it, and {@link #apply} says which were skipped.
 */
final class StudioItemParts {
    static final String[] RARITIES = {"common", "uncommon", "rare", "epic"};
    static final String[] EQUIP_SLOTS = {"head", "chest", "legs", "feet"};
    static final String[] TOOL_KINDS = {"pickaxe", "axe", "shovel", "hoe"};
    /** The same lists as {@code item::TRIM_PATTERNS} and {@code item::TRIM_MATERIALS}. */
    static final String[] TRIM_PATTERNS = {"sentry", "dune", "coast", "wild", "ward", "eye", "vex", "tide", "snout", "rib", "spire", "wayfinder", "shaper", "silence", "raiser", "host", "flow", "bolt"};
    static final String[] TRIM_MATERIALS = {"quartz", "iron", "netherite", "redstone", "copper", "gold", "emerald", "diamond", "lapis", "amethyst", "resin"};
    /** The player's own attack damage and speed, which item modifiers add to. */
    private static final double BASE_DAMAGE = 1;
    private static final double BASE_SPEED = 4;

    private final Plugin plugin;
    private final Path ids;
    private final boolean definitions;
    private volatile long idsModified = -1;
    private volatile Map<String, Object> numbers = Map.of();

    StudioItemParts(Plugin plugin) {
        this.plugin = plugin;
        this.ids = plugin.getDataFolder().toPath().resolve("studio").resolve("pack").resolve("ids.json");
        this.definitions = definitionsOn(serverVersion());
    }

    private static String serverVersion() {
        try {
            return Bukkit.getMinecraftVersion();
        } catch (RuntimeException | LinkageError old) {
            return Bukkit.getBukkitVersion();
        }
    }

    /** 1.21.4 brought item model definitions ({@code assets/<ns>/items/}) and the data component API. */
    static boolean definitionsOn(String version) {
        String[] parts = version.split("[^0-9]+");
        int major = parts.length > 0 && !parts[0].isEmpty() ? Integer.parseInt(parts[0]) : 0;
        int minor = parts.length > 1 ? Integer.parseInt(parts[1]) : 0;
        int patch = parts.length > 2 ? Integer.parseInt(parts[2]) : 0;
        if (major >= 26) {
            return true;
        }
        return minor > 21 || (minor == 21 && patch >= 4);
    }

    /** What is wrong with the M1 fields, or null. Same rules as {@code crates/studio-format/src/item.rs}. */
    static String problem(Map<String, Object> data) {
        if (data.get("look") instanceof Map<?, ?> raw) {
            Map<String, Object> look = StudioActions.map(raw);
            String texture = StudioText.text(look, "texture", "").trim();
            String model = StudioText.text(look, "model", "").trim();
            if (!texture.isEmpty() && !model.isEmpty()) {
                return "\"look\" takes a texture or a model, not both.";
            }
        } else if (data.get("look") != null) {
            return "\"look\" must be an object with a texture or a model.";
        }
        String rarity = StudioText.text(data, "rarity", "");
        if (!rarity.isEmpty() && !List.of(RARITIES).contains(rarity)) {
            return "\"rarity\" must be one of: " + String.join(", ", RARITIES) + ".";
        }
        if (data.get("equip") instanceof Map<?, ?> raw && !List.of(EQUIP_SLOTS).contains(StudioText.text(StudioActions.map(raw), "slot", ""))) {
            return "\"equip.slot\" must be one of: " + String.join(", ", EQUIP_SLOTS) + ".";
        }
        if (data.get("equip") instanceof Map<?, ?> raw && StudioActions.map(raw).get("trim") instanceof Map<?, ?> rawTrim) {
            Map<String, Object> trim = StudioActions.map(rawTrim);
            String[][] checks = {{"pattern", String.join(",", TRIM_PATTERNS)}, {"material", String.join(",", TRIM_MATERIALS)}};
            for (String[] check : checks) {
                String name = StudioText.text(trim, check[0], "").trim().toLowerCase(Locale.ROOT);
                if (name.isEmpty()) {
                    return "\"equip.trim\" needs a " + check[0] + ".";
                }
                if (!name.contains(":") && !List.of(check[1].split(",")).contains(name)) {
                    return "\"equip.trim." + check[0] + "\" isn't a vanilla one: " + check[1].replace(",", ", ") + "; another needs its namespace (like mypack:" + name + ").";
                }
            }
        }
        String disc = StudioText.text(data, "disc", "").trim();
        if (!disc.isEmpty() && !disc.matches("[a-z0-9_-]{1,48}")) {
            return "\"disc\" is a studio song's file name (a-z, 0-9, - and _).";
        }
        if (data.get("tool") instanceof Map<?, ?> raw) {
            for (String kind : StudioText.lines(StudioActions.map(raw), "mines")) {
                if (!List.of(TOOL_KINDS).contains(kind.trim())) {
                    return "\"tool.mines\" takes: " + String.join(", ", TOOL_KINDS) + ".";
                }
            }
        }
        return null;
    }

    static boolean hasLook(Map<String, Object> data) {
        if (!(data.get("look") instanceof Map<?, ?> raw)) {
            return false;
        }
        Map<String, Object> look = StudioActions.map(raw);
        return !StudioText.text(look, "texture", "").isBlank() || !StudioText.text(look, "model", "").isBlank();
    }

    /** Before 1.21.4: the custom model data number the app's pack gave this item, or -1 (no pack built yet). */
    @SuppressWarnings("unchecked")
    int oldModel(String id) {
        if (definitions) {
            return -1;
        }
        try {
            long modified = Files.exists(ids) ? Files.getLastModifiedTime(ids).toMillis() : 0;
            if (modified != idsModified) {
                idsModified = modified;
                Map<String, Object> next = Map.of();
                if (modified != 0 && MiniJson.parse(Files.readString(ids, StandardCharsets.UTF_8)) instanceof Map<?, ?> all
                        && all.get("given") instanceof Map<?, ?> given && given.get("cmd") instanceof Map<?, ?> cmd) {
                    next = (Map<String, Object>) cmd;
                }
                numbers = next;
            }
        } catch (java.io.IOException | RuntimeException unreadable) {
            numbers = Map.of();
        }
        return numbers.get(id) instanceof Double number ? number.intValue() : -1;
    }

    /**
     * Sets the item's components on {@code stack} (after its meta is set, which would replace them). Returns the
     * names of the parts this server couldn't take.
     */
    List<String> apply(String id, Map<String, Object> data, ItemStack stack) {
        List<String> skipped = new ArrayList<>();
        String safe = id == null ? "item" : id;
        if (hasLook(data) && definitions) {
            part("look", skipped, () -> Components.look(stack, safe));
        }
        boolean durable = StudioText.number(data, "durability", 0, 0, Integer.MAX_VALUE) > 0;
        if (durable) {
            int most = StudioText.number(data, "durability", 0, 1, Integer.MAX_VALUE);
            part("durability", skipped, () -> Components.durability(stack, most));
        } else if (data.get("maxStack") instanceof Double) {
            int most = StudioText.number(data, "maxStack", 64, 1, 99);
            part("stack size", skipped, () -> Components.stackSize(stack, most));
        }
        String rarity = StudioText.text(data, "rarity", "");
        if (!rarity.isEmpty()) {
            part("rarity", skipped, () -> Components.rarity(stack, rarity));
        }
        if (Boolean.TRUE.equals(data.get("glow"))) {
            part("glow", skipped, () -> Components.glow(stack));
        }
        if (Boolean.TRUE.equals(data.get("hideTooltip"))) {
            // 1.21.5 and later; 1.21.4's hide_tooltip isn't in this API any more.
            part("hide tooltip", skipped, () -> Components.hideTooltip(stack));
        }
        if (data.get("food") instanceof Map<?, ?> raw) {
            Map<String, Object> food = StudioActions.map(raw);
            part("food", skipped, () -> Components.food(stack, food));
        }
        double cooldown = number(data, "cooldown", 0, 0, 3600);
        if (cooldown > 0) {
            part("cooldown", skipped, () -> Components.cooldown(stack, safe, cooldown));
        }
        if (data.get("tool") instanceof Map<?, ?> raw) {
            Map<String, Object> tool = StudioActions.map(raw);
            part("tool", skipped, () -> Components.tool(stack, tool));
        }
        Map<String, Object> attack = data.get("attack") instanceof Map<?, ?> raw ? StudioActions.map(raw) : Map.of();
        Map<String, Object> equip = data.get("equip") instanceof Map<?, ?> raw ? StudioActions.map(raw) : Map.of();
        if (!equip.isEmpty()) {
            part("equip", skipped, () -> Components.equip(stack, safe, equip));
        }
        if (equip.get("trim") instanceof Map<?, ?> rawTrim) {
            Map<String, Object> trim = StudioActions.map(rawTrim);
            part("armor trim", skipped, () -> Components.trim(stack, StudioText.text(trim, "pattern", ""), StudioText.text(trim, "material", "")));
        }
        String disc = StudioText.text(data, "disc", "").trim();
        if (!disc.isEmpty()) {
            part("music disc", skipped, () -> Components.disc(stack, disc));
        }
        if (!attack.isEmpty() || number(equip, "armor", 0, 0, 30) > 0 || number(equip, "toughness", 0, 0, 20) > 0) {
            part("attack and armor", skipped, () -> Components.attributes(plugin, stack, safe, attack, equip));
        }
        return skipped;
    }

    private static void part(String name, List<String> skipped, Runnable set) {
        try {
            set.run();
        } catch (RuntimeException | LinkageError missing) {
            // An older server without this component (NoSuchFieldError, NoClassDefFoundError, or Components failing
            // to load at all before 1.21.4) or a value it refused.
            skipped.add(name);
        }
    }

    static double number(Map<String, Object> data, String key, double fallback, double least, double most) {
        double value = data.get(key) instanceof Double one && Double.isFinite(one) ? one : fallback;
        return Math.max(least, Math.min(most, value));
    }

    /**
     * Every use of the data component API, in a class of its own: on a server without that API this class fails to
     * load (inside {@link #part}'s try) instead of taking {@link StudioItemParts} and the items down with it.
     */
    private static final class Components {
        private Components() {
        }

        static void look(ItemStack stack, String id) {
            stack.setData(DataComponentTypes.ITEM_MODEL, Key.key("monolab", id));
        }

        static void durability(ItemStack stack, int most) {
            stack.setData(DataComponentTypes.MAX_STACK_SIZE, 1);
            stack.setData(DataComponentTypes.MAX_DAMAGE, most);
            stack.setData(DataComponentTypes.DAMAGE, 0);
        }

        static void stackSize(ItemStack stack, int most) {
            stack.setData(DataComponentTypes.MAX_STACK_SIZE, most);
        }

        static void rarity(ItemStack stack, String rarity) {
            stack.setData(DataComponentTypes.RARITY, ItemRarity.valueOf(rarity.toUpperCase(Locale.ROOT)));
        }

        static void glow(ItemStack stack) {
            stack.setData(DataComponentTypes.ENCHANTMENT_GLINT_OVERRIDE, true);
        }

        static void hideTooltip(ItemStack stack) {
            stack.setData(DataComponentTypes.TOOLTIP_DISPLAY, TooltipDisplay.tooltipDisplay().hideTooltip(true).build());
        }

        static void cooldown(ItemStack stack, String id, double seconds) {
            stack.setData(DataComponentTypes.USE_COOLDOWN, UseCooldown.useCooldown((float) seconds).cooldownGroup(Key.key("monolab", id)).build());
        }

        static void food(ItemStack stack, Map<String, Object> food) {
            stack.setData(DataComponentTypes.FOOD, FoodProperties.food()
                    .nutrition(StudioText.number(food, "nutrition", 4, 0, 1000))
                    .saturation((float) number(food, "saturation", 2.4, 0, 1000))
                    .canAlwaysEat(Boolean.TRUE.equals(food.get("alwaysEat")))
                    .build());
            boolean drink = "drink".equals(StudioText.text(food, "animation", "eat"));
            stack.setData(DataComponentTypes.CONSUMABLE, Consumable.consumable()
                    .consumeSeconds((float) number(food, "seconds", 1.6, 0.05, 60))
                    .animation(drink ? ItemUseAnimation.DRINK : ItemUseAnimation.EAT)
                    .sound(Key.key(drink ? "entity.generic.drink" : "entity.generic.eat"))
                    .build());
            String leftover = StudioText.text(food, "leftover", "").trim();
            if (!leftover.isEmpty()) {
                org.bukkit.Material material = org.bukkit.Material.matchMaterial(leftover);
                if (material != null && material.isItem() && !material.isAir()) {
                    stack.setData(DataComponentTypes.USE_REMAINDER, UseRemainder.useRemainder(new ItemStack(material)));
                }
            }
        }

        static void tool(ItemStack stack, Map<String, Object> tool) {
            float speed = (float) number(tool, "speed", 6, 0, 1000);
            Tool.Builder builder = Tool.tool().defaultMiningSpeed(1).damagePerBlock(StudioText.number(tool, "damagePerBlock", 1, 0, 1000));
            var blocks = RegistryAccess.registryAccess().getRegistry(RegistryKey.BLOCK);
            // Own rules first (the game takes the first that matches): vanilla blocks by name; studio blocks
            // (studio:<id>) are dug on the add-on's clock, which reads the same rules.
            if (tool.get("rules") instanceof List<?> rules) {
                for (Object raw : rules) {
                    if (!(raw instanceof Map<?, ?> rawRule)) {
                        continue;
                    }
                    Map<String, Object> rule = StudioActions.map(rawRule);
                    List<io.papermc.paper.registry.TypedKey<BlockType>> named = new ArrayList<>();
                    for (String name : StudioText.lines(rule, "blocks")) {
                        String clean = name.trim().toLowerCase(Locale.ROOT);
                        if (clean.isEmpty() || clean.startsWith("studio:")) {
                            continue;
                        }
                        Key key = Key.key(clean.contains(":") ? clean : "minecraft:" + clean);
                        if (blocks.get(key) != null) {
                            named.add(io.papermc.paper.registry.TypedKey.create(RegistryKey.BLOCK, key));
                        }
                    }
                    if (!named.isEmpty()) {
                        builder.addRule(Tool.rule(io.papermc.paper.registry.set.RegistrySet.keySet(RegistryKey.BLOCK, named), (float) number(rule, "speed", 6, 0, 1000),
                                Boolean.FALSE.equals(rule.get("drops")) ? TriState.FALSE : TriState.TRUE));
                    }
                }
            }
            for (String kind : StudioText.lines(tool, "mines")) {
                TagKey<BlockType> tag = TagKey.create(RegistryKey.BLOCK, Key.key("minecraft", "mineable/" + kind.trim().toLowerCase(Locale.ROOT)));
                builder.addRule(Tool.rule(blocks.getTag(tag), speed, TriState.TRUE));
            }
            stack.setData(DataComponentTypes.TOOL, builder.build());
        }

        private static EquipmentSlot slot(String name) {
            return switch (name) {
                case "head" -> EquipmentSlot.HEAD;
                case "chest" -> EquipmentSlot.CHEST;
                case "legs" -> EquipmentSlot.LEGS;
                default -> EquipmentSlot.FEET;
            };
        }

        private static EquipmentSlotGroup group(String name) {
            return switch (name) {
                case "head" -> EquipmentSlotGroup.HEAD;
                case "chest" -> EquipmentSlotGroup.CHEST;
                case "legs" -> EquipmentSlotGroup.LEGS;
                default -> EquipmentSlotGroup.FEET;
            };
        }

        static void equip(ItemStack stack, String id, Map<String, Object> equip) {
            Equippable.Builder builder = Equippable.equippable(slot(StudioText.text(equip, "slot", "head")));
            boolean worn = !StudioText.text(equip, "texture", "").isBlank() || !StudioText.text(equip, "legsTexture", "").isBlank();
            if (worn) {
                // The app's pack has equipment/<id>.json drawing the armor textures.
                builder.assetId(Key.key("monolab", id));
            }
            stack.setData(DataComponentTypes.EQUIPPABLE, builder.build());
        }

        /** A studio song (registered when the server started) for jukeboxes to play. */
        static void disc(ItemStack stack, String song) {
            org.bukkit.JukeboxSong found = RegistryAccess.registryAccess().getRegistry(RegistryKey.JUKEBOX_SONG).get(Key.key("monolab", song));
            if (found == null) {
                throw new IllegalArgumentException("no song " + song);
            }
            stack.setData(DataComponentTypes.JUKEBOX_PLAYABLE, io.papermc.paper.datacomponent.item.JukeboxPlayable.jukeboxPlayable(found).build());
        }

        /** An armor trim from the server's registries (vanilla, or one a datapack added). */
        static void trim(ItemStack stack, String pattern, String material) {
            Key patternKey = Key.key(pattern.contains(":") ? pattern.trim().toLowerCase(Locale.ROOT) : "minecraft:" + pattern.trim().toLowerCase(Locale.ROOT));
            Key materialKey = Key.key(material.contains(":") ? material.trim().toLowerCase(Locale.ROOT) : "minecraft:" + material.trim().toLowerCase(Locale.ROOT));
            org.bukkit.inventory.meta.trim.TrimPattern foundPattern = RegistryAccess.registryAccess().getRegistry(RegistryKey.TRIM_PATTERN).get(patternKey);
            org.bukkit.inventory.meta.trim.TrimMaterial foundMaterial = RegistryAccess.registryAccess().getRegistry(RegistryKey.TRIM_MATERIAL).get(materialKey);
            if (foundPattern == null || foundMaterial == null) {
                throw new IllegalArgumentException("unknown trim");
            }
            stack.setData(DataComponentTypes.TRIM, io.papermc.paper.datacomponent.item.ItemArmorTrim.itemArmorTrim(new org.bukkit.inventory.meta.trim.ArmorTrim(foundMaterial, foundPattern)).build());
        }

        static void attributes(Plugin plugin, ItemStack stack, String id, Map<String, Object> attack, Map<String, Object> equip) {
            ItemAttributeModifiers.Builder builder = ItemAttributeModifiers.itemAttributes();
            Function<String, NamespacedKey> key = name -> new NamespacedKey(plugin, id.replace('-', '_') + "_" + name);
            if (attack.get("damage") instanceof Double) {
                builder.addModifier(Attribute.ATTACK_DAMAGE, new AttributeModifier(key.apply("damage"), number(attack, "damage", 1, 0, 2048) - BASE_DAMAGE,
                        AttributeModifier.Operation.ADD_NUMBER, EquipmentSlotGroup.MAINHAND));
            }
            if (attack.get("speed") instanceof Double) {
                builder.addModifier(Attribute.ATTACK_SPEED, new AttributeModifier(key.apply("speed"), number(attack, "speed", 4, 0, 1024) - BASE_SPEED,
                        AttributeModifier.Operation.ADD_NUMBER, EquipmentSlotGroup.MAINHAND));
            }
            if (!equip.isEmpty()) {
                EquipmentSlotGroup where = group(StudioText.text(equip, "slot", "head"));
                double armor = number(equip, "armor", 0, 0, 30);
                double toughness = number(equip, "toughness", 0, 0, 20);
                if (armor > 0) {
                    builder.addModifier(Attribute.ARMOR, new AttributeModifier(key.apply("armor"), armor, AttributeModifier.Operation.ADD_NUMBER, where));
                }
                if (toughness > 0) {
                    builder.addModifier(Attribute.ARMOR_TOUGHNESS, new AttributeModifier(key.apply("toughness"), toughness, AttributeModifier.Operation.ADD_NUMBER, where));
                }
            }
            stack.setData(DataComponentTypes.ATTRIBUTE_MODIFIERS, builder.build());
        }
    }
}

package com.github.chicodev11.monolab.addon;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ThreadLocalRandom;
import java.util.function.BiConsumer;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.TextDecoration;
import org.bukkit.Bukkit;
import org.bukkit.Chunk;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Particle;
import org.bukkit.Registry;
import org.bukkit.SoundCategory;
import org.bukkit.SoundGroup;
import org.bukkit.World;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeInstance;
import org.bukkit.attribute.AttributeModifier;
import org.bukkit.block.Block;
import org.bukkit.block.BlockFace;
import org.bukkit.block.BlockState;
import org.bukkit.block.data.BlockData;
import org.bukkit.block.data.type.Farmland;
import org.bukkit.entity.ExperienceOrb;
import org.bukkit.entity.LivingEntity;
import org.bukkit.entity.Player;
import org.bukkit.event.Event;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.block.Action;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.block.BlockDamageAbortEvent;
import org.bukkit.event.block.BlockDamageEvent;
import org.bukkit.event.block.BlockExplodeEvent;
import org.bukkit.event.block.BlockPhysicsEvent;
import org.bukkit.event.block.BlockPistonExtendEvent;
import org.bukkit.event.block.BlockPistonRetractEvent;
import org.bukkit.event.block.BlockPlaceEvent;
import org.bukkit.event.block.NotePlayEvent;
import org.bukkit.event.entity.EntityExplodeEvent;
import org.bukkit.event.player.PlayerInteractEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.event.world.ChunkLoadEvent;
import org.bukkit.event.world.ChunkUnloadEvent;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.EquipmentSlotGroup;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.Plugin;
import org.bukkit.potion.PotionEffect;
import org.bukkit.potion.PotionEffectType;
import org.bukkit.util.BoundingBox;

/**
 * Studio maker M3: custom blocks ({@code block}). A studio block is a vanilla block state the game never shows on its
 * own (a note block's instrument and note, a mushroom block's faces, a tripwire's wires) that the app's pack draws
 * as ours; which state is the number in {@code studio/pack/ids.json} (kinds {@code block} and {@code plant}). The
 * borrowed blocks' own behaviour is held still (note blocks don't change or play, mushroom faces and wires don't
 * update), and breaking runs on our clock: the player's break speed is 0 while we count the block's own time.
 * Same rules as {@code crates/studio-format/src/block.rs}.
 *
 * <p>Crops ({@code growth}) borrow one plant state per stage ({@code <id>/<stage>} in {@code ids.json}). Tripwire gets
 * no random ticks, so each chunk keeps where its growing crops are ({@code crops}, packed spots) and once a second
 * every one rolls its chance on its own region's thread. A grown crop leaves the list.
 */
final class StudioBlocks implements Listener {
    /** When each player last placed one of ours (see {@code onInteract}). */
    private final Map<UUID, Long> lastPlaced = new ConcurrentHashMap<>();
    static final String[] SHAPES = {"cube", "plant"};
    static final String[] ACTION_LISTS = {"placeActions", "breakActions", "useActions"};
    static final String[] INSTRUMENTS = {
        "harp", "basedrum", "snare", "hat", "bass", "flute", "bell", "guitar", "chime", "xylophone", "iron_xylophone",
        "cow_bell", "didgeridoo", "bit", "banjo", "pling"
    };
    static final String[] MUSHROOMS = {"brown_mushroom_block", "red_mushroom_block", "mushroom_stem"};
    static final String[] FACES = {"down", "east", "north", "south", "up", "west"};
    static final String[] WIRE = {"attached", "disarmed", "east", "north", "powered", "south", "west"};
    static final int NOTES = 25;
    static final int NOTE_STATES = INSTRUMENTS.length * NOTES * 2 - 1;
    static final int CUBE_STATES = NOTE_STATES + 63 * MUSHROOMS.length;
    static final int PLANT_STATES = 127;
    static final double MOST_HARDNESS = 50;
    /** Blocks further than this from a breaking block don't see its cracks. */
    private static final double CRACK_DISTANCE = 32;

    /** A loaded block: its file, its borrowed state and the item that places it ({@code null} when it has none yet). */
    record Placed(String id, Map<String, Object> data, String shape, BlockData state, List<BlockData> stages) {}

    /** A crop's rules, the same as {@code block::Growth}. {@code last} is the fully grown stage's number. */
    record Growth(int last, double minutes, int light, List<String> soil, boolean water, boolean bonemeal, String harvestLoot, int harvestExp) {
        boolean soilOk(Material below) {
            return soil.isEmpty() || soil.contains(below.getKey().getKey());
        }

        double chance(double seconds) {
            return Math.max(0, Math.min(1, seconds / (minutes * 60)));
        }
    }

    static final int MOST_STAGES = 15;

    private final Plugin plugin;
    private final Studio studio;
    private final StudioActions actions;
    private final BiConsumer<String, Throwable> warn;
    private final NamespacedKey tag;
    private final NamespacedKey slow;
    private final PackIds ids;
    private final boolean definitions;
    private volatile Map<String, Placed> blocks = Map.of();
    private volatile Map<BlockData, Placed> byState = Map.of();
    /** Studio item id → the block it places. */
    private volatile Map<String, Placed> byItem = Map.of();
    private volatile boolean mushroomsUsed;
    private volatile boolean plantsUsed;
    private final Map<UUID, Breaking> breaking = new ConcurrentHashMap<>();
    /** Plain note blocks that were powered at their last physics update (to play on the rising edge). */
    private final Set<String> poweredNotes = new HashSet<>();
    /** Borrowed stage states of crops → their stage (1..); a crop's own state is stage 0. */
    private volatile Map<BlockData, Integer> stageByState = Map.of();
    /** Loaded chunks with growing crops, by {@code world:x:z}. */
    private final Map<String, CropChunk> crops = new ConcurrentHashMap<>();
    private final NamespacedKey cropsKey;
    private io.papermc.paper.threadedregions.scheduler.ScheduledTask growing;

    /** One loaded chunk's growing crops, as packed spots ({@link #cropSpot(Block)}). */
    private record CropChunk(World world, int x, int z, Set<Integer> spots) {}

    /** The borrowed state one of our blocks is placed as, for world generators; null if there's none (yet). */
    BlockData stateOf(String id) {
        Placed one = blocks.get(id);
        return one == null ? null : one.state();
    }

    /** The studio item file a stack was made from, or null (set by the plugin: items are another feature). */
    private volatile java.util.function.Function<ItemStack, Map<String, Object>> itemData = stack -> null;

    void itemData(java.util.function.Function<ItemStack, Map<String, Object>> lookup) {
        itemData = lookup;
    }

    /** {@code block::dig_with}: break ticks for a held stack, a studio tool's own rules counting. */
    private int ticksWith(Placed one, ItemStack held) {
        double[] studio = studioDig(one, held);
        if (studio != null) {
            boolean needs = Boolean.TRUE.equals(one.data().get("needsTool"));
            return breakTicks(hardness(one.data()), studio[0], !needs || studio[1] > 0);
        }
        return ticksFor(one.data(), held == null ? Material.AIR : held.getType());
    }

    private boolean harvestsWith(Placed one, ItemStack held) {
        double[] studio = studioDig(one, held);
        if (studio != null) {
            return !Boolean.TRUE.equals(one.data().get("needsTool")) || studio[1] > 0;
        }
        return harvests(one.data(), held == null ? Material.AIR : held.getType());
    }

    /** {@code item::digs_studio_block}: (speed, 1 when it counts as the right tool), or null. */
    private double[] studioDig(Placed one, ItemStack held) {
        Map<String, Object> item = held == null ? null : itemData.apply(held);
        if (item == null || !(item.get("tool") instanceof Map<?, ?> rawTool)) {
            return null;
        }
        Map<String, Object> tool = StudioActions.map(rawTool);
        if (tool.get("rules") instanceof List<?> rules) {
            for (Object raw : rules) {
                if (raw instanceof Map<?, ?> rawRule) {
                    Map<String, Object> rule = StudioActions.map(rawRule);
                    for (String name : StudioText.lines(rule, "blocks")) {
                        if (name.trim().toLowerCase(java.util.Locale.ROOT).equals("studio:" + one.id())) {
                            return new double[] {StudioItemParts.number(rule, "speed", 6, 0, 1000), Boolean.FALSE.equals(rule.get("drops")) ? 0 : 1};
                        }
                    }
                }
            }
        }
        String blockTool = StudioText.text(one.data(), "tool", "").trim();
        if (!blockTool.isEmpty()) {
            for (String kind : StudioText.lines(tool, "mines")) {
                if (kind.trim().equals(blockTool)) {
                    return new double[] {StudioItemParts.number(tool, "speed", 6, 0, 1000), 1};
                }
            }
        }
        return null;
    }

    /** One player breaking one of our blocks: ticks counted against ticks needed. */
    private static final class Breaking {
        final Block block;
        final int needed;
        int done;
        io.papermc.paper.threadedregions.scheduler.ScheduledTask task;

        Breaking(Block block, int needed) {
            this.block = block;
            this.needed = needed;
        }
    }

    private static volatile StudioBlocks current;

    /** The borrowed state one of our blocks is placed as (for the rule step "Set a block"); null when unknown. */
    static BlockData stateFor(String id) {
        StudioBlocks running = current;
        return running == null ? null : running.stateOf(id);
    }

    StudioBlocks(Plugin plugin, Studio studio, StudioActions actions, BiConsumer<String, Throwable> warn) {
        current = this;
        this.plugin = plugin;
        this.studio = studio;
        this.actions = actions;
        this.warn = warn;
        this.tag = new NamespacedKey(plugin, "block");
        this.slow = new NamespacedKey(plugin, "block_breaking");
        this.cropsKey = new NamespacedKey(plugin, "crops");
        this.ids = new PackIds(plugin.getDataFolder().toPath().resolve("studio").resolve("pack").resolve("ids.json"));
        this.definitions = StudioItemParts.definitionsOn(serverVersion());
    }

    private static String serverVersion() {
        try {
            return Bukkit.getMinecraftVersion();
        } catch (RuntimeException | LinkageError old) {
            return Bukkit.getBukkitVersion();
        }
    }

    void register() {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "block";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                load(resources);
            }
        });
        growing = Bukkit.getGlobalRegionScheduler().runAtFixedRate(plugin, task -> growTick(), 20L, 20L);
        if (!folia()) {
            // Chunks loaded before us; on Folia no thread may read them all, and they come with their load events.
            for (World world : Bukkit.getWorlds()) {
                for (Chunk chunk : world.getLoadedChunks()) {
                    readCrops(chunk);
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

    private void load(List<Studio.Resource> resources) {
        Map<String, Placed> next = new HashMap<>();
        Map<BlockData, Placed> states = new HashMap<>();
        Map<String, Placed> items = new HashMap<>();
        Map<BlockData, Integer> stageStates = new HashMap<>();
        boolean mushrooms = false;
        boolean plants = false;
        List<String> hints = updateHints();
        for (Studio.Resource one : resources) {
            Map<String, Object> data = one.data();
            String problem = problem(data);
            if (problem != null) {
                studio.problem("block", one.id(), problem);
                continue;
            }
            String shape = shape(data);
            int number = ids.number("plant".equals(shape) ? "plant" : "block", one.id());
            if (number <= 0) {
                studio.problem("block", one.id(), "This block has no state yet: save it in the app so the resource pack is built.");
                continue;
            }
            String text = state(shape, number);
            if (text == null) {
                studio.problem("block", one.id(), "No free block state is left for this block (" + ("plant".equals(shape) ? PLANT_STATES + " plants" : CUBE_STATES + " cube blocks") + " at most).");
                continue;
            }
            BlockData state;
            try {
                state = Bukkit.createBlockData(text);
            } catch (IllegalArgumentException refused) {
                studio.problem("block", one.id(), "This server doesn't know the block state " + text + ".");
                continue;
            }
            List<BlockData> stages = new ArrayList<>();
            Growth growth = growth(data);
            String stageProblem = null;
            for (int stage = 1; growth != null && stage <= growth.last(); stage++) {
                int stageNumber = ids.number("plant", one.id() + "/" + stage);
                String stageText = stageNumber > 0 ? state(shape, stageNumber) : null;
                BlockData stageState = null;
                try {
                    stageState = stageText == null ? null : Bukkit.createBlockData(stageText);
                } catch (IllegalArgumentException refused) {
                    // Reported below.
                }
                if (stageState == null) {
                    stageProblem = stageNumber <= 0 ? "Stage " + stage + " has no state yet: save it in the app so the resource pack is built." : "No free plant state is left for stage " + stage + " (" + PLANT_STATES + " plants at most).";
                    break;
                }
                stages.add(stageState);
            }
            if (stageProblem != null) {
                studio.problem("block", one.id(), stageProblem);
                continue;
            }
            Placed placed = new Placed(one.id(), data, shape, state, List.copyOf(stages));
            next.put(one.id(), placed);
            states.put(state, placed);
            for (int at = 0; at < stages.size(); at++) {
                states.put(stages.get(at), placed);
                stageStates.put(stages.get(at), at + 1);
            }
            String link = StudioText.text(data, "item", "").trim();
            if (!link.isEmpty()) {
                items.put(link, placed);
            }
            mushrooms |= number > NOTE_STATES && !"plant".equals(shape);
            plants |= "plant".equals(shape);
            List<String> mine = new ArrayList<>();
            for (String hint : hints) {
                if (hint.startsWith(state.getMaterial().getKey().getKey() + ":")) {
                    mine.add(hint.substring(hint.indexOf(':') + 1));
                }
            }
            if (!mine.isEmpty()) {
                studio.problem("block", one.id(), "Works, but " + String.join(" ", mine));
            }
        }
        blocks = Map.copyOf(next);
        byState = Map.copyOf(states);
        byItem = Map.copyOf(items);
        stageByState = Map.copyOf(stageStates);
        mushroomsUsed = mushrooms;
        plantsUsed = plants;
    }

    /**
     * Paper's own switches that stop the borrowed blocks from updating their shape, which this listener can't fully
     * stop on its own: {@code "<block>:<hint>"} for each switch that is off.
     */
    private List<String> updateHints() {
        List<String> out = new ArrayList<>();
        Path file = plugin.getServer().getWorldContainer().toPath().resolve("config").resolve("paper-global.yml");
        String text;
        try {
            text = Files.exists(file) ? Files.readString(file, StandardCharsets.UTF_8) : "";
        } catch (java.io.IOException | RuntimeException unreadable) {
            return out;
        }
        String[][] switches = {
            {"note_block", "disable-noteblock-updates"},
            {"brown_mushroom_block", "disable-mushroom-block-updates"},
            {"red_mushroom_block", "disable-mushroom-block-updates"},
            {"mushroom_stem", "disable-mushroom-block-updates"},
            {"tripwire", "disable-tripwire-updates"},
        };
        for (String[] one : switches) {
            if (!text.contains(one[1] + ": true")) {
                out.add(one[0] + ":turn on block-updates." + one[1] + " in config/paper-global.yml so a block next to it changing can't turn it into another.");
            }
        }
        return out;
    }

    // ---------- the shared rules ----------

    static String shape(Map<String, Object> data) {
        String one = StudioText.text(data, "shape", "cube");
        return List.of(SHAPES).contains(one) ? one : "cube";
    }

    /** {@code minecraft:note_block[instrument=bass,note=3,powered=false]} for a number, or null past the last. */
    static String state(String shape, int number) {
        if (number <= 0) {
            return null;
        }
        StringBuilder out = new StringBuilder("minecraft:");
        if ("plant".equals(shape)) {
            if (number > PLANT_STATES) {
                return null;
            }
            out.append("tripwire[");
            for (int bit = 0; bit < WIRE.length; bit++) {
                out.append(bit == 0 ? "" : ",").append(WIRE[bit]).append('=').append((number >> bit & 1) == 1);
            }
            return out.append(']').toString();
        }
        if (number <= NOTE_STATES) {
            return out.append("note_block[instrument=").append(INSTRUMENTS[number / (NOTES * 2)])
                    .append(",note=").append((number / 2) % NOTES)
                    .append(",powered=").append(number % 2 == 1).append(']').toString();
        }
        int rest = number - NOTE_STATES - 1;
        if (rest >= 63 * MUSHROOMS.length) {
            return null;
        }
        int faces = rest % 63;
        out.append(MUSHROOMS[rest / 63]).append('[');
        for (int bit = 0; bit < FACES.length; bit++) {
            out.append(bit == 0 ? "" : ",").append(FACES[bit]).append('=').append((faces >> bit & 1) == 1);
        }
        return out.append(']').toString();
    }

    static double hardness(Map<String, Object> data) {
        return StudioItemParts.number(data, "hardness", 1, 0, MOST_HARDNESS);
    }

    /** The vanilla tier speed of {@code material} when it is a {@code tool}, or -1. */
    static double toolSpeed(Material material, String tool) {
        if (tool.isEmpty() || material == null) {
            return -1;
        }
        String name = material.getKey().getKey();
        if (!name.endsWith("_" + tool)) {
            return -1;
        }
        return switch (name.substring(0, name.length() - tool.length() - 1)) {
            case "wooden" -> 2;
            case "stone" -> 4;
            case "copper" -> 5;
            case "iron" -> 6;
            case "diamond" -> 8;
            case "netherite" -> 9;
            case "golden" -> 12;
            default -> 1;
        };
    }

    static int breakTicks(double hardness, double speed, boolean harvest) {
        if (hardness <= 0) {
            return 0;
        }
        double perTick = Math.max(0.01, speed) / hardness / (harvest ? 30 : 100);
        return perTick >= 1 ? 0 : (int) Math.ceil(1 / perTick);
    }

    static boolean harvests(Map<String, Object> data, Material held) {
        return !Boolean.TRUE.equals(data.get("needsTool")) || toolSpeed(held, StudioText.text(data, "tool", "")) > 0;
    }

    static int ticksFor(Map<String, Object> data, Material held) {
        double speed = toolSpeed(held, StudioText.text(data, "tool", ""));
        return breakTicks(hardness(data), speed > 0 ? speed : 1, harvests(data, held));
    }

    /** A plant's growing rules, or null when it doesn't grow. Same rules as {@code block::growth}. */
    static Growth growth(Map<String, Object> data) {
        if (!"plant".equals(shape(data)) || !(data.get("growth") instanceof Map<?, ?> raw)) {
            return null;
        }
        Map<String, Object> part = StudioActions.map(raw);
        int stages = 0;
        for (String one : StudioText.lines(part, "stages")) {
            if (!one.trim().isEmpty() && stages < MOST_STAGES) {
                stages++;
            }
        }
        if (stages == 0) {
            return null;
        }
        List<String> soil = new ArrayList<>();
        if (part.get("soil") instanceof List<?>) {
            for (String one : StudioText.lines(part, "soil")) {
                String bare = one.trim().toLowerCase(java.util.Locale.ROOT).replaceFirst("^minecraft:", "");
                if (!bare.isEmpty()) {
                    soil.add(bare);
                }
            }
        } else {
            soil.add("farmland");
        }
        Map<String, Object> harvest = StudioText.object(part, "harvest");
        return new Growth(stages, StudioItemParts.number(part, "minutes", 5, 0.05, 1440), StudioText.number(part, "light", 0, 0, 15), List.copyOf(soil),
                Boolean.TRUE.equals(part.get("water")), !Boolean.FALSE.equals(part.get("bonemeal")), StudioText.text(harvest, "loot", "").trim(), StudioText.number(harvest, "exp", 0, 0, 1000));
    }

    /** What is wrong with a block file, or null. Same rules as {@code block::problems}. */
    static String problem(Map<String, Object> data) {
        if (!List.of(SHAPES).contains(StudioText.text(data, "shape", "cube"))) {
            return "\"shape\" must be one of: " + String.join(", ", SHAPES) + ".";
        }
        if (!(data.get("look") instanceof Map<?, ?> raw)) {
            return data.get("look") == null ? "Pick a texture or a model in \"look\"." : "\"look\" must be an object with a texture or a model.";
        }
        Map<String, Object> look = StudioActions.map(raw);
        String texture = StudioText.text(look, "texture", "").trim();
        String model = StudioText.text(look, "model", "").trim();
        if (texture.isEmpty() && model.isEmpty()) {
            return "\"look\" needs a texture or a model.";
        }
        if (!texture.isEmpty() && !model.isEmpty()) {
            return "\"look\" takes a texture or a model, not both.";
        }
        String tool = StudioText.text(data, "tool", "");
        if (!tool.isEmpty() && !List.of(StudioItemParts.TOOL_KINDS).contains(tool)) {
            return "\"tool\" must be one of: " + String.join(", ", StudioItemParts.TOOL_KINDS) + ".";
        }
        if (Boolean.TRUE.equals(data.get("needsTool")) && tool.isEmpty()) {
            return "\"needsTool\" needs a \"tool\".";
        }
        Object growth = data.get("growth");
        if (growth != null) {
            if (!(growth instanceof Map<?, ?> rawGrowth)) {
                return "\"growth\" must be an object.";
            }
            if (!"plant".equals(StudioText.text(data, "shape", "cube"))) {
                return "\"growth\" is for plants only.";
            }
            List<String> stages = StudioText.lines(StudioActions.map(rawGrowth), "stages");
            if (stages.stream().allMatch(one -> one.trim().isEmpty())) {
                return "\"growth\" needs at least one later stage in \"stages\".";
            }
            if (stages.size() > MOST_STAGES) {
                return "\"growth\" takes at most " + MOST_STAGES + " later stages.";
            }
        }
        for (String list : ACTION_LISTS) {
            String wrong = StudioActions.problem(data.get(list), (kind, id) -> true);
            if (wrong != null) {
                return wrong;
            }
        }
        return null;
    }

    // ---------- items ----------

    /** A block's own item ({@code block/<id>}), for the {@code give} action and loot. Null when there is none. */
    ItemStack stack(String ref) {
        if (!ref.startsWith("block/")) {
            return null;
        }
        Placed one = blocks.get(ref.substring(6));
        return one == null ? null : ownItem(one);
    }

    private ItemStack ownItem(Placed one) {
        String link = StudioText.text(one.data(), "item", "").trim();
        if (!link.isEmpty()) {
            return actions.stack("studio:" + link, 1);
        }
        // Not a block item (like CraftEngine): with one the client would put down a note block or tripwire itself before
        // the server places ours, and play its own sound too.
        ItemStack stack = new ItemStack(Material.PAPER);
        ItemMeta meta = stack.getItemMeta();
        if (StudioText.has(one.data(), "name")) {
            meta.displayName(plain(StudioText.render(StudioText.text(one.data(), "name", ""), null)));
        }
        List<String> lore = StudioText.lines(one.data(), "lore");
        if (!lore.isEmpty()) {
            List<Component> lines = new ArrayList<>();
            for (String line : lore) {
                lines.add(plain(StudioText.render(line, null)));
            }
            meta.lore(lines);
        }
        meta.getPersistentDataContainer().set(tag, PersistentDataType.STRING, one.id());
        if (!definitions) {
            int number = ids.number("cmd", "block:" + one.id());
            if (number > 0) {
                setModel(meta, number);
            }
        }
        stack.setItemMeta(meta);
        if (definitions) {
            try {
                Model.set(stack, "block/" + one.id());
            } catch (RuntimeException | LinkageError old) {
                // No item model component here; the item shows as a plain note block.
            }
        }
        return stack;
    }

    @SuppressWarnings("deprecation")
    static void setModel(ItemMeta meta, int number) {
        meta.setCustomModelData(number);
    }

    static Component plain(Component text) {
        return text.decorationIfAbsent(TextDecoration.ITALIC, TextDecoration.State.FALSE);
    }

    /** Kept apart so a server without data components fails here, not when the class loads. */
    static final class Model {
        static void set(ItemStack stack, String path) {
            stack.setData(io.papermc.paper.datacomponent.DataComponentTypes.ITEM_MODEL, net.kyori.adventure.key.Key.key("monolab", path));
        }
    }

    /** A middle click on one of ours: what breaking it gives back, known in the inventory by the block it places. */
    StudioPicks.Pick pick(Block block) {
        Placed one = at(block);
        ItemStack item = one == null ? null : ownItem(one);
        if (item == null) {
            return null;
        }
        return new StudioPicks.Pick(item, stack -> {
            Placed placed = placedBy(stack);
            return placed != null && placed.id().equals(one.id());
        });
    }

    /** The block a stack places: its own item, or a studio item a block links to. */
    private Placed placedBy(ItemStack stack) {
        if (stack == null || !stack.hasItemMeta()) {
            return null;
        }
        String own = stack.getItemMeta().getPersistentDataContainer().get(tag, PersistentDataType.STRING);
        if (own != null) {
            return blocks.get(own);
        }
        String item = StudioItems.idOf(plugin, stack);
        return item == null ? null : byItem.get(item);
    }

    /** The studio block at a spot, or null. */
    Placed at(Block block) {
        Map<BlockData, Placed> states = byState;
        return states.isEmpty() ? null : states.get(block.getBlockData());
    }

    private boolean borrowed(Material type) {
        return type == Material.NOTE_BLOCK
                || (mushroomsUsed && (type == Material.BROWN_MUSHROOM_BLOCK || type == Material.RED_MUSHROOM_BLOCK || type == Material.MUSHROOM_STEM))
                || (plantsUsed && type == Material.TRIPWIRE);
    }

    // ---------- placing and using ----------

    @EventHandler(priority = EventPriority.LOW)
    public void onInteract(PlayerInteractEvent event) {
        if (event.getAction() != Action.RIGHT_CLICK_BLOCK || event.getClickedBlock() == null || blocks.isEmpty()) {
            return;
        }
        Player player = event.getPlayer();
        Block clicked = event.getClickedBlock();
        Placed there = at(clicked);
        if (there != null && !player.isSneaking()) {
            // Ours: no tuning; its use actions instead.
            event.setUseInteractedBlock(Event.Result.DENY);
            Growth growth = growth(there.data());
            ItemStack held = event.getItem();
            if (growth != null && growth.bonemeal() && held != null && held.getType() == Material.BONE_MEAL) {
                event.setUseItemInHand(Event.Result.DENY);
                bonemeal(player, clicked, there, growth, held);
                return;
            }
            if (event.getHand() == EquipmentSlot.HAND && run(there, "useActions", player)) {
                event.setUseItemInHand(Event.Result.DENY);
                return;
            }
        } else if (clicked.getType() == Material.NOTE_BLOCK && !player.isSneaking()) {
            // Retuning a plain note block would turn it into one of ours, so we keep its note and tune it ourselves.
            event.setUseInteractedBlock(Event.Result.DENY);
            if (event.getHand() == EquipmentSlot.HAND && placedBy(event.getItem()) == null) {
                event.setUseItemInHand(Event.Result.DENY);
                setNote(clicked, (note(clicked) + 1) % 25);
                playNote(clicked);
                return;
            }
        }
        Placed placing = placedBy(event.getItem());
        if (placing == null || event.getHand() == null) {
            return;
        }
        event.setCancelled(true);
        // One click can arrive as two interact events; the second would place another block against the first.
        long now = System.currentTimeMillis();
        Long before = lastPlaced.put(player.getUniqueId(), now);
        if (before != null && now - before < 150) {
            return;
        }
        place(player, placing, clicked, event.getBlockFace(), event.getItem(), event.getHand());
    }

    private void place(Player player, Placed one, Block clicked, BlockFace face, ItemStack item, EquipmentSlot hand) {
        Block target = clicked.isReplaceable() ? clicked : clicked.getRelative(face);
        if (!target.isReplaceable() || target.getY() >= target.getWorld().getMaxHeight() || target.getY() < target.getWorld().getMinHeight()) {
            return;
        }
        boolean plant = "plant".equals(one.shape());
        Growth growth = growth(one.data());
        Material below = target.getRelative(BlockFace.DOWN).getType();
        if (plant && (growth != null ? !growth.soilOk(below) || !below.isSolid() && growth.soil().isEmpty() : !below.isSolid())) {
            return;
        }
        if (!plant && !target.getWorld().getNearbyEntities(BoundingBox.of(target), entity -> entity instanceof LivingEntity && entity.getType() != org.bukkit.entity.EntityType.ARMOR_STAND || entity instanceof Player).isEmpty()) {
            return;
        }
        BlockState replaced = target.getState();
        target.setBlockData(one.state(), false);
        BlockPlaceEvent check = new BlockPlaceEvent(target, replaced, clicked, item, player, true, hand);
        Bukkit.getPluginManager().callEvent(check);
        if (check.isCancelled() || !check.canBuild()) {
            replaced.update(true, false);
            return;
        }
        if (player.getGameMode() != GameMode.CREATIVE) {
            item.setAmount(item.getAmount() - 1);
        }
        player.swingHand(hand);
        sound(one, target, true);
        if (growth != null) {
            trackCrop(target);
        }
        run(one, "placeActions", player);
    }

    /** A vanilla note or mushroom block placed: its plain look, so it never matches one of ours. */
    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onVanillaPlace(BlockPlaceEvent event) {
        Block block = event.getBlockPlaced();
        if (blocks.isEmpty() || at(block) != null && placedBy(event.getItemInHand()) != null) {
            return;
        }
        Material type = block.getType();
        if (type == Material.NOTE_BLOCK) {
            forgetNote(block);
        }
        if (type == Material.NOTE_BLOCK || (mushroomsUsed && (type == Material.BROWN_MUSHROOM_BLOCK || type == Material.RED_MUSHROOM_BLOCK || type == Material.MUSHROOM_STEM))) {
            block.setBlockData(type.createBlockData(), false);
        }
    }

    @EventHandler(priority = EventPriority.HIGHEST, ignoreCancelled = true)
    public void onPhysics(BlockPhysicsEvent event) {
        Block block = event.getBlock();
        if (blocks.isEmpty() || !borrowed(block.getType())) {
            return;
        }
        event.setCancelled(true);
        if (block.getType() == Material.NOTE_BLOCK && at(block) == null) {
            // A plain note block never turns powered (that state may be ours), so we play its rising edge.
            String spot = spot(block);
            boolean powered = block.isBlockIndirectlyPowered() || block.isBlockPowered();
            if (powered && poweredNotes.add(spot)) {
                playNote(block);
            } else if (!powered) {
                poweredNotes.remove(spot);
            }
        }
    }

    @EventHandler(priority = EventPriority.HIGHEST, ignoreCancelled = true)
    public void onNote(NotePlayEvent event) {
        Block block = event.getBlock();
        if (at(block) != null) {
            event.setCancelled(true);
        } else if (!blocks.isEmpty() && block.getType() == Material.NOTE_BLOCK) {
            // A punch on a plain note block: its world state is always note 0, so we play the note we keep.
            event.setCancelled(true);
            playNote(block);
        }
    }

    // ---------- plain note blocks ----------
    // Plain note blocks stay `harp, note 0` while studio blocks borrow the other note block states. CraftEngine
    // registers real server blocks so vanilla ones keep working; Paper's API can't, so we keep each plain note
    // block's note in its chunk and tune and play it ourselves.

    private static String spot(Block block) {
        return block.getWorld().getUID() + ":" + block.getX() + "," + block.getY() + "," + block.getZ();
    }

    private NamespacedKey noteKey(Block block) {
        return new NamespacedKey(plugin, "note." + (block.getX() & 15) + "." + (block.getY() - block.getWorld().getMinHeight()) + "." + (block.getZ() & 15));
    }

    private int note(Block block) {
        Integer note = block.getChunk().getPersistentDataContainer().get(noteKey(block), PersistentDataType.INTEGER);
        return note == null ? 0 : Math.max(0, Math.min(24, note));
    }

    private void setNote(Block block, int note) {
        block.getChunk().getPersistentDataContainer().set(noteKey(block), PersistentDataType.INTEGER, note);
    }

    private void forgetNote(Block block) {
        block.getChunk().getPersistentDataContainer().remove(noteKey(block));
        poweredNotes.remove(spot(block));
    }

    /** Plays a plain note block's kept note with the instrument under it (only with air above, like the game). */
    private void playNote(Block block) {
        if (!block.getRelative(BlockFace.UP).getType().isAir()) {
            return;
        }
        int note = note(block);
        String instrument = instrumentUnder(block.getRelative(BlockFace.DOWN).getType().getKey().getKey());
        Location center = block.getLocation().add(0.5, 0.5, 0.5);
        float pitch = (float) Math.pow(2, (note - 12) / 12.0);
        block.getWorld().playSound(center, "minecraft:block.note_block." + instrument, SoundCategory.RECORDS, 3f, pitch);
        block.getWorld().spawnParticle(Particle.NOTE, center.clone().add(0, 0.7, 0), 0, note / 24.0, 0, 0, 1);
    }

    private static final String[] WOOD = {"_planks", "_log", "_wood", "_stem", "_hyphae", "bookshelf", "chest", "crafting_table", "note_block", "jukebox", "barrel", "lectern", "composter", "bamboo_block", "mushroom_block", "loom", "_table", "beehive"};
    private static final String[] STONE = {"stone", "cobble", "_ore", "netherrack", "obsidian", "quartz", "brick", "terracotta", "concrete", "deepslate", "basalt", "andesite", "diorite", "granite", "tuff", "calcite", "prismarine", "purpur", "nylium", "coral_block", "bedrock", "furnace", "dispenser", "dropper", "observer", "ender_chest"};

    /** The same table as `studio_format::block::instrument_under` (the Pumpkin add-on's). */
    static String instrumentUnder(String name) {
        switch (name) {
            case "clay", "honeycomb_block": return "flute";
            case "gold_block": return "bell";
            case "packed_ice": return "chime";
            case "bone_block": return "xylophone";
            case "iron_block": return "iron_xylophone";
            case "soul_sand": return "cow_bell";
            case "pumpkin", "carved_pumpkin", "jack_o_lantern": return "didgeridoo";
            case "emerald_block": return "bit";
            case "hay_block": return "banjo";
            case "glowstone": return "pling";
            case "gravel", "suspicious_gravel", "suspicious_sand": return "snare";
            case "sea_lantern", "beacon": return "hat";
            default: break;
        }
        if (name.endsWith("_wool")) {
            return "guitar";
        }
        if (name.equals("sand") || name.equals("red_sand") || name.endsWith("concrete_powder")) {
            return "snare";
        }
        if (name.contains("glass")) {
            return "hat";
        }
        if (!name.contains("ender_chest")) {
            for (String part : WOOD) {
                if (name.contains(part)) {
                    return "bass";
                }
            }
        }
        for (String part : STONE) {
            if (name.contains(part)) {
                return "basedrum";
            }
        }
        return "harp";
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onPistonPush(BlockPistonExtendEvent event) {
        for (Block one : event.getBlocks()) {
            if (at(one) != null) {
                event.setCancelled(true);
                return;
            }
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onPistonPull(BlockPistonRetractEvent event) {
        for (Block one : event.getBlocks()) {
            if (at(one) != null) {
                event.setCancelled(true);
                return;
            }
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onEntityExplode(EntityExplodeEvent event) {
        exploded(event.blockList());
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onBlockExplode(BlockExplodeEvent event) {
        exploded(event.blockList());
    }

    /** Ours go out of the explosion's list (it would drop note blocks) and drop their own item instead. */
    private void exploded(List<Block> list) {
        if (blocks.isEmpty()) {
            return;
        }
        list.removeIf(block -> {
            Placed one = at(block);
            if (one == null) {
                return false;
            }
            harvest(one, block, false);
            block.setType(Material.AIR, false);
            if (ownDrops(one)) {
                dropOwn(one, block);
            }
            return true;
        });
    }

    private static boolean ownDrops(Placed one) {
        return !Boolean.FALSE.equals(StudioText.object(one.data(), "drops").get("self"));
    }

    // ---------- breaking ----------

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onDamage(BlockDamageEvent event) {
        Placed one = at(event.getBlock());
        Player player = event.getPlayer();
        stopBreaking(player, false);
        if (one == null || player.getGameMode() == GameMode.CREATIVE) {
            return;
        }
        int ticks = ticksWith(one, event.getItemInHand());
        if (ticks <= 0) {
            event.setInstaBreak(true);
            return;
        }
        Breaking now = new Breaking(event.getBlock(), ticks);
        breaking.put(player.getUniqueId(), now);
        holdBreakSpeed(player, true);
        now.task = player.getScheduler().runAtFixedRate(plugin, task -> tickBreaking(player, now), () -> breaking.remove(player.getUniqueId(), now), 1L, 1L);
    }

    private void tickBreaking(Player player, Breaking now) {
        if (breaking.get(player.getUniqueId()) != now || at(now.block) == null || !player.isOnline()) {
            stopBreaking(player, true);
            return;
        }
        now.done++;
        float progress = Math.min(1f, now.done / (float) now.needed);
        cracks(now.block, progress);
        if (now.done >= now.needed) {
            stopBreaking(player, true);
            player.breakBlock(now.block);
        }
    }

    @EventHandler(priority = EventPriority.MONITOR)
    public void onDamageAbort(BlockDamageAbortEvent event) {
        stopBreaking(event.getPlayer(), true);
    }

    @EventHandler
    public void onQuit(PlayerQuitEvent event) {
        stopBreaking(event.getPlayer(), true);
    }

    private void stopBreaking(Player player, boolean clearCracks) {
        Breaking was = breaking.remove(player.getUniqueId());
        if (was == null) {
            return;
        }
        if (was.task != null) {
            was.task.cancel();
        }
        holdBreakSpeed(player, false);
        if (clearCracks) {
            cracks(was.block, 0);
        }
    }

    /** Cracks drawn for players nearby; 0 clears them. */
    private void cracks(Block block, float progress) {
        Location at = block.getLocation();
        int source = at.hashCode();
        for (Player one : block.getWorld().getPlayers()) {
            if (one.getLocation().distanceSquared(at) <= CRACK_DISTANCE * CRACK_DISTANCE) {
                one.sendBlockDamage(at, progress, source);
            }
        }
    }

    /** While breaking ours the player's own break speed is 0, so the client waits for our cracks. */
    private void holdBreakSpeed(Player player, boolean hold) {
        Attribute speed = breakSpeed();
        AttributeInstance instance = speed == null ? null : player.getAttribute(speed);
        if (instance == null) {
            // Before break speed was an attribute: mining fatigue holds it nearly still.
            if (hold) {
                player.addPotionEffect(new PotionEffect(PotionEffectType.MINING_FATIGUE, PotionEffect.INFINITE_DURATION, 255, false, false, false));
            } else {
                player.removePotionEffect(PotionEffectType.MINING_FATIGUE);
            }
            return;
        }
        instance.removeModifier(slow);
        if (hold) {
            instance.addTransientModifier(new AttributeModifier(slow, -1, AttributeModifier.Operation.MULTIPLY_SCALAR_1, EquipmentSlotGroup.ANY));
        }
    }

    private static Attribute breakSpeed() {
        for (String name : new String[] {"block_break_speed", "player.block_break_speed"}) {
            try {
                Attribute found = Registry.ATTRIBUTE.get(NamespacedKey.minecraft(name));
                if (found != null) {
                    return found;
                }
            } catch (RuntimeException | LinkageError missing) {
                // Try the older name.
            }
        }
        return null;
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onBreak(BlockBreakEvent event) {
        Placed one = at(event.getBlock());
        if (one == null) {
            return;
        }
        Player player = event.getPlayer();
        event.setDropItems(false);
        event.setExpToDrop(0);
        boolean creative = player.getGameMode() == GameMode.CREATIVE;
        ItemStack held = player.getInventory().getItemInMainHand();
        sound(one, event.getBlock(), false);
        if (!creative && harvestsWith(one, held)) {
            Map<String, Object> drops = StudioText.object(one.data(), "drops");
            if (ownDrops(one)) {
                dropOwn(one, event.getBlock());
            }
            int exp = StudioText.number(drops, "exp", 0, 0, 1000);
            if (exp > 0) {
                event.getBlock().getWorld().spawn(event.getBlock().getLocation().add(0.5, 0.5, 0.5), ExperienceOrb.class, orb -> orb.setExperience(exp));
            }
            String loot = StudioText.text(drops, "loot", "").trim();
            StudioLoot tables = StudioLoot.get();
            if (!loot.isEmpty() && tables != null) {
                // Like the game's own blocks: the roll drops where the block was, not into the player's hands.
                for (ItemStack stack : tables.roll(StudioLoot.clean(loot))) {
                    event.getBlock().getWorld().dropItemNaturally(event.getBlock().getLocation().add(0.5, 0.5, 0.5), stack);
                }
            }
            harvest(one, event.getBlock(), true);
        }
        run(one, "breakActions", player);
    }

    private void dropOwn(Placed one, Block block) {
        ItemStack stack = ownItem(one);
        if (stack != null) {
            block.getWorld().dropItemNaturally(block.getLocation().add(0.5, 0.5, 0.5), stack);
        }
    }

    // ---------- shared ----------

    /** The block's place or break sound, from the vanilla block it sounds like. */
    private static void sound(Placed one, Block block, boolean place) {
        Material like = Material.matchMaterial(StudioText.text(one.data(), "sounds", "plant".equals(one.shape()) ? "short_grass" : "stone"));
        if (like == null || !like.isBlock()) {
            like = "plant".equals(one.shape()) ? Material.SHORT_GRASS : Material.STONE;
        }
        try {
            SoundGroup group = like.createBlockData().getSoundGroup();
            block.getWorld().playSound(block.getLocation().add(0.5, 0.5, 0.5), place ? group.getPlaceSound() : group.getBreakSound(), group.getVolume(), group.getPitch() * 0.8f);
        } catch (RuntimeException | LinkageError quiet) {
            // No sound rather than a failed place.
        }
    }

    /** Runs an action list; false when the block has none. */
    private boolean run(Placed one, String key, Player player) {
        if (!(one.data().get(key) instanceof List<?> list) || list.isEmpty()) {
            return false;
        }
        actions.run(list, new StudioActions.Run(player, Map.of("block", one.id())));
        return true;
    }

    void stop() {
        for (Player player : Bukkit.getOnlinePlayers()) {
            try {
                stopBreaking(player, true);
            } catch (RuntimeException ignored) {
                // Shutting down.
            }
        }
        blocks = Map.of();
        byState = Map.of();
        byItem = Map.of();
        stageByState = Map.of();
        if (growing != null) {
            growing.cancel();
            growing = null;
        }
        crops.clear();
    }

    // ---------- crops ----------

    /** A crop's stage at a spot: 0 for its own state, 1.. for the later ones. */
    private int stageOf(Block block) {
        Integer stage = stageByState.get(block.getBlockData());
        return stage == null ? 0 : stage;
    }

    private static void setStage(Block block, Placed one, int stage) {
        block.setBlockData(stage <= 0 ? one.state() : one.stages().get(Math.min(stage, one.stages().size()) - 1), false);
    }

    /** A fully grown crop's harvest roll (and experience) where it stood. */
    private void harvest(Placed one, Block block, boolean exp) {
        Growth growth = growth(one.data());
        if (growth == null || stageOf(block) < growth.last()) {
            return;
        }
        Location center = block.getLocation().add(0.5, 0.5, 0.5);
        StudioLoot tables = StudioLoot.get();
        if (!growth.harvestLoot().isEmpty() && tables != null) {
            for (ItemStack stack : tables.roll(StudioLoot.clean(growth.harvestLoot()))) {
                block.getWorld().dropItemNaturally(center, stack);
            }
        }
        if (exp && growth.harvestExp() > 0) {
            block.getWorld().spawn(center, ExperienceOrb.class, orb -> orb.setExperience(growth.harvestExp()));
        }
    }

    /** Bone meal on one of our crops: one or two stages, the meal used up (not when it is already grown). */
    private void bonemeal(Player player, Block block, Placed one, Growth growth, ItemStack held) {
        int stage = stageOf(block);
        if (stage >= growth.last()) {
            return;
        }
        int bonemealStages = ThreadLocalRandom.current().nextDouble() < 0.5 ? 1 : 2;
        setStage(block, one, Math.min(growth.last(), stage + bonemealStages));
        if (player.getGameMode() != GameMode.CREATIVE) {
            held.setAmount(held.getAmount() - 1);
        }
        Location center = block.getLocation().add(0.5, 0.5, 0.5);
        block.getWorld().spawnParticle(Particle.HAPPY_VILLAGER, center, 12, 0.3, 0.3, 0.3, 0);
        block.getWorld().playSound(center, "minecraft:item.bone_meal.use", SoundCategory.BLOCKS, 1f, 1f);
    }

    /** A crop's spot in its chunk: x and z (4 bits each), then height above the world's floor. */
    private static int cropSpot(Block block) {
        return (block.getX() & 15) | (block.getZ() & 15) << 4 | (block.getY() - block.getWorld().getMinHeight()) << 8;
    }

    private static String chunkKey(World world, int x, int z) {
        return world.getUID() + ":" + x + ":" + z;
    }

    private void trackCrop(Block block) {
        Chunk chunk = block.getChunk();
        CropChunk entry = crops.computeIfAbsent(chunkKey(block.getWorld(), chunk.getX(), chunk.getZ()), key -> new CropChunk(block.getWorld(), chunk.getX(), chunk.getZ(), ConcurrentHashMap.newKeySet()));
        if (entry.spots().add(cropSpot(block))) {
            saveCrops(chunk, entry);
        }
    }

    private void saveCrops(Chunk chunk, CropChunk entry) {
        int[] packed = entry.spots().stream().mapToInt(Integer::intValue).toArray();
        if (packed.length == 0) {
            chunk.getPersistentDataContainer().remove(cropsKey);
            crops.remove(chunkKey(entry.world(), entry.x(), entry.z()), entry);
        } else {
            chunk.getPersistentDataContainer().set(cropsKey, PersistentDataType.INTEGER_ARRAY, packed);
        }
    }

    private void readCrops(Chunk chunk) {
        int[] packed = chunk.getPersistentDataContainer().get(cropsKey, PersistentDataType.INTEGER_ARRAY);
        if (packed == null || packed.length == 0) {
            return;
        }
        Set<Integer> spots = ConcurrentHashMap.newKeySet();
        for (int one : packed) {
            spots.add(one);
        }
        crops.put(chunkKey(chunk.getWorld(), chunk.getX(), chunk.getZ()), new CropChunk(chunk.getWorld(), chunk.getX(), chunk.getZ(), spots));
    }

    @EventHandler
    public void onChunkLoad(ChunkLoadEvent event) {
        readCrops(event.getChunk());
    }

    @EventHandler
    public void onChunkUnload(ChunkUnloadEvent event) {
        crops.remove(chunkKey(event.getWorld(), event.getChunk().getX(), event.getChunk().getZ()));
    }

    /** Once a second: every chunk with growing crops rolls them on its own region's thread. */
    private void growTick() {
        if (crops.isEmpty() || stageByState.isEmpty()) {
            return;
        }
        for (CropChunk entry : crops.values()) {
            try {
                Bukkit.getRegionScheduler().execute(plugin, entry.world(), entry.x(), entry.z(), () -> growChunk(entry));
            } catch (RuntimeException unloaded) {
                // The world went away; its chunks' unload events clear them.
            }
        }
    }

    private void growChunk(CropChunk entry) {
        World world = entry.world();
        if (!world.isChunkLoaded(entry.x(), entry.z())) {
            return;
        }
        Chunk chunk = world.getChunkAt(entry.x(), entry.z());
        boolean changed = false;
        ThreadLocalRandom random = ThreadLocalRandom.current();
        for (Integer packed : List.copyOf(entry.spots())) {
            Block block = world.getBlockAt(entry.x() * 16 + (packed & 15), world.getMinHeight() + (packed >>> 8), entry.z() * 16 + (packed >> 4 & 15));
            Placed one = at(block);
            Growth growth = one == null ? null : growth(one.data());
            int stage = one == null ? 0 : stageOf(block);
            if (growth == null || stage >= growth.last()) {
                changed |= entry.spots().remove(packed);
                continue;
            }
            if (random.nextDouble() >= growth.chance(1) || !canGrow(block, growth)) {
                continue;
            }
            setStage(block, one, stage + 1);
            if (stage + 1 >= growth.last()) {
                changed |= entry.spots().remove(packed);
            }
        }
        if (changed) {
            saveCrops(chunk, entry);
        }
    }

    private static boolean canGrow(Block block, Growth growth) {
        if (block.getLightLevel() < growth.light()) {
            return false;
        }
        Block below = block.getRelative(BlockFace.DOWN);
        if (!growth.soilOk(below.getType())) {
            return false;
        }
        return !growth.water() || below.getBlockData() instanceof Farmland farmland && farmland.getMoisture() > 0;
    }

    /** {@code studio/pack/ids.json}, the numbers the app's pack gave out, reread when it changes. */
    static final class PackIds {
        private final Path file;
        private volatile long modified = -1;
        private volatile Map<String, Object> given = Map.of();

        PackIds(Path file) {
            this.file = file;
        }

        /** The number {@code name} has in {@code kind}, or -1. */
        int number(String kind, String name) {
            refresh();
            return given.get(kind) instanceof Map<?, ?> numbers && numbers.get(name) instanceof Double number ? number.intValue() : -1;
        }

        @SuppressWarnings("unchecked")
        private void refresh() {
            try {
                long now = Files.exists(file) ? Files.getLastModifiedTime(file).toMillis() : 0;
                if (now == modified) {
                    return;
                }
                modified = now;
                Map<String, Object> next = Map.of();
                if (now != 0 && MiniJson.parse(Files.readString(file, StandardCharsets.UTF_8)) instanceof Map<?, ?> all && all.get("given") instanceof Map<?, ?> map) {
                    next = (Map<String, Object>) map;
                }
                given = next;
            } catch (java.io.IOException | RuntimeException unreadable) {
                given = Map.of();
            }
        }
    }
}

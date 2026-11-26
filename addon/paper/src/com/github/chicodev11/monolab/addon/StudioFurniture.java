package com.github.chicodev11.monolab.addon;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.function.BiConsumer;
import net.kyori.adventure.text.Component;
import org.bukkit.Bukkit;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Sound;
import org.bukkit.block.Block;
import org.bukkit.block.data.BlockData;
import org.bukkit.block.data.Levelled;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Interaction;
import org.bukkit.entity.ItemDisplay;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.block.Action;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.block.BlockPlaceEvent;
import org.bukkit.event.entity.EntityDismountEvent;
import org.bukkit.event.player.PlayerInteractEntityEvent;
import org.bukkit.event.player.PlayerInteractEvent;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.Plugin;
import org.bukkit.util.Transformation;
import org.joml.AxisAngle4f;
import org.joml.Vector3f;

/**
 * Studio maker M3: furniture ({@code furniture}). A placed piece is an item display (the look, from the app's pack:
 * {@code monolab:furniture/<id>}) and an interaction entity for its hitbox, both saved with the world and tagged with
 * the file name; a solid piece also fills its cells with barriers, a lit one with light blocks. Right click sits
 * (when it has a seat) and runs its use actions; hitting it breaks it. Turning and cell rules are the same as
 * {@code crates/studio-format/src/furniture.rs}.
 */
final class StudioFurniture implements Listener {
    static final String[] DISPLAYS = {"fixed", "none", "head", "ground"};
    static final int[] TURNS = {1, 4, 8};
    static final String[] ACTION_LISTS = {"useActions", "breakActions"};
    static final int MOST_CELLS = 27;
    static final int MOST_REACH = 4;

    private final Plugin plugin;
    private final Studio studio;
    private final StudioActions actions;
    private final BiConsumer<String, Throwable> warn;
    private final StudioBlocks.PackIds ids;
    private final boolean definitions;
    /** On both entities: the file name. On the display: its cells and its hitbox; on the hitbox: its display. */
    private final NamespacedKey tag;
    private final NamespacedKey cellsKey;
    private final NamespacedKey hitboxKey;
    private final NamespacedKey displayKey;
    private final NamespacedKey seatKey;
    /** When each player last placed a piece: one click can arrive as two interact events, and the second would put
     * another piece against the first one's barrier. */
    private final Map<UUID, Long> lastPlaced = new java.util.concurrent.ConcurrentHashMap<>();
    private volatile Map<String, Studio.Resource> pieces = Map.of();
    private volatile Map<String, Studio.Resource> byItem = Map.of();

    StudioFurniture(Plugin plugin, Studio studio, StudioActions actions, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.studio = studio;
        this.actions = actions;
        this.warn = warn;
        this.ids = new StudioBlocks.PackIds(plugin.getDataFolder().toPath().resolve("studio").resolve("pack").resolve("ids.json"));
        this.definitions = StudioItemParts.definitionsOn(Bukkit.getBukkitVersion());
        this.tag = new NamespacedKey(plugin, "furniture");
        this.cellsKey = new NamespacedKey(plugin, "furniture_cells");
        this.hitboxKey = new NamespacedKey(plugin, "furniture_hitbox");
        this.displayKey = new NamespacedKey(plugin, "furniture_display");
        this.seatKey = new NamespacedKey(plugin, "furniture_seat");
    }

    void register() {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "furniture";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, Studio.Resource> next = new HashMap<>();
                Map<String, Studio.Resource> items = new HashMap<>();
                for (Studio.Resource one : resources) {
                    String problem = problem(one.data());
                    if (problem != null) {
                        studio.problem("furniture", one.id(), problem);
                        continue;
                    }
                    next.put(one.id(), one);
                    String link = StudioText.text(one.data(), "item", "").trim();
                    if (!link.isEmpty()) {
                        items.put(link, one);
                    }
                    if (!definitions && ids.number("cmd", "furniture:" + one.id()) <= 0) {
                        studio.problem("furniture", one.id(), "Works, but it has no look yet: save it in the app so the resource pack is built.");
                    }
                }
                pieces = Map.copyOf(next);
                byItem = Map.copyOf(items);
            }
        });
    }

    // ---------- the shared rules ----------

    /** What is wrong with a furniture file, or null. */
    static String problem(Map<String, Object> data) {
        String look = StudioItemParts.problem(Map.of("look", data.getOrDefault("look", Map.of())));
        if (look != null) {
            return look;
        }
        if (!StudioItemParts.hasLook(data)) {
            return "Pick a texture or a model in \"look\".";
        }
        if (!List.of(DISPLAYS).contains(StudioText.text(data, "display", "fixed"))) {
            return "\"display\" must be one of: " + String.join(", ", DISPLAYS) + ".";
        }
        if (data.get("turns") != null && turns(data) != StudioText.number(data, "turns", 0, -100, 100)) {
            return "\"turns\" must be 1, 4 or 8.";
        }
        if (data.get("cells") instanceof List<?> list && list.size() > MOST_CELLS) {
            return "A piece of furniture takes up to " + MOST_CELLS + " cells.";
        }
        for (String key : ACTION_LISTS) {
            String wrong = StudioActions.problem(data.get(key), (kind, id) -> true);
            if (wrong != null) {
                return wrong;
            }
        }
        return null;
    }

    static int turns(Map<String, Object> data) {
        int one = StudioText.number(data, "turns", 4, 1, 8);
        for (int allowed : TURNS) {
            if (allowed == one) {
                return one;
            }
        }
        return 4;
    }

    /** Facing the player, snapped to {@code turns} ways (0 = south). Always in [0, 360). */
    static float snapYaw(double playerYaw, int turns) {
        double facing = ((playerYaw + 180) % 360 + 360) % 360;
        if (turns <= 1) {
            return 0;
        }
        double step = 360.0 / turns;
        return (float) (((Math.round(facing / step) * step) % 360 + 360) % 360);
    }

    /** Cells before turning: (x, y, z) each, at least the one placed on. */
    static List<int[]> cells(Map<String, Object> data) {
        List<int[]> out = new ArrayList<>();
        if (data.get("cells") instanceof List<?> list) {
            for (Object raw : list) {
                if (!(raw instanceof Map<?, ?> map)) {
                    continue;
                }
                Map<String, Object> one = StudioActions.map(map);
                int[] cell = {
                    StudioText.number(one, "x", 0, -MOST_REACH, MOST_REACH),
                    StudioText.number(one, "y", 0, -MOST_REACH, MOST_REACH),
                    StudioText.number(one, "z", 0, -MOST_REACH, MOST_REACH)
                };
                if (out.stream().noneMatch(seen -> java.util.Arrays.equals(seen, cell))) {
                    out.add(cell);
                }
                if (out.size() == MOST_CELLS) {
                    break;
                }
            }
        }
        if (out.isEmpty()) {
            out.add(new int[] {0, 0, 0});
        }
        return out;
    }

    /** A cell turned by the nearest quarter of {@code yaw}. */
    static int[] turnCell(int[] cell, double yaw) {
        int quarter = (int) Math.floorMod(Math.round((((yaw % 360) + 360) % 360) / 90.0), 4L);
        return switch (quarter) {
            case 1 -> new int[] {-cell[2], cell[1], cell[0]};
            case 2 -> new int[] {-cell[0], cell[1], -cell[2]};
            case 3 -> new int[] {cell[2], cell[1], -cell[0]};
            default -> cell.clone();
        };
    }

    static boolean solid(Map<String, Object> data) {
        return Boolean.TRUE.equals(data.get("solid"));
    }

    static int light(Map<String, Object> data) {
        return solid(data) ? 0 : StudioText.number(data, "light", 0, 0, 15);
    }

    // ---------- items ----------

    /** A piece's own item ({@code furniture/<id>}), for the {@code give} action. Null when there is none. */
    ItemStack stack(String ref) {
        if (!ref.startsWith("furniture/")) {
            return null;
        }
        Studio.Resource one = pieces.get(ref.substring(10));
        return one == null ? null : ownItem(one);
    }

    private ItemStack ownItem(Studio.Resource one) {
        String link = StudioText.text(one.data(), "item", "").trim();
        if (!link.isEmpty()) {
            return actions.stack("studio:" + link, 1);
        }
        ItemStack stack = look(one.id());
        ItemMeta meta = stack.getItemMeta();
        if (StudioText.has(one.data(), "name")) {
            meta.displayName(StudioBlocks.plain(StudioText.render(StudioText.text(one.data(), "name", ""), null)));
        }
        List<String> lore = StudioText.lines(one.data(), "lore");
        if (!lore.isEmpty()) {
            List<Component> lines = new ArrayList<>();
            for (String line : lore) {
                lines.add(StudioBlocks.plain(StudioText.render(line, null)));
            }
            meta.lore(lines);
        }
        meta.getPersistentDataContainer().set(tag, PersistentDataType.STRING, one.id());
        stack.setItemMeta(meta);
        return stack;
    }

    /** A flint showing the piece's look: what the display holds, and its own item's base. */
    private ItemStack look(String id) {
        ItemStack stack = new ItemStack(Material.FLINT);
        if (definitions) {
            try {
                StudioBlocks.Model.set(stack, "furniture/" + id);
            } catch (RuntimeException | LinkageError old) {
                // Shows as flint.
            }
        } else {
            int number = ids.number("cmd", "furniture:" + id);
            if (number > 0) {
                ItemMeta meta = stack.getItemMeta();
                StudioBlocks.setModel(meta, number);
                stack.setItemMeta(meta);
            }
        }
        return stack;
    }

    private Studio.Resource placedBy(ItemStack stack) {
        if (stack == null || !stack.hasItemMeta()) {
            return null;
        }
        String own = stack.getItemMeta().getPersistentDataContainer().get(tag, PersistentDataType.STRING);
        if (own != null) {
            return pieces.get(own);
        }
        String item = StudioItems.idOf(plugin, stack);
        return item == null ? null : byItem.get(item);
    }

    // ---------- placing ----------

    @EventHandler(priority = EventPriority.LOW)
    public void onPlace(PlayerInteractEvent event) {
        if (event.getAction() != Action.RIGHT_CLICK_BLOCK || event.getClickedBlock() == null || event.getHand() == null || pieces.isEmpty()) {
            return;
        }
        Studio.Resource one = placedBy(event.getItem());
        if (one == null) {
            return;
        }
        event.setCancelled(true);
        long now = System.currentTimeMillis();
        Long before = lastPlaced.put(event.getPlayer().getUniqueId(), now);
        if (before != null && now - before < 150) {
            return;
        }
        try {
            place(event.getPlayer(), one, event.getClickedBlock(), event.getBlockFace(), event.getItem(), event.getHand());
        } catch (RuntimeException error) {
            warn.accept("A studio furniture piece couldn't be placed: " + one.id(), error);
        }
    }

    private void place(Player player, Studio.Resource one, Block clicked, org.bukkit.block.BlockFace face, ItemStack item, EquipmentSlot hand) {
        Map<String, Object> data = one.data();
        Block base = clicked.isReplaceable() ? clicked : clicked.getRelative(face);
        float yaw = snapYaw(player.getLocation().getYaw(), turns(data));
        List<Block> taken = new ArrayList<>();
        for (int[] cell : cells(data)) {
            int[] turned = turnCell(cell, yaw);
            Block spot = base.getRelative(turned[0], turned[1], turned[2]);
            if (!spot.isReplaceable() || spot.getY() < spot.getWorld().getMinHeight() || spot.getY() >= spot.getWorld().getMaxHeight()) {
                return;
            }
            taken.add(spot);
        }
        BlockPlaceEvent check = new BlockPlaceEvent(base, base.getState(), clicked, item, player, true, hand);
        Bukkit.getPluginManager().callEvent(check);
        if (check.isCancelled() || !check.canBuild()) {
            return;
        }
        Location at = base.getLocation().add(0.5, 0, 0.5);
        at.setYaw(yaw);
        at.setPitch(0);
        StringBuilder cellText = new StringBuilder();
        for (Block spot : taken) {
            cellText.append(cellText.length() == 0 ? "" : ";").append(spot.getX()).append(',').append(spot.getY()).append(',').append(spot.getZ());
        }
        double lift = StudioItemParts.number(data, "y", 0.5, -4, 8);
        float scale = (float) StudioItemParts.number(data, "scale", 1, 0.05, 8);
        ItemDisplay display = base.getWorld().spawn(at.clone().add(0, lift, 0), ItemDisplay.class, entity -> {
            entity.setItemStack(look(one.id()));
            entity.setItemDisplayTransform(transform(StudioText.text(data, "display", "fixed")));
            entity.setTransformation(new Transformation(new Vector3f(), new AxisAngle4f(), new Vector3f(scale, scale, scale), new AxisAngle4f()));
            entity.setPersistent(true);
            PersistentDataContainer tags = entity.getPersistentDataContainer();
            tags.set(tag, PersistentDataType.STRING, one.id());
            tags.set(cellsKey, PersistentDataType.STRING, cellText.toString());
        });
        Map<String, Object> box = StudioText.object(data, "hitbox");
        // A solid piece's barriers fill the same space: a hair wider so clicks find the hitbox first.
        float grow = solid(data) ? 0.02f : 0;
        Interaction hitbox = base.getWorld().spawn(at, Interaction.class, entity -> {
            entity.setInteractionWidth((float) StudioItemParts.number(box, "width", 1, 0.1, 8) + grow);
            entity.setInteractionHeight((float) StudioItemParts.number(box, "height", 1, 0.1, 8) + grow);
            entity.setResponsive(true);
            entity.setPersistent(true);
            entity.getPersistentDataContainer().set(tag, PersistentDataType.STRING, one.id());
            entity.getPersistentDataContainer().set(displayKey, PersistentDataType.STRING, display.getUniqueId().toString());
        });
        display.getPersistentDataContainer().set(hitboxKey, PersistentDataType.STRING, hitbox.getUniqueId().toString());
        int light = light(data);
        for (Block spot : taken) {
            if (solid(data)) {
                spot.setType(Material.BARRIER, false);
            } else if (light > 0) {
                BlockData lit = Material.LIGHT.createBlockData();
                if (lit instanceof Levelled level) {
                    level.setLevel(light);
                }
                spot.setBlockData(lit, false);
            }
        }
        if (player.getGameMode() != GameMode.CREATIVE) {
            item.setAmount(item.getAmount() - 1);
        }
        player.swingHand(hand);
        base.getWorld().playSound(at, Sound.BLOCK_WOOD_PLACE, 1, 0.9f);
    }

    private static ItemDisplay.ItemDisplayTransform transform(String name) {
        return switch (name) {
            case "none" -> ItemDisplay.ItemDisplayTransform.NONE;
            case "head" -> ItemDisplay.ItemDisplayTransform.HEAD;
            case "ground" -> ItemDisplay.ItemDisplayTransform.GROUND;
            default -> ItemDisplay.ItemDisplayTransform.FIXED;
        };
    }

    // ---------- using and breaking ----------

    /** A middle click on a piece's display or hitbox: what breaking it gives, known by the piece it places. */
    StudioPicks.Pick pick(Entity entity) {
        ItemDisplay display = displayOf(entity);
        return display == null ? null : pickOf(display);
    }

    /** A middle click on a solid piece's barrier or a lit piece's light: the piece whose cell it is. */
    StudioPicks.Pick pick(Block block) {
        if (pieces.isEmpty() || block.getType() != Material.BARRIER && block.getType() != Material.LIGHT) {
            return null;
        }
        String cell = block.getX() + "," + block.getY() + "," + block.getZ();
        Location center = block.getLocation().add(0.5, 0.5, 0.5);
        for (Entity near : block.getWorld().getNearbyEntities(center, 8, 8, 8, one -> one instanceof ItemDisplay && one.getPersistentDataContainer().has(tag))) {
            String cells = near.getPersistentDataContainer().getOrDefault(cellsKey, PersistentDataType.STRING, "");
            if (List.of(cells.split(";")).contains(cell)) {
                return pickOf((ItemDisplay) near);
            }
        }
        return null;
    }

    private StudioPicks.Pick pickOf(ItemDisplay display) {
        String id = display.getPersistentDataContainer().get(tag, PersistentDataType.STRING);
        Studio.Resource one = id == null ? null : pieces.get(id);
        ItemStack item = one == null ? null : ownItem(one);
        if (item == null) {
            return null;
        }
        return new StudioPicks.Pick(item, stack -> {
            Studio.Resource placed = placedBy(stack);
            return placed != null && placed.id().equals(one.id());
        });
    }

    /** The display of a piece from either of its entities, or null when it isn't ours. */
    private ItemDisplay displayOf(Entity entity) {
        if (entity instanceof ItemDisplay display && display.getPersistentDataContainer().has(tag)) {
            return display;
        }
        if (entity instanceof Interaction hitbox) {
            String id = hitbox.getPersistentDataContainer().get(displayKey, PersistentDataType.STRING);
            if (id != null && Bukkit.getEntity(UUID.fromString(id)) instanceof ItemDisplay display) {
                return display;
            }
        }
        return null;
    }

    @EventHandler(priority = EventPriority.HIGH)
    public void onUse(PlayerInteractEntityEvent event) {
        if (!(event.getRightClicked() instanceof Interaction hitbox) || !hitbox.getPersistentDataContainer().has(tag) || event.getHand() != EquipmentSlot.HAND) {
            return;
        }
        event.setCancelled(true);
        Player player = event.getPlayer();
        String id = hitbox.getPersistentDataContainer().get(tag, PersistentDataType.STRING);
        Studio.Resource one = id == null ? null : pieces.get(id);
        if (one == null) {
            return;
        }
        if (one.data().get("seat") instanceof Map<?, ?> raw && player.getVehicle() == null) {
            double height = StudioItemParts.number(StudioActions.map(raw), "height", 0.5, -1, 4);
            Location seat = hitbox.getLocation().add(0, height, 0);
            ItemDisplay display = displayOf(hitbox);
            seat.setYaw(display != null ? display.getLocation().getYaw() + 180 : player.getLocation().getYaw());
            boolean taken = hitbox.getWorld().getNearbyEntities(seat, 0.3, 0.3, 0.3, entity -> entity.getPersistentDataContainer().has(seatKey) && !entity.getPassengers().isEmpty()).size() > 0;
            if (!taken) {
                ItemDisplay chair = hitbox.getWorld().spawn(seat, ItemDisplay.class, entity -> {
                    entity.setPersistent(false);
                    entity.getPersistentDataContainer().set(seatKey, PersistentDataType.BYTE, (byte) 1);
                });
                chair.addPassenger(player);
            }
        }
        if (one.data().get("useActions") instanceof List<?> list && !list.isEmpty()) {
            actions.run(list, new StudioActions.Run(player, Map.of("furniture", one.id())));
        }
    }

    @EventHandler
    public void onStand(EntityDismountEvent event) {
        Entity seat = event.getDismounted();
        if (seat.getPersistentDataContainer().has(seatKey)) {
            Location up = event.getEntity().getLocation().add(0, 0.6, 0);
            seat.remove();
            event.getEntity().getScheduler().run(plugin, task -> event.getEntity().teleportAsync(up), null);
        }
    }

    @EventHandler(priority = EventPriority.HIGH)
    public void onHit(io.papermc.paper.event.player.PrePlayerAttackEntityEvent event) {
        ItemDisplay display = displayOf(event.getAttacked());
        if (display == null) {
            return;
        }
        event.setCancelled(true);
        Player player = event.getPlayer();
        Block base = display.getLocation().getBlock();
        BlockBreakEvent check = new BlockBreakEvent(base, player);
        check.setDropItems(false);
        Bukkit.getPluginManager().callEvent(check);
        if (check.isCancelled()) {
            return;
        }
        String id = display.getPersistentDataContainer().get(tag, PersistentDataType.STRING);
        Studio.Resource one = id == null ? null : pieces.get(id);
        remove(display);
        display.getWorld().playSound(display.getLocation(), Sound.BLOCK_WOOD_BREAK, 1, 0.9f);
        if (one == null) {
            return;
        }
        if (player.getGameMode() != GameMode.CREATIVE && !Boolean.FALSE.equals(one.data().get("drops"))) {
            ItemStack drop = ownItem(one);
            if (drop != null) {
                display.getWorld().dropItemNaturally(display.getLocation(), drop);
            }
        }
        if (one.data().get("breakActions") instanceof List<?> list && !list.isEmpty()) {
            actions.run(list, new StudioActions.Run(player, Map.of("furniture", one.id())));
        }
    }

    /** Takes a piece out of the world: its two entities and what it put on its cells. */
    private void remove(ItemDisplay display) {
        PersistentDataContainer tags = display.getPersistentDataContainer();
        String hitbox = tags.get(hitboxKey, PersistentDataType.STRING);
        if (hitbox != null && Bukkit.getEntity(UUID.fromString(hitbox)) instanceof Interaction box) {
            box.remove();
        }
        String cellText = tags.getOrDefault(cellsKey, PersistentDataType.STRING, "");
        for (String one : cellText.split(";")) {
            String[] parts = one.split(",");
            if (parts.length != 3) {
                continue;
            }
            try {
                Block spot = display.getWorld().getBlockAt(Integer.parseInt(parts[0]), Integer.parseInt(parts[1]), Integer.parseInt(parts[2]));
                if (spot.getType() == Material.BARRIER || spot.getType() == Material.LIGHT) {
                    spot.setType(Material.AIR, false);
                }
            } catch (NumberFormatException skipped) {
                // A cell we didn't write.
            }
        }
        display.remove();
    }

    void stop() {
        pieces = Map.of();
        byItem = Map.of();
    }
}

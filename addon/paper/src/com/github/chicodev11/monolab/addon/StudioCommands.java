package com.github.chicodev11.monolab.addon;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.BiConsumer;
import java.util.regex.Pattern;
import net.kyori.adventure.text.Component;
import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.bukkit.command.Command;
import org.bukkit.command.CommandMap;
import org.bukkit.command.CommandSender;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.inventory.InventoryClickEvent;
import org.bukkit.event.inventory.InventoryDragEvent;
import org.bukkit.inventory.Inventory;
import org.bukkit.inventory.InventoryHolder;
import org.bukkit.inventory.ItemStack;
import org.bukkit.plugin.Plugin;

/**
 * Studio S3: custom commands ({@code command}) that run a list of actions, and chest menus
 * ({@code menu}) whose slots run actions when clicked. Commands are registered on the server's
 * command map under the {@code monolab:} prefix too, and removed again when their file goes.
 */
final class StudioCommands implements Listener {
    private static final Pattern NAME = Pattern.compile("[a-z0-9_-]{1,32}");
    private static final String PREFIX = "monolab";

    private final Plugin plugin;
    private final StudioActions actions;
    private final StudioItems items;
    private final Studio studio;
    private final BiConsumer<String, Throwable> warn;
    private final List<Command> registered = new ArrayList<>();
    private final Map<String, Long> cooldowns = new ConcurrentHashMap<>();
    private volatile Map<String, Studio.Resource> menus = Map.of();
    private volatile List<Studio.Resource> commands = List.of();

    StudioCommands(Plugin plugin, StudioActions actions, StudioItems items, Studio studio, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.actions = actions;
        this.items = items;
        this.studio = studio;
        this.warn = warn;
    }

    void register() {
        studio.add(feature("menu", list -> {
            Map<String, Studio.Resource> next = new HashMap<>();
            for (Studio.Resource one : list) {
                next.put(one.id(), one);
            }
            menus = next;
            // A menu's own command (its "command" field) goes with the command list.
            registerAll();
        }));
        studio.add(feature("command", list -> {
            commands = list;
            registerAll();
        }));
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

    // ---------- commands ----------

    private CommandMap map() {
        return Bukkit.getCommandMap();
    }

    private void unregisterAll() {
        CommandMap map = map();
        Map<String, Command> known = map.getKnownCommands();
        for (Command one : registered) {
            one.unregister(map);
            known.values().removeIf(existing -> existing == one);
        }
        registered.clear();
    }

    /** Puts every studio command (and every menu's own command) on the command map again. */
    private void registerAll() {
        unregisterAll();
        CommandMap map = map();
        for (Studio.Resource one : commands) {
            String name = StudioText.text(one.data(), "name", one.id()).toLowerCase(Locale.ROOT).trim();
            if (!NAME.matcher(name).matches()) {
                studio.problem("command", one.id(), "The command name may only use a-z, 0-9, - and _ (up to 32).");
                continue;
            }
            String problem = StudioActions.problem(one.data().get("actions"), studio::has);
            if (problem != null) {
                studio.problem("command", one.id(), problem);
                continue;
            }
            List<String> aliases = new ArrayList<>();
            for (String alias : StudioText.lines(one.data(), "aliases")) {
                String clean = alias.toLowerCase(Locale.ROOT).trim();
                if (NAME.matcher(clean).matches() && !clean.equals(name)) {
                    aliases.add(clean);
                }
            }
            Command command = new StudioCommand(name, aliases, one);
            if (!map.register(PREFIX, command)) {
                studio.problem("command", one.id(), "Another plugin already has /" + name + ", so it works as /monolab:" + name + ".");
            }
            registered.add(command);
        }
        for (Studio.Resource one : menus.values()) {
            String name = StudioText.text(one.data(), "command", "").toLowerCase(Locale.ROOT).trim();
            if (name.isEmpty()) {
                continue;
            }
            if (!NAME.matcher(name).matches()) {
                studio.problem("menu", one.id(), "The command name may only use a-z, 0-9, - and _ (up to 32).");
                continue;
            }
            Command command = new MenuCommand(name, one.id(), StudioText.text(one.data(), "permission", ""));
            if (!map.register(PREFIX, command)) {
                studio.problem("menu", one.id(), "Another plugin already has /" + name + ", so it works as /monolab:" + name + ".");
            }
            registered.add(command);
        }
        // Players' clients learn the new command list (for tab completion and red/white typing).
        for (Player player : Bukkit.getOnlinePlayers()) {
            player.getScheduler().run(plugin, task -> player.updateCommands(), null);
        }
    }

    private final class StudioCommand extends Command {
        private final Studio.Resource resource;

        StudioCommand(String name, List<String> aliases, Studio.Resource resource) {
            super(name, StudioText.text(resource.data(), "description", "A studio command."), "/" + name, aliases);
            this.resource = resource;
            String permission = StudioText.text(resource.data(), "permission", "");
            if (!permission.isBlank()) {
                setPermission(permission);
            }
        }

        @Override
        public boolean execute(CommandSender sender, String label, String[] args) {
            Map<String, Object> data = resource.data();
            String permission = StudioText.text(data, "permission", "");
            if (!permission.isBlank() && !sender.hasPermission(permission)) {
                String text = StudioText.text(data, "permissionMessage", "<red>You can't use this command.");
                sender.sendMessage(StudioText.render(text, sender instanceof Player player ? player : null));
                return true;
            }
            Player player = sender instanceof Player one ? one : null;
            if (player == null && !Boolean.TRUE.equals(data.get("console"))) {
                sender.sendMessage(Component.text("Only players can use /" + getName() + "."));
                return true;
            }
            int cooldown = StudioText.number(data, "cooldownSeconds", 0, 0, 24 * 60 * 60);
            if (player != null && cooldown > 0) {
                String key = resource.id() + "/" + player.getUniqueId();
                long now = System.currentTimeMillis();
                long until = cooldowns.getOrDefault(key, 0L);
                if (now < until) {
                    long left = (until - now + 999) / 1000;
                    String text = StudioText.text(data, "cooldownMessage", "<gray>Wait <seconds>s before using this again.");
                    sender.sendMessage(StudioText.render(text.replace("<seconds>", String.valueOf(left)), player));
                    return true;
                }
                cooldowns.put(key, now + cooldown * 1000L);
            }
            Map<String, String> extra = new HashMap<>();
            for (int at = 0; at < 9; at++) {
                extra.put("arg" + (at + 1), at < args.length ? args[at] : "");
            }
            extra.put("args", String.join(" ", args));
            extra.put("sender", sender.getName());
            actions.run(data.get("actions"), new StudioActions.Run(player, extra));
            return true;
        }
    }

    private final class MenuCommand extends Command {
        private final String menu;

        MenuCommand(String name, String menu, String permission) {
            super(name, "Opens the " + menu + " menu.", "/" + name, List.of());
            this.menu = menu;
            if (!permission.isBlank()) {
                setPermission(permission);
            }
        }

        @Override
        public boolean execute(CommandSender sender, String label, String[] args) {
            if (!(sender instanceof Player player)) {
                sender.sendMessage(Component.text("Only players can open menus."));
                return true;
            }
            open(player, menu);
            return true;
        }
    }

    // ---------- menus ----------

    /** Marks our inventories so clicks in them are ours to handle. */
    static final class MenuHolder implements InventoryHolder {
        final String menu;
        final UUID viewer;
        final Map<Integer, Map<String, Object>> slots = new HashMap<>();
        Inventory inventory;

        MenuHolder(String menu, UUID viewer) {
            this.menu = menu;
            this.viewer = viewer;
        }

        @Override
        public Inventory getInventory() {
            return inventory;
        }
    }

    /** Opens a menu for a player; runs on the player's thread. */
    void open(Player player, String id) {
        Studio.Resource one = menus.get(id);
        if (one == null) {
            player.sendMessage(Component.text("That menu doesn't exist (any more)."));
            return;
        }
        Map<String, Object> data = one.data();
        String permission = StudioText.text(data, "permission", "");
        if (!permission.isBlank() && !player.hasPermission(permission)) {
            player.sendMessage(StudioText.render("<red>You can't open this menu.", player));
            return;
        }
        int rows = StudioText.number(data, "rows", 3, 1, 6);
        MenuHolder holder = new MenuHolder(id, player.getUniqueId());
        String title = StudioGlyphs.menuTitle(StudioText.text(data, "background", ""), StudioText.text(data, "title", id));
        Inventory inventory = Bukkit.createInventory(holder, rows * 9, StudioText.render(title, player));
        holder.inventory = inventory;
        if (data.get("fill") instanceof String fill && !fill.isBlank()) {
            Material material = Material.matchMaterial(fill);
            if (material != null && material.isItem() && !material.isAir()) {
                ItemStack pane = new ItemStack(material);
                var meta = pane.getItemMeta();
                if (meta != null) {
                    meta.displayName(Component.text(" "));
                    pane.setItemMeta(meta);
                }
                for (int at = 0; at < rows * 9; at++) {
                    inventory.setItem(at, pane.clone());
                }
            }
        }
        if (data.get("slots") instanceof List<?> slots) {
            for (Object raw : slots) {
                if (!(raw instanceof Map<?, ?> map)) {
                    continue;
                }
                Map<String, Object> slot = StudioActions.map(map);
                int at = StudioText.number(slot, "slot", -1, -1, rows * 9 - 1);
                if (at < 0) {
                    continue;
                }
                ItemStack stack = slotItem(slot, player);
                if (stack != null) {
                    inventory.setItem(at, stack);
                }
                holder.slots.put(at, slot);
            }
        }
        player.openInventory(inventory);
    }

    private ItemStack slotItem(Map<String, Object> slot, Player player) {
        Object raw = slot.get("item");
        if (raw instanceof String name) {
            return actions.stack(name, StudioText.number(slot, "amount", 1, 1, 64));
        }
        if (raw instanceof Map<?, ?> map) {
            return items.build(null, StudioActions.map(map), player);
        }
        return null;
    }

    @EventHandler(priority = EventPriority.LOWEST)
    public void onClick(InventoryClickEvent event) {
        if (!(event.getView().getTopInventory().getHolder() instanceof MenuHolder holder)) {
            return;
        }
        event.setCancelled(true);
        if (!(event.getWhoClicked() instanceof Player player) || event.getClickedInventory() != event.getView().getTopInventory()) {
            return;
        }
        Map<String, Object> slot = holder.slots.get(event.getRawSlot());
        if (slot == null) {
            return;
        }
        try {
            if (actions.test(slot.get("if"), new StudioActions.Run(player, Map.of()))) {
                StudioActions.Run run = new StudioActions.Run(player, Map.of("menu", holder.menu));
                if (Boolean.TRUE.equals(slot.get("close"))) {
                    // Close first: closing after the actions would also close a dialog or menu they just opened.
                    player.getScheduler().run(plugin, task -> {
                        player.closeInventory();
                        actions.run(slot.get("actions"), run);
                    }, null);
                } else {
                    actions.run(slot.get("actions"), run);
                }
            }
        } catch (RuntimeException error) {
            warn.accept("A studio menu click failed in " + holder.menu, error);
        }
    }

    @EventHandler(priority = EventPriority.LOWEST)
    public void onDrag(InventoryDragEvent event) {
        if (event.getView().getTopInventory().getHolder() instanceof MenuHolder) {
            event.setCancelled(true);
        }
    }

    void stop() {
        try {
            unregisterAll();
        } catch (RuntimeException error) {
            warn.accept("Could not remove the studio commands", error);
        }
        for (Player player : Bukkit.getOnlinePlayers()) {
            if (player.getOpenInventory().getTopInventory().getHolder() instanceof MenuHolder) {
                try {
                    player.closeInventory();
                } catch (RuntimeException ignored) {
                    // Shutting down.
                }
            }
        }
    }
}

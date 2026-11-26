package com.github.chicodev11.monolab.addon;

import io.papermc.paper.event.player.AsyncChatEvent;
import java.time.LocalTime;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.function.BiConsumer;
import java.util.function.Consumer;
import net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer;
import org.bukkit.Bukkit;
import org.bukkit.Material;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Item;
import org.bukkit.entity.Player;
import org.bukkit.event.block.Action;
import org.bukkit.event.entity.EntityDamageByEntityEvent;
import org.bukkit.event.entity.EntityDamageEvent;
import org.bukkit.event.entity.EntityDeathEvent;
import org.bukkit.event.entity.EntityPickupItemEvent;
import org.bukkit.event.inventory.CraftItemEvent;
import org.bukkit.event.player.PlayerDropItemEvent;
import org.bukkit.event.player.PlayerFishEvent;
import org.bukkit.event.player.PlayerGameModeChangeEvent;
import org.bukkit.event.player.PlayerInteractEntityEvent;
import org.bukkit.event.player.PlayerInteractEvent;
import org.bukkit.event.player.PlayerItemConsumeEvent;
import org.bukkit.event.player.PlayerLevelChangeEvent;
import org.bukkit.event.player.PlayerTeleportEvent;
import org.bukkit.event.player.PlayerToggleSneakEvent;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.block.BlockPlaceEvent;
import org.bukkit.event.entity.PlayerDeathEvent;
import org.bukkit.event.player.PlayerChangedWorldEvent;
import org.bukkit.event.player.PlayerCommandPreprocessEvent;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.event.player.PlayerRespawnEvent;
import org.bukkit.plugin.Plugin;

/**
 * Studio S5: rules — "when (an event) → if (conditions) → then (actions), else (other actions)".
 * Every run is reported on the live line as a {@code trace}, so the app can show what happened.
 */
final class StudioRules implements Listener {
    static final String[] EVENTS = {
        "join", "first_join", "quit", "chat", "death", "respawn", "break", "place", "world_change", "command", "timer", "schedule",
        "click", "click_entity", "kill", "hit", "hurt", "eat", "craft", "fish", "sneak", "drop", "pickup", "level_up", "teleport",
        "gamemode", "server_start"
    };
    /** Traces sent per second at most; the rest are counted, not sent. */
    private static final int MOST_TRACES = 20;

    private final Plugin plugin;
    private final StudioActions actions;
    private final Studio studio;
    private final BiConsumer<String, Throwable> warn;
    private volatile Consumer<String> traces = message -> {};
    private volatile Map<String, List<Studio.Resource>> byEvent = Map.of();
    private final Map<String, Long> cooldowns = new ConcurrentHashMap<>();
    private final Map<String, Integer> timers = new HashMap<>();
    private final AtomicInteger sentThisSecond = new AtomicInteger();
    private String lastMinute = "";

    StudioRules(Plugin plugin, StudioActions actions, Studio studio, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.actions = actions;
        this.studio = studio;
        this.warn = warn;
    }

    void onTraces(Consumer<String> sink) {
        traces = sink;
    }

    void register() {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "rule";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, List<Studio.Resource>> next = new HashMap<>();
                for (Studio.Resource one : resources) {
                    String problem = problem(one.data());
                    if (problem != null) {
                        studio.problem("rule", one.id(), problem);
                        continue;
                    }
                    next.computeIfAbsent(event(one.data()), key -> new java.util.ArrayList<>()).add(one);
                }
                byEvent = next;
                timers.keySet().removeIf(id -> resources.stream().noneMatch(one -> one.id().equals(id)));
            }
        });
    }

    private static Map<String, Object> when(Map<String, Object> data) {
        return data.get("when") instanceof Map<?, ?> map ? StudioActions.map(map) : Map.of();
    }

    private static String event(Map<String, Object> data) {
        return StudioText.text(when(data), "event", "");
    }

    private String problem(Map<String, Object> data) {
        String event = event(data);
        // M6: "plugin:name" is an event another plugin fires through MonolabApi.studioTrigger.
        if (!java.util.Arrays.asList(EVENTS).contains(event) && !MonolabApi.studioName(event)) {
            return "\"when.event\" must be one of: " + String.join(", ", EVENTS) + ", or a plugin's event like shop:buy.";
        }
        if ("schedule".equals(event) && StudioText.lines(when(data), "at").stream().noneMatch(StudioRules::isTime)) {
            return "\"when.at\" needs times like 08:00.";
        }
        String then = StudioActions.problem(data.get("then"), studio::has);
        return then != null ? then : StudioActions.problem(data.get("else"), studio::has);
    }

    private static boolean isTime(String text) {
        return text.matches("([01]\\d|2[0-3]):[0-5]\\d");
    }

    // ---------- running ----------

    /** Runs every rule for {@code event} that matches; {@code extra} are the event's placeholders. */
    private void fire(String event, Player player, Map<String, String> extra, java.util.function.Predicate<Map<String, Object>> matches) {
        List<Studio.Resource> rules = byEvent.get(event);
        if (rules != null) {
            fire(rules, event, player, extra, matches);
        }
    }

    private void fire(List<Studio.Resource> rules, String event, Player player, Map<String, String> extra,
            java.util.function.Predicate<Map<String, Object>> matches) {
        for (Studio.Resource rule : rules) {
            try {
                Map<String, Object> data = rule.data();
                if (!matches.test(when(data))) {
                    continue;
                }
                int cooldown = StudioText.number(data, "cooldownSeconds", 0, 0, 24 * 60 * 60);
                if (cooldown > 0) {
                    String key = rule.id() + "/" + (player == null ? "-" : player.getUniqueId());
                    long now = System.currentTimeMillis();
                    if (now < cooldowns.getOrDefault(key, 0L)) {
                        continue;
                    }
                    cooldowns.put(key, now + cooldown * 1000L);
                }
                StudioActions.Run run = new StudioActions.Run(player, extra);
                boolean passed = actions.test(data.get("if"), run);
                List<String> ran = actions.run(passed ? data.get("then") : data.get("else"), run);
                trace(rule.id(), event, player, passed, ran);
            } catch (RuntimeException error) {
                warn.accept("The studio rule " + rule.id() + " failed", error);
                trace(rule.id(), event, player, false, List.of("error: " + error.getMessage()));
            }
        }
    }

    private void fire(String event, Player player, Map<String, String> extra) {
        fire(event, player, extra, matcher(event, extra));
    }

    /** Events that need no player: the timed ones, the server starting and other plugins' events. */
    static boolean playerless(String event) {
        return "timer".equals(event) || "schedule".equals(event) || "server_start".equals(event) || event.contains(":");
    }

    /** Runs the rules for an event a {@code cancel} step may stop; true when one did. Call on the event's thread. */
    private boolean fireCancellable(String event, Player player, Map<String, String> extra) {
        boolean[] cancel = {false};
        boolean[] before = StudioActions.CANCEL.get();
        StudioActions.CANCEL.set(cancel);
        try {
            fire(event, player, extra);
        } finally {
            StudioActions.CANCEL.set(before);
        }
        return cancel[0];
    }

    private boolean has(String event) {
        return byEvent.containsKey(event);
    }

    /** Fires {@code server_start} once, a few seconds after the add-on starts (rules are read by then). */
    void started() {
        Bukkit.getGlobalRegionScheduler().runDelayed(plugin, task -> fire("server_start", null, Map.of()), 100L);
    }

    /** A mob as rules name it: a studio mob's id, else the game's name ({@code zombie}, {@code player}). */
    static String entityName(Entity entity) {
        String own = StudioMobs.mobIdOf(entity);
        return own != null ? own : entity.getType().getKey().getKey();
    }

    /** An item as rules name it: {@code studio:<id>} for ours, else the game's name ({@code bread}). */
    private String itemName(ItemStack stack) {
        if (stack == null) {
            return "";
        }
        String own = StudioItems.idOf(plugin, stack);
        return own != null ? "studio:" + own : stack.getType().getKey().getKey();
    }

    private static String clean(String name) {
        String out = name.trim().toLowerCase(Locale.ROOT);
        return out.startsWith("minecraft:") ? out.substring(10) : out;
    }

    /** True when the list is empty or names the value (case and {@code minecraft:} don't matter). */
    private static boolean named(Map<String, Object> when, String key, String value) {
        List<String> wanted = StudioText.lines(when, key);
        wanted.removeIf(String::isBlank);
        return wanted.isEmpty() || wanted.stream().anyMatch(one -> clean(one).equals(clean(value)));
    }

    /** Whether a rule's {@code when} settings take an event with these placeholders (the same check the real events use). */
    static java.util.function.Predicate<Map<String, Object>> matcher(String event, Map<String, String> extra) {
        String value = switch (event) {
            case "chat" -> extra.getOrDefault("message", "");
            case "break", "place" -> extra.getOrDefault("block", "");
            case "world_change" -> extra.getOrDefault("world", "");
            case "command" -> extra.getOrDefault("command", "");
            case "click_entity", "kill", "hit" -> extra.getOrDefault("entity", "");
            case "eat", "craft", "fish", "drop", "pickup" -> extra.getOrDefault("item", "");
            case "gamemode" -> extra.getOrDefault("gamemode", "");
            default -> "";
        };
        return switch (event) {
            case "chat" -> when -> {
                String contains = StudioText.text(when, "contains", "");
                return contains.isBlank() || value.toLowerCase(Locale.ROOT).contains(contains.toLowerCase(Locale.ROOT));
            };
            case "break", "place" -> when -> listed(when, "blocks", value);
            case "world_change" -> when -> {
                String to = StudioText.text(when, "world", "");
                return to.isBlank() || to.equals(value);
            };
            case "command" -> when -> {
                String starts = StudioText.text(when, "command", "").trim();
                if (starts.startsWith("/")) {
                    starts = starts.substring(1);
                }
                String line = value.startsWith("/") ? value.substring(1) : value;
                return starts.isBlank() || line.toLowerCase(Locale.ROOT).startsWith(starts.toLowerCase(Locale.ROOT));
            };
            case "click" -> when -> {
                String button = StudioText.text(when, "button", "right");
                return ("any".equals(button) || button.equals(extra.getOrDefault("button", ""))) && named(when, "blocks", extra.getOrDefault("block", "air"));
            };
            case "click_entity", "kill", "hit" -> when -> named(when, "entities", value);
            case "eat", "craft", "fish", "drop", "pickup" -> when -> named(when, "items", value);
            case "gamemode" -> when -> {
                String mode = StudioText.text(when, "mode", "any");
                return "any".equals(mode) || mode.equals(value);
            };
            default -> when -> true;
        };
    }

    /**
     * The app's test tool: runs the rules for {@code event} (only {@code only} when it isn't blank) as if it happened to
     * {@code player} with these placeholders. Cooldowns are skipped and not started. Call it on the player's thread
     * (the global region's when {@code player} is null). Each run is traced as usual and also returned.
     */
    List<Map<String, Object>> test(String event, String only, Player player, Map<String, String> extra) {
        List<Map<String, Object>> out = new java.util.ArrayList<>();
        java.util.function.Predicate<Map<String, Object>> matches = matcher(event, extra);
        for (Studio.Resource rule : byEvent.getOrDefault(event, List.of())) {
            if (!only.isBlank() && !only.equals(rule.id())) {
                continue;
            }
            Map<String, Object> row = new HashMap<>();
            row.put("rule", rule.id());
            try {
                Map<String, Object> data = rule.data();
                if (!matches.test(when(data))) {
                    row.put("matched", false);
                    out.add(row);
                    continue;
                }
                StudioActions.Run run = new StudioActions.Run(player, extra);
                boolean passed = actions.test(data.get("if"), run);
                List<String> ran = actions.run(passed ? data.get("then") : data.get("else"), run);
                trace(rule.id(), event, player, passed, ran);
                row.put("matched", true);
                row.put("passed", passed);
                row.put("ran", ran);
            } catch (RuntimeException error) {
                warn.accept("The studio rule " + rule.id() + " failed in a test", error);
                row.put("matched", true);
                row.put("passed", false);
                row.put("ran", List.of("error: " + error.getMessage()));
            }
            out.add(row);
        }
        return out;
    }

    private void trace(String rule, String event, Player player, boolean passed, List<String> ran) {
        if (sentThisSecond.incrementAndGet() > MOST_TRACES) {
            return;
        }
        traces.accept(new Json()
                .put("type", "trace")
                .put("rule", rule)
                .put("event", event)
                .put("player", player == null ? "" : player.getName())
                .put("passed", passed)
                .put("ran", ran)
                .put("at", System.currentTimeMillis())
                .end());
    }

    private static boolean listed(Map<String, Object> when, String key, String value) {
        List<String> wanted = StudioText.lines(when, key);
        wanted.removeIf(String::isBlank);
        if (wanted.isEmpty()) {
            return true;
        }
        for (String one : wanted) {
            Material material = Material.matchMaterial(one);
            String name = material == null ? one.toLowerCase(Locale.ROOT) : material.name().toLowerCase(Locale.ROOT);
            if (name.equalsIgnoreCase(value)) {
                return true;
            }
        }
        return false;
    }

    /**
     * M6: a plugin fired {@code event} ("shop:buy") through MonolabApi.studioTrigger, from any thread. The rules run
     * on the player's thread (the server's when there is none or they left); {@code vars} are their placeholders.
     */
    void fireExternal(String event, UUID playerId, Map<String, String> vars) {
        if (!MonolabApi.studioName(event) || !byEvent.containsKey(event)) {
            return;
        }
        Map<String, String> extra = vars == null ? Map.of() : vars;
        Player player = playerId == null ? null : Bukkit.getPlayer(playerId);
        if (player != null) {
            player.getScheduler().run(plugin, task -> fire(event, player, extra), null);
        } else {
            Bukkit.getGlobalRegionScheduler().execute(plugin, () -> fire(event, null, extra));
        }
    }

    /** Called once a second on the global region thread: timers and schedules. */
    void second() {
        sentThisSecond.set(0);
        List<Studio.Resource> timed = byEvent.get("timer");
        if (timed != null) {
            for (Studio.Resource rule : timed) {
                int every = StudioText.number(when(rule.data()), "everySeconds", 60, 1, 24 * 60 * 60);
                int count = timers.merge(rule.id(), 1, Integer::sum);
                if (count < every) {
                    continue;
                }
                timers.put(rule.id(), 0);
                runTimed(rule, "timer");
            }
        }
        String minute = LocalTime.now().withSecond(0).withNano(0).toString();
        if (!minute.equals(lastMinute)) {
            lastMinute = minute;
            List<Studio.Resource> scheduled = byEvent.get("schedule");
            if (scheduled != null) {
                for (Studio.Resource rule : scheduled) {
                    if (StudioText.lines(when(rule.data()), "at").contains(minute)) {
                        runTimed(rule, "schedule");
                    }
                }
            }
        }
    }

    /** A timed rule runs once for the server, or once per online player when {@code when.perPlayer} is on. */
    private void runTimed(Studio.Resource rule, String event) {
        List<Studio.Resource> one = List.of(rule);
        if (Boolean.TRUE.equals(when(rule.data()).get("perPlayer"))) {
            for (Player player : Bukkit.getOnlinePlayers()) {
                player.getScheduler().run(plugin, task -> fire(one, event, player, Map.of(), when -> true), null);
            }
        } else {
            fire(one, event, null, Map.of(), when -> true);
        }
    }

    // ---------- events ----------

    @EventHandler(priority = EventPriority.MONITOR)
    public void onJoin(PlayerJoinEvent event) {
        Player player = event.getPlayer();
        boolean first = !player.hasPlayedBefore();
        player.getScheduler().runDelayed(plugin, task -> {
            if (first) {
                fire("first_join", player, Map.of());
            }
            fire("join", player, Map.of());
        }, null, 5L);
    }

    @EventHandler(priority = EventPriority.MONITOR)
    public void onQuit(PlayerQuitEvent event) {
        fire("quit", event.getPlayer(), Map.of());
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onChat(AsyncChatEvent event) {
        Player player = event.getPlayer();
        String message = PlainTextComponentSerializer.plainText().serialize(event.message());
        // Chat arrives off the server thread; rules run on the player's.
        player.getScheduler().run(plugin, task -> fire("chat", player, Map.of("message", message)), null);
    }

    @EventHandler(priority = EventPriority.MONITOR)
    public void onDeath(PlayerDeathEvent event) {
        Player player = event.getEntity();
        Player killer = player.getKiller();
        fire("death", player, Map.of("killer", killer == null ? "" : killer.getName()));
    }

    @EventHandler(priority = EventPriority.MONITOR)
    public void onRespawn(PlayerRespawnEvent event) {
        Player player = event.getPlayer();
        player.getScheduler().runDelayed(plugin, task -> fire("respawn", player, Map.of()), null, 2L);
    }

    private static Map<String, String> at(String key, String value, org.bukkit.block.Block block) {
        return Map.of(key, value, "x", String.valueOf(block.getX()), "y", String.valueOf(block.getY()), "z", String.valueOf(block.getZ()));
    }

    // Events a rule may cancel run at HIGH, so the cancel lands before MONITOR listeners see it.
    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onBreak(BlockBreakEvent event) {
        if (!has("break")) {
            return;
        }
        String block = event.getBlock().getType().name().toLowerCase(Locale.ROOT);
        if (fireCancellable("break", event.getPlayer(), at("block", block, event.getBlock()))) {
            event.setCancelled(true);
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onPlace(BlockPlaceEvent event) {
        if (!has("place")) {
            return;
        }
        String block = event.getBlockPlaced().getType().name().toLowerCase(Locale.ROOT);
        if (fireCancellable("place", event.getPlayer(), at("block", block, event.getBlockPlaced()))) {
            event.setCancelled(true);
        }
    }

    /** Clicks on air come in already cancelled, so this one listens to cancelled events too. */
    @EventHandler(priority = EventPriority.HIGH)
    public void onClick(PlayerInteractEvent event) {
        if (!has("click") || event.getHand() != EquipmentSlot.HAND || event.getAction() == Action.PHYSICAL) {
            return;
        }
        String button = event.getAction().isRightClick() ? "right" : "left";
        org.bukkit.block.Block block = event.getClickedBlock();
        Map<String, String> extra = block == null
                ? Map.of("button", button, "block", "air", "x", "", "y", "", "z", "")
                : Map.of("button", button, "block", block.getType().getKey().getKey(), "x", String.valueOf(block.getX()), "y", String.valueOf(block.getY()), "z", String.valueOf(block.getZ()));
        if (fireCancellable("click", event.getPlayer(), extra)) {
            event.setUseInteractedBlock(org.bukkit.event.Event.Result.DENY);
            event.setUseItemInHand(org.bukkit.event.Event.Result.DENY);
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onClickEntity(PlayerInteractEntityEvent event) {
        if (!has("click_entity") || event.getHand() != EquipmentSlot.HAND) {
            return;
        }
        Entity entity = event.getRightClicked();
        String name = entity.customName() != null ? PlainTextComponentSerializer.plainText().serialize(entity.customName()) : entity.getName();
        if (fireCancellable("click_entity", event.getPlayer(), Map.of("entity", entityName(entity), "entity_name", name))) {
            event.setCancelled(true);
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onHit(EntityDamageByEntityEvent event) {
        if (!has("hit") || !(event.getDamager() instanceof Player player) || StudioActions.bySkill()) {
            return;
        }
        if (fireCancellable("hit", player, Map.of("entity", entityName(event.getEntity()), "amount", amount(event.getFinalDamage())))) {
            event.setCancelled(true);
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onHurt(EntityDamageEvent event) {
        if (!has("hurt") || !(event.getEntity() instanceof Player player)) {
            return;
        }
        String attacker = event instanceof EntityDamageByEntityEvent by ? entityName(by.getDamager()) : "";
        String cause = event.getCause().name().toLowerCase(Locale.ROOT);
        if (fireCancellable("hurt", player, Map.of("cause", cause, "amount", amount(event.getFinalDamage()), "attacker", attacker))) {
            event.setCancelled(true);
        }
    }

    private static String amount(double value) {
        return value == Math.rint(value) ? String.valueOf((long) value) : String.format(Locale.ROOT, "%.1f", value);
    }

    @EventHandler(priority = EventPriority.MONITOR)
    public void onKill(EntityDeathEvent event) {
        Player killer = event.getEntity().getKiller();
        if (!has("kill") || killer == null) {
            return;
        }
        String entity = entityName(event.getEntity());
        killer.getScheduler().run(plugin, task -> fire("kill", killer, Map.of("entity", entity)), null);
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onEat(PlayerItemConsumeEvent event) {
        if (has("eat") && fireCancellable("eat", event.getPlayer(), Map.of("item", itemName(event.getItem()), "amount", "1"))) {
            event.setCancelled(true);
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onCraft(CraftItemEvent event) {
        if (!has("craft") || !(event.getWhoClicked() instanceof Player player)) {
            return;
        }
        ItemStack result = event.getRecipe().getResult();
        if (fireCancellable("craft", player, Map.of("item", itemName(result), "amount", String.valueOf(result.getAmount())))) {
            event.setCancelled(true);
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onFish(PlayerFishEvent event) {
        if (!has("fish") || event.getState() != PlayerFishEvent.State.CAUGHT_FISH || !(event.getCaught() instanceof Item caught)) {
            return;
        }
        ItemStack stack = caught.getItemStack();
        if (fireCancellable("fish", event.getPlayer(), Map.of("item", itemName(stack), "amount", String.valueOf(stack.getAmount())))) {
            event.setCancelled(true);
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onSneak(PlayerToggleSneakEvent event) {
        if (has("sneak") && event.isSneaking()) {
            fire("sneak", event.getPlayer(), Map.of());
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onDrop(PlayerDropItemEvent event) {
        if (!has("drop")) {
            return;
        }
        ItemStack stack = event.getItemDrop().getItemStack();
        if (fireCancellable("drop", event.getPlayer(), Map.of("item", itemName(stack), "amount", String.valueOf(stack.getAmount())))) {
            event.setCancelled(true);
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onPickup(EntityPickupItemEvent event) {
        if (!has("pickup") || !(event.getEntity() instanceof Player player)) {
            return;
        }
        ItemStack stack = event.getItem().getItemStack();
        if (fireCancellable("pickup", player, Map.of("item", itemName(stack), "amount", String.valueOf(stack.getAmount())))) {
            event.setCancelled(true);
        }
    }

    @EventHandler(priority = EventPriority.MONITOR)
    public void onLevel(PlayerLevelChangeEvent event) {
        if (has("level_up") && event.getNewLevel() > event.getOldLevel()) {
            fire("level_up", event.getPlayer(), Map.of("level", String.valueOf(event.getNewLevel())));
        }
    }

    /** Teleports by plugins (ours too) don't count, so a rule that teleports can't set itself off. */
    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onTeleport(PlayerTeleportEvent event) {
        if (!has("teleport") || event.getCause() == PlayerTeleportEvent.TeleportCause.PLUGIN || event.getCause() == PlayerTeleportEvent.TeleportCause.UNKNOWN) {
            return;
        }
        org.bukkit.Location to = event.getTo();
        Map<String, String> extra = Map.of("world", to.getWorld() == null ? "" : to.getWorld().getName(), "x", String.valueOf(to.getBlockX()), "y", String.valueOf(to.getBlockY()), "z", String.valueOf(to.getBlockZ()));
        if (fireCancellable("teleport", event.getPlayer(), extra)) {
            event.setCancelled(true);
        }
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onGameMode(PlayerGameModeChangeEvent event) {
        if (has("gamemode") && fireCancellable("gamemode", event.getPlayer(), Map.of("gamemode", event.getNewGameMode().name().toLowerCase(Locale.ROOT)))) {
            event.setCancelled(true);
        }
    }

    @EventHandler(priority = EventPriority.MONITOR)
    public void onWorld(PlayerChangedWorldEvent event) {
        String world = event.getPlayer().getWorld().getName();
        fire("world_change", event.getPlayer(), Map.of("world", world));
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onCommand(PlayerCommandPreprocessEvent event) {
        if (!has("command")) {
            return;
        }
        String line = event.getMessage().startsWith("/") ? event.getMessage().substring(1) : event.getMessage();
        if (fireCancellable("command", event.getPlayer(), Map.of("command", line))) {
            event.setCancelled(true);
        }
    }
}

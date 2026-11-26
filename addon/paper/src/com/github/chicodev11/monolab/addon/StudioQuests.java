package com.github.chicodev11.monolab.addon;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.Function;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.NamespacedKey;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Item;
import org.bukkit.entity.LivingEntity;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.block.BlockBreakEvent;
import org.bukkit.event.block.BlockPlaceEvent;
import org.bukkit.event.entity.EntityDeathEvent;
import org.bukkit.event.entity.EntityPickupItemEvent;
import org.bukkit.event.inventory.CraftItemEvent;
import org.bukkit.event.player.PlayerFishEvent;
import org.bukkit.event.player.PlayerInteractEntityEvent;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.Plugin;

/**
 * Quests ({@code quest} files, same rules as {@code studio_format::quest} on Pumpkin): steps in order (kill, collect,
 * break, place, craft, fish, talk to a studio NPC, go to a spot), then {@code rewards} and an optional studio
 * advancement. The {@code quest} action starts, abandons or completes one; the {@code quest} condition checks a
 * player's state with it. Progress is kept on the player ({@code monolab:quests}) as JSON.
 */
final class StudioQuests implements Listener {
    static final String[] STEP_TYPES = {"kill", "collect", "break", "place", "craft", "fish", "talk", "visit"};
    private static final int MOST_DEPTH_STEPS = 32;

    private final Plugin plugin;
    private final StudioActions actions;
    private final Studio studio;
    private final NamespacedKey key;
    private final Function<Entity, String> mobId;
    private final Function<Entity, String> npcId;
    private volatile Map<String, Map<String, Object>> quests = Map.of();
    private final Map<UUID, Long> lastTalk = new ConcurrentHashMap<>();
    private ScheduledTask task;

    private record Step(String kind, List<String> targets, int amount, String text, String world, double x, double y, double z, double radius) {}

    /** quest id → (step, count); and the done ones. */
    private record Progress(Map<String, int[]> active, Set<String> done) {}

    StudioQuests(Plugin plugin, StudioActions actions, Studio studio, Function<Entity, String> mobId, Function<Entity, String> npcId) {
        this.plugin = plugin;
        this.actions = actions;
        this.studio = studio;
        this.key = new NamespacedKey(plugin, "quests");
        this.mobId = mobId;
        this.npcId = npcId;
    }

    void register() {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "quest";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, Map<String, Object>> next = new HashMap<>();
                for (Studio.Resource one : resources) {
                    String problem = problem(one.data());
                    if (problem != null) {
                        studio.problem("quest", one.id(), problem);
                    }
                    next.put(one.id(), one.data());
                }
                quests = Map.copyOf(next);
            }
        });
        actions.action("quest", (step, run) -> {
            Player player = run.player();
            if (player != null) {
                player.getScheduler().run(plugin, ignored -> act(player, StudioText.text(step, "quest", "").trim(), StudioText.text(step, "mode", "start"), run), null);
            }
        });
        actions.condition("quest", (step, run) -> run.player() != null
                && state(read(run.player()), StudioText.text(step, "quest", "").trim()).equals(StudioText.text(step, "state", "active")));
        task = Bukkit.getGlobalRegionScheduler().runAtFixedRate(plugin, ignored -> visits(), 20L, 20L);
    }

    void stop() {
        if (task != null) {
            task.cancel();
        }
    }

    // ---------- rules (studio_format::quest) ----------

    private static String clean(String text) {
        String out = text.trim().toLowerCase(Locale.ROOT);
        return out.startsWith("minecraft:") ? out.substring(10) : out;
    }

    private static double decimal(Map<String, Object> data, String name, double fallback, double least, double most) {
        Object value = data.get(name);
        double read = value instanceof Number number ? number.doubleValue() : fallback;
        if (value instanceof String text) {
            try {
                read = Double.parseDouble(text.trim());
            } catch (NumberFormatException ignored) {
                read = fallback;
            }
        }
        return Double.isFinite(read) ? Math.max(least, Math.min(most, read)) : fallback;
    }

    static List<Step> steps(Map<String, Object> data) {
        List<Step> out = new ArrayList<>();
        if (!(data.get("steps") instanceof List<?> list)) {
            return out;
        }
        for (Object raw : list) {
            if (out.size() >= MOST_DEPTH_STEPS) {
                break;
            }
            if (!(raw instanceof Map<?, ?> map)) {
                continue;
            }
            Map<String, Object> one = StudioActions.map(map);
            String kind = StudioText.text(one, "type", "kill");
            if (!List.of(STEP_TYPES).contains(kind)) {
                kind = "kill";
            }
            List<String> targets = new ArrayList<>();
            for (String part : StudioText.text(one, "target", "").split(",")) {
                if (!clean(part).isEmpty()) {
                    targets.add(clean(part));
                }
            }
            int amount = kind.equals("talk") || kind.equals("visit") ? 1 : StudioText.number(one, "amount", 1, 1, 100_000);
            out.add(new Step(kind, targets, amount, StudioText.text(one, "text", "").trim(), StudioText.text(one, "world", "").trim(),
                    decimal(one, "x", 0, -3.0e7, 3.0e7), decimal(one, "y", 64, -2048, 4096), decimal(one, "z", 0, -3.0e7, 3.0e7), decimal(one, "radius", 5, 1, 512)));
        }
        return out;
    }

    static String stepText(Step step) {
        if (!step.text().isEmpty()) {
            return step.text();
        }
        String what = step.targets().isEmpty() ? "anything" : String.join(" or ", step.targets()).replace('_', ' ');
        return switch (step.kind()) {
            case "kill" -> "Kill " + step.amount() + " " + what;
            case "collect" -> "Collect " + step.amount() + " " + what;
            case "break" -> "Break " + step.amount() + " " + what;
            case "place" -> "Place " + step.amount() + " " + what;
            case "craft" -> "Craft " + step.amount() + " " + what;
            case "fish" -> "Catch " + step.amount() + " " + what;
            case "talk" -> "Talk to " + what;
            default -> String.format(Locale.ROOT, "Go to %.0f %.0f %.0f", step.x(), step.y(), step.z());
        };
    }

    static String name(Map<String, Object> data, String id) {
        String name = StudioText.text(data, "name", "").trim();
        return name.isEmpty() ? id.replace('-', ' ').replace('_', ' ') : name;
    }

    static String problem(Map<String, Object> data) {
        List<Step> list = steps(data);
        if (list.isEmpty()) {
            return "A quest needs \"steps\": what players do, in order.";
        }
        for (int at = 0; at < list.size(); at++) {
            if (list.get(at).kind().equals("talk") && list.get(at).targets().isEmpty()) {
                return "Step " + (at + 1) + " needs the NPC to talk to (\"target\").";
            }
        }
        return null;
    }

    private static boolean repeatable(Map<String, Object> data) {
        return Boolean.TRUE.equals(data.get("repeatable"));
    }

    private Progress read(Player player) {
        Progress out = new Progress(new LinkedHashMap<>(), new LinkedHashSet<>());
        String text = player.getPersistentDataContainer().get(key, PersistentDataType.STRING);
        if (text == null || text.isBlank()) {
            return out;
        }
        try {
            JsonObject root = JsonParser.parseString(text).getAsJsonObject();
            if (root.has("active") && root.get("active").isJsonObject()) {
                for (Map.Entry<String, JsonElement> one : root.getAsJsonObject("active").entrySet()) {
                    JsonArray at = one.getValue().getAsJsonArray();
                    out.active().put(one.getKey(), new int[] {at.get(0).getAsInt(), at.get(1).getAsInt()});
                }
            }
            if (root.has("done") && root.get("done").isJsonArray()) {
                root.getAsJsonArray("done").forEach(one -> out.done().add(one.getAsString()));
            }
        } catch (RuntimeException ignored) {
            // A broken record starts over.
        }
        return out;
    }

    private void write(Player player, Progress progress) {
        JsonObject root = new JsonObject();
        JsonObject active = new JsonObject();
        progress.active().forEach((id, at) -> {
            JsonArray pair = new JsonArray();
            pair.add(at[0]);
            pair.add(at[1]);
            active.add(id, pair);
        });
        JsonArray done = new JsonArray();
        progress.done().forEach(done::add);
        root.add("active", active);
        root.add("done", done);
        player.getPersistentDataContainer().set(key, PersistentDataType.STRING, root.toString());
    }

    private static String state(Progress progress, String id) {
        return progress.active().containsKey(id) ? "active" : progress.done().contains(id) ? "done" : "new";
    }

    // ---------- what happens ----------

    private interface Fits {
        boolean test(Step step);
    }

    /** Moves the player's active quests on; on the player's own thread. */
    private void advance(Player player, Fits fits, int amount) {
        if (quests.isEmpty()) {
            return;
        }
        Progress progress = read(player);
        if (progress.active().isEmpty()) {
            return;
        }
        boolean changed = false;
        for (String id : new ArrayList<>(progress.active().keySet())) {
            Map<String, Object> data = quests.get(id);
            if (data == null) {
                continue;
            }
            List<Step> list = steps(data);
            int[] at = progress.active().get(id);
            StudioActions.Run run = new StudioActions.Run(player, Map.of("quest", id, "quest_name", name(data, id)));
            if (at[0] >= list.size()) {
                finish(progress, id);
                changed = true;
                finished(data, run);
                continue;
            }
            Step step = list.get(at[0]);
            if (!fits.test(step)) {
                continue;
            }
            changed = true;
            int count = Math.min(step.amount(), at[1] + Math.max(1, amount));
            String quest = "<gold>" + name(data, id) + "</gold>";
            if (count < step.amount()) {
                progress.active().put(id, new int[] {at[0], count});
                actions.run(List.of(Map.of("type", "actionbar", "text", quest + " <gray>·</gray> " + stepText(step) + " <gray>(" + count + "/" + step.amount() + ")")), run);
            } else if (at[0] + 1 < list.size()) {
                progress.active().put(id, new int[] {at[0] + 1, 0});
                actions.run(List.of(
                        Map.of("type", "message", "text", quest + " <gray>›</gray> " + stepText(list.get(at[0] + 1))),
                        Map.of("type", "sound", "sound", "entity.experience_orb.pickup", "volume", 1.0, "pitch", 1.2)), run);
            } else {
                finish(progress, id);
                finished(data, run);
            }
        }
        if (changed) {
            write(player, progress);
        }
    }

    private static void finish(Progress progress, String id) {
        progress.active().remove(id);
        progress.done().add(id);
    }

    private void finished(Map<String, Object> data, StudioActions.Run run) {
        actions.run(List.of(
                Map.of("type", "title", "title", "<gold>Quest complete", "subtitle", run.extra().getOrDefault("quest_name", ""), "stayTicks", 50.0),
                Map.of("type", "sound", "sound", "ui.toast.challenge_complete", "volume", 1.0, "pitch", 1.0)), run);
        actions.run(data.get("rewards"), run);
        String advancement = StudioText.text(data, "advancement", "").trim();
        if (!advancement.isEmpty()) {
            actions.run(List.of(Map.of("type", "advancement", "advancement", advancement)), run);
        }
    }

    private void act(Player player, String id, String mode, StudioActions.Run base) {
        Map<String, Object> data = quests.get(id);
        if (data == null) {
            return;
        }
        Progress progress = read(player);
        StudioActions.Run run = base.with("quest", id).with("quest_name", name(data, id));
        switch (mode) {
            case "abandon" -> {
                if (progress.active().remove(id) != null) {
                    write(player, progress);
                }
            }
            case "complete" -> {
                if (state(progress, id).equals("done") && !repeatable(data)) {
                    return;
                }
                finish(progress, id);
                write(player, progress);
                finished(data, run);
            }
            default -> {
                List<Step> list = steps(data);
                if (progress.active().containsKey(id) || (progress.done().contains(id) && !repeatable(data)) || list.isEmpty()) {
                    return;
                }
                progress.active().put(id, new int[] {0, 0});
                write(player, progress);
                actions.run(List.of(Map.of("type", "message", "text", "<gold>" + name(data, id) + "</gold> <gray>›</gray> " + stepText(list.get(0)))), run);
                actions.run(data.get("startActions"), run);
            }
        }
    }

    private void count(Player player, String kind, String target, int amount) {
        String clean = clean(target);
        player.getScheduler().run(plugin, ignored -> advance(player, step -> step.kind().equals(kind) && (step.targets().isEmpty() || step.targets().contains(clean)), amount), null);
    }

    private void visits() {
        if (quests.isEmpty()) {
            return;
        }
        for (Player player : Bukkit.getOnlinePlayers()) {
            player.getScheduler().run(plugin, ignored -> {
                Location at = player.getLocation();
                String world = at.getWorld().getName();
                advance(player, step -> step.kind().equals("visit")
                        && (step.world().isEmpty() || step.world().equalsIgnoreCase(world))
                        && Math.sqrt(Math.pow(step.x() - at.getX(), 2) + Math.pow(step.y() - at.getY(), 2) + Math.pow(step.z() - at.getZ(), 2)) <= step.radius(), 1);
            }, null);
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onKill(EntityDeathEvent event) {
        LivingEntity dead = event.getEntity();
        Player killer = dead.getKiller();
        if (killer == null || quests.isEmpty()) {
            return;
        }
        String studioMob = mobId.apply(dead);
        if (studioMob != null) {
            count(killer, "kill", "studio:" + studioMob, 1);
        }
        count(killer, "kill", dead.getType().getKey().getKey(), 1);
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onBreak(BlockBreakEvent event) {
        if (!quests.isEmpty()) {
            count(event.getPlayer(), "break", event.getBlock().getType().getKey().getKey(), 1);
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onPlace(BlockPlaceEvent event) {
        if (!quests.isEmpty()) {
            count(event.getPlayer(), "place", event.getBlockPlaced().getType().getKey().getKey(), 1);
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onPickup(EntityPickupItemEvent event) {
        if (!quests.isEmpty() && event.getEntity() instanceof Player player) {
            count(player, "collect", event.getItem().getItemStack().getType().getKey().getKey(), event.getItem().getItemStack().getAmount());
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onCraft(CraftItemEvent event) {
        if (!quests.isEmpty() && event.getWhoClicked() instanceof Player player) {
            int made = event.getRecipe().getResult().getAmount();
            count(player, "craft", event.getRecipe().getResult().getType().getKey().getKey(), made);
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onFish(PlayerFishEvent event) {
        if (!quests.isEmpty() && event.getState() == PlayerFishEvent.State.CAUGHT_FISH && event.getCaught() instanceof Item item) {
            count(event.getPlayer(), "fish", item.getItemStack().getType().getKey().getKey(), 1);
        }
    }

    /** Talking to a studio NPC; their own click handler cancels the event, so this listens to cancelled ones too. */
    @EventHandler(priority = EventPriority.MONITOR)
    public void onTalk(PlayerInteractEntityEvent event) {
        if (quests.isEmpty() || event.getHand() != EquipmentSlot.HAND) {
            return;
        }
        String id = npcId.apply(event.getRightClicked());
        if (id == null) {
            return;
        }
        // A click can come as two events (plain and "at"); count it once.
        long now = System.currentTimeMillis();
        Long last = lastTalk.put(event.getPlayer().getUniqueId(), now);
        if (last != null && now - last < 250) {
            return;
        }
        count(event.getPlayer(), "talk", id, 1);
    }
}

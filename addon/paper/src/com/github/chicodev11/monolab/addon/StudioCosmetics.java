package com.github.chicodev11.monolab.addon;

import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.TreeMap;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import org.bukkit.Bukkit;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.NamespacedKey;
import org.bukkit.Particle;
import org.bukkit.entity.Entity;
import org.bukkit.entity.ItemDisplay;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.inventory.ItemStack;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.Plugin;
import org.bukkit.potion.PotionEffectType;
import org.bukkit.util.Transformation;
import org.joml.Quaternionf;
import org.joml.Vector3f;

/**
 * Studio cosmetics ({@code cosmetic}): hats and back pieces (an item display riding the player, turned with the body)
 * and trails (a particle where they walk). What a player wears is {@code slot=id;...} in their {@code
 * monolab:cosmetics}. Same rules as {@code crates/studio-format/src/cosmetic.rs}.
 */
final class StudioCosmetics implements Listener {
    static final String[] SLOTS = {"hat", "back", "trail"};
    private static final long EVERY_TICKS = 2;
    private static volatile StudioCosmetics current;

    private final Plugin plugin;
    private final StudioActions actions;
    private final Studio studio;
    private final NamespacedKey worn;
    private final NamespacedKey mark;
    private volatile Map<String, Map<String, Object>> cosmetics = Map.of();
    /** Displays shown per player and slot, with the cosmetic id each shows. */
    private final Map<UUID, Map<String, ItemDisplay>> shown = new ConcurrentHashMap<>();
    private final Map<UUID, Location> last = new ConcurrentHashMap<>();
    private ScheduledTask task;

    StudioCosmetics(Plugin plugin, StudioActions actions, Studio studio) {
        this.plugin = plugin;
        this.actions = actions;
        this.studio = studio;
        this.worn = new NamespacedKey(plugin, "cosmetics");
        this.mark = new NamespacedKey(plugin, "cosmetic");
    }

    static StudioCosmetics get() {
        return current;
    }

    void register() {
        current = this;
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "cosmetic";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, Map<String, Object>> next = new HashMap<>();
                for (Studio.Resource one : resources) {
                    String problem = problem(one.data());
                    if (problem != null) {
                        studio.problem("cosmetic", one.id(), problem);
                        continue;
                    }
                    next.put(one.id(), one.data());
                }
                cosmetics = Map.copyOf(next);
                // Made again on the next update with the new look.
                for (Player player : Bukkit.getOnlinePlayers()) {
                    actions.onPlayer(player, () -> clear(player.getUniqueId()));
                }
            }
        });
        task = Bukkit.getGlobalRegionScheduler().runAtFixedRate(plugin, scheduled -> {
            for (Player player : Bukkit.getOnlinePlayers()) {
                actions.onPlayer(player, () -> update(player));
            }
        }, EVERY_TICKS, EVERY_TICKS);
    }

    void stop() {
        if (task != null) {
            task.cancel();
        }
        for (UUID id : List.copyOf(shown.keySet())) {
            clear(id);
        }
        current = null;
    }

    // ---------- the shared rules ----------

    static String slot(Map<String, Object> data) {
        String raw = StudioText.text(data, "slot", "hat").trim();
        return List.of(SLOTS).contains(raw) ? raw : "hat";
    }

    static String problem(Map<String, Object> data) {
        String raw = StudioText.text(data, "slot", "");
        if (!raw.isEmpty() && !List.of(SLOTS).contains(raw)) {
            return "\"slot\" must be one of: " + String.join(", ", SLOTS) + ".";
        }
        if ("trail".equals(slot(data))) {
            String name = particleName(data);
            if (name.isEmpty()) {
                return "A trail needs a \"particle\" (flame, heart, end_rod...).";
            }
            if (StudioActions.particle(name) == null) {
                return "\"particle\" isn't a particle this server can show alone: " + name + ".";
            }
        } else if (StudioText.text(data, "item", "").isBlank()) {
            return "A hat or back piece needs an \"item\": studio:<id> or a vanilla item.";
        }
        return null;
    }

    private static String particleName(Map<String, Object> data) {
        String name = StudioText.text(data, "particle", "").trim().toLowerCase(Locale.ROOT);
        return name.startsWith("minecraft:") ? name.substring(10) : name;
    }

    static Map<String, String> parse(String text) {
        Map<String, String> out = new TreeMap<>();
        for (String one : text == null ? new String[0] : text.split(";")) {
            int at = one.indexOf('=');
            if (at > 0) {
                String slot = one.substring(0, at);
                String id = one.substring(at + 1);
                if (List.of(SLOTS).contains(slot) && id.matches("[a-z0-9_-]{1,48}")) {
                    out.put(slot, id);
                }
            }
        }
        return out;
    }

    static String write(Map<String, String> list) {
        List<String> parts = new ArrayList<>();
        list.forEach((slot, id) -> parts.add(slot + "=" + id));
        return String.join(";", parts);
    }

    // ---------- wearing ----------

    /** The {@code cosmetic} action: {@code mode} is toggle (default), wear or remove. On the player's thread. */
    void act(Player player, String id, String mode) {
        Map<String, Object> data = cosmetics.get(id);
        if (data == null) {
            return;
        }
        String slot = slot(data);
        Map<String, String> list = parse(player.getPersistentDataContainer().get(worn, PersistentDataType.STRING));
        boolean wearing = id.equals(list.get(slot));
        boolean wear = switch (mode) {
            case "wear" -> true;
            case "remove" -> false;
            default -> !wearing;
        };
        String permission = StudioText.text(data, "permission", "").trim();
        if (wear && !permission.isEmpty() && !player.hasPermission(permission)) {
            return;
        }
        if (wear) {
            list.put(slot, id);
        } else if (wearing) {
            list.remove(slot);
        } else {
            return;
        }
        if (list.isEmpty()) {
            player.getPersistentDataContainer().remove(worn);
        } else {
            player.getPersistentDataContainer().set(worn, PersistentDataType.STRING, write(list));
        }
        update(player);
    }

    private void update(Player player) {
        UUID uuid = player.getUniqueId();
        Map<String, String> list = parse(player.getPersistentDataContainer().get(worn, PersistentDataType.STRING));
        boolean hidden = player.getGameMode() == GameMode.SPECTATOR || player.hasPotionEffect(PotionEffectType.INVISIBILITY) || player.isDead();
        Map<String, ItemDisplay> mine = shown.computeIfAbsent(uuid, key -> new ConcurrentHashMap<>());
        for (String slot : new String[] {"hat", "back"}) {
            String id = list.get(slot);
            Map<String, Object> data = id == null ? null : cosmetics.get(id);
            boolean wears = data != null && slot.equals(slot(data)) && allowed(player, data) && !hidden;
            ItemDisplay display = mine.get(slot);
            if (!wears) {
                if (display != null) {
                    mine.remove(slot);
                    display.remove();
                }
                continue;
            }
            boolean good = display != null && display.isValid() && player.equals(display.getVehicle())
                    && id.equals(display.getPersistentDataContainer().get(mark, PersistentDataType.STRING));
            if (!good) {
                if (display != null) {
                    display.remove();
                }
                display = spawn(player, id, data);
                if (display == null) {
                    mine.remove(slot);
                    continue;
                }
                mine.put(slot, display);
            }
            display.setRotation(player.getBodyYaw(), 0);
        }
        String trail = list.get("trail");
        Map<String, Object> data = trail == null ? null : cosmetics.get(trail);
        Location now = player.getLocation();
        Location before = last.put(uuid, now);
        if (data != null && "trail".equals(slot(data)) && allowed(player, data) && !hidden && before != null
                && before.getWorld() == now.getWorld() && before.distanceSquared(now) > 0.0025 && !player.isFlying()) {
            Particle particle = StudioActions.particle(particleName(data));
            if (particle != null) {
                now.getWorld().spawnParticle(particle, now.clone().add(0, 0.1, 0), StudioText.number(data, "count", 2, 1, 20), 0.15, 0.05, 0.15, 0);
            }
        }
    }

    private static boolean allowed(Player player, Map<String, Object> data) {
        String permission = StudioText.text(data, "permission", "").trim();
        return permission.isEmpty() || player.hasPermission(permission);
    }

    private ItemDisplay spawn(Player player, String id, Map<String, Object> data) {
        ItemStack item = actions.stack(StudioText.text(data, "item", "").trim(), 1);
        if (item == null) {
            return null;
        }
        float scale = (float) StudioItemParts.number(data, "scale", 1, 0.1, 4);
        float up = (float) StudioItemParts.number(data, "up", 0, -2, 2);
        float back = (float) StudioItemParts.number(data, "back", 0, -2, 2);
        boolean onBack = "back".equals(slot(data));
        Vector3f translation = onBack ? new Vector3f(0, -0.75f + up, -0.3f - back) : new Vector3f(0, -0.2f + up, -back);
        ItemDisplay display = player.getWorld().spawn(player.getLocation(), ItemDisplay.class, made -> {
            made.setPersistent(false);
            made.setItemStack(item);
            made.setItemDisplayTransform(ItemDisplay.ItemDisplayTransform.HEAD);
            made.setTransformation(new Transformation(translation, new Quaternionf(), new Vector3f(scale, scale, scale), new Quaternionf()));
            made.setTeleportDuration((int) EVERY_TICKS);
            made.getPersistentDataContainer().set(mark, PersistentDataType.STRING, id);
        });
        if (!player.addPassenger(display)) {
            display.remove();
            return null;
        }
        return display;
    }

    private void clear(UUID uuid) {
        Map<String, ItemDisplay> mine = shown.remove(uuid);
        if (mine != null) {
            for (Entity one : mine.values()) {
                one.remove();
            }
        }
    }

    @EventHandler(priority = EventPriority.MONITOR)
    public void onQuit(PlayerQuitEvent event) {
        clear(event.getPlayer().getUniqueId());
        last.remove(event.getPlayer().getUniqueId());
    }
}

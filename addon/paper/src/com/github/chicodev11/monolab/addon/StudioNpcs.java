package com.github.chicodev11.monolab.addon;

import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.BiConsumer;
import net.kyori.adventure.text.Component;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.World;
import org.bukkit.entity.Ageable;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.LivingEntity;
import org.bukkit.entity.Player;
import org.bukkit.entity.Villager;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.entity.CreatureSpawnEvent;
import org.bukkit.event.entity.EntityDamageEvent;
import org.bukkit.event.player.PlayerInteractAtEntityEvent;
import org.bukkit.event.player.PlayerInteractEntityEvent;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.Plugin;

/**
 * Studio NPCs: a mob that stands at a spot, has a name over its head and runs actions when a player right-clicks it.
 * Like holograms, the entity is never saved into the world, so a crash or a removed file leaves nothing behind;
 * {@link #tick} puts it back while its chunk is loaded. It can't be hurt, doesn't move and turns to the nearest player.
 */
final class StudioNpcs implements Listener {
    static final long TICK_EVERY = 5L;
    /** How far an NPC looks for a player to turn to. */
    private static final double LOOK_DISTANCE = 8;

    private final Plugin plugin;
    private final Studio studio;
    private final StudioActions actions;
    private final BiConsumer<String, Throwable> warn;
    private final NamespacedKey key;
    private volatile Map<String, Studio.Resource> npcs = Map.of();
    private final Map<String, LivingEntity> shown = new ConcurrentHashMap<>();
    private final Map<String, String> reported = new ConcurrentHashMap<>();
    private final Map<String, Long> cooldowns = new ConcurrentHashMap<>();
    private long ticks;

    StudioNpcs(Plugin plugin, Studio studio, StudioActions actions, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.studio = studio;
        this.actions = actions;
        this.warn = warn;
        this.key = new NamespacedKey(plugin, "npc");
    }

    void register() {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "npc";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, Studio.Resource> next = new HashMap<>();
                for (Studio.Resource one : resources) {
                    next.put(one.id(), one);
                }
                npcs = Map.copyOf(next);
                reported.clear();
                for (String id : List.copyOf(shown.keySet())) {
                    take(id);
                }
                ticks = 0;
                tick();
            }
        });
    }

    /** Every 5 ticks on the global region thread: once a second spawn what is missing, every time turn heads. */
    void tick() {
        boolean spawn = ticks++ % 4 == 0;
        for (Studio.Resource one : npcs.values()) {
            Map<String, Object> data = one.data();
            Location at = place(one.id(), data);
            if (at == null || !at.getWorld().isChunkLoaded(at.getBlockX() >> 4, at.getBlockZ() >> 4)) {
                continue;
            }
            LivingEntity entity = shown.get(one.id());
            boolean missing = entity == null || !entity.isValid();
            if (missing && !spawn) {
                continue;
            }
            Bukkit.getRegionScheduler().execute(plugin, at, () -> {
                try {
                    if (missing) {
                        spawn(one.id(), data, at);
                    } else if (!Boolean.FALSE.equals(data.get("lookAtPlayers"))) {
                        look(entity, at);
                    }
                } catch (RuntimeException error) {
                    warn.accept("A studio NPC failed: " + one.id(), error);
                }
            });
        }
    }

    private Location place(String id, Map<String, Object> data) {
        String name = StudioText.text(data, "world", "world");
        World world = Bukkit.getWorld(name);
        if (world == null) {
            report(id, "There is no world named \"" + name + "\".");
            return null;
        }
        if (!(data.get("x") instanceof Double x) || !(data.get("y") instanceof Double y) || !(data.get("z") instanceof Double z)) {
            report(id, "\"x\", \"y\" and \"z\" are needed.");
            return null;
        }
        float yaw = data.get("yaw") instanceof Double turn ? turn.floatValue() : 0f;
        return new Location(world, x, y, z, yaw, 0f);
    }

    /** The entity type by its Minecraft name; only living ones that can stand somewhere. */
    static EntityType type(String name) {
        String clean = name.trim().toLowerCase(Locale.ROOT);
        NamespacedKey id = NamespacedKey.fromString(clean.isEmpty() ? "villager" : clean);
        EntityType type = id == null ? null : Registry.ENTITY_TYPE.get(id);
        if (type == null || !type.isSpawnable() || !type.isAlive() || type == EntityType.PLAYER || type == EntityType.ENDER_DRAGON || type == EntityType.WITHER) {
            return null;
        }
        return type;
    }

    /** On the region thread of {@code at}. */
    private void spawn(String id, Map<String, Object> data, Location at) {
        EntityType type = type(StudioText.text(data, "entity", "villager"));
        if (type == null) {
            report(id, "\"" + StudioText.text(data, "entity", "villager") + "\" isn't a mob an NPC can be.");
            return;
        }
        String name = StudioText.text(data, "name", "");
        Component rendered = name.isBlank() ? null : StudioText.render(name, null);
        Entity made = at.getWorld().spawnEntity(at, type, CreatureSpawnEvent.SpawnReason.CUSTOM, entity -> {
            entity.setPersistent(false);
            entity.getPersistentDataContainer().set(key, PersistentDataType.STRING, id);
            entity.setInvulnerable(true);
            entity.setSilent(true);
            entity.setGravity(false);
            if (rendered != null) {
                entity.customName(rendered);
                entity.setCustomNameVisible(true);
            }
            entity.setGlowing(Boolean.TRUE.equals(data.get("glow")));
            if (entity instanceof LivingEntity living) {
                living.setAI(false);
                living.setCollidable(false);
                living.setRemoveWhenFarAway(false);
                living.setCanPickupItems(false);
            }
            if (entity instanceof Ageable young) {
                if (Boolean.TRUE.equals(data.get("baby"))) {
                    young.setBaby();
                } else {
                    young.setAdult();
                }
            }
            if (entity instanceof Villager villager) {
                Villager.Profession profession = profession(StudioText.text(data, "profession", ""));
                if (profession != null) {
                    villager.setProfession(profession);
                }
            }
        });
        if (made instanceof LivingEntity living) {
            shown.put(id, living);
        } else {
            made.remove();
        }
    }

    private static Villager.Profession profession(String name) {
        NamespacedKey id = NamespacedKey.fromString(name.trim().toLowerCase(Locale.ROOT));
        return id == null || name.isBlank() ? null : Registry.VILLAGER_PROFESSION.get(id);
    }

    /** Turns to the nearest player close by; back to its own facing when nobody is. On the entity's region thread. */
    private static void look(LivingEntity entity, Location home) {
        Player nearest = null;
        double best = LOOK_DISTANCE * LOOK_DISTANCE;
        for (Player player : entity.getWorld().getPlayers()) {
            double distance = player.getLocation().distanceSquared(entity.getLocation());
            if (distance < best) {
                best = distance;
                nearest = player;
            }
        }
        if (nearest == null) {
            entity.setRotation(home.getYaw(), 0f);
            return;
        }
        // Its AI is off, so set the rotation itself (the look controller only runs with AI).
        Location eyes = entity.getEyeLocation();
        Location face = eyes.clone().setDirection(nearest.getEyeLocation().toVector().subtract(eyes.toVector()));
        entity.setRotation(face.getYaw(), face.getPitch());
    }

    /** The studio id of an NPC entity, or null. */
    String idOf(Entity entity) {
        return entity.getPersistentDataContainer().get(key, PersistentDataType.STRING);
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onInteract(PlayerInteractEntityEvent event) {
        clicked(event);
    }

    /**
     * Older clients send a plain and an "at" interact for one click, 26.x only the "at" one: whichever comes first
     * runs, the other one of the same click is only cancelled.
     */
    private void clicked(PlayerInteractEntityEvent event) {
        String id = idOf(event.getRightClicked());
        if (id == null) {
            return;
        }
        // Never trade, ride or dye: an NPC only runs its actions, once per click (not for the off hand too).
        event.setCancelled(true);
        if (event.getHand() != EquipmentSlot.HAND || !firstOfClick(event.getPlayer(), event.getRightClicked())) {
            return;
        }
        Studio.Resource one = npcs.get(id);
        if (one == null) {
            return;
        }
        Player player = event.getPlayer();
        Map<String, Object> data = one.data();
        StudioActions.Run run = new StudioActions.Run(player, Map.of("npc", id));
        if (!actions.test(data.get("if"), run)) {
            actions.run(data.get("deny"), run);
            return;
        }
        int cooldown = StudioText.number(data, "cooldownSeconds", 0, 0, 24 * 60 * 60);
        if (cooldown > 0) {
            String key = id + "/" + player.getUniqueId();
            long now = System.currentTimeMillis();
            if (cooldowns.getOrDefault(key, 0L) > now) {
                return;
            }
            cooldowns.put(key, now + cooldown * 1000L);
        }
        actions.run(data.get("actions"), run);
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onAtInteract(PlayerInteractAtEntityEvent event) {
        clicked(event);
    }

    private static final Map<String, Long> LAST_CLICK = new ConcurrentHashMap<>();

    /** True for the first interact event of a click on {@code entity} (both kinds arrive in the same tick). */
    static boolean firstOfClick(Player player, Entity entity) {
        long now = System.currentTimeMillis();
        Long before = LAST_CLICK.put(player.getUniqueId() + "/" + entity.getUniqueId(), now);
        if (LAST_CLICK.size() > 512) {
            LAST_CLICK.entrySet().removeIf(one -> now - one.getValue() > 1000);
        }
        return before == null || now - before > 100;
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onDamage(EntityDamageEvent event) {
        if (idOf(event.getEntity()) != null) {
            event.setCancelled(true);
        }
    }

    private void report(String id, String problem) {
        if (!problem.equals(reported.put(id, problem))) {
            studio.problem("npc", id, problem);
        }
    }

    private void take(String id) {
        LivingEntity entity = shown.remove(id);
        if (entity != null && entity.isValid()) {
            entity.getScheduler().run(plugin, task -> entity.remove(), null);
        }
    }

    /** When the plugin turns off: nothing can be scheduled any more, so this runs right here. */
    void stop() {
        npcs = Map.of();
        for (LivingEntity entity : shown.values()) {
            try {
                if (entity.isValid()) {
                    entity.remove();
                }
            } catch (RuntimeException otherThread) {
                // Folia: not this region's entity. It isn't saved, so it goes with its chunk.
            }
        }
        shown.clear();
        reported.clear();
    }
}

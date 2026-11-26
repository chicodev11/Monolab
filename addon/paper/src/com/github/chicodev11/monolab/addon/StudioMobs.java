package com.github.chicodev11.monolab.addon;

import com.destroystokyo.paper.entity.ai.Goal;
import com.destroystokyo.paper.entity.ai.GoalType;
import com.destroystokyo.paper.entity.ai.MobGoals;
import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ThreadLocalRandom;
import java.util.function.BiConsumer;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer;
import org.bukkit.Bukkit;
import org.bukkit.GameMode;
import org.bukkit.Location;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.World;
import org.bukkit.attribute.Attribute;
import org.bukkit.attribute.AttributeInstance;
import org.bukkit.block.Block;
import org.bukkit.block.BlockFace;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.LivingEntity;
import org.bukkit.entity.Mob;
import org.bukkit.entity.Player;
import org.bukkit.entity.Projectile;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.entity.CreatureSpawnEvent;
import org.bukkit.event.entity.EntityCombustEvent;
import org.bukkit.event.entity.EntityDamageByEntityEvent;
import org.bukkit.event.entity.EntityDamageEvent;
import org.bukkit.event.entity.EntityDeathEvent;
import org.bukkit.event.entity.EntityPickupItemEvent;
import org.bukkit.event.entity.EntityTargetLivingEntityEvent;
import org.bukkit.event.player.PlayerInteractAtEntityEvent;
import org.bukkit.event.player.PlayerInteractEntityEvent;
import org.bukkit.event.world.EntitiesLoadEvent;
import org.bukkit.event.world.EntitiesUnloadEvent;
import org.bukkit.inventory.EntityEquipment;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.Plugin;

/**
 * Studio maker M5: mobs ({@code mob}). A studio mob is a vanilla mob ({@code base}) with its own name, stats, gear,
 * drops and skills, tagged {@code monolab:mob} = its file name so it is known again after a restart (it is saved with
 * the world like any mob). With a {@code model} (M4) the base is invisible and the model rides it, walking while it
 * moves and playing its spawn, attack, hurt, interact and death roles. Skills run actions (the shared ones plus damage,
 * heal, potion...) on targets when something happens: spawn, attack, damaged, death, timer, interact, target. The
 * field rules are the same as {@code crates/studio-format/src/mob.rs}.
 *
 * <p>Threads: everything about one mob runs on its own entity scheduler (Folia); natural spawning picks a spot on the
 * player's thread and spawns on that spot's region thread.
 */
final class StudioMobs implements Listener {
    /** {@link #tick} runs once a second on the global region thread. */
    static final long TICK_EVERY = 20L;
    static final String[] TRIGGERS = {"spawn", "attack", "damaged", "death", "timer", "interact", "target"};
    static final String[] TARGETS = {"self", "target", "trigger", "nearby"};
    static final String[] SLOTS = {"head", "chest", "legs", "feet", "hand", "offhand"};
    private static final EquipmentSlot[] SLOT_TYPES = {
        EquipmentSlot.HEAD, EquipmentSlot.CHEST, EquipmentSlot.LEGS, EquipmentSlot.FEET, EquipmentSlot.HAND, EquipmentSlot.OFF_HAND
    };
    static final String[] AI = {"attack", "wander", "look", "burn", "pickup"};
    private static final boolean[] AI_DEFAULTS = {true, true, true, false, false};
    static final int MOST_SKILLS = 32;
    static final int MOST_EXP = 100_000;
    static final int MOST_ALIVE = 200;
    static final double MOST_SECONDS = 86_400;
    /** How often each live mob looks at itself (walk or idle, timers, its model), in ticks. */
    private static final long PULSE = 5L;
    /** Every this many seconds, tagged mobs near players that aren't run yet (loaded before the add-on) are picked up. */
    private static final int SCAN_EVERY = 5;
    private static final int MOST_PICKED = 36;

    /** A stat: its key in the file, its range, and the game's attribute names (now, then before 1.21.3). */
    private record Stat(String key, double least, double most, String... names) {}

    private static final Stat[] STATS = {
        new Stat("health", 1, 1024, "max_health", "generic.max_health"),
        new Stat("damage", 0, 2048, "attack_damage", "generic.attack_damage"),
        new Stat("speed", 0, 4, "movement_speed", "generic.movement_speed"),
        new Stat("armor", 0, 30, "armor", "generic.armor"),
        new Stat("toughness", 0, 20, "armor_toughness", "generic.armor_toughness"),
        new Stat("knockbackResistance", 0, 1, "knockback_resistance", "generic.knockback_resistance"),
        new Stat("followRange", 1, 256, "follow_range", "generic.follow_range"),
        new Stat("size", 0.0625, 16, "scale", "generic.scale"),
    };
    private static final Map<String, Optional<Attribute>> ATTRIBUTES = new ConcurrentHashMap<>();

    /** One studio mob in the world that we run. */
    private static final class Alive {
        final String id;
        final LivingEntity entity;
        volatile StudioModels.Instance model;
        volatile ScheduledTask task;
        volatile UUID lastTarget;
        /** When each skill (by its place in the list) may run again, in millis. */
        final Map<Integer, Long> ready = new ConcurrentHashMap<>();
        /** What it picked up from the ground: still drops when vanilla drops are off. */
        final List<ItemStack> picked = java.util.Collections.synchronizedList(new ArrayList<>());
        long pulses;
        Location last;

        Alive(String id, LivingEntity entity) {
            this.id = id;
            this.entity = entity;
        }
    }

    private static volatile StudioMobs current;

    private final Plugin plugin;
    private final Studio studio;
    private final StudioActions actions;
    private final BiConsumer<String, Throwable> warn;
    private final NamespacedKey key;
    private volatile Map<String, Studio.Resource> mobs = Map.of();
    private final Map<UUID, Alive> live = new ConcurrentHashMap<>();
    private final Map<String, String> reported = new ConcurrentHashMap<>();
    private long seconds;

    StudioMobs(Plugin plugin, Studio studio, StudioActions actions, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.studio = studio;
        this.actions = actions;
        this.warn = warn;
        this.key = new NamespacedKey(plugin, "mob");
    }

    // ---------- for other features ----------

    /** The running feature. Null before the add-on starts. */
    static StudioMobs get() {
        return current;
    }

    /** The studio mob an entity is, or null (for rule events that name mobs). */
    static String mobIdOf(org.bukkit.entity.Entity entity) {
        StudioMobs running = current;
        return running == null || entity == null ? null : entity.getPersistentDataContainer().get(running.key, org.bukkit.persistence.PersistentDataType.STRING);
    }

    /** Whether a usable studio mob has this id (for the summon action). */
    static boolean has(String id) {
        StudioMobs running = current;
        return running != null && id != null && running.mobs.containsKey(id);
    }

    /**
     * Spawns a studio mob at a spot. Call on that spot's region thread. Null when there is no such mob or it couldn't
     * spawn (another plugin stopped it).
     */
    static LivingEntity summon(String id, Location at) {
        StudioMobs running = current;
        Studio.Resource one = running == null || id == null ? null : running.mobs.get(id);
        return one == null ? null : running.spawn(one, at, CreatureSpawnEvent.SpawnReason.CUSTOM);
    }

    /** A studio mob with vanilla drops off: loot tables for its base mob don't drop from it either. */
    static boolean ownDropsOnly(Entity entity) {
        StudioMobs running = current;
        if (running == null) {
            return false;
        }
        String id = running.idOf(entity);
        Studio.Resource one = id == null ? null : running.mobs.get(id);
        return one != null && !Boolean.TRUE.equals(one.data().get("vanillaDrops"));
    }

    /** A stat's attribute on an entity ({@code health}, {@code speed}...), or null when it has none. */
    static AttributeInstance attributeOf(LivingEntity living, String stat) {
        Attribute attribute = attribute(stat);
        return attribute == null ? null : living.getAttribute(attribute);
    }

    private static Attribute attribute(String stat) {
        return ATTRIBUTES.computeIfAbsent(stat, wanted -> {
            for (Stat one : STATS) {
                if (!one.key().equals(wanted)) {
                    continue;
                }
                for (String name : one.names()) {
                    try {
                        Attribute found = Registry.ATTRIBUTE.get(NamespacedKey.minecraft(name));
                        if (found != null) {
                            return Optional.of(found);
                        }
                    } catch (RuntimeException | LinkageError older) {
                        // Try the older name.
                    }
                }
            }
            return Optional.empty();
        }).orElse(null);
    }

    void register() {
        current = this;
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "mob";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, Studio.Resource> next = new HashMap<>();
                reported.clear();
                for (Studio.Resource one : resources) {
                    String problem = problem(one.data());
                    if (problem != null) {
                        report(one.id(), problem);
                        continue;
                    }
                    next.put(one.id(), one);
                    String model = StudioText.text(one.data(), "model", "").trim();
                    String loot = StudioText.text(one.data(), "loot", "").trim();
                    if (!model.isEmpty() && !studio.has("model", model)) {
                        report(one.id(), "Works, but there is no studio model called " + model + ".");
                    } else if (!loot.isEmpty() && !studio.has("loot", loot)) {
                        report(one.id(), "Works, but there is no studio loot table called " + loot + ".");
                    }
                }
                mobs = Map.copyOf(next);
                // Mobs already out there take the new file: name, stats, look and AI (gear and health stay).
                for (Alive alive : live.values()) {
                    Studio.Resource now = next.get(alive.id);
                    alive.ready.clear();
                    alive.entity.getScheduler().run(plugin, task -> refresh(alive, now), null);
                }
            }
        });
    }

    // ---------- the shared rules ----------

    /** What is wrong with a mob file, or null. Numbers out of range are clamped, not refused. */
    static String problem(Map<String, Object> data) {
        String base = base(data);
        if (type(base) == null) {
            return "\"" + base + "\" isn't a mob a studio mob can be based on.";
        }
        if (data.get("name") != null && !(data.get("name") instanceof String)) {
            return "\"name\" must be text.";
        }
        if (data.get("equipment") != null && !(data.get("equipment") instanceof Map)) {
            return "\"equipment\" maps a slot (head, hand...) to an item.";
        }
        if (data.get("ai") != null && !(data.get("ai") instanceof Map)) {
            return "\"ai\" holds switches: " + String.join(", ", AI) + ".";
        }
        if (data.get("spawn") != null && !(data.get("spawn") instanceof Map)) {
            return "\"spawn\" must be an object.";
        }
        Object skills = data.get("skills");
        if (skills != null && !(skills instanceof List)) {
            return "\"skills\" must be a list.";
        }
        if (skills instanceof List<?> list) {
            int number = 0;
            for (Object raw : list) {
                number++;
                if (number > MOST_SKILLS) {
                    break;
                }
                if (!(raw instanceof Map<?, ?> map)) {
                    return "Skill " + number + " must be an object.";
                }
                Map<String, Object> skill = StudioActions.map(map);
                if (!List.of(TRIGGERS).contains(StudioText.text(skill, "trigger", ""))) {
                    return "Skill " + number + ": \"trigger\" must be one of: " + String.join(", ", TRIGGERS) + ".";
                }
                Object targets = skill.get("targets");
                if (targets != null && !(targets instanceof String named && List.of(TARGETS).contains(named))) {
                    return "Skill " + number + ": \"targets\" must be one of: " + String.join(", ", TARGETS) + ".";
                }
                if (skill.get("conditions") != null && !(skill.get("conditions") instanceof List)) {
                    return "Skill " + number + ": \"conditions\" must be a list.";
                }
                String wrong = StudioActions.problem(skill.get("actions"), (kind, id) -> true);
                if (wrong != null) {
                    return "Skill " + number + ": " + wrong;
                }
            }
        }
        return null;
    }

    /** The base mob's name, lower case; {@code zombie} when missing. */
    static String base(Map<String, Object> data) {
        String one = StudioLoot.clean(StudioText.text(data, "base", ""));
        return one.isEmpty() ? "zombie" : one;
    }

    /** The entity type a mob can be based on: a living, spawnable mob (not a player, armor stand or boss). */
    static EntityType type(String name) {
        EntityType type = StudioNpcs.type(name);
        if (type == null || type.getEntityClass() == null || !Mob.class.isAssignableFrom(type.getEntityClass())) {
            return null;
        }
        return type;
    }

    /** An AI switch; see {@link #AI} for what each is when the file doesn't say. */
    static boolean ai(Map<String, Object> data, String name) {
        for (int at = 0; at < AI.length; at++) {
            if (AI[at].equals(name)) {
                Object set = StudioText.object(data, "ai").get(name);
                return set instanceof Boolean one ? one : AI_DEFAULTS[at];
            }
        }
        return false;
    }

    private static double decimal(Map<String, Object> data, String key, double fallback, double least, double most) {
        double read = data.get(key) instanceof Double one && Double.isFinite(one) ? one : fallback;
        return Math.max(least, Math.min(most, read));
    }

    /** The skills that are objects, at most {@link #MOST_SKILLS}; a skill's place in this list is its cooldown key. */
    private static List<Map<String, Object>> skills(Map<String, Object> data) {
        List<Map<String, Object>> out = new ArrayList<>();
        if (data.get("skills") instanceof List<?> list) {
            for (Object raw : list) {
                if (out.size() == MOST_SKILLS) {
                    break;
                }
                if (raw instanceof Map<?, ?> one) {
                    out.add(StudioActions.map(one));
                }
            }
        }
        return out;
    }

    /** The studio id of a mob entity, or null. */
    String idOf(Entity entity) {
        return entity.getPersistentDataContainer().get(key, PersistentDataType.STRING);
    }

    // ---------- spawning ----------

    /** On the region thread of {@code at}. */
    private LivingEntity spawn(Studio.Resource one, Location at, CreatureSpawnEvent.SpawnReason reason) {
        Map<String, Object> data = one.data();
        EntityType type = type(base(data));
        if (type == null || at.getWorld() == null) {
            return null;
        }
        Entity made;
        try {
            made = at.getWorld().spawnEntity(at, type, reason, entity -> {
                entity.getPersistentDataContainer().set(key, PersistentDataType.STRING, one.id());
                if (entity instanceof LivingEntity living) {
                    looks(living, data, true);
                }
            });
        } catch (RuntimeException error) {
            warn.accept("A studio mob couldn't spawn: " + one.id(), error);
            return null;
        }
        if (!(made instanceof LivingEntity living) || !living.isValid()) {
            return null; // Another plugin stopped the spawn.
        }
        Alive alive = adopt(living, one);
        if (alive != null) {
            StudioModels.Instance model = alive.model;
            if (model != null) {
                model.play("spawn");
            }
            fire(alive, "spawn", null);
        }
        return living;
    }

    /** Name, flags and stats; when {@code fresh} (just spawned) also full health and gear. On the entity's thread. */
    private void looks(LivingEntity entity, Map<String, Object> data, boolean fresh) {
        String name = StudioText.text(data, "name", "");
        if (name.isBlank()) {
            entity.customName(null);
            entity.setCustomNameVisible(false);
        } else {
            entity.customName(StudioText.render(name, null));
            entity.setCustomNameVisible(!Boolean.FALSE.equals(data.get("showName")));
        }
        entity.setSilent(Boolean.TRUE.equals(data.get("silent")));
        entity.setGlowing(Boolean.TRUE.equals(data.get("glowing")));
        entity.setRemoveWhenFarAway(!Boolean.TRUE.equals(data.get("persistent")));
        entity.setCanPickupItems(ai(data, "pickup"));
        for (Stat stat : STATS) {
            if (!(data.get(stat.key()) instanceof Double value) || !Double.isFinite(value)) {
                continue;
            }
            AttributeInstance instance = attributeOf(entity, stat.key());
            if (instance != null) {
                instance.setBaseValue(Math.max(stat.least(), Math.min(stat.most(), value)));
            }
        }
        AttributeInstance max = attributeOf(entity, "health");
        if (max != null) {
            double most = max.getValue();
            if (fresh) {
                entity.setHealth(Math.max(0.5, most));
            } else if (entity.getHealth() > most) {
                entity.setHealth(most);
            }
        }
        if (fresh) {
            equip(entity, data);
        }
    }

    private void equip(LivingEntity entity, Map<String, Object> data) {
        EntityEquipment gear = entity.getEquipment();
        if (gear == null) {
            return;
        }
        gear.clear();
        Map<String, Object> wanted = StudioText.object(data, "equipment");
        for (int at = 0; at < SLOTS.length; at++) {
            String item = StudioText.text(wanted, SLOTS[at], "").trim();
            if (item.isEmpty()) {
                continue;
            }
            ItemStack stack = actions.stack(item, 1);
            if (stack != null) {
                gear.setItem(SLOT_TYPES[at], stack);
            }
        }
        // Gear drops by dropEquipment on death, never by the game's chances.
        gear.setHelmetDropChance(0f);
        gear.setChestplateDropChance(0f);
        gear.setLeggingsDropChance(0f);
        gear.setBootsDropChance(0f);
        gear.setItemInMainHandDropChance(0f);
        gear.setItemInOffHandDropChance(0f);
    }

    /** The AI switches that are off. Goals aren't saved, so this runs again whenever a mob is picked up. */
    private void ai(LivingEntity entity, Map<String, Object> data) {
        entity.setCanPickupItems(ai(data, "pickup"));
        if (!(entity instanceof Mob mob)) {
            return;
        }
        boolean attack = ai(data, "attack");
        boolean wander = ai(data, "wander");
        boolean look = ai(data, "look");
        if (!attack) {
            mob.setTarget(null);
        }
        if (attack && wander && look) {
            return;
        }
        try {
            MobGoals goals = Bukkit.getMobGoals();
            if (!attack) {
                goals.removeAllGoals(mob, GoalType.TARGET);
            }
            for (Goal<Mob> goal : new ArrayList<>(goals.getAllGoals(mob))) {
                String name = goal.getKey().getNamespacedKey().getKey();
                if ((!wander && wanders(name)) || (!look && name.contains("look"))) {
                    goals.removeGoal(mob, goal);
                }
            }
        } catch (RuntimeException | LinkageError unsupported) {
            // A server without Paper's goal API: those switches do nothing (attack still works by cancelling targets).
        }
    }

    private static boolean wanders(String goal) {
        return goal.contains("stroll") || goal.contains("wander") || goal.contains("random_swim") || goal.contains("random_fly");
    }

    /** Starts running a mob that is in the world. On the entity's thread. Null when it is already gone. */
    private Alive adopt(LivingEntity entity, Studio.Resource one) {
        Alive known = live.get(entity.getUniqueId());
        if (known != null) {
            return known;
        }
        Alive alive = new Alive(one.id(), entity);
        live.put(entity.getUniqueId(), alive);
        ai(entity, one.data());
        attachModel(alive, one.data());
        ScheduledTask task = entity.getScheduler().runAtFixedRate(plugin, running -> pulse(alive), () -> forget(alive), PULSE, PULSE);
        if (task == null) {
            forget(alive);
            return null;
        }
        alive.task = task;
        return alive;
    }

    /** A tagged mob loaded from the world (a restart, a chunk coming back). On the entity's thread. */
    private void adoptLoaded(LivingEntity entity) {
        if (!entity.isValid() || live.containsKey(entity.getUniqueId())) {
            return;
        }
        String id = idOf(entity);
        Studio.Resource one = id == null ? null : mobs.get(id);
        if (one != null) {
            adopt(entity, one);
        }
    }

    /** The mob's file changed (or went: {@code one} null). On the entity's thread. */
    private void refresh(Alive alive, Studio.Resource one) {
        LivingEntity entity = alive.entity;
        if (!entity.isValid()) {
            return;
        }
        StudioModels.Instance old = alive.model;
        alive.model = null;
        if (old != null) {
            old.remove();
        }
        if (one == null) {
            // Its file is gone or off: it stays a plain mob, seen as itself.
            entity.setInvisible(false);
            return;
        }
        looks(entity, one.data(), false);
        ai(entity, one.data());
        attachModel(alive, one.data());
    }

    /** Puts the file's model on the mob (the base turns invisible), or shows the base when there is none. */
    private void attachModel(Alive alive, Map<String, Object> data) {
        String model = StudioText.text(data, "model", "").trim();
        StudioModels models = StudioModels.get();
        StudioModels.Instance made = model.isEmpty() || models == null ? null : models.attach(model, alive.entity, false);
        alive.model = made;
        alive.entity.setInvisible(made != null);
    }

    private void forget(Alive alive) {
        live.remove(alive.entity.getUniqueId(), alive);
        ScheduledTask task = alive.task;
        if (task != null) {
            task.cancel();
        }
        StudioModels.Instance model = alive.model;
        alive.model = null;
        if (model != null) {
            model.remove();
        }
    }

    private int count(String id) {
        int out = 0;
        for (Alive alive : live.values()) {
            if (alive.id.equals(id)) {
                out++;
            }
        }
        return out;
    }

    // ---------- every few ticks, per mob ----------

    /** Every {@link #PULSE} ticks on the mob's thread: walk or idle, timers, a model that wasn't there yet. */
    private void pulse(Alive alive) {
        LivingEntity entity = alive.entity;
        if (!entity.isValid() || entity.isDead()) {
            return;
        }
        Studio.Resource one = mobs.get(alive.id);
        if (one == null) {
            return;
        }
        alive.pulses++;
        StudioModels.Instance model = alive.model;
        if (model != null && !model.alive()) {
            alive.model = null;
            model = null;
        }
        if (model == null && alive.pulses % 20 == 0 && !StudioText.text(one.data(), "model", "").isBlank()) {
            // The model's rig wasn't built yet, or it went with its chunk: try again every 5 seconds.
            attachModel(alive, one.data());
            model = alive.model;
        }
        Location now = entity.getLocation();
        Location last = alive.last;
        if (model != null && last != null && last.getWorld() == now.getWorld()) {
            double dx = now.getX() - last.getX();
            double dz = now.getZ() - last.getZ();
            model.moving(dx * dx + dz * dz > 0.01);
        }
        alive.last = now;
        if (entity instanceof Mob mob && mob.getTarget() == null) {
            alive.lastTarget = null;
        }
        long ticks = alive.pulses * PULSE;
        List<Map<String, Object>> skills = skills(one.data());
        for (int index = 0; index < skills.size(); index++) {
            Map<String, Object> skill = skills.get(index);
            if (!"timer".equals(StudioText.text(skill, "trigger", ""))) {
                continue;
            }
            long every = Math.max(PULSE, Math.round(decimal(skill, "every", 1, 1, MOST_SECONDS) * 20));
            if (ticks / every != (ticks - PULSE) / every) {
                runSkill(alive, index, skill, null);
            }
        }
    }

    // ---------- skills ----------

    /** Runs every skill of this trigger. {@code other} is the trigger's other entity (who was hit, who hurt it...). */
    private void fire(Alive alive, String trigger, Entity other) {
        Studio.Resource one = mobs.get(alive.id);
        if (one == null) {
            return;
        }
        List<Map<String, Object>> skills = skills(one.data());
        for (int index = 0; index < skills.size(); index++) {
            Map<String, Object> skill = skills.get(index);
            if (trigger.equals(StudioText.text(skill, "trigger", ""))) {
                runSkill(alive, index, skill, other);
            }
        }
    }

    private void runSkill(Alive alive, int index, Map<String, Object> skill, Entity other) {
        long now = System.currentTimeMillis();
        Long ready = alive.ready.get(index);
        if (ready != null && ready > now) {
            return;
        }
        double chance = decimal(skill, "chance", 100, 0, 100);
        if (chance < 100 && ThreadLocalRandom.current().nextDouble() * 100 >= chance) {
            return;
        }
        double cooldown = decimal(skill, "cooldown", 0, 0, MOST_SECONDS);
        if (cooldown > 0) {
            alive.ready.put(index, now + (long) (cooldown * 1000));
        }
        String mobName = displayName(alive);
        for (Entity target : targets(alive, skill, other)) {
            Player player = target instanceof Player one ? one : null;
            Map<String, String> extra = new HashMap<>();
            extra.put("mob", mobName);
            extra.put("mob_id", alive.id);
            extra.put("target", nameOf(target));
            StudioActions.Run run = new StudioActions.Run(player, extra, alive.entity, target);
            try {
                if (actions.test(skill.get("conditions"), run)) {
                    actions.run(skill.get("actions"), run);
                }
            } catch (RuntimeException error) {
                warn.accept("A studio mob's skill failed: " + alive.id, error);
            }
        }
    }

    private List<Entity> targets(Alive alive, Map<String, Object> skill, Entity other) {
        LivingEntity mob = alive.entity;
        return switch (StudioText.text(skill, "targets", "trigger")) {
            case "self" -> List.of(mob);
            case "target" -> {
                LivingEntity target = mob instanceof Mob one ? one.getTarget() : null;
                yield target == null ? List.of() : List.of(target);
            }
            case "nearby" -> {
                double radius = decimal(skill, "radius", 8, 1, 64);
                List<Entity> near = new ArrayList<>();
                for (Player player : mob.getWorld().getNearbyPlayers(mob.getLocation(), radius)) {
                    if (player.getGameMode() != GameMode.SPECTATOR && !player.isDead()) {
                        near.add(player);
                    }
                }
                yield near;
            }
            default -> other == null ? List.of() : List.of(other);
        };
    }

    /** The mob's name as plain text (the file's name, else its own). */
    private String displayName(Alive alive) {
        Studio.Resource one = mobs.get(alive.id);
        String name = one == null ? "" : StudioText.text(one.data(), "name", "");
        if (!name.isBlank()) {
            return PlainTextComponentSerializer.plainText().serialize(StudioText.render(name, null));
        }
        return nameOf(alive.entity);
    }

    private static String nameOf(Entity entity) {
        if (entity instanceof Player player) {
            return player.getName();
        }
        Component custom = entity.customName();
        return custom != null ? PlainTextComponentSerializer.plainText().serialize(custom) : entity.getName();
    }

    /** Who really did it: an arrow's shooter, else the entity itself. */
    private static Entity source(Entity damager) {
        if (damager instanceof Projectile shot && shot.getShooter() instanceof Entity shooter) {
            return shooter;
        }
        return damager;
    }

    // ---------- events ----------

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onDamage(EntityDamageEvent event) {
        if (StudioActions.bySkill()) {
            return; // A skill's own damage doesn't start more skills.
        }
        Entity hurt = event.getEntity();
        Entity by = event instanceof EntityDamageByEntityEvent hit ? source(hit.getDamager()) : null;
        Alive victim = live.get(hurt.getUniqueId());
        if (victim != null) {
            StudioModels.Instance model = victim.model;
            if (model != null) {
                model.play("hurt");
            }
            fire(victim, "damaged", by);
        }
        Alive attacker = by == null ? null : live.get(by.getUniqueId());
        if (attacker != null && attacker != victim) {
            StudioModels.Instance model = attacker.model;
            if (model != null) {
                model.play("attack");
            }
            fire(attacker, "attack", hurt);
        }
    }

    /** Drops first (before loot tables add theirs), so vanillaDrops off clears only the game's own. */
    @EventHandler(priority = EventPriority.LOW, ignoreCancelled = true)
    public void onDrops(EntityDeathEvent event) {
        LivingEntity entity = event.getEntity();
        String id = idOf(entity);
        Studio.Resource one = id == null ? null : mobs.get(id);
        if (one == null) {
            return;
        }
        Map<String, Object> data = one.data();
        if (!Boolean.TRUE.equals(data.get("vanillaDrops"))) {
            List<ItemStack> kept = new ArrayList<>();
            Alive alive = live.get(entity.getUniqueId());
            if (alive != null) {
                synchronized (alive.picked) {
                    for (ItemStack drop : event.getDrops()) {
                        for (ItemStack picked : alive.picked) {
                            if (drop != null && drop.isSimilar(picked)) {
                                kept.add(drop);
                                break;
                            }
                        }
                    }
                }
            }
            event.getDrops().clear();
            event.getDrops().addAll(kept);
        }
        if (Boolean.TRUE.equals(data.get("dropEquipment"))) {
            Map<String, Object> gear = StudioText.object(data, "equipment");
            for (String slot : SLOTS) {
                String item = StudioText.text(gear, slot, "").trim();
                ItemStack stack = item.isEmpty() ? null : actions.stack(item, 1);
                if (stack != null) {
                    event.getDrops().add(stack);
                }
            }
        }
        String loot = StudioText.text(data, "loot", "").trim();
        StudioLoot tables = StudioLoot.get();
        if (!loot.isEmpty() && tables != null) {
            event.getDrops().addAll(tables.roll(loot));
        }
        if (data.get("exp") instanceof Double exp && Double.isFinite(exp)) {
            event.setDroppedExp((int) Math.max(0, Math.min(MOST_EXP, Math.round(exp))));
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onDeath(EntityDeathEvent event) {
        LivingEntity entity = event.getEntity();
        Alive alive = live.get(entity.getUniqueId());
        if (alive == null) {
            return;
        }
        Entity killer = entity.getKiller();
        if (killer == null && entity.getLastDamageCause() instanceof EntityDamageByEntityEvent hit) {
            killer = source(hit.getDamager());
        }
        fire(alive, "death", killer);
        StudioModels.Instance model = alive.model;
        alive.model = null;
        if (model != null) {
            model.finish("death");
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onInteract(PlayerInteractEntityEvent event) {
        interacted(event);
    }

    /** 26.x clients send only the "at" interact; older ones send both (see {@link StudioNpcs#firstOfClick}). */
    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onInteractAt(PlayerInteractAtEntityEvent event) {
        interacted(event);
    }

    private void interacted(PlayerInteractEntityEvent event) {
        if (event.getHand() != EquipmentSlot.HAND) {
            return; // The off hand sends one more.
        }
        Alive alive = live.get(event.getRightClicked().getUniqueId());
        if (alive == null || !StudioNpcs.firstOfClick(event.getPlayer(), event.getRightClicked())) {
            return;
        }
        StudioModels.Instance model = alive.model;
        if (model != null) {
            model.play("interact");
        }
        fire(alive, "interact", event.getPlayer());
    }

    /** {@code ai.attack} off: it never picks a target. */
    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onTargetCheck(EntityTargetLivingEntityEvent event) {
        if (event.getTarget() == null) {
            return;
        }
        String id = idOf(event.getEntity());
        Studio.Resource one = id == null ? null : mobs.get(id);
        if (one != null && !ai(one.data(), "attack")) {
            event.setCancelled(true);
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onTarget(EntityTargetLivingEntityEvent event) {
        Alive alive = live.get(event.getEntity().getUniqueId());
        if (alive == null) {
            return;
        }
        LivingEntity target = event.getTarget();
        if (target == null) {
            alive.lastTarget = null;
            return;
        }
        if (target.getUniqueId().equals(alive.lastTarget)) {
            return;
        }
        alive.lastTarget = target.getUniqueId();
        fire(alive, "target", target);
    }

    /** {@code ai.burn} off: no burning in daylight (fire and lava still burn). */
    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onCombust(EntityCombustEvent event) {
        if (event.getClass() != EntityCombustEvent.class) {
            return; // By a block or an entity: a real fire.
        }
        String id = idOf(event.getEntity());
        Studio.Resource one = id == null ? null : mobs.get(id);
        if (one != null && !ai(one.data(), "burn")) {
            event.setCancelled(true);
        }
    }

    @EventHandler(priority = EventPriority.MONITOR, ignoreCancelled = true)
    public void onPickup(EntityPickupItemEvent event) {
        Alive alive = live.get(event.getEntity().getUniqueId());
        if (alive != null && alive.picked.size() < MOST_PICKED) {
            alive.picked.add(event.getItem().getItemStack().clone());
        }
    }

    @EventHandler(priority = EventPriority.MONITOR)
    public void onLoad(EntitiesLoadEvent event) {
        if (mobs.isEmpty()) {
            return;
        }
        for (Entity entity : event.getEntities()) {
            if (entity instanceof LivingEntity living && !(entity instanceof Player) && !live.containsKey(entity.getUniqueId()) && idOf(entity) != null) {
                living.getScheduler().run(plugin, task -> adoptLoaded(living), null);
            }
        }
    }

    @EventHandler(priority = EventPriority.MONITOR)
    public void onUnload(EntitiesUnloadEvent event) {
        for (Entity entity : event.getEntities()) {
            Alive alive = live.get(entity.getUniqueId());
            if (alive != null) {
                forget(alive);
            }
        }
    }

    // ---------- once a second ----------

    /** Every second on the global region thread: natural spawning, and picking up tagged mobs near players. */
    void tick() {
        long now = ++seconds;
        if (mobs.isEmpty()) {
            return;
        }
        if (now % SCAN_EVERY == 0) {
            for (Player player : Bukkit.getOnlinePlayers()) {
                player.getScheduler().run(plugin, task -> scan(player), null);
            }
        }
        for (Studio.Resource one : mobs.values()) {
            Map<String, Object> spawn = StudioText.object(one.data(), "spawn");
            if (!Boolean.TRUE.equals(spawn.get("enabled"))) {
                continue;
            }
            int every = StudioText.number(spawn, "every", 30, 1, 3600);
            if (now % every != 0 || count(one.id()) >= StudioText.number(spawn, "max", 10, 1, MOST_ALIVE)) {
                continue;
            }
            double chance = decimal(spawn, "chance", 50, 0, 100);
            for (Player player : Bukkit.getOnlinePlayers()) {
                if (ThreadLocalRandom.current().nextDouble() * 100 < chance) {
                    player.getScheduler().run(plugin, task -> trySpawn(one, player), null);
                }
            }
        }
    }

    /** On the player's thread: picks a spot 16..radius blocks away and spawns there on that spot's thread. */
    private void trySpawn(Studio.Resource one, Player player) {
        if (!player.isOnline() || player.getGameMode() == GameMode.SPECTATOR) {
            return;
        }
        Map<String, Object> spawn = StudioText.object(one.data(), "spawn");
        World world = player.getWorld();
        List<String> worlds = StudioText.lines(spawn, "worlds");
        worlds.replaceAll(String::trim);
        worlds.removeIf(String::isEmpty);
        if (!worlds.isEmpty() && !worlds.contains(world.getName())) {
            return;
        }
        double radius = decimal(spawn, "radius", 32, 16, 128);
        ThreadLocalRandom dice = ThreadLocalRandom.current();
        double angle = dice.nextDouble() * Math.PI * 2;
        double distance = 16 + dice.nextDouble() * (radius - 16);
        Location from = player.getLocation();
        int x = (int) Math.floor(from.getX() + Math.cos(angle) * distance);
        int z = (int) Math.floor(from.getZ() + Math.sin(angle) * distance);
        Bukkit.getRegionScheduler().execute(plugin, world, x >> 4, z >> 4, () -> spawnAt(one.id(), world, x, z));
    }

    /** On the region thread of (x, z): on the surface, when light, biome and the alive count allow. */
    private void spawnAt(String id, World world, int x, int z) {
        try {
            Studio.Resource one = mobs.get(id);
            if (one == null || !world.isChunkLoaded(x >> 4, z >> 4)) {
                return;
            }
            Map<String, Object> spawn = StudioText.object(one.data(), "spawn");
            if (count(id) >= StudioText.number(spawn, "max", 10, 1, MOST_ALIVE)) {
                return;
            }
            Block ground = world.getHighestBlockAt(x, z);
            if (!ground.getType().isSolid()) {
                return; // Water, lava or nothing.
            }
            Block air = ground.getRelative(BlockFace.UP);
            if (air.getY() >= world.getMaxHeight()) {
                return;
            }
            int light = air.getLightLevel();
            if (light < StudioText.number(spawn, "lightMin", 0, 0, 15) || light > StudioText.number(spawn, "lightMax", 15, 0, 15)) {
                return;
            }
            List<String> biomes = StudioText.lines(spawn, "biomes");
            biomes.replaceAll(StudioLoot::clean);
            biomes.removeIf(String::isEmpty);
            if (!biomes.isEmpty() && !biomes.contains(biomeOf(air))) {
                return;
            }
            Location at = air.getLocation().add(0.5, 0, 0.5);
            at.setYaw((float) (ThreadLocalRandom.current().nextDouble() * 360));
            spawn(one, at, CreatureSpawnEvent.SpawnReason.NATURAL);
        } catch (RuntimeException error) {
            warn.accept("A studio mob couldn't spawn: " + id, error);
        }
    }

    private static String biomeOf(Block block) {
        try {
            return block.getBiome().getKey().getKey();
        } catch (RuntimeException | LinkageError unknown) {
            return "";
        }
    }

    /** On the player's thread: tagged mobs close by that we don't run yet. */
    private void scan(Player player) {
        if (!player.isOnline()) {
            return;
        }
        for (Entity near : player.getNearbyEntities(48, 32, 48)) {
            if (near instanceof LivingEntity living && !(near instanceof Player) && !live.containsKey(near.getUniqueId()) && idOf(near) != null) {
                living.getScheduler().run(plugin, task -> adoptLoaded(living), null);
            }
        }
    }

    private void report(String id, String problem) {
        if (!problem.equals(reported.put(id, problem))) {
            studio.problem("mob", id, problem);
        }
    }

    /**
     * When the plugin turns off: nothing can be scheduled any more. Models go (StudioModels removes them), so mobs
     * are made visible again here, where this thread may touch them; the rest turn visible when picked up again.
     */
    void stop() {
        mobs = Map.of();
        for (Alive alive : live.values()) {
            ScheduledTask task = alive.task;
            if (task != null) {
                task.cancel();
            }
            try {
                if (alive.entity.isValid()) {
                    alive.entity.setInvisible(false);
                }
            } catch (RuntimeException otherThread) {
                // Folia: not this region's entity.
            }
        }
        live.clear();
        reported.clear();
        if (current == this) {
            current = null;
        }
    }
}

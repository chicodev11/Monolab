package com.github.chicodev11.monolab.addon;

import io.papermc.paper.threadedregions.scheduler.ScheduledTask;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.BiConsumer;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.World;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Interaction;
import org.bukkit.entity.ItemDisplay;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.EventPriority;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerInteractEntityEvent;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.persistence.PersistentDataType;
import org.bukkit.plugin.Plugin;
import org.bukkit.util.Transformation;
import org.joml.Quaternionf;
import org.joml.Vector3f;

/**
 * Studio models (M4): a Blockbench model shown as one item display per bone. The app bakes everything into a rig,
 * {@code studio/pack/models/<id>.json} (see {@code src-tauri/src/studio_pack_models.rs}): each part's item model and
 * its pose for the rest frame and every tick of every animation. Here we only spawn the displays and step through
 * frames, each change smoothed over one tick by the display's own interpolation. Nothing is saved into the world.
 *
 * <p>Other features use {@link #get()}: {@link #spawn} at a spot, {@link #attach} to an entity, then
 * {@link Instance#play} and {@link Instance#remove}. A model file with {@code place} is shown there by itself, like a
 * hologram. The {@code animation} action plays an animation on every placed copy of a model.
 */
final class StudioModels implements Listener {
    static final long TICK_EVERY = 20L;

    record Part(String bone, String item) {}

    record Animation(String loop, List<float[]> frames) {}

    record Rig(String id, List<Part> parts, float[] rest, Map<String, Animation> animations, float[] seat, List<String> problems) {
        static final Rig MISSING = new Rig("", List.of(), new float[0], Map.of(), null, List.of("Save the model in the app so the studio pack is built."));
    }

    private static volatile StudioModels current;

    private final Plugin plugin;
    private final Studio studio;
    private final StudioActions actions;
    private final BiConsumer<String, Throwable> warn;
    private final Path rigFolder;
    private final NamespacedKey key;
    private volatile List<Studio.Resource> models = List.of();
    private final Map<String, Rig> rigs = new ConcurrentHashMap<>();
    private final Map<String, Long> rigStamps = new ConcurrentHashMap<>();
    /** Placed by their file ({@code place}), by model id. */
    private final Map<String, Instance> placed = new ConcurrentHashMap<>();
    /** Every live copy, placed or not, by the entity uuid of any of its parts or hitbox. */
    private final Map<UUID, Instance> byEntity = new ConcurrentHashMap<>();
    /** Copies that follow an entity (an NPC, a mob), by that entity's uuid. */
    private final Map<UUID, Instance> following = new ConcurrentHashMap<>();
    private final Set<Instance> live = ConcurrentHashMap.newKeySet();
    private final Map<String, String> reported = new ConcurrentHashMap<>();

    StudioModels(Plugin plugin, Studio studio, StudioActions actions, Path studioRoot, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.studio = studio;
        this.actions = actions;
        this.warn = warn;
        this.rigFolder = studioRoot.resolve("pack").resolve("models");
        this.key = new NamespacedKey(plugin, "model");
    }

    /** The running feature, for NPCs, furniture and mobs. Null before the add-on starts. */
    static StudioModels get() {
        return current;
    }

    void register() {
        current = this;
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "model";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                models = resources;
                reported.clear();
                rigStamps.clear();
                for (Studio.Resource one : resources) {
                    Rig rig = rig(one.id());
                    String problem = problemOf(one.data(), rig);
                    if (problem != null) {
                        report(one.id(), problem);
                    }
                }
                // Placed copies are rebuilt from scratch: the next tick spawns what is there now.
                for (String id : List.copyOf(placed.keySet())) {
                    Instance gone = placed.remove(id);
                    if (gone != null) {
                        gone.remove();
                    }
                }
                tick();
            }
        });
        actions.action("animation", (step, run) -> {
            String model = StudioText.text(step, "model", "");
            String name = StudioText.text(step, "animation", "");
            if (name.isEmpty()) {
                return;
            }
            if (model.isEmpty()) {
                // In a mob's skill: that mob's own model.
                Instance own = run.caster() == null ? null : following.get(run.caster().getUniqueId());
                if (own != null) {
                    own.play(name);
                }
                return;
            }
            playOn(model, name);
        });
    }

    // ---------- rigs ----------

    /** The model's rig, reread when the file changed. Any thread. */
    Rig rig(String id) {
        if (!id.matches("[a-z0-9_-]{1,48}")) {
            return Rig.MISSING;
        }
        Path file = rigFolder.resolve(id + ".json");
        long stamp;
        try {
            stamp = Files.exists(file) ? Files.getLastModifiedTime(file).toMillis() ^ Files.size(file) : Long.MIN_VALUE;
        } catch (IOException gone) {
            stamp = Long.MIN_VALUE;
        }
        Long seen = rigStamps.get(id);
        Rig known = rigs.get(id);
        if (seen != null && seen == stamp && known != null) {
            return known;
        }
        rigStamps.put(id, stamp);
        Rig read = Rig.MISSING;
        if (stamp != Long.MIN_VALUE) {
            try {
                read = parse(id, Files.readString(file, StandardCharsets.UTF_8));
            } catch (IOException | RuntimeException broken) {
                read = new Rig(id, List.of(), new float[0], Map.of(), null, List.of("The model's rig can't be read. Save the model in the app again."));
            }
        }
        rigs.put(id, read);
        return read;
    }

    @SuppressWarnings("unchecked")
    static Rig parse(String id, String text) {
        if (!(MiniJson.parse(text) instanceof Map<?, ?> raw)) {
            return Rig.MISSING;
        }
        Map<String, Object> root = (Map<String, Object>) raw;
        List<Part> parts = new ArrayList<>();
        if (root.get("parts") instanceof List<?> list) {
            for (Object one : list) {
                if (one instanceof Map<?, ?> part) {
                    Map<String, Object> map = (Map<String, Object>) part;
                    parts.add(new Part(StudioText.text(map, "bone", ""), StudioText.text(map, "item", "")));
                }
            }
        }
        Map<String, Animation> animations = new HashMap<>();
        for (Map.Entry<String, Object> one : StudioText.object(root, "animations").entrySet()) {
            if (!(one.getValue() instanceof Map<?, ?> anim)) {
                continue;
            }
            Map<String, Object> map = (Map<String, Object>) anim;
            List<float[]> frames = new ArrayList<>();
            if (map.get("frames") instanceof List<?> list) {
                for (Object frame : list) {
                    frames.add(floats(frame));
                }
            }
            animations.put(one.getKey(), new Animation(StudioText.text(map, "loop", "once"), List.copyOf(frames)));
        }
        float[] seat = root.get("seat") instanceof List<?> ? floats(root.get("seat")) : null;
        return new Rig(id, List.copyOf(parts), floats(root.get("rest")), Map.copyOf(animations), seat, StudioText.lines(root, "problems"));
    }

    private static float[] floats(Object raw) {
        if (!(raw instanceof List<?> list)) {
            return new float[0];
        }
        float[] out = new float[list.size()];
        for (int at = 0; at < out.length; at++) {
            out[at] = list.get(at) instanceof Double one ? one.floatValue() : 0f;
        }
        return out;
    }

    private String problemOf(Map<String, Object> data, Rig rig) {
        if (!rig.problems().isEmpty()) {
            return String.join(" ", rig.problems());
        }
        if (rig.parts().isEmpty()) {
            return "The model has nothing to show.";
        }
        return null;
    }

    private Studio.Resource resource(String id) {
        for (Studio.Resource one : models) {
            if (one.id().equals(id)) {
                return one;
            }
        }
        return null;
    }

    // ---------- placed models ----------

    /** Every second on the global region thread: spawn placed models whose chunk is loaded, show by permission. */
    void tick() {
        for (Studio.Resource one : models) {
            Map<String, Object> place = StudioText.object(one.data(), "place");
            if (place.isEmpty()) {
                continue;
            }
            World world = Bukkit.getWorld(StudioText.text(place, "world", "world"));
            if (world == null) {
                report(one.id(), "There is no world named \"" + StudioText.text(place, "world", "world") + "\".");
                continue;
            }
            if (!(place.get("x") instanceof Double x) || !(place.get("y") instanceof Double y) || !(place.get("z") instanceof Double z)) {
                continue;
            }
            float yaw = place.get("yaw") instanceof Double turn ? turn.floatValue() : 0f;
            Location at = new Location(world, x, y, z, yaw, 0f);
            Instance shown = placed.get(one.id());
            if (shown != null && shown.alive()) {
                continue;
            }
            if (!world.isChunkLoaded(at.getBlockX() >> 4, at.getBlockZ() >> 4)) {
                continue;
            }
            Bukkit.getRegionScheduler().execute(plugin, at, () -> {
                try {
                    Instance made = spawn(one.id(), at);
                    if (made != null) {
                        Instance old = placed.put(one.id(), made);
                        if (old != null) {
                            old.remove();
                        }
                    }
                } catch (RuntimeException error) {
                    warn.accept("A studio model failed: " + one.id(), error);
                }
            });
        }
        for (Instance one : live) {
            one.showByPermission();
        }
    }

    /** Plays an animation (or role) on every placed copy of a model. Any thread. */
    void playOn(String model, String animation) {
        for (Instance one : live) {
            if (one.model.equals(model)) {
                one.play(animation);
            }
        }
    }

    // ---------- the API ----------

    /** Shows a model at a spot (on that spot's region thread). Null when it has no rig yet or the server is too old. */
    Instance spawn(String model, Location at) {
        Rig rig = rig(model);
        Studio.Resource file = resource(model);
        if (rig.parts().isEmpty() || file == null || Boolean.FALSE.equals(file.data().get("enabled"))) {
            return null;
        }
        Instance made = new Instance(model, rig, file.data(), null, true);
        if (!made.build(at)) {
            return null;
        }
        return made;
    }

    private final Map<java.util.UUID, Instance> emotes = new java.util.concurrent.ConcurrentHashMap<>();

    /**
     * The {@code emote} action: the model rides the player, plays the animation once and goes (a new emote ends the
     * last one). On the player's thread. Returns why it couldn't, or null.
     */
    String emote(org.bukkit.entity.Player player, String model, String animation) {
        Instance was = emotes.remove(player.getUniqueId());
        if (was != null) {
            was.remove();
        }
        Instance made = attach(model, player, false);
        if (made == null) {
            return "The model " + model + " can't be shown (no parts yet, or turned off).";
        }
        long length = made.length(animation);
        if (length <= 0) {
            made.remove();
            return "The model " + model + " has no animation " + animation + ".";
        }
        emotes.put(player.getUniqueId(), made);
        made.play(animation);
        player.getScheduler().runDelayed(plugin, done -> {
            made.remove();
            emotes.remove(player.getUniqueId(), made);
        }, made::remove, Math.min(20L * 60, length + 1));
        return null;
    }

    /** Shows a model on an entity, following it (on that entity's thread). The model goes when the entity does. */
    Instance attach(String model, Entity entity) {
        return attach(model, entity, true);
    }

    /**
     * The same; {@code withHitbox} false leaves out the model's hitbox and seat, for a mob whose own (invisible) body
     * is what players hit and click.
     */
    Instance attach(String model, Entity entity, boolean withHitbox) {
        Rig rig = rig(model);
        Studio.Resource file = resource(model);
        if (rig.parts().isEmpty() || file == null || Boolean.FALSE.equals(file.data().get("enabled"))) {
            return null;
        }
        Instance made = new Instance(model, rig, file.data(), entity, withHitbox);
        Location at = entity.getLocation();
        at.setPitch(0f);
        if (!made.build(at)) {
            return null;
        }
        return made;
    }

    /** One copy of a model in the world. */
    final class Instance {
        final String model;
        private final Rig rig;
        private final Map<String, Object> data;
        private final Entity follow;
        private final boolean withHitbox;
        private final List<ItemDisplay> parts = new ArrayList<>();
        private Interaction hitbox;
        private ItemDisplay seat;
        private ScheduledTask task;
        private volatile String playing;
        private volatile long started;
        private long ticks;
        private int lastFrame = Integer.MIN_VALUE;
        private String lastAnimation;
        private volatile boolean removed;
        /** Set by {@link #finish}: stays where it is, plays its last animation and goes. */
        private volatile boolean finishing;

        private Instance(String model, Rig rig, Map<String, Object> data, Entity follow, boolean withHitbox) {
            this.model = model;
            this.rig = rig;
            this.data = data;
            this.follow = follow;
            this.withHitbox = withHitbox;
            this.playing = idle();
        }

        private String idle() {
            String name = StudioText.text(StudioText.object(data, "animations"), "idle", "");
            return rig.animations().containsKey(name) ? name : null;
        }

        /** A role (idle, walk...) or an animation's own name. */
        private String resolve(String name) {
            String byRole = StudioText.text(StudioText.object(data, "animations"), name, "");
            if (rig.animations().containsKey(byRole)) {
                return byRole;
            }
            return rig.animations().containsKey(name) ? name : null;
        }

        private boolean build(Location at) {
            ItemStack[] items = new ItemStack[rig.parts().size()];
            try {
                for (int index = 0; index < items.length; index++) {
                    ItemStack stack = new ItemStack(Material.PAPER);
                    ItemMeta meta = stack.getItemMeta();
                    meta.setItemModel(NamespacedKey.fromString(rig.parts().get(index).item()));
                    stack.setItemMeta(meta);
                    items[index] = stack;
                }
            } catch (LinkageError tooOld) {
                report(model, "Models need a Minecraft 1.21.4 or later server.");
                return false;
            }
            boolean hidden = "permission".equals(StudioText.text(data, "visibility", "everyone"));
            World world = at.getWorld();
            for (int index = 0; index < items.length; index++) {
                ItemStack stack = items[index];
                Transformation pose = pose(rig.rest(), index);
                ItemDisplay part = world.spawn(at, ItemDisplay.class, made -> {
                    made.setPersistent(false);
                    made.getPersistentDataContainer().set(key, PersistentDataType.STRING, model);
                    made.setItemStack(stack);
                    made.setItemDisplayTransform(ItemDisplay.ItemDisplayTransform.NONE);
                    made.setTransformation(pose);
                    made.setInterpolationDuration(1);
                    made.setTeleportDuration(1);
                    made.setVisibleByDefault(!hidden);
                });
                parts.add(part);
                byEntity.put(part.getUniqueId(), this);
            }
            double[] box = withHitbox ? hitboxOf(data) : null;
            if (box != null) {
                hitbox = world.spawn(at, Interaction.class, made -> {
                    made.setPersistent(false);
                    made.getPersistentDataContainer().set(key, PersistentDataType.STRING, model);
                    made.setInteractionWidth((float) box[0]);
                    made.setInteractionHeight((float) box[1]);
                    made.setResponsive(true);
                    made.setVisibleByDefault(!hidden);
                });
                byEntity.put(hitbox.getUniqueId(), this);
            }
            float[] spot = withHitbox ? rig.seat() : null;
            if (spot != null && spot.length == 3) {
                seat = world.spawn(seatAt(at, spot), ItemDisplay.class, made -> {
                    made.setPersistent(false);
                    made.getPersistentDataContainer().set(key, PersistentDataType.STRING, model);
                    made.setTeleportDuration(1);
                });
                byEntity.put(seat.getUniqueId(), this);
            }
            live.add(this);
            if (follow != null) {
                Instance before = following.put(follow.getUniqueId(), this);
                if (before != null && before != this) {
                    before.remove();
                }
            }
            started = 0;
            task = parts.get(0).getScheduler().runAtFixedRate(plugin, running -> step(), this::remove, 1L, 1L);
            return true;
        }

        private static Location seatAt(Location at, float[] spot) {
            // The rig is in model space; turn the seat by the model's yaw like the displays turn.
            double yaw = Math.toRadians(-at.getYaw());
            double x = spot[0] * Math.cos(yaw) + spot[2] * Math.sin(yaw);
            double z = -spot[0] * Math.sin(yaw) + spot[2] * Math.cos(yaw);
            return at.clone().add(x, spot[1], z);
        }

        boolean alive() {
            return !removed && !parts.isEmpty() && parts.get(0).isValid();
        }

        /** Plays an animation or role from its start. Unknown names are ignored. Any thread. */
        void play(String name) {
            String found = resolve(name);
            if (found != null) {
                playing = found;
                started = -1;
            }
        }

        /** Ticks an animation (or role) lasts, 0 when the model has none by that name. */
        long length(String name) {
            String found = resolve(name);
            Animation animation = found == null ? null : rig.animations().get(found);
            return animation == null ? 0 : animation.frames().size();
        }

        /** Back to idle (or the rest pose). */
        void stopAnimation() {
            playing = idle();
            started = -1;
        }

        /**
         * For a mob: walk while it moves, idle while it stands. Leaves a one-off animation (attack, hurt...) playing
         * until it ends. Any thread.
         */
        void moving(boolean moving) {
            if (finishing) {
                return;
            }
            String idle = idle();
            String walk = resolve("walk");
            String now = playing;
            if (!java.util.Objects.equals(now, idle) && !java.util.Objects.equals(now, walk)) {
                return;
            }
            String want = moving && walk != null ? walk : idle;
            if (!java.util.Objects.equals(want, now)) {
                playing = want;
                started = -1;
            }
        }

        /**
         * Stops following (its entity died), plays a role or animation once where it is, then goes; goes at once when
         * the model has no such animation. Any thread.
         */
        void finish(String name) {
            if (removed) {
                return;
            }
            finishing = true;
            String found = resolve(name);
            Animation animation = found == null ? null : rig.animations().get(found);
            if (animation == null || animation.frames().isEmpty() || parts.isEmpty()) {
                remove();
                return;
            }
            playing = found;
            started = -1;
            long length = Math.max(1, Math.min(20L * 30, animation.frames().size() + 1L));
            ItemDisplay first = parts.get(0);
            if (first.getScheduler().runDelayed(plugin, running -> remove(), this::remove, length) == null) {
                remove();
            }
        }

        /** Every tick on the first part's thread. */
        private void step() {
            if (removed) {
                return;
            }
            ticks++;
            if (started < 0) {
                started = ticks;
                lastFrame = Integer.MIN_VALUE;
            }
            if (follow != null && !finishing) {
                if (!follow.isValid()) {
                    remove();
                    return;
                }
                Location at = follow.getLocation();
                at.setPitch(0f);
                for (ItemDisplay part : parts) {
                    part.teleportAsync(at);
                }
                if (hitbox != null) {
                    hitbox.teleportAsync(at);
                }
                if (seat != null && rig.seat() != null) {
                    seat.teleportAsync(seatAt(at, rig.seat()));
                }
            }
            String name = playing;
            Animation animation = name == null ? null : rig.animations().get(name);
            float[] frame = rig.rest();
            int index = -1;
            if (animation != null && !animation.frames().isEmpty()) {
                int count = animation.frames().size();
                long elapsed = ticks - started;
                switch (animation.loop()) {
                    case "loop" -> index = (int) (elapsed % Math.max(1, count - 1));
                    case "hold" -> index = (int) Math.min(elapsed, count - 1);
                    default -> {
                        if (elapsed >= count && finishing) {
                            return; // Holds the last frame until it goes.
                        }
                        if (elapsed >= count) {
                            String back = idle();
                            playing = back;
                            started = -1;
                            return;
                        }
                        index = (int) elapsed;
                    }
                }
                frame = animation.frames().get(index);
            }
            if (index == lastFrame && java.util.Objects.equals(name, lastAnimation)) {
                return;
            }
            lastFrame = index;
            lastAnimation = name;
            for (int at = 0; at < parts.size(); at++) {
                ItemDisplay part = parts.get(at);
                part.setInterpolationDelay(0);
                part.setInterpolationDuration(1);
                part.setTransformation(pose(frame, at));
            }
        }

        /** Shows permission-only models to players who have the permission (every second). */
        private void showByPermission() {
            if (removed || !"permission".equals(StudioText.text(data, "visibility", "everyone"))) {
                return;
            }
            String permission = StudioText.text(data, "permission", "");
            List<Entity> all = new ArrayList<>(parts);
            if (hitbox != null) {
                all.add(hitbox);
            }
            for (Player player : Bukkit.getOnlinePlayers()) {
                player.getScheduler().run(plugin, running -> {
                    boolean may = !permission.isEmpty() && player.hasPermission(permission);
                    for (Entity one : all) {
                        if (may) {
                            player.showEntity(plugin, one);
                        } else {
                            player.hideEntity(plugin, one);
                        }
                    }
                }, null);
            }
        }

        void click(Player player) {
            if (seat != null && seat.isValid() && seat.getPassengers().isEmpty()) {
                seat.addPassenger(player);
            }
            StudioActions.Run run = new StudioActions.Run(player, Map.of("model", model));
            actions.run(data.get("clickActions"), run);
        }

        /** Takes the model out of the world. Any thread. */
        void remove() {
            if (removed) {
                return;
            }
            removed = true;
            live.remove(this);
            placed.remove(model, this);
            if (follow != null) {
                following.remove(follow.getUniqueId(), this);
            }
            if (task != null) {
                task.cancel();
            }
            List<Entity> all = new ArrayList<>(parts);
            if (hitbox != null) {
                all.add(hitbox);
            }
            if (seat != null) {
                all.add(seat);
            }
            for (Entity one : all) {
                byEntity.remove(one.getUniqueId());
                if (one.isValid()) {
                    one.getScheduler().run(plugin, running -> one.remove(), null);
                }
            }
        }

        /** When the plugin turns off: right here, nothing can be scheduled any more. */
        private void removeNow() {
            removed = true;
            List<Entity> all = new ArrayList<>(parts);
            if (hitbox != null) {
                all.add(hitbox);
            }
            if (seat != null) {
                all.add(seat);
            }
            for (Entity one : all) {
                try {
                    if (one.isValid()) {
                        one.remove();
                    }
                } catch (RuntimeException otherThread) {
                    // Folia: not this region's entity. It isn't saved, so it goes with its chunk.
                }
            }
        }
    }

    /** The part's transformation in a frame: 10 numbers each (translation, rotation x y z w, scale). */
    static Transformation pose(float[] frame, int part) {
        int at = part * 10;
        if (frame.length < at + 10) {
            return new Transformation(new Vector3f(), new Quaternionf(), new Vector3f(1f, 1f, 1f), new Quaternionf());
        }
        return new Transformation(
                new Vector3f(frame[at], frame[at + 1], frame[at + 2]),
                new Quaternionf(frame[at + 3], frame[at + 4], frame[at + 5], frame[at + 6]),
                new Vector3f(frame[at + 7], frame[at + 8], frame[at + 9]),
                new Quaternionf());
    }

    /** {width, height} in blocks, or null for no hitbox. Same rules as {@code studio_format::model::hitbox}. */
    static double[] hitboxOf(Map<String, Object> data) {
        Map<String, Object> box = StudioText.object(data, "hitbox");
        double width = box.get("width") instanceof Double one ? Math.max(0, Math.min(16, one)) : 0;
        double height = box.get("height") instanceof Double one ? Math.max(0, Math.min(16, one)) : 0;
        return width > 0 && height > 0 ? new double[] {width, height} : null;
    }

    @EventHandler(priority = EventPriority.HIGH, ignoreCancelled = true)
    public void onInteract(PlayerInteractEntityEvent event) {
        Instance one = byEntity.get(event.getRightClicked().getUniqueId());
        if (one == null) {
            return;
        }
        event.setCancelled(true);
        if (event.getHand() != EquipmentSlot.HAND) {
            return;
        }
        one.click(event.getPlayer());
    }

    private void report(String id, String problem) {
        if (!problem.equals(reported.put(id, problem))) {
            studio.problem("model", id, problem);
        }
    }

    /** When the plugin turns off. */
    void stop() {
        models = List.of();
        for (Instance one : live) {
            one.removeNow();
        }
        live.clear();
        placed.clear();
        byEntity.clear();
        following.clear();
        reported.clear();
        if (current == this) {
            current = null;
        }
    }
}

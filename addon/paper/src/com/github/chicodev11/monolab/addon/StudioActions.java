package com.github.chicodev11.monolab.addon;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.time.Duration;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Random;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.BiConsumer;
import java.util.function.BiFunction;
import java.util.function.Function;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.title.Title;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Particle;
import org.bukkit.Registry;
import org.bukkit.World;
import org.bukkit.attribute.AttributeInstance;
import org.bukkit.entity.AbstractArrow;
import org.bukkit.entity.Arrow;
import org.bukkit.entity.Damageable;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.Fireball;
import org.bukkit.entity.LargeFireball;
import org.bukkit.entity.LivingEntity;
import org.bukkit.entity.Player;
import org.bukkit.entity.Projectile;
import org.bukkit.entity.SmallFireball;
import org.bukkit.entity.Snowball;
import org.bukkit.entity.Trident;
import org.bukkit.entity.WitherSkull;
import org.bukkit.event.entity.CreatureSpawnEvent;
import org.bukkit.inventory.ItemStack;
import org.bukkit.plugin.Plugin;
import org.bukkit.potion.PotionEffect;
import org.bukkit.potion.PotionEffectType;
import org.bukkit.util.Vector;

/**
 * What studio commands, menus, items and rules do: a list of actions ("send a message", "play a sound",
 * "give an item"...) run for a player, and conditions that decide whether they run. Variables (per player
 * and for the whole server) live in {@code studio/data/variables.json}.
 */
final class StudioActions {
    /**
     * Text placeholders for one run: {@code <arg1>}, {@code <message>}... Values are plain text. A mob's skill (M5) also
     * names its {@code caster} (the mob) and the {@code target} entity the actions run on; elsewhere both are null and
     * entity actions use the player.
     */
    record Run(Player player, Map<String, String> extra, Entity caster, Entity target) {
        Run(Player player, Map<String, String> extra) {
            this(player, extra, null, null);
        }

        Run with(String key, String value) {
            Map<String, String> more = new HashMap<>(extra);
            more.put(key, value == null ? "" : value);
            return new Run(player, more, caster, target);
        }

        /** Who entity actions (damage, heal, potion...) act on: the skill's target, else the player. */
        Entity subject() {
            return target != null ? target : player;
        }

        /** Who fires projectiles and pushes away: the mob in a skill, else the player. */
        Entity source() {
            return caster != null ? caster : player;
        }
    }

    static final String[] ACTIONS = {
        "message", "broadcast", "title", "actionbar", "sound", "command", "teleport", "give", "menu", "close", "set", "wait",
        "dialog", "advancement", "loot", "animation", "damage", "heal", "potion", "launch", "particle", "projectile", "summon",
        "enchant", "cosmetic", "emote", "trade", "function", "quest", "structure", "shop", "money",
        "if", "repeat", "each_player", "cancel", "stop", "take", "xp", "gamemode", "kick", "block", "fill", "bossbar", "food",
        "health", "set_permission", "set_group", "manage_world", "region"
    };
    static final String[] PROJECTILES = {"arrow", "snowball", "fireball", "small_fireball", "wither_skull", "trident"};
    /** Set while a skill's {@code damage} action hurts something, so that hit doesn't start more skills. */
    private static final ThreadLocal<Boolean> SKILL_HIT = ThreadLocal.withInitial(() -> false);
    static final String[] CONDITIONS = {"permission", "world", "variable", "chance", "online", "holding", "advancement", "quest", "money",
        "has_item", "gamemode", "health", "level", "sneaking", "time", "weather", "group"};
    /** A repeat runs its steps this many times at most. */
    private static final int MOST_REPEATS = 100;
    /** Steps one run may go through in all, counting each branch and round. */
    private static final int MOST_RUN = 2000;
    /** Set by a {@code cancel} step while a cancellable event's rules run on this thread (StudioRules reads it). */
    static final ThreadLocal<boolean[]> CANCEL = new ThreadLocal<>();
    /** A step put in front of a repeat's round: the rest of the run sees {@code <index>} as that round. */
    private static final String ROUND = "\0round";
    /** A run may not wait longer than ten minutes in all, nor chain more than this many actions. */
    private static final int MOST_WAIT = 20 * 60 * 10;
    private static final int MOST_STEPS = 200;

    private final Plugin plugin;
    private final BiConsumer<String, Throwable> warn;
    private final Path variablesFile;
    private final Map<String, Object> serverVars = new ConcurrentHashMap<>();
    private final Map<UUID, Map<String, Object>> playerVars = new ConcurrentHashMap<>();
    private final Random random = new Random();
    private volatile boolean dirty;
    private volatile Function<String, ItemStack> items = id -> null;
    private volatile BiConsumer<Player, String> menus = (player, id) -> {};
    /** Actions and conditions other studio features add (open a dialog, grant an advancement...), by type. */
    private final Map<String, BiConsumer<Map<String, Object>, Run>> moreActions = new ConcurrentHashMap<>();
    private final Map<String, java.util.function.BiPredicate<Map<String, Object>, Run>> moreConditions = new ConcurrentHashMap<>();

    StudioActions(Plugin plugin, Path studioRoot, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.warn = warn;
        this.variablesFile = studioRoot.resolve("data").resolve("variables.json");
        load();
        StudioText.variables = this::variables;
    }

    /** Where studio items come from (by id) and how menus open. */
    void connect(Function<String, ItemStack> items, BiConsumer<Player, String> menus) {
        this.items = items;
        this.menus = menus;
    }

    /** Another feature's action type; it runs on whatever thread the run is on, so it hops to the player's itself. */
    void action(String type, BiConsumer<Map<String, Object>, Run> step) {
        moreActions.put(type, step);
    }

    void condition(String type, java.util.function.BiPredicate<Map<String, Object>, Run> test) {
        moreConditions.put(type, test);
    }

    // ---------- text ----------

    /** Text with our placeholders, the run's extras and {@code <var:name>} / {@code <pvar:name>}, as a component. */
    Component render(String text, Run run) {
        return StudioText.render(variables(text, run), run.player(), escaped(run));
    }

    /** The same, as plain text for commands (no MiniMessage). */
    String plain(String text, Run run) {
        String out = StudioText.fill(variables(text, run), run.player(), Map.of());
        for (Map.Entry<String, String> one : run.extra().entrySet()) {
            out = out.replace("<" + one.getKey() + ">", one.getValue());
        }
        return out;
    }

    /** Values a player typed (a dialog's inputs) go into a command only as a name, number or id. */
    static final java.util.regex.Pattern SAFE = java.util.regex.Pattern.compile("[A-Za-z0-9_.-]{0,64}");

    /**
     * A command's text with placeholders filled. {@code <input:key>} must be {@link #SAFE} (else {@code null}:
     * the command doesn't run, so nobody can type {@code @a} or a second argument into it); {@code <input:key|raw>}
     * puts it in as typed, on one line, for a plugin's last free-text argument.
     */
    String commandLine(String text, Run run) {
        String out = text == null ? "" : text;
        Map<String, String> raw = new HashMap<>();
        for (Map.Entry<String, String> one : run.extra().entrySet()) {
            if (one.getKey().startsWith("input:")) {
                if (out.contains("<" + one.getKey() + ">") && !SAFE.matcher(one.getValue()).matches()) {
                    return null;
                }
                // Put raw text in after the other placeholders, so what was typed is never read as one.
                String mark = "\u0000" + raw.size() + "\u0000";
                if (out.contains("<" + one.getKey() + "|raw>")) {
                    out = out.replace("<" + one.getKey() + "|raw>", mark);
                    raw.put(mark, one.getValue());
                }
            }
        }
        out = plain(out, run);
        for (Map.Entry<String, String> one : raw.entrySet()) {
            out = out.replace(one.getKey(), one.getValue());
        }
        return out;
    }

    private static Map<String, String> escaped(Run run) {
        Map<String, String> out = new HashMap<>();
        for (Map.Entry<String, String> one : run.extra().entrySet()) {
            out.put(one.getKey(), StudioText.plain(one.getValue()));
        }
        return out;
    }

    private String variables(String text, Run run) {
        return variables(text, run.player());
    }

    /** {@code <var:name>} and, with a player, {@code <pvar:name>} filled in. */
    String variables(String text, Player player) {
        if (text == null || text.indexOf("var:") < 0) {
            return text == null ? "" : text;
        }
        StringBuilder out = new StringBuilder();
        int at = 0;
        while (at < text.length()) {
            int open = text.indexOf('<', at);
            if (open < 0) {
                break;
            }
            int close = text.indexOf('>', open);
            if (close < 0) {
                break;
            }
            String tag = text.substring(open + 1, close);
            String value = null;
            if (tag.startsWith("var:")) {
                value = show(serverVars.get(tag.substring(4)));
            } else if (tag.startsWith("pvar:") && player != null) {
                value = show(mine(player).get(tag.substring(5)));
            }
            if (value == null) {
                out.append(text, at, close + 1);
            } else {
                out.append(text, at, open).append(value);
            }
            at = close + 1;
        }
        return out.append(text.substring(Math.min(at, text.length()))).toString();
    }

    private static String show(Object value) {
        if (value == null) {
            return "0";
        }
        if (value instanceof Double number && number == Math.rint(number) && Math.abs(number) < 1e15) {
            return String.valueOf(number.longValue());
        }
        return String.valueOf(value);
    }

    // ---------- conditions ----------

    /** True when every condition holds (an empty list always holds). */
    boolean test(Object conditions, Run run) {
        if (!(conditions instanceof List<?> list)) {
            return true;
        }
        for (Object raw : list) {
            if (raw instanceof Map<?, ?> one && !holds(map(one), run)) {
                return false;
            }
        }
        return true;
    }

    private boolean holds(Map<String, Object> one, Run run) {
        boolean result = check(one, run);
        return Boolean.TRUE.equals(one.get("negate")) != result;
    }

    private boolean check(Map<String, Object> one, Run run) {
        Player player = run.player();
        String type = StudioText.text(one, "type", "");
        return switch (type) {
            case "permission" -> player != null && player.hasPermission(StudioText.text(one, "permission", ""));
            case "world" -> {
                List<String> worlds = StudioText.lines(one, "worlds");
                worlds.removeIf(String::isBlank);
                yield player != null && (worlds.isEmpty() || worlds.contains(player.getWorld().getName()));
            }
            case "variable" -> compare(
                    lookup(StudioText.text(one, "variable", ""), StudioText.text(one, "scope", "player"), player),
                    StudioText.text(one, "op", "="),
                    plain(StudioText.scalar(one, "value", ""), run));
            case "chance" -> random.nextDouble() * 100 < number(one.get("percent"), 50);
            case "online" -> compare((double) Bukkit.getOnlinePlayers().size(), StudioText.text(one, "op", ">="), StudioText.scalar(one, "value", "1"));
            case "holding" -> player != null && holding(player, StudioText.text(one, "item", ""));
            default -> {
                if (StudioExpansions.external(type)) {
                    yield StudioExpansions.condition(type, one, player, text -> plain(text, run));
                }
                java.util.function.BiPredicate<Map<String, Object>, Run> more = moreConditions.get(type);
                yield more == null || more.test(one, run);
            }
        };
    }

    private boolean holding(Player player, String wanted) {
        ItemStack hand = player.getInventory().getItemInMainHand();
        if (wanted.isBlank()) {
            return !hand.getType().isAir();
        }
        if (wanted.startsWith("studio:")) {
            return wanted.substring(7).equals(StudioItems.idOf(plugin, hand));
        }
        Material material = Material.matchMaterial(wanted);
        return material != null && hand.getType() == material;
    }

    private static boolean compare(Object left, String op, String right) {
        Double a = left instanceof Double number ? number : parse(String.valueOf(left));
        Double b = parse(right);
        if (a != null && b != null) {
            int order = Double.compare(a, b);
            return switch (op) {
                case "!=" -> order != 0;
                case ">" -> order > 0;
                case "<" -> order < 0;
                case ">=" -> order >= 0;
                case "<=" -> order <= 0;
                default -> order == 0;
            };
        }
        boolean same = String.valueOf(left == null ? "" : left).equalsIgnoreCase(right);
        return op.equals("!=") != same;
    }

    private static Double parse(String text) {
        try {
            return Double.parseDouble(text.trim());
        } catch (RuntimeException notNumber) {
            return null;
        }
    }

    private static double number(Object value, double fallback) {
        if (value instanceof Double one) {
            return one;
        }
        Double parsed = value instanceof String text ? parse(text) : null;
        return parsed == null ? fallback : parsed;
    }

    // ---------- actions ----------

    /** Runs the actions in order on the right thread; a {@code wait} puts the rest off. Returns the types it started. */
    List<String> run(Object actions, Run run) {
        List<Map<String, Object>> steps = new ArrayList<>();
        // Functions go in place of their step now (studio_format::function::expand), so a wait inside one puts the
        // rest of the whole run off.
        expand(actions, steps, 0);
        List<String> started = new ArrayList<>();
        for (Map<String, Object> one : steps) {
            started.add(StudioText.text(one, "type", "?"));
        }
        continueAt(steps, 0, run, 0);
        return started;
    }

    /** {@code then}/{@code else} of an if, or a loop's body, with functions put in place. */
    private List<Map<String, Object>> branch(Object actions) {
        List<Map<String, Object>> out = new ArrayList<>();
        expand(actions, out, 0);
        return out;
    }

    /** Functions inside functions, at most (that also stops one that runs itself). */
    private static final int MOST_DEPTH = 8;
    private volatile Map<String, Map<String, Object>> functions = Map.of();

    /** Reads the {@code function} files: named action lists other lists run with {@code {"type": "function"}}. */
    void functions(Studio studio) {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "function";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                Map<String, Map<String, Object>> next = new HashMap<>();
                for (Studio.Resource one : resources) {
                    Object list = one.data().get("actions");
                    if (!(list instanceof List<?> steps) || steps.isEmpty()) {
                        studio.problem("function", one.id(), "A function needs \"actions\": the steps it runs.");
                    } else if (steps.stream().anyMatch(step -> step instanceof Map<?, ?> map && "function".equals(map.get("type")) && one.id().equals(map.get("function")))) {
                        studio.problem("function", one.id(), "It runs itself; that stops after 8 rounds.");
                    }
                    next.put(one.id(), one.data());
                }
                functions = Map.copyOf(next);
            }
        });
    }

    private void expand(Object actions, List<Map<String, Object>> out, int depth) {
        if (!(actions instanceof List<?> list)) {
            return;
        }
        for (Object raw : list) {
            if (out.size() >= MOST_STEPS) {
                return;
            }
            if (!(raw instanceof Map<?, ?> one)) {
                continue;
            }
            Map<String, Object> step = map(one);
            if (!"function".equals(step.get("type"))) {
                out.add(step);
            } else if (depth < MOST_DEPTH) {
                Map<String, Object> function = functions.get(StudioText.text(step, "function", "").trim());
                if (function != null) {
                    expand(function.get("actions"), out, depth + 1);
                }
            }
        }
    }

    private void continueAt(List<Map<String, Object>> steps, int from, Run run, int waited) {
        for (int at = from; at < steps.size(); at++) {
            Map<String, Object> one = steps.get(at);
            String kind = StudioText.text(one, "type", "");
            // Flow: an if or a repeat puts its steps in front of the rest, so a wait inside one waits for all after it.
            if ("if".equals(kind) || "repeat".equals(kind)) {
                List<Map<String, Object>> next = new ArrayList<>();
                if ("if".equals(kind)) {
                    next.addAll(branch(test(one.get("if"), run) ? one.get("then") : one.get("else")));
                } else {
                    List<Map<String, Object>> body = branch(one.get("actions"));
                    int times = StudioText.number(one, "times", 3, 0, MOST_REPEATS);
                    for (int round = 1; round <= times && !body.isEmpty(); round++) {
                        next.add(Map.of("type", ROUND, "index", String.valueOf(round)));
                        next.addAll(body);
                    }
                    if (!body.isEmpty() && times > 0) {
                        next.add(Map.of("type", ROUND, "index", run.extra().getOrDefault("index", "")));
                    }
                }
                next.addAll(steps.subList(at + 1, steps.size()));
                if (next.size() > MOST_RUN) {
                    next = new ArrayList<>(next.subList(0, MOST_RUN));
                }
                continueAt(next, 0, run, waited);
                return;
            }
            if (ROUND.equals(kind)) {
                continueAt(steps, at + 1, run.with("index", StudioText.text(one, "index", "")), waited);
                return;
            }
            if ("stop".equals(kind)) {
                return;
            }
            if ("cancel".equals(kind)) {
                boolean[] cancel = CANCEL.get();
                if (cancel != null) {
                    cancel[0] = true;
                }
                continue;
            }
            if ("each_player".equals(kind)) {
                Object body = one.get("actions");
                for (Player each : Bukkit.getOnlinePlayers()) {
                    Run theirs = new Run(each, run.extra(), run.caster(), run.target());
                    each.getScheduler().run(plugin, task -> run(body, theirs), null);
                }
                continue;
            }
            if ("wait".equals(kind)) {
                int ticks = (int) Math.max(1, Math.min(MOST_WAIT - waited, number(one.get("ticks"), 20)));
                if (waited + ticks > MOST_WAIT) {
                    return;
                }
                int next = at + 1;
                Runnable rest = () -> continueAt(steps, next, run, waited + ticks);
                if (run.player() != null) {
                    run.player().getScheduler().runDelayed(plugin, task -> rest.run(), null, ticks);
                } else {
                    Bukkit.getGlobalRegionScheduler().runDelayed(plugin, task -> rest.run(), ticks);
                }
                return;
            }
            try {
                step(one, run);
            } catch (RuntimeException error) {
                warn.accept("A studio action failed: " + StudioText.text(one, "type", "?"), error);
            }
        }
    }

    private void step(Map<String, Object> one, Run run) {
        Player player = run.player();
        String type = StudioText.text(one, "type", "");
        switch (type) {
            case "message" -> onPlayer(player, () -> player.sendMessage(render(StudioText.text(one, "text", ""), run)));
            case "broadcast" -> {
                Component text = render(StudioText.text(one, "text", ""), run);
                for (Player each : Bukkit.getOnlinePlayers()) {
                    each.getScheduler().run(plugin, task -> each.sendMessage(text), null);
                }
                Bukkit.getConsoleSender().sendMessage(text);
            }
            case "title" -> onPlayer(player, () -> player.showTitle(Title.title(
                    render(StudioText.text(one, "title", ""), run),
                    render(StudioText.text(one, "subtitle", ""), run),
                    Title.Times.times(Duration.ofMillis(250), Duration.ofMillis(50L * StudioText.number(one, "stayTicks", 60, 10, 20 * 30)), Duration.ofMillis(250)))));
            case "actionbar" -> onPlayer(player, () -> player.sendActionBar(render(StudioText.text(one, "text", ""), run)));
            case "sound" -> onPlayer(player, () -> {
                String sound = StudioText.text(one, "sound", "entity.experience_orb.pickup").toLowerCase(Locale.ROOT);
                player.playSound(player.getLocation(), sound, (float) number(one.get("volume"), 1), (float) number(one.get("pitch"), 1));
            });
            case "command" -> {
                String line = commandLine(StudioText.text(one, "command", ""), run);
                if (line == null) {
                    onPlayer(player, () -> player.sendMessage(Component.text("That can't go into a command. Use letters, numbers, _ . and - only.")));
                    return;
                }
                line = line.trim();
                if (line.startsWith("/")) {
                    line = line.substring(1);
                }
                if (line.isBlank() || line.indexOf('\n') >= 0) {
                    return;
                }
                String command = line;
                if ("player".equals(StudioText.text(one, "as", "console")) && player != null) {
                    onPlayer(player, () -> player.performCommand(command));
                } else {
                    Bukkit.getGlobalRegionScheduler().execute(plugin, () -> Bukkit.dispatchCommand(Bukkit.getConsoleSender(), command));
                }
            }
            case "teleport" -> onPlayer(player, () -> {
                World world = Bukkit.getWorld(StudioText.text(one, "world", player.getWorld().getName()));
                if (world == null) {
                    return;
                }
                Location to = new Location(world, number(one.get("x"), 0), number(one.get("y"), 64), number(one.get("z"), 0),
                        (float) number(one.get("yaw"), 0), (float) number(one.get("pitch"), 0));
                player.teleportAsync(to);
            });
            case "give" -> onPlayer(player, () -> {
                ItemStack stack = stack(StudioText.text(one, "item", ""), StudioText.number(one, "amount", 1, 1, 64 * 36));
                if (stack == null) {
                    return;
                }
                for (ItemStack left : player.getInventory().addItem(stack).values()) {
                    player.getWorld().dropItemNaturally(player.getLocation(), left);
                }
            });
            case "enchant" -> onPlayer(player, () -> {
                StudioEnchants enchants = StudioEnchants.get();
                if (enchants != null) {
                    enchants.set(player.getInventory().getItemInMainHand(), StudioText.text(one, "enchant", "").trim(), StudioText.number(one, "level", 1, 0, 255), player);
                }
            });
            case "trade" -> onPlayer(player, () -> {
                StudioTrades trades = StudioTrades.get();
                if (trades != null) {
                    trades.open(player, StudioText.text(one, "trade", "").trim());
                }
            });
            case "emote" -> onPlayer(player, () -> {
                StudioModels models = StudioModels.get();
                String model = StudioText.text(one, "model", "").trim();
                if (models != null && !model.isEmpty()) {
                    models.emote(player, model, StudioText.text(one, "animation", "emote").trim());
                }
            });
            case "cosmetic" -> onPlayer(player, () -> {
                StudioCosmetics cosmetics = StudioCosmetics.get();
                if (cosmetics != null) {
                    cosmetics.act(player, StudioText.text(one, "cosmetic", "").trim(), StudioText.text(one, "mode", "toggle"));
                }
            });
            case "menu" -> onPlayer(player, () -> menus.accept(player, StudioText.text(one, "menu", "")));
            case "close" -> onPlayer(player, player::closeInventory);
            case "set" -> {
                String name = StudioText.text(one, "variable", "");
                if (name.isBlank() || name.length() > 64) {
                    return;
                }
                String scope = StudioText.text(one, "scope", "player");
                if ("player".equals(scope) && player == null) {
                    return;
                }
                Map<String, Object> vars = "server".equals(scope) ? serverVars : mine(player);
                String value = plain(StudioText.scalar(one, "value", ""), run);
                String op = StudioText.text(one, "op", "set");
                vars.put(name, change(vars.get(name), op, value));
                dirty = true;
            }
            case "damage", "heal", "potion", "launch", "particle", "projectile", "summon" -> entityStep(type, one, run);
            default -> {
                if (StudioExpansions.external(type)) {
                    StudioExpansions.action(type, one, player, text -> plain(text, run));
                    return;
                }
                BiConsumer<Map<String, Object>, Run> more = moreActions.get(type);
                if (more != null) {
                    more.accept(one, run);
                }
                // Unknown actions are skipped, so newer files still half-work on an older add-on.
            }
        }
    }

    private static Object change(Object was, String op, String value) {
        Double given = parse(value);
        if ("set".equals(op)) {
            if (given != null) {
                return given;
            }
            return value;
        }
        Double before = was instanceof Double number ? number : (was == null ? null : parse(String.valueOf(was)));
        double now = before == null ? 0 : before;
        double by = given == null ? 0 : given;
        return "subtract".equals(op) ? now - by : now + by;
    }

    // ---------- entity actions (M5) ----------

    /** True while a skill's own {@code damage} action is hurting something (on that thread). */
    static boolean bySkill() {
        return SKILL_HIT.get();
    }

    /** damage, heal, potion, launch, particle, projectile and summon: on the run's target, else its player. */
    private void entityStep(String type, Map<String, Object> one, Run run) {
        Entity subject = run.subject();
        Entity source = run.source();
        if (subject == null) {
            return;
        }
        switch (type) {
            case "damage" -> onEntity(subject, () -> {
                if (!(subject instanceof Damageable hurt)) {
                    return;
                }
                double amount = Math.max(0, Math.min(2048, number(one.get("amount"), 4)));
                Entity by = run.caster() != null && run.caster() != subject && run.caster().isValid() ? run.caster() : null;
                SKILL_HIT.set(true);
                try {
                    if (by != null) {
                        hurt.damage(amount, by);
                    } else {
                        hurt.damage(amount);
                    }
                } finally {
                    SKILL_HIT.set(false);
                }
            });
            case "heal" -> onEntity(subject, () -> {
                if (!(subject instanceof LivingEntity living) || living.isDead()) {
                    return;
                }
                double amount = Math.max(0, Math.min(2048, number(one.get("amount"), 4)));
                AttributeInstance max = StudioMobs.attributeOf(living, "health");
                double most = max == null ? living.getHealth() + amount : max.getValue();
                living.setHealth(Math.max(0, Math.min(most, living.getHealth() + amount)));
            });
            case "potion" -> onEntity(subject, () -> {
                PotionEffectType effect = effect(StudioText.text(one, "effect", "speed"));
                if (effect == null || !(subject instanceof LivingEntity living)) {
                    return;
                }
                int seconds = StudioText.number(one, "seconds", 5, 1, 60 * 60);
                int level = StudioText.number(one, "level", 1, 1, 256);
                living.addPotionEffect(new PotionEffect(effect, seconds * 20, level - 1));
            });
            case "launch" -> onEntity(subject, () -> {
                double up = Math.max(-4, Math.min(4, number(one.get("up"), 0.8)));
                double forward = Math.max(-4, Math.min(4, number(one.get("forward"), 0)));
                Vector way = null;
                if (run.caster() != null && run.caster() != subject && run.caster().getWorld().equals(subject.getWorld())) {
                    // In a skill: away from the mob.
                    way = subject.getLocation().toVector().subtract(run.caster().getLocation().toVector()).setY(0);
                }
                if (way == null || way.lengthSquared() < 1e-6) {
                    way = subject.getLocation().getDirection().setY(0);
                }
                if (way.lengthSquared() < 1e-6) {
                    way = new Vector(0, 0, 0);
                } else {
                    way.normalize().multiply(forward);
                }
                subject.setVelocity(new Vector(way.getX(), up, way.getZ()));
            });
            case "particle" -> atSpot(subject, at -> {
                Particle particle = particle(StudioText.text(one, "particle", "flame"));
                if (particle == null) {
                    return;
                }
                int count = StudioText.number(one, "count", 10, 1, 200);
                double height = subject.isValid() ? subject.getHeight() : 1;
                at.getWorld().spawnParticle(particle, at.clone().add(0, height / 2, 0), count, 0.3, height / 3, 0.3, 0.02);
            });
            case "projectile" -> {
                if (source instanceof LivingEntity shooter) {
                    onEntity(shooter, () -> shoot(shooter, subject == shooter ? null : subject, one));
                }
            }
            case "summon" -> atSpot(subject, at -> {
                String mob = StudioText.text(one, "mob", "").trim();
                if (mob.isEmpty()) {
                    return;
                }
                int count = StudioText.number(one, "count", 1, 1, 20);
                java.util.concurrent.ThreadLocalRandom dice = java.util.concurrent.ThreadLocalRandom.current();
                for (int made = 0; made < count; made++) {
                    Location spot = at.clone().add(count > 1 ? dice.nextDouble(-1, 1) : 0, 0, count > 1 ? dice.nextDouble(-1, 1) : 0);
                    if (StudioMobs.has(mob)) {
                        StudioMobs.summon(mob, spot);
                        continue;
                    }
                    EntityType vanilla = StudioNpcs.type(mob);
                    if (vanilla != null) {
                        spot.getWorld().spawnEntity(spot, vanilla, CreatureSpawnEvent.SpawnReason.CUSTOM);
                    }
                }
            });
            default -> {
                // Not an entity action.
            }
        }
    }

    /** Fires from the shooter toward the target (its middle), or where the shooter looks. On the shooter's thread. */
    private static void shoot(LivingEntity shooter, Entity target, Map<String, Object> one) {
        Class<? extends Projectile> kind = switch (StudioText.text(one, "projectile", "arrow").trim().toLowerCase(Locale.ROOT)) {
            case "snowball" -> Snowball.class;
            case "fireball" -> LargeFireball.class;
            case "small_fireball" -> SmallFireball.class;
            case "wither_skull" -> WitherSkull.class;
            case "trident" -> Trident.class;
            default -> Arrow.class;
        };
        double speed = Math.max(0.1, Math.min(5, number(one.get("speed"), 1.5)));
        Location eyes = shooter.getEyeLocation();
        Vector way = null;
        if (target != null && target.isValid() && target.getWorld().equals(shooter.getWorld())) {
            way = target.getLocation().add(0, target.getHeight() / 2, 0).toVector().subtract(eyes.toVector());
        }
        if (way == null || way.lengthSquared() < 1e-6) {
            way = eyes.getDirection();
        }
        Vector velocity = way.normalize().multiply(speed);
        Projectile fired = shooter.launchProjectile(kind, velocity);
        if (fired instanceof Fireball fireball) {
            fireball.setDirection(velocity.clone().normalize());
            fireball.setVelocity(velocity);
        }
        if (fired instanceof AbstractArrow arrow) {
            arrow.setPickupStatus(AbstractArrow.PickupStatus.DISALLOWED);
        }
    }

    /** A vanilla effect by its key ({@code speed}, {@code minecraft:regeneration}). */
    static PotionEffectType effect(String name) {
        NamespacedKey key = NamespacedKey.fromString(StudioLoot.clean(name));
        if (key == null) {
            return null;
        }
        try {
            return Registry.EFFECT.get(key);
        } catch (RuntimeException | LinkageError missing) {
            return null;
        }
    }

    /** A vanilla particle by its key ({@code flame}); only ones that need no extra data (colour, block...). */
    static Particle particle(String name) {
        String key = StudioLoot.clean(name);
        for (Particle one : Particle.values()) {
            try {
                if (one.getKey().getKey().equals(key)) {
                    return one.getDataType() == Void.class ? one : null;
                }
            } catch (RuntimeException | LinkageError skipped) {
                // A particle this server can't name.
            }
        }
        return null;
    }

    /** On an entity's own thread; nothing when it is gone. */
    void onEntity(Entity entity, Runnable task) {
        if (entity == null || !entity.isValid()) {
            return;
        }
        entity.getScheduler().run(plugin, scheduled -> task.run(), null);
    }

    /** Where an entity is, on that spot's thread; a dead mob (a death skill) still has its last spot. */
    private void atSpot(Entity entity, java.util.function.Consumer<Location> task) {
        if (entity.isValid()) {
            entity.getScheduler().run(plugin, scheduled -> task.accept(entity.getLocation()), null);
            return;
        }
        Location last = entity.getLocation();
        if (last.getWorld() != null) {
            Bukkit.getRegionScheduler().execute(plugin, last, () -> task.accept(last));
        }
    }

    void onPlayer(Player player, Runnable task) {
        if (player == null || !player.isOnline()) {
            return;
        }
        player.getScheduler().run(plugin, scheduled -> task.run(), null);
    }

    /** A studio item ({@code studio:<id>}) or a material ({@code diamond}). */
    ItemStack stack(String item, int amount) {
        ItemStack stack;
        if (item.startsWith("studio:")) {
            // studio:item/<id> is the same as studio:<id> (mobs' equipment names items that way).
            String ref = item.substring(7);
            ItemStack made = items.apply(ref.startsWith("item/") ? ref.substring(5) : ref);
            stack = made == null ? null : made.clone();
        } else {
            Material material = Material.matchMaterial(item);
            stack = material == null || !material.isItem() || material.isAir() ? null : new ItemStack(material);
        }
        if (stack != null) {
            stack.setAmount(Math.max(1, Math.min(amount, stack.getMaxStackSize() * 36)));
        }
        return stack;
    }

    // ---------- variables ----------

    /** A player variable as a number: 0 when it isn't set or isn't a number (shops keep money there). */
    double playerNumber(Player player, String name) {
        Object value = mine(player).get(name);
        Double number = value instanceof Double one ? one : (value == null ? null : parse(String.valueOf(value)));
        return number == null ? 0 : number;
    }

    void setPlayerNumber(Player player, String name, double value) {
        mine(player).put(name, value);
        dirty = true;
    }

    private Map<String, Object> mine(Player player) {
        return playerVars.computeIfAbsent(player.getUniqueId(), id -> new ConcurrentHashMap<>());
    }

    private Object lookup(String name, String scope, Player player) {
        if ("server".equals(scope)) {
            return serverVars.get(name);
        }
        return player == null ? null : mine(player).get(name);
    }

    @SuppressWarnings("unchecked")
    private void load() {
        try {
            if (!Files.isRegularFile(variablesFile)) {
                return;
            }
            Object parsed = MiniJson.parse(Files.readString(variablesFile, StandardCharsets.UTF_8));
            if (!(parsed instanceof Map<?, ?> all)) {
                return;
            }
            if (all.get("server") instanceof Map<?, ?> server) {
                ((Map<String, Object>) server).forEach((key, value) -> {
                    if (value != null) {
                        serverVars.put(key, value);
                    }
                });
            }
            if (all.get("players") instanceof Map<?, ?> players) {
                ((Map<String, Object>) players).forEach((id, values) -> {
                    try {
                        UUID uuid = UUID.fromString(id);
                        if (values instanceof Map<?, ?> map) {
                            Map<String, Object> mine = new ConcurrentHashMap<>();
                            ((Map<String, Object>) map).forEach((key, value) -> {
                                if (value != null) {
                                    mine.put(key, value);
                                }
                            });
                            playerVars.put(uuid, mine);
                        }
                    } catch (IllegalArgumentException skipped) {
                        // Not a player id.
                    }
                });
            }
        } catch (IOException | RuntimeException error) {
            warn.accept("Could not read studio/data/variables.json; starting with empty variables.", error);
        }
    }

    /** Writes the variables when they changed. Runs off the server thread. */
    void save() {
        if (!dirty) {
            return;
        }
        dirty = false;
        StringBuilder players = new StringBuilder("{");
        for (Map.Entry<UUID, Map<String, Object>> one : playerVars.entrySet()) {
            if (one.getValue().isEmpty()) {
                continue;
            }
            if (players.length() > 1) {
                players.append(',');
            }
            players.append(quote(one.getKey().toString())).append(':').append(values(one.getValue()));
        }
        players.append('}');
        String text = new Json().put("schema", Studio.SCHEMA).raw("server", values(serverVars)).raw("players", players.toString()).end();
        try {
            Files.createDirectories(variablesFile.getParent());
            Path temp = variablesFile.resolveSibling("variables.json.tmp");
            Files.writeString(temp, text, StandardCharsets.UTF_8);
            Files.move(temp, variablesFile, StandardCopyOption.REPLACE_EXISTING);
        } catch (IOException error) {
            dirty = true;
            warn.accept("Could not save studio/data/variables.json", error);
        }
    }

    private static String values(Map<String, Object> vars) {
        StringBuilder out = new StringBuilder("{");
        for (Map.Entry<String, Object> one : vars.entrySet()) {
            if (out.length() > 1) {
                out.append(',');
            }
            out.append(quote(one.getKey())).append(':');
            out.append(one.getValue() instanceof Double number ? show(number) : quote(String.valueOf(one.getValue())));
        }
        return out.append('}').toString();
    }

    private static String quote(String text) {
        return new Json().put("v", text).end().replaceFirst("^\\{\"v\":", "").replaceFirst("}$", "");
    }

    @SuppressWarnings("unchecked")
    static Map<String, Object> map(Map<?, ?> raw) {
        return (Map<String, Object>) raw;
    }

    /** Checks an action list's shape, for status.json. {@code null} = fine. */
    static String problem(Object actions, BiFunction<String, String, Boolean> known) {
        if (actions == null) {
            return null;
        }
        if (!(actions instanceof List<?> list)) {
            return "Actions must be a list.";
        }
        for (Object raw : list) {
            if (!(raw instanceof Map<?, ?> one) || !(one.get("type") instanceof String type)) {
                return "Every action needs a \"type\".";
            }
            if ("menu".equals(type) && one.get("menu") instanceof String menu && !known.apply("menu", menu)) {
                return "There is no menu called " + menu + ".";
            }
            if ("give".equals(type) && one.get("item") instanceof String item && item.startsWith("studio:")) {
                // studio:<item>, or studio:block/<id> and studio:furniture/<id> for their own items.
                String ref = item.substring(7);
                int slash = ref.indexOf('/');
                String kind = slash > 0 ? ref.substring(0, slash) : "item";
                String id = slash > 0 ? ref.substring(slash + 1) : ref;
                if (!known.apply(kind, id)) {
                    return "There is no studio " + kind + " called " + id + ".";
                }
            }
            if (("animation".equals(type) || "emote".equals(type)) && one.get("model") instanceof String model && !model.isEmpty() && !known.apply("model", model)) {
                return "There is no studio model called " + model + ".";
            }
            if ("function".equals(type) && one.get("function") instanceof String function && !known.apply("function", function.trim())) {
                return "There is no function called " + function.trim() + ".";
            }
            if ("summon".equals(type) && one.get("mob") instanceof String mob && !mob.isBlank()
                    && StudioNpcs.type(mob) == null && !known.apply("mob", mob.trim())) {
                return "There is no studio mob or vanilla mob called " + mob.trim() + ".";
            }
            if ("projectile".equals(type) && one.get("projectile") instanceof String kind && !List.of(PROJECTILES).contains(kind)) {
                return "\"projectile\" must be one of: " + String.join(", ", PROJECTILES) + ".";
            }
            for (String kind : new String[] {"dialog", "loot", "advancement", "enchant", "cosmetic", "trade", "shop"}) {
                if (kind.equals(type) && one.get(kind) instanceof String id && !known.apply(kind, id)) {
                    return "There is no studio " + kind + " called " + id + ".";
                }
            }
        }
        return null;
    }
}

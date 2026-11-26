package com.github.chicodev11.monolab.addon;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeSet;
import java.util.UUID;
import java.util.concurrent.Executor;
import java.util.function.Function;
import java.util.regex.Pattern;

/**
 * Lets other plugins and mods show live values and buttons in the Monolab app's Integrations tab.
 *
 * <pre>
 * MonolabApi.status("Shop", "sales", "Sales today", 12);
 * MonolabApi.status("Shop", "balance", "Bank", 1520.5, "coins");
 * MonolabApi.action("Shop", "Give money", args -> {
 *     economy.give(args.get("player"), Double.parseDouble(args.get("amount")));
 *     return "Gave " + args.get("amount");
 * }, "player:player:Player", "number:amount:Amount");
 * </pre>
 *
 * Owner is usually the plugin or mod name. Values show up within a few seconds. A button calls your handler on the
 * server thread when a person presses it in the app; nothing goes through the console. Declare Monolab as a (soft)
 * dependency so this class is on your classpath.
 *
 * <p>The studio (Paper) can use your plugin too. Names are "plugin:name" (a-z, 0-9, _ and -, one colon); others
 * are ignored. Describe the fields in plugins/&lt;YourPlugin&gt;/monolab-studio.json and the app draws a form for them.
 *
 * <pre>
 * MonolabApi.studioAction("Shop", "shop:give_money", (player, fields) -&gt; economy.give(player, Double.parseDouble(fields.get("amount"))));
 * MonolabApi.studioCondition("Shop", "shop:has_money", (player, fields) -&gt; economy.has(player, Double.parseDouble(fields.get("amount"))));
 * MonolabApi.studioPlaceholder("Shop", "shop:balance", player -&gt; String.valueOf(economy.balance(player)));   // &lt;ext:shop:balance&gt;
 * MonolabApi.studioTrigger("Shop", "shop:buy", player, Map.of("item", "apple", "price", "5"));             // rules with when.event "shop:buy"
 * </pre>
 */
public final class MonolabApi {
    /** Runs when the button is pressed. Arguments are already checked against their declared types. */
    @FunctionalInterface
    public interface Handler {
        /** Returns a short message for the app, or null. Throwing shows the error message there instead. */
        String run(Map<String, String> args) throws Exception;
    }

    /** A studio action ("shop:give_money"). {@code player} is null when the studio runs it for the server. */
    @FunctionalInterface
    public interface StudioAction {
        /** {@code fields} are the step's other keys as text, placeholders already filled. Throwing is logged. */
        void run(UUID player, Map<String, String> fields) throws Exception;
    }

    /** A studio condition ("shop:has_money"). Throwing counts as false. */
    @FunctionalInterface
    public interface StudioCondition {
        boolean test(UUID player, Map<String, String> fields) throws Exception;
    }

    /** Where {@link #studioTrigger} goes: the platform's studio sets it (Paper's StudioExpansions). */
    @FunctionalInterface
    interface StudioTriggers {
        void fire(String event, UUID player, Map<String, String> vars);
    }

    private static final int TEXT = 200;
    /** "plugin:name": lower-case a-z, 0-9, _ and -, one colon. */
    private static final Pattern STUDIO_NAME = Pattern.compile("[a-z0-9_-]{1,32}:[a-z0-9_-]{1,64}");
    private static final Pattern VAR_NAME = Pattern.compile("[A-Za-z0-9_.-]{1,64}");
    private static volatile StudioTriggers studioTriggers = (event, player, vars) -> {};
    private static final int PER_OWNER = 50;
    private static final Pattern PLAYER = Pattern.compile("[A-Za-z0-9_]{1,16}");
    private static final Map<String, Owner> owners = new LinkedHashMap<>();
    private static boolean changed = true;
    private static long writtenAt;
    private static Executor serverThread = Runnable::run;

    private record Status(String key, String label, String value, String unit) {}

    private record Action(String label, Handler handler, List<String[]> args) {}

    private static final class Owner {
        final Map<String, Status> status = new LinkedHashMap<>();
        final Map<String, Action> actions = new LinkedHashMap<>();
        final Map<String, StudioAction> studioActions = new LinkedHashMap<>();
        final Map<String, StudioCondition> studioConditions = new LinkedHashMap<>();
        final Map<String, Function<UUID, String>> studioPlaceholders = new LinkedHashMap<>();
        final List<String> studioTriggers = new ArrayList<>();

        boolean shown() {
            return !status.isEmpty() || !actions.isEmpty();
        }
    }

    private MonolabApi() {}

    /** Shows (or updates) one value under the owner's tab. */
    public static void status(String owner, String key, String label, Object value) {
        status(owner, key, label, value, "");
    }

    /** Shows (or updates) one value with a unit, such as "coins" or "%". */
    public static synchronized void status(String owner, String key, String label, Object value, String unit) {
        Owner one = owner(owner);
        if (one == null || blank(key)) {
            return;
        }
        if (!one.status.containsKey(key) && one.status.size() >= PER_OWNER) {
            return;
        }
        Status next = new Status(cut(key), cut(blank(label) ? key : label), cut(String.valueOf(value)), cut(unit == null ? "" : unit));
        if (!next.equals(one.status.put(key, next))) {
            changed = true;
        }
    }

    /**
     * Adds a button that calls the handler when pressed in the app. Arguments are "type:name:label" with type
     * player, number or text (the label is optional); the app asks for each one before calling.
     */
    public static synchronized void action(String owner, String label, Handler handler, String... args) {
        Owner one = owner(owner);
        if (one == null || blank(label) || handler == null) {
            return;
        }
        if (!one.actions.containsKey(label) && one.actions.size() >= PER_OWNER) {
            return;
        }
        List<String[]> parsed = new ArrayList<>();
        for (String arg : args) {
            String[] parts = arg == null ? new String[0] : arg.split(":", 3);
            if (parts.length < 2 || blank(parts[1])) {
                continue;
            }
            String type = switch (parts[0]) {
                case "player", "number" -> parts[0];
                default -> "text";
            };
            parsed.add(new String[] {cut(parts[1]), cut(parts.length > 2 ? parts[2] : parts[1]), type});
        }
        Action previous = one.actions.put(cut(label), new Action(cut(label), handler, parsed));
        if (previous == null || previous.args().size() != parsed.size()) {
            changed = true;
        }
    }

    // ---------- studio ----------

    /** Adds a studio action type, used in rules, commands, items... as {@code {"type": "shop:give_money", ...}}. */
    public static synchronized void studioAction(String owner, String type, StudioAction handler) {
        Owner one = studioOwner(owner, type, handler);
        if (one == null || (!one.studioActions.containsKey(type) && one.studioActions.size() >= PER_OWNER)) {
            return;
        }
        if (one.studioActions.put(type, handler) == null) {
            changed = true;
        }
    }

    /** Adds a studio condition type, used in a rule's "if" as {@code {"type": "shop:has_money", ...}}. */
    public static synchronized void studioCondition(String owner, String type, StudioCondition test) {
        Owner one = studioOwner(owner, type, test);
        if (one == null || (!one.studioConditions.containsKey(type) && one.studioConditions.size() >= PER_OWNER)) {
            return;
        }
        if (one.studioConditions.put(type, test) == null) {
            changed = true;
        }
    }

    /** Adds a placeholder for studio text, written {@code <ext:shop:balance>}. {@code player} may be null. */
    public static synchronized void studioPlaceholder(String owner, String key, Function<UUID, String> value) {
        Owner one = studioOwner(owner, key, value);
        if (one == null || (!one.studioPlaceholders.containsKey(key) && one.studioPlaceholders.size() >= PER_OWNER)) {
            return;
        }
        if (one.studioPlaceholders.put(key, value) == null) {
            changed = true;
        }
    }

    /**
     * Runs the studio rules whose {@code when.event} is {@code event} ("shop:buy"). {@code player} may be null;
     * {@code vars} become placeholders in those rules ({@code <item>}, {@code <price>}). Safe from any thread.
     */
    public static void studioTrigger(String owner, String event, UUID player, Map<String, String> vars) {
        if (!noteTrigger(owner, event)) {
            return;
        }
        Map<String, String> clean = new LinkedHashMap<>();
        if (vars != null) {
            for (Map.Entry<String, String> one : vars.entrySet()) {
                if (clean.size() >= PER_OWNER) {
                    break;
                }
                if (one.getKey() != null && VAR_NAME.matcher(one.getKey()).matches()) {
                    clean.put(one.getKey(), one.getValue() == null ? "" : cut(one.getValue()));
                }
            }
        }
        try {
            studioTriggers.fire(event, player, Map.copyOf(clean));
        } catch (RuntimeException | LinkageError ignored) {
            // The studio reports its own trouble; a plugin's call never fails because of it.
        }
    }

    private static synchronized boolean noteTrigger(String owner, String event) {
        Owner one = studioOwner(owner, event, Boolean.TRUE);
        if (one == null) {
            return false;
        }
        if (!one.studioTriggers.contains(event)) {
            if (one.studioTriggers.size() >= PER_OWNER) {
                return false;
            }
            one.studioTriggers.add(event);
            changed = true;
        }
        return true;
    }

    private static Owner studioOwner(String owner, String name, Object given) {
        if (given == null || name == null || !STUDIO_NAME.matcher(name).matches()) {
            return null;
        }
        return owner(owner);
    }

    /** True for a "plugin:name" studio type, placeholder or event. */
    static boolean studioName(String name) {
        return name != null && STUDIO_NAME.matcher(name).matches();
    }

    /** The platform's studio hands over where triggers go. */
    static void studioTriggers(StudioTriggers sink) {
        studioTriggers = sink == null ? (event, player, vars) -> {} : sink;
    }

    /** The handler for a studio action type, or null. Call it outside this class's lock (this returns first). */
    static synchronized StudioAction studioActionFor(String type) {
        for (Owner one : owners.values()) {
            StudioAction found = one.studioActions.get(type);
            if (found != null) {
                return found;
            }
        }
        return null;
    }

    static synchronized StudioCondition studioConditionFor(String type) {
        for (Owner one : owners.values()) {
            StudioCondition found = one.studioConditions.get(type);
            if (found != null) {
                return found;
            }
        }
        return null;
    }

    static synchronized Function<UUID, String> studioPlaceholderFor(String key) {
        for (Owner one : owners.values()) {
            Function<UUID, String> found = one.studioPlaceholders.get(key);
            if (found != null) {
                return found;
            }
        }
        return null;
    }

    /** {"actions":[...],"conditions":[...],"placeholders":[...],"triggers":[...]}: what is registered now. */
    private static String studioJson() {
        TreeSet<String> actions = new TreeSet<>();
        TreeSet<String> conditions = new TreeSet<>();
        TreeSet<String> placeholders = new TreeSet<>();
        TreeSet<String> triggers = new TreeSet<>();
        for (Owner one : owners.values()) {
            actions.addAll(one.studioActions.keySet());
            conditions.addAll(one.studioConditions.keySet());
            placeholders.addAll(one.studioPlaceholders.keySet());
            triggers.addAll(one.studioTriggers);
        }
        return new Json()
                .put("actions", new ArrayList<>(actions))
                .put("conditions", new ArrayList<>(conditions))
                .put("placeholders", new ArrayList<>(placeholders))
                .put("triggers", new ArrayList<>(triggers))
                .end();
    }

    /** Removes everything this owner showed, for example when the plugin disables. */
    public static synchronized void clear(String owner) {
        if (owner != null && owners.remove(owner) != null) {
            changed = true;
        }
    }

    /** The platform hands over how to run work on the server thread. */
    static void serverThread(Executor executor) {
        serverThread = executor;
    }

    /** The integrations.json text when it should be written: after a change, and every 30 seconds so the app knows it's fresh. */
    static synchronized String takeJson() {
        long now = System.currentTimeMillis();
        if (!changed && now - writtenAt < 30_000) {
            return null;
        }
        changed = false;
        writtenAt = now;
        StringBuilder out = new StringBuilder("{\"at\":").append(now).append(",\"owners\":{");
        boolean firstOwner = true;
        for (Map.Entry<String, Owner> entry : owners.entrySet()) {
            if (!entry.getValue().shown()) {
                // Only studio names: no tab for it in Integrations.
                continue;
            }
            if (!firstOwner) {
                out.append(',');
            }
            firstOwner = false;
            out.append(quote(entry.getKey())).append(":{\"status\":[");
            boolean head = true;
            for (Status one : entry.getValue().status.values()) {
                if (!head) {
                    out.append(',');
                }
                head = false;
                out.append(new Json().put("key", one.key()).put("label", one.label()).put("value", one.value()).put("unit", one.unit()).end());
            }
            out.append("],\"actions\":[");
            head = true;
            for (Action one : entry.getValue().actions.values()) {
                if (!head) {
                    out.append(',');
                }
                head = false;
                StringBuilder args = new StringBuilder("[");
                for (String[] arg : one.args()) {
                    if (args.length() > 1) {
                        args.append(',');
                    }
                    args.append(new Json().put("name", arg[0]).put("label", arg[1]).put("type", arg[2]).end());
                }
                out.append(new Json().put("label", one.label()).raw("args", args.append(']').toString()).end());
            }
            out.append("]}");
        }
        return out.append("},\"studio\":").append(studioJson()).append('}').toString();
    }

    /** Picks up button presses the app left in actions/ and answers each one in a matching .result.json. */
    static void serveRequests(Path folder) {
        Path inbox = folder.resolve("actions");
        if (!Files.isDirectory(inbox)) {
            return;
        }
        List<Path> found;
        try (var listing = Files.list(inbox)) {
            found = listing.filter(path -> {
                String name = path.getFileName().toString();
                return name.startsWith("req-") && name.endsWith(".json") && !name.endsWith(".result.json");
            }).toList();
        } catch (IOException trouble) {
            return;
        }
        for (Path request : found) {
            String name = request.getFileName().toString();
            Path answer = inbox.resolve(name.substring(0, name.length() - ".json".length()) + ".result.json");
            String text;
            try {
                text = Files.readString(request, StandardCharsets.UTF_8);
                Files.deleteIfExists(request);
            } catch (IOException trouble) {
                continue;
            }
            Prepared prepared = prepare(text);
            if (prepared.problem != null) {
                answer(answer, false, prepared.problem);
                continue;
            }
            serverThread.execute(() -> {
                try {
                    String message = prepared.handler.run(prepared.args);
                    answer(answer, true, message == null ? "" : message);
                } catch (Exception | LinkageError trouble) {
                    answer(answer, false, trouble.getMessage() == null ? trouble.getClass().getSimpleName() : trouble.getMessage());
                }
            });
        }
    }

    private record Prepared(Handler handler, Map<String, String> args, String problem) {}

    private static synchronized Prepared prepare(String text) {
        if (!(MiniJson.parse(text) instanceof Map<?, ?> request)) {
            return new Prepared(null, null, "The request is malformed.");
        }
        Owner owner = owners.get(String.valueOf(request.get("owner")));
        Action action = owner == null ? null : owner.actions.get(String.valueOf(request.get("label")));
        if (action == null) {
            return new Prepared(null, null, "This button is no longer offered by the plugin.");
        }
        Map<?, ?> given = request.get("args") instanceof Map<?, ?> map ? map : Map.of();
        Map<String, String> args = new LinkedHashMap<>();
        for (String[] arg : action.args()) {
            String value = given.get(arg[0]) == null ? "" : String.valueOf(given.get(arg[0])).trim();
            boolean fine = switch (arg[2]) {
                case "player" -> PLAYER.matcher(value).matches();
                case "number" -> value.matches("-?\\d+(\\.\\d+)?");
                default -> !value.isEmpty() && value.length() <= TEXT && value.indexOf('\n') < 0 && value.indexOf('\r') < 0;
            };
            if (!fine) {
                return new Prepared(null, null, "Check the value for " + arg[1] + ".");
            }
            args.put(arg[0], value);
        }
        return new Prepared(action.handler(), args, null);
    }

    private static void answer(Path file, boolean ok, String message) {
        try {
            Files.writeString(file, new Json().put("ok", ok).put("message", cut(message)).end(), StandardCharsets.UTF_8);
        } catch (IOException ignored) {
            // The app stops waiting after a while.
        }
    }

    private static Owner owner(String name) {
        if (blank(name)) {
            return null;
        }
        String key = cut(name);
        if (!owners.containsKey(key) && owners.size() >= PER_OWNER) {
            return null;
        }
        return owners.computeIfAbsent(key, ignored -> new Owner());
    }

    private static boolean blank(String text) {
        return text == null || text.isBlank();
    }

    private static String cut(String text) {
        String clean = text.replace('\n', ' ').replace('\r', ' ').trim();
        return clean.length() > TEXT ? clean.substring(0, TEXT) : clean;
    }

    private static String quote(String text) {
        String inner = new Json().put("v", text).end();
        return inner.substring(inner.indexOf(':') + 1, inner.length() - 1);
    }
}

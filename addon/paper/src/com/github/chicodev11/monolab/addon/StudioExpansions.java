package com.github.chicodev11.monolab.addon;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.BiConsumer;
import java.util.function.Function;
import java.util.function.UnaryOperator;
import org.bukkit.Bukkit;
import org.bukkit.entity.Player;
import org.bukkit.plugin.Plugin;

/**
 * Studio M6: what other plugins add to the studio through {@link MonolabApi} — action and condition types
 * ("shop:give_money"), placeholders ({@code <ext:shop:balance>}) and rule events ("shop:buy"). Studio steps whose
 * "type" has a colon come here; their other keys go to the plugin as text, placeholders filled.
 */
final class StudioExpansions {
    /** The same trouble is logged at most once a minute per type. */
    private static final long QUIET_MS = 60_000;
    private static final int MOST_FIELDS = 50;
    private static final int TEXT = 200;
    private static volatile Plugin plugin;
    private static volatile BiConsumer<String, Throwable> warn = (message, error) -> {};
    private static final Map<String, Long> warned = new ConcurrentHashMap<>();

    private StudioExpansions() {}

    /** Hands {@link MonolabApi#studioTrigger} on to the rules. Call once the rules exist. */
    static void install(Plugin owner, StudioRules rules, BiConsumer<String, Throwable> warnings) {
        plugin = owner;
        warn = warnings;
        MonolabApi.studioTriggers(rules::fireExternal);
    }

    /** On disable: plugin triggers go nowhere again. */
    static void uninstall() {
        MonolabApi.studioTriggers(null);
    }

    /** A plugin's type ("shop:give_money"), not one of the studio's own. */
    static boolean external(String type) {
        return type != null && type.indexOf(':') > 0;
    }

    /**
     * Runs a plugin's action on the player's thread (the server's when there is no player). {@code fill} fills
     * placeholders in the step's texts (StudioActions#plain for the run). False when no plugin offers the type
     * (the step is skipped, like an unknown one) or there is no player online to run it for.
     */
    static boolean action(String type, Map<String, Object> step, Player player, UnaryOperator<String> fill) {
        MonolabApi.StudioAction handler = MonolabApi.studioActionFor(type);
        if (handler == null) {
            quietly(type, "No plugin offers the studio action " + type + " (is it installed and enabled?).", null);
            return false;
        }
        Map<String, String> fields = fields(step, fill);
        UUID id = player == null ? null : player.getUniqueId();
        Runnable call = () -> {
            try {
                handler.run(id, fields);
            } catch (Exception | LinkageError error) {
                quietly(type, "The studio action " + type + " failed", error);
            }
        };
        Plugin owner = plugin;
        if (owner == null) {
            return false;
        }
        if (player != null) {
            if (!player.isOnline()) {
                return false;
            }
            player.getScheduler().run(owner, task -> call.run(), null);
        } else {
            Bukkit.getGlobalRegionScheduler().execute(owner, call);
        }
        return true;
    }

    /** A plugin's condition, tested here and now. A type no plugin offers, or one that throws, is false. */
    static boolean condition(String type, Map<String, Object> step, Player player, UnaryOperator<String> fill) {
        MonolabApi.StudioCondition test = MonolabApi.studioConditionFor(type);
        if (test == null) {
            quietly(type, "No plugin offers the studio condition " + type + " (is it installed and enabled?).", null);
            return false;
        }
        try {
            return test.test(player == null ? null : player.getUniqueId(), fields(step, fill));
        } catch (Exception | LinkageError error) {
            quietly(type, "The studio condition " + type + " failed", error);
            return false;
        }
    }

    /** {@code <ext:shop:balance>}'s value as plain text, or null when no plugin offers it. */
    static String placeholder(String key, Player player) {
        if (!MonolabApi.studioName(key)) {
            return null;
        }
        Function<UUID, String> value = MonolabApi.studioPlaceholderFor(key);
        if (value == null) {
            return null;
        }
        try {
            String text = value.apply(player == null ? null : player.getUniqueId());
            return text == null ? "" : cut(text);
        } catch (RuntimeException | LinkageError error) {
            quietly(key, "The studio placeholder " + key + " failed", error);
            return "";
        }
    }

    /** Every key but "type" (and a condition's "negate"), as text: texts filled, whole numbers without ".0". */
    private static Map<String, String> fields(Map<String, Object> step, UnaryOperator<String> fill) {
        Map<String, String> out = new LinkedHashMap<>();
        for (Map.Entry<String, Object> one : step.entrySet()) {
            String key = one.getKey();
            if ("type".equals(key) || "negate".equals(key) || out.size() >= MOST_FIELDS) {
                continue;
            }
            out.put(key, text(one.getValue(), fill));
        }
        return Map.copyOf(out);
    }

    private static String text(Object value, UnaryOperator<String> fill) {
        if (value == null) {
            return "";
        }
        if (value instanceof String one) {
            String filled = fill == null ? one : fill.apply(one);
            return filled == null ? "" : filled;
        }
        if (value instanceof Double number && number == Math.rint(number) && Math.abs(number) < 1e15) {
            return String.valueOf(number.longValue());
        }
        if (value instanceof List<?> list) {
            StringBuilder out = new StringBuilder();
            for (Object one : list) {
                if (out.length() > 0) {
                    out.append('\n');
                }
                out.append(text(one, fill));
            }
            return out.toString();
        }
        return String.valueOf(value);
    }

    private static String cut(String text) {
        String clean = text.replace('\n', ' ').replace('\r', ' ');
        return clean.length() > TEXT ? clean.substring(0, TEXT) : clean;
    }

    private static void quietly(String type, String message, Throwable error) {
        long now = System.currentTimeMillis();
        Long last = warned.get(type);
        if (last != null && now - last < QUIET_MS) {
            return;
        }
        warned.put(type, now);
        warn.accept(message, error);
    }
}

package com.github.chicodev11.monolab.addon;

import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.minimessage.MiniMessage;
import org.bukkit.Bukkit;
import org.bukkit.OfflinePlayer;
import org.bukkit.entity.Player;

/**
 * Studio text: MiniMessage with our placeholders ({@code <player>}, {@code <online>}...), other plugins' through
 * MonolabApi ({@code <ext:shop:balance>}) and, when installed, PlaceholderAPI's {@code %...%}. Only MiniMessage
 * methods that Adventure 4 (1.21 servers) and 5 (26.x) both have are used, so placeholders are put into the text,
 * escaped, before it is parsed.
 */
final class StudioText {
    private static final MiniMessage MINI = MiniMessage.miniMessage();
    private static Method papi;
    private static boolean papiChecked;

    private StudioText() {}

    /** Our placeholders for a player (or the server, when {@code player} is null). */
    static String fill(String text, Player player, Map<String, String> extra) {
        if (text == null || text.isEmpty()) {
            return "";
        }
        String out = text;
        if (player != null && out.indexOf('%') >= 0) {
            out = placeholderApi(player, out);
        }
        out = swap(out, "online", String.valueOf(Bukkit.getOnlinePlayers().size()));
        out = swap(out, "max", String.valueOf(Bukkit.getMaxPlayers()));
        out = swap(out, "tps", tps());
        if (player != null) {
            out = swap(out, "player", player.getName());
            out = swap(out, "world", player.getWorld().getName());
            out = swap(out, "ping", String.valueOf(player.getPing()));
        }
        out = external(out, player);
        for (Map.Entry<String, String> one : extra.entrySet()) {
            out = out.replace("<" + one.getKey() + ">", one.getValue());
        }
        return out;
    }

    /** {@code <ext:plugin:key>}: a plugin's placeholder (StudioExpansions). One no plugin offers stays as written. */
    private static String external(String text, Player player) {
        if (!text.contains("<ext:")) {
            return text;
        }
        StringBuilder out = new StringBuilder();
        int at = 0;
        while (at < text.length()) {
            int open = text.indexOf("<ext:", at);
            if (open < 0) {
                break;
            }
            int close = text.indexOf('>', open);
            if (close < 0) {
                break;
            }
            String value = StudioExpansions.placeholder(text.substring(open + 5, close), player);
            out.append(text, at, open).append(value == null ? text.substring(open, close + 1) : MINI.escapeTags(value));
            at = close + 1;
        }
        return out.append(text.substring(Math.min(at, text.length()))).toString();
    }

    private static String swap(String text, String name, String value) {
        String tag = "<" + name + ">";
        return text.contains(tag) ? text.replace(tag, MINI.escapeTags(value)) : text;
    }

    static Component render(String text, Player player) {
        return render(text, player, Map.of());
    }

    /**
     * Fills {@code <var:name>} and {@code <pvar:name>} (set by {@link StudioActions}), so boards, tab lists, boss bars
     * and holograms show variables like actions do. The player may be null (then only server variables).
     */
    static volatile java.util.function.BiFunction<String, Player, String> variables = (text, player) -> text;

    /** A field that may be text, a number or true/false (a variable's value), as text. */
    static String scalar(Map<String, Object> data, String key, String fallback) {
        Object value = data.get(key);
        if (value instanceof String one) {
            return one;
        }
        if (value instanceof Double number) {
            return number == Math.rint(number) && Math.abs(number) < 1e15 ? String.valueOf(number.longValue()) : String.valueOf(number);
        }
        if (value instanceof Boolean flag) {
            return flag.toString();
        }
        return fallback;
    }

    /**
     * {@code extra} values are MiniMessage already (see {@link #mini}). Glyph images ({@code :ruby:}) are swapped
     * in the studio's own text before placeholders are filled, so a player's name or chat line never becomes one.
     */
    static Component render(String text, Player player, Map<String, String> extra) {
        String filled = fill(StudioGlyphs.swap(variables.apply(text, player)), player, extra);
        try {
            return MINI.deserialize(filled);
        } catch (RuntimeException broken) {
            return Component.text(filled);
        }
    }

    /** A component as MiniMessage, to put into another text as an {@code extra} value. */
    static String mini(Component component) {
        return MINI.serialize(component);
    }

    /** Plain text with tags escaped, to put into another text as an {@code extra} value. */
    static String plain(String text) {
        return MINI.escapeTags(text);
    }

    private static String tps() {
        try {
            double now = Bukkit.getTPS()[0];
            return String.format(Locale.ROOT, "%.1f", Math.min(20.0, now));
        } catch (RuntimeException | NoSuchMethodError folia) {
            return "20.0";
        }
    }

    private static String placeholderApi(Player player, String text) {
        if (!papiChecked) {
            papiChecked = true;
            try {
                if (Bukkit.getPluginManager().isPluginEnabled("PlaceholderAPI")) {
                    papi = Class.forName("me.clip.placeholderapi.PlaceholderAPI").getMethod("setPlaceholders", OfflinePlayer.class, String.class);
                }
            } catch (ReflectiveOperationException | LinkageError missing) {
                papi = null;
            }
        }
        if (papi == null) {
            return text;
        }
        try {
            return (String) papi.invoke(null, player, text);
        } catch (ReflectiveOperationException | RuntimeException failed) {
            return text;
        }
    }

    // ---------- reading resource fields ----------

    static String text(Map<String, Object> data, String key, String fallback) {
        Object value = data.get(key);
        return value instanceof String one ? one : fallback;
    }

    static boolean has(Map<String, Object> data, String key) {
        return data.get(key) instanceof String;
    }

    static int number(Map<String, Object> data, String key, int fallback, int least, int most) {
        Object value = data.get(key);
        int read = value instanceof Double one ? (int) Math.round(one) : fallback;
        return Math.max(least, Math.min(most, read));
    }

    static List<String> lines(Map<String, Object> data, String key) {
        List<String> out = new ArrayList<>();
        if (data.get(key) instanceof List<?> list) {
            for (Object one : list) {
                out.add(one instanceof String text ? text : "");
            }
        } else if (data.get(key) instanceof String one) {
            out.add(one);
        }
        return out;
    }

    /** A nested object, or an empty one. */
    @SuppressWarnings("unchecked")
    static Map<String, Object> object(Map<String, Object> data, String key) {
        return data.get(key) instanceof Map<?, ?> one ? (Map<String, Object>) one : Map.of();
    }

    /** {@code show.worlds} (empty = every world) and {@code show.permission} (empty = everyone). */
    @SuppressWarnings("unchecked")
    static boolean shows(Map<String, Object> data, Player player) {
        if (!(data.get("show") instanceof Map<?, ?> raw)) {
            return true;
        }
        Map<String, Object> show = (Map<String, Object>) raw;
        List<String> worlds = lines(show, "worlds");
        worlds.removeIf(String::isBlank);
        if (!worlds.isEmpty() && !worlds.contains(player.getWorld().getName())) {
            return false;
        }
        String permission = text(show, "permission", "");
        return permission.isBlank() || player.hasPermission(permission);
    }

    /** The one with the highest {@code priority} (then the first by name) that shows for this player. */
    static Studio.Resource pick(List<Studio.Resource> resources, Player player) {
        Studio.Resource best = null;
        int top = Integer.MIN_VALUE;
        for (Studio.Resource one : resources) {
            if (player != null && !shows(one.data(), player)) {
                continue;
            }
            int priority = number(one.data(), "priority", 0, -1000, 1000);
            if (best == null || priority > top) {
                best = one;
                top = priority;
            }
        }
        return best;
    }
}

package com.github.chicodev11.monolab.addon;

import io.papermc.paper.connection.PlayerGameConnection;
import io.papermc.paper.event.player.PlayerCustomClickEvent;
import java.security.SecureRandom;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import java.util.function.BiConsumer;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.TextComponent;
import net.kyori.adventure.text.event.ClickEvent;
import net.kyori.adventure.text.event.HoverEvent;
import net.kyori.adventure.text.format.ShadowColor;
import net.kyori.adventure.text.format.Style;
import net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * Studio maker M2: canvas dialogs ({@code "layout": "canvas"}). Pictures, text and click areas at pixel spots,
 * drawn into one dialog body text with glyphs and spaces, ported from the user's DialogUI ({@code UiCanvas},
 * {@code ClickRouter}):
 * <ul>
 *   <li>The body is text in 9-pixel lines: {@code y} splits into the line and the spot in it, which picks one of
 *       the {@code monolab:canvas_0..8} fonts (the same glyphs with lower ascents).</li>
 *   <li>{@code x} is reached with spaces ({@code monolab:space}); every line is padded to the same width, and
 *       lines are centered, so they all start at the same {@code x}.</li>
 *   <li>A click on a glyph sends a custom click ({@code monolab:canvas/<token>}). Tokens are per player and
 *       thrown away when that player opens another canvas, so an old or someone else's token does nothing.</li>
 *   <li>On one line later elements draw over earlier ones; a lower line draws over the lines above it.</li>
 *   <li>{@code if} (conditions) hides an element for players it doesn't hold for. The canvas's {@code after} says
 *       what a click does: {@code close}, {@code stay} (the older {@code "close": false}) or {@code refresh} (draw it
 *       again once the actions ran). A button's {@code pressed} glyph shows for {@link #PRESSED_TICKS} ticks after a
 *       click while the dialog stays open.</li>
 * </ul>
 * Same rules as {@code crates/studio-format/src/canvas.rs} (Pumpkin) and the app's {@code StudioCanvas.tsx}.
 * Only loaded on servers with dialogs (1.21.6+).
 */
final class StudioCanvas implements Listener {
    static final int LINE = 9;
    static final int BODY_PADDING = 4;
    static final int MOST_BODY = 1024;
    static final int WRAP_SLACK = 32;
    static final int MOST_ELEMENTS = 400;
    static final List<String> TYPES = List.of("image", "text", "button", "area");
    static final List<String> AFTERS = List.of("close", "stay", "refresh");
    /** Actions that take the player somewhere else, so the canvas isn't drawn again after them. */
    static final List<String> LEAVING = List.of("dialog", "menu", "close", "shop", "trade");
    static final int PRESSED_TICKS = 3;
    static final int MOST_VARIABLES = 64;
    private static final String NAMESPACE = "monolab";
    private static final String PREFIX = "canvas/";
    private static final Style NO_SHADOW = Style.style().shadowColor(ShadowColor.none()).build();
    private static final Key SPACE = Key.key("monolab", "space");

    /** What a player's tokens run: the dialog, the element (its {@code actions}), its index and the canvas's {@code after}. */
    private record Target(String dialog, Map<String, Object> element, int index, String after) {}

    private final StudioActions actions;
    private final SecureRandom random = new SecureRandom();
    private final Map<UUID, Map<String, Target>> tokens = new ConcurrentHashMap<>();
    /** A button to draw pressed the next time this player's canvas is drawn. */
    private final Map<UUID, Integer> pressing = new ConcurrentHashMap<>();
    /** Draws a dialog again for a player, on that player's thread (set by {@link StudioDialogs}). */
    private volatile BiConsumer<Player, String> redraw;

    StudioCanvas(StudioActions actions) {
        this.actions = actions;
    }

    record Rendered(Component content, int width) {}

    void redrawWith(BiConsumer<Player, String> drawer) {
        this.redraw = drawer;
    }

    /** What a click does to the dialog: {@code after}, or the older {@code "close": false} (stay). */
    static String after(Map<String, Object> canvas) {
        Object after = canvas.get("after");
        if (after instanceof String one && AFTERS.contains(one)) {
            return one;
        }
        return Boolean.FALSE.equals(canvas.get("close")) ? "stay" : "close";
    }

    /** The leaving action types ({@link #LEAVING}) anywhere in some actions, nested ones too (not in conditions). */
    static java.util.Set<String> leaving(Object steps) {
        java.util.Set<String> out = new java.util.LinkedHashSet<>();
        walk(steps, out, 0);
        return out;
    }

    private static void walk(Object value, java.util.Set<String> out, int depth) {
        if (depth > 32) {
            return;
        }
        if (value instanceof List<?> list) {
            for (Object one : list) {
                walk(one, out, depth + 1);
            }
        } else if (value instanceof Map<?, ?> map) {
            if (map.get("type") instanceof String type && LEAVING.contains(type)) {
                out.add(type);
            }
            for (Map.Entry<?, ?> entry : map.entrySet()) {
                if (!"if".equals(entry.getKey())) {
                    walk(entry.getValue(), out, depth + 1);
                }
            }
        }
    }

    /** The glyph a button shows just after a click, or null. */
    static String pressedGlyph(Map<String, Object> element) {
        String name = StudioText.text(element, "pressed", "");
        return "button".equals(StudioText.text(element, "type", "")) && StudioGlyphs.validName(name) ? name : null;
    }

    /** What is wrong with a dialog's starting {@code variables} ({@code {"name": value}}). */
    static String variablesProblem(Map<String, Object> dialog) {
        Object value = dialog.get("variables");
        if (value == null) {
            return null;
        }
        if (!(value instanceof Map<?, ?> vars)) {
            return "\"variables\" must be an object of names and values.";
        }
        if (vars.size() > MOST_VARIABLES) {
            return "A dialog can start at most " + MOST_VARIABLES + " variables.";
        }
        for (Map.Entry<?, ?> one : vars.entrySet()) {
            String name = String.valueOf(one.getKey());
            if (name.isBlank() || name.length() > 64) {
                return "A variable's name must be 1 to 64 letters.";
            }
            if (!(one.getValue() instanceof String || one.getValue() instanceof Double || one.getValue() instanceof Boolean)) {
                return "The variable " + name + " needs a plain value (text or a number).";
            }
        }
        return null;
    }

    /** Forgets a player's canvas clicks (another, non-canvas dialog took its place). */
    void forget(Player player) {
        tokens.remove(player.getUniqueId());
        pressing.remove(player.getUniqueId());
    }

    /** The dialog's canvas object, or null when it isn't a canvas dialog. */
    static Map<String, Object> canvasOf(Map<String, Object> dialog) {
        return "canvas".equals(dialog.get("layout")) ? StudioText.object(dialog, "canvas") : null;
    }

    static List<Map<String, Object>> elements(Map<String, Object> canvas) {
        List<Map<String, Object>> out = new ArrayList<>();
        if (canvas.get("elements") instanceof List<?> list) {
            for (Object one : list) {
                if (one instanceof Map<?, ?> map) {
                    out.add(StudioActions.map(map));
                }
            }
        }
        return out;
    }

    /** What is wrong with a canvas. Glyph names are checked once the pack has a glyph table. */
    static String problem(Map<String, Object> canvas) {
        int width = StudioText.number(canvas, "width", 176, 1, 4096);
        if (width + BODY_PADDING * 2 + WRAP_SLACK > MOST_BODY) {
            return "A canvas can be at most " + (MOST_BODY - BODY_PADDING * 2 - WRAP_SLACK) + " wide.";
        }
        if (canvas.containsKey("after") && !(canvas.get("after") instanceof String after && AFTERS.contains(after))) {
            return "The canvas's \"after\" must be close, stay or refresh.";
        }
        if (!(canvas.get("elements") instanceof List<?> list)) {
            return null;
        }
        if (list.size() > MOST_ELEMENTS) {
            return "A canvas can have at most " + MOST_ELEMENTS + " elements.";
        }
        StudioGlyphs.Table glyphs = StudioGlyphs.table();
        for (int at = 0; at < list.size(); at++) {
            if (!(list.get(at) instanceof Map<?, ?> raw)) {
                return "Every canvas element must be an object.";
            }
            Map<String, Object> one = StudioActions.map(raw);
            String type = StudioText.text(one, "type", "");
            if (!TYPES.contains(type)) {
                return "Canvas element " + (at + 1) + " has no type (image, text, button or area).";
            }
            if (one.get("y") instanceof Double y && y < 0) {
                return "Canvas element " + (at + 1) + " is above the canvas (y below 0).";
            }
            if (one.containsKey("if") && !(one.get("if") instanceof List<?>)) {
                return "Canvas element " + (at + 1) + ": \"if\" must be a list of conditions.";
            }
            String pressed = StudioText.text(one, "pressed", "");
            if (type.equals("button") && !pressed.isEmpty()) {
                if (!StudioGlyphs.validName(pressed)) {
                    return "Canvas element " + (at + 1) + ": the pressed picture must be a glyph name.";
                }
                if (!glyphs.byName().isEmpty()) {
                    StudioGlyphs.Glyph glyph = glyphs.get(pressed);
                    if (glyph == null) {
                        return "There is no glyph :" + pressed + ": (make it as a glyph image, then build the pack).";
                    }
                    if (!glyph.canvas()) {
                        return ":" + pressed + ": is too small for its picture to go in a canvas. Make its height 8 or more.";
                    }
                }
            }
            if (type.equals("image") || type.equals("button")) {
                String name = StudioText.text(one, "glyph", "");
                if (!StudioGlyphs.validName(name)) {
                    return "Canvas element " + (at + 1) + " needs a glyph picture.";
                }
                if (!glyphs.byName().isEmpty()) {
                    StudioGlyphs.Glyph glyph = glyphs.get(name);
                    if (glyph == null) {
                        return "There is no glyph :" + name + ": (make it as a glyph image, then build the pack).";
                    }
                    if (!glyph.canvas()) {
                        return ":" + name + ": is too small for its picture to go in a canvas. Make its height 8 or more.";
                    }
                }
            }
            if (type.equals("area") && (StudioText.number(one, "w", 0, 0, 4096) <= 0 || StudioText.number(one, "h", 0, 0, 4096) <= 0)) {
                return "Canvas area " + (at + 1) + " needs a width and height.";
            }
            if (type.equals("button") || type.equals("area")) {
                String steps = StudioActions.problem(one.get("actions"), (kind, id) -> true);
                if (steps != null) {
                    return steps;
                }
            }
        }
        return null;
    }

    // ---------- clicks ----------

    @EventHandler
    public void onCustomClick(PlayerCustomClickEvent event) {
        Key key = event.getIdentifier();
        if (!key.namespace().equals(NAMESPACE) || !key.value().startsWith(PREFIX)) {
            return;
        }
        if (!(event.getCommonConnection() instanceof PlayerGameConnection connection)) {
            return;
        }
        Player player = connection.getPlayer();
        Map<String, Target> mine = tokens.get(player.getUniqueId());
        Target target = mine == null ? null : mine.get(key.value().substring(PREFIX.length()));
        if (target == null) {
            return;
        }
        String permission = StudioText.text(target.element(), "permission", "");
        if (!permission.isBlank() && !player.hasPermission(permission)) {
            return;
        }
        Map<String, String> extra = new HashMap<>();
        extra.put("dialog", target.dialog());
        Object steps = target.element().get("actions");
        java.util.Set<String> leaves = leaving(steps);
        boolean stays = !"close".equals(target.after()) && leaves.isEmpty();
        BiConsumer<Player, String> drawer = redraw;
        boolean pressed = stays && drawer != null && pressedGlyph(target.element()) != null;
        UUID id = player.getUniqueId();
        actions.onPlayer(player, () -> {
            if (pressed) {
                pressing.put(id, target.index());
                drawer.accept(player, target.dialog());
            }
            actions.run(steps, new StudioActions.Run(player, extra));
            if (pressed) {
                // Put the button back unless another dialog took this one's place meanwhile.
                Map<String, Target> shown = tokens.get(id);
                player.getScheduler().runDelayed(JavaPlugin.getProvidingPlugin(StudioCanvas.class), task -> {
                    if (tokens.get(id) == shown) {
                        drawer.accept(player, target.dialog());
                    }
                }, null, PRESSED_TICKS);
            } else if (stays && "refresh".equals(target.after()) && drawer != null) {
                drawer.accept(player, target.dialog());
            } else if (!"close".equals(target.after()) && leaves.equals(java.util.Set.of("close"))) {
                // The close action shuts the open dialog too when it was left open.
                player.closeDialog();
            }
        });
    }

    @EventHandler
    public void onQuit(PlayerQuitEvent event) {
        tokens.remove(event.getPlayer().getUniqueId());
        pressing.remove(event.getPlayer().getUniqueId());
    }

    private ClickEvent<?> token(Map<String, Target> issued, String dialog, Map<String, Object> element, int index, String after) {
        byte[] bytes = new byte[8];
        random.nextBytes(bytes);
        String token = HexFormat.of().formatHex(bytes);
        issued.put(token, new Target(dialog, element, index, after));
        return ClickEvent.custom(Key.key(NAMESPACE, PREFIX + token));
    }

    // ---------- drawing ----------

    private sealed interface Element permits Image, Text, Area {}

    private record Image(StudioGlyphs.Glyph glyph, int x, int y, Style style) implements Element {}

    private record Text(Component content, String plain, int x, int y, boolean shadow, int advance) implements Element {}

    private record Area(int x, int y, int w, int h, Style style) implements Element {}

    /**
     * Draws the dialog's canvas for a player, and forgets that player's older click tokens. Null when the dialog
     * isn't a canvas or is too wide to show.
     */
    Rendered render(Player player, String dialogId, Map<String, Object> data) {
        Map<String, Object> canvas = canvasOf(data);
        if (canvas == null) {
            forget(player);
            return null;
        }
        String after = after(canvas);
        Integer pressed = pressing.remove(player.getUniqueId());
        StudioActions.Run conditions = new StudioActions.Run(player, Map.of("dialog", dialogId));
        StudioGlyphs.Table glyphs = StudioGlyphs.table();
        StudioGlyphs.Widths widths = StudioGlyphs.widths();
        int width = StudioText.number(canvas, "width", 176, 1, MOST_BODY);
        int height = StudioText.number(canvas, "height", 166, 1, 2048);
        Map<String, Target> issued = new ConcurrentHashMap<>();
        tokens.put(player.getUniqueId(), issued);

        List<Element> found = new ArrayList<>();
        List<Map<String, Object>> list = elements(canvas);
        for (int at = 0; at < list.size() && at < MOST_ELEMENTS; at++) {
            Map<String, Object> one = list.get(at);
            if (one.get("if") instanceof List<?> wanted && !wanted.isEmpty() && !actions.test(wanted, conditions)) {
                continue;
            }
            String type = StudioText.text(one, "type", "");
            int x = StudioText.number(one, "x", 0, -2048, 2048);
            int y = StudioText.number(one, "y", 0, 0, 2048);
            Style style = NO_SHADOW;
            if (type.equals("button") || type.equals("area")) {
                String permission = StudioText.text(one, "permission", "");
                if (permission.isBlank() || player.hasPermission(permission)) {
                    Style.Builder clickable = style.toBuilder().clickEvent(token(issued, dialogId, one, at, after));
                    String tooltip = StudioText.text(one, "tooltip", "");
                    if (!tooltip.isBlank()) {
                        clickable.hoverEvent(HoverEvent.showText(StudioText.render(tooltip, player)));
                    }
                    style = clickable.build();
                } else if (type.equals("area")) {
                    continue;
                }
            }
            switch (type) {
                case "image", "button" -> {
                    StudioGlyphs.Glyph glyph = glyphs.get(StudioText.text(one, "glyph", ""));
                    String down = pressed != null && pressed == at ? pressedGlyph(one) : null;
                    StudioGlyphs.Glyph look = down == null ? null : glyphs.get(down);
                    if (look != null && look.canvas()) {
                        glyph = look;
                    }
                    if (glyph != null && glyph.canvas()) {
                        found.add(new Image(glyph, x, y, style));
                    }
                }
                case "text" -> {
                    Component content = StudioText.render(StudioText.text(one, "text", ""), player);
                    String plain = PlainTextComponentSerializer.plainText().serialize(content);
                    if (plain.indexOf('\n') >= 0) {
                        // A line break would push every line under it down; the canvas keeps one line.
                        content = content.replaceText(builder -> builder.matchLiteral("\n").replacement(" "));
                        plain = plain.replace('\n', ' ');
                    }
                    int advance = widths.width(plain);
                    int shown = Math.max(0, advance - 1);
                    int left = switch (StudioText.text(one, "align", "left")) {
                        case "center" -> x - shown / 2;
                        case "right" -> x - shown;
                        default -> x;
                    };
                    found.add(new Text(content, plain, left, y, !Boolean.FALSE.equals(one.get("shadow")), advance));
                }
                case "area" -> {
                    int w = StudioText.number(one, "w", 0, 0, MOST_BODY);
                    int h = StudioText.number(one, "h", 0, 0, 2048);
                    if (w > 0 && h > 0 && style.clickEvent() != null) {
                        found.add(new Area(x, y, w, h, style));
                    }
                }
                default -> {
                    // Unknown elements are skipped, so newer files still half-work.
                }
            }
        }

        int count = Math.max(1, (height + LINE - 1) / LINE);
        for (Element one : found) {
            int last = switch (one) {
                case Image image -> image.y() / LINE;
                case Text text -> textLine(widths, text);
                case Area area -> (area.y() + area.h() - 1) / LINE;
            };
            count = Math.max(count, last + 1);
        }
        LineBuilder[] lines = new LineBuilder[count];
        for (int at = 0; at < count; at++) {
            lines[at] = new LineBuilder();
        }
        int[] left = new int[count];
        int[] right = new int[count];
        java.util.Arrays.fill(left, Integer.MAX_VALUE);
        java.util.Arrays.fill(right, Integer.MIN_VALUE);
        for (Element one : found) {
            switch (one) {
                case Image image -> {
                    LineBuilder line = lines[image.y() / LINE];
                    line.moveTo(image.x());
                    line.write(canvasFont(image.y() % LINE), image.glyph().text(), image.style(), image.glyph().advance());
                }
                case Text text -> {
                    Style wrapper = Style.empty();
                    if (widths.allBitmap(text.plain()) && text.y() % LINE != 0) {
                        wrapper = wrapper.font(Key.key("monolab", "text_" + (text.y() % LINE)));
                    }
                    if (!text.shadow()) {
                        wrapper = wrapper.shadowColor(ShadowColor.none());
                    }
                    LineBuilder line = lines[textLine(widths, text)];
                    line.moveTo(text.x());
                    line.component(Component.text().style(wrapper).append(text.content()).build(), text.advance());
                }
                case Area area -> {
                    for (int at = area.y() / LINE; at <= (area.y() + area.h() - 1) / LINE; at++) {
                        lines[at].moveTo(area.x());
                        lines[at].write(SPACE, StudioGlyphs.spaces(area.w()), area.style(), area.w());
                        left[at] = Math.min(left[at], area.x());
                        right[at] = Math.max(right[at], area.x() + area.w());
                    }
                }
            }
        }
        // The game only looks for a click on spaces inside the box of the glyphs drawn on that line, so a
        // see-through glyph goes at both ends of every line with an area.
        if (glyphs.hit() != null) {
            for (int at = 0; at < count; at++) {
                if (left[at] > right[at]) {
                    continue;
                }
                lines[at].moveTo(left[at]);
                lines[at].write(canvasFont(0), glyphs.hit(), NO_SHADOW, glyphs.hitAdvance());
                lines[at].moveTo(right[at] - (glyphs.hitAdvance() - 1));
                lines[at].write(canvasFont(0), glyphs.hit(), NO_SHADOW, glyphs.hitAdvance());
            }
        }
        int lineWidth = width;
        for (LineBuilder line : lines) {
            lineWidth = Math.max(lineWidth, line.most);
        }
        int body = lineWidth + BODY_PADDING * 2 + WRAP_SLACK;
        if (body > MOST_BODY) {
            return null;
        }
        TextComponent.Builder root = Component.text();
        for (int at = 0; at < count; at++) {
            lines[at].moveTo(lineWidth);
            if (at > 0) {
                root.append(Component.newline());
            }
            root.append(lines[at].build());
        }
        return new Rendered(root.build(), body);
    }

    /** The line text lands on: pixel spots for bitmap letters, the nearest line when unifont ones (Korean...) are in. */
    private static int textLine(StudioGlyphs.Widths widths, Text text) {
        return widths.allBitmap(text.plain()) ? text.y() / LINE : Math.round(text.y() / (float) LINE);
    }

    private static Key canvasFont(int offset) {
        return Key.key("monolab", "canvas_" + Math.max(0, Math.min(LINE - 1, offset)));
    }

    /** One line: where the pen is, and runs of the same font and style merged into one part. */
    private static final class LineBuilder {
        private final List<Component> parts = new ArrayList<>();
        private final StringBuilder pending = new StringBuilder();
        private Key font;
        private Style style = Style.empty();
        private int pen;
        private int most;

        void moveTo(int x) {
            int delta = x - pen;
            if (delta != 0) {
                write(SPACE, StudioGlyphs.spaces(delta), NO_SHADOW, delta);
            }
        }

        void write(Key withFont, String text, Style withStyle, int advance) {
            if (!withFont.equals(font) || !withStyle.equals(style)) {
                flush();
                font = withFont;
                style = withStyle;
            }
            pending.append(text);
            pen += advance;
            most = Math.max(most, pen);
        }

        void component(Component component, int advance) {
            flush();
            font = null;
            parts.add(component);
            pen += advance;
            most = Math.max(most, pen);
        }

        private void flush() {
            if (pending.isEmpty() || font == null) {
                return;
            }
            parts.add(Component.text(pending.toString(), style.font(font)));
            pending.setLength(0);
        }

        Component build() {
            flush();
            return Component.textOfChildren(parts.toArray(Component[]::new));
        }
    }
}

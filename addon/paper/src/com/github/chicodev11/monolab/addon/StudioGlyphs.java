package com.github.chicodev11.monolab.addon;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.function.BiConsumer;

/**
 * Studio maker M2: glyph images. The app builds {@code studio/pack/glyphs.json} with the resource pack: each
 * {@code font} file's short name, the character it got and how wide it draws. Studio text turns {@code :ruby:}
 * into that character ({@link #swap}), and canvas dialogs ({@link StudioCanvas}) place them. The Pumpkin add-on
 * has the same rules in {@code crates/studio-format/src/glyphs.rs}; keep the two in step.
 */
final class StudioGlyphs {
    static final String GLYPH_FONT = "monolab:glyph";
    static final String SPACE_FONT = "monolab:space";
    static final int SPACE_UP = 0xE000;
    static final int SPACE_DOWN = 0xE100;
    static final int SPACE_POWERS = 8;
    static final int MOST_NAME = 32;

    record Glyph(String id, String text, int advance, int width, int height, boolean canvas) {}

    /** {@code glyphs.json}, read; {@code hit} is the see-through glyph that makes a canvas line clickable. */
    record Table(Map<String, Glyph> byName, String hit, int hitAdvance, Map<String, String> problems) {
        static final Table EMPTY = new Table(Map.of(), null, 2, Map.of());

        Glyph get(String name) {
            return name == null ? null : byName.get(name.replace(":", ""));
        }
    }

    private static volatile Path file;
    private static volatile Table table = Table.EMPTY;
    private static long checkedAt;
    private static long stamp = Long.MIN_VALUE;
    private static volatile Widths widths;

    private final Studio studio;
    private final BiConsumer<String, Throwable> warn;

    StudioGlyphs(Studio studio, Path studioRoot, BiConsumer<String, Throwable> warn) {
        this.studio = studio;
        this.warn = warn;
        file = studioRoot.resolve("pack").resolve("glyphs.json");
    }

    void register() {
        studio.add(new Studio.Feature() {
            @Override
            public String kind() {
                return "font";
            }

            @Override
            public void apply(List<Studio.Resource> resources) {
                refresh(true);
                Table now = table;
                for (Studio.Resource one : resources) {
                    String problem = now.problems().get(one.id());
                    if (problem == null && !StudioText.has(one.data(), "image")) {
                        problem = "Pick a picture from the studio's textures.";
                    }
                    if (problem != null) {
                        studio.problem("font", one.id(), problem);
                    }
                }
            }
        });
        try {
            refresh(true);
        } catch (RuntimeException error) {
            warn.accept("Could not read the studio's glyphs.json", error);
        }
    }

    /** The table, reread at most every 2 seconds when the file changed. Any thread. */
    static Table table() {
        refresh(false);
        return table;
    }

    private static synchronized void refresh(boolean now) {
        Path at = file;
        if (at == null) {
            return;
        }
        long time = System.currentTimeMillis();
        if (!now && time - checkedAt < 2000) {
            return;
        }
        checkedAt = time;
        long seen;
        try {
            seen = Files.exists(at) ? Files.getLastModifiedTime(at).toMillis() ^ Files.size(at) : Long.MIN_VALUE;
        } catch (IOException gone) {
            seen = Long.MIN_VALUE;
        }
        if (seen == stamp) {
            return;
        }
        stamp = seen;
        if (seen == Long.MIN_VALUE) {
            table = Table.EMPTY;
            return;
        }
        try {
            table = parse(Files.readString(at, StandardCharsets.UTF_8));
        } catch (IOException | RuntimeException broken) {
            table = Table.EMPTY;
        }
    }

    @SuppressWarnings("unchecked")
    static Table parse(String text) {
        if (!(MiniJson.parse(text) instanceof Map<?, ?> raw)) {
            return Table.EMPTY;
        }
        Map<String, Object> root = (Map<String, Object>) raw;
        Map<String, Glyph> glyphs = new HashMap<>();
        for (Map.Entry<String, Object> one : StudioText.object(root, "glyphs").entrySet()) {
            if (!(one.getValue() instanceof Map<?, ?> map) || !validName(one.getKey())) {
                continue;
            }
            Map<String, Object> glyph = (Map<String, Object>) map;
            String character = StudioText.text(glyph, "char", "");
            if (character.codePointCount(0, character.length()) != 1) {
                continue;
            }
            glyphs.put(one.getKey(), new Glyph(
                    StudioText.text(glyph, "id", one.getKey()),
                    character,
                    StudioText.number(glyph, "advance", 0, 0, 1024),
                    StudioText.number(glyph, "width", 0, 0, 1024),
                    StudioText.number(glyph, "height", 0, 0, 1024),
                    !Boolean.FALSE.equals(glyph.get("canvas"))));
        }
        Map<String, Object> hit = StudioText.object(root, "hit");
        Map<String, String> problems = new HashMap<>();
        for (Map.Entry<String, Object> one : StudioText.object(root, "problems").entrySet()) {
            if (one.getValue() instanceof String why) {
                problems.put(one.getKey(), why);
            }
        }
        String hitText = StudioText.text(hit, "char", "");
        return new Table(Map.copyOf(glyphs), hitText.isEmpty() ? null : hitText, StudioText.number(hit, "advance", 2, 1, 16), Map.copyOf(problems));
    }

    static boolean validName(String name) {
        return name != null && !name.isEmpty() && name.length() <= MOST_NAME && name.chars().allMatch(one -> (one >= 'a' && one <= 'z') || (one >= '0' && one <= '9') || one == '_');
    }

    /** One glyph as MiniMessage, white and plain so the text around it doesn't tint it. */
    static String glyphMini(String character) {
        return "<font:" + GLYPH_FONT + "><white><!bold><!italic><!underlined><!strikethrough><!obfuscated>" + character + "</font>";
    }

    /** {@code :ruby:} → its glyph. Unknown names and anything inside a {@code <tag>} stay; {@code \:} keeps a colon. */
    static String swap(String text) {
        if (text == null || text.indexOf(':') < 0) {
            return text;
        }
        Table now = table();
        if (now.byName().isEmpty()) {
            return text;
        }
        StringBuilder out = new StringBuilder(text.length());
        int at = 0;
        while (at < text.length()) {
            char letter = text.charAt(at);
            char next = at + 1 < text.length() ? text.charAt(at + 1) : 0;
            if (letter == '\\' && next == ':') {
                out.append(':');
                at += 2;
                continue;
            }
            if (letter == '\\' && (next == '<' || next == '\\')) {
                out.append(letter).append(next);
                at += 2;
                continue;
            }
            if (letter == '<') {
                int end = text.indexOf('>', at);
                int stop = end < 0 ? text.length() : end + 1;
                out.append(text, at, stop);
                at = stop;
                continue;
            }
            if (letter == ':') {
                int end = at + 1;
                while (end < text.length() && end - at - 1 <= MOST_NAME) {
                    char one = text.charAt(end);
                    if (!((one >= 'a' && one <= 'z') || (one >= '0' && one <= '9') || one == '_')) {
                        break;
                    }
                    end++;
                }
                if (end < text.length() && text.charAt(end) == ':') {
                    Glyph glyph = now.byName().get(text.substring(at + 1, end));
                    if (glyph != null) {
                        out.append(glyphMini(glyph.text()));
                        at = end + 1;
                        continue;
                    }
                }
            }
            out.append(letter);
            at++;
        }
        return out.toString();
    }

    /** How far in a menu's title starts from the screen's left edge. */
    static final int MENU_TITLE_LEFT = 8;

    /**
     * A menu's title with its {@code background} glyph drawn under the whole screen: the pen goes back to the
     * screen's left edge, draws the glyph and comes back, so the title sits where it was. An unknown name leaves
     * the title alone. Same as {@code Glyphs::menu_title} in {@code crates/studio-format/src/glyphs.rs}.
     */
    static String menuTitle(String background, String title) {
        Glyph glyph = background == null || background.isBlank() ? null : table().get(background.trim());
        if (glyph == null) {
            return title;
        }
        return "<font:" + SPACE_FONT + ">" + spaces(-MENU_TITLE_LEFT) + "</font>" + glyphMini(glyph.text())
                + "<font:" + SPACE_FONT + ">" + spaces(MENU_TITLE_LEFT - glyph.advance()) + "</font>" + title;
    }

    /** Characters of {@link #SPACE_FONT} that move the pen {@code pixels} (left when below 0). */
    static String spaces(int pixels) {
        StringBuilder out = new StringBuilder();
        int base = pixels >= 0 ? SPACE_UP : SPACE_DOWN;
        int left = Math.abs(pixels);
        int largest = 1 << SPACE_POWERS;
        while (left >= largest * 2) {
            out.appendCodePoint(base + SPACE_POWERS);
            left -= largest;
        }
        for (int power = SPACE_POWERS; power >= 0; power--) {
            if ((left & (1 << power)) != 0) {
                out.appendCodePoint(base + power);
            }
        }
        return out.toString();
    }

    static Widths widths() {
        Widths now = widths;
        if (now == null) {
            synchronized (StudioGlyphs.class) {
                now = widths;
                if (now == null) {
                    now = Widths.load();
                    widths = now;
                }
            }
        }
        return now;
    }

    /**
     * The game's own letter widths ({@code minecraft:default}, Minecraft 26.3), with the 1-pixel gap. The table
     * ({@code vanilla_widths.txt}: start, end, width, b bitmap or u unifont) comes from the user's DialogUI.
     */
    static final class Widths {
        private static final int UNKNOWN = 8;
        private final int[] starts;
        private final int[] ends;
        private final byte[] widths;
        private final boolean[] bitmap;

        private Widths(int[] starts, int[] ends, byte[] widths, boolean[] bitmap) {
            this.starts = starts;
            this.ends = ends;
            this.widths = widths;
            this.bitmap = bitmap;
        }

        static Widths load() {
            int count = 0;
            int[] starts = new int[16384];
            int[] ends = new int[16384];
            byte[] sizes = new byte[16384];
            boolean[] bitmap = new boolean[16384];
            try (InputStream in = StudioGlyphs.class.getResourceAsStream("/vanilla_widths.txt")) {
                if (in != null) {
                    BufferedReader reader = new BufferedReader(new InputStreamReader(in, StandardCharsets.UTF_8));
                    String line;
                    while ((line = reader.readLine()) != null) {
                        if (line.isEmpty() || line.startsWith("#")) {
                            continue;
                        }
                        String[] parts = line.split(" ");
                        if (parts.length < 4) {
                            continue;
                        }
                        if (count == starts.length) {
                            starts = Arrays.copyOf(starts, count * 2);
                            ends = Arrays.copyOf(ends, count * 2);
                            sizes = Arrays.copyOf(sizes, count * 2);
                            bitmap = Arrays.copyOf(bitmap, count * 2);
                        }
                        starts[count] = Integer.parseInt(parts[0], 16);
                        ends[count] = Integer.parseInt(parts[1], 16);
                        sizes[count] = (byte) Integer.parseInt(parts[2]);
                        bitmap[count] = parts[3].equals("b");
                        count++;
                    }
                }
            } catch (IOException | RuntimeException broken) {
                // No table: every letter counts as 8 wide, so text may sit a little off.
            }
            return new Widths(Arrays.copyOf(starts, count), Arrays.copyOf(ends, count), Arrays.copyOf(sizes, count), Arrays.copyOf(bitmap, count));
        }

        private int find(int code) {
            int at = Arrays.binarySearch(starts, code);
            if (at < 0) {
                at = -at - 2;
            }
            return at >= 0 && code <= ends[at] ? at : -1;
        }

        int width(int code) {
            int at = find(code);
            return at >= 0 ? widths[at] : UNKNOWN;
        }

        int width(String text) {
            return text.codePoints().map(this::width).sum();
        }

        boolean allBitmap(String text) {
            return text.codePoints().allMatch(code -> {
                int at = find(code);
                return at >= 0 && bitmap[at];
            });
        }
    }
}

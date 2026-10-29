package com.github.chicodev11.monolab.addon;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Locale;
import java.util.Map;

/** config.yml next to the add-on's data: which features run. Reread within a few seconds of a change. */
final class AddonConfig {
    static final String[][] KEYS = {
        {"map", "World map tiles drawn from saved region files. Needs a restart."},
        {"live-map", "Live map updates around players (up to 0.6 ms a tick)."},
        {"web-map", "Web map page on its own port. Needs a restart."},
        {"inventories", "What online players carry, every 5 seconds."},
        {"loaded-chunks", "Which chunks are loaded, every 5 seconds."},
        {"commands", "Command list for console autocomplete."},
        {"performance-log", "TPS and memory once a minute, for history."},
        {"jvm-stats", "Garbage collection, memory areas and tick times for the advanced monitor."},
    };
    private static final long RECHECK_MS = 5_000;
    private static Path file;
    private static long checkedAt;
    private static long modified = -1;
    private static Map<String, Boolean> values = new LinkedHashMap<>();

    private AddonConfig() {}

    static synchronized void load(Path folder) {
        file = folder.resolve("config.yml");
        if (!Files.exists(file)) {
            try {
                Files.createDirectories(folder);
                Files.writeString(file, defaults(), StandardCharsets.UTF_8);
            } catch (IOException ignored) {
                // Read-only folder: every feature stays on.
            }
        }
        checkedAt = 0;
        refresh();
    }

    static synchronized boolean on(String key) {
        long now = System.currentTimeMillis();
        if (now - checkedAt > RECHECK_MS) {
            checkedAt = now;
            refresh();
        }
        return values.getOrDefault(key, true);
    }

    private static void refresh() {
        if (file == null) {
            return;
        }
        try {
            long stamp = Files.getLastModifiedTime(file).toMillis();
            if (stamp == modified) {
                return;
            }
            modified = stamp;
            Map<String, Boolean> read = new LinkedHashMap<>();
            for (String line : Files.readAllLines(file, StandardCharsets.UTF_8)) {
                int hash = line.indexOf('#');
                String bare = (hash >= 0 ? line.substring(0, hash) : line).trim();
                int colon = bare.indexOf(':');
                if (colon <= 0 || Character.isWhitespace(line.charAt(0))) {
                    continue;
                }
                String value = bare.substring(colon + 1).trim().toLowerCase(Locale.ROOT);
                if (value.equals("false") || value.equals("true")) {
                    read.put(bare.substring(0, colon).trim(), value.equals("true"));
                }
            }
            values = read;
        } catch (IOException | RuntimeException missing) {
            values = new LinkedHashMap<>();
        }
    }

    private static String defaults() {
        StringBuilder out = new StringBuilder("# Monolab add-on. Set a feature to false to turn it off.\n")
                .append("# The Monolab app changes these too (Monitor tab, add-on settings).\n");
        for (String[] key : KEYS) {
            out.append("\n# ").append(key[1]).append('\n').append(key[0]).append(": true\n");
        }
        return out.toString();
    }
}

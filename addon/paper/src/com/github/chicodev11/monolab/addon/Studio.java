package com.github.chicodev11.monolab.addon;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.function.BiConsumer;
import java.util.regex.Pattern;

/**
 * The Studio: in-server features (scoreboard, tab list, MOTD...) defined by JSON files the Monolab app edits.
 * One resource = one file, {@code studio/<kind>/<id>.json}. The files are the real data: this class rereads
 * them when they change and writes {@code studio/status.json} so the app can show what loaded and what didn't.
 */
final class Studio {
    /** File format version. Older files are read as they are; newer ones are left alone until the add-on updates. */
    static final int SCHEMA = 1;
    static final String[] KINDS = {
        "scoreboard", "tab", "motd", "chat", "message", "announcement", "bossbar", "hologram", "npc", "dialog",
        "command", "menu", "item", "recipe", "loot", "advancement", "rule", "world", "font", "model", "block", "furniture", "mob", "enchant", "song", "painting", "cosmetic", "trade", "function", "quest", "structure", "biome", "shop"
    };
    private static final Pattern ID = Pattern.compile("[a-z0-9_-]{1,48}");
    private static final long MOST_BYTES = 256 * 1024;

    /** A loaded file. {@code data} is null when it couldn't be used; {@code error} says why. */
    record Resource(String kind, String id, Map<String, Object> data, String error) {
        boolean enabled() {
            return data != null && !Boolean.FALSE.equals(data.get("enabled"));
        }
    }

    /** A feature that runs one kind of resource. Called on the server thread with the usable, enabled ones. */
    interface Feature {
        String kind();

        void apply(List<Resource> resources);
    }

    private final Path root;
    private final String addonVersion;
    private final BiConsumer<String, Throwable> warn;
    private final java.util.function.Consumer<Runnable> serverThread;
    private final List<Feature> features = new ArrayList<>();
    private final Map<Path, Long> stamps = new HashMap<>();
    private final Map<String, Resource> loaded = new TreeMap<>();
    /** What a feature found wrong with a file it was given (a recipe the server refused...), by kind/id. */
    private final Map<String, String> problems = new java.util.concurrent.ConcurrentHashMap<>();
    private boolean wasOn;
    /** Kinds turned off one by one ({@code studio-<kind>: false} in config.yml), e.g. because another plugin does that job. */
    private java.util.Set<String> off = java.util.Set.of();
    private boolean statusWritten;
    private String lastStatus = "{}";

    Studio(Path root, String addonVersion, BiConsumer<String, Throwable> warn, java.util.function.Consumer<Runnable> serverThread) {
        this.root = root;
        this.addonVersion = addonVersion;
        this.warn = warn;
        this.serverThread = serverThread;
    }

    String version() {
        return addonVersion;
    }

    /** The studio folder ({@code <server>/monolab/studio}). */
    java.nio.file.Path root() {
        return root;
    }

    void add(Feature feature) {
        features.add(feature);
    }

    /** Checks the files for changes; cheap when nothing changed. Runs off the server thread. */
    synchronized void poll() {
        boolean on = AddonConfig.on("studio");
        if (!on) {
            if (wasOn || !statusWritten) {
                loaded.clear();
                stamps.clear();
                applyAll();
                writeStatus(false);
            }
            wasOn = false;
            return;
        }
        boolean changed = !wasOn || !statusWritten;
        wasOn = true;
        java.util.Set<String> nowOff = new java.util.TreeSet<>();
        for (String kind : KINDS) {
            if (!AddonConfig.on("studio-" + kind)) {
                nowOff.add(kind);
            }
        }
        if (!nowOff.equals(off)) {
            off = java.util.Set.copyOf(nowOff);
            changed = true;
        }
        Map<Path, Long> seen = new HashMap<>();
        try {
            Files.createDirectories(root);
        } catch (IOException ignored) {
            // Read-only folder: nothing to load.
        }
        for (String kind : KINDS) {
            Path folder = root.resolve(kind);
            if (!Files.isDirectory(folder)) {
                continue;
            }
            try (DirectoryStream<Path> files = Files.newDirectoryStream(folder, "*.json")) {
                for (Path file : files) {
                    long stamp = Files.getLastModifiedTime(file).toMillis() ^ Files.size(file);
                    seen.put(file, stamp);
                    Long before = stamps.get(file);
                    if (before == null || before != stamp) {
                        changed = true;
                        Resource one = read(kind, file);
                        loaded.put(kind + "/" + one.id(), one);
                    }
                }
            } catch (IOException error) {
                warn.accept("Could not read the studio folder " + folder, error);
            }
        }
        for (Path gone : stamps.keySet()) {
            if (!seen.containsKey(gone)) {
                changed = true;
                String kind = gone.getParent().getFileName().toString();
                loaded.remove(kind + "/" + stem(gone));
            }
        }
        stamps.clear();
        stamps.putAll(seen);
        if (changed) {
            applyAll();
            writeStatus(true);
        }
    }

    /** Rereads now instead of waiting for the next poll, and returns status.json as it is after. */
    synchronized String reloadNow() {
        poll();
        return lastStatus;
    }

    /** A feature couldn't use a file: shows as that file's error in status.json. Any thread. */
    void problem(String kind, String id, String text) {
        problems.put(kind + "/" + id, text == null ? "This file couldn't be used." : text);
        synchronized (this) {
            writeStatus(wasOn);
        }
    }

    /** Whether a usable, enabled resource exists (for checks across kinds, like a menu a command opens). */
    synchronized boolean has(String kind, String id) {
        Resource one = loaded.get(kind + "/" + id);
        return one != null && one.enabled() && !off.contains(kind);
    }

    /** Forgets what was loaded, for when the plugin stops; each feature cleans up after itself. */
    synchronized void stop() {
        loaded.clear();
        stamps.clear();
    }

    private void applyAll() {
        problems.clear();
        Map<String, List<Resource>> byKind = new HashMap<>();
        for (Resource one : loaded.values()) {
            if (one.enabled() && !off.contains(one.kind())) {
                byKind.computeIfAbsent(one.kind(), key -> new ArrayList<>()).add(one);
            }
        }
        for (Feature feature : features) {
            List<Resource> mine = List.copyOf(byKind.getOrDefault(feature.kind(), List.of()));
            serverThread.accept(() -> {
                try {
                    feature.apply(mine);
                } catch (RuntimeException error) {
                    warn.accept("A studio feature failed: " + feature.kind(), error);
                }
            });
        }
    }

    /** The turned-off kinds as a JSON array (kind names need no escaping). */
    private String offList() {
        StringBuilder out = new StringBuilder("[");
        for (String kind : off) {
            if (out.length() > 1) {
                out.append(',');
            }
            out.append('"').append(kind).append('"');
        }
        return out.append(']').toString();
    }

    private static String stem(Path file) {
        String name = file.getFileName().toString();
        return name.endsWith(".json") ? name.substring(0, name.length() - 5) : name;
    }

    private static Resource read(String kind, Path file) {
        String id = stem(file);
        try {
            if (Files.size(file) > MOST_BYTES) {
                return new Resource(kind, id, null, "The file is too big (over 256 KB).");
            }
            return check(kind, id, Files.readString(file, StandardCharsets.UTF_8));
        } catch (IOException error) {
            return new Resource(kind, id, null, "Could not read the file: " + error.getMessage());
        }
    }

    /** Parses and checks one file's text. Kind-specific fields are checked by each feature. */
    @SuppressWarnings("unchecked")
    static Resource check(String kind, String id, String text) {
        if (!ID.matcher(id).matches()) {
            return new Resource(kind, id, null, "File names may only use a-z, 0-9, - and _ (up to 48).");
        }
        Object parsed;
        try {
            parsed = MiniJson.parse(text);
        } catch (RuntimeException error) {
            return new Resource(kind, id, null, "Invalid JSON: " + error.getMessage());
        }
        if (!(parsed instanceof Map)) {
            return new Resource(kind, id, null, "The file must be a JSON object.");
        }
        Map<String, Object> data = (Map<String, Object>) parsed;
        if (!(data.get("schema") instanceof Double schema)) {
            return new Resource(kind, id, null, "\"schema\" is missing.");
        }
        if (schema < 1 || schema != Math.floor(schema)) {
            return new Resource(kind, id, null, "\"schema\" must be a whole number from 1.");
        }
        if (schema > SCHEMA) {
            return new Resource(kind, id, null,
                    "Made for a newer Monolab add-on (schema " + schema.intValue() + "). Update the add-on.");
        }
        Object named = data.get("id");
        if (named != null && !id.equals(named)) {
            return new Resource(kind, id, null, "\"id\" must match the file name (" + id + ").");
        }
        Object enabled = data.get("enabled");
        if (enabled != null && !(enabled instanceof Boolean)) {
            return new Resource(kind, id, null, "\"enabled\" must be true or false.");
        }
        return new Resource(kind, id, data, null);
    }

    private void writeStatus(boolean on) {
        StringBuilder list = new StringBuilder("[");
        for (Resource one : loaded.values()) {
            if (list.length() > 1) {
                list.append(',');
            }
            Json row = new Json().put("kind", one.kind()).put("id", one.id()).put("enabled", one.enabled());
            String error = one.error() != null ? one.error() : problems.get(one.kind() + "/" + one.id());
            list.append(row.put("error", error).end());
        }
        list.append(']');
        String text = new Json()
                .put("schema", SCHEMA)
                .put("addon", addonVersion)
                .put("at", System.currentTimeMillis())
                .put("on", on)
                .raw("off", offList())
                .raw("resources", list.toString())
                .end();
        lastStatus = text;
        try {
            Files.createDirectories(root);
            Path temp = root.resolve("status.json.tmp");
            Files.writeString(temp, text, StandardCharsets.UTF_8);
            try {
                Files.move(temp, root.resolve("status.json"), StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
            } catch (AtomicMoveNotSupportedException plain) {
                Files.move(temp, root.resolve("status.json"), StandardCopyOption.REPLACE_EXISTING);
            }
            statusWritten = true;
        } catch (IOException error) {
            warn.accept("Could not write studio/status.json", error);
        }
    }
}

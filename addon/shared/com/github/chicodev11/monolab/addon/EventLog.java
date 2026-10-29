package com.github.chicodev11.monolab.addon;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.time.LocalDate;
import java.util.ArrayDeque;
import java.util.List;
import java.util.Queue;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.BiConsumer;

final class EventLog {
    private final Path folder;
    private final BiConsumer<String, Throwable> onTrouble;
    private final Queue<String> pending = new ConcurrentLinkedQueue<>();
    private final AtomicReference<String> pendingLive = new AtomicReference<>();
    private final AtomicReference<String> pendingLoaded = new AtomicReference<>();
    private final AtomicReference<String> pendingInventories = new AtomicReference<>();

    private LocalDate sweptOn;

    private static final int KEEP_DAYS = 90;

    EventLog(Path folder, BiConsumer<String, Throwable> onTrouble) {
        this.folder = folder;
        this.onTrouble = onTrouble;
        AddonConfig.load(folder);
        try {
            Files.createDirectories(folder.resolve("actions"));
        } catch (IOException ignored) {
            // Buttons from the app just won't arrive.
        }
    }

    void add(String line) {
        pending.add(line);
    }

    void setLive(String json) {
        pendingLive.set(json);
    }

    void setLoaded(String json) {
        if (!AddonConfig.on("loaded-chunks")) {
            return;
        }
        pendingLoaded.set(json);
    }

    void setInventories(String json) {
        if (!AddonConfig.on("inventories")) {
            return;
        }
        pendingInventories.set(json);
    }

    void flush() {
        writeLive();
        writeNow(pendingLoaded, "loaded.json");
        writeNow(pendingInventories, "inventories.json");
        MonolabApi.serveRequests(folder);
        String api = MonolabApi.takeJson();
        if (api != null) {
            writeNow(new AtomicReference<>(api), "integrations.json");
        }
        LocalDate today = LocalDate.now();
        append("events-" + today + ".jsonl");
        sweep(today);
    }

    private void append(String name) {
        if (pending.isEmpty()) {
            return;
        }
        Queue<String> batch = new ArrayDeque<>();
        for (String line = pending.poll(); line != null; line = pending.poll()) {
            batch.add(line);
        }
        StringBuilder text = new StringBuilder(batch.size() * 128);
        for (String line : batch) {
            text.append(line).append('\n');
        }
        try {
            Files.createDirectories(folder);
            Files.writeString(
                    folder.resolve(name),
                    text.toString(),
                    StandardCharsets.UTF_8,
                    StandardOpenOption.CREATE,
                    StandardOpenOption.APPEND);
        } catch (IOException error) {
            onTrouble.accept("Couldn't write event records.", error);
        }
    }

    private void sweep(LocalDate today) {
        if (today.equals(sweptOn)) {
            return;
        }
        sweptOn = today;
        if (!Files.isDirectory(folder)) {
            return;
        }
        try (var found = Files.list(folder)) {
            List<Path> all = found.toList();
            for (Path path : all) {
                String name = path.getFileName().toString();
                if (!name.startsWith("events-") || !name.endsWith(".jsonl")) {
                    continue;
                }
                String stamp = name.substring("events-".length(), name.length() - ".jsonl".length());
                try {
                    if (LocalDate.parse(stamp).isBefore(today.minusDays(KEEP_DAYS))) {
                        Files.deleteIfExists(path);
                    }
                } catch (RuntimeException ignored) {
                    // Not a date name. Not ours, so leave it.
                }
            }
        } catch (IOException error) {
            onTrouble.accept("Couldn't delete old records.", error);
        }
    }

    private void writeLive() {
        writeNow(pendingLive, "live.json");
    }

    private void writeNow(AtomicReference<String> pending, String name) {
        String json = pending.getAndSet(null);
        if (json == null) {
            return;
        }
        try {
            Files.createDirectories(folder);
            Path temporary = folder.resolve(name + ".tmp");
            Files.writeString(temporary, json, StandardCharsets.UTF_8);
            Files.move(
                    temporary,
                    folder.resolve(name),
                    StandardCopyOption.REPLACE_EXISTING);
        } catch (IOException error) {
            onTrouble.accept("Couldn't write the current state.", error);
        }
    }
}

package com.github.chicodev11.monolab.addon;

import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.net.URI;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.Executors;
import java.util.function.BiConsumer;
import net.kyori.adventure.resource.ResourcePackInfo;
import net.kyori.adventure.resource.ResourcePackRequest;
import net.kyori.adventure.text.Component;
import org.bukkit.Bukkit;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.plugin.Plugin;

/**
 * Studio maker M0: the resource pack the app builds into {@code studio/pack/monolab.zip}, handed to every player.
 * {@code pack.json} says how: {@code self} serves it from a small web server here (the address the player joined
 * with, or a fixed one), {@code url} gives an address the user uploaded it to, {@code off} sends nothing. It goes
 * next to the server's own pack, never instead of it.
 */
final class StudioPack implements Listener {
    private final Plugin plugin;
    private final Path folder;
    private final BiConsumer<String, Throwable> warn;
    private volatile String sha1 = "";
    private volatile byte[] zip = new byte[0];
    private volatile String mode = "off";
    private volatile int port;
    private volatile String address = "";
    private volatile boolean required;
    private long seenModified = -1;
    private HttpServer web;
    private int webPort;

    StudioPack(Plugin plugin, Path studioRoot, BiConsumer<String, Throwable> warn) {
        this.plugin = plugin;
        this.folder = studioRoot.resolve("pack");
        this.warn = warn;
    }

    /** Off the server thread every few seconds: rereads pack.json and the zip when they changed. */
    void check() {
        Path info = folder.resolve("pack.json");
        try {
            long modified = Files.exists(info) ? Files.getLastModifiedTime(info).toMillis() : 0;
            if (modified == seenModified) {
                return;
            }
            seenModified = modified;
            String before = key();
            String oldSha = sha1;
            if (modified == 0) {
                mode = "off";
            } else {
                read(info);
            }
            serve();
            if (!before.equals(key())) {
                for (Player player : Bukkit.getOnlinePlayers()) {
                    player.getScheduler().run(plugin, task -> send(player, oldSha), null);
                }
            }
        } catch (IOException | RuntimeException error) {
            warn.accept("Could not read the studio pack", error);
        }
    }

    private String key() {
        return mode + "|" + sha1 + "|" + port + "|" + address + "|" + required;
    }

    @SuppressWarnings("unchecked")
    private void read(Path info) throws IOException {
        Object parsed = MiniJson.parse(Files.readString(info, StandardCharsets.UTF_8));
        if (!(parsed instanceof Map<?, ?> raw)) {
            mode = "off";
            return;
        }
        Map<String, Object> data = (Map<String, Object>) raw;
        Map<String, Object> sending = data.get("sending") instanceof Map<?, ?> one ? (Map<String, Object>) one : Map.of();
        String wanted = StudioText.text(data, "sha1", "");
        byte[] bytes = Files.exists(folder.resolve("monolab.zip")) ? Files.readAllBytes(folder.resolve("monolab.zip")) : new byte[0];
        String actual = sha1Of(bytes);
        if (wanted.isEmpty() || !wanted.equalsIgnoreCase(actual)) {
            // The zip is still being written (or was changed by hand): wait for the next look.
            seenModified = -1;
            return;
        }
        zip = bytes;
        sha1 = actual;
        mode = StudioText.text(sending, "mode", "off");
        port = StudioText.number(sending, "port", 25590, 1024, 65535);
        address = StudioText.text(sending, "address", "").trim();
        required = Boolean.TRUE.equals(sending.get("required"));
    }

    /** The web server runs only in {@code self} mode, on the chosen port. */
    private synchronized void serve() {
        if (!"self".equals(mode)) {
            stopWeb();
            return;
        }
        if (web != null && webPort == port) {
            return;
        }
        stopWeb();
        try {
            HttpServer made = HttpServer.create(new InetSocketAddress(port), 16);
            made.createContext("/monolab/", exchange -> {
                try (exchange) {
                    byte[] body = zip;
                    String path = exchange.getRequestURI().getPath();
                    if (!"GET".equals(exchange.getRequestMethod()) || !path.equals("/monolab/" + sha1 + ".zip") || body.length == 0) {
                        exchange.sendResponseHeaders(404, -1);
                        return;
                    }
                    exchange.getResponseHeaders().set("Content-Type", "application/zip");
                    exchange.sendResponseHeaders(200, body.length);
                    try (OutputStream out = exchange.getResponseBody()) {
                        out.write(body);
                    }
                }
            });
            made.setExecutor(Executors.newFixedThreadPool(2, task -> {
                Thread thread = new Thread(task, "Monolab studio pack");
                thread.setDaemon(true);
                return thread;
            }));
            made.start();
            web = made;
            webPort = port;
        } catch (IOException error) {
            warn.accept("The studio pack's web server couldn't open port " + port + " (is something else using it?)", error);
        }
    }

    private void stopWeb() {
        if (web != null) {
            web.stop(0);
            web = null;
        }
    }

    synchronized void stop() {
        stopWeb();
    }

    @EventHandler
    public void onJoin(PlayerJoinEvent event) {
        Player player = event.getPlayer();
        player.getScheduler().runDelayed(plugin, task -> send(player, ""), null, 20L);
    }

    /** On the player's thread. Takes off the old studio pack (if it changed) and sends the current one. */
    private void send(Player player, String oldSha) {
        if (!oldSha.isEmpty() && !oldSha.equals(sha1)) {
            player.removeResourcePacks(idOf(oldSha));
        }
        URI uri = uriFor(player);
        if (uri == null || sha1.isEmpty()) {
            return;
        }
        player.sendResourcePacks(ResourcePackRequest.resourcePackRequest()
                .packs(ResourcePackInfo.resourcePackInfo(idOf(sha1), uri, sha1))
                .replace(false)
                .required(required)
                .prompt(Component.text("This server's own items, sounds and pictures.")));
    }

    private URI uriFor(Player player) {
        try {
            return switch (mode) {
                case "url" -> address.startsWith("http") ? URI.create(address) : null;
                case "self" -> {
                    String host = address;
                    if (host.isEmpty()) {
                        InetSocketAddress joined = player.getVirtualHost();
                        host = joined == null ? "" : joined.getHostString();
                    }
                    if (host.isEmpty()) {
                        yield null;
                    }
                    // The host the player typed may carry a port of its own; only the name is ours.
                    if (host.contains(":") && !host.startsWith("[")) {
                        host = "[" + host + "]";
                    }
                    yield URI.create("http://" + host + ":" + port + "/monolab/" + sha1 + ".zip");
                }
                default -> null;
            };
        } catch (IllegalArgumentException bad) {
            return null;
        }
    }

    private static UUID idOf(String sha1) {
        return UUID.nameUUIDFromBytes(("monolab-studio-" + sha1).getBytes(StandardCharsets.UTF_8));
    }

    private static String sha1Of(byte[] bytes) {
        try {
            return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-1").digest(bytes));
        } catch (java.security.NoSuchAlgorithmException impossible) {
            return "";
        }
    }
}

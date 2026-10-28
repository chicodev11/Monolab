package com.github.chicodev11.monolab.addon;

import java.awt.image.BufferedImage;
import java.io.BufferedInputStream;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.Base64;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Semaphore;
import java.util.concurrent.SynchronousQueue;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import java.util.function.BiConsumer;
import javax.imageio.ImageIO;

/**
 * Serves the world map on a port of its own, so friends open it straight from the server (forward the port on the
 * router). Settings come from {@code web.json} next to the map folder, which the Monolab app writes; the file is
 * watched, so changes apply without restarting the server.
 */
final class MapWeb {
    private static final int MOST_Z = 5;
    private static final int LEAST_Z = -3;
    private static final int DIRECT_Z = 3;
    private static final String RULES = "web1";
    private static final int MAX_CONNECTIONS = 48;

    private final Path out;
    private final Path config;
    private final Path status;
    private final Path live;
    private final BiConsumer<String, Throwable> onTrouble;
    private final Semaphore drawing = new Semaphore(Math.max(2, Math.min(4, Runtime.getRuntime().availableProcessors() / 2)));
    private final Object loading = new Object();
    private final Map<String, Object[]> tiles = new LinkedHashMap<>(64, 0.75f, true) {
        @Override
        protected boolean removeEldestEntry(Map.Entry<String, Object[]> eldest) {
            return size() > 400;
        }
    };
    private final Map<String, MapView.Tile> regions = new LinkedHashMap<>(16, 0.75f, true) {
        @Override
        protected boolean removeEldestEntry(Map.Entry<String, MapView.Tile> eldest) {
            return size() > 12;
        }
    };
    private final Map<String, Object[]> indexes = new HashMap<>();
    private final Map<String, Object[]> heads = new HashMap<>();
    private final Map<String, Integer> warmed = new HashMap<>();
    private volatile long lastTile;
    private HttpClient web;

    private volatile boolean running;
    private Thread thread;
    private ServerSocket socket;
    private ExecutorService pool;
    private Settings now = new Settings();
    private long configStamp = Long.MIN_VALUE;
    private MapView.Palette palette;
    private long paletteStamp = Long.MIN_VALUE;
    private long paletteChecked;

    private static final class Settings {
        boolean enabled;
        int port = 8100;
        String token = "";
        boolean players;
        String name = "";
        Map<String, String> labels = new LinkedHashMap<>();
    }

    MapWeb(Path out, BiConsumer<String, Throwable> onTrouble) {
        this.out = out;
        Path data = out.toAbsolutePath().getParent();
        this.config = data.resolve("web.json");
        this.status = data.resolve("web-status.json");
        this.live = data.resolve("live.json");
        this.onTrouble = onTrouble;
    }

    void start() {
        running = true;
        thread = new Thread(this::watch, "Monolab web");
        thread.setDaemon(true);
        thread.start();
        Thread warming = new Thread(this::warm, "Monolab web warm");
        warming.setDaemon(true);
        warming.setPriority(Thread.MIN_PRIORITY);
        warming.start();
    }

    void stop() {
        running = false;
        if (thread != null) {
            thread.interrupt();
        }
        close();
        writeStatus(false, 0, "");
    }

    // ---- settings -----------------------------------------------------------------------------------------

    private void watch() {
        while (running) {
            try {
                long stamp = Files.isRegularFile(config) ? Files.getLastModifiedTime(config).toMillis() : -1;
                if (stamp != configStamp) {
                    configStamp = stamp;
                    apply(read());
                } else if (now.enabled && socket == null) {
                    apply(now);
                }
            } catch (Throwable trouble) {
                onTrouble.accept("Couldn't update the web map", trouble);
            }
            try {
                Thread.sleep(3_000L);
            } catch (InterruptedException ignored) {
                return;
            }
        }
    }

    private Settings read() {
        Settings next = new Settings();
        try {
            if (Files.isRegularFile(config) && MiniJson.parse(Files.readString(config)) instanceof Map<?, ?> map) {
                next.enabled = Boolean.TRUE.equals(map.get("enabled"));
                if (map.get("port") instanceof Number number) {
                    next.port = number.intValue();
                }
                next.token = map.get("token") instanceof String text ? text.replaceAll("[^A-Za-z0-9]", "") : "";
                next.players = Boolean.TRUE.equals(map.get("players"));
                next.name = map.get("name") instanceof String text ? text : "";
                if (map.get("labels") instanceof Map<?, ?> labels) {
                    for (Map.Entry<?, ?> one : labels.entrySet()) {
                        if (one.getValue() instanceof String text) {
                            next.labels.put(String.valueOf(one.getKey()), text);
                        }
                    }
                }
            }
        } catch (Exception unreadable) {
            onTrouble.accept("Couldn't read web.json", unreadable);
        }
        if (next.port < 1024 || next.port > 65535) {
            next.port = 8100;
        }
        return next;
    }

    private synchronized void apply(Settings next) {
        if (!running) {
            return;
        }
        boolean rebind = socket == null || next.port != now.port || !next.enabled;
        now = next;
        if (!next.enabled) {
            close();
            writeStatus(false, 0, "");
            return;
        }
        if (!rebind) {
            writeStatus(true, next.port, "");
            return;
        }
        close();
        try {
            ServerSocket opened = new ServerSocket();
            opened.setReuseAddress(true);
            opened.bind(new InetSocketAddress(next.port));
            socket = opened;
            ThreadPoolExecutor workers = new ThreadPoolExecutor(2, MAX_CONNECTIONS, 30, TimeUnit.SECONDS, new SynchronousQueue<>(), task -> {
                Thread one = new Thread(task, "Monolab web request");
                one.setDaemon(true);
                return one;
            });
            pool = workers;
            Thread accepting = new Thread(() -> accept(opened, workers), "Monolab web accept");
            accepting.setDaemon(true);
            accepting.start();
            writeStatus(true, next.port, "");
        } catch (IOException busy) {
            socket = null;
            writeStatus(false, next.port, busy.getMessage() == null ? busy.toString() : busy.getMessage());
        }
    }

    private synchronized void close() {
        ServerSocket old = socket;
        socket = null;
        if (old != null) {
            try {
                old.close();
            } catch (IOException ignored) {
                // Already closed.
            }
        }
        if (pool != null) {
            pool.shutdownNow();
            pool = null;
        }
    }

    private void writeStatus(boolean on, int port, String error) {
        try {
            Files.createDirectories(status.getParent());
            Path temp = status.resolveSibling("web-status.json.tmp");
            Files.writeString(temp, new Json()
                    .put("at", System.currentTimeMillis())
                    .put("running", on)
                    .put("port", port)
                    .put("error", error.isEmpty() ? null : error)
                    .end());
            Files.move(temp, status, StandardCopyOption.REPLACE_EXISTING);
        } catch (IOException ignored) {
            // Only a status for the app to show.
        }
    }

    // ---- http ---------------------------------------------------------------------------------------------

    private void accept(ServerSocket opened, ExecutorService workers) {
        while (running && !opened.isClosed()) {
            try {
                Socket client = opened.accept();
                try {
                    workers.execute(() -> handle(client));
                } catch (RuntimeException full) {
                    reply(client, 503, "text/plain; charset=utf-8", "Busy".getBytes(StandardCharsets.UTF_8), "no-store");
                }
            } catch (IOException closed) {
                return;
            }
        }
    }

    /** One connection carries many requests (keep-alive): a new TCP handshake per tile is what made far viewers slow. */
    private void handle(Socket client) {
        try (client) {
            client.setSoTimeout(15_000);
            client.setTcpNoDelay(true);
            InputStream in = new BufferedInputStream(client.getInputStream());
            for (int served = 0; served < 1000 && running; served++) {
                String first = line(in);
                if (first == null) {
                    return;
                }
                String match = "";
                boolean close = false;
                for (int i = 0; i < 80; i++) {
                    String header = line(in);
                    if (header == null || header.isEmpty()) {
                        break;
                    }
                    String lower = header.toLowerCase(Locale.ROOT);
                    if (lower.startsWith("if-none-match:")) {
                        match = header.substring(14).trim();
                    } else if (lower.startsWith("connection:") && lower.contains("close")) {
                        close = true;
                    }
                }
                String[] parts = first.split(" ");
                if (parts.length < 2 || !(parts[0].equals("GET") || parts[0].equals("HEAD"))) {
                    reply(client, 405, "text/plain; charset=utf-8", "no".getBytes(StandardCharsets.UTF_8), "no-store");
                    return;
                }
                if (!route(client, parts[0].equals("HEAD"), parts[1], match) || close) {
                    return;
                }
            }
        } catch (IOException | RuntimeException ignored) {
            // A viewer that went away, or an idle connection timing out.
        }
    }

    private static String line(InputStream in) throws IOException {
        StringBuilder text = new StringBuilder();
        while (text.length() < 8192) {
            int c = in.read();
            if (c < 0) {
                return text.length() == 0 ? null : text.toString();
            }
            if (c == '\n') {
                int end = text.length();
                return end > 0 && text.charAt(end - 1) == '\r' ? text.substring(0, end - 1) : text.toString();
            }
            text.append((char) c);
        }
        throw new IOException("line too long");
    }

    /** Answers one request; false when the connection should close afterwards. */
    private boolean route(Socket client, boolean head, String target, String match) throws IOException {
        Settings settings = now;
        String path = target;
        String query = "";
        int ask = target.indexOf('?');
        if (ask >= 0) {
            path = target.substring(0, ask);
            query = target.substring(ask + 1);
        }
        if (!settings.token.isEmpty()) {
            String prefix = "/" + settings.token;
            if (path.equals(prefix)) {
                redirect(client, prefix + "/");
                return false;
            }
            if (!path.startsWith(prefix + "/")) {
                reply(client, 404, "text/plain; charset=utf-8", "This map is not shared.".getBytes(StandardCharsets.UTF_8), "no-store");
                return true;
            }
            path = path.substring(prefix.length());
        }
        if (path.startsWith("/tile/") && path.endsWith(".png")) {
            String[] bits = path.substring(6, path.length() - 4).split("/");
            if (bits.length == 5) {
                query = "realm=" + bits[0] + "&night=" + ("n".equals(bits[1]) ? 1 : 0) + "&z=" + bits[2] + "&tx=" + bits[3] + "&ty=" + bits[4];
                path = "/tile";
            }
        }
        switch (path) {
            case "/", "/index.html" -> send(client, head, 200, "text/html; charset=utf-8", MapPage.HTML.getBytes(StandardCharsets.UTF_8), "no-cache");
            case "/info" -> send(client, head, 200, "application/json; charset=utf-8", info(settings).getBytes(StandardCharsets.UTF_8), "no-store");
            case "/bounds" -> {
                String bounds = bounds(param(query, "realm"));
                if (bounds == null) {
                    send(client, head, 404, "application/json; charset=utf-8", "{\"error\":\"none\"}".getBytes(StandardCharsets.UTF_8), "no-store");
                } else {
                    send(client, head, 200, "application/json; charset=utf-8", bounds.getBytes(StandardCharsets.UTF_8), "no-store");
                }
            }
            case "/changes" -> {
                String realm = param(query, "realm");
                String body = realmOk(realm) ? stamps(realm) : "[]";
                send(client, head, 200, "application/json; charset=utf-8", body.getBytes(StandardCharsets.UTF_8), "no-store");
            }
            case "/players" -> send(client, head, 200, "application/json; charset=utf-8", players(settings).getBytes(StandardCharsets.UTF_8), "no-store");
            case "/tile" -> tile(client, head, query, match);
            case "/head" -> {
                byte[] face = settings.players ? head(param(query, "name")) : null;
                if (face == null) {
                    send(client, head, 404, "text/plain; charset=utf-8", "none".getBytes(StandardCharsets.UTF_8), "max-age=600");
                } else {
                    send(client, head, 200, "image/png", face, "max-age=86400");
                }
            }
            default -> send(client, head, 404, "text/plain; charset=utf-8", "no".getBytes(StandardCharsets.UTF_8), "no-store");
        }
        return true;
    }

    private void tile(Socket client, boolean head, String query, String match) throws IOException {
        lastTile = System.currentTimeMillis();
        String realm = param(query, "realm");
        if (!realmOk(realm)) {
            send(client, head, 404, "text/plain; charset=utf-8", "none".getBytes(StandardCharsets.UTF_8), "no-store");
            return;
        }
        int z = Math.max(LEAST_Z, Math.min(MOST_Z, number(param(query, "z"))));
        int tx = number(param(query, "tx"));
        int ty = number(param(query, "ty"));
        boolean night = param(query, "night").equals("1");
        boolean[] any = {false};
        String tag = "\"" + signature(realm, z, tx, ty, night, any) + "\"";
        if (!any[0]) {
            send(client, head, 404, "text/plain; charset=utf-8", "none".getBytes(StandardCharsets.UTF_8), "max-age=15");
            return;
        }
        if (tag.equals(match)) {
            reply(client, 304, "image/png", new byte[0], "max-age=120", 0, tag);
            return;
        }
        byte[] png = null;
        try {
            if (!drawing.tryAcquire(25, TimeUnit.SECONDS)) {
                send(client, head, 503, "text/plain; charset=utf-8", "busy".getBytes(StandardCharsets.UTF_8), "no-store");
                return;
            }
            try {
                png = tile(realm, z, tx, ty, night);
            } finally {
                drawing.release();
            }
        } catch (InterruptedException cut) {
            Thread.currentThread().interrupt();
            return;
        } catch (IOException | RuntimeException trouble) {
            onTrouble.accept("Couldn't draw a web map tile", trouble);
        }
        if (png == null) {
            send(client, head, 404, "text/plain; charset=utf-8", "none".getBytes(StandardCharsets.UTF_8), "max-age=15");
        } else {
            reply(client, 200, "image/png", head ? new byte[0] : png, "max-age=120", png.length, tag);
        }
    }

    private void send(Socket client, boolean head, int code, String kind, byte[] body, String cache) throws IOException {
        if (head) {
            reply(client, code, kind, new byte[0], cache, body.length);
        } else {
            reply(client, code, kind, body, cache);
        }
    }

    private static void reply(Socket client, int code, String kind, byte[] body, String cache) {
        reply(client, code, kind, body, cache, body.length);
    }

    private static void reply(Socket client, int code, String kind, byte[] body, String cache, int length) {
        reply(client, code, kind, body, cache, length, null);
    }

    private static void reply(Socket client, int code, String kind, byte[] body, String cache, int length, String tag) {
        try {
            OutputStream stream = client.getOutputStream();
            String head = "HTTP/1.1 " + code + (code == 304 ? " Not Modified" : " OK") + "\r\nContent-Type: " + kind
                    + (code == 304 ? "" : "\r\nContent-Length: " + length)
                    + (tag == null ? "" : "\r\nETag: " + tag)
                    + "\r\nCache-Control: " + cache + "\r\nX-Content-Type-Options: nosniff\r\nConnection: keep-alive\r\nKeep-Alive: timeout=15\r\n\r\n";
            stream.write(head.getBytes(StandardCharsets.ISO_8859_1));
            stream.write(body);
            stream.flush();
        } catch (IOException ignored) {
            // The viewer left.
        }
    }

    private static void redirect(Socket client, String to) throws IOException {
        String head = "HTTP/1.1 302 Found\r\nLocation: " + to + "\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        client.getOutputStream().write(head.getBytes(StandardCharsets.ISO_8859_1));
        client.getOutputStream().flush();
    }

    private static String param(String query, String name) {
        for (String pair : query.split("&")) {
            int eq = pair.indexOf('=');
            if (eq > 0 && pair.substring(0, eq).equals(name)) {
                try {
                    return java.net.URLDecoder.decode(pair.substring(eq + 1), StandardCharsets.UTF_8);
                } catch (IllegalArgumentException bad) {
                    return "";
                }
            }
        }
        return "";
    }

    private static int number(String text) {
        try {
            return Integer.parseInt(text.trim());
        } catch (NumberFormatException bad) {
            return 0;
        }
    }

    // ---- what the page asks for ---------------------------------------------------------------------------

    private static boolean realmOk(String realm) {
        return !realm.isEmpty() && realm.length() <= 96 && !realm.contains("..") && !realm.startsWith(".")
                && realm.chars().allMatch(c -> (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '_' || c == '.' || c == '-');
    }

    private List<String> realms() {
        List<String> found = new ArrayList<>();
        try (var list = Files.list(out)) {
            for (Path one : (Iterable<Path>) list::iterator) {
                String name = one.getFileName().toString();
                if (Files.isDirectory(one) && realmOk(name) && !index(name).isEmpty()) {
                    found.add(name);
                }
            }
        } catch (IOException none) {
            return found;
        }
        List<String> order = List.of("overworld", "overworld-cave", "nether", "end");
        found.sort((a, b) -> {
            int x = order.indexOf(a);
            int y = order.indexOf(b);
            if (x >= 0 || y >= 0) {
                return Integer.compare(x < 0 ? 99 : x, y < 0 ? 99 : y);
            }
            return a.compareTo(b);
        });
        return found;
    }

    private String info(Settings settings) {
        StringBuilder realms = new StringBuilder("[");
        for (String realm : realms()) {
            if (realms.length() > 1) {
                realms.append(',');
            }
            realms.append('"').append(realm).append('"');
        }
        realms.append(']');
        Json labels = new Json();
        for (Map.Entry<String, String> one : settings.labels.entrySet()) {
            labels.put(one.getKey(), one.getValue());
        }
        return new Json()
                .put("name", settings.name)
                .raw("realms", realms.toString())
                .raw("labels", labels.end())
                .put("players", settings.players)
                .end();
    }

    private String bounds(String realm) {
        if (!realmOk(realm)) {
            return null;
        }
        Map<Long, Long> known = index(realm);
        if (known.isEmpty()) {
            return null;
        }
        int minX = Integer.MAX_VALUE;
        int minZ = Integer.MAX_VALUE;
        int maxX = Integer.MIN_VALUE;
        int maxZ = Integer.MIN_VALUE;
        for (long key : known.keySet()) {
            int rx = (int) (key >> 32);
            int rz = (int) key;
            minX = Math.min(minX, rx);
            maxX = Math.max(maxX, rx);
            minZ = Math.min(minZ, rz);
            maxZ = Math.max(maxZ, rz);
        }
        return "[" + minX * MapView.SPAN + "," + minZ * MapView.SPAN + "," + (maxX + 1) * MapView.SPAN + "," + (maxZ + 1) * MapView.SPAN + "]";
    }

    static String realmOf(String world) {
        int colon = world.indexOf(':');
        if (colon > 0 && !world.startsWith("minecraft:")) {
            return world.substring(0, colon) + "." + world.substring(colon + 1).replace('/', '.');
        }
        return world.contains("nether") ? "nether" : world.contains("the_end") || world.endsWith("_end") || world.equals("end") ? "end" : "overworld";
    }

    private String players(Settings settings) {
        if (!settings.players) {
            return "[]";
        }
        StringBuilder list = new StringBuilder("[");
        try {
            if (Files.isRegularFile(live) && MiniJson.parse(Files.readString(live)) instanceof Map<?, ?> map
                    && map.get("players") instanceof List<?> people) {
                for (Object one : people) {
                    if (!(one instanceof Map<?, ?> player) || !(player.get("x") instanceof Number x) || !(player.get("z") instanceof Number z)) {
                        continue;
                    }
                    String realm = realmOf(String.valueOf(player.get("world")).toLowerCase(Locale.ROOT));
                    if (list.length() > 1) {
                        list.append(',');
                    }
                    list.append(new Json()
                            .put("name", String.valueOf(player.get("name")))
                            .put("realm", realm)
                            .put("x", Math.round(x.doubleValue()))
                            .put("z", Math.round(z.doubleValue()))
                            .end());
                }
            }
        } catch (Exception unreadable) {
            // Players are a nice extra; an unreadable file just shows nobody.
        }
        return list.append(']').toString();
    }

    // ---- tiles --------------------------------------------------------------------------------------------

    /** Every drawn region with its stamp, so an open page can tell which tiles the add-on just redrew. */
    private String stamps(String realm) {
        StringBuilder out = new StringBuilder("[");
        for (Map.Entry<Long, Long> one : index(realm).entrySet()) {
            if (out.length() > 1) {
                out.append(',');
            }
            long at = one.getKey();
            out.append('[').append((int) (at >> 32)).append(',').append((int) at).append(',').append(one.getValue()).append(']');
        }
        return out.append(']').toString();
    }

    private static long key(int rx, int rz) {
        return ((long) rx << 32) | (rz & 0xFFFFFFFFL);
    }

    /** Which regions are drawn for a realm and when they last changed. Listed again at most every few seconds. */
    private Map<Long, Long> index(String realm) {
        synchronized (indexes) {
            Object[] held = indexes.get(realm);
            long nowMs = System.currentTimeMillis();
            if (held != null && nowMs - (Long) held[0] < 3_000L) {
                @SuppressWarnings("unchecked")
                Map<Long, Long> cached = (Map<Long, Long>) held[1];
                return cached;
            }
            Map<Long, Long> found = new HashMap<>();
            try (var list = Files.list(out.resolve(realm))) {
                for (Path one : (Iterable<Path>) list::iterator) {
                    String name = one.getFileName().toString();
                    if (!name.startsWith("r.") || !name.endsWith(".mnm")) {
                        continue;
                    }
                    String[] bits = name.substring(2, name.length() - 4).split("\\.");
                    if (bits.length != 2) {
                        continue;
                    }
                    try {
                        found.put(key(Integer.parseInt(bits[0]), Integer.parseInt(bits[1])),
                                Files.getLastModifiedTime(one).toMillis() * 31 + Files.size(one));
                    } catch (NumberFormatException | IOException skipped) {
                        // A tile being replaced right now.
                    }
                }
            } catch (IOException none) {
                // No such realm.
            }
            indexes.put(realm, new Object[] {nowMs, found});
            return found;
        }
    }

    private synchronized MapView.Palette palette() {
        long nowMs = System.currentTimeMillis();
        if (palette != null && nowMs - paletteChecked < 10_000L) {
            return palette;
        }
        paletteChecked = nowMs;
        try {
            Path file = out.resolve("palette.json");
            long stamp = Files.isRegularFile(file) ? Files.getLastModifiedTime(file).toMillis() : -1;
            if (palette == null || stamp != paletteStamp) {
                palette = MapView.Palette.read(out);
                paletteStamp = stamp;
                synchronized (loading) {
                    tiles.clear();
                }
            }
        } catch (IOException unreadable) {
            if (palette == null) {
                palette = MapView.Palette.read(out);
            }
        }
        return palette;
    }

    private MapView.Tile region(String realm, int rx, int rz) {
        Long stamp = index(realm).get(key(rx, rz));
        if (stamp == null) {
            return MapView.EMPTY;
        }
        String id = realm + "|" + rx + "|" + rz + "|" + stamp;
        synchronized (regions) {
            MapView.Tile held = regions.get(id);
            if (held != null) {
                return held;
            }
        }
        try {
            MapView.Tile one = MapView.parse(Files.readAllBytes(out.resolve(realm).resolve("r." + rx + "." + rz + ".mnm")));
            synchronized (regions) {
                regions.put(id, one);
            }
            return one;
        } catch (IOException | RuntimeException unreadable) {
            return MapView.EMPTY;
        }
    }

    private String signature(String realm, int z, int tx, int ty, boolean night, boolean[] any) {
        Map<Long, Long> known = index(realm);
        double blocks = MapView.TILE * Math.pow(2, z);
        long left = (long) Math.floor(tx * blocks);
        long right = (long) Math.floor((tx + 1) * blocks) - 1;
        long top = (long) Math.floor(ty * blocks);
        long bottom = (long) Math.floor((ty + 1) * blocks) - 1;
        int rx0 = (int) Math.floorDiv(left, (long) MapView.SPAN);
        int rx1 = (int) Math.floorDiv(right, (long) MapView.SPAN);
        int rz0 = (int) Math.floorDiv(top, (long) MapView.SPAN);
        int rz1 = (int) Math.floorDiv(bottom, (long) MapView.SPAN);
        long hash = 1125899906842597L;
        hash = 31 * hash + RULES.hashCode();
        hash = 31 * hash + (night ? 1 : 0);
        hash = 31 * hash + z;
        for (int rz = rz0 - 1; rz <= rz1 + 1; rz++) {
            for (int rx = rx0 - 1; rx <= rx1 + 1; rx++) {
                Long stamp = known.get(key(rx, rz));
                if (stamp != null) {
                    hash = 31 * hash + key(rx, rz);
                    hash = 31 * hash + stamp;
                    if (rx >= rx0 && rx <= rx1 && rz >= rz0 && rz <= rz1) {
                        any[0] = true;
                    }
                }
            }
        }
        return Long.toHexString(hash);
    }

    private byte[] tile(String realm, int z, int tx, int ty, boolean night) throws IOException {
        boolean[] any = {false};
        String sig = signature(realm, z, tx, ty, night, any);
        if (!any[0]) {
            return null;
        }
        String id = realm + "|" + (night ? "n" : "d") + z + "|" + tx + "|" + ty;
        synchronized (loading) {
            Object[] held = tiles.get(id);
            if (held != null && sig.equals(held[0])) {
                return (byte[]) held[1];
            }
        }
        Path folder = out.resolve("web").resolve(realm);
        String name = (night ? "n" : "d") + z + "_" + tx + "_" + ty;
        Path png = folder.resolve(name + ".png");
        Path mark = folder.resolve(name + ".sig");
        byte[] bytes = null;
        try {
            if (Files.isRegularFile(mark) && sig.equals(Files.readString(mark).trim()) && Files.isRegularFile(png)) {
                bytes = Files.readAllBytes(png);
            }
        } catch (IOException unreadable) {
            bytes = null;
        }
        if (bytes == null) {
            bytes = MapView.png(draw(realm, z, tx, ty, night), MapView.TILE);
            try {
                Files.createDirectories(folder);
                Path temp = folder.resolve(name + ".tmp");
                Files.write(temp, bytes);
                Files.move(temp, png, StandardCopyOption.REPLACE_EXISTING);
                Files.writeString(mark, sig);
            } catch (IOException cannotKeep) {
                // The picture is still served; it just gets drawn again next time.
            }
        }
        synchronized (loading) {
            tiles.put(id, new Object[] {sig, bytes});
        }
        return bytes;
    }

    // ---- drawing ahead ------------------------------------------------------------------------------------

    /**
     * Draws the zoomed-out levels in the background so the first look at a big world doesn't wait for hundreds of
     * regions. Steps aside whenever someone is loading tiles, and only redraws what changed.
     */
    private void warm() {
        while (running) {
            try {
                Thread.sleep(8_000L);
                if (now.enabled && socket != null) {
                    for (String realm : realms()) {
                        warmRealm(realm);
                    }
                }
            } catch (InterruptedException stopped) {
                return;
            } catch (Throwable trouble) {
                onTrouble.accept("Couldn't draw the web map ahead", trouble);
            }
        }
    }

    private void warmRealm(String realm) throws InterruptedException, IOException {
        Map<Long, Long> known = index(realm);
        int stamp = known.hashCode();
        synchronized (warmed) {
            Integer done = warmed.get(realm);
            if (done != null && done == stamp) {
                return;
            }
        }
        for (int z : new int[] {DIRECT_Z, DIRECT_Z + 1, MOST_Z, DIRECT_Z - 1}) {
            long span = (long) (MapView.TILE * Math.pow(2, z));
            java.util.Set<Long> todo = new java.util.LinkedHashSet<>();
            for (long one : known.keySet()) {
                long rx = one >> 32;
                long rz = (int) one;
                long x0 = Math.floorDiv(rx * MapView.SPAN, span);
                long x1 = Math.floorDiv(rx * MapView.SPAN + MapView.SPAN - 1, span);
                long z0 = Math.floorDiv(rz * MapView.SPAN, span);
                long z1 = Math.floorDiv(rz * MapView.SPAN + MapView.SPAN - 1, span);
                for (long tz = z0; tz <= z1; tz++) {
                    for (long tx = x0; tx <= x1; tx++) {
                        todo.add(key((int) tx, (int) tz));
                    }
                }
            }
            for (long one : todo) {
                int tx = (int) (one >> 32);
                int ty = (int) one;
                boolean[] any = {false};
                String sig = signature(realm, z, tx, ty, false, any);
                Path mark = out.resolve("web").resolve(realm).resolve("d" + z + "_" + tx + "_" + ty + ".sig");
                if (!any[0] || (Files.isRegularFile(mark) && sig.equals(Files.readString(mark).trim()))) {
                    continue;
                }
                while (System.currentTimeMillis() - lastTile < 1_500L) {
                    Thread.sleep(400L);
                }
                if (!running || !now.enabled) {
                    return;
                }
                drawing.acquire();
                try {
                    tile(realm, z, tx, ty, false);
                } finally {
                    drawing.release();
                }
            }
        }
        synchronized (warmed) {
            warmed.put(realm, stamp);
        }
    }

    // ---- player faces -------------------------------------------------------------------------------------

    /** An 8x8 face (with the hat layer) for someone online, looked up once a day; null when unknown. */
    private byte[] head(String name) {
        if (!name.matches("[A-Za-z0-9_]{1,16}") || !online(name)) {
            return null;
        }
        String id = name.toLowerCase(Locale.ROOT);
        long nowMs = System.currentTimeMillis();
        synchronized (heads) {
            Object[] held = heads.get(id);
            if (held != null && nowMs - (Long) held[0] < (held[1] == null ? 600_000L : 86_400_000L)) {
                return (byte[]) held[1];
            }
        }
        Path file = out.resolve("web").resolve("heads").resolve(id + ".png");
        byte[] face = null;
        try {
            if (Files.isRegularFile(file) && nowMs - Files.getLastModifiedTime(file).toMillis() < 86_400_000L) {
                face = Files.readAllBytes(file);
            } else {
                face = fetchFace(name);
                if (face != null) {
                    Files.createDirectories(file.getParent());
                    Files.write(file, face);
                }
            }
        } catch (Exception unreachable) {
            face = null;
        }
        synchronized (heads) {
            heads.put(id, new Object[] {nowMs, face});
        }
        return face;
    }

    private boolean online(String name) {
        try {
            if (Files.isRegularFile(live) && MiniJson.parse(Files.readString(live)) instanceof Map<?, ?> map
                    && map.get("players") instanceof List<?> people) {
                for (Object one : people) {
                    if (one instanceof Map<?, ?> player && name.equalsIgnoreCase(String.valueOf(player.get("name")))) {
                        return true;
                    }
                }
            }
        } catch (Exception unreadable) {
            return false;
        }
        return false;
    }

    private byte[] fetchFace(String name) throws Exception {
        if (web == null) {
            web = HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(5)).followRedirects(HttpClient.Redirect.NORMAL).build();
        }
        if (!(MiniJson.parse(fetchText("https://api.mojang.com/users/profiles/minecraft/" + name)) instanceof Map<?, ?> profile)
                || !(profile.get("id") instanceof String uuid) || !uuid.matches("[0-9a-fA-F]{32}")) {
            return null;
        }
        if (!(MiniJson.parse(fetchText("https://sessionserver.mojang.com/session/minecraft/profile/" + uuid)) instanceof Map<?, ?> session)
                || !(session.get("properties") instanceof List<?> properties)) {
            return null;
        }
        String skin = null;
        for (Object one : properties) {
            if (one instanceof Map<?, ?> property && "textures".equals(property.get("name")) && property.get("value") instanceof String value
                    && MiniJson.parse(new String(Base64.getDecoder().decode(value), StandardCharsets.UTF_8)) instanceof Map<?, ?> textures
                    && textures.get("textures") instanceof Map<?, ?> kinds && kinds.get("SKIN") instanceof Map<?, ?> of
                    && of.get("url") instanceof String url) {
                skin = url.replace("http://", "https://");
            }
        }
        if (skin == null || !skin.startsWith("https://textures.minecraft.net/")) {
            return null;
        }
        HttpResponse<byte[]> got = web.send(HttpRequest.newBuilder(URI.create(skin)).timeout(Duration.ofSeconds(8)).build(), HttpResponse.BodyHandlers.ofByteArray());
        BufferedImage picture = got.statusCode() == 200 ? ImageIO.read(new ByteArrayInputStream(got.body())) : null;
        if (picture == null || picture.getWidth() < 64 || picture.getHeight() < 32) {
            return null;
        }
        boolean hatShown = picture.getHeight() >= 64;
        for (int y = 0; y < 8 && !hatShown; y++) {
            for (int x = 0; x < 8; x++) {
                if ((picture.getRGB(40 + x, 8 + y) >>> 24) < 128) {
                    hatShown = true;
                    break;
                }
            }
        }
        BufferedImage face = new BufferedImage(8, 8, BufferedImage.TYPE_INT_ARGB);
        for (int y = 0; y < 8; y++) {
            for (int x = 0; x < 8; x++) {
                int base = picture.getRGB(8 + x, 8 + y) | 0xFF000000;
                int hat = picture.getRGB(40 + x, 8 + y);
                face.setRGB(x, y, hatShown && (hat >>> 24) > 0 ? hat | 0xFF000000 : base);
            }
        }
        ByteArrayOutputStream png = new ByteArrayOutputStream();
        ImageIO.write(face, "png", png);
        return png.toByteArray();
    }

    private String fetchText(String url) throws Exception {
        HttpResponse<String> got = web.send(HttpRequest.newBuilder(URI.create(url)).timeout(Duration.ofSeconds(8)).build(), HttpResponse.BodyHandlers.ofString());
        return got.statusCode() == 200 ? got.body() : "";
    }

    private int[] draw(String realm, int z, int tx, int ty, boolean night) throws IOException {
        if (z > DIRECT_Z) {
            int[][] quarters = new int[4][];
            for (int q = 0; q < 4; q++) {
                byte[] child = tile(realm, z - 1, tx * 2 + (q & 1), ty * 2 + (q >> 1), night);
                quarters[q] = child == null ? null : MapView.decode(child, MapView.TILE);
            }
            return MapView.shrink(quarters, MapView.TILE);
        }
        double bpp = Math.pow(2, z);
        int span = (int) (MapView.TILE * bpp);
        int[] voidRgb = MapView.voidColor(realm);
        return MapView.render(
                (rx, rz) -> region(realm, rx, rz),
                palette(),
                tx * span,
                ty * span,
                MapView.TILE,
                (float) bpp,
                night,
                voidRgb,
                realm.endsWith("-cave"));
    }
}

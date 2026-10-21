package com.github.chicodev11.monolab.addon;

import java.io.BufferedInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.net.ServerSocket;
import java.net.Socket;
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
    private final Semaphore drawing = new Semaphore(2);
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

    private void handle(Socket client) {
        try (client) {
            client.setSoTimeout(10_000);
            InputStream in = new BufferedInputStream(client.getInputStream());
            String first = line(in);
            if (first == null) {
                return;
            }
            for (int i = 0; i < 80; i++) {
                String header = line(in);
                if (header == null || header.isEmpty()) {
                    break;
                }
            }
            String[] parts = first.split(" ");
            if (parts.length < 2 || !(parts[0].equals("GET") || parts[0].equals("HEAD"))) {
                reply(client, 405, "text/plain; charset=utf-8", "no".getBytes(StandardCharsets.UTF_8), "no-store");
                return;
            }
            route(client, parts[0].equals("HEAD"), parts[1]);
        } catch (IOException | RuntimeException ignored) {
            // A viewer that went away mid-reply.
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

    private void route(Socket client, boolean head, String target) throws IOException {
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
                return;
            }
            if (!path.startsWith(prefix + "/")) {
                reply(client, 404, "text/plain; charset=utf-8", "This map is not shared.".getBytes(StandardCharsets.UTF_8), "no-store");
                return;
            }
            path = path.substring(prefix.length());
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
            case "/players" -> send(client, head, 200, "application/json; charset=utf-8", players(settings).getBytes(StandardCharsets.UTF_8), "no-store");
            case "/tile" -> tile(client, head, query);
            default -> send(client, head, 404, "text/plain; charset=utf-8", "no".getBytes(StandardCharsets.UTF_8), "no-store");
        }
    }

    private void tile(Socket client, boolean head, String query) throws IOException {
        String realm = param(query, "realm");
        if (!realmOk(realm)) {
            send(client, head, 404, "text/plain; charset=utf-8", "none".getBytes(StandardCharsets.UTF_8), "no-store");
            return;
        }
        int z = Math.max(LEAST_Z, Math.min(MOST_Z, number(param(query, "z"))));
        int tx = number(param(query, "tx"));
        int ty = number(param(query, "ty"));
        boolean night = param(query, "night").equals("1");
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
            send(client, head, 200, "image/png", png, "max-age=20");
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
        try {
            OutputStream stream = client.getOutputStream();
            String head = "HTTP/1.1 " + code + " OK\r\nContent-Type: " + kind + "\r\nContent-Length: " + length
                    + "\r\nCache-Control: " + cache + "\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n";
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
                    String world = String.valueOf(player.get("world")).toLowerCase(Locale.ROOT);
                    String realm = world.contains("nether") ? "nether" : world.contains("the_end") || world.endsWith("_end") || world.equals("end") ? "end" : "overworld";
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

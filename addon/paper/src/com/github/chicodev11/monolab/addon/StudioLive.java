package com.github.chicodev11.monolab.addon;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.security.SecureRandom;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.function.BiConsumer;

/**
 * The studio's live line on this computer: a TCP port on 127.0.0.1, one JSON message per line. The port and a
 * random token go to {@code studio/live.json}, so only someone who can read the server folder can connect.
 * The app sends {@code hello{token}} first, then {@code reload} after saving a file. Files stay the real data:
 * without this line the add-on still rereads them every two seconds.
 */
final class StudioLive {
    private static final int MOST_CLIENTS = 4;
    private static final int MOST_LINE = 64 * 1024;

    private final Path root;
    private final Studio studio;
    private final BiConsumer<String, Throwable> warn;
    private final List<Socket> clients = new CopyOnWriteArrayList<>();
    private ServerSocket server;
    private String token;

    StudioLive(Path root, Studio studio, BiConsumer<String, Throwable> warn) {
        this.root = root;
        this.studio = studio;
        this.warn = warn;
    }

    void start() {
        try {
            server = new ServerSocket(0, 8, InetAddress.getLoopbackAddress());
            byte[] bytes = new byte[24];
            new SecureRandom().nextBytes(bytes);
            StringBuilder hex = new StringBuilder();
            for (byte one : bytes) {
                hex.append(String.format("%02x", one));
            }
            token = hex.toString();
            Files.createDirectories(root);
            Files.writeString(root.resolve("live.json"), new Json()
                    .put("schema", Studio.SCHEMA)
                    .put("port", server.getLocalPort())
                    .put("token", token)
                    .put("pid", ProcessHandle.current().pid())
                    .end(), StandardCharsets.UTF_8);
        } catch (IOException error) {
            warn.accept("The studio's live connection could not start; files still reload every two seconds.", error);
            return;
        }
        Thread accept = new Thread(this::acceptLoop, "Monolab studio live");
        accept.setDaemon(true);
        accept.start();
    }

    void stop() {
        try {
            if (server != null) {
                server.close();
            }
        } catch (IOException ignored) {
            // Closing anyway.
        }
        for (Socket one : clients) {
            try {
                one.close();
            } catch (IOException ignored) {
                // Already gone.
            }
        }
        clients.clear();
        try {
            Files.deleteIfExists(root.resolve("live.json"));
        } catch (IOException ignored) {
            // A stale live.json only means the app tries once and falls back to files.
        }
    }

    /** Sends one message to every connected app. */
    void broadcast(String json) {
        for (Socket one : clients) {
            send(one, json);
        }
    }

    private void acceptLoop() {
        while (!server.isClosed()) {
            Socket socket;
            try {
                socket = server.accept();
            } catch (IOException closed) {
                return;
            }
            if (clients.size() >= MOST_CLIENTS) {
                try {
                    socket.close();
                } catch (IOException ignored) {
                    // Turned away.
                }
                continue;
            }
            Thread handler = new Thread(() -> serve(socket), "Monolab studio client");
            handler.setDaemon(true);
            handler.start();
        }
    }

    private static boolean same(String a, String b) {
        return a != null && MessageDigest.isEqual(a.getBytes(StandardCharsets.UTF_8), b.getBytes(StandardCharsets.UTF_8));
    }

    @SuppressWarnings("unchecked")
    private void serve(Socket socket) {
        try (socket; BufferedReader in = new BufferedReader(new InputStreamReader(socket.getInputStream(), StandardCharsets.UTF_8))) {
            socket.setSoTimeout(10_000);
            String first = in.readLine();
            Object hello = first == null || first.length() > MOST_LINE ? null : safeParse(first);
            if (!(hello instanceof Map<?, ?> map) || !"hello".equals(map.get("type")) || !same((String) ((Map<String, Object>) map).get("token"), token)) {
                send(socket, new Json().put("type", "error").put("message", "Wrong token.").end());
                return;
            }
            clients.add(socket);
            // After the hello the app may stay connected to hear events.
            socket.setSoTimeout(0);
            send(socket, new Json().put("type", "welcome").put("addon", studio.version()).end());
            String line;
            while ((line = in.readLine()) != null) {
                if (line.length() > MOST_LINE) {
                    break;
                }
                Object message = safeParse(line);
                String type = message instanceof Map<?, ?> one && one.get("type") instanceof String text ? text : "";
                switch (type) {
                    case "reload" -> send(socket, new Json().put("type", "status").raw("status", studio.reloadNow()).end());
                    case "ping" -> send(socket, new Json().put("type", "pong").end());
                    default -> send(socket, new Json().put("type", "error").put("message", "Unknown message.").end());
                }
            }
        } catch (IOException gone) {
            // The app went away.
        } finally {
            clients.remove(socket);
        }
    }

    private static Object safeParse(String line) {
        try {
            return MiniJson.parse(line);
        } catch (RuntimeException broken) {
            return null;
        }
    }

    private static void send(Socket socket, String json) {
        try {
            OutputStream out = socket.getOutputStream();
            synchronized (socket) {
                out.write((json + "\n").getBytes(StandardCharsets.UTF_8));
                out.flush();
            }
        } catch (IOException gone) {
            // Dropped; the read loop cleans up.
        }
    }
}

package com.github.chicodev11.monolab.addon;

import java.awt.image.BufferedImage;
import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.time.Duration;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.function.BiConsumer;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;
import javax.imageio.ImageIO;

final class MapColors {
    private MapColors() {}

    static void build(Path out, Path server, String game, Map<String, Integer> fallback, BiConsumer<String, Throwable> onTrouble, boolean download) {
        List<Path> mods = new ArrayList<>();
        try (var files = Files.list(server.resolve("mods"))) {
            for (Path one : (Iterable<Path>) files::iterator) {
                if (one.getFileName().toString().endsWith(".jar")) mods.add(one);
            }
        } catch (IOException ignored) {
            // A server without mods (Paper).
        }
        mods.sort(null);
        StringBuilder keyText = new StringBuilder("v3|").append(game);
        for (Path one : mods) {
            try {
                keyText.append('|').append(one.getFileName()).append(':').append(Files.size(one));
            } catch (IOException ignored) {
                keyText.append('|').append(one.getFileName());
            }
        }
        String key = Integer.toHexString(keyText.toString().hashCode());
        Path palette = out.resolve("palette.json");
        Path supplied = game.isEmpty() ? null : out.resolve("client-" + game + ".jar");
        boolean have = supplied != null && Files.isRegularFile(supplied);
        try {
            String text = Files.isRegularFile(palette) ? Files.readString(palette) : "";
            if (text.contains("\"key\":\"" + key + "\"")) {
                if (!text.contains("\"vanilla\":false") || (!have && !download)) {
                    return;
                }
            } else if (!download && !have) {
                return;
            }
        } catch (IOException ignored) {
            // Rebuild.
        }
        List<ZipFile> zips = new ArrayList<>();
        Path client = null;
        try {
            Files.createDirectories(out);
            client = have ? supplied : download ? downloadClient(out, game, onTrouble) : null;
            if (client != null) zips.add(new ZipFile(client.toFile()));
            for (Path one : mods) {
                try {
                    zips.add(new ZipFile(one.toFile()));
                } catch (IOException broken) {
                    // Skip broken jars.
                }
            }
            new Builder(zips).write(out, key, fallback, client != null);
        } catch (Throwable error) {
            onTrouble.accept("Couldn't build the map palette", error);
        } finally {
            for (ZipFile zip : zips) {
                try {
                    zip.close();
                } catch (IOException ignored) {
                    // Ignore close failures.
                }
            }
            if (client != null) {
                try {
                    Files.deleteIfExists(client);
                } catch (IOException ignored) {
                    // Delete next time.
                }
            }
        }
    }

    private static Path downloadClient(Path out, String game, BiConsumer<String, Throwable> onTrouble) {
        try {
            HttpClient http = HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(15)).followRedirects(HttpClient.Redirect.NORMAL).build();
            Map<?, ?> manifest = (Map<?, ?>) MiniJson.parse(get(http, "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"));
            String url = null;
            for (Object one : (List<?>) manifest.get("versions")) {
                Map<?, ?> version = (Map<?, ?>) one;
                if (game.equals(version.get("id"))) url = String.valueOf(version.get("url"));
            }
            if (url == null) return null;
            Map<?, ?> info = (Map<?, ?>) MiniJson.parse(get(http, url));
            Map<?, ?> download = (Map<?, ?>) ((Map<?, ?>) info.get("downloads")).get("client");
            String jarUrl = String.valueOf(download.get("url"));
            String sha1 = String.valueOf(download.get("sha1"));
            if (!jarUrl.startsWith("https://piston-data.mojang.com/") && !jarUrl.startsWith("https://launcher.mojang.com/")) return null;
            Path target = out.resolve("client-" + game + ".jar.part");
            HttpResponse<Path> answer = http.send(HttpRequest.newBuilder(URI.create(jarUrl)).timeout(Duration.ofMinutes(10)).build(), HttpResponse.BodyHandlers.ofFile(target));
            if (answer.statusCode() != 200) {
                Files.deleteIfExists(target);
                return null;
            }
            MessageDigest digest = MessageDigest.getInstance("SHA-1");
            try (InputStream in = Files.newInputStream(target)) {
                byte[] buffer = new byte[1 << 16];
                for (int n; (n = in.read(buffer)) > 0; ) digest.update(buffer, 0, n);
            }
            StringBuilder hex = new StringBuilder();
            for (byte b : digest.digest()) hex.append(String.format("%02x", b));
            if (!hex.toString().equalsIgnoreCase(sha1)) {
                Files.deleteIfExists(target);
                return null;
            }
            return target;
        } catch (Throwable error) {
            onTrouble.accept("Couldn't download vanilla textures (client jar) — drawing with map item colors", error);
            return null;
        }
    }

    private static String get(HttpClient http, String url) throws IOException, InterruptedException {
        HttpResponse<String> answer = http.send(HttpRequest.newBuilder(URI.create(url)).timeout(Duration.ofSeconds(30)).build(), HttpResponse.BodyHandlers.ofString());
        if (answer.statusCode() != 200) throw new IOException(url + " → " + answer.statusCode());
        return answer.body();
    }

    private static final class Builder {
        private final List<ZipFile> zips;
        private final Map<String, Object> jsonCache = new HashMap<>();
        private final Map<String, Integer> colorCache = new HashMap<>();

        Builder(List<ZipFile> zips) {
            this.zips = zips;
        }

        private byte[] read(String path) {
            for (ZipFile zip : zips) {
                ZipEntry entry = zip.getEntry(path);
                if (entry == null) continue;
                try (InputStream in = zip.getInputStream(entry)) {
                    return in.readAllBytes();
                } catch (IOException ignored) {
                    return null;
                }
            }
            return null;
        }

        private Object json(String path) {
            if (jsonCache.containsKey(path)) return jsonCache.get(path);
            Object value = null;
            byte[] bytes = read(path);
            if (bytes != null) {
                try {
                    value = MiniJson.parse(new String(bytes, StandardCharsets.UTF_8));
                } catch (RuntimeException broken) {
                    value = null;
                }
            }
            jsonCache.put(path, value);
            return value;
        }

        private static String[] split(String id, String fallbackNs) {
            int colon = id.indexOf(':');
            return colon < 0 ? new String[] {fallbackNs, id} : new String[] {id.substring(0, colon), id.substring(colon + 1)};
        }

        private void model(String id, Map<String, Object> textures, boolean[] tinted, int depth) {
            if (depth > 12) return;
            String[] part = split(id, "minecraft");
            Object raw = json("assets/" + part[0] + "/models/" + part[1] + ".json");
            if (!(raw instanceof Map<?, ?> model)) return;
            if (model.get("parent") instanceof String parent) model(parent, textures, tinted, depth + 1);
            if (model.get("textures") instanceof Map<?, ?> own) {
                for (Map.Entry<?, ?> one : own.entrySet()) textures.put(String.valueOf(one.getKey()), one.getValue());
            }
            if (model.get("elements") instanceof List<?> elements) {
                tinted[0] = false;
                for (Object element : elements) {
                    if (element instanceof Map<?, ?> e && e.get("faces") instanceof Map<?, ?> faces) {
                        Object up = faces.get("up");
                        if (up instanceof Map<?, ?> face && face.get("tintindex") != null) tinted[0] = true;
                    }
                }
            }
        }

        private static String texRef(Object value) {
            if (value instanceof Map<?, ?> map) value = map.get("sprite") != null ? map.get("sprite") : map.get("texture");
            return value instanceof String text ? text : null;
        }

        private Integer textureColor(String id) {
            String[] part = split(id, "minecraft");
            String path = "assets/" + part[0] + "/textures/" + part[1] + ".png";
            if (colorCache.containsKey(path)) return colorCache.get(path);
            Integer color = null;
            byte[] bytes = read(path);
            if (bytes != null) {
                try {
                    BufferedImage img = ImageIO.read(new ByteArrayInputStream(bytes));
                    if (img != null) {
                        int w = img.getWidth(), h = Math.min(img.getHeight(), w);
                        long r = 0, g = 0, b = 0, n = 0;
                        java.awt.image.Raster raster = img.getRaster();
                        boolean gray = img.getColorModel().getColorSpace().getType() == java.awt.color.ColorSpace.TYPE_GRAY;
                        int bands = raster.getNumBands();
                        int depth = img.getColorModel().getComponentSize(0);
                        for (int y = 0; y < h; y++) {
                            for (int x = 0; x < w; x++) {
                                if (gray) {
                                    int alpha = bands > 1 ? raster.getSample(x, y, 1) : (1 << depth) - 1;
                                    int value = raster.getSample(x, y, 0);
                                    if (depth > 8) {
                                        alpha >>= depth - 8;
                                        value >>= depth - 8;
                                    }
                                    if (alpha < 30) continue;
                                    r += value;
                                    g += value;
                                    b += value;
                                    n++;
                                    continue;
                                }
                                int p = img.getRGB(x, y);
                                if ((p >>> 24) < 30) continue;
                                r += (p >> 16) & 255;
                                g += (p >> 8) & 255;
                                b += p & 255;
                                n++;
                            }
                        }
                        if (n > 0) color = (int) (r / n) << 16 | (int) (g / n) << 8 | (int) (b / n);
                    }
                } catch (IOException | RuntimeException broken) {
                    color = null;
                }
            }
            colorCache.put(path, color);
            return color;
        }

        private static final String[] KEYS = {"top", "end", "up", "all", "texture", "cross", "plant", "crop", "layer0", "particle", "side"};

        void write(Path out, String key, Map<String, Integer> fallback, boolean vanilla) throws IOException {
            Map<String, String> blocks = new LinkedHashMap<>();
            Map<String, String> biomes = new LinkedHashMap<>();
            byte[] grass = null;
            byte[] foliage = null;
            for (ZipFile zip : zips) {
                for (ZipEntry entry : java.util.Collections.list(zip.entries())) {
                    String name = entry.getName();
                    if (name.startsWith("assets/") && name.contains("/blockstates/") && name.endsWith(".json")) {
                        String ns = name.substring(7, name.indexOf('/', 7));
                        String block = ns + ":" + name.substring(name.lastIndexOf('/') + 1, name.length() - 5);
                        if (!blocks.containsKey(block)) {
                            String line = block(name, block);
                            if (line != null) blocks.put(block, line);
                        }
                    } else if (name.startsWith("data/") && name.contains("/worldgen/biome/") && name.endsWith(".json")) {
                        String ns = name.substring(5, name.indexOf('/', 5));
                        String biome = ns + ":" + name.substring(name.indexOf("/worldgen/biome/") + 16, name.length() - 5);
                        if (!biomes.containsKey(biome)) {
                            String line = biome(name);
                            if (line != null) biomes.put(biome, line);
                        }
                    } else if (name.equals("assets/minecraft/textures/colormap/grass.png") && grass == null) {
                        grass = read(name);
                    } else if (name.equals("assets/minecraft/textures/colormap/foliage.png") && foliage == null) {
                        foliage = read(name);
                    }
                }
            }
            for (Map.Entry<String, Integer> one : fallback.entrySet()) {
                String name = one.getKey();
                if (name.contains("|") || blocks.containsKey(name)) continue;
                if (one.getValue() == 0) continue;
                blocks.put(name, "[\"" + hex(one.getValue()) + "\",\"none\"]");
            }
            if (grass != null) Files.write(out.resolve("grass.png"), grass);
            if (foliage != null) Files.write(out.resolve("foliage.png"), foliage);
            StringBuilder text = new StringBuilder("{\"key\":\"").append(key).append("\",\"vanilla\":").append(vanilla).append(",\"blocks\":{");
            boolean first = true;
            for (Map.Entry<String, String> one : blocks.entrySet()) {
                if (!first) text.append(',');
                first = false;
                text.append('"').append(one.getKey()).append("\":").append(one.getValue());
            }
            text.append("},\"biomes\":{");
            first = true;
            for (Map.Entry<String, String> one : biomes.entrySet()) {
                if (!first) text.append(',');
                first = false;
                text.append('"').append(one.getKey()).append("\":").append(one.getValue());
            }
            text.append("}}");
            Path temp = out.resolve("palette.json.tmp");
            Files.writeString(temp, text);
            Files.move(temp, out.resolve("palette.json"), java.nio.file.StandardCopyOption.REPLACE_EXISTING);
        }

        private String block(String path, String block) {
            Object raw = json(path);
            if (!(raw instanceof Map<?, ?> state)) return null;
            String modelId = null;
            if (state.get("variants") instanceof Map<?, ?> variants && !variants.isEmpty()) {
                Object first = variants.values().iterator().next();
                if (first instanceof List<?> list && !list.isEmpty()) first = list.get(0);
                if (first instanceof Map<?, ?> v) modelId = (String) v.get("model");
            } else if (state.get("multipart") instanceof List<?> parts && !parts.isEmpty()) {
                if (parts.get(0) instanceof Map<?, ?> part) {
                    Object apply = part.get("apply");
                    if (apply instanceof List<?> list && !list.isEmpty()) apply = list.get(0);
                    if (apply instanceof Map<?, ?> a) modelId = (String) a.get("model");
                }
            }
            if (modelId == null) return null;
            Map<String, Object> textures = new HashMap<>();
            boolean[] tinted = {false};
            model(modelId, textures, tinted, 0);
            Integer color = null;
            for (String one : KEYS) {
                String ref = texRef(textures.get(one));
                for (int hop = 0; ref != null && ref.startsWith("#") && hop < 8; hop++) ref = texRef(textures.get(ref.substring(1)));
                if (ref != null && !ref.startsWith("#")) {
                    color = textureColor(ref);
                    if (color != null) break;
                }
            }
            if (color == null) return null;
            String name = block.substring(block.indexOf(':') + 1);
            String tint = "none";
            if (block.startsWith("minecraft:")) {
                if (name.matches("grass_block|short_grass|tall_grass|fern|large_fern|sugar_cane|vine|bush")) tint = "grass";
                else if (name.equals("birch_leaves")) tint = "fixed:80a755";
                else if (name.equals("spruce_leaves")) tint = "fixed:619961";
                else if (name.equals("mangrove_leaves")) tint = "fixed:92c648";
                else if (name.endsWith("_leaves") && !name.matches(".*(cherry|azalea|pale_oak).*")) tint = "foliage";
                else if (name.equals("water") || name.equals("bubble_column")) tint = "water";
                else if (name.equals("lily_pad")) tint = "fixed:208030";
            } else if (tinted[0]) {
                tint = name.contains("leaves") || name.contains("leaf") ? "foliage" : name.contains("water") ? "water" : "grass";
            }
            return "[\"" + hex(color) + "\",\"" + tint + "\"]";
        }

        private String biome(String path) {
            Object raw = json(path);
            if (!(raw instanceof Map<?, ?> biome)) return null;
            double temp = biome.get("temperature") instanceof Double d ? d : 0.8;
            double rain = biome.get("downfall") instanceof Double d ? d : 0.4;
            Map<?, ?> effects = biome.get("effects") instanceof Map<?, ?> e ? e : Map.of();
            return "[" + temp + "," + rain + ",\"" + color(effects.get("water_color"), "3f76e4") + "\",\""
                    + color(effects.get("grass_color"), "") + "\",\"" + color(effects.get("foliage_color"), "") + "\",\""
                    + (effects.get("grass_color_modifier") instanceof String m ? m : "") + "\"]";
        }

        private static String color(Object value, String otherwise) {
            if (value instanceof Double d) return hex(d.intValue());
            if (value instanceof String text && text.startsWith("#") && text.length() >= 7) return text.substring(1, 7).toLowerCase();
            return otherwise;
        }
    }

    static String hex(int rgb) {
        return String.format("%06x", rgb & 0xFFFFFF);
    }
}

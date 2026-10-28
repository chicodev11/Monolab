package com.github.chicodev11.monolab.addon;

import java.awt.image.BufferedImage;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.zip.GZIPInputStream;
import javax.imageio.ImageIO;

/**
 * Draws the {@code .mnm} tiles {@link MapRender} writes as top-down PNG pictures. This is the same picture the
 * Monolab app draws (worldmap.rs), so the web map and the in-app map look alike.
 */
final class MapView {
    static final int SPAN = MapRender.SPAN;
    static final int TILE = 128;
    private static final int NONE = 0xFFFF;
    private static final int VOID_MARK = 0xFFFE;

    private MapView() {
    }

    /** One 512 x 512 region of the map. */
    static final class Tile {
        String[] blocks = new String[0];
        String[] biomes = new String[0];
        final char[] block = new char[SPAN * SPAN];
        final char[] floor = new char[SPAN * SPAN];
        final char[] biome = new char[SPAN * SPAN];
        final short[] height = new short[SPAN * SPAN];
        final byte[] depth = new byte[SPAN * SPAN];
        final byte[] light = new byte[SPAN * SPAN];
        final boolean[] voided = new boolean[SPAN * SPAN];
        boolean present;
        boolean marksVoid;

        Tile() {
            java.util.Arrays.fill(block, (char) NONE);
            java.util.Arrays.fill(floor, (char) NONE);
        }

        boolean isVoid(int at) {
            return present && (!marksVoid || voided[at]);
        }
    }

    static final Tile EMPTY = new Tile();

    static Tile parse(byte[] gz) throws IOException {
        byte[] raw;
        try (GZIPInputStream in = new GZIPInputStream(new ByteArrayInputStream(gz))) {
            raw = in.readAllBytes();
        }
        if (raw.length < 4 || raw[0] != 'M' || raw[1] != 'N' || raw[2] != 'M' || raw[3] != '2') {
            throw new IOException("Not a Monolab map tile");
        }
        int[] at = {4};
        String[] blocks = names(raw, at);
        String[] biomes = names(raw, at);
        if (raw.length < at[0] + SPAN * SPAN * 10) {
            throw new IOException("The map tile is cut short");
        }
        Tile tile = new Tile();
        tile.blocks = blocks;
        tile.biomes = biomes;
        tile.present = true;
        short[] lift = new short[blocks.length];
        for (int i = 0; i < blocks.length; i++) {
            lift[i] = (short) plantLift(blocks[i]);
        }
        int p = at[0];
        for (int i = 0; i < SPAN * SPAN; i++, p += 10) {
            int block = u16(raw, p);
            if (block == VOID_MARK) {
                block = NONE;
                tile.voided[i] = true;
                tile.marksVoid = true;
            }
            tile.block[i] = (char) block;
            tile.floor[i] = (char) u16(raw, p + 2);
            tile.biome[i] = (char) u16(raw, p + 4);
            int lifted = block < lift.length ? lift[block] : 0;
            tile.height[i] = (short) ((short) u16(raw, p + 6) - lifted);
            tile.depth[i] = raw[p + 8];
            tile.light[i] = raw[p + 9];
        }
        return tile;
    }

    private static int u16(byte[] raw, int at) {
        return (raw[at] & 0xFF) << 8 | (raw[at + 1] & 0xFF);
    }

    private static String[] names(byte[] raw, int[] at) throws IOException {
        int count = u16(raw, at[0]);
        at[0] += 2;
        String[] out = new String[count];
        for (int i = 0; i < count; i++) {
            int length = u16(raw, at[0]);
            if (at[0] + 2 + length > raw.length) {
                throw new IOException("The map tile is cut short");
            }
            out[i] = new String(raw, at[0] + 2, length, StandardCharsets.UTF_8);
            at[0] += 2 + length;
        }
        return out;
    }

    private static final java.util.Set<String> TALL = java.util.Set.of(
            "tall_grass", "large_fern", "sunflower", "lilac", "rose_bush", "peony", "tall_seagrass");
    private static final java.util.Set<String> LOW = java.util.Set.of(
            "short_grass", "grass", "fern", "dead_bush", "dandelion", "poppy", "blue_orchid", "allium", "azure_bluet",
            "oxeye_daisy", "cornflower", "lily_of_the_valley", "wither_rose", "torchflower", "pink_petals", "wildflowers",
            "leaf_litter", "bush", "firefly_bush", "short_dry_grass", "snow", "moss_carpet", "pale_moss_carpet", "sweet_berry_bush");

    private static int plantLift(String name) {
        String shortName = name.substring(name.lastIndexOf(':') + 1);
        if (TALL.contains(shortName) || shortName.startsWith("tall_")) {
            return 2;
        }
        if (LOW.contains(shortName)
                || shortName.endsWith("_tulip")
                || shortName.endsWith("_sapling")
                || shortName.endsWith("_mushroom")
                || shortName.endsWith("_carpet")
                || shortName.endsWith("_flower")
                || (shortName.endsWith("_grass") && !shortName.equals("grass_block"))) {
            return 1;
        }
        return 0;
    }

    // ---- colors -------------------------------------------------------------------------------------------

    private static final int T_NONE = 0;
    private static final int T_GRASS = 1;
    private static final int T_FOLIAGE = 2;
    private static final int T_WATER = 3;
    private static final int T_FIXED = 4;

    private static final class Biome {
        float temperature = 0.8f;
        float downfall = 0.4f;
        int water = 0x3f76e4;
        int grass = -1;
        int foliage = -1;
        String modifier = "";
    }

    /** Block colors and biome tints, read from the palette.json the addon builds next to the tiles. */
    static final class Palette {
        private final Map<String, int[]> blocks = new HashMap<>();
        private final Map<String, Biome> biomes = new HashMap<>();
        private int[] grass;
        private int grassW;
        private int grassH;
        private int[] foliage;
        private int foliageW;
        private int foliageH;

        static Palette read(Path folder) {
            Palette palette = new Palette();
            try {
                Object root = MiniJson.parse(Files.readString(folder.resolve("palette.json")));
                if (root instanceof Map<?, ?> map) {
                    if (map.get("blocks") instanceof Map<?, ?> blocks) {
                        for (Map.Entry<?, ?> one : blocks.entrySet()) {
                            if (one.getValue() instanceof List<?> entry && !entry.isEmpty()) {
                                int color = hex(String.valueOf(entry.get(0)), 0x808080);
                                String tint = entry.size() > 1 ? String.valueOf(entry.get(1)) : "none";
                                int kind = T_NONE;
                                int fixed = 0;
                                switch (tint) {
                                    case "grass" -> kind = T_GRASS;
                                    case "foliage" -> kind = T_FOLIAGE;
                                    case "water" -> kind = T_WATER;
                                    default -> {
                                        if (tint.startsWith("fixed:")) {
                                            kind = T_FIXED;
                                            fixed = hex(tint.substring(6), 0xFFFFFF);
                                        }
                                    }
                                }
                                palette.blocks.put(String.valueOf(one.getKey()), new int[] {color, kind, fixed});
                            }
                        }
                    }
                    if (map.get("biomes") instanceof Map<?, ?> biomes) {
                        for (Map.Entry<?, ?> one : biomes.entrySet()) {
                            if (one.getValue() instanceof List<?> entry) {
                                Biome biome = new Biome();
                                if (entry.size() > 0 && entry.get(0) instanceof Number n) biome.temperature = n.floatValue();
                                if (entry.size() > 1 && entry.get(1) instanceof Number n) biome.downfall = n.floatValue();
                                biome.water = hex(text(entry, 2), 0x3f76e4);
                                biome.grass = hex(text(entry, 3), -1);
                                biome.foliage = hex(text(entry, 4), -1);
                                biome.modifier = text(entry, 5);
                                palette.biomes.put(String.valueOf(one.getKey()), biome);
                            }
                        }
                    }
                }
            } catch (Exception unreadable) {
                // Without a palette every block is plain grey; the map still draws.
            }
            BufferedImage grass = image(folder.resolve("grass.png"));
            if (grass != null) {
                palette.grassW = grass.getWidth();
                palette.grassH = grass.getHeight();
                palette.grass = grass.getRGB(0, 0, palette.grassW, palette.grassH, null, 0, palette.grassW);
            }
            BufferedImage foliage = image(folder.resolve("foliage.png"));
            if (foliage != null) {
                palette.foliageW = foliage.getWidth();
                palette.foliageH = foliage.getHeight();
                palette.foliage = foliage.getRGB(0, 0, palette.foliageW, palette.foliageH, null, 0, palette.foliageW);
            }
            return palette;
        }

        private static BufferedImage image(Path file) {
            try {
                return Files.isRegularFile(file) ? ImageIO.read(file.toFile()) : null;
            } catch (Exception unreadable) {
                return null;
            }
        }

        private static String text(List<?> list, int at) {
            return at < list.size() && list.get(at) != null ? String.valueOf(list.get(at)) : "";
        }

        private static int hex(String text, int otherwise) {
            if (text == null || text.isEmpty()) {
                return otherwise;
            }
            try {
                return Integer.parseInt(text, 16);
            } catch (NumberFormatException bad) {
                return otherwise;
            }
        }

        private static int colormap(int[] map, int w, int h, Biome biome, int otherwise) {
            if (map == null) {
                return otherwise;
            }
            float temp = Math.min(1f, Math.max(0f, biome.temperature));
            float rain = Math.min(1f, Math.max(0f, biome.downfall)) * temp;
            int x = Math.min((int) ((1f - temp) * 255f), w - 1);
            int y = Math.min((int) ((1f - rain) * 255f), h - 1);
            return map[y * w + x] & 0xFFFFFF;
        }

        private static final Biome FALLBACK = new Biome();

        /** Grass, foliage and water tint of a biome. */
        int[] tints(Biome biome) {
            Biome b = biome == null ? FALLBACK : biome;
            int grassTint = b.grass >= 0 ? b.grass : colormap(grass, grassW, grassH, b, 0x91bd59);
            switch (b.modifier) {
                case "swamp" -> grassTint = 0x6a7039;
                case "dark_forest" -> grassTint = ((grassTint & 0xfefefe) + 0x28340a) >> 1;
                default -> {
                }
            }
            int foliageTint = b.foliage >= 0 ? b.foliage : colormap(foliage, foliageW, foliageH, b, 0x48b518);
            return new int[] {grassTint, foliageTint, b.water};
        }

        int[] block(String name) {
            return blocks.get(name);
        }

        int color(String name, int[] tints) {
            int[] entry = blocks.get(name);
            if (entry == null) {
                return 0x808080;
            }
            int tint;
            switch (entry[1]) {
                case T_GRASS -> tint = tints[0];
                case T_FOLIAGE -> tint = tints[1];
                case T_WATER -> tint = tints[2];
                case T_FIXED -> tint = entry[2];
                default -> {
                    return entry[0];
                }
            }
            return multiply(entry[0], tint);
        }

        Biome biomeOf(Tile tile, int at) {
            int index = tile.biome[at];
            return index < tile.biomes.length ? biomes.get(tile.biomes[index]) : null;
        }
    }

    private static float[] channels(int color) {
        return new float[] {(color >> 16) & 255, (color >> 8) & 255, color & 255};
    }

    private static int multiply(int a, int b) {
        float[] x = channels(a);
        float[] y = channels(b);
        return pack(x[0] * y[0] / 255f, x[1] * y[1] / 255f, x[2] * y[2] / 255f);
    }

    private static int pack(float r, float g, float b) {
        return clamp(r) << 16 | clamp(g) << 8 | clamp(b);
    }

    private static int clamp(float v) {
        return (int) Math.min(255f, Math.max(0f, v));
    }

    // ---- smooth biome tints -------------------------------------------------------------------------------

    /** Source of regions while drawing, so the caller can cache them. */
    interface Regions {
        Tile get(int rx, int rz);
    }

    private static final class Cursor {
        final Regions regions;
        private int lastX = Integer.MIN_VALUE;
        private int lastZ = Integer.MIN_VALUE;
        private Tile last;
        private final List<int[]> keys = new ArrayList<>();
        private final List<Tile> tiles = new ArrayList<>();
        int at;

        Cursor(Regions regions) {
            this.regions = regions;
        }

        Tile tile(int x, int z) {
            int rx = Math.floorDiv(x, SPAN);
            int rz = Math.floorDiv(z, SPAN);
            at = Math.floorMod(z, SPAN) * SPAN + Math.floorMod(x, SPAN);
            if (rx == lastX && rz == lastZ) {
                return last;
            }
            Tile found = null;
            for (int i = 0; i < keys.size(); i++) {
                int[] key = keys.get(i);
                if (key[0] == rx && key[1] == rz) {
                    found = tiles.get(i);
                    break;
                }
            }
            if (found == null) {
                found = regions.get(rx, rz);
                keys.add(new int[] {rx, rz});
                tiles.add(found);
            }
            lastX = rx;
            lastZ = rz;
            last = found;
            return found;
        }
    }

    private static final class TintField {
        final int x0;
        final int z0;
        final int cell;
        final int w;
        final int h;
        final float[] data;
        final boolean[] has;

        TintField(Palette palette, Cursor cursor, int left, int top, int spanX, int spanZ, int stride, int radius) {
            cell = Math.max(stride, 4);
            int reach = Math.max(radius / cell, 1);
            int margin = (reach + 2) * cell;
            x0 = left - margin;
            z0 = top - margin;
            w = (spanX + 2 * margin) / cell + 1;
            h = (spanZ + 2 * margin) / cell + 1;
            float[] raw = new float[w * h * 9];
            boolean[] rawHas = new boolean[w * h];
            for (int j = 0; j < h; j++) {
                for (int i = 0; i < w; i++) {
                    Tile tile = cursor.tile(x0 + i * cell + cell / 2, z0 + j * cell + cell / 2);
                    int at = cursor.at;
                    if (tile.block[at] == NONE) {
                        continue;
                    }
                    int index = tile.biome[at];
                    Biome biome = index < tile.biomes.length ? palette.biomes.get(tile.biomes[index]) : null;
                    if (biome == null) {
                        continue;
                    }
                    int[] tints = palette.tints(biome);
                    for (int k = 0; k < 3; k++) {
                        float[] c = channels(tints[k]);
                        System.arraycopy(c, 0, raw, (j * w + i) * 9 + k * 3, 3);
                    }
                    rawHas[j * w + i] = true;
                }
            }
            float[] flag = new float[w * h];
            for (int i = 0; i < flag.length; i++) {
                flag[i] = rawHas[i] ? 1 : 0;
            }
            float[][] pair = blur(blur(new float[][] {raw, flag}, true, reach), false, reach);
            data = pair[0];
            has = new boolean[w * h];
            for (int i = 0; i < has.length; i++) {
                has[i] = pair[1][i] > 0;
            }
        }

        private float[][] blur(float[][] in, boolean horizontal, int reach) {
            float[] src = in[0];
            float[] flag = in[1];
            float[] out = new float[w * h * 9];
            float[] outFlag = new float[w * h];
            float[] sum = new float[9];
            for (int j = 0; j < h; j++) {
                for (int i = 0; i < w; i++) {
                    java.util.Arrays.fill(sum, 0);
                    int n = 0;
                    for (int d = -reach; d <= reach; d++) {
                        int ii = horizontal ? i + d : i;
                        int jj = horizontal ? j : j + d;
                        if (ii < 0 || jj < 0 || ii >= w || jj >= h || flag[jj * w + ii] <= 0) {
                            continue;
                        }
                        int base = (jj * w + ii) * 9;
                        for (int k = 0; k < 9; k++) {
                            sum[k] += src[base + k];
                        }
                        n++;
                    }
                    if (n > 0) {
                        int base = (j * w + i) * 9;
                        for (int k = 0; k < 9; k++) {
                            out[base + k] = sum[k] / n;
                        }
                        outFlag[j * w + i] = 1;
                    }
                }
            }
            return new float[][] {out, outFlag};
        }

        /** Smoothed tints at one block, or null where no biome is known. */
        int[] at(int x, int z) {
            float fx = (x - x0) / (float) cell - 0.5f;
            float fz = (z - z0) / (float) cell - 0.5f;
            int i0 = (int) Math.floor(fx);
            int j0 = (int) Math.floor(fz);
            float tx = fx - i0;
            float tz = fz - j0;
            float[] sum = new float[9];
            float weight = 0;
            float[] parts = {(1 - tx) * (1 - tz), tx * (1 - tz), (1 - tx) * tz, tx * tz};
            int[][] offsets = {{0, 0}, {1, 0}, {0, 1}, {1, 1}};
            for (int n = 0; n < 4; n++) {
                int i = i0 + offsets[n][0];
                int j = j0 + offsets[n][1];
                if (i < 0 || j < 0 || i >= w || j >= h || parts[n] <= 0 || !has[j * w + i]) {
                    continue;
                }
                int base = (j * w + i) * 9;
                for (int k = 0; k < 9; k++) {
                    sum[k] += data[base + k] * parts[n];
                }
                weight += parts[n];
            }
            if (weight <= 0) {
                return null;
            }
            for (int k = 0; k < 9; k++) {
                sum[k] /= weight;
            }
            return new int[] {pack(sum[0], sum[1], sum[2]), pack(sum[3], sum[4], sum[5]), pack(sum[6], sum[7], sum[8])};
        }
    }

    // ---- shading ------------------------------------------------------------------------------------------

    private static float[] mute(float[] c, float dark, float saturation) {
        float luma = c[0] * 0.299f + c[1] * 0.587f + c[2] * 0.114f;
        return new float[] {
            luma * dark + (c[0] - luma) * saturation,
            luma * dark + (c[1] - luma) * saturation,
            luma * dark + (c[2] - luma) * saturation};
    }

    private static boolean isFoliage(Palette palette, String name) {
        String shortName = name.substring(name.lastIndexOf(':') + 1);
        int[] entry = palette.block(name);
        return (entry != null && entry[1] == T_FOLIAGE)
                || shortName.contains("leaves")
                || shortName.equals("vine")
                || shortName.endsWith("_vines")
                || shortName.endsWith("_vine");
    }

    private static int shade(Palette palette, Tile tile, int at, Integer west, Integer north, boolean night, int[] blend, boolean cave) {
        String name = tile.blocks[tile.block[at]];
        int[] tints = blend != null ? blend : palette.tints(palette.biomeOf(tile, at));
        int depth = tile.depth[at] & 0xFF;
        int height = tile.height[at];
        float[] color;
        if (depth > 0) {
            int water = palette.color("minecraft:water", tints);
            if (tile.floor[at] < tile.blocks.length) {
                float[] bed = channels(palette.color(tile.blocks[tile.floor[at]], tints));
                for (int i = 0; i < 3; i++) {
                    bed[i] *= 0.8f;
                }
                float[] wet = channels(water);
                float a = Math.min(0.45f + depth * 0.035f, 0.9f);
                float dark = 1f - Math.min(depth, 30) * 0.011f;
                color = channels(pack(
                        (bed[0] * (1 - a) + wet[0] * a) * dark,
                        (bed[1] * (1 - a) + wet[1] * a) * dark,
                        (bed[2] * (1 - a) + wet[2] * a) * dark));
            } else {
                color = channels(water);
            }
        } else {
            String shortName = name.substring(name.lastIndexOf(':') + 1);
            int[] entry = palette.block(name);
            if (shortName.equals("vine")) {
                color = channels(multiply(entry != null ? entry[0] : 0x808080, tints[1]));
            } else {
                color = channels(palette.color(name, tints));
            }
        }
        int[] entry = palette.block(name);
        boolean foliage = isFoliage(palette, name);
        if (depth > 0) {
            color = mute(tone(color), 0.86f, 0.85f);
        } else if (foliage) {
            for (int i = 0; i < 3; i++) {
                color[i] *= 0.97f;
            }
        } else if (entry != null && entry[1] == T_GRASS) {
            color = tone(color);
        } else if (name.contains("aercloud")) {
            for (int i = 0; i < 3; i++) {
                color[i] *= 0.86f;
            }
        } else if (!name.endsWith(":podzol")) {
            for (int i = 0; i < 3; i++) {
                color[i] *= 1.06f;
            }
        }
        if (cave) {
            float h = height;
            float dn = h - (north != null ? north : height);
            float slope = dn < -0.25f ? 0.82f : dn > 0.25f ? 1.08f : 1f;
            float deep = Math.min(1.1f, Math.max(0.5f, 0.5f + (h + 64f) / 96f * 0.6f));
            for (int i = 0; i < 3; i++) {
                color[i] *= slope * deep;
            }
        } else if (depth == 0) {
            float h = height;
            float dn = h - (north != null ? north : height);
            float dnw = h - (west != null ? west : height);
            float light;
            if (dn < -0.25f) {
                light = 0.57f;
            } else if (dn > 0.25f) {
                light = dnw > 0.25f ? 1.18f : dnw < -0.25f ? 0.8f : 1.05f;
            } else if (Math.abs(dnw) > 0.25f) {
                light = 0.84f - Math.max(Math.min(Math.abs(dnw), 4f) - 1f, 0f) * 0.055f;
            } else {
                light = 1f;
            }
            for (int i = 0; i < 3; i++) {
                color[i] *= light;
            }
        }
        if (night) {
            float lamp = Math.min(tile.light[at] & 0xFF, 15) / 15f;
            float glow = 0.22f + 0.9f * lamp * lamp;
            color = new float[] {color[0] * glow * 1.05f, color[1] * glow, color[2] * (glow * 0.85f + 0.12f)};
        }
        return pack(color[0], color[1], color[2]);
    }

    private static float[] tone(float[] c) {
        return new float[] {c[0] * 1.12f, c[1] * 1.12f, c[2] * 1.12f};
    }

    static int[] voidColor(String realm) {
        if (realm.equals("overworld") || realm.equals("nether")) {
            return null;
        }
        if (realm.endsWith("-cave") || MapRender.depthLayer(realm)) {
            return new int[] {20, 20, 20};
        }
        return new int[] {8, 0, 24};
    }

    // ---- drawing ------------------------------------------------------------------------------------------

    /** Draws {@code size} x {@code size} pixels, each covering {@code bpp} blocks, starting at block (left, top). ARGB. */
    static int[] render(Regions regions, Palette palette, int left, int top, int size, float bpp, boolean night, int[] voidRgb, boolean cave) {
        int stride = Math.max((int) Math.floor(bpp), 1);
        Cursor cursor = new Cursor(regions);
        int span = (int) Math.ceil(size * bpp);
        TintField land = new TintField(palette, cursor, left, top, span, span, stride, 4);
        TintField water = new TintField(palette, cursor, left, top, span, span, stride, 16);
        int[] pixels = new int[size * size];
        int samples = Math.min(stride, 4);
        int step = stride / samples;
        for (int py = 0; py < size; py++) {
            for (int px = 0; px < size; px++) {
                int x = left + (int) Math.floor(px * bpp);
                int z = top + (int) Math.floor(py * bpp);
                x -= Math.floorMod(x, stride);
                z -= Math.floorMod(z, stride);
                float sr = 0;
                float sg = 0;
                float sb = 0;
                int count = 0;
                for (int sy = 0; sy < samples; sy++) {
                    for (int sx = 0; sx < samples; sx++) {
                        int bx = x + sx * step;
                        int bz = z + sy * step;
                        Tile tile = cursor.tile(bx, bz);
                        int at = cursor.at;
                        if (tile.block[at] == NONE || tile.block[at] >= tile.blocks.length) {
                            if (voidRgb != null && tile.isVoid(at)) {
                                sr += voidRgb[0];
                                sg += voidRgb[1];
                                sb += voidRgb[2];
                                count++;
                            }
                            continue;
                        }
                        Integer west = heightAt(cursor, bx - 1, bz - 1);
                        Integer north = heightAt(cursor, bx, bz - 1);
                        int[] blend = null;
                        int[] near = land.at(bx, bz);
                        if (near != null) {
                            int[] wet = water.at(bx, bz);
                            blend = new int[] {near[0], near[1], wet != null ? wet[2] : near[2]};
                        }
                        int rgb = shade(palette, tile, at, west, north, night, blend, cave);
                        sr += (rgb >> 16) & 255;
                        sg += (rgb >> 8) & 255;
                        sb += rgb & 255;
                        count++;
                    }
                }
                int out = py * size + px;
                if (count == 0) {
                    if (voidRgb != null) {
                        Tile tile = cursor.tile(x, z);
                        if (tile.isVoid(cursor.at)) {
                            pixels[out] = 0xFF000000 | voidRgb[0] << 16 | voidRgb[1] << 8 | voidRgb[2];
                        }
                    }
                    continue;
                }
                int r = Math.round(sr / count);
                int g = Math.round(sg / count);
                int b = Math.round(sb / count);
                pixels[out] = 0xFF000000 | Math.min(255, r) << 16 | Math.min(255, g) << 8 | Math.min(255, b);
            }
        }
        return pixels;
    }

    private static Integer heightAt(Cursor cursor, int x, int z) {
        Tile tile = cursor.tile(x, z);
        int at = cursor.at;
        return tile.block[at] != NONE ? Integer.valueOf(tile.height[at]) : null;
    }

    static byte[] png(int[] argb, int size) throws IOException {
        BufferedImage image = new BufferedImage(size, size, BufferedImage.TYPE_INT_ARGB);
        image.setRGB(0, 0, size, size, argb, 0, size);
        ByteArrayOutputStream out = new ByteArrayOutputStream(16 * 1024);
        ImageIO.write(image, "png", out);
        return out.toByteArray();
    }

    /** Four pictures one zoom level closer, joined into one that covers four times the ground. */
    static int[] shrink(int[][] quarters, int size) {
        int half = size / 2;
        int[] out = new int[size * size];
        for (int q = 0; q < 4; q++) {
            int[] from = quarters[q];
            if (from == null) {
                continue;
            }
            int ox = (q & 1) * half;
            int oy = (q >> 1) * half;
            for (int y = 0; y < half; y++) {
                for (int x = 0; x < half; x++) {
                    int a = 0;
                    int r = 0;
                    int g = 0;
                    int b = 0;
                    for (int dy = 0; dy < 2; dy++) {
                        for (int dx = 0; dx < 2; dx++) {
                            int p = from[(y * 2 + dy) * size + x * 2 + dx];
                            int alpha = p >>> 24;
                            a += alpha;
                            r += ((p >> 16) & 255) * alpha;
                            g += ((p >> 8) & 255) * alpha;
                            b += (p & 255) * alpha;
                        }
                    }
                    if (a == 0) {
                        continue;
                    }
                    out[(oy + y) * size + ox + x] = Math.min(255, a / 4) << 24 | (r / a) << 16 | (g / a) << 8 | (b / a);
                }
            }
        }
        return out;
    }

    static int[] decode(byte[] png, int size) throws IOException {
        BufferedImage image = ImageIO.read(new ByteArrayInputStream(png));
        if (image == null || image.getWidth() != size || image.getHeight() != size) {
            throw new IOException("Unexpected tile picture");
        }
        return image.getRGB(0, 0, size, size, null, 0, size);
    }
}

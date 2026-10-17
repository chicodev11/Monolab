
use std::io::Read;
use std::path::{Path, PathBuf};

pub const SPAN: usize = 512;
const MAX_SIDE: u32 = 4096;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Realm {
    Nether,
    End,
}

pub fn realm_key(value: &str) -> String {
    let fine = !value.is_empty()
        && value.len() <= 96
        && !value.contains("..")
        && !value.starts_with('.')
        && value.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-'));
    if fine { value.to_string() } else { "overworld".to_string() }
}

pub fn modded_dims(world: &Path) -> Vec<(String, PathBuf)> {
    fn walk(base: &Path, at: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
        let Ok(list) = std::fs::read_dir(at) else { return };
        for entry in list.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if entry.file_name() == "region" {
                let Ok(rel) = at.strip_prefix(base) else { continue };
                let rel = rel.to_string_lossy().replace('\\', "/");
                if matches!(rel.as_str(), "minecraft/overworld" | "minecraft/the_nether" | "minecraft/the_end") || rel.is_empty() {
                    continue;
                }
                let key: String = rel
                    .to_lowercase()
                    .replace('/', ".")
                    .chars()
                    .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-') { c } else { '_' })
                    .collect();
                if realm_key(&key) == key {
                    out.push((key, at.to_path_buf()));
                }
            } else if depth < 4 {
                walk(base, &path, depth + 1, out);
            }
        }
    }
    let base = world.join("dimensions");
    let mut out = Vec::new();
    walk(&base, &base, 0, &mut out);
    out.sort();
    out
}

pub fn region_dir(server: &Path, level_name: &str, realm: Realm) -> Option<PathBuf> {
    let world = server.join(level_name);
    let candidates: Vec<PathBuf> = match realm {
        Realm::Nether => vec![
            world.join("dimensions/minecraft/the_nether/region"),
            world.join("DIM-1/region"),
            server
                .join(format!("{level_name}_nether"))
                .join("DIM-1/region"),
        ],
        Realm::End => vec![
            world.join("dimensions/minecraft/the_end/region"),
            world.join("DIM1/region"),
            server
                .join(format!("{level_name}_the_end"))
                .join("DIM1/region"),
        ],
    };
    candidates.into_iter().find(|path| path.is_dir())
}

pub fn tile_name(rx: i32, rz: i32) -> String {
    format!("r.{rx}.{rz}.mnm")
}

pub fn tile_coords(name: &str) -> Option<(i32, i32)> {
    let rest = name.strip_prefix("r.")?.strip_suffix(".mnm")?;
    let (x, z) = rest.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

pub struct Tile {
    pub blocks: Vec<String>,
    pub biomes: Vec<String>,
    pub block: Vec<u16>,
    pub floor: Vec<u16>,
    pub biome: Vec<u16>,
    pub height: Vec<i16>,
    pub depth: Vec<u8>,
    pub light: Vec<u8>,
    pub present: bool,
    pub void: Vec<bool>,
    pub marks_void: bool,
}

impl Tile {
    pub fn is_void(&self, at: usize) -> bool {
        self.present && (!self.marks_void || self.void[at])
    }

    pub fn empty() -> Tile {
        Tile {
            blocks: Vec::new(),
            biomes: Vec::new(),
            block: vec![u16::MAX; SPAN * SPAN],
            floor: vec![u16::MAX; SPAN * SPAN],
            biome: vec![0; SPAN * SPAN],
            height: vec![0; SPAN * SPAN],
            depth: vec![0; SPAN * SPAN],
            light: vec![0; SPAN * SPAN],
            present: false,
            void: vec![false; SPAN * SPAN],
            marks_void: false,
        }
    }
}

fn read_utf(raw: &[u8], at: &mut usize) -> Option<String> {
    let length = u16::from_be_bytes([*raw.get(*at)?, *raw.get(*at + 1)?]) as usize;
    let text = raw.get(*at + 2..*at + 2 + length)?;
    *at += 2 + length;
    Some(String::from_utf8_lossy(text).into_owned())
}

fn read_names(raw: &[u8], at: &mut usize) -> Option<Vec<String>> {
    let count = u16::from_be_bytes([*raw.get(*at)?, *raw.get(*at + 1)?]) as usize;
    *at += 2;
    (0..count).map(|_| read_utf(raw, at)).collect()
}

pub fn parse_tile(bytes: &[u8]) -> Option<Tile> {
    let mut raw = Vec::with_capacity(SPAN * SPAN * 10 + 4096);
    flate2::read::GzDecoder::new(bytes).read_to_end(&mut raw).ok()?;
    if raw.get(..4)? != b"MNM2" {
        return None;
    }
    let mut at = 4;
    let blocks = read_names(&raw, &mut at)?;
    let biomes = read_names(&raw, &mut at)?;
    let cells = raw.get(at..at + SPAN * SPAN * 10)?;
    let mut tile = Tile::empty();
    let lift: Vec<i16> = blocks.iter().map(|name| plant_lift(name)).collect();
    tile.blocks = blocks;
    tile.biomes = biomes;
    tile.present = true;
    for (index, cell) in cells.as_chunks::<10>().0.iter().enumerate() {
        tile.block[index] = u16::from_be_bytes([cell[0], cell[1]]);
        if tile.block[index] == 0xFFFE {
            tile.block[index] = u16::MAX;
            tile.void[index] = true;
            tile.marks_void = true;
        }
        tile.floor[index] = u16::from_be_bytes([cell[2], cell[3]]);
        tile.biome[index] = u16::from_be_bytes([cell[4], cell[5]]);
        let lifted = lift.get(tile.block[index] as usize).copied().unwrap_or(0);
        tile.height[index] = i16::from_be_bytes([cell[6], cell[7]]) - lifted;
        tile.depth[index] = cell[8];
        tile.light[index] = cell[9];
    }
    Some(tile)
}

fn plant_lift(name: &str) -> i16 {
    let short = name.rsplit(':').next().unwrap_or(name);
    const TALL: [&str; 7] = ["tall_grass", "large_fern", "sunflower", "lilac", "rose_bush", "peony", "tall_seagrass"];
    if TALL.contains(&short) || short.starts_with("tall_") {
        return 2;
    }
    const LOW: [&str; 24] = [
        "short_grass", "grass", "fern", "dead_bush", "dandelion", "poppy", "blue_orchid", "allium", "azure_bluet",
        "oxeye_daisy", "cornflower", "lily_of_the_valley", "wither_rose", "torchflower", "pink_petals", "wildflowers",
        "leaf_litter", "bush", "firefly_bush", "short_dry_grass", "snow", "moss_carpet", "pale_moss_carpet", "sweet_berry_bush",
    ];
    if LOW.contains(&short)
        || short.ends_with("_tulip")
        || short.ends_with("_sapling")
        || short.ends_with("_mushroom")
        || short.ends_with("_carpet")
        || short.ends_with("_flower")
        || short.ends_with("_grass") && short != "grass_block"
    {
        return 1;
    }
    0
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tint {
    None,
    Grass,
    Foliage,
    Water,
    Fixed(u32),
}

#[derive(Clone, Debug)]
pub struct Biome {
    pub temperature: f32,
    pub downfall: f32,
    pub water: u32,
    pub grass: Option<u32>,
    pub foliage: Option<u32>,
    pub modifier: String,
}

pub struct Palette {
    pub blocks: std::collections::HashMap<String, (u32, Tint)>,
    pub biomes: std::collections::HashMap<String, Biome>,
    pub grass: Option<image::RgbImage>,
    pub foliage: Option<image::RgbImage>,
}

fn hex(text: &str) -> Option<u32> {
    u32::from_str_radix(text, 16).ok().filter(|_| !text.is_empty())
}

impl Palette {
    pub fn empty() -> Palette {
        Palette {
            blocks: Default::default(),
            biomes: Default::default(),
            grass: None,
            foliage: None,
        }
    }

    pub fn parse(json: &[u8], grass: Option<&[u8]>, foliage: Option<&[u8]>) -> Palette {
        let mut palette = Palette::empty();
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(json) else {
            return palette;
        };
        if let Some(blocks) = value.get("blocks").and_then(|one| one.as_object()) {
            for (name, entry) in blocks {
                let color = entry.get(0).and_then(|one| one.as_str()).and_then(hex).unwrap_or(0x80_80_80);
                let tint = match entry.get(1).and_then(|one| one.as_str()).unwrap_or("none") {
                    "grass" => Tint::Grass,
                    "foliage" => Tint::Foliage,
                    "water" => Tint::Water,
                    other => other.strip_prefix("fixed:").and_then(hex).map(Tint::Fixed).unwrap_or(Tint::None),
                };
                palette.blocks.insert(name.clone(), (color, tint));
            }
        }
        if let Some(biomes) = value.get("biomes").and_then(|one| one.as_object()) {
            for (name, entry) in biomes {
                let text = |at: usize| entry.get(at).and_then(|one| one.as_str()).unwrap_or("");
                palette.biomes.insert(
                    name.clone(),
                    Biome {
                        temperature: entry.get(0).and_then(|one| one.as_f64()).unwrap_or(0.8) as f32,
                        downfall: entry.get(1).and_then(|one| one.as_f64()).unwrap_or(0.4) as f32,
                        water: hex(text(2)).unwrap_or(0x3f_76_e4),
                        grass: hex(text(3)),
                        foliage: hex(text(4)),
                        modifier: text(5).to_string(),
                    },
                );
            }
        }
        let decode = |bytes: Option<&[u8]>| bytes.and_then(|one| image::load_from_memory(one).ok()).map(|one| one.to_rgb8());
        palette.grass = decode(grass);
        palette.foliage = decode(foliage);
        palette
    }

    fn colormap(map: &Option<image::RgbImage>, biome: &Biome, otherwise: u32) -> u32 {
        let Some(map) = map else { return otherwise };
        let temp = biome.temperature.clamp(0.0, 1.0);
        let rain = biome.downfall.clamp(0.0, 1.0) * temp;
        let x = (((1.0 - temp) * 255.0) as u32).min(map.width() - 1);
        let y = (((1.0 - rain) * 255.0) as u32).min(map.height() - 1);
        let [r, g, b] = map.get_pixel(x, y).0;
        u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
    }

    fn tints(&self, biome: Option<&Biome>) -> [u32; 3] {
        let fallback = Biome {
            temperature: 0.8,
            downfall: 0.4,
            water: 0x3f_76_e4,
            grass: None,
            foliage: None,
            modifier: String::new(),
        };
        let biome = biome.unwrap_or(&fallback);
        let grass = biome.grass.unwrap_or_else(|| Palette::colormap(&self.grass, biome, 0x91_bd_59));
        let grass = match biome.modifier.as_str() {
            "swamp" => 0x6a_70_39,
            "dark_forest" => ((grass & 0xfe_fe_fe) + 0x28_34_0a) >> 1,
            _ => grass,
        };
        let foliage = biome.foliage.unwrap_or_else(|| Palette::colormap(&self.foliage, biome, 0x48_b5_18));
        [grass, foliage, biome.water]
    }

    #[cfg(test)]
    fn color(&self, name: &str, biome: Option<&Biome>) -> u32 {
        self.color_with(name, self.tints(biome))
    }

    fn color_with(&self, name: &str, tints: [u32; 3]) -> u32 {
        let (base, tint) = self.blocks.get(name).copied().unwrap_or((0x80_80_80, Tint::None));
        let tint = match tint {
            Tint::None => return base,
            Tint::Fixed(color) => color,
            Tint::Grass => tints[0],
            Tint::Foliage => tints[1],
            Tint::Water => tints[2],
        };
        multiply(base, tint)
    }
}

struct TintField {
    x0: i32,
    z0: i32,
    cell: i32,
    w: usize,
    h: usize,
    data: Vec<Option<[f32; 9]>>,
}

impl TintField {
    #[allow(clippy::too_many_arguments)]
    fn build(
        palette: &Palette,
        tile_at: &mut dyn FnMut(i32, i32) -> (std::sync::Arc<Tile>, usize),
        left: i32,
        top: i32,
        span_x: i32,
        span_z: i32,
        stride: i32,
        radius: i32,
    ) -> TintField {
        let cell = stride.max(4);
        let reach = (radius / cell).max(1);
        let margin = (reach + 2) * cell;
        let (x0, z0) = (left - margin, top - margin);
        let w = ((span_x + 2 * margin) / cell + 1) as usize;
        let h = ((span_z + 2 * margin) / cell + 1) as usize;
        let mut raw: Vec<Option<[f32; 9]>> = vec![None; w * h];
        for j in 0..h {
            for i in 0..w {
                let (bx, bz) = (x0 + i as i32 * cell + cell / 2, z0 + j as i32 * cell + cell / 2);
                let (tile, at) = tile_at(bx, bz);
                if tile.block[at] == u16::MAX {
                    continue;
                }
                let Some(biome) = tile.biomes.get(tile.biome[at] as usize).and_then(|one| palette.biomes.get(one)) else {
                    continue;
                };
                let tints = palette.tints(Some(biome));
                let mut value = [0f32; 9];
                for (k, tint) in tints.iter().enumerate() {
                    value[k * 3..k * 3 + 3].copy_from_slice(&channels(*tint));
                }
                raw[j * w + i] = Some(value);
            }
        }
        let blur = |src: &[Option<[f32; 9]>], horizontal: bool| -> Vec<Option<[f32; 9]>> {
            let mut out = vec![None; w * h];
            for j in 0..h {
                for i in 0..w {
                    let mut sum = [0f32; 9];
                    let mut n = 0f32;
                    for d in -reach..=reach {
                        let (ii, jj) = if horizontal { (i as i32 + d, j as i32) } else { (i as i32, j as i32 + d) };
                        if ii < 0 || jj < 0 || ii >= w as i32 || jj >= h as i32 {
                            continue;
                        }
                        if let Some(value) = src[jj as usize * w + ii as usize] {
                            for k in 0..9 {
                                sum[k] += value[k];
                            }
                            n += 1.0;
                        }
                    }
                    if n > 0.0 {
                        out[j * w + i] = Some(sum.map(|one| one / n));
                    }
                }
            }
            out
        };
        let data = blur(&blur(&raw, true), false);
        TintField { x0, z0, cell, w, h, data }
    }

    fn at(&self, x: i32, z: i32) -> Option<[u32; 3]> {
        let fx = (x - self.x0) as f32 / self.cell as f32 - 0.5;
        let fz = (z - self.z0) as f32 / self.cell as f32 - 0.5;
        let (i0, j0) = (fx.floor() as i32, fz.floor() as i32);
        let (tx, tz) = (fx - i0 as f32, fz - j0 as f32);
        let mut sum = [0f32; 9];
        let mut weight = 0f32;
        for (di, dj, part) in [(0, 0, (1.0 - tx) * (1.0 - tz)), (1, 0, tx * (1.0 - tz)), (0, 1, (1.0 - tx) * tz), (1, 1, tx * tz)] {
            let (i, j) = (i0 + di, j0 + dj);
            if i < 0 || j < 0 || i >= self.w as i32 || j >= self.h as i32 || part <= 0.0 {
                continue;
            }
            if let Some(value) = self.data[j as usize * self.w + i as usize] {
                for k in 0..9 {
                    sum[k] += value[k] * part;
                }
                weight += part;
            }
        }
        (weight > 0.0).then(|| {
            let v = sum.map(|one| one / weight);
            [pack([v[0], v[1], v[2]]), pack([v[3], v[4], v[5]]), pack([v[6], v[7], v[8]])]
        })
    }
}

fn channels(color: u32) -> [f32; 3] {
    [((color >> 16) & 255) as f32, ((color >> 8) & 255) as f32, (color & 255) as f32]
}

fn multiply(a: u32, b: u32) -> u32 {
    let (a, b) = (channels(a), channels(b));
    pack([a[0] * b[0] / 255.0, a[1] * b[1] / 255.0, a[2] * b[2] / 255.0])
}

fn mute(c: [f32; 3], dark: f32, saturation: f32) -> [f32; 3] {
    let luma = c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114;
    c.map(|one| luma * dark + (one - luma) * saturation)
}

fn tone(c: [f32; 3]) -> [f32; 3] {
    c.map(|one| one * 1.12)
}

fn pack(c: [f32; 3]) -> u32 {
    let one = |v: f32| v.clamp(0.0, 255.0) as u32;
    one(c[0]) << 16 | one(c[1]) << 8 | one(c[2])
}

fn is_foliage(palette: &Palette, name: &str) -> bool {
    let short = name.rsplit(':').next().unwrap_or(name);
    matches!(palette.blocks.get(name), Some((_, Tint::Foliage)))
        || short.contains("leaves")
        || short == "vine"
        || short.ends_with("_vines")
        || short.ends_with("_vine")
}

#[allow(clippy::too_many_arguments)]
fn shade(
    palette: &Palette,
    tile: &Tile,
    at: usize,
    west: Option<i16>,
    north: Option<i16>,
    scale: i32,
    night: bool,
    blend: Option<[u32; 3]>,
    cave: bool,
) -> u32 {
    let name = &tile.blocks[tile.block[at] as usize];
    let biome = tile.biomes.get(tile.biome[at] as usize).and_then(|one| palette.biomes.get(one));
    let tints = blend.unwrap_or_else(|| palette.tints(biome));
    let depth = tile.depth[at];
    let height = tile.height[at];
    let mut color = channels(if depth > 0 {
        let water = palette.color_with("minecraft:water", tints);
        match tile.blocks.get(tile.floor[at] as usize) {
            Some(floor) => {
                let bed = channels(palette.color_with(floor, tints)).map(|one| one * 0.8);
                let water = channels(water);
                let a = (0.45 + f32::from(depth) * 0.035).min(0.9);
                let dark = 1.0 - f32::from(depth.min(30)) * 0.011;
                pack([0, 1, 2].map(|i| (bed[i] * (1.0 - a) + water[i] * a) * dark))
            }
            None => water,
        }
    } else {
        let short = name.rsplit(':').next().unwrap_or(name);
        if short == "vine" {
            multiply(palette.blocks.get(name.as_str()).map(|one| one.0).unwrap_or(0x80_80_80), tints[1])
        } else {
            palette.color_with(name, tints)
        }
    });
    let foliage = is_foliage(palette, name);
    if depth > 0 {
        color = mute(tone(color), 0.86, 0.85);
    } else if foliage {
        color = color.map(|one| one * 0.97);
    } else if matches!(palette.blocks.get(name.as_str()), Some((_, Tint::Grass))) {
        color = tone(color);
    } else if name.contains("aercloud") {
        color = color.map(|one| one * 0.86);
    } else if !name.ends_with(":podzol") {
        color = color.map(|one| one * 1.06);
    }
    if cave {
        let h = f32::from(height);
        let dn = (h - f32::from(north.unwrap_or(height))) / scale as f32;
        let slope = if dn < -0.25 { 0.82 } else if dn > 0.25 { 1.08 } else { 1.0 };
        let deep = (0.5 + (h + 64.0) / 96.0 * 0.6).clamp(0.5, 1.1);
        color = color.map(|one| one * slope * deep);
    } else if depth == 0 {
        let h = f32::from(height);
        let dn = (h - f32::from(north.unwrap_or(height))) / scale as f32;
        let dnw = (h - f32::from(west.unwrap_or(height))) / scale as f32;
        let light = if dn < -0.25 {
            0.57
        } else if dn > 0.25 {
            if dnw > 0.25 {
                1.18
            } else if dnw < -0.25 {
                0.8
            } else {
                1.05
            }
        } else if dnw.abs() > 0.25 {
            0.84 - (dnw.abs().min(4.0) - 1.0).max(0.0) * 0.055
        } else {
            1.0
        };
        color = color.map(|one| one * light);
    }
    if night {
        let lamp = f32::from(tile.light[at].min(15)) / 15.0;
        let glow = 0.22 + 0.9 * lamp * lamp;
        color = [color[0] * glow * 1.05, color[1] * glow, color[2] * (glow * 0.85 + 0.12)];
    }
    pack(color)
}

pub fn void_color(realm: &str) -> Option<[u8; 3]> {
    match realm {
        "overworld" | "nether" => None,
        key if key.ends_with("-cave") => Some([20, 20, 20]),
        _ => Some([8, 0, 24]),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render_view(
    fetch: &mut dyn FnMut(i32, i32) -> std::sync::Arc<Tile>,
    palette: &Palette,
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    blocks_per_pixel: f32,
    night: bool,
    void: Option<[u8; 3]>,
    cave: bool,
) -> Result<Vec<u8>, String> {
    let width = width.clamp(16, MAX_SIDE);
    let height = height.clamp(16, MAX_SIDE);
    let bpp = blocks_per_pixel.clamp(0.05, 64.0);
    let stride = bpp.floor().max(1.0) as i32;
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let mut held: Vec<((i32, i32), std::sync::Arc<Tile>)> = Vec::new();
    let mut tile_at = |x: i32, z: i32| -> (std::sync::Arc<Tile>, usize) {
        let rx = x.div_euclid(SPAN as i32);
        let rz = z.div_euclid(SPAN as i32);
        let tile = match held.iter().find(|(at, _)| *at == (rx, rz)) {
            Some((_, tile)) => tile.clone(),
            None => {
                let tile = fetch(rx, rz);
                held.push(((rx, rz), tile.clone()));
                tile
            }
        };
        let at = z.rem_euclid(SPAN as i32) as usize * SPAN + x.rem_euclid(SPAN as i32) as usize;
        (tile, at)
    };
    let (span_x, span_z) = ((width as f32 * bpp).ceil() as i32, (height as f32 * bpp).ceil() as i32);
    let land = TintField::build(palette, &mut tile_at, left, top, span_x, span_z, stride, 4);
    let water = TintField::build(palette, &mut tile_at, left, top, span_x, span_z, stride, 16);
    let height_at = |x: i32, z: i32, tile_at: &mut dyn FnMut(i32, i32) -> (std::sync::Arc<Tile>, usize)| {
        let (tile, at) = tile_at(x, z);
        (tile.block[at] != u16::MAX).then(|| tile.height[at])
    };
    for py in 0..height {
        for px in 0..width {
            let x = left + (px as f32 * bpp).floor() as i32;
            let z = top + (py as f32 * bpp).floor() as i32;
            let (x, z) = (x - x.rem_euclid(stride), z - z.rem_euclid(stride));
            let samples = stride.min(4);
            let step = stride / samples;
            let mut sum = [0f32; 3];
            let mut count = 0f32;
            for sy in 0..samples {
                for sx in 0..samples {
                    let (bx, bz) = (x + sx * step, z + sy * step);
                    let (tile, at) = tile_at(bx, bz);
                    if tile.block[at] == u16::MAX || tile.block[at] as usize >= tile.blocks.len() {
                        if let (Some(color), true) = (void, tile.is_void(at)) {
                            for i in 0..3 {
                                sum[i] += f32::from(color[i]);
                            }
                            count += 1.0;
                        }
                        continue;
                    }
                    let west = height_at(bx - 1, bz - 1, &mut tile_at);
                    let north = height_at(bx, bz - 1, &mut tile_at);
                    let color = channels(shade(palette, &tile, at, west, north, 1, night, land.at(bx, bz).map(|one| [one[0], one[1], water.at(bx, bz).map_or(one[2], |w| w[2])]), cave));
                    for i in 0..3 {
                        sum[i] += color[i];
                    }
                    count += 1.0;
                }
            }
            let out = ((py * width + px) * 4) as usize;
            if count == 0.0 {
                if let Some(color) = void {
                    let (tile, at) = tile_at(x, z);
                    if tile.is_void(at) {
                        pixels[out..out + 3].copy_from_slice(&color);
                        pixels[out + 3] = 255;
                    }
                }
                continue;
            }
            for i in 0..3 {
                pixels[out + i] = (sum[i] / count).round().clamp(0.0, 255.0) as u8;
            }
            pixels[out + 3] = 255;
        }
    }
    let image = image::RgbaImage::from_raw(width, height, pixels).ok_or("Could not create the map image.")?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;

    #[test]
    fn finds_modded_dimensions_and_keeps_keys_safe() {
        let root = std::env::temp_dir().join(format!("se-dims-{}", std::process::id()));
        let world = root.join("world");
        for dir in [
            "dimensions/minecraft/overworld/region",
            "dimensions/aether/the_aether/region",
            "dimensions/twilightforest/twilight_forest/region",
        ] {
            std::fs::create_dir_all(world.join(dir)).unwrap();
        }
        let keys: Vec<String> = modded_dims(&world).into_iter().map(|(key, _)| key).collect();
        assert_eq!(keys, ["aether.the_aether", "twilightforest.twilight_forest"]);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(realm_key("nether"), "nether");
        assert_eq!(realm_key("aether.the_aether"), "aether.the_aether");
        assert_eq!(realm_key("../etc"), "overworld");
        assert_eq!(realm_key("a/b"), "overworld");
        assert_eq!(realm_key(""), "overworld");
    }

    fn tile_bytes(blocks: &[&str], fill: impl Fn(usize, usize) -> (u16, u16, i16, u8)) -> Vec<u8> {
        let mut raw = b"MNM2".to_vec();
        let names = |raw: &mut Vec<u8>, list: &[&str]| {
            raw.extend_from_slice(&(list.len() as u16).to_be_bytes());
            for one in list {
                raw.extend_from_slice(&(one.len() as u16).to_be_bytes());
                raw.extend_from_slice(one.as_bytes());
            }
        };
        names(&mut raw, blocks);
        names(&mut raw, &["minecraft:plains"]);
        for z in 0..SPAN {
            for x in 0..SPAN {
                let (block, floor, height, depth) = fill(x, z);
                raw.extend_from_slice(&block.to_be_bytes());
                raw.extend_from_slice(&floor.to_be_bytes());
                raw.extend_from_slice(&0u16.to_be_bytes());
                raw.extend_from_slice(&height.to_be_bytes());
                raw.push(depth);
                raw.push(0);
            }
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&raw).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn reads_tile_names() {
        assert_eq!(tile_coords("r.0.0.mnm"), Some((0, 0)));
        assert_eq!(tile_coords("r.-1.2.mnm"), Some((-1, 2)));
        assert_eq!(tile_coords("r.0.0.mca"), None);
        assert_eq!(tile_name(-3, 4), "r.-3.4.mnm");
    }

    #[test]
    fn parses_what_the_addon_writes() {
        let bytes = tile_bytes(&["minecraft:stone", "minecraft:water"], |x, _| {
            if x == 0 { (u16::MAX, u16::MAX, 0, 0) } else { (0, u16::MAX, x as i16 - 64, 0) }
        });
        let tile = parse_tile(&bytes).expect("could not read the tile");
        assert_eq!(tile.blocks, ["minecraft:stone", "minecraft:water"]);
        assert_eq!(tile.height[3 * SPAN + 5], 5 - 64);
        assert_eq!(tile.block[3 * SPAN], u16::MAX);
        assert!(parse_tile(b"not gzip").is_none());
    }

    #[test]
    fn paints_the_void_only_where_the_addon_drew() {
        let palette = Palette::parse(br#"{"blocks":{"minecraft:end_stone":["dbde9e","none"]},"biomes":{}}"#, None, None);
        let drawn = Arc::new(parse_tile(&tile_bytes(&["minecraft:end_stone"], |x, _| {
            if x < 256 { (0, u16::MAX, 60, 0) } else { (u16::MAX, u16::MAX, 0, 0) }
        })).unwrap());
        let mut fetch = |rx: i32, _rz: i32| if rx == 0 { drawn.clone() } else { Arc::new(Tile::empty()) };
        let png = render_view(&mut fetch, &palette, 0, 0, 64, 16, 16.0, false, void_color("end"), false).unwrap();
        let image = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(image.get_pixel(20, 8).0, [8, 0, 24, 255], "empty cells in drawn regions are void");
        assert_eq!(image.get_pixel(40, 8).0[3], 0, "missing regions stay transparent");
        assert_ne!(image.get_pixel(4, 8).0, [8, 0, 24, 255], "islands keep their color");
        assert_eq!(void_color("overworld"), None);
        assert_eq!(void_color("overworld-cave"), Some([20, 20, 20]));
        assert_eq!(realm_key("overworld-cave"), "overworld-cave");

        let marked = Arc::new(parse_tile(&tile_bytes(&["minecraft:end_stone"], |x, _| {
            if x < 256 { (0, u16::MAX, 60, 0) } else if x < 384 { (0xFFFE, u16::MAX, 0, 0) } else { (u16::MAX, u16::MAX, 0, 0) }
        })).unwrap());
        assert!(marked.marks_void && marked.block[300] == u16::MAX);
        let mut fetch = |_rx: i32, _rz: i32| marked.clone();
        let png = render_view(&mut fetch, &palette, 0, 0, 512, 16, 16.0, false, void_color("end"), false).unwrap();
        let image = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(image.get_pixel(20, 8).0, [8, 0, 24, 255], "recorded void");
        assert_eq!(image.get_pixel(28, 8).0[3], 0, "ungenerated chunks are transparent");
    }

    #[test]
    fn tints_grass_by_biome_and_lets_the_seabed_show() {
        let palette = Palette::parse(
            br#"{"blocks":{"minecraft:grass_block":["ffffff","grass"],"minecraft:water":["ffffff","water"],"minecraft:sand":["e0d0a0","none"]},
                 "biomes":{"minecraft:plains":[0.8,0.4,"3f76e4","91bd59","",""],"minecraft:swamp":[0.8,0.9,"617b64","","","swamp"]}}"#,
            None,
            None,
        );
        assert_eq!(palette.color("minecraft:grass_block", palette.biomes.get("minecraft:plains")), 0x91_bd_59);
        assert_eq!(palette.color("minecraft:grass_block", palette.biomes.get("minecraft:swamp")), 0x6a_70_39);
        let tile = Arc::new(parse_tile(&tile_bytes(&["minecraft:water", "minecraft:sand"], |x, _| (0, 1, 62, if x < 8 { 1 } else { 12 }))).unwrap());
        let shallow = channels(shade(&palette, &tile, 2, None, None, 1, false, None, false));
        let deep = channels(shade(&palette, &tile, 20, None, None, 1, false, None, false));
        assert!(shallow[0] > deep[0] + 8.0, "{shallow:?} {deep:?}");
    }

    #[test]
    fn lights_slopes_from_the_north_west() {
        let palette = Palette::parse(br#"{"blocks":{"minecraft:stone":["808080","none"]},"biomes":{}}"#, None, None);
        let tile = Arc::new(parse_tile(&tile_bytes(&["minecraft:stone"], |_, _| (0, u16::MAX, 70, 0))).unwrap());
        let flat = channels(shade(&palette, &tile, 0, Some(70), Some(70), 1, false, None, false))[0];
        let facing = channels(shade(&palette, &tile, 0, Some(66), Some(66), 1, false, None, false))[0];
        let away = channels(shade(&palette, &tile, 0, Some(74), Some(74), 1, false, None, false))[0];
        assert!(facing > flat && flat > away);
    }

    #[test]
    #[ignore]
    fn draws_real_tiles() {
        let dir = std::path::PathBuf::from(std::env::var("SE_MAP_DIR").expect("SE_MAP_DIR"));
        let realm = std::env::var("SE_MAP_REALM").unwrap_or_else(|_| "overworld".into());
        let out = std::env::var("SE_MAP_OUT").expect("SE_MAP_OUT");
        let read = |name: &str| std::fs::read(dir.join(name)).ok();
        let palette = Palette::parse(&read("palette.json").unwrap_or_default(), read("grass.png").as_deref(), read("foliage.png").as_deref());
        let mut fetch = |rx: i32, rz: i32| {
            Arc::new(
                std::fs::read(dir.join(&realm).join(tile_name(rx, rz)))
                    .ok()
                    .and_then(|bytes| parse_tile(&bytes))
                    .unwrap_or_else(Tile::empty),
            )
        };
        let left: i32 = std::env::var("SE_MAP_LEFT").ok().and_then(|one| one.parse().ok()).unwrap_or(-512);
        let top: i32 = std::env::var("SE_MAP_TOP").ok().and_then(|one| one.parse().ok()).unwrap_or(-512);
        let size: u32 = std::env::var("SE_MAP_SIZE").ok().and_then(|one| one.parse().ok()).unwrap_or(1024);
        let night = std::env::var("SE_MAP_NIGHT").is_ok();
        let bpp: f32 = std::env::var("SE_MAP_BPP").ok().and_then(|one| one.parse().ok()).unwrap_or(1.0);
        let tall: u32 = std::env::var("SE_MAP_TALL").ok().and_then(|one| one.parse().ok()).unwrap_or(size);
        let px = |blocks: u32| ((blocks as f32) / bpp).round() as u32;
        let png = render_view(&mut fetch, &palette, left, top, px(size), px(tall), bpp, night, void_color(&realm), realm.ends_with("-cave")).unwrap();
        std::fs::write(out, png).unwrap();
    }
}

#[cfg(test)]
mod block_count {
    use super::*;
    #[test]
    #[ignore]
    fn counts_real_blocks() {
        let dir = std::path::PathBuf::from(std::env::var("SE_MAP_DIR").expect("SE_MAP_DIR"));
        let num = |key: &str| -> i32 { std::env::var(key).ok().and_then(|one| one.parse().ok()).unwrap() };
        let (x0, z0, w, h) = (num("SE_X"), num("SE_Z"), num("SE_W"), num("SE_H"));
        let palette = Palette::parse(&std::fs::read(dir.join("palette.json")).unwrap(), None, None);
        let picture = std::env::var("SE_PNG").ok().and_then(|f| image::open(f).ok()).map(|one| one.to_rgba8());
        let mut count: std::collections::BTreeMap<String, usize> = Default::default();
        for z in z0..z0 + h {
            for x in x0..x0 + w {
                let (rx, rz) = (x.div_euclid(512), z.div_euclid(512));
                let realm = std::env::var("SE_MAP_REALM").unwrap_or_else(|_| "overworld".into());
                let Some(tile) = std::fs::read(dir.join(realm).join(tile_name(rx, rz))).ok().and_then(|b| parse_tile(&b)) else { continue };
                let at = (z.rem_euclid(512) as usize) * SPAN + x.rem_euclid(512) as usize;
                let name = tile.blocks.get(tile.block[at] as usize).cloned().unwrap_or_default();
                let tint = palette.blocks.get(&name).map(|one| format!("{:?}", one.1)).unwrap_or("?".into());
                if let Some(png) = &picture {
                    let (px, pz) = ((x + 512) as u32, (z + 512) as u32);
                    if px >= png.width() || pz >= png.height() {
                        continue;
                    }
                    let [r, g, _, _] = png.get_pixel(px, pz).0;
                    if !(g > 105 && f32::from(g) > f32::from(r) * 1.9) {
                        continue;
                    }
                }
                *count.entry(format!("{name} [{tint}]")).or_default() += 1;
            }
        }
        let mut list: Vec<_> = count.into_iter().collect();
        list.sort_by_key(|one| std::cmp::Reverse(one.1));
        for (name, n) in list.iter().take(15) {
            println!("{n:6} {name}");
        }
    }
}

#[cfg(test)]
mod block_dump {
    use super::*;
    #[test]
    #[ignore]
    fn dumps_blocks() {
        let dir = std::path::PathBuf::from(std::env::var("SE_MAP_DIR").expect("SE_MAP_DIR"));
        let num = |key: &str| -> i32 { std::env::var(key).ok().and_then(|one| one.parse().ok()).unwrap() };
        let (x0, z0, w, h) = (num("SE_X"), num("SE_Z"), num("SE_W"), num("SE_H"));
        let mut tiles: std::collections::HashMap<(i32, i32), Option<Tile>> = Default::default();
        let mut out = String::new();
        for z in z0..z0 + h {
            for x in x0..x0 + w {
                let (rx, rz) = (x.div_euclid(512), z.div_euclid(512));
                let tile = tiles
                    .entry((rx, rz))
                    .or_insert_with(|| std::fs::read(dir.join("overworld").join(tile_name(rx, rz))).ok().and_then(|b| parse_tile(&b)));
                let line = match tile {
                    Some(tile) => {
                        let at = (z.rem_euclid(512) as usize) * SPAN + x.rem_euclid(512) as usize;
                        format!(
                            "{}\t{}\t{}\t{}\n",
                            tile.blocks.get(tile.block[at] as usize).cloned().unwrap_or_default(),
                            tile.biomes.get(tile.biome[at] as usize).cloned().unwrap_or_default(),
                            tile.depth[at],
                            tile.height[at]
                        )
                    }
                    None => "\t\t0\t0\n".to_string(),
                };
                out.push_str(&line);
            }
        }
        std::fs::write(std::env::var("SE_DUMP").unwrap(), out).unwrap();
    }
}

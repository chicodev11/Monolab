use pumpkin_plugin_api::Server;
use pumpkin_plugin_api::wit::pumpkin::plugin::world::{BlockPos, World};
use serde_json::{Map as JsonMap, Value, json};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

const RADIUS: i32 = 6;
const REFILL_MS: u64 = 2_000;
const FLUSH_MS: u64 = 1_000;
const BUDGET_MICROS: u128 = 1_000;
const SPAN: usize = 512;
const NONE: i32 = -1;
const VOID: i32 = -2;
const KEEP_REGIONS: usize = 6;
const NETHER_CEILING: i32 = 127;
const WANTED_SECONDS: u64 = 600;
const ALWAYS_WET: [&str; 6] = [
    "minecraft:water",
    "minecraft:kelp",
    "minecraft:kelp_plant",
    "minecraft:seagrass",
    "minecraft:tall_seagrass",
    "minecraft:bubble_column",
];

/// Vanilla map item colors by map color id, used until the app supplies a palette from the client jar.
const MAP_COLORS: [u32; 62] = [
    0x000000, 0x7FB238, 0xF7E9A3, 0xC7C7C7, 0xFF0000, 0xA0A0FF, 0xA7A7A7, 0x007C00, 0xFFFFFF, 0xA4A8B8, 0x976D4D,
    0x707070, 0x4040FF, 0x8F7748, 0xFFFCF5, 0xD87F33, 0xB24CD8, 0x6699D8, 0xE5E533, 0x7FCC19, 0xF27FA5, 0x4C4C4C,
    0x999999, 0x4C7F99, 0x7F3FB2, 0x334CB2, 0x664C33, 0x667F33, 0x993333, 0x191919, 0xFAEE4D, 0x5CDBD5, 0x4A80FF,
    0x00D93A, 0x815631, 0x700200, 0xD1B1A1, 0x9F5224, 0x95576C, 0x706C8A, 0xBA8524, 0x677535, 0xA04D4E, 0x392923,
    0x876B62, 0x575C5C, 0x7A4958, 0x4C3E5C, 0x4C3223, 0x4C522A, 0x8E3C2E, 0x251610, 0xBD3031, 0x943F61, 0x5C191D,
    0x167E86, 0x3A8E8C, 0x562C3E, 0x14B485, 0x646464, 0xD8AF93, 0x7FA796,
];

struct Look {
    name: String,
    visible: bool,
    water: bool,
    color: u8,
}

struct Columns {
    top: Vec<Option<String>>,
    floor: Vec<Option<String>>,
    biome: Vec<String>,
    height: [i16; 256],
    depth: [u8; 256],
    light: [u8; 256],
    empty: [bool; 256],
    hash: u64,
}

struct Region {
    blocks: Vec<String>,
    block_ids: HashMap<String, i32>,
    biomes: Vec<String>,
    biome_ids: HashMap<String, u16>,
    block: Vec<i32>,
    floor: Vec<i32>,
    biome: Vec<u16>,
    height: Vec<i16>,
    depth: Vec<u8>,
    light: Vec<u8>,
}

type Key = (String, i32, i32);
type Batch = Vec<(i32, i32, Columns)>;

/// Draws the map from loaded chunks near players, the way `MapLive` does on Java servers.
/// Pumpkin plugins can't read region files, so this is the only source of map tiles there.
pub struct Map {
    out: PathBuf,
    worlds: HashMap<String, (World, i32)>,
    queue: VecDeque<Key>,
    filled_at: u64,
    flushed_at: u64,
    seen: HashMap<Key, u64>,
    looks: HashMap<u16, Look>,
    pending: HashMap<Key, Batch>,
    regions: Vec<(Key, Region)>,
    colors: HashMap<String, u8>,
    colors_changed: bool,
    depth: Option<Depth>,
}

/// Progress of the height layer the app is showing: chunks queued for it and chunks looked at so far.
struct Depth {
    layer: String,
    queued: HashSet<(i32, i32)>,
    done: HashSet<(i32, i32)>,
    written: (usize, usize),
}

impl Map {
    pub fn new(folder: &Path) -> Self {
        Self {
            out: folder.join("map"),
            worlds: HashMap::new(),
            queue: VecDeque::new(),
            filled_at: 0,
            flushed_at: 0,
            seen: HashMap::new(),
            looks: HashMap::new(),
            pending: HashMap::new(),
            regions: Vec::new(),
            colors: HashMap::new(),
            colors_changed: false,
            depth: None,
        }
    }

    pub fn tick(&mut self, server: &Server, now: u64) -> Result<(), String> {
        if self.queue.is_empty() && now.saturating_sub(self.filled_at) >= REFILL_MS {
            self.filled_at = now;
            self.fill(server);
        }
        let start = Instant::now();
        while let Some((layer, cx, cz)) = self.queue.pop_front() {
            if let Some(depth) = self.depth.as_mut().filter(|depth| depth.layer == layer) {
                depth.done.insert((cx, cz));
            }
            if let Some(columns) = self.sample(&layer, cx, cz) {
                let key = (layer.clone(), cx, cz);
                if self.seen.insert(key, columns.hash) != Some(columns.hash) {
                    let region = (layer, cx.div_euclid(32), cz.div_euclid(32));
                    self.pending.entry(region).or_default().push((cx, cz, columns));
                }
            }
            if start.elapsed().as_micros() >= BUDGET_MICROS {
                break;
            }
        }
        if now.saturating_sub(self.flushed_at) >= FLUSH_MS {
            self.flushed_at = now;
            self.flush()?;
            self.write_depth_status(now)
                .map_err(|error| format!("Couldn't write the map height progress: {error}"))?;
        }
        Ok(())
    }

    /// The same `status.json` the Java addons write, so the app can show how far a height layer got.
    fn write_depth_status(&mut self, now: u64) -> std::io::Result<()> {
        let Some(depth) = self.depth.as_mut() else {
            return Ok(());
        };
        let counts = (depth.done.len(), depth.queued.len());
        if counts == depth.written {
            return Ok(());
        }
        depth.written = counts;
        let dir = self.out.join(&depth.layer);
        fs::create_dir_all(&dir)?;
        let text = json!({ "at": now, "done": counts.0, "total": counts.1 }).to_string();
        write_atomic(&dir.join("status.json"), text.as_bytes())
    }

    pub fn flush(&mut self) -> Result<(), String> {
        let pending: Vec<(Key, Batch)> = self.pending.drain().collect();
        for (key, chunks) in pending {
            let tile = self.tile_path(&key);
            let mut region = match self.regions.iter().position(|(one, _)| *one == key) {
                Some(at) => self.regions.remove(at).1,
                None => read_tile(&tile),
            };
            for (cx, cz, columns) in &chunks {
                region.patch(*cx, *cz, columns);
            }
            let written = write_tile(&tile, &region);
            self.regions.push((key, region));
            if self.regions.len() > KEEP_REGIONS {
                self.regions.remove(0);
            }
            written.map_err(|error| format!("Couldn't write the map tile {}: {error}", tile.display()))?;
        }
        if self.colors_changed {
            self.colors_changed = false;
            self.write_palette()
                .map_err(|error| format!("Couldn't write the map palette: {error}"))?;
        }
        Ok(())
    }

    fn tile_path(&self, (layer, rx, rz): &Key) -> PathBuf {
        self.out.join(layer).join(format!("r.{rx}.{rz}.mnm"))
    }

    fn fill(&mut self, server: &Server) {
        let mut players = Vec::new();
        self.worlds.clear();
        let wanted = self.wanted_layer();
        if self.depth.as_ref().map(|depth| &depth.layer) != wanted.as_ref() {
            self.depth = wanted.clone().map(|layer| Depth { layer, queued: HashSet::new(), done: HashSet::new(), written: (0, 0) });
        }
        for player in server.get_all_players() {
            let world = player.get_world();
            let Some(layer) = layer_of(&world.get_dimension()) else {
                continue;
            };
            let (x, _, z) = player.get_position();
            let (cx, cz) = ((x.floor() as i32).div_euclid(16), (z.floor() as i32).div_euclid(16));
            if let Some(depth) = wanted.as_ref().filter(|depth| split_depth(depth).is_some_and(|(realm, _)| realm == layer)) {
                players.push((depth.clone(), cx, cz));
            }
            players.push((layer.clone(), cx, cz));
            self.worlds.entry(layer).or_insert_with(|| {
                let min_y = world.get_min_y();
                (world, min_y)
            });
        }
        let mut added = HashSet::new();
        for ring in 0..=RADIUS {
            for (layer, pcx, pcz) in &players {
                for dx in -ring..=ring {
                    for dz in -ring..=ring {
                        if dx.abs().max(dz.abs()) != ring {
                            continue;
                        }
                        let key = (layer.clone(), pcx + dx, pcz + dz);
                        if added.insert(key.clone()) {
                            if let Some(depth) = self.depth.as_mut().filter(|depth| depth.layer == key.0) {
                                depth.queued.insert((key.1, key.2));
                            }
                            self.queue.push_back(key);
                        }
                    }
                }
            }
        }
    }

    fn sample(&mut self, layer: &str, cx: i32, cz: i32) -> Option<Columns> {
        let (realm, cutoff) = match split_depth(layer) {
            Some((realm, y)) => (realm, y),
            None if layer == "nether" => (layer, NETHER_CEILING),
            None => (layer, i32::MAX),
        };
        // Only the loaded check goes through the chunk resource: its other calls trap the whole plugin
        // when the chunk unloads mid-sample, while the world calls read loaded chunks and fall back to air.
        let (world, min_y) = self.worlds.remove(realm)?;
        let loaded = world.get_chunk(cx, cz).is_some();
        let out = loaded.then(|| self.columns(&world, cx, cz, min_y, cutoff));
        self.worlds.insert(realm.to_string(), (world, min_y));
        out
    }

    fn look(&mut self, world: &World, pos: BlockPos) -> &Look {
        let id = world.get_block_state_id(pos);
        if !self.looks.contains_key(&id) {
            let state = world.get_block_state(pos);
            let waterlogged = state
                .properties
                .iter()
                .any(|(key, value)| key == "waterlogged" && value == "true");
            let name = if state.block_name.contains(':') { state.block_name } else { format!("minecraft:{}", state.block_name) };
            let look = Look {
                visible: !state.is_air && state.map_color != 0,
                water: waterlogged || ALWAYS_WET.contains(&name.as_str()),
                color: state.map_color,
                name,
            };
            if look.visible && !self.colors.contains_key(&look.name) {
                self.colors.insert(look.name.clone(), look.color);
                self.colors_changed = true;
            }
            self.looks.insert(id, look);
        }
        &self.looks[&id]
    }

    fn columns(&mut self, world: &World, cx: i32, cz: i32, min_y: i32, cutoff: i32) -> Columns {
        let ceiling = cutoff != i32::MAX;
        let mut out = Columns {
            top: vec![None; 256],
            floor: vec![None; 256],
            biome: vec![String::new(); 256],
            height: [0; 256],
            depth: [0; 256],
            light: [0; 256],
            empty: [false; 256],
            hash: 1_125_899_906_842_597,
        };
        for z in 0..16 {
            for x in 0..16 {
                let at = (z * 16 + x) as usize;
                let (bx, bz) = (cx * 16 + x, cz * 16 + z);
                let start = world.get_top_block_y(bx, bz);
                if start < min_y {
                    out.empty[at] = !ceiling;
                    out.hash = mix(out.hash, if ceiling { 1 } else { 2 });
                    continue;
                }
                let mut open = !ceiling;
                let mut y = start;
                if ceiling {
                    if start < cutoff - 1 {
                        open = true;
                    } else {
                        y = cutoff - 1;
                    }
                }
                let mut top: Option<String> = None;
                let mut floor: Option<String> = None;
                let mut top_y = i32::MIN;
                let mut water = 0u32;
                while y >= min_y {
                    let look = self.look(world, BlockPos { x: bx, y, z: bz });
                    let (visible, fluid) = (look.visible, look.water);
                    if !open {
                        if !visible && !fluid {
                            open = true;
                        }
                        y -= 1;
                        continue;
                    }
                    if top_y == i32::MIN {
                        if fluid {
                            top_y = y;
                            top = Some("minecraft:water".into());
                            water = 1;
                        } else if visible {
                            top_y = y;
                            top = Some(look.name.clone());
                            break;
                        }
                    } else if fluid {
                        water += 1;
                    } else if visible {
                        floor = Some(look.name.clone());
                        break;
                    }
                    y -= 1;
                }
                let Some(top) = top else {
                    out.empty[at] = !ceiling;
                    out.hash = mix(out.hash, if ceiling { 3 } else { 4 });
                    continue;
                };
                let biome = biome_name(&format!("{:?}", world.get_biome(BlockPos { x: bx, y: top_y, z: bz })));
                let light = world.get_block_light(BlockPos { x: bx, y: top_y + 1, z: bz });
                out.hash = mix(out.hash, text_hash(&top));
                out.hash = mix(out.hash, floor.as_deref().map_or(0, text_hash));
                out.hash = mix(out.hash, text_hash(&biome));
                out.hash = mix(out.hash, top_y as u64);
                out.hash = mix(out.hash, u64::from(water.min(254)));
                out.hash = mix(out.hash, u64::from(light));
                out.top[at] = Some(top);
                out.floor[at] = floor;
                out.biome[at] = biome;
                out.height[at] = top_y as i16;
                out.depth[at] = water.min(254) as u8;
                out.light[at] = light;
            }
        }
        out
    }

    /// The depth layer the app is showing, while it keeps rewriting `requests.json`.
    fn wanted_layer(&self) -> Option<String> {
        let path = self.out.join("requests.json");
        let age = fs::metadata(&path).ok()?.modified().ok()?.elapsed().map_or(0, |age| age.as_secs());
        if age > WANTED_SECONDS {
            return None;
        }
        let value: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
        let layer = value.get("layers")?.get(0)?.get("layer")?.as_str()?;
        split_depth(layer).is_some().then(|| layer.to_string())
    }

    fn write_palette(&self) -> std::io::Result<()> {
        let path = self.out.join("palette.json");
        if fs::read_to_string(&path).is_ok_and(|text| text.contains("\"vanilla\":true")) {
            return Ok(());
        }
        let mut blocks = JsonMap::new();
        for (name, color) in &self.colors {
            let rgb = MAP_COLORS.get(*color as usize).copied().unwrap_or(0x808080);
            blocks.insert(name.clone(), json!([format!("{rgb:06x}"), "none"]));
        }
        let text = json!({ "key": "pumpkin", "vanilla": false, "blocks": Value::Object(blocks), "biomes": {} });
        fs::create_dir_all(&self.out)?;
        write_atomic(&path, text.to_string().as_bytes())
    }
}

impl Region {
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
            block_ids: HashMap::new(),
            biomes: Vec::new(),
            biome_ids: HashMap::new(),
            block: vec![NONE; SPAN * SPAN],
            floor: vec![NONE; SPAN * SPAN],
            biome: vec![0; SPAN * SPAN],
            height: vec![0; SPAN * SPAN],
            depth: vec![0; SPAN * SPAN],
            light: vec![0; SPAN * SPAN],
        }
    }

    fn block_id(&mut self, name: &str) -> i32 {
        if let Some(id) = self.block_ids.get(name) {
            return *id;
        }
        let id = self.blocks.len() as i32;
        self.blocks.push(name.to_string());
        self.block_ids.insert(name.to_string(), id);
        id
    }

    fn biome_id(&mut self, name: &str) -> u16 {
        if let Some(id) = self.biome_ids.get(name) {
            return *id;
        }
        let id = self.biomes.len() as u16;
        self.biomes.push(name.to_string());
        self.biome_ids.insert(name.to_string(), id);
        id
    }

    fn patch(&mut self, cx: i32, cz: i32, columns: &Columns) {
        let base_x = cx.rem_euclid(32) as usize * 16;
        let base_z = cz.rem_euclid(32) as usize * 16;
        for z in 0..16 {
            for x in 0..16 {
                let from = z * 16 + x;
                let at = (base_z + z) * SPAN + base_x + x;
                let Some(top) = &columns.top[from] else {
                    if columns.empty[from] {
                        self.block[at] = VOID;
                        self.floor[at] = NONE;
                    }
                    continue;
                };
                self.block[at] = self.block_id(top);
                self.floor[at] = columns.floor[from].as_deref().map_or(NONE, |name| self.block_id(name));
                self.biome[at] = self.biome_id(&columns.biome[from]);
                self.height[at] = columns.height[from];
                self.depth[at] = columns.depth[from];
                self.light[at] = columns.light[from];
            }
        }
    }
}

fn read_names(raw: &[u8], at: &mut usize) -> Option<Vec<String>> {
    let count = u16::from_be_bytes([*raw.get(*at)?, *raw.get(*at + 1)?]) as usize;
    *at += 2;
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        let length = u16::from_be_bytes([*raw.get(*at)?, *raw.get(*at + 1)?]) as usize;
        *at += 2;
        names.push(String::from_utf8_lossy(raw.get(*at..*at + length)?).into_owned());
        *at += length;
    }
    Some(names)
}

fn read_tile(path: &Path) -> Region {
    let mut region = Region::new();
    let Ok(bytes) = fs::read(path) else {
        return region;
    };
    let mut raw = Vec::new();
    if flate2::read::GzDecoder::new(bytes.as_slice()).read_to_end(&mut raw).is_err() || raw.get(..4) != Some(b"MNM2") {
        return region;
    }
    let mut at = 4;
    let (Some(blocks), Some(biomes)) = (read_names(&raw, &mut at), read_names(&raw, &mut at)) else {
        return region;
    };
    let Some(cells) = raw.get(at..at + SPAN * SPAN * 10) else {
        return region;
    };
    let block_map: Vec<i32> = blocks.iter().map(|name| region.block_id(name)).collect();
    let biome_map: Vec<u16> = biomes.iter().map(|name| region.biome_id(name)).collect();
    for (index, cell) in cells.as_chunks::<10>().0.iter().enumerate() {
        let b = u16::from_be_bytes([cell[0], cell[1]]);
        let f = u16::from_be_bytes([cell[2], cell[3]]);
        let bio = u16::from_be_bytes([cell[4], cell[5]]) as usize;
        region.block[index] = match b {
            0xFFFE => VOID,
            0xFFFF => NONE,
            _ => block_map.get(b as usize).copied().unwrap_or(NONE),
        };
        region.floor[index] = block_map.get(f as usize).copied().unwrap_or(NONE);
        region.biome[index] = biome_map.get(bio).copied().unwrap_or(0);
        region.height[index] = i16::from_be_bytes([cell[6], cell[7]]);
        region.depth[index] = cell[8];
        region.light[index] = cell[9];
    }
    region
}

fn write_names(out: &mut Vec<u8>, names: &[String]) {
    out.extend_from_slice(&(names.len() as u16).to_be_bytes());
    for name in names {
        out.extend_from_slice(&(name.len() as u16).to_be_bytes());
        out.extend_from_slice(name.as_bytes());
    }
}

fn write_tile(path: &Path, region: &Region) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity(SPAN * SPAN * 10 + 4096);
    raw.extend_from_slice(b"MNM2");
    write_names(&mut raw, &region.blocks);
    write_names(&mut raw, &region.biomes);
    for at in 0..SPAN * SPAN {
        let b: u16 = match region.block[at] {
            VOID => 0xFFFE,
            id if id < 0 => 0xFFFF,
            id => id as u16,
        };
        let f: u16 = if region.floor[at] < 0 { 0xFFFF } else { region.floor[at] as u16 };
        raw.extend_from_slice(&b.to_be_bytes());
        raw.extend_from_slice(&f.to_be_bytes());
        raw.extend_from_slice(&region.biome[at].to_be_bytes());
        raw.extend_from_slice(&region.height[at].to_be_bytes());
        raw.push(region.depth[at]);
        raw.push(region.light[at]);
    }
    let mut packed = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    packed.write_all(&raw)?;
    let bytes = packed.finish()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_atomic(path, &bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}

fn mix(hash: u64, value: u64) -> u64 {
    hash.wrapping_mul(31).wrapping_add(value)
}

fn text_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3))
}

/// `BambooJungle` → `minecraft:bamboo_jungle`; the same for entity types.
pub fn biome_name(debug: &str) -> String {
    let short = debug.rsplit("::").next().unwrap_or(debug);
    let mut name = String::from("minecraft:");
    for (index, letter) in short.chars().enumerate() {
        if letter.is_ascii_uppercase() {
            if index > 0 {
                name.push('_');
            }
            name.push(letter.to_ascii_lowercase());
        } else {
            name.push(letter);
        }
    }
    name
}

/// `overworld-y32` → (`overworld`, 32).
fn split_depth(layer: &str) -> Option<(&str, i32)> {
    let (realm, y) = layer.rsplit_once("-y")?;
    let y: i32 = y.parse().ok()?;
    (!realm.is_empty() && (-4096..=4096).contains(&y)).then_some((realm, y))
}

fn layer_of(dimension: &str) -> Option<String> {
    if dimension.is_empty() {
        return None;
    }
    let full = if dimension.contains(':') { dimension.to_string() } else { format!("minecraft:{dimension}") };
    Some(match full.as_str() {
        "minecraft:overworld" => "overworld".into(),
        "minecraft:the_nether" => "nether".into(),
        "minecraft:the_end" => "end".into(),
        _ => {
            let key: String = full
                .to_lowercase()
                .chars()
                .map(|letter| match letter {
                    ':' | '/' => '.',
                    'a'..='z' | '0'..='9' | '_' | '.' | '-' => letter,
                    _ => '_',
                })
                .collect();
            if key.len() > 96 {
                return None;
            }
            key
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_biomes_and_layers() {
        assert_eq!(biome_name("BambooJungle"), "minecraft:bamboo_jungle");
        assert_eq!(biome_name("Biome::Plains"), "minecraft:plains");
        assert_eq!(layer_of("minecraft:the_nether").as_deref(), Some("nether"));
        assert_eq!(layer_of("overworld").as_deref(), Some("overworld"));
        assert_eq!(layer_of("Mod:Deep/Place").as_deref(), Some("mod.deep.place"));
        assert_eq!(split_depth("overworld-y-20"), Some(("overworld", -20)));
        assert_eq!(split_depth("mod.deep-place-y32"), Some(("mod.deep-place", 32)));
        assert_eq!(split_depth("overworld"), None);
    }

    #[test]
    fn tiles_round_trip() {
        let dir = std::env::temp_dir().join(format!("monolab-map-{}", std::process::id()));
        let path = dir.join("overworld").join("r.0.0.mnm");
        let mut region = Region::new();
        let mut columns = Columns {
            top: vec![None; 256],
            floor: vec![None; 256],
            biome: vec![String::new(); 256],
            height: [0; 256],
            depth: [0; 256],
            light: [0; 256],
            empty: [false; 256],
            hash: 0,
        };
        columns.top[17] = Some("minecraft:water".into());
        columns.floor[17] = Some("minecraft:sand".into());
        columns.biome[17] = "minecraft:ocean".into();
        columns.height[17] = -3;
        columns.depth[17] = 5;
        columns.empty[18] = true;
        region.patch(-31, 1, &columns);
        write_tile(&path, &region).unwrap();
        let back = read_tile(&path);
        let at = (16 + 1) * SPAN + 16 + 1;
        assert_eq!(back.blocks[back.block[at] as usize], "minecraft:water");
        assert_eq!(back.blocks[back.floor[at] as usize], "minecraft:sand");
        assert_eq!(back.height[at], -3);
        assert_eq!(back.depth[at], 5);
        assert_eq!(back.block[at + 1], VOID);
        assert_eq!(back.block[0], NONE);
        let _ = fs::remove_dir_all(dir);
    }
}

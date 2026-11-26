//! Studio world generator: a world file with `"type": "custom"` and a `"generator"` block. Pure math, no blocks
//! are placed here: the app draws its preview from it, the Pumpkin add-on runs it in its chunk generator, and the
//! Paper add-on has the same steps in Java (`StudioWorldGen.java`) — keep the two in step, number for number
//! (same hash, same noise, same order), so the preview shows what the server makes.
//!
//! ```jsonc
//! "generator": {
//!   "warp": { "type": "spiral", "strength": 1, "x": 0, "z": 0 },   // none | spiral | domain (+ "scale", "amount")
//!   "sea": 63, "base": 68, "height": 40, "overhangs": 0, "climate": 512,
//!   "layers": [{ "noise": "perlin", "scale": 512, "octaves": 5, "amount": 1 }],   // perlin | ridged | billow | cellular
//!   "stone": "stone", "fluid": "water", "bedrock": true,
//!   "biomes": [{ "biome": "desert", "temperature": [0.6, 1], "humidity": [0, 0.4], "height": [64, 400] }],
//!   "surface": [{ "biome": "desert", "top": "sand", "under": "sandstone", "depth": 4, "underwater": "sand" }],
//!   "caves": [{ "type": "spaghetti", "scale": 64, "width": 0.07 }, { "type": "cheese", "scale": 96, "threshold": 0.55 }],
//!   "features": [{ "type": "ore", "block": "iron_ore", "y": [-48, 64], "per_chunk": 8, "size": 6 },
//!                { "type": "tree", "trunk": "oak_log", "leaves": "oak_leaves", "per_chunk": 2, "height": [4, 6], "biomes": ["forest"] },
//!                { "type": "boulder", "block": "mossy_cobblestone", "per_chunk": 0.1, "size": 2 },
//!                { "type": "patch", "block": "short_grass", "per_chunk": 12 },
//!                { "type": "structure", "structure": "ruin", "per_chunk": 0.05, "rotation": "random", "lift": 0 }],
//!   "keep": { "structures": false, "mobs": true, "carvers": false, "decorations": false }
//! }
//! ```
//!
//! More (2026-11-23):
//! - `"shape"`: a tree of nodes that makes the ground instead of `layers` (`{"type": "add", "inputs": [...]}`, see
//!   `SHAPES`). Its value is used like the layers' sum: height = base + height × value.
//! - Biome rules may lift (`offset`, blocks) and stretch (`stretch`, × hills) the ground where they hold; across
//!   `"blend"` (climate units, 0.05 by default) the next rule's ground fades in, so edges don't step.
//! - Biomes may be our own (`studio:<id>`, the `biome` kind).
//! - `"mode": "vanilla"`: the server's own ground (`"terrain"`: normal, amplified, large_biomes). Paper puts the biome
//!   rules on it (by warmth and wetness; where none holds the server's own biome stays); Pumpkin twists it with a
//!   spiral `warp` (fork `noise-warped`). Layers, surface, caves and features are not used then.

use crate::structure::{self, Structure};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::Arc;

type Data = Map<String, Value>;

pub const NOISES: [&str; 4] = ["perlin", "ridged", "billow", "cellular"];
pub const WARPS: [&str; 3] = ["none", "spiral", "domain"];
pub const CAVES: [&str; 2] = ["spaghetti", "cheese"];
pub const FEATURES: [&str; 5] = ["ore", "tree", "boulder", "patch", "structure"];
/// Shape nodes. `noise` (noise, scale, octaves, amount), `constant` (value), `add`/`multiply`/`min`/`max` (inputs),
/// `clamp` (input, min, max), `abs` (input), `power` (input, exponent; keeps the sign), `terrace` (input, steps),
/// `remap` (input, from, to), `climate` (which: warmth or wetness, -1..1), `distance` (x, z, radius: 1 in the middle,
/// 0 at the radius, lower beyond).
pub const SHAPES: [&str; 13] = ["noise", "constant", "add", "multiply", "min", "max", "clamp", "abs", "power", "terrace", "remap", "climate", "distance"];
pub const MODES: [&str; 2] = ["custom", "vanilla"];
pub const TERRAINS: [&str; 3] = ["normal", "amplified", "large_biomes"];
/// How deep a shape may nest, and how many nodes it may have.
pub const MOST_SHAPE_DEPTH: usize = 12;
pub const MOST_SHAPE_NODES: usize = 64;

/// The seed a custom world generates from: a number as it is, other text the way Java hashes a string, and with
/// no seed the world's name (so the preview and the server agree without a seed).
pub fn seed_of(seed: &str, world: &str) -> i64 {
    let seed = seed.trim();
    if seed.is_empty() {
        return java_hash(world) as i64;
    }
    seed.parse::<i64>().unwrap_or_else(|_| java_hash(seed) as i64)
}

/// Java's `String.hashCode`.
pub fn java_hash(text: &str) -> i32 {
    text.encode_utf16().fold(0i32, |hash, unit| hash.wrapping_mul(31).wrapping_add(unit as i32))
}

// ---------- hash, random, noise (the Java copy must match these exactly) ----------

fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

fn hash(seed: i64, x: i32, y: i32, z: i32) -> u64 {
    mix((seed as u64)
        ^ (x as i64 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (y as i64 as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
        ^ (z as i64 as u64).wrapping_mul(0x1656_67b1_9e37_79f9))
}

/// A small random sequence (splitmix64).
pub struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        mix(self.0)
    }
    /// 0 up to (not including) 1.
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// `from..=to`.
    fn between(&mut self, from: i32, to: i32) -> i32 {
        if to <= from {
            return from;
        }
        from + (self.unit() * f64::from(to - from + 1)).floor() as i32
    }
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

fn grad2(h: u64, x: f64, y: f64) -> f64 {
    match h & 7 {
        0 => x + y,
        1 => -x + y,
        2 => x - y,
        3 => -x - y,
        4 => x,
        5 => -x,
        6 => y,
        _ => -y,
    }
}

fn grad3(h: u64, x: f64, y: f64, z: f64) -> f64 {
    let h = h & 15;
    let u = if h < 8 { x } else { y };
    let v = if h < 4 {
        y
    } else if h == 12 || h == 14 {
        x
    } else {
        z
    };
    (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
}

/// Gradient noise, about -1..1.
fn perlin2(seed: i64, x: f64, z: f64) -> f64 {
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    let (ix, iz) = (x0 as i32, z0 as i32);
    let (u, v) = (fade(fx), fade(fz));
    let a = lerp(u, grad2(hash(seed, ix, 0, iz), fx, fz), grad2(hash(seed, ix.wrapping_add(1), 0, iz), fx - 1.0, fz));
    let b = lerp(u, grad2(hash(seed, ix, 0, iz.wrapping_add(1)), fx, fz - 1.0), grad2(hash(seed, ix.wrapping_add(1), 0, iz.wrapping_add(1)), fx - 1.0, fz - 1.0));
    (lerp(v, a, b) * 0.9).clamp(-1.0, 1.0)
}

fn perlin3(seed: i64, x: f64, y: f64, z: f64) -> f64 {
    let (x0, y0, z0) = (x.floor(), y.floor(), z.floor());
    let (fx, fy, fz) = (x - x0, y - y0, z - z0);
    let (ix, iy, iz) = (x0 as i32, y0 as i32, z0 as i32);
    let (u, v, w) = (fade(fx), fade(fy), fade(fz));
    let g = |dx: i32, dy: i32, dz: i32| grad3(hash(seed, ix.wrapping_add(dx), iy.wrapping_add(dy), iz.wrapping_add(dz)), fx - f64::from(dx), fy - f64::from(dy), fz - f64::from(dz));
    let x00 = lerp(u, g(0, 0, 0), g(1, 0, 0));
    let x10 = lerp(u, g(0, 1, 0), g(1, 1, 0));
    let x01 = lerp(u, g(0, 0, 1), g(1, 0, 1));
    let x11 = lerp(u, g(0, 1, 1), g(1, 1, 1));
    (lerp(w, lerp(v, x00, x10), lerp(v, x01, x11)) * 0.9).clamp(-1.0, 1.0)
}

/// Distance to the nearest random point, as 1 (on a point) down to -1.
fn cellular2(seed: i64, x: f64, z: f64) -> f64 {
    let (cx, cz) = (x.floor() as i32, z.floor() as i32);
    let mut nearest = 2.0f64;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let (px, pz) = (cx.wrapping_add(dx), cz.wrapping_add(dz));
            let mut rng = Rng::new(hash(seed, px, 7, pz));
            let fx = f64::from(px) + rng.unit() - x;
            let fz = f64::from(pz) + rng.unit() - z;
            nearest = nearest.min((fx * fx + fz * fz).sqrt());
        }
    }
    1.0 - 2.0 * nearest.min(1.0)
}

fn one_noise(kind: Noise, seed: i64, x: f64, z: f64) -> f64 {
    match kind {
        Noise::Perlin => perlin2(seed, x, z),
        Noise::Ridged => {
            let r = 1.0 - perlin2(seed, x, z).abs();
            r * r * 2.0 - 1.0
        }
        Noise::Billow => perlin2(seed, x, z).abs() * 2.0 - 1.0,
        Noise::Cellular => cellular2(seed, x, z),
    }
}

/// Octaves of one noise, each twice as fine and half as strong, kept about -1..1.
fn fbm(kind: Noise, seed: i64, x: f64, z: f64, octaves: u32) -> f64 {
    let (mut sum, mut total, mut amp, mut freq) = (0.0, 0.0, 1.0, 1.0);
    for octave in 0..octaves.max(1) {
        sum += amp * one_noise(kind, seed.wrapping_add(i64::from(octave) * 131), x * freq, z * freq);
        total += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / total
}

// ---------- the generator ----------

#[derive(Clone, Copy, Debug, PartialEq)]
enum Noise {
    Perlin,
    Ridged,
    Billow,
    Cellular,
}

#[derive(Clone, Debug)]
struct Layer {
    noise: Noise,
    scale: f64,
    octaves: u32,
    amount: f64,
}

#[derive(Clone, Debug)]
enum Warp {
    None,
    Spiral { strength: f64, x: f64, z: f64 },
    Domain { scale: f64, amount: f64 },
}

#[derive(Clone, Debug)]
struct BiomeRule {
    biome: String,
    temperature: (f64, f64),
    humidity: (f64, f64),
    height: (f64, f64),
    /// Blocks the ground is lifted by where this rule holds.
    offset: f64,
    /// How much of the hills it keeps there (1 = all).
    stretch: f64,
}

/// One node of a `shape` tree. Noise nodes carry their own seed: the world's plus their place in the tree.
#[derive(Clone, Debug)]
enum Shape {
    Noise { noise: Noise, scale: f64, octaves: u32, amount: f64, seed: i64 },
    Constant(f64),
    Add(Vec<Shape>),
    Multiply(Vec<Shape>),
    Min(Vec<Shape>),
    Max(Vec<Shape>),
    Clamp(Box<Shape>, f64, f64),
    Abs(Box<Shape>),
    Power(Box<Shape>, f64),
    Terrace(Box<Shape>, f64),
    Remap(Box<Shape>, (f64, f64), (f64, f64)),
    Climate { wet: bool },
    Distance { x: f64, z: f64, radius: f64 },
}

impl Shape {
    /// Reads a node and what feeds it, pre-order (noise seeds count in that order). Too deep or too many nodes,
    /// and the rest reads as 0.
    fn read(data: &Data, seed: i64, depth: usize, count: &mut usize) -> Shape {
        *count += 1;
        if depth > MOST_SHAPE_DEPTH || *count > MOST_SHAPE_NODES {
            return Shape::Constant(0.0);
        }
        let input = |count: &mut usize| match data.get("input").and_then(Value::as_object) {
            Some(one) => Box::new(Shape::read(one, seed, depth + 1, count)),
            None => Box::new(Shape::Constant(0.0)),
        };
        let inputs = |count: &mut usize| objects(data, "inputs").map(|one| Shape::read(one, seed, depth + 1, count)).collect::<Vec<_>>();
        match text(data, "type", "constant").as_str() {
            "noise" => Shape::Noise {
                noise: noise_of(&text(data, "noise", "perlin")),
                scale: num(data, "scale", 256.0).max(1.0),
                octaves: num(data, "octaves", 4.0).clamp(1.0, 8.0) as u32,
                amount: num(data, "amount", 1.0),
                seed: seed.wrapping_add(20_000 + 1000 * *count as i64),
            },
            "add" => Shape::Add(inputs(count)),
            "multiply" => Shape::Multiply(inputs(count)),
            "min" => Shape::Min(inputs(count)),
            "max" => Shape::Max(inputs(count)),
            "clamp" => {
                let (least, most) = (num(data, "min", -1.0), num(data, "max", 1.0));
                Shape::Clamp(input(count), least.min(most), least.max(most))
            }
            "abs" => Shape::Abs(input(count)),
            "power" => Shape::Power(input(count), num(data, "exponent", 2.0).clamp(0.1, 8.0)),
            "terrace" => Shape::Terrace(input(count), num(data, "steps", 4.0).clamp(1.0, 64.0).round()),
            "remap" => Shape::Remap(input(count), pair(data, "from", (-1.0, 1.0)), pair(data, "to", (0.0, 1.0))),
            "climate" => Shape::Climate { wet: text(data, "which", "warmth") == "wetness" },
            "distance" => Shape::Distance { x: num(data, "x", 0.0), z: num(data, "z", 0.0), radius: num(data, "radius", 512.0).max(1.0) },
            _ => Shape::Constant(num(data, "value", 0.0)),
        }
    }

    /// The value at a warped spot with its climate (0..1 each).
    fn value(&self, x: f64, z: f64, temperature: f64, humidity: f64) -> f64 {
        let all = |list: &[Shape]| list.iter().map(|one| one.value(x, z, temperature, humidity)).collect::<Vec<f64>>();
        match self {
            Shape::Noise { noise, scale, octaves, amount, seed } => amount * fbm(*noise, *seed, x / scale, z / scale, *octaves),
            Shape::Constant(value) => *value,
            Shape::Add(list) => all(list).into_iter().sum(),
            Shape::Multiply(list) => all(list).into_iter().product(),
            Shape::Min(list) => all(list).into_iter().reduce(f64::min).unwrap_or(0.0),
            Shape::Max(list) => all(list).into_iter().reduce(f64::max).unwrap_or(0.0),
            Shape::Clamp(one, least, most) => one.value(x, z, temperature, humidity).clamp(*least, *most),
            Shape::Abs(one) => one.value(x, z, temperature, humidity).abs(),
            Shape::Power(one, exponent) => {
                let value = one.value(x, z, temperature, humidity);
                value.signum() * value.abs().powf(*exponent)
            }
            Shape::Terrace(one, steps) => {
                let scaled = one.value(x, z, temperature, humidity) * steps;
                let floor = scaled.floor();
                (floor + fade(scaled - floor)) / steps
            }
            Shape::Remap(one, (a, b), (c, d)) => {
                let value = one.value(x, z, temperature, humidity);
                if (b - a).abs() < 1e-12 { *c } else { c + (value - a) / (b - a) * (d - c) }
            }
            Shape::Climate { wet } => (if *wet { humidity } else { temperature }) * 2.0 - 1.0,
            Shape::Distance { x: cx, z: cz, radius } => {
                let (dx, dz) = (x - cx, z - cz);
                1.0 - (dx * dx + dz * dz).sqrt() / radius
            }
        }
    }
}

/// How much a climate value is inside a rule's range, 0..1, with `blend` wide soft edges. An edge at the end of the
/// scale (0 or 1) is always inside.
fn inside(value: f64, (from, to): (f64, f64), blend: f64) -> f64 {
    if blend <= 0.0 {
        return if value >= from && value <= to { 1.0 } else { 0.0 };
    }
    let low = if from <= 0.0 { 1.0 } else { ((value - from) / blend + 0.5).clamp(0.0, 1.0) };
    let high = if to >= 1.0 { 1.0 } else { ((to - value) / blend + 0.5).clamp(0.0, 1.0) };
    low.min(high)
}

#[derive(Clone, Debug)]
pub struct Surface {
    biome: String,
    pub top: String,
    pub under: String,
    pub depth: i32,
    pub underwater: String,
}

#[derive(Clone, Debug)]
enum Cave {
    Spaghetti { scale: f64, width: f64 },
    Cheese { scale: f64, threshold: f64 },
}

#[derive(Clone, Debug)]
enum FeatureKind {
    Ore { block: String, y: (i32, i32), size: i32 },
    Tree { trunk: String, leaves: String, height: (i32, i32) },
    Boulder { block: String, size: i32 },
    Patch { block: String },
    /// A studio structure (`structures/<id>.json`, given with `add_structure`), its middle on the ground.
    Structure { id: String, rotation: String, lift: i32 },
}

#[derive(Clone, Debug)]
struct Feature {
    kind: FeatureKind,
    per_chunk: f64,
    biomes: Vec<String>,
}

/// What the generator keeps from the server's own world generation (Paper only for now).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Keep {
    pub structures: bool,
    pub mobs: bool,
    pub carvers: bool,
    pub decorations: bool,
}

#[derive(Clone, Debug)]
pub struct Generator {
    seed: i64,
    warp: Warp,
    pub sea: i32,
    base: f64,
    height: f64,
    overhangs: f64,
    climate: f64,
    layers: Vec<Layer>,
    pub stone: String,
    pub fluid: String,
    pub bedrock: bool,
    biomes: Vec<BiomeRule>,
    surface: Vec<Surface>,
    fallback_surface: Surface,
    caves: Vec<Cave>,
    features: Vec<Feature>,
    pub keep: Keep,
    /// Structures the features use, each turned 0–3 quarters: (width, depth, blocks from the low corner).
    structures: HashMap<String, Arc<[Turned; 4]>>,
    /// The node tree that makes the ground instead of `layers`.
    shape: Option<Shape>,
    /// How wide (in climate units) biome rules' ground fades into the next.
    blend: f64,
    /// Whether any biome rule lifts or stretches the ground (else the plain sum is used, as before).
    shaped: bool,
    /// `"mode": "vanilla"`: the server's own ground of this preset.
    vanilla: Option<String>,
}

type Turned = (i32, i32, Vec<(i32, i32, i32, String)>);

/// One column's 2D facts: its terrain height (before caves and overhangs), climate and biome.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub height: i32,
    pub temperature: f64,
    pub humidity: f64,
    /// Index into the biome rules; `usize::MAX` for plains.
    biome: usize,
}

/// A block a feature puts down, and what may be there already.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: String,
    pub only: Only,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Only {
    /// Anything there is replaced.
    Any,
    /// Only into air (leaves, plants).
    Air,
    /// Only where the generator's stone is (ores).
    Stone,
}

/// What a stretch of one column is, bottom `from` to top `to`, both inclusive.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub from: i32,
    pub to: i32,
    pub block: String,
}

fn num(data: &Data, key: &str, fallback: f64) -> f64 {
    data.get(key).and_then(Value::as_f64).filter(|one| one.is_finite()).unwrap_or(fallback)
}

fn text(data: &Data, key: &str, fallback: &str) -> String {
    match data.get(key).and_then(Value::as_str).map(str::trim) {
        Some(one) if !one.is_empty() => one.to_string(),
        _ => fallback.to_string(),
    }
}

fn range(data: &Data, key: &str, fallback: (f64, f64)) -> (f64, f64) {
    match data.get(key).and_then(Value::as_array).map(|list| list.iter().filter_map(Value::as_f64).collect::<Vec<_>>()) {
        Some(list) if list.len() == 2 => (list[0].min(list[1]), list[0].max(list[1])),
        _ => fallback,
    }
}

fn pair(data: &Data, key: &str, fallback: (f64, f64)) -> (f64, f64) {
    match data.get(key).and_then(Value::as_array).map(|list| list.iter().filter_map(Value::as_f64).collect::<Vec<_>>()) {
        Some(list) if list.len() == 2 => (list[0], list[1]),
        _ => fallback,
    }
}

fn objects<'a>(data: &'a Data, key: &str) -> impl Iterator<Item = &'a Data> {
    data.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object)
}

/// `minecraft:desert`, `desert` → `desert`; `studio:ruby` stays as it is.
pub fn short(name: &str) -> String {
    let name = name.trim().to_ascii_lowercase();
    name.strip_prefix("minecraft:").map(str::to_string).unwrap_or(name)
}

fn noise_of(name: &str) -> Noise {
    match name {
        "ridged" => Noise::Ridged,
        "billow" => Noise::Billow,
        "cellular" => Noise::Cellular,
        _ => Noise::Perlin,
    }
}

impl Generator {
    /// Reads a `"generator"` block. Unknown or missing values fall back quietly; `problems` names them.
    pub fn new(data: &Data, seed: i64) -> Self {
        let warp_data = data.get("warp").and_then(Value::as_object).cloned().unwrap_or_default();
        let warp = match text(&warp_data, "type", "none").as_str() {
            "spiral" => Warp::Spiral { strength: num(&warp_data, "strength", 1.0), x: num(&warp_data, "x", 0.0), z: num(&warp_data, "z", 0.0) },
            "domain" => Warp::Domain { scale: num(&warp_data, "scale", 256.0).max(1.0), amount: num(&warp_data, "amount", 64.0) },
            _ => Warp::None,
        };
        let mut layers: Vec<Layer> = objects(data, "layers")
            .map(|one| Layer {
                noise: noise_of(&text(one, "noise", "perlin")),
                scale: num(one, "scale", 256.0).max(1.0),
                octaves: num(one, "octaves", 4.0).clamp(1.0, 8.0) as u32,
                amount: num(one, "amount", 1.0),
            })
            .collect();
        if layers.is_empty() && data.get("layers").is_none() {
            layers.push(Layer { noise: Noise::Perlin, scale: 384.0, octaves: 5, amount: 1.0 });
        }
        let biomes = objects(data, "biomes")
            .map(|one| BiomeRule {
                biome: short(&text(one, "biome", "plains")),
                temperature: range(one, "temperature", (0.0, 1.0)),
                humidity: range(one, "humidity", (0.0, 1.0)),
                height: range(one, "height", (-4096.0, 4096.0)),
                offset: num(one, "offset", 0.0).clamp(-256.0, 256.0),
                stretch: num(one, "stretch", 1.0).clamp(0.0, 8.0),
            })
            .collect::<Vec<BiomeRule>>();
        let shaped = biomes.iter().any(|rule| rule.offset != 0.0 || rule.stretch != 1.0);
        let shape = data.get("shape").and_then(Value::as_object).map(|one| Shape::read(one, seed, 0, &mut 0));
        let vanilla = (text(data, "mode", "custom") == "vanilla").then(|| fields_choice(data, "terrain", &TERRAINS, "normal"));
        let surface_of = |one: &Data| Surface {
            biome: short(&text(one, "biome", "")),
            top: short(&text(one, "top", "grass_block")),
            under: short(&text(one, "under", "dirt")),
            depth: num(one, "depth", 3.0).clamp(0.0, 32.0) as i32,
            underwater: short(&text(one, "underwater", "gravel")),
        };
        let mut surface: Vec<Surface> = objects(data, "surface").map(surface_of).collect();
        let fallback_surface = match surface.iter().position(|one| one.biome.is_empty()) {
            Some(index) => surface.remove(index),
            None => surface_of(&Data::new()),
        };
        let caves = objects(data, "caves")
            .filter_map(|one| match text(one, "type", "").as_str() {
                "spaghetti" => Some(Cave::Spaghetti { scale: num(one, "scale", 64.0).max(4.0), width: num(one, "width", 0.07).clamp(0.0, 1.0) }),
                "cheese" => Some(Cave::Cheese { scale: num(one, "scale", 96.0).max(4.0), threshold: num(one, "threshold", 0.55).clamp(-1.0, 1.0) }),
                _ => None,
            })
            .collect();
        let features = objects(data, "features")
            .filter_map(|one| {
                let size = num(one, "size", 3.0).clamp(1.0, 16.0) as i32;
                let kind = match text(one, "type", "").as_str() {
                    "ore" => {
                        let (from, to) = range(one, "y", (-48.0, 64.0));
                        FeatureKind::Ore { block: short(&text(one, "block", "coal_ore")), y: (from as i32, to as i32), size }
                    }
                    "tree" => {
                        let (from, to) = range(one, "height", (4.0, 6.0));
                        FeatureKind::Tree { trunk: short(&text(one, "trunk", "oak_log")), leaves: short(&text(one, "leaves", "oak_leaves")), height: (from.max(1.0) as i32, to.clamp(1.0, 32.0) as i32) }
                    }
                    "boulder" => FeatureKind::Boulder { block: short(&text(one, "block", "mossy_cobblestone")), size: size.min(6) },
                    "patch" => FeatureKind::Patch { block: short(&text(one, "block", "short_grass")) },
                    "structure" => FeatureKind::Structure {
                        id: text(one, "structure", "").trim().to_string(),
                        rotation: fields_choice(one, "rotation", &structure::ROTATIONS, "random"),
                        lift: num(one, "lift", 0.0).clamp(-64.0, 64.0) as i32,
                    },
                    _ => return None,
                };
                let biomes = one.get("biomes").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(short).filter(|one| !one.is_empty()).collect();
                Some(Feature { kind, per_chunk: num(one, "per_chunk", 1.0).clamp(0.0, 64.0), biomes })
            })
            .collect();
        let keep_data = data.get("keep").and_then(Value::as_object).cloned().unwrap_or_default();
        let flag = |key: &str, fallback: bool| keep_data.get(key).and_then(Value::as_bool).unwrap_or(fallback);
        Self {
            seed,
            warp,
            sea: num(data, "sea", 63.0).clamp(-64.0, 320.0) as i32,
            base: num(data, "base", 68.0).clamp(-64.0, 320.0),
            height: num(data, "height", 40.0).clamp(0.0, 256.0),
            overhangs: num(data, "overhangs", 0.0).clamp(0.0, 2.0),
            climate: num(data, "climate", 512.0).max(16.0),
            layers,
            stone: short(&text(data, "stone", "stone")),
            fluid: short(&text(data, "fluid", "water")),
            bedrock: data.get("bedrock").and_then(Value::as_bool) != Some(false),
            biomes,
            surface,
            fallback_surface,
            caves,
            features,
            keep: Keep { structures: flag("structures", false), mobs: flag("mobs", true), carvers: flag("carvers", false), decorations: flag("decorations", false) },
            structures: HashMap::new(),
            shape,
            blend: num(data, "blend", 0.05).clamp(0.0, 0.5),
            shaped,
            vanilla,
        }
    }

    /// `"mode": "vanilla"`: the preset of the server's own ground (normal, amplified, large_biomes).
    pub fn vanilla(&self) -> Option<&str> {
        self.vanilla.as_deref()
    }

    /// A spiral warp's strength and middle, for the fork's `noise-warped` on vanilla ground.
    pub fn spiral(&self) -> Option<(f64, f64, f64)> {
        match self.warp {
            Warp::Spiral { strength, x, z } => Some((strength, x, z)),
            _ => None,
        }
    }

    /// The climate (warmth, wetness; 0..1) at a spot already warped.
    fn climate_at(&self, wx: f64, wz: f64) -> (f64, f64) {
        let temperature = (fbm(Noise::Perlin, self.seed.wrapping_add(11), wx / self.climate, wz / self.climate, 3) + 1.0) / 2.0;
        let humidity = (fbm(Noise::Perlin, self.seed.wrapping_add(12), wx / self.climate, wz / self.climate, 3) + 1.0) / 2.0;
        (temperature, humidity)
    }

    /// On vanilla ground: the biome the rules give a spot by warmth and wetness only (heights aren't known), or
    /// none where no rule holds and the server's own biome stays. Not warped: the fork's twist is its own.
    pub fn vanilla_biome(&self, x: i32, z: i32) -> Option<&str> {
        let (temperature, humidity) = self.climate_at(f64::from(x), f64::from(z));
        let within = |value: f64, (from, to): (f64, f64)| value >= from && value <= to;
        self.biomes.iter().find(|rule| within(temperature, rule.temperature) && within(humidity, rule.humidity)).map(|rule| rule.biome.as_str())
    }

    /// The lift and stretch the biome rules give a climate, each rule fading into the next across `blend`.
    fn lift(&self, temperature: f64, humidity: f64) -> (f64, f64) {
        let (mut offset, mut stretch, mut left) = (0.0, 0.0, 1.0);
        for rule in &self.biomes {
            let take = left * inside(temperature, rule.temperature, self.blend) * inside(humidity, rule.humidity, self.blend);
            offset += take * rule.offset;
            stretch += take * rule.stretch;
            left -= take;
        }
        (offset, stretch + left)
    }

    /// The structure ids the features use, for the add-ons to read and hand over with `add_structure`.
    pub fn structure_ids(&self) -> Vec<String> {
        let mut out: Vec<String> = self.features.iter().filter_map(|one| if let FeatureKind::Structure { id, .. } = &one.kind { Some(id.clone()) } else { None }).collect();
        out.sort();
        out.dedup();
        out
    }

    /// Gives a feature its structure. One wider or deeper than `structure::MOST_FEATURE_SIZE` is refused (false).
    pub fn add_structure(&mut self, id: &str, built: &Structure) -> bool {
        if built.size.0 > structure::MOST_FEATURE_SIZE || built.size.2 > structure::MOST_FEATURE_SIZE {
            return false;
        }
        let turn = |quarter: u8| {
            let (w, d) = built.footprint(quarter);
            (w, d, built.placements(quarter).into_iter().map(|(x, y, z, state)| (x, y, z, short(&state))).collect())
        };
        self.structures.insert(id.to_string(), Arc::new([turn(0), turn(1), turn(2), turn(3)]));
        true
    }

    /// Where a spot reads its noise, after the warp.
    fn warped(&self, x: f64, z: f64) -> (f64, f64) {
        match self.warp {
            Warp::None => (x, z),
            Warp::Spiral { strength, x: cx, z: cz } => {
                let (dx, dz) = (x - cx, z - cz);
                let r = (dx * dx + dz * dz).sqrt();
                // Twists fast near the middle and slower further out, so arms form around it.
                let theta = strength * 2.0 * (1.0 + r / 96.0).ln();
                let (sin, cos) = theta.sin_cos();
                (cx + dx * cos - dz * sin, cz + dx * sin + dz * cos)
            }
            Warp::Domain { scale, amount } => (
                x + amount * perlin2(self.seed.wrapping_add(77), x / scale, z / scale),
                z + amount * perlin2(self.seed.wrapping_add(78), x / scale, z / scale),
            ),
        }
    }

    pub fn column(&self, x: i32, z: i32) -> Column {
        let (wx, wz) = self.warped(f64::from(x), f64::from(z));
        let (temperature, humidity) = self.climate_at(wx, wz);
        let sum = match &self.shape {
            Some(shape) => shape.value(wx, wz, temperature, humidity),
            None => {
                let mut sum = 0.0;
                for (index, layer) in self.layers.iter().enumerate() {
                    sum += layer.amount * fbm(layer.noise, self.seed.wrapping_add(1000 * (index as i64 + 1)), wx / layer.scale, wz / layer.scale, layer.octaves);
                }
                sum
            }
        };
        let height = if self.shaped {
            let (offset, stretch) = self.lift(temperature, humidity);
            (self.base + offset + self.height * stretch * sum).round() as i32
        } else {
            (self.base + self.height * sum).round() as i32
        };
        let biome = self
            .biomes
            .iter()
            .position(|rule| {
                let within = |value: f64, (from, to): (f64, f64)| value >= from && value <= to;
                within(temperature, rule.temperature) && within(humidity, rule.humidity) && within(f64::from(height), rule.height)
            })
            .unwrap_or(usize::MAX);
        Column { height, temperature, humidity, biome }
    }

    pub fn biome(&self, column: &Column) -> &str {
        self.biomes.get(column.biome).map_or("plains", |rule| rule.biome.as_str())
    }

    /// Every biome this generator can give, for the add-ons' biome lists.
    pub fn biome_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.biomes.iter().map(|rule| rule.biome.clone()).collect();
        names.push("plains".into());
        names.dedup();
        names
    }

    pub fn surface(&self, column: &Column) -> &Surface {
        let biome = self.biome(column);
        self.surface.iter().find(|one| one.biome == biome).unwrap_or(&self.fallback_surface)
    }

    /// The block a map shows from above: the top block, or the fluid over sunken ground.
    pub fn top_block(&self, column: &Column) -> &str {
        if column.height < self.sea {
            &self.fluid
        } else {
            &self.surface(column).top
        }
    }

    fn ground(&self, x: i32, y: i32, z: i32, column: &Column) -> bool {
        if self.overhangs <= 0.0 {
            return y <= column.height;
        }
        let (wx, wz) = self.warped(f64::from(x), f64::from(z));
        let density = f64::from(column.height - y) / 16.0 + self.overhangs * perlin3(self.seed.wrapping_add(21), wx / 48.0, f64::from(y) / 32.0, wz / 48.0);
        density > 0.0
    }

    fn cave(&self, x: i32, y: i32, z: i32) -> bool {
        self.caves.iter().enumerate().any(|(index, cave)| {
            let seed = self.seed.wrapping_add(300 + 10 * index as i64);
            let (fx, fy, fz) = (f64::from(x), f64::from(y), f64::from(z));
            match *cave {
                Cave::Spaghetti { scale, width } => {
                    perlin3(seed, fx / scale, fy / (scale * 0.5), fz / scale).abs() < width
                        && perlin3(seed.wrapping_add(1), fx / scale, fy / (scale * 0.5), fz / scale).abs() < width
                }
                Cave::Cheese { scale, threshold } => perlin3(seed, fx / scale, fy / (scale * 0.6), fz / scale) > threshold,
            }
        })
    }

    /// The highest y the ground can reach in a column.
    fn reach(&self, column: &Column) -> i32 {
        column.height + (16.0 * self.overhangs).ceil() as i32 + 1
    }

    /// The column's blocks, bottom first, from `min_y` to `max_y` (both inclusive). Air isn't listed.
    pub fn runs(&self, x: i32, z: i32, min_y: i32, max_y: i32) -> Vec<Run> {
        let column = self.column(x, z);
        let surface = self.surface(&column).clone();
        let top = self.reach(&column).max(self.sea).min(max_y);
        let mut out: Vec<Run> = Vec::new();
        let mut put = |y: i32, block: &str| match out.last_mut() {
            Some(last) if last.block == block && last.from == y + 1 => last.from = y,
            _ => out.push(Run { from: y, to: y, block: block.to_string() }),
        };
        // How deep under the last open sky (or water) we are; -1 while in the open.
        let mut depth = -1;
        let mut wet = false;
        for y in (min_y..=top).rev() {
            if self.bedrock && y == min_y {
                put(y, "bedrock");
                continue;
            }
            if !self.ground(x, y, z, &column) {
                depth = -1;
                if y < self.sea {
                    put(y, &self.fluid);
                    wet = true;
                } else {
                    wet = false;
                }
                continue;
            }
            depth += 1;
            if !self.caves.is_empty() && y > min_y + 4 && y < column.height - 4 && self.cave(x, y, z) {
                continue;
            }
            let block = if depth == 0 {
                if wet || y < self.sea - 1 { surface.underwater.as_str() } else { surface.top.as_str() }
            } else if depth <= surface.depth {
                if wet { surface.underwater.as_str() } else { surface.under.as_str() }
            } else {
                self.stone.as_str()
            };
            put(y, block);
        }
        out.reverse();
        out
    }

    /// The top solid y of a column (caves ignored), for features.
    fn top_y(&self, x: i32, z: i32, column: &Column) -> i32 {
        if self.overhangs <= 0.0 {
            return column.height;
        }
        let mut y = self.reach(column);
        while y > column.height - 64 && !self.ground(x, y, z, column) {
            y -= 1;
        }
        y
    }

    /// Feature blocks inside chunk (`cx`, `cz`), including the parts of features that start next door, so trees
    /// and boulders aren't cut at chunk edges.
    pub fn features(&self, cx: i32, cz: i32, min_y: i32, max_y: i32) -> Vec<Place> {
        let mut out = Vec::new();
        for (index, feature) in self.features.iter().enumerate() {
            // A structure reaches up to 16 blocks from its middle, so it may start two chunks away.
            let reach = if matches!(feature.kind, FeatureKind::Structure { .. }) { 2 } else { 1 };
            for nx in cx - reach..=cx + reach {
                for nz in cz - reach..=cz + reach {
                    let mut rng = Rng::new(hash(self.seed.wrapping_add(5000 + index as i64), nx, 0, nz));
                    let mut count = feature.per_chunk.floor() as i32;
                    if rng.unit() < feature.per_chunk.fract() {
                        count += 1;
                    }
                    for _ in 0..count {
                        let x = nx * 16 + rng.between(0, 15);
                        let z = nz * 16 + rng.between(0, 15);
                        let column = self.column(x, z);
                        if !feature.biomes.is_empty() && !feature.biomes.iter().any(|one| one == self.biome(&column)) {
                            rng.next();
                            continue;
                        }
                        self.place(&feature.kind, &mut rng, x, z, &column, &mut out);
                    }
                }
            }
        }
        out.retain(|one| one.x.div_euclid(16) == cx && one.z.div_euclid(16) == cz && one.y >= min_y && one.y <= max_y);
        out
    }

    fn place(&self, kind: &FeatureKind, rng: &mut Rng, x: i32, z: i32, column: &Column, out: &mut Vec<Place>) {
        let dry = column.height >= self.sea;
        match kind {
            FeatureKind::Ore { block, y, size } => {
                let (mut px, mut py, mut pz) = (x, rng.between(y.0, y.1), z);
                for _ in 0..*size {
                    out.push(Place { x: px, y: py, z: pz, block: block.clone(), only: Only::Stone });
                    match rng.between(0, 5) {
                        0 => px += 1,
                        1 => px -= 1,
                        2 => py += 1,
                        3 => py -= 1,
                        4 => pz += 1,
                        _ => pz -= 1,
                    }
                }
            }
            FeatureKind::Tree { trunk, leaves, height } => {
                let tall = rng.between(height.0, height.1);
                if !dry {
                    return;
                }
                let ground = self.top_y(x, z, column);
                if self.top_block(column) == self.fluid {
                    return;
                }
                let crown = ground + tall;
                for dy in -2..=1 {
                    let radius: i32 = if dy >= 0 { 1 } else { 2 };
                    for dx in -radius..=radius {
                        for dz in -radius..=radius {
                            let corner = dx.abs() == radius && dz.abs() == radius;
                            if corner && (dy == 1 || rng.unit() < 0.5) {
                                continue;
                            }
                            out.push(Place { x: x + dx, y: crown + dy, z: z + dz, block: leaves.clone(), only: Only::Air });
                        }
                    }
                }
                for y in ground + 1..crown {
                    out.push(Place { x, y, z, block: trunk.clone(), only: Only::Any });
                }
            }
            FeatureKind::Boulder { block, size } => {
                if !dry {
                    return;
                }
                let ground = self.top_y(x, z, column);
                let r = *size;
                for dx in -r..=r {
                    for dy in -r..=r {
                        for dz in -r..=r {
                            if dx * dx + dy * dy + dz * dz <= r * r {
                                out.push(Place { x: x + dx, y: ground + dy, z: z + dz, block: block.clone(), only: Only::Any });
                            }
                        }
                    }
                }
            }
            FeatureKind::Patch { block } => {
                if dry && self.top_block(column) != self.fluid {
                    out.push(Place { x, y: self.top_y(x, z, column) + 1, z, block: block.clone(), only: Only::Air });
                }
            }
            FeatureKind::Structure { id, rotation, lift } => {
                let quarter = structure::quarter_of(rotation, rng.between(0, 3) as u8);
                let Some(turned) = self.structures.get(id) else { return };
                if !dry || self.top_block(column) == self.fluid {
                    return;
                }
                let (w, d, blocks) = &turned[quarter as usize];
                let (sx, sy, sz) = (x - w / 2, self.top_y(x, z, column) + 1 + lift, z - d / 2);
                out.extend(blocks.iter().map(|(dx, dy, dz, state)| Place { x: sx + dx, y: sy + dy, z: sz + dz, block: state.clone(), only: Only::Any }));
            }
        }
    }

    /// Every block name the generator puts down, for the add-ons to look up once.
    pub fn block_names(&self) -> Vec<String> {
        let mut names = vec![self.stone.clone(), self.fluid.clone(), "bedrock".into()];
        for one in self.surface.iter().chain(std::iter::once(&self.fallback_surface)) {
            names.extend([one.top.clone(), one.under.clone(), one.underwater.clone()]);
        }
        for feature in &self.features {
            match &feature.kind {
                FeatureKind::Ore { block, .. } | FeatureKind::Boulder { block, .. } | FeatureKind::Patch { block } => names.push(block.clone()),
                FeatureKind::Tree { trunk, leaves, .. } => names.extend([trunk.clone(), leaves.clone()]),
                FeatureKind::Structure { .. } => {}
            }
        }
        for turned in self.structures.values() {
            for (_, _, blocks) in turned.iter() {
                names.extend(blocks.iter().map(|one| one.3.clone()));
            }
        }
        names.sort();
        names.dedup();
        names
    }
}

/// What is wrong in a `"generator"` block, for the app and the add-ons' status.
pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    let mut known = |value: Option<&str>, list: &[&str], what: &str| {
        if let Some(one) = value {
            if !list.contains(&one) {
                out.push(format!("Unknown {what} \"{one}\" (use {}).", list.join(", ")));
            }
        }
    };
    known(data.get("warp").and_then(Value::as_object).and_then(|one| one.get("type")).and_then(Value::as_str), &WARPS, "warp");
    for layer in objects(data, "layers") {
        known(layer.get("noise").and_then(Value::as_str), &NOISES, "noise");
    }
    known(data.get("mode").and_then(Value::as_str), &MODES, "mode");
    known(data.get("terrain").and_then(Value::as_str), &TERRAINS, "terrain");
    // (node count, deepest) of the shape, told after the names.
    let mut shape_size = None;
    if let Some(shape) = data.get("shape").and_then(Value::as_object) {
        let mut nodes = Vec::new();
        shape_nodes(shape, 0, &mut nodes);
        for (node, _) in &nodes {
            known(node.get("type").and_then(Value::as_str), &SHAPES, "node");
            known(node.get("noise").and_then(Value::as_str), &NOISES, "noise");
        }
        shape_size = Some((nodes.len(), nodes.iter().map(|one| one.1).max().unwrap_or(0)));
    }
    for cave in objects(data, "caves") {
        known(cave.get("type").and_then(Value::as_str), &CAVES, "cave");
    }
    for feature in objects(data, "features") {
        known(feature.get("type").and_then(Value::as_str), &FEATURES, "feature");
    }
    if objects(data, "features").any(|one| one.get("type").and_then(Value::as_str) == Some("structure") && text(one, "structure", "").trim().is_empty()) {
        out.push("A structure feature needs a structure.".into());
    }
    if let Some((count, deepest)) = shape_size {
        if count > MOST_SHAPE_NODES {
            out.push(format!("The shape has {count} nodes; only the first {MOST_SHAPE_NODES} are used."));
        }
        if deepest > MOST_SHAPE_DEPTH {
            out.push(format!("The shape nests deeper than {MOST_SHAPE_DEPTH}; deeper nodes count as 0."));
        }
    }
    let vanilla = data.get("mode").and_then(Value::as_str) == Some("vanilla");
    if vanilla && data.get("warp").and_then(Value::as_object).and_then(|one| one.get("type")).and_then(Value::as_str) == Some("domain") {
        out.push("On the server's own ground only the spiral twist works (Pumpkin).".into());
    }
    if !vanilla && data.get("shape").is_none() && num(data, "sea", 63.0) > num(data, "base", 68.0) + num(data, "height", 40.0) {
        out.push("The sea is above the highest ground: the whole world is under water.".into());
    }
    out
}

/// Every node of a shape with its depth, pre-order.
fn shape_nodes<'a>(data: &'a Data, depth: usize, out: &mut Vec<(&'a Data, usize)>) {
    out.push((data, depth));
    if out.len() > MOST_SHAPE_NODES * 4 {
        return;
    }
    if let Some(one) = data.get("input").and_then(Value::as_object) {
        shape_nodes(one, depth + 1, out);
    }
    for one in objects(data, "inputs") {
        shape_nodes(one, depth + 1, out);
    }
}

/// A top-down map for the app: `size`×`size` spots `step` blocks apart from (`x`, `z`), row by row: the height,
/// the block seen from above and the biome. On vanilla ground only the biome is known (empty: the server's own).
pub fn preview(generator: &Generator, x: i32, z: i32, size: u32, step: u32) -> Vec<(i32, String, String)> {
    let step = step.max(1) as i32;
    let mut out = Vec::with_capacity((size * size) as usize);
    for row in 0..size as i32 {
        for col in 0..size as i32 {
            let (bx, bz) = (x + col * step, z + row * step);
            if generator.vanilla.is_some() {
                out.push((generator.sea, String::new(), generator.vanilla_biome(bx, bz).unwrap_or_default().to_string()));
                continue;
            }
            let column = generator.column(bx, bz);
            out.push((column.height, generator.top_block(&column).to_string(), generator.biome(&column).to_string()));
        }
    }
    out
}

fn fields_choice(data: &Data, key: &str, list: &[&str], fallback: &str) -> String {
    let value = text(data, key, fallback);
    if list.contains(&value.trim()) { value.trim().to_string() } else { fallback.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data(value: Value) -> Data {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn seeds_like_java() {
        assert_eq!(java_hash("hello"), 99_162_322);
        assert_eq!(seed_of("12345", "w"), 12345);
        assert_eq!(seed_of("hello", "w"), 99_162_322);
        assert_eq!(seed_of(" ", "spiral"), i64::from(java_hash("spiral")));
    }

    #[test]
    fn same_seed_same_world() {
        let spec = data(json!({ "layers": [{ "noise": "ridged", "scale": 128 }], "caves": [{ "type": "cheese" }] }));
        let (a, b) = (Generator::new(&spec, 7), Generator::new(&spec, 7));
        assert_eq!(a.runs(100, -40, -64, 320), b.runs(100, -40, -64, 320));
        assert_ne!(Generator::new(&spec, 8).column(100, -40), a.column(100, -40));
    }

    #[test]
    fn noise_stays_in_range() {
        for i in 0..2000 {
            let (x, z) = (f64::from(i) * 0.37 - 300.0, f64::from(i) * 0.91 + 11.0);
            for kind in [Noise::Perlin, Noise::Ridged, Noise::Billow, Noise::Cellular] {
                let value = fbm(kind, 3, x, z, 4);
                assert!((-1.0..=1.0).contains(&value), "{kind:?} {value}");
            }
            assert!((-1.0..=1.0).contains(&perlin3(3, x, z * 0.5, x - z)));
        }
    }

    #[test]
    fn columns_are_built_bottom_up() {
        let flat = data(json!({ "layers": [], "base": 70, "height": 0, "sea": 63, "surface": [{ "top": "grass_block", "under": "dirt", "depth": 2 }] }));
        let runs = Generator::new(&flat, 1).runs(0, 0, -64, 320);
        assert_eq!(runs[0], Run { from: -64, to: -64, block: "bedrock".into() });
        assert_eq!(runs.last().unwrap(), &Run { from: 70, to: 70, block: "grass_block".into() });
        assert_eq!(runs[runs.len() - 2], Run { from: 68, to: 69, block: "dirt".into() });
        let sunk = data(json!({ "layers": [], "base": 50, "height": 0, "sea": 63 }));
        let runs = Generator::new(&sunk, 1).runs(0, 0, -64, 320);
        assert_eq!(runs.last().unwrap(), &Run { from: 51, to: 62, block: "water".into() });
        assert_eq!(runs[runs.len() - 2].block, "gravel");
    }

    #[test]
    fn spiral_turns_the_map() {
        let plain = data(json!({ "layers": [{ "scale": 64 }] }));
        let mut spiral = plain.clone();
        spiral.insert("warp".into(), json!({ "type": "spiral", "strength": 3 }));
        let (a, b) = (Generator::new(&plain, 5), Generator::new(&spiral, 5));
        assert_eq!(a.column(0, 0).height, b.column(0, 0).height);
        assert_ne!(a.column(400, 0).height, b.column(400, 0).height);
    }

    #[test]
    fn biomes_and_features() {
        let spec = data(json!({
            "layers": [], "base": 80, "height": 0,
            "biomes": [{ "biome": "minecraft:Desert", "temperature": [0, 1] }],
            "surface": [{ "biome": "desert", "top": "sand", "under": "sandstone" }],
            "features": [{ "type": "tree", "per_chunk": 4 }, { "type": "ore", "block": "iron_ore", "per_chunk": 3, "y": [0, 10] }],
        }));
        let generator = Generator::new(&spec, 9);
        let column = generator.column(3, 3);
        assert_eq!(generator.biome(&column), "desert");
        assert_eq!(generator.top_block(&column), "sand");
        let placed = generator.features(0, 0, -64, 320);
        assert!(placed.iter().all(|one| (0..16).contains(&one.x) && (0..16).contains(&one.z)));
        assert!(placed.iter().any(|one| one.block == "oak_log" && one.y > 80));
        assert!(placed.iter().any(|one| one.block == "iron_ore" && one.only == Only::Stone));
        assert!(generator.block_names().contains(&"iron_ore".to_string()));
    }

    #[test]
    fn names_what_is_wrong() {
        assert!(problems(&data(json!({ "warp": { "type": "twist" } })))[0].contains("warp"));
        assert!(problems(&data(json!({ "layers": [{ "noise": "x" }] })))[0].contains("noise"));
        assert!(problems(&data(json!({ "sea": 200 })))[0].contains("under water"));
        assert!(problems(&data(json!({}))).is_empty());
    }
}

/// Numbers the Java copy (`StudioWorldGen.java`) gave for the same files on 2026-11-22. If these change, the Paper
/// add-on must change the same way.
#[cfg(test)]
mod parity {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_the_java_copy() {
        let a = json!({ "warp": { "type": "spiral", "strength": 2 }, "layers": [
            { "noise": "perlin", "scale": 128, "octaves": 4, "amount": 1 }, { "noise": "ridged", "scale": 64, "octaves": 2, "amount": 0.3 },
            { "noise": "cellular", "scale": 90, "octaves": 1, "amount": 0.2 }, { "noise": "billow", "scale": 50, "octaves": 2, "amount": 0.1 }],
            "biomes": [{ "biome": "desert", "temperature": [0.5, 1] }],
            "features": [{ "type": "tree", "per_chunk": 3 }, { "type": "ore", "per_chunk": 4.5, "size": 5 }, { "type": "boulder", "per_chunk": 0.5 }, { "type": "patch", "per_chunk": 6 }] });
        let b = json!({ "warp": { "type": "domain", "scale": 100, "amount": 40 }, "overhangs": 0.5, "caves": [{ "type": "cheese", "threshold": 0.3 }, { "type": "spaghetti", "width": 0.2 }] });
        let spots = [(0, 0), (100, -37), (-512, 800), (3001, 2999)];
        let want = [([76, 70, 68, 86], ["desert", "plains", "plains", "plains"], 268), ([68, 75, 66, 60], ["plains"; 4], 0)];
        for (spec, (heights, biomes, placed)) in [a, b].into_iter().zip(want) {
            let made = Generator::new(spec.as_object().unwrap(), 12345);
            for (index, (x, z)) in spots.into_iter().enumerate() {
                let column = made.column(x, z);
                assert_eq!((column.height, made.biome(&column)), (heights[index], biomes[index]), "{x} {z}");
            }
            let features = made.features(0, 0, -64, 319);
            assert_eq!(features.len(), placed);
        }
        let made = Generator::new(json!({ "warp": { "type": "spiral", "strength": 2 } }).as_object().unwrap(), 12345);
        assert!((made.column(100, -37).temperature - 0.498_444_494).abs() < 1e-6);
    }

    /// Shapes, biome lifts and vanilla biomes (2026-11-23, `StudioWorldGen.java` gave the same).
    #[test]
    fn matches_the_java_copy_with_shapes() {
        let specs: Vec<Value> = serde_json::from_str(r#"[{"shape":{"type":"add","inputs":[{"type":"noise","noise":"ridged","scale":120,"octaves":3,"amount":0.8},{"type":"terrace","steps":5,"input":{"type":"noise","scale":300}},{"type":"multiply","inputs":[{"type":"climate","which":"wetness"},{"type":"constant","value":0.3}]},{"type":"power","exponent":1.5,"input":{"type":"distance","x":40,"z":-20,"radius":900}},{"type":"clamp","min":-0.2,"max":0.4,"input":{"type":"noise","noise":"cellular","scale":80}},{"type":"remap","from":[-1,1],"to":[0,0.5],"input":{"type":"max","inputs":[{"type":"noise","noise":"billow","scale":64},{"type":"abs","input":{"type":"noise","scale":40}}]}}]},"biomes":[{"biome":"desert","temperature":[0.55,1],"offset":25,"stretch":1.6},{"biome":"studio:ash","humidity":[0.6,1],"offset":-12,"stretch":0.4}],"blend":0.12,"warp":{"type":"spiral","strength":1.5}},{"mode":"vanilla","biomes":[{"biome":"studio:ash","temperature":[0.5,1],"humidity":[0,0.5]}]}]"#).unwrap();
        let spots = [(0, 0), (100, -37), (-512, 800), (3001, 2999), (77, 1234), (-2048, -999)];
        let shaped = Generator::new(specs[0].as_object().unwrap(), 12345);
        let want = [(165, "plains"), (151, "desert"), (98, "plains"), (-188, "plains"), (119, "plains"), (33, "studio:ash")];
        for (index, (x, z)) in spots.into_iter().enumerate() {
            let column = shaped.column(x, z);
            assert_eq!((column.height, shaped.biome(&column)), want[index], "{x} {z}");
        }
        let vanilla = Generator::new(specs[1].as_object().unwrap(), 12345);
        let want = [Some("studio:ash"), None, Some("studio:ash"), Some("studio:ash"), None, None];
        for (index, (x, z)) in spots.into_iter().enumerate() {
            assert_eq!(vanilla.vanilla_biome(x, z), want[index], "{x} {z}");
        }
    }

}

#[cfg(test)]
mod structure_tests {
    use super::*;
    use serde_json::json;

    fn data(value: Value) -> Data {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn structure_features() {
        let spec = data(json!({ "height": 0, "base": 70, "sea": 0, "layers": [], "features": [{ "type": "structure", "structure": "hut", "per_chunk": 1, "rotation": "90" }] }));
        let mut generator = Generator::new(&spec, 1);
        assert_eq!(generator.structure_ids(), vec!["hut".to_string()]);
        let hut = Structure { size: (3, 2, 1), palette: vec!["oak_stairs[facing=north]".into()], blocks: vec![(0, 0, 0, 0), (2, 1, 0, 0)] };
        assert!(generator.add_structure("hut", &hut));
        let found: Vec<Place> = (-3..=3).flat_map(|cx| (-3..=3).map(move |cz| (cx, cz))).flat_map(|(cx, cz)| generator.features(cx, cz, -64, 320)).collect();
        assert!(!found.is_empty());
        assert!(found.iter().all(|one| one.block == "oak_stairs[facing=east]"));
        assert!(generator.block_names().contains(&"oak_stairs[facing=east]".to_string()));
        let wide = Structure { size: (40, 1, 1), ..Structure::default() };
        assert!(!generator.add_structure("wide", &wide));
        assert!(problems(&data(json!({ "features": [{ "type": "structure" }] }))).iter().any(|one| one.contains("needs a structure")));
    }
}

#[cfg(test)]
mod shape_tests {
    use super::*;
    use serde_json::json;

    fn data(value: Value) -> Data {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn shapes_make_the_ground() {
        let flat = data(json!({ "base": 64, "height": 10, "shape": { "type": "add", "inputs": [{ "type": "constant", "value": 0.5 }, { "type": "constant", "value": 0.3 }] } }));
        assert_eq!(Generator::new(&flat, 1).column(5, 5).height, 72);
        let clamped = data(json!({ "base": 64, "height": 10, "shape": { "type": "clamp", "min": -0.2, "max": 0.2, "input": { "type": "constant", "value": 3 } } }));
        assert_eq!(Generator::new(&clamped, 1).column(0, 0).height, 66);
        let terrace = data(json!({ "base": 0, "height": 100, "shape": { "type": "terrace", "steps": 4, "input": { "type": "constant", "value": 0.5 } } }));
        assert_eq!(Generator::new(&terrace, 1).column(0, 0).height, 50);
        let island = data(json!({ "base": 0, "height": 100, "shape": { "type": "distance", "radius": 100 } }));
        let made = Generator::new(&island, 1);
        assert_eq!(made.column(0, 0).height, 100);
        assert_eq!(made.column(50, 0).height, 50);
        let remap = data(json!({ "base": 0, "height": 100, "shape": { "type": "remap", "from": [0, 1], "to": [1, 3], "input": { "type": "constant", "value": 0.5 } } }));
        assert_eq!(Generator::new(&remap, 1).column(0, 0).height, 200);
        let power = data(json!({ "base": 0, "height": 100, "shape": { "type": "power", "exponent": 2, "input": { "type": "constant", "value": -0.5 } } }));
        assert_eq!(Generator::new(&power, 1).column(0, 0).height, -25);
        // The same noise as a layer and as a node gives hills, and two noise nodes don't share a seed.
        let noisy = data(json!({ "shape": { "type": "add", "inputs": [{ "type": "noise", "scale": 64 }, { "type": "noise", "scale": 64, "amount": -1 }] } }));
        let made = Generator::new(&noisy, 3);
        assert!((0..40).any(|i| made.column(i * 37, i * 11).height != 68));
        assert!(problems(&data(json!({ "shape": { "type": "spin" } }))).iter().any(|one| one.contains("node")));
    }

    #[test]
    fn biome_ground_blends_across_edges() {
        let spec = |blend: f64| data(json!({ "layers": [], "base": 64, "height": 0, "blend": blend, "climate": 64,
            "biomes": [{ "biome": "desert", "temperature": [0.5, 1], "offset": 40 }] }));
        let (hard, soft) = (Generator::new(&spec(0.0), 7), Generator::new(&spec(0.2), 7));
        let mut steps_hard = 0;
        let mut steps_soft = 0;
        let mut biggest_soft = 0;
        for x in 0..2000 {
            let (a, b) = (hard.column(x, 0).height, hard.column(x + 1, 0).height);
            let (c, d) = (soft.column(x, 0).height, soft.column(x + 1, 0).height);
            assert!([64, 104].contains(&a));
            if a != b {
                steps_hard += 1;
            }
            if c != d {
                steps_soft += 1;
            }
            biggest_soft = biggest_soft.max((c - d).abs());
        }
        assert!(steps_hard > 0);
        // Softer edges climb in many small steps instead of a few 40-block cliffs.
        assert!(steps_soft > steps_hard);
        assert!(biggest_soft < 40);
    }

    #[test]
    fn vanilla_mode_picks_biomes_by_climate() {
        let spec = data(json!({ "mode": "vanilla", "terrain": "amplified", "warp": { "type": "spiral", "strength": 2 },
            "biomes": [{ "biome": "studio:ash", "temperature": [0.5, 1] }] }));
        let made = Generator::new(&spec, 11);
        assert_eq!(made.vanilla(), Some("amplified"));
        assert_eq!(made.spiral(), Some((2.0, 0.0, 0.0)));
        let picked: Vec<Option<&str>> = (0..400).map(|i| made.vanilla_biome(i * 50, -i * 30)).collect();
        assert!(picked.contains(&Some("studio:ash")));
        assert!(picked.contains(&None));
        assert!(made.biome_names().contains(&"studio:ash".to_string()));
        let map = preview(&made, 0, 0, 8, 64);
        assert!(map.iter().all(|one| one.1.is_empty()));
        assert!(problems(&data(json!({ "mode": "vanilla", "warp": { "type": "domain" } }))).iter().any(|one| one.contains("spiral")));
        assert!(problems(&data(json!({ "mode": "sideways" }))).iter().any(|one| one.contains("mode")));
        assert!(Generator::new(&data(json!({})), 1).vanilla().is_none());
    }
}

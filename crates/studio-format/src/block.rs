//! Studio maker M3: custom blocks (`block` kind). A studio block borrows a vanilla block state the game has but never
//! shows on its own (a note block's instrument and note, a mushroom block's faces, a tripwire's wires) and the app's
//! pack draws our model for it. Which state a block has is a number in `studio/pack/ids.json` (kind `block` for
//! cubes, `plant` for plants), given once and never moved or handed on, so a placed block never turns into another.
//! Shared by the app's pack builder (`src-tauri/src/studio_pack_blocks.rs`) and the Pumpkin add-on; the Paper add-on
//! keeps the same rules in `StudioBlocks.java`.
//!
//! ```json
//! "shape": "cube",                                    // or "plant" (no collision, drawn as a cross)
//! "look": { "texture": "ruby_ore" },                  // or { "model": "ruby_ore" }, or side + "top" + "bottom"
//! "hardness": 3, "tool": "pickaxe", "needsTool": true,
//! "light": 0,                                         // 0-15; Pumpkin only (a real block there, a borrowed state on Paper)
//! "sounds": "stone",                                  // a vanilla block whose sounds it uses
//! "drops": { "self": true, "loot": "", "exp": 0 },
//! "item": "",                                         // a studio item that places it; empty = its own item
//! "name": "Ruby ore", "lore": [],
//! "placeActions": [], "breakActions": [], "useActions": [],
//! "growth": {                                         // plants only: a crop that grows in stages
//!   "stages": ["wheat_1", "wheat_2", "wheat_3"],      // the look of each later stage (the block's own look is stage 0)
//!   "minutes": 5, "light": 9,                         // average minutes per stage; least light to grow (Paper only)
//!   "soil": ["farmland"], "water": false,             // what it is planted on ([] = any solid block); moist farmland only
//!   "bonemeal": true,                                 // bone meal grows it one or two stages
//!   "harvest": { "loot": "wheat", "exp": 1 }          // rolled on top of "drops" when it breaks fully grown
//! }
//! ```
//!
//! Each later stage borrows a plant state of its own, numbered in `ids.json` kind `plant` as `<id>/<stage>`.

use crate::fields::{self, Data};
use crate::item::{self, TOOL_KINDS};
use serde_json::Value;

pub const SHAPES: [&str; 2] = ["cube", "plant"];
/// What starts each action list: placing it, breaking it, right-clicking it.
pub const ACTION_LISTS: [&str; 3] = ["placeActions", "breakActions", "useActions"];
/// `ids.json` kinds: cube blocks and plant blocks are numbered apart (they borrow different blocks).
pub const IDS_CUBE: &str = "block";
pub const IDS_PLANT: &str = "plant";
/// Numbers start at 1; 0 is each family's vanilla look, never given.
pub const FIRST: u32 = 1;
/// The items a block is placed with when it has no studio item of its own: also the base items whose custom model
/// data the pack overrides before 1.21.4. Not block items (like CraftEngine): with `note_block` or `string` the client
/// put the vanilla block down itself before the server placed ours, and played its own place sound too.
pub const CUBE_ITEM: &str = "paper";
pub const PLANT_ITEM: &str = "paper";
pub const MOST_HARDNESS: f64 = 50.0;
/// Stages a crop may have past its first.
pub const MOST_STAGES: usize = 15;
/// Seconds between two random ticks of one block at the default `randomTickSpeed` (3 per 16×16×16 section a tick).
pub const RANDOM_TICK_SECONDS: f64 = 4096.0 / 3.0 / 20.0;
/// What a crop is planted on when its file doesn't say.
pub const DEFAULT_SOIL: [&str; 1] = ["farmland"];

/// Note block instruments every version since 1.14 has (later ones add mob heads; those states stay vanilla).
pub const INSTRUMENTS: [&str; 16] = [
    "harp", "basedrum", "snare", "hat", "bass", "flute", "bell", "guitar", "chime", "xylophone", "iron_xylophone", "cow_bell", "didgeridoo", "bit", "banjo", "pling",
];
pub const NOTES: u32 = 25;
/// Note block states we may give: 16 instruments × 25 notes × powered, less `harp, note 0, unpowered` (vanilla).
pub const NOTE_STATES: u32 = INSTRUMENTS.len() as u32 * NOTES * 2 - 1;
pub const MUSHROOMS: [&str; 3] = ["brown_mushroom_block", "red_mushroom_block", "mushroom_stem"];
/// Mushroom block faces, as the game sorts them.
pub const FACES: [&str; 6] = ["down", "east", "north", "south", "up", "west"];
/// 64 face mixes per mushroom block, less "every face" (what a placed one has).
pub const MUSHROOM_STATES: u32 = 63 * MUSHROOMS.len() as u32;
pub const CUBE_STATES: u32 = NOTE_STATES + MUSHROOM_STATES;
/// Tripwire properties, as the game sorts them.
pub const WIRE: [&str; 7] = ["attached", "disarmed", "east", "north", "powered", "south", "west"];
/// 128 tripwire states less "all off" (what a placed string has).
pub const PLANT_STATES: u32 = 127;

/// One borrowed block state: the block (no `minecraft:`) and its properties, sorted like the game sorts them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub block: &'static str,
    pub properties: Vec<(&'static str, String)>,
}

impl State {
    /// `instrument=bass,note=3,powered=false`: a blockstate file's variant key, and Paper's `[...]` part.
    pub fn key(&self) -> String {
        self.properties.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join(",")
    }

    /// `minecraft:note_block[instrument=bass,note=3,powered=false]`.
    pub fn text(&self) -> String {
        format!("minecraft:{}[{}]", self.block, self.key())
    }
}

/// The state behind a block's number, or `None` past the last (or at 0).
pub fn state(shape: &str, number: u32) -> Option<State> {
    if number == 0 {
        return None;
    }
    if shape == "plant" {
        if number > PLANT_STATES {
            return None;
        }
        let properties = WIRE.iter().enumerate().map(|(bit, name)| (*name, (number >> bit & 1 == 1).to_string())).collect();
        return Some(State { block: "tripwire", properties });
    }
    if number <= NOTE_STATES {
        let instrument = INSTRUMENTS[(number / (NOTES * 2)) as usize];
        let note = (number / 2) % NOTES;
        let properties = vec![("instrument", instrument.to_string()), ("note", note.to_string()), ("powered", (number % 2 == 1).to_string())];
        return Some(State { block: "note_block", properties });
    }
    let rest = number - NOTE_STATES - 1;
    if rest >= MUSHROOM_STATES {
        return None;
    }
    let faces = rest % 63;
    let properties = FACES.iter().enumerate().map(|(bit, name)| (*name, (faces >> bit & 1 == 1).to_string())).collect();
    Some(State { block: MUSHROOMS[(rest / 63) as usize], properties })
}

/// How many states a shape has to give.
pub fn most(shape: &str) -> u32 {
    if shape == "plant" { PLANT_STATES } else { CUBE_STATES }
}

pub fn ids_kind(shape: &str) -> &'static str {
    if shape == "plant" { IDS_PLANT } else { IDS_CUBE }
}

pub fn shape(data: &Data) -> String {
    let one = fields::text(data, "shape", "cube");
    if SHAPES.contains(&one.as_str()) { one } else { "cube".into() }
}

/// How a block looks: one texture on every side, a side texture with its own top and bottom, or a model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    pub texture: String,
    pub top: String,
    pub bottom: String,
    pub model: String,
}

pub fn look(data: &Data) -> Option<Look> {
    let Some(Value::Object(look)) = data.get("look") else { return None };
    let read = |key: &str| fields::text(look, key, "").trim().to_string();
    let one = Look { texture: read("texture"), top: read("top"), bottom: read("bottom"), model: read("model") };
    (!one.texture.is_empty() || !one.model.is_empty()).then_some(one)
}

/// Where the block's drops come from.
#[derive(Clone, Debug, PartialEq)]
pub struct Drops {
    /// Drops its own item (or the studio item that places it).
    pub own: bool,
    /// A studio loot table rolled on top.
    pub loot: String,
    pub exp: u32,
}

pub fn drops(data: &Data) -> Drops {
    let part = item::part(data, "drops");
    let empty = Data::new();
    let drops = part.unwrap_or(&empty);
    Drops {
        own: drops.get("self").and_then(Value::as_bool) != Some(false),
        loot: fields::text(drops, "loot", "").trim().to_string(),
        exp: fields::number(drops, "exp", 0, 0, 1000) as u32,
    }
}

pub fn hardness(data: &Data) -> f64 {
    item::float(data, "hardness", 1.0, 0.0, MOST_HARDNESS)
}

/// The light a block gives, 0-15. Only Pumpkin can: there it is a block of its own (`custom-block.wit`), while Paper's
/// borrowed note block and tripwire states give none.
pub fn light(data: &Data) -> u8 {
    fields::number(data, "light", 0, 0, 15) as u8
}

/// The vanilla block whose sounds a studio block makes (`stone`, `oak_planks`...), without `minecraft:`.
pub fn sounds(data: &Data) -> String {
    let fallback = if shape(data) == "plant" { "short_grass" } else { "stone" };
    let raw = fields::text(data, "sounds", fallback).trim().to_ascii_lowercase();
    let bare = raw.trim_start_matches("minecraft:").to_string();
    if bare.is_empty() { fallback.into() } else { bare }
}

/// How fast a held item mines a block that wants `tool`: the vanilla speed of its tier, or `None` when it isn't that
/// tool. `material` is the item's (`minecraft:iron_pickaxe`).
pub fn tool_speed(material: &str, tool: &str) -> Option<f64> {
    if tool.is_empty() {
        return None;
    }
    let bare = material.trim().trim_start_matches("minecraft:");
    let tier = bare.strip_suffix(&format!("_{tool}"))?;
    Some(match tier {
        "wooden" => 2.0,
        "stone" => 4.0,
        "copper" => 5.0,
        "iron" => 6.0,
        "diamond" => 8.0,
        "netherite" => 9.0,
        "golden" => 12.0,
        _ => 1.0,
    })
}

/// Ticks to break a block, like the game counts it: `hardness × 30` ticks per speed when the item may harvest it,
/// `× 100` when it may not. 0 = breaks at once.
pub fn break_ticks(hardness: f64, speed: f64, harvest: bool) -> u32 {
    if hardness <= 0.0 {
        return 0;
    }
    let per_tick = speed.max(0.01) / hardness / if harvest { 30.0 } else { 100.0 };
    if per_tick >= 1.0 { 0 } else { (1.0 / per_tick).ceil() as u32 }
}

/// Break ticks for a player holding `material`, from the block's fields.
pub fn ticks_for(data: &Data, material: &str) -> u32 {
    let tool = fields::text(data, "tool", "");
    let speed = tool_speed(material, &tool);
    let needs = data.get("needsTool").and_then(Value::as_bool) == Some(true);
    break_ticks(hardness(data), speed.unwrap_or(1.0), !needs || speed.is_some())
}

/// Break ticks and whether it drops, for a player holding `material`, which may be studio item `item` (its tool
/// rules and `tool.mines` count for our block `id`).
pub fn dig_with(data: &Data, id: &str, material: &str, item: Option<&Data>) -> (u32, bool) {
    let tool = fields::text(data, "tool", "");
    let needs = data.get("needsTool").and_then(Value::as_bool) == Some(true);
    if let Some((speed, right)) = item.and_then(|item| item::digs_studio_block(item, id, &tool)) {
        let harvest = !needs || right;
        return (break_ticks(hardness(data), speed, harvest), harvest);
    }
    (ticks_for(data, material), harvests(data, material))
}

/// Whether breaking it with `material` drops anything.
pub fn harvests(data: &Data, material: &str) -> bool {
    data.get("needsTool").and_then(Value::as_bool) != Some(true) || tool_speed(material, &fields::text(data, "tool", "")).is_some()
}

/// A crop's growing rules (`growth`), for plants with at least one later stage.
#[derive(Clone, Debug, PartialEq)]
pub struct Growth {
    /// The look of stages 1.. (a texture, or a model when the block's look is a model).
    pub stages: Vec<String>,
    /// Average minutes per stage.
    pub minutes: f64,
    /// Least light to grow, 0-15.
    pub light: u8,
    /// Blocks it may stand on, without `minecraft:`; empty = any solid block.
    pub soil: Vec<String>,
    /// Grows only on moist farmland.
    pub water: bool,
    pub bonemeal: bool,
    /// A studio loot table rolled when it breaks fully grown.
    pub harvest_loot: String,
    pub harvest_exp: u32,
}

impl Growth {
    /// The last stage's number (fully grown).
    pub fn last(&self) -> u32 {
        self.stages.len() as u32
    }

    /// Whether a crop may stand on (and grow on) `below` (`minecraft:farmland`).
    pub fn soil_ok(&self, below: &str) -> bool {
        let bare = below.trim().trim_start_matches("minecraft:");
        self.soil.is_empty() || self.soil.iter().any(|one| one == bare)
    }

    /// The chance that one check grows it a stage, when checks come every `seconds`.
    pub fn chance(&self, seconds: f64) -> f64 {
        (seconds / (self.minutes * 60.0)).clamp(0.0, 1.0)
    }
}

pub fn growth(data: &Data) -> Option<Growth> {
    if shape(data) != "plant" {
        return None;
    }
    let part = item::part(data, "growth")?;
    let stages: Vec<String> = fields::lines(part, "stages").iter().map(|one| one.trim().to_string()).filter(|one| !one.is_empty()).take(MOST_STAGES).collect();
    if stages.is_empty() {
        return None;
    }
    let soil = match part.get("soil") {
        Some(Value::Array(_)) => fields::lines(part, "soil").iter().map(|one| one.trim().trim_start_matches("minecraft:").to_ascii_lowercase()).filter(|one| !one.is_empty()).collect(),
        _ => DEFAULT_SOIL.iter().map(|one| (*one).to_string()).collect(),
    };
    let empty = Data::new();
    let harvest = item::part(part, "harvest").unwrap_or(&empty);
    Some(Growth {
        stages,
        minutes: item::float(part, "minutes", 5.0, 0.05, 1440.0),
        light: fields::number(part, "light", 0, 0, 15) as u8,
        soil,
        water: part.get("water").and_then(Value::as_bool) == Some(true),
        bonemeal: part.get("bonemeal").and_then(Value::as_bool) != Some(false),
        harvest_loot: fields::text(harvest, "loot", "").trim().to_string(),
        harvest_exp: fields::number(harvest, "exp", 0, 0, 1000) as u32,
    })
}

/// A later stage's name in `ids.json` kind `plant`.
pub fn stage_key(id: &str, stage: u32) -> String {
    format!("{id}/{stage}")
}

/// How many stages bone meal grows a crop: one or two, from a roll in 0..1.
pub fn bonemeal_stages(roll: f64) -> u32 {
    if roll < 0.5 { 1 } else { 2 }
}

/// What is wrong with a block file, for its status. Empty = fine.
pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    let raw_shape = fields::text(data, "shape", "cube");
    if !SHAPES.contains(&raw_shape.as_str()) {
        out.push(format!("\"shape\" must be one of: {}.", SHAPES.join(", ")));
    }
    match data.get("look") {
        Some(Value::Object(look)) => {
            let texture = fields::text(look, "texture", "");
            let model = fields::text(look, "model", "");
            if texture.trim().is_empty() && model.trim().is_empty() {
                out.push("\"look\" needs a texture or a model.".into());
            }
            if !texture.trim().is_empty() && !model.trim().is_empty() {
                out.push("\"look\" takes a texture or a model, not both.".into());
            }
            for key in ["texture", "top", "bottom", "model"] {
                let name = fields::text(look, key, "");
                if !name.trim().is_empty() && !crate::valid_id(name.trim()) {
                    out.push(format!("\"look\": {} isn't an asset name (a-z, 0-9, - and _).", name.trim()));
                }
            }
        }
        None | Some(Value::Null) => out.push("Pick a texture or a model in \"look\".".into()),
        Some(_) => out.push("\"look\" must be an object with a texture or a model.".into()),
    }
    let tool = fields::text(data, "tool", "");
    if !tool.is_empty() && !TOOL_KINDS.contains(&tool.as_str()) {
        out.push(format!("\"tool\" must be one of: {}.", TOOL_KINDS.join(", ")));
    }
    if data.get("needsTool").and_then(Value::as_bool) == Some(true) && tool.is_empty() {
        out.push("\"needsTool\" needs a \"tool\".".into());
    }
    let item = fields::text(data, "item", "");
    if !item.trim().is_empty() && !crate::valid_id(item.trim()) {
        out.push("\"item\" must be a studio item's file name.".into());
    }
    match data.get("growth") {
        None | Some(Value::Null) => {}
        Some(Value::Object(part)) => {
            if raw_shape != "plant" {
                out.push("\"growth\" is for plants only.".into());
            }
            let stages = fields::lines(part, "stages");
            if stages.iter().all(|one| one.trim().is_empty()) {
                out.push("\"growth\" needs at least one later stage in \"stages\".".into());
            }
            if stages.len() > MOST_STAGES {
                out.push(format!("\"growth\" takes at most {MOST_STAGES} later stages."));
            }
            for name in stages.iter().map(|one| one.trim()).filter(|one| !one.is_empty()) {
                if !crate::valid_id(name) {
                    out.push(format!("\"growth\": {name} isn't an asset name (a-z, 0-9, - and _)."));
                }
            }
        }
        Some(_) => out.push("\"growth\" must be an object.".into()),
    }
    out
}

/// The instrument a plain note block plays over `below` (`minecraft:oak_planks`), as the game picks it. Plain note
/// blocks stay at `harp, note 0` in the world (every other state may be one of ours), so the add-ons play them.
pub fn instrument_under(below: &str) -> &'static str {
    let name = below.trim_start_matches("minecraft:");
    match name {
        "clay" | "honeycomb_block" => return "flute",
        "gold_block" => return "bell",
        "packed_ice" => return "chime",
        "bone_block" => return "xylophone",
        "iron_block" => return "iron_xylophone",
        "soul_sand" => return "cow_bell",
        "pumpkin" | "carved_pumpkin" | "jack_o_lantern" => return "didgeridoo",
        "emerald_block" => return "bit",
        "hay_block" => return "banjo",
        "glowstone" => return "pling",
        "gravel" | "suspicious_gravel" | "suspicious_sand" => return "snare",
        "sea_lantern" | "beacon" => return "hat",
        _ => {}
    }
    if name.ends_with("_wool") {
        return "guitar";
    }
    if name == "sand" || name == "red_sand" || name.ends_with("concrete_powder") {
        return "snare";
    }
    if name.contains("glass") {
        return "hat";
    }
    const WOOD: [&str; 18] = [
        "_planks", "_log", "_wood", "_stem", "_hyphae", "bookshelf", "chest", "crafting_table", "note_block", "jukebox", "barrel", "lectern", "composter", "bamboo_block", "mushroom_block", "loom", "_table", "beehive",
    ];
    if WOOD.iter().any(|part| name.contains(part)) && !name.contains("ender_chest") {
        return "bass";
    }
    const STONE: [&str; 26] = [
        "stone", "cobble", "_ore", "netherrack", "obsidian", "quartz", "brick", "terracotta", "concrete", "deepslate", "basalt", "andesite", "diorite", "granite", "tuff", "calcite",
        "prismarine", "purpur", "nylium", "coral_block", "bedrock", "furnace", "dispenser", "dropper", "observer", "ender_chest",
    ];
    if STONE.iter().any(|part| name.contains(part)) {
        return "basedrum";
    }
    "harp"
}

/// A note's pitch (0 to 24 → 0.5 to 2.0).
pub fn note_pitch(note: u32) -> f32 {
    ((note.min(NOTES - 1) as f32 - 12.0) / 12.0).exp2()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_note_blocks_take_their_instrument_from_below() {
        assert_eq!(instrument_under("minecraft:oak_planks"), "bass");
        assert_eq!(instrument_under("minecraft:sandstone"), "basedrum");
        assert_eq!(instrument_under("minecraft:sand"), "snare");
        assert_eq!(instrument_under("minecraft:white_stained_glass"), "hat");
        assert_eq!(instrument_under("minecraft:gold_block"), "bell");
        assert_eq!(instrument_under("minecraft:red_wool"), "guitar");
        assert_eq!(instrument_under("minecraft:dirt"), "harp");
        assert!((note_pitch(12) - 1.0).abs() < 1e-6);
        assert!((note_pitch(24) - 2.0).abs() < 1e-6);
    }
    use serde_json::json;
    use std::collections::HashSet;

    fn data(value: Value) -> Data {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn every_number_is_its_own_state() {
        let mut seen = HashSet::new();
        for number in 1..=CUBE_STATES {
            let one = state("cube", number).unwrap();
            assert!(seen.insert(one.text()), "{number} repeats {}", one.text());
        }
        for number in 1..=PLANT_STATES {
            assert!(seen.insert(state("plant", number).unwrap().text()));
        }
        assert_eq!(state("cube", 0), None);
        assert_eq!(state("cube", CUBE_STATES + 1), None);
        assert_eq!(state("plant", PLANT_STATES + 1), None);
        // Vanilla looks are never given.
        assert!(!seen.contains("minecraft:note_block[instrument=harp,note=0,powered=false]"));
        for block in MUSHROOMS {
            assert!(!seen.contains(&format!("minecraft:{block}[down=true,east=true,north=true,south=true,up=true,west=true]")));
        }
        assert!(!seen.contains("minecraft:tripwire[attached=false,disarmed=false,east=false,north=false,powered=false,south=false,west=false]"));
    }

    #[test]
    fn states_read_right() {
        assert_eq!(state("cube", 1).unwrap().text(), "minecraft:note_block[instrument=harp,note=0,powered=true]");
        assert_eq!(state("cube", 2).unwrap().key(), "instrument=harp,note=1,powered=false");
        assert_eq!(state("cube", 50).unwrap().key(), "instrument=basedrum,note=0,powered=false");
        assert_eq!(state("cube", NOTE_STATES).unwrap().key(), "instrument=pling,note=24,powered=true");
        assert_eq!(state("cube", NOTE_STATES + 1).unwrap().text(), "minecraft:brown_mushroom_block[down=false,east=false,north=false,south=false,up=false,west=false]");
        assert_eq!(state("cube", CUBE_STATES).unwrap().text(), "minecraft:mushroom_stem[down=false,east=true,north=true,south=true,up=true,west=true]");
        assert_eq!(state("plant", 1).unwrap().key(), "attached=true,disarmed=false,east=false,north=false,powered=false,south=false,west=false");
    }

    #[test]
    fn break_times() {
        assert_eq!(break_ticks(0.0, 1.0, true), 0);
        // Stone by hand: 1.5 hardness, may not harvest → 150 ticks; with a wooden pickaxe 23.
        assert_eq!(break_ticks(1.5, 1.0, false), 150);
        assert_eq!(break_ticks(1.5, 2.0, true), 23);
        assert_eq!(tool_speed("minecraft:iron_pickaxe", "pickaxe"), Some(6.0));
        assert_eq!(tool_speed("iron_axe", "pickaxe"), None);
        assert_eq!(tool_speed("iron_pickaxe", ""), None);
        let ore = data(json!({ "hardness": 3, "tool": "pickaxe", "needsTool": true }));
        assert_eq!(ticks_for(&ore, "air"), 300);
        assert_eq!(ticks_for(&ore, "diamond_pickaxe"), 12);
        assert!(!harvests(&ore, "air"));
        assert!(harvests(&ore, "stone_pickaxe"));
    }

    #[test]
    fn crops() {
        let wheat = data(json!({ "shape": "plant", "look": { "texture": "w0" }, "growth": { "stages": ["w1", " ", "w2"], "minutes": 2, "light": 9, "harvest": { "loot": "wheat", "exp": 2 } } }));
        assert!(problems(&wheat).is_empty(), "{:?}", problems(&wheat));
        let grow = growth(&wheat).unwrap();
        assert_eq!(grow.stages, vec!["w1", "w2"]);
        assert_eq!(grow.last(), 2);
        assert!(grow.soil_ok("minecraft:farmland") && !grow.soil_ok("minecraft:dirt"));
        assert!(grow.bonemeal && !grow.water);
        assert_eq!((grow.harvest_loot.as_str(), grow.harvest_exp, grow.light), ("wheat", 2, 9));
        assert!((grow.chance(1.0) - 1.0 / 120.0).abs() < 1e-9);
        assert_eq!(grow.chance(1000.0), 1.0);
        let any = growth(&data(json!({ "shape": "plant", "growth": { "stages": ["a"], "soil": [] } }))).unwrap();
        assert!(any.soil_ok("minecraft:stone"));
        assert_eq!(growth(&data(json!({ "growth": { "stages": ["a"] } }))), None, "cubes don't grow");
        assert_eq!(growth(&data(json!({ "shape": "plant", "growth": { "stages": [] } }))), None);
        assert_eq!(problems(&data(json!({ "look": { "texture": "a" }, "growth": { "stages": ["b"] } }))).len(), 1);
        assert_eq!(problems(&data(json!({ "shape": "plant", "look": { "texture": "a" }, "growth": { "stages": [] } }))).len(), 1);
        assert_eq!(problems(&data(json!({ "shape": "plant", "look": { "texture": "a" }, "growth": { "stages": ["B C"] } }))).len(), 1);
        assert_eq!(stage_key("wheat", 3), "wheat/3");
        assert_eq!((bonemeal_stages(0.2), bonemeal_stages(0.9)), (1, 2));
    }

    #[test]
    fn studio_tools_dig_our_blocks() {
        let ore = data(json!({ "hardness": 3, "tool": "pickaxe", "needsTool": true }));
        let rule = data(json!({ "tool": { "rules": [{ "blocks": ["studio:ruby"], "speed": 30 }] } }));
        assert_eq!(dig_with(&ore, "ruby", "minecraft:stick", Some(&rule)), (3, true));
        assert_eq!(dig_with(&ore, "other", "minecraft:stick", Some(&rule)), (300, false));
        let pick = data(json!({ "tool": { "mines": ["pickaxe"], "speed": 6 } }));
        assert_eq!(dig_with(&ore, "x", "minecraft:paper", Some(&pick)), (15, true));
        assert_eq!(dig_with(&ore, "x", "minecraft:iron_pickaxe", None), (15, true));
    }

    #[test]
    fn fields_and_problems() {
        let good = data(json!({ "look": { "texture": "ruby_ore" }, "tool": "pickaxe", "needsTool": true }));
        assert!(problems(&good).is_empty(), "{:?}", problems(&good));
        assert_eq!(shape(&good), "cube");
        assert_eq!(sounds(&good), "stone");
        assert_eq!(sounds(&data(json!({ "shape": "plant" }))), "short_grass");
        assert_eq!(drops(&good), Drops { own: true, loot: String::new(), exp: 0 });
        assert!(!drops(&data(json!({ "drops": { "self": false, "loot": "ores" } }))).own);
        assert_eq!(problems(&data(json!({}))).len(), 1);
        assert_eq!(problems(&data(json!({ "look": { "texture": "a" }, "shape": "slab" }))).len(), 1);
        assert_eq!(problems(&data(json!({ "look": { "texture": "a", "model": "b" } }))).len(), 1);
        assert_eq!(problems(&data(json!({ "look": { "texture": "a" }, "needsTool": true }))).len(), 1);
        assert_eq!(problems(&data(json!({ "look": { "texture": "a" }, "tool": "sword" }))).len(), 1);
        let sides = look(&data(json!({ "look": { "texture": "side", "top": "top" } }))).unwrap();
        assert_eq!((sides.texture.as_str(), sides.top.as_str(), sides.bottom.as_str()), ("side", "top", ""));
    }
}

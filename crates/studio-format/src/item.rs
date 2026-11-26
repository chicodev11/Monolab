//! Studio maker M1: what a custom item (`item` kind) may say beyond material, name and lore. Shared by the app's pack
//! builder (`src-tauri/src/studio_pack_items.rs`) and the Pumpkin add-on; the Paper add-on keeps the same rules in
//! `StudioItems.java`.
//!
//! ```json
//! "look": { "texture": "ruby_sword", "handheld": true },          // or { "model": "ruby_sword" }
//! "look": { "texture": "ruby_bow", "pulling": ["ruby_bow_0", "ruby_bow_1", "ruby_bow_2"] },
//! "maxStack": 16, "rarity": "rare", "durability": 500, "hideTooltip": false, "cooldown": 1.5,
//! "food": { "nutrition": 4, "saturation": 2.4, "alwaysEat": false, "seconds": 1.6, "animation": "eat", "leftover": "bowl" },
//! "tool": { "mines": ["pickaxe"], "speed": 8, "damagePerBlock": 1,
//!           "rules": [{ "blocks": ["obsidian", "studio:ruby_ore"], "speed": 20, "drops": true }] },
//! "attack": { "damage": 7, "speed": 1.6 },
//! "equip": { "slot": "head", "armor": 3, "toughness": 0, "texture": "ruby_armor", "legsTexture": "ruby_armor_legs",
//!            "set": "ruby", "setEffects": [{ "effect": "speed", "level": 1 }],
//!            "trim": { "pattern": "sentry", "material": "gold" } },
//! "useActions": [], "eatActions": [], "hitActions": [], "breakActions": []
//! ```

use crate::fields::{self, Data};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const RARITIES: [&str; 4] = ["common", "uncommon", "rare", "epic"];
pub const EQUIP_SLOTS: [&str; 4] = ["head", "chest", "legs", "feet"];
/// Which blocks a tool mines fast: the game's `mineable/<kind>` block tags.
pub const TOOL_KINDS: [&str; 4] = ["pickaxe", "axe", "shovel", "hoe"];
pub const FOOD_ANIMATIONS: [&str; 2] = ["eat", "drink"];
/// The action lists an item can have, and what starts each.
pub const ACTION_LISTS: [&str; 4] = ["useActions", "eatActions", "hitActions", "breakActions"];
/// Bow-style pulling frames, at most.
pub const MOST_FRAMES: usize = 3;
/// Vanilla armor trim patterns and materials (1.21.5). Other `namespace:path` ids are taken as they are (a pattern a
/// datapack or the Pumpkin fork registered).
pub const TRIM_PATTERNS: [&str; 18] = [
    "sentry", "dune", "coast", "wild", "ward", "eye", "vex", "tide", "snout", "rib", "spire", "wayfinder", "shaper", "silence", "raiser", "host", "flow", "bolt",
];
pub const TRIM_MATERIALS: [&str; 11] = ["quartz", "iron", "netherite", "redstone", "copper", "gold", "emerald", "diamond", "lapis", "amethyst", "resin"];
/// How long a set effect lasts each time it is given (seconds): given again every second while the set is worn, and
/// long enough that night vision doesn't flicker.
pub const SET_EFFECT_SECONDS: u32 = 15;

/// How an item looks, from the studio's assets (`studio/assets/textures/<texture>.png` or `models/<model>.json`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    pub texture: String,
    pub model: String,
    /// Held like a tool (`item/handheld`) instead of flat (`item/generated`). `None` = guess from the material.
    pub handheld: Option<bool>,
    /// Textures shown while the item is being used (drawn back), first to last.
    pub pulling: Vec<String>,
}

impl Look {
    pub fn is_model(&self) -> bool {
        !self.model.is_empty()
    }
}

/// The item's look, or `None` when it has none (or names nothing).
pub fn look(data: &Data) -> Option<Look> {
    let Some(Value::Object(look)) = data.get("look") else { return None };
    let texture = fields::text(look, "texture", "").trim().to_string();
    let model = fields::text(look, "model", "").trim().to_string();
    if texture.is_empty() && model.is_empty() {
        return None;
    }
    let pulling = fields::lines(look, "pulling").into_iter().map(|one| one.trim().to_string()).filter(|one| !one.is_empty()).take(MOST_FRAMES).collect();
    Some(Look { texture, model, handheld: look.get("handheld").and_then(Value::as_bool), pulling })
}

/// Tools and weapons are held like one when the item doesn't say.
pub fn handheld_material(material: &str) -> bool {
    let bare = material.trim().trim_start_matches("minecraft:");
    ["_sword", "_axe", "_pickaxe", "_shovel", "_hoe", "_spear"].iter().any(|end| bare.ends_with(end)) || matches!(bare, "stick" | "blaze_rod" | "breeze_rod" | "bone" | "fishing_rod" | "carrot_on_a_stick" | "warped_fungus_on_a_stick" | "mace" | "trident")
}

/// The object under `key`, when there is one.
pub fn part<'a>(data: &'a Data, key: &str) -> Option<&'a Data> {
    data.get(key).and_then(Value::as_object)
}

pub fn float(data: &Data, key: &str, fallback: f64, least: f64, most: f64) -> f64 {
    data.get(key).and_then(Value::as_f64).filter(|one| one.is_finite()).unwrap_or(fallback).clamp(least, most)
}

/// A tool's own rule for some blocks: vanilla names (`minecraft:obsidian`) or studio blocks (`studio:<id>`).
#[derive(Clone, Debug, PartialEq)]
pub struct ToolRule {
    pub blocks: Vec<String>,
    pub speed: f64,
    /// Breaking those blocks with it drops their loot (counts as the right tool).
    pub drops: bool,
}

pub const MOST_TOOL_RULES: usize = 16;

fn block_name(raw: &str) -> String {
    let clean = raw.trim().to_ascii_lowercase();
    if clean.contains(':') { clean } else { format!("minecraft:{clean}") }
}

pub fn tool_rules(data: &Data) -> Vec<ToolRule> {
    let Some(Value::Array(list)) = part(data, "tool").and_then(|tool| tool.get("rules")) else { return Vec::new() };
    list.iter()
        .filter_map(Value::as_object)
        .take(MOST_TOOL_RULES)
        .filter_map(|one| {
            let blocks: Vec<String> = fields::lines(one, "blocks").iter().filter(|name| !name.trim().is_empty()).map(|name| block_name(name)).collect();
            (!blocks.is_empty()).then(|| ToolRule { blocks, speed: float(one, "speed", 6.0, 0.0, 1000.0), drops: one.get("drops").and_then(Value::as_bool) != Some(false) })
        })
        .collect()
}

/// How a studio item digs one of our blocks (`id`, which wants `block_tool`): (speed, counts as the right tool), from
/// its own rule naming `studio:<id>`, else its `tool.mines` holding that tool kind. `None` = like any other item.
pub fn digs_studio_block(item: &Data, id: &str, block_tool: &str) -> Option<(f64, bool)> {
    let name = format!("studio:{id}");
    if let Some(rule) = tool_rules(item).into_iter().find(|rule| rule.blocks.contains(&name)) {
        return Some((rule.speed, rule.drops));
    }
    let tool = part(item, "tool")?;
    let kind = block_tool.trim();
    (!kind.is_empty() && fields::lines(tool, "mines").iter().any(|one| one.trim() == kind)).then(|| (float(tool, "speed", 6.0, 0.0, 1000.0), true))
}

/// One effect a full armor set gives.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SetEffect {
    /// Without `minecraft:`.
    pub effect: String,
    /// 1 = the first level.
    pub level: u32,
}

/// An armor set: every studio item with the same `equip.set`, and the effects their pieces list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArmorSet {
    pub pieces: BTreeSet<String>,
    pub effects: Vec<SetEffect>,
}

impl ArmorSet {
    /// Whether `worn` (the studio item ids in the armor slots) holds every piece.
    pub fn complete(&self, worn: &[String]) -> bool {
        !self.pieces.is_empty() && self.pieces.iter().all(|piece| worn.contains(piece))
    }
}

/// The set an item belongs to, when it says one.
pub fn set_name(data: &Data) -> Option<String> {
    let name = fields::text(part(data, "equip")?, "set", "").trim().to_string();
    (!name.is_empty()).then_some(name)
}

fn set_effects(equip: &Data) -> Vec<SetEffect> {
    let Some(Value::Array(list)) = equip.get("setEffects") else { return Vec::new() };
    list.iter()
        .filter_map(Value::as_object)
        .filter_map(|one| {
            let effect = fields::text(one, "effect", "").trim().trim_start_matches("minecraft:").to_ascii_lowercase();
            (!effect.is_empty()).then(|| SetEffect { effect, level: fields::number(one, "level", 1, 1, 255) as u32 })
        })
        .collect()
}

/// Every armor set among enabled items: set name → its pieces and effects (each effect once, the highest level).
pub fn armor_sets<'a>(items: impl IntoIterator<Item = (&'a str, &'a Data)>) -> BTreeMap<String, ArmorSet> {
    let mut out: BTreeMap<String, ArmorSet> = BTreeMap::new();
    for (id, data) in items {
        let (Some(name), Some(equip)) = (set_name(data), part(data, "equip")) else { continue };
        let set = out.entry(name).or_default();
        set.pieces.insert(id.to_string());
        for effect in set_effects(equip) {
            match set.effects.iter_mut().find(|one| one.effect == effect.effect) {
                Some(have) => have.level = have.level.max(effect.level),
                None => set.effects.push(effect),
            }
        }
    }
    for set in out.values_mut() {
        set.effects.sort();
    }
    out
}

/// An armor trim on the item: pattern and material, each `namespace:path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trim {
    pub pattern: String,
    pub material: String,
}

fn namespaced(name: &str) -> String {
    let clean = name.trim().to_ascii_lowercase();
    if clean.contains(':') { clean } else { format!("minecraft:{clean}") }
}

pub fn trim(data: &Data) -> Option<Trim> {
    let trim = part(part(data, "equip")?, "trim")?;
    let pattern = fields::text(trim, "pattern", "");
    let material = fields::text(trim, "material", "");
    (!pattern.trim().is_empty() && !material.trim().is_empty()).then(|| Trim { pattern: namespaced(&pattern), material: namespaced(&material) })
}

/// What is wrong with the M1 fields, for the file's status. Empty = fine.
/// The studio song (`song` kind) the item plays in a jukebox, as `monolab:<id>`.
pub fn disc(data: &Data) -> Option<String> {
    let id = fields::text(data, "disc", "").trim().to_string();
    crate::valid_id(&id).then(|| format!("monolab:{id}"))
}

pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    let disc = fields::text(data, "disc", "");
    if !disc.trim().is_empty() && !crate::valid_id(disc.trim()) {
        out.push("\"disc\" is a studio song's file name (a-z, 0-9, - and _).".into());
    }
    if let Some(Value::Object(look)) = data.get("look") {
        let texture = fields::text(look, "texture", "");
        let model = fields::text(look, "model", "");
        if !texture.trim().is_empty() && !model.trim().is_empty() {
            out.push("\"look\" takes a texture or a model, not both.".into());
        }
        for name in [texture.trim().to_string(), model.trim().to_string()].into_iter().chain(fields::lines(look, "pulling")) {
            if !name.trim().is_empty() && !crate::valid_id(name.trim()) {
                out.push(format!("\"look\": {name} isn't an asset name (a-z, 0-9, - and _)."));
            }
        }
        if fields::lines(look, "pulling").len() > MOST_FRAMES {
            out.push(format!("\"look.pulling\" takes up to {MOST_FRAMES} pictures."));
        }
    } else if data.get("look").is_some_and(|one| !one.is_null()) {
        out.push("\"look\" must be an object with a texture or a model.".into());
    }
    if let Some(rarity) = data.get("rarity").and_then(Value::as_str) {
        if !rarity.is_empty() && !RARITIES.contains(&rarity) {
            out.push(format!("\"rarity\" must be one of: {}.", RARITIES.join(", ")));
        }
    }
    if let Some(equip) = part(data, "equip") {
        let slot = fields::text(equip, "slot", "");
        if !EQUIP_SLOTS.contains(&slot.as_str()) {
            out.push(format!("\"equip.slot\" must be one of: {}.", EQUIP_SLOTS.join(", ")));
        }
        if let Some(trim) = part(equip, "trim") {
            for (key, known) in [("pattern", &TRIM_PATTERNS[..]), ("material", &TRIM_MATERIALS[..])] {
                let name = fields::text(trim, key, "").trim().to_ascii_lowercase();
                if name.is_empty() {
                    out.push(format!("\"equip.trim\" needs a {key}."));
                } else if !name.contains(':') && !known.contains(&name.as_str()) {
                    out.push(format!("\"equip.trim.{key}\" isn't a vanilla one: {}; another needs its namespace (like mypack:{name}).", known.join(", ")));
                }
            }
        }
    }
    if let Some(tool) = part(data, "tool") {
        for kind in fields::lines(tool, "mines") {
            if !TOOL_KINDS.contains(&kind.trim()) {
                out.push(format!("\"tool.mines\" takes: {}.", TOOL_KINDS.join(", ")));
                break;
            }
        }
        if let Some(Value::Array(rules)) = tool.get("rules") {
            for rule in rules.iter().filter_map(Value::as_object) {
                for name in fields::lines(rule, "blocks").iter().map(|one| block_name(one)) {
                    let (space, path) = name.split_once(':').unwrap_or(("minecraft", &name));
                    if !matches!(space, "minecraft" | "studio") || path.is_empty() || !path.bytes().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == b'_' || one == b'-' || one == b'/' || one == b'.') {
                        out.push(format!("\"tool.rules\": {name} isn't a block name (like obsidian, or studio:ruby_ore)."));
                    }
                }
            }
        }
    }
    if let Some(food) = part(data, "food") {
        let animation = fields::text(food, "animation", "eat");
        if !FOOD_ANIMATIONS.contains(&animation.as_str()) {
            out.push(format!("\"food.animation\" must be one of: {}.", FOOD_ANIMATIONS.join(", ")));
        }
    }
    if fields::number(data, "durability", 0, 0, i64::MAX) > 0 && fields::number(data, "maxStack", 1, 1, 99) > 1 {
        out.push("An item with durability stacks to 1; \"maxStack\" is ignored.".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data(value: Value) -> Data {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn reads_the_look() {
        assert_eq!(look(&data(json!({}))), None);
        assert_eq!(look(&data(json!({ "look": { "texture": " " } }))), None);
        let one = look(&data(json!({ "look": { "texture": "ruby", "pulling": ["a", "b", "c", "d"], "handheld": true } }))).unwrap();
        assert_eq!(one.texture, "ruby");
        assert_eq!(one.pulling, vec!["a", "b", "c"]);
        assert_eq!(one.handheld, Some(true));
        assert!(look(&data(json!({ "look": { "model": "gem" } }))).unwrap().is_model());
        assert!(handheld_material("minecraft:diamond_sword"));
        assert!(!handheld_material("paper"));
    }

    #[test]
    fn tool_rules_for_blocks() {
        let pick = data(json!({ "tool": { "mines": ["pickaxe"], "speed": 9, "rules": [{ "blocks": ["Obsidian", "studio:ruby_ore"], "speed": 30 }, { "blocks": [] }] } }));
        let rules = tool_rules(&pick);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].blocks, vec!["minecraft:obsidian", "studio:ruby_ore"]);
        assert!(rules[0].drops);
        assert_eq!(digs_studio_block(&pick, "ruby_ore", "pickaxe"), Some((30.0, true)));
        assert_eq!(digs_studio_block(&pick, "marble", "pickaxe"), Some((9.0, true)));
        assert_eq!(digs_studio_block(&pick, "log", "axe"), None);
        assert!(problems(&pick).is_empty());
        assert_eq!(problems(&data(json!({ "tool": { "rules": [{ "blocks": ["other:thing"] }] } }))).len(), 1);
    }

    #[test]
    fn armor_sets_and_trims() {
        let helmet = data(json!({ "equip": { "slot": "head", "set": "ruby", "setEffects": [{ "effect": "minecraft:speed", "level": 1 }, { "effect": "haste" }], "trim": { "pattern": "sentry", "material": "gold" } } }));
        let boots = data(json!({ "equip": { "slot": "feet", "set": " ruby ", "setEffects": [{ "effect": "speed", "level": 2 }] } }));
        let other = data(json!({ "equip": { "slot": "chest" } }));
        let sets = armor_sets([("helmet", &helmet), ("boots", &boots), ("shirt", &other)]);
        assert_eq!(sets.len(), 1);
        let ruby = &sets["ruby"];
        assert_eq!(ruby.pieces.len(), 2);
        assert_eq!(ruby.effects, vec![SetEffect { effect: "haste".into(), level: 1 }, SetEffect { effect: "speed".into(), level: 2 }]);
        assert!(ruby.complete(&["boots".into(), "helmet".into(), "x".into()]));
        assert!(!ruby.complete(&["boots".into()]));
        assert_eq!(trim(&helmet), Some(Trim { pattern: "minecraft:sentry".into(), material: "minecraft:gold".into() }));
        assert_eq!(trim(&boots), None);
        assert!(problems(&helmet).is_empty());
        assert_eq!(problems(&data(json!({ "equip": { "slot": "head", "trim": { "pattern": "nope", "material": "gold" } } }))).len(), 1);
        assert!(problems(&data(json!({ "equip": { "slot": "head", "trim": { "pattern": "mine:star", "material": "gold" } } }))).is_empty());
    }

    #[test]
    fn finds_problems() {
        assert!(problems(&data(json!({ "look": { "texture": "ruby" }, "rarity": "rare", "equip": { "slot": "head" } }))).is_empty());
        assert_eq!(problems(&data(json!({ "look": { "texture": "a", "model": "b" } }))).len(), 1);
        assert_eq!(problems(&data(json!({ "look": { "texture": "Bad Name" } }))).len(), 1);
        assert_eq!(problems(&data(json!({ "rarity": "legendary" }))).len(), 1);
        assert_eq!(problems(&data(json!({ "equip": { "slot": "hand" } }))).len(), 1);
        assert_eq!(problems(&data(json!({ "tool": { "mines": ["sword"] } }))).len(), 1);
        assert_eq!(problems(&data(json!({ "durability": 100, "maxStack": 16 }))).len(), 1);
    }
}

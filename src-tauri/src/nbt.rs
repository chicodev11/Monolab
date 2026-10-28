
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub uuid: String,
    pub dimension: String,
    pub position: [f64; 3],
    pub health: f32,
    pub food: i32,
    pub experience_level: i32,
    pub game_mode: String,
    pub inventory: Vec<InventoryItem>,
    pub ender_items: Vec<InventoryItem>,
    pub modified_at: u64,
    pub live: bool,
}

impl PlayerSnapshot {
    pub fn blank(uuid: &str) -> Self {
        PlayerSnapshot {
            uuid: uuid.to_string(),
            dimension: String::new(),
            position: [0.0; 3],
            health: 0.0,
            food: 0,
            experience_level: 0,
            game_mode: String::new(),
            inventory: Vec::new(),
            ender_items: Vec::new(),
            modified_at: 0,
            live: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItem {
    pub id: String,
    pub count: i64,
    pub slot: i64,
    pub name: String,
    pub damage: i64,
    pub max_damage: i64,
    pub enchantments: Vec<Enchantment>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Enchantment {
    pub id: String,
    pub level: i64,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorldSettings {
    pub level_name: String,
    pub seed: String,
    pub difficulty: u8,
    pub hardcore: bool,
    pub spawn: [i32; 3],
    pub last_played: i64,
    pub game_rules: Vec<GameRule>,
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GameRule {
    pub name: String,
    pub value: String,
}

fn read_gzip_nbt(path: &Path) -> Result<fastnbt::Value, String> {
    let raw = fs::read(path).map_err(|error| format!("Could not read the file: {error}"))?;
    decode_nbt(raw)
}

fn decode_nbt(raw: Vec<u8>) -> Result<fastnbt::Value, String> {
    let mut decoded = Vec::new();
    if GzDecoder::new(&raw[..]).read_to_end(&mut decoded).is_err() {
        decoded = raw;
    }
    fastnbt::from_bytes(&decoded).map_err(|error| format!("Could not read NBT: {error}"))
}

fn write_gzip_nbt(path: &Path, value: &fastnbt::Value) -> Result<(), String> {
    let bytes = fastnbt::to_bytes(value)
        .map_err(|error| format!("Could not write NBT: {error}"))?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    let compressed = encoder.finish().map_err(|error| error.to_string())?;
    fs::write(path, compressed).map_err(|error| format!("Could not save the file: {error}"))
}

fn compound(value: &fastnbt::Value) -> Option<&std::collections::HashMap<String, fastnbt::Value>> {
    match value {
        fastnbt::Value::Compound(map) => Some(map),
        _ => None,
    }
}

fn as_f64(value: Option<&fastnbt::Value>) -> f64 {
    match value {
        Some(fastnbt::Value::Double(number)) => *number,
        Some(fastnbt::Value::Float(number)) => f64::from(*number),
        Some(fastnbt::Value::Int(number)) => f64::from(*number),
        Some(fastnbt::Value::Long(number)) => *number as f64,
        _ => 0.0,
    }
}

fn as_i64(value: Option<&fastnbt::Value>) -> i64 {
    match value {
        Some(fastnbt::Value::Byte(number)) => i64::from(*number),
        Some(fastnbt::Value::Short(number)) => i64::from(*number),
        Some(fastnbt::Value::Int(number)) => i64::from(*number),
        Some(fastnbt::Value::Long(number)) => *number,
        Some(fastnbt::Value::Double(number)) => *number as i64,
        Some(fastnbt::Value::Float(number)) => *number as i64,
        _ => 0,
    }
}

fn as_string(value: Option<&fastnbt::Value>) -> String {
    match value {
        Some(fastnbt::Value::String(text)) => text.clone(),
        Some(other) => format!("{other:?}"),
        None => String::new(),
    }
}

fn game_mode_label(mode: i64) -> String {
    match mode {
        0 => "Survival",
        1 => "Creative",
        2 => "Adventure",
        3 => "Spectator",
        _ => "Unknown",
    }
    .into()
}

pub fn read_player_data(path: &Path, uuid: &str) -> Result<PlayerSnapshot, String> {
    let mut snapshot = snapshot_of(read_gzip_nbt(path)?, uuid)?;
    snapshot.modified_at = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
        .unwrap_or(0);
    Ok(snapshot)
}

pub fn read_player_bytes(raw: Vec<u8>, uuid: &str) -> Result<PlayerSnapshot, String> {
    snapshot_of(decode_nbt(raw)?, uuid)
}

fn snapshot_of(value: fastnbt::Value, uuid: &str) -> Result<PlayerSnapshot, String> {
    let root = compound(&value).ok_or("The player data is malformed.")?;
    let position = match root.get("Pos") {
        Some(fastnbt::Value::List(items)) if items.len() >= 3 => [
            as_f64(items.first()),
            as_f64(items.get(1)),
            as_f64(items.get(2)),
        ],
        _ => [0.0, 0.0, 0.0],
    };
    let health = match root.get("Health") {
        Some(fastnbt::Value::Float(number)) => *number,
        other => as_f64(other) as f32,
    };
    let mut inventory = items_in(root.get("Inventory"));
    if let Some(equipment) = root.get("equipment").and_then(compound) {
        for (key, slot) in [("head", 103), ("chest", 102), ("legs", 101), ("feet", 100), ("offhand", -106)] {
            let Some(entry) = equipment.get(key).and_then(compound) else { continue };
            if inventory.iter().any(|item| item.slot == slot) {
                continue;
            }
            let Some(mut item) = item_of(entry, slot) else { continue };
            item.count = item.count.max(1);
            inventory.push(item);
        }
    }
    let ender_items = items_in(root.get("EnderItems"));
    let modified_at = 0;
    Ok(PlayerSnapshot {
        uuid: uuid.to_string(),
        dimension: as_string(root.get("Dimension")),
        position,
        health,
        food: as_i64(root.get("foodLevel")) as i32,
        experience_level: as_i64(root.get("XpLevel")) as i32,
        game_mode: game_mode_label(as_i64(root.get("playerGameType"))),
        inventory,
        ender_items,
        modified_at,
        live: false,
    })
}

type Map = std::collections::HashMap<String, fastnbt::Value>;

fn items_in(list: Option<&fastnbt::Value>) -> Vec<InventoryItem> {
    match list {
        Some(fastnbt::Value::List(items)) => items
            .iter()
            .filter_map(|item| {
                let entry = compound(item)?;
                item_of(entry, as_i64(entry.get("Slot")))
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn item_of(entry: &Map, slot: i64) -> Option<InventoryItem> {
    let id = as_string(entry.get("id"));
    if id.is_empty() {
        return None;
    }
    let parts = entry.get("components").and_then(compound);
    let tag = entry.get("tag").and_then(compound);
    let part = |key: &str| parts.and_then(|map| map.get(key));
    let damage = part("minecraft:damage").or_else(|| tag.and_then(|map| map.get("Damage")));
    let name = part("minecraft:custom_name").or_else(|| {
        tag.and_then(|map| map.get("display"))
            .and_then(compound)
            .and_then(|display| display.get("Name"))
    });
    let mut enchantments = Vec::new();
    for key in ["minecraft:enchantments", "minecraft:stored_enchantments"] {
        enchantments.extend(enchantments_of(part(key)));
    }
    for key in ["Enchantments", "StoredEnchantments"] {
        enchantments.extend(enchantments_of(tag.and_then(|map| map.get(key))));
    }
    Some(InventoryItem {
        id,
        count: as_i64(entry.get("count").or_else(|| entry.get("Count"))),
        slot,
        name: name.map(text_of).unwrap_or_default().trim().to_string(),
        damage: as_i64(damage).max(0),
        max_damage: as_i64(part("minecraft:max_damage")).max(0),
        enchantments,
    })
}

fn enchantments_of(value: Option<&fastnbt::Value>) -> Vec<Enchantment> {
    let mut found: Vec<Enchantment> = match value {
        Some(fastnbt::Value::Compound(map)) => {
            let levels = map.get("levels").and_then(compound).unwrap_or(map);
            levels
                .iter()
                .filter(|(_, level)| !matches!(level, fastnbt::Value::Compound(_) | fastnbt::Value::List(_)))
                .map(|(id, level)| Enchantment { id: id.clone(), level: as_i64(Some(level)) })
                .collect()
        }
        Some(fastnbt::Value::List(items)) => items
            .iter()
            .filter_map(|item| {
                let entry = compound(item)?;
                let Some(fastnbt::Value::String(id)) = entry.get("id") else { return None };
                Some(Enchantment { id: id.clone(), level: as_i64(entry.get("lvl")) })
            })
            .collect(),
        _ => Vec::new(),
    };
    found.retain(|one| one.level > 0 && one.id != "show_in_tooltip");
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

fn text_of(value: &fastnbt::Value) -> String {
    match value {
        fastnbt::Value::String(text) => {
            let trimmed = text.trim_start();
            if trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed.starts_with('"') {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    return json_text(&json);
                }
            }
            text.clone()
        }
        fastnbt::Value::Compound(map) => {
            let mut out = match map.get("text") {
                Some(fastnbt::Value::String(text)) => text.clone(),
                _ => match map.get("translate") {
                    Some(fastnbt::Value::String(key)) => key.clone(),
                    _ => String::new(),
                },
            };
            if let Some(fastnbt::Value::List(extra)) = map.get("extra") {
                out.extend(extra.iter().map(text_of));
            }
            out
        }
        fastnbt::Value::List(parts) => parts.iter().map(text_of).collect(),
        _ => String::new(),
    }
}

fn json_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(parts) => parts.iter().map(json_text).collect(),
        serde_json::Value::Object(map) => {
            let mut out = map
                .get("text")
                .and_then(|text| text.as_str())
                .or_else(|| map.get("translate").and_then(|key| key.as_str()))
                .unwrap_or_default()
                .to_string();
            if let Some(serde_json::Value::Array(extra)) = map.get("extra") {
                out.extend(extra.iter().map(json_text));
            }
            out
        }
        _ => String::new(),
    }
}

fn overworld_data_dir(level_dat: &Path) -> Option<std::path::PathBuf> {
    level_dat
        .parent()
        .map(|world| world.join("dimensions").join("minecraft").join("overworld").join("data").join("minecraft"))
}

fn is_new_layout(data: &std::collections::HashMap<String, fastnbt::Value>) -> bool {
    data.get("difficulty_settings").and_then(compound).is_some()
}

fn difficulty_from_name(name: &str) -> u8 {
    match name {
        "peaceful" => 0,
        "easy" => 1,
        "normal" => 2,
        "hard" => 3,
        _ => 2,
    }
}

fn difficulty_name(level: u8) -> &'static str {
    match level {
        0 => "peaceful",
        1 => "easy",
        2 => "normal",
        _ => "hard",
    }
}

fn rule_value_text(value: &fastnbt::Value) -> String {
    match value {
        fastnbt::Value::Byte(bit) => if *bit != 0 { "true".into() } else { "false".into() },
        fastnbt::Value::String(text) => text.clone(),
        other => as_i64(Some(other)).to_string(),
    }
}

pub fn read_level_data(path: &Path) -> Result<WorldSettings, String> {
    let value = read_gzip_nbt(path)?;
    let root = compound(&value).ok_or("level.dat is malformed.")?;
    let data = root
        .get("Data")
        .and_then(compound)
        .ok_or("level.dat has no Data entry.")?;
    let mut game_rules = data
        .get("GameRules")
        .and_then(compound)
        .map(|rules| {
            rules
                .iter()
                .map(|(name, value)| GameRule {
                    name: name.clone(),
                    value: rule_value_text(value),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut seed = data
        .get("WorldGenSettings")
        .and_then(compound)
        .map(|settings| as_i64(settings.get("seed")))
        .filter(|seed| *seed != 0)
        .unwrap_or_else(|| as_i64(data.get("RandomSeed")));
    let (difficulty, spawn) = if is_new_layout(data) {
        let name = data
            .get("difficulty_settings")
            .and_then(compound)
            .map(|settings| as_string(settings.get("difficulty")))
            .unwrap_or_default();
        let pos = data
            .get("spawn")
            .and_then(compound)
            .and_then(|spawn| match spawn.get("pos") {
                Some(fastnbt::Value::IntArray(array)) if array.len() >= 3 => {
                    Some([array[0], array[1], array[2]])
                }
                _ => None,
            })
            .unwrap_or([0, 0, 0]);
        if let Some(dir) = overworld_data_dir(path) {
            let rules_file = dir.join("game_rules.dat");
            if rules_file.is_file() {
                let rules = read_gzip_nbt(&rules_file)?;
                if let Some(map) = compound(&rules).and_then(|root| root.get("data")).and_then(compound) {
                    game_rules = map
                        .iter()
                        .map(|(name, value)| GameRule {
                            name: name.clone(),
                            value: rule_value_text(value),
                        })
                        .collect();
                }
            }
            let gen_file = dir.join("world_gen_settings.dat");
            if seed == 0 && gen_file.is_file() {
                if let Ok(gen) = read_gzip_nbt(&gen_file) {
                    seed = compound(&gen)
                        .and_then(|root| root.get("data"))
                        .and_then(compound)
                        .map(|settings| as_i64(settings.get("seed")))
                        .unwrap_or(0);
                }
            }
        }
        (difficulty_from_name(&name), pos)
    } else {
        (
            as_i64(data.get("Difficulty")) as u8,
            [
                as_i64(data.get("SpawnX")) as i32,
                as_i64(data.get("SpawnY")) as i32,
                as_i64(data.get("SpawnZ")) as i32,
            ],
        )
    };
    game_rules.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(WorldSettings {
        level_name: as_string(data.get("LevelName")),
        seed: seed.to_string(),
        difficulty,
        hardcore: as_i64(data.get("hardcore")) != 0,
        spawn,
        last_played: as_i64(data.get("LastPlayed")),
        game_rules,
    })
}

pub fn file_hash(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub fn write_level_data(
    path: &Path,
    difficulty: Option<u8>,
    hardcore: Option<bool>,
    spawn: Option<[i32; 3]>,
    rules: &[GameRule],
) -> Result<String, String> {
    let original_hash = file_hash(path)?;
    let mut value = read_gzip_nbt(path)?;
    let fastnbt::Value::Compound(root) = &mut value else {
        return Err("level.dat is malformed.".into());
    };
    let Some(fastnbt::Value::Compound(data)) = root.get_mut("Data") else {
        return Err("level.dat has no Data entry.".into());
    };
    let new_layout = is_new_layout(data);
    if let Some(difficulty) = difficulty {
        if difficulty > 3 {
            return Err("Difficulty must be between 0 (peaceful) and 3 (hard).".into());
        }
        if new_layout {
            if let Some(fastnbt::Value::Compound(settings)) = data.get_mut("difficulty_settings") {
                settings.insert(
                    "difficulty".into(),
                    fastnbt::Value::String(difficulty_name(difficulty).into()),
                );
            }
        } else {
            data.insert("Difficulty".into(), fastnbt::Value::Byte(difficulty as i8));
        }
    }
    if let Some(hardcore) = hardcore {
        data.insert(
            "hardcore".into(),
            fastnbt::Value::Byte(if hardcore { 1 } else { 0 }),
        );
    }
    if let Some([x, y, z]) = spawn {
        if new_layout {
            if let Some(fastnbt::Value::Compound(point)) = data.get_mut("spawn") {
                point.insert("pos".into(), fastnbt::Value::IntArray(fastnbt::IntArray::new(vec![x, y, z])));
            }
        } else {
            data.insert("SpawnX".into(), fastnbt::Value::Int(x));
            data.insert("SpawnY".into(), fastnbt::Value::Int(y));
            data.insert("SpawnZ".into(), fastnbt::Value::Int(z));
        }
    }
    if !rules.is_empty() {
        if new_layout {
            let rules_file = overworld_data_dir(path)
                .map(|dir| dir.join("game_rules.dat"))
                .filter(|file| file.is_file())
                .ok_or("The game rules file (game_rules.dat) could not be found.")?;
            let mut rules_value = read_gzip_nbt(&rules_file)?;
            let Some(fastnbt::Value::Compound(existing)) = compound_mut(&mut rules_value).and_then(|root| root.get_mut("data")) else {
                return Err("The game rules file is malformed.".into());
            };
            for rule in rules {
                let Some(current) = existing.get(&rule.name) else {
                    return Err(format!("Unknown game rule: {}", rule.name));
                };
                let next = match current {
                    fastnbt::Value::Byte(_) => match rule.value.trim() {
                        "true" | "1" => fastnbt::Value::Byte(1),
                        "false" | "0" => fastnbt::Value::Byte(0),
                        _ => return Err(format!("{} must be true or false.", rule.name)),
                    },
                    fastnbt::Value::Int(_) => fastnbt::Value::Int(
                        rule.value
                            .trim()
                            .parse::<i32>()
                            .map_err(|_| format!("{} accepts numbers only.", rule.name))?,
                    ),
                    fastnbt::Value::Long(_) => fastnbt::Value::Long(
                        rule.value
                            .trim()
                            .parse::<i64>()
                            .map_err(|_| format!("{} accepts numbers only.", rule.name))?,
                    ),
                    fastnbt::Value::String(_) => fastnbt::Value::String(rule.value.clone()),
                    _ => return Err(format!("{} is a type the app cannot change.", rule.name)),
                };
                existing.insert(rule.name.clone(), next);
            }
            write_gzip_nbt(&rules_file, &rules_value)?;
        } else {
            let Some(fastnbt::Value::Compound(existing)) = data.get_mut("GameRules") else {
                return Err("level.dat has no game rules entry.".into());
            };
            for rule in rules {
                if !existing.contains_key(&rule.name) {
                    return Err(format!("Unknown game rule: {}", rule.name));
                }
                existing.insert(
                    rule.name.clone(),
                    fastnbt::Value::String(rule.value.clone()),
                );
            }
        }
    }
    write_gzip_nbt(path, &value)?;
    Ok(original_hash)
}

/// Change one stack's count in a player file (0 removes it). Returns the new gzip bytes; the caller saves them.
pub fn edit_player_item(raw: Vec<u8>, ender: bool, slot: i64, count: i64) -> Result<Vec<u8>, String> {
    let mut value = decode_nbt(raw)?;
    let root = compound_mut(&mut value).ok_or("The player data is malformed.")?;
    let count = count.clamp(0, 99);
    let set_count = |entry: &mut Map| {
        if let Some(old) = entry.get_mut("Count") {
            *old = fastnbt::Value::Byte(count as i8);
        } else {
            entry.insert("count".into(), fastnbt::Value::Int(count as i32));
        }
    };
    let mut done = false;
    if let Some(fastnbt::Value::List(items)) = root.get_mut(if ender { "EnderItems" } else { "Inventory" }) {
        if let Some(at) = items.iter().position(|item| compound(item).is_some_and(|entry| as_i64(entry.get("Slot")) == slot)) {
            if count == 0 {
                items.remove(at);
            } else if let Some(entry) = compound_mut(&mut items[at]) {
                set_count(entry);
            }
            done = true;
        }
    }
    if !done && !ender {
        let key = match slot {
            103 => "head",
            102 => "chest",
            101 => "legs",
            100 => "feet",
            -106 => "offhand",
            _ => "",
        };
        if let Some(equipment) = root.get_mut("equipment").and_then(compound_mut) {
            if equipment.contains_key(key) {
                if count == 0 {
                    equipment.remove(key);
                } else if let Some(entry) = equipment.get_mut(key).and_then(compound_mut) {
                    set_count(entry);
                }
                done = true;
            }
        }
    }
    if !done {
        return Err("That slot is already empty.".into());
    }
    let bytes = fastnbt::to_bytes(&value).map_err(|error| format!("Could not write NBT: {error}"))?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&bytes).map_err(|error| error.to_string())?;
    encoder.finish().map_err(|error| error.to_string())
}

fn compound_mut(value: &mut fastnbt::Value) -> Option<&mut std::collections::HashMap<String, fastnbt::Value>> {
    match value {
        fastnbt::Value::Compound(map) => Some(map),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::game_mode_label;

    #[test]
    fn reads_armor_and_offhand_from_the_equipment_compound() {
        use fastnbt::Value;
        use std::collections::HashMap;
        let item = |id: &str, count: Option<i32>| {
            let mut map = HashMap::new();
            map.insert("id".to_string(), Value::String(id.to_string()));
            if let Some(count) = count {
                map.insert("count".to_string(), Value::Int(count));
            }
            Value::Compound(map)
        };
        let mut slot0 = match item("minecraft:stone", Some(5)) { Value::Compound(map) => map, _ => unreachable!() };
        slot0.insert("Slot".to_string(), Value::Byte(0));
        let mut equipment = HashMap::new();
        equipment.insert("head".to_string(), item("minecraft:diamond_helmet", None));
        equipment.insert("feet".to_string(), item("minecraft:iron_boots", Some(1)));
        equipment.insert("offhand".to_string(), item("minecraft:shield", Some(1)));
        let mut root = HashMap::new();
        root.insert("Inventory".to_string(), Value::List(vec![Value::Compound(slot0)]));
        root.insert("equipment".to_string(), Value::Compound(equipment));
        let snapshot = super::snapshot_of(Value::Compound(root), "u").unwrap();
        let at = |slot: i64| snapshot.inventory.iter().find(|one| one.slot == slot).map(|one| (one.id.as_str(), one.count));
        assert_eq!(at(0), Some(("minecraft:stone", 5)));
        assert_eq!(at(103), Some(("minecraft:diamond_helmet", 1)));
        assert_eq!(at(100), Some(("minecraft:iron_boots", 1)));
        assert_eq!(at(-106), Some(("minecraft:shield", 1)));
        assert_eq!(at(102), None);
    }

    #[test]
    fn edits_and_removes_stacks_in_a_player_file() {
        use fastnbt::Value;
        use std::collections::HashMap;
        let stack = |slot: i8, id: &str, count: Value, key: &str| {
            let mut map = HashMap::new();
            map.insert("Slot".to_string(), Value::Byte(slot));
            map.insert("id".to_string(), Value::String(id.into()));
            map.insert(key.to_string(), count);
            Value::Compound(map)
        };
        let mut helmet = HashMap::new();
        helmet.insert("id".to_string(), Value::String("minecraft:iron_helmet".into()));
        let mut equipment = HashMap::new();
        equipment.insert("head".to_string(), Value::Compound(helmet));
        let mut root = HashMap::new();
        root.insert("Inventory".to_string(), Value::List(vec![stack(0, "minecraft:stone", Value::Int(5), "count"), stack(1, "minecraft:dirt", Value::Byte(9), "Count")]));
        root.insert("EnderItems".to_string(), Value::List(vec![stack(3, "minecraft:diamond", Value::Int(2), "count")]));
        root.insert("equipment".to_string(), Value::Compound(equipment));
        let raw = fastnbt::to_bytes(&Value::Compound(root)).unwrap();
        let raw = super::edit_player_item(raw, false, 0, 64).unwrap();
        let raw = super::edit_player_item(raw, false, 1, 0).unwrap();
        let raw = super::edit_player_item(raw, true, 3, 1).unwrap();
        let raw = super::edit_player_item(raw, false, 103, 0).unwrap();
        assert!(super::edit_player_item(raw.clone(), false, 7, 1).is_err());
        let snapshot = super::read_player_bytes(raw, "u").unwrap();
        let at = |slot: i64| snapshot.inventory.iter().find(|one| one.slot == slot).map(|one| one.count);
        assert_eq!(at(0), Some(64));
        assert_eq!(at(1), None);
        assert_eq!(at(103), None);
        assert_eq!(snapshot.ender_items[0].count, 1);
    }

    #[test]
    fn reads_item_details_in_old_and_new_shapes() {
        use fastnbt::Value;
        use std::collections::HashMap;
        let map = |pairs: Vec<(&str, Value)>| -> Value {
            Value::Compound(pairs.into_iter().map(|(key, value)| (key.to_string(), value)).collect::<HashMap<_, _>>())
        };
        let new_sword = map(vec![
            ("id", Value::String("minecraft:diamond_sword".into())),
            ("count", Value::Int(1)),
            ("Slot", Value::Byte(0)),
            ("components", map(vec![
                ("minecraft:damage", Value::Int(61)),
                ("minecraft:enchantments", map(vec![
                    ("minecraft:sharpness", Value::Int(5)),
                    ("minecraft:unbreaking", Value::Int(3)),
                ])),
                ("minecraft:custom_name", map(vec![
                    ("text", Value::String("Ex".into())),
                    ("extra", Value::List(vec![Value::String("calibur".into())])),
                ])),
            ])),
        ]);
        let mid_book = map(vec![
            ("id", Value::String("minecraft:enchanted_book".into())),
            ("count", Value::Int(1)),
            ("Slot", Value::Byte(1)),
            ("components", map(vec![
                ("minecraft:stored_enchantments", map(vec![
                    ("levels", map(vec![("minecraft:mending", Value::Int(1))])),
                    ("show_in_tooltip", Value::Byte(1)),
                ])),
                ("minecraft:custom_name", Value::String(r#"{"text":"Book"}"#.into())),
            ])),
        ]);
        let old_pick = map(vec![
            ("id", Value::String("minecraft:iron_pickaxe".into())),
            ("Count", Value::Byte(1)),
            ("Slot", Value::Byte(2)),
            ("tag", map(vec![
                ("Damage", Value::Int(9)),
                ("Enchantments", Value::List(vec![map(vec![
                    ("id", Value::String("minecraft:efficiency".into())),
                    ("lvl", Value::Short(4)),
                ])])),
                ("display", map(vec![("Name", Value::String(r#"{"text":"Digger"}"#.into()))])),
            ])),
        ]);
        let pearl = map(vec![
            ("id", Value::String("minecraft:ender_pearl".into())),
            ("count", Value::Int(16)),
            ("Slot", Value::Byte(26)),
        ]);
        let root = map(vec![
            ("Inventory", Value::List(vec![new_sword, mid_book, old_pick])),
            ("EnderItems", Value::List(vec![pearl])),
        ]);
        let snapshot = super::snapshot_of(root, "u").unwrap();
        let at = |slot: i64| snapshot.inventory.iter().find(|one| one.slot == slot).unwrap();
        let sword = at(0);
        assert_eq!(sword.name, "Excalibur");
        assert_eq!(sword.damage, 61);
        let levels: Vec<_> = sword.enchantments.iter().map(|one| (one.id.as_str(), one.level)).collect();
        assert_eq!(levels, vec![("minecraft:sharpness", 5), ("minecraft:unbreaking", 3)]);
        let book = at(1);
        assert_eq!(book.name, "Book");
        assert_eq!(book.enchantments.len(), 1);
        assert_eq!(book.enchantments[0].id, "minecraft:mending");
        let pick = at(2);
        assert_eq!((pick.name.as_str(), pick.damage, pick.count), ("Digger", 9, 1));
        assert_eq!(pick.enchantments[0].level, 4);
        assert_eq!(snapshot.ender_items.len(), 1);
        assert_eq!((snapshot.ender_items[0].slot, snapshot.ender_items[0].count), (26, 16));
    }

    #[test]
    fn reads_and_writes_a_26x_world() {
        use std::collections::HashMap;
        use fastnbt::Value;
        let dir = std::env::temp_dir().join(format!("se-nbt26-{}", std::process::id()));
        let data_dir = dir.join("dimensions/minecraft/overworld/data/minecraft");
        std::fs::create_dir_all(&data_dir).unwrap();
        let level = dir.join("level.dat");
        let mut settings = HashMap::new();
        settings.insert("locked".to_string(), Value::Byte(0));
        settings.insert("difficulty".to_string(), Value::String("easy".into()));
        let mut spawn = HashMap::new();
        spawn.insert("pos".to_string(), Value::IntArray(fastnbt::IntArray::new(vec![7, 64, -3])));
        spawn.insert("yaw".to_string(), Value::Float(0.0));
        let mut data = HashMap::new();
        data.insert("difficulty_settings".to_string(), Value::Compound(settings));
        data.insert("spawn".to_string(), Value::Compound(spawn));
        data.insert("hardcore".to_string(), Value::Byte(0));
        data.insert("LevelName".to_string(), Value::String("world".into()));
        data.insert("LastPlayed".to_string(), Value::Long(5));
        let mut root = HashMap::new();
        root.insert("Data".to_string(), Value::Compound(data));
        super::write_gzip_nbt(&level, &Value::Compound(root)).unwrap();
        let mut rules = HashMap::new();
        rules.insert("minecraft:keep_inventory".to_string(), Value::Byte(0));
        rules.insert("minecraft:random_tick_speed".to_string(), Value::Int(3));
        let mut rules_root = HashMap::new();
        rules_root.insert("data".to_string(), Value::Compound(rules));
        super::write_gzip_nbt(&data_dir.join("game_rules.dat"), &Value::Compound(rules_root)).unwrap();
        let mut gen = HashMap::new();
        gen.insert("seed".to_string(), Value::Long(-42));
        let mut gen_root = HashMap::new();
        gen_root.insert("data".to_string(), Value::Compound(gen));
        super::write_gzip_nbt(&data_dir.join("world_gen_settings.dat"), &Value::Compound(gen_root)).unwrap();

        let read = super::read_level_data(&level).unwrap();
        assert_eq!(read.seed, "-42");
        assert_eq!(read.difficulty, 1);
        assert_eq!(read.spawn, [7, 64, -3]);
        let keep = read.game_rules.iter().find(|r| r.name == "minecraft:keep_inventory").unwrap();
        assert_eq!(keep.value, "false");
        let tick = read.game_rules.iter().find(|r| r.name == "minecraft:random_tick_speed").unwrap();
        assert_eq!(tick.value, "3");

        super::write_level_data(
            &level,
            Some(3),
            Some(true),
            Some([1, 2, 3]),
            &[
                super::GameRule { name: "minecraft:keep_inventory".into(), value: "true".into() },
                super::GameRule { name: "minecraft:random_tick_speed".into(), value: "20".into() },
            ],
        )
        .unwrap();
        let again = super::read_level_data(&level).unwrap();
        assert_eq!(again.difficulty, 3);
        assert!(again.hardcore);
        assert_eq!(again.spawn, [1, 2, 3]);
        assert_eq!(again.game_rules.iter().find(|r| r.name == "minecraft:keep_inventory").unwrap().value, "true");
        assert_eq!(again.game_rules.iter().find(|r| r.name == "minecraft:random_tick_speed").unwrap().value, "20");
        let bad = super::write_level_data(
            &level,
            None,
            None,
            None,
            &[super::GameRule { name: "minecraft:random_tick_speed".into(), value: "fast".into() }],
        );
        assert!(bad.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn labels_every_game_mode() {
        assert_eq!(game_mode_label(0), "Survival");
        assert_eq!(game_mode_label(1), "Creative");
        assert_eq!(game_mode_label(2), "Adventure");
        assert_eq!(game_mode_label(3), "Spectator");
        assert_eq!(game_mode_label(9), "Unknown");
    }
}

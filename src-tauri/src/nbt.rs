
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
    pub modified_at: u64,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItem {
    pub id: String,
    pub count: i64,
    pub slot: i64,
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
    let inventory = match root.get("Inventory") {
        Some(fastnbt::Value::List(items)) => items
            .iter()
            .filter_map(|item| {
                let entry = compound(item)?;
                Some(InventoryItem {
                    id: as_string(entry.get("id")),
                    count: as_i64(entry.get("count").or_else(|| entry.get("Count"))),
                    slot: as_i64(entry.get("Slot")),
                })
            })
            .filter(|item| !item.id.is_empty())
            .collect(),
        _ => Vec::new(),
    };
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
        modified_at,
    })
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

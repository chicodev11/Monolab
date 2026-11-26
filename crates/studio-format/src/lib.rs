//! The Monolab studio's file format, shared by the app (`src-tauri/src/studio.rs`) and the Pumpkin add-on
//! (`addon/pumpkin`). One resource = one JSON file, `studio/<kind>/<id>.json`. The Paper add-on is Java and
//! keeps its own copy of these rules (`Studio.java`, `StudioText.java`); keep the three in step.

pub mod advancement;
pub mod announcement;
pub mod bbmodel;
pub mod biome;
pub mod block;
pub mod canvas;
pub mod cosmetic;
pub mod enchant;
pub mod fields;
pub mod function;
pub mod furniture;
pub mod glyphs;
pub mod item;
pub mod mini;
pub mod mob;
pub mod model;
pub mod painting;
pub mod quest;
pub mod region;
pub mod shop;
pub mod song;
pub mod structure;
pub mod trade;
pub mod worldgen;

use serde_json::{Map, Value};

/// File format version. Older files are read as they are; newer ones are left alone until the add-on updates.
pub const SCHEMA: u64 = 1;
pub const KINDS: [&str; 33] = [
    "scoreboard",
    "tab",
    "motd",
    "chat",
    "message",
    "announcement",
    "bossbar",
    "hologram",
    "npc",
    "dialog",
    "command",
    "menu",
    "item",
    "recipe",
    "loot",
    "advancement",
    "rule",
    "world",
    "font",
    "model",
    "block",
    "furniture",
    "mob",
    "enchant",
    "song",
    "painting",
    "cosmetic",
    "trade",
    "function",
    "quest",
    "structure",
    "biome",
    "shop",
];
pub const MOST_BYTES: usize = 256 * 1024;
/// What the Pumpkin add-on runs: every kind (some with parts Pumpkin's plugin API can't do yet, which the add-on
/// reports in that file's status).
pub const PUMPKIN_KINDS: [&str; 33] = KINDS;
/// Boss bar colors and styles, as Minecraft names them (the add-ons map them to their own types).
pub const BOSSBAR_COLORS: [&str; 7] = ["pink", "blue", "red", "green", "yellow", "purple", "white"];
pub const BOSSBAR_STYLES: [&str; 5] = ["progress", "notched_6", "notched_10", "notched_12", "notched_20"];
/// How a hologram turns: `center` faces the player, `fixed` doesn't turn.
pub const HOLOGRAM_BILLBOARDS: [&str; 4] = ["center", "fixed", "vertical", "horizontal"];
/// Where a loot table drops from (`none`: only the `loot` action gives it; `chest`: containers filling from a
/// vanilla loot table such as `chests/simple_dungeon`; `fishing`: each catch).
pub const LOOT_SOURCES: [&str; 5] = ["none", "block", "mob", "chest", "fishing"];
/// Advancement frames, as Minecraft names them.
pub const ADVANCEMENT_FRAMES: [&str; 3] = ["task", "goal", "challenge"];
/// Dialog input kinds.
pub const DIALOG_INPUTS: [&str; 4] = ["text", "bool", "number", "choice"];
/// How a dialog lays out its body: `list` (lines of text and items) or `canvas` (pictures at pixel spots, M2).
pub const DIALOG_LAYOUTS: [&str; 2] = ["list", "canvas"];
/// How a studio world generates the first time, and which dimension it is.
pub const WORLD_TYPES: [&str; 6] = ["normal", "flat", "amplified", "large_biomes", "void", "custom"];
pub const WORLD_ENVIRONMENTS: [&str; 3] = ["normal", "nether", "end"];

pub fn valid_kind(kind: &str) -> bool {
    KINDS.contains(&kind)
}

/// File names: a-z, 0-9, `-` and `_`, up to 48.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 48 && id.bytes().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == b'-' || one == b'_')
}

/// Parses and checks one file's text the way the add-ons do. Kind-specific fields are checked by each feature.
pub fn check(id: &str, text: &str) -> Result<Map<String, Value>, String> {
    if !valid_id(id) {
        return Err("File names may only use a-z, 0-9, - and _ (up to 48).".into());
    }
    if text.len() > MOST_BYTES {
        return Err("The file is too big (over 256 KB).".into());
    }
    let value: Value = serde_json::from_str(text).map_err(|error| format!("Invalid JSON: {error}"))?;
    let Value::Object(map) = value else { return Err("The file must be a JSON object.".into()) };
    let Some(schema) = map.get("schema").and_then(Value::as_f64) else { return Err("\"schema\" is missing.".into()) };
    if schema < 1.0 || schema.fract() != 0.0 {
        return Err("\"schema\" must be a whole number from 1.".into());
    }
    if schema > SCHEMA as f64 {
        return Err(format!("Made for a newer Monolab add-on (schema {}). Update the add-on.", schema as i64));
    }
    if let Some(named) = map.get("id") {
        if named.as_str() != Some(id) {
            return Err(format!("\"id\" must match the file name ({id})."));
        }
    }
    if map.get("enabled").is_some_and(|one| !one.is_boolean()) {
        return Err("\"enabled\" must be true or false.".into());
    }
    Ok(map)
}

/// A checked file is on unless it says `"enabled": false`.
pub fn enabled(data: &Map<String, Value>) -> bool {
    data.get("enabled").and_then(Value::as_bool) != Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_like_the_addon() {
        assert!(check("lobby", r#"{"schema":1,"id":"lobby"}"#).is_ok());
        assert_eq!(check("lobby", "{").unwrap_err().split(':').next(), Some("Invalid JSON"));
        assert_eq!(check("lobby", "[]").unwrap_err(), "The file must be a JSON object.");
        assert_eq!(check("lobby", "{}").unwrap_err(), "\"schema\" is missing.");
        assert_eq!(check("lobby", r#"{"schema":1.5}"#).unwrap_err(), "\"schema\" must be a whole number from 1.");
        assert!(check("lobby", r#"{"schema":9}"#).unwrap_err().contains("newer"));
        assert!(check("lobby", r#"{"schema":1,"id":"other"}"#).unwrap_err().contains("match"));
        assert_eq!(check("lobby", r#"{"schema":1,"enabled":"no"}"#).unwrap_err(), "\"enabled\" must be true or false.");
        assert!(check("Big", r#"{"schema":1}"#).is_err());
        assert!(!enabled(&check("a", r#"{"schema":1,"enabled":false}"#).unwrap()));
        assert!(enabled(&check("a", r#"{"schema":1}"#).unwrap()));
    }
}

//! Biomes (`biome` files): biomes of our own, `monolab:<id>`, with their sky, fog, water, grass and leaf colours,
//! floating particles, music and ambient sound. A world generator names one as `studio:<id>` in its biome rules.
//! Both add-ons add them while the server starts (Paper writes them into a data pack its bootstrap hands over, the
//! fork takes them with `register-registry-entry("worldgen/biome")`), so one made or changed later works after
//! the next restart.
//!
//! The vanilla server needs more than looks: which caves, ores, plants and mobs a biome has. Ours take plains' (the
//! fork treats every custom biome as plains on the server anyway), so a vanilla world with them still has ores.

use crate::{fields, item};
use serde_json::{json, Map, Value};

type Data = Map<String, Value>;

pub const REGISTRY: &str = "worldgen/biome";
/// Plains' carvers, features and mob spawns (Minecraft 26.x), which every studio biome copies.
const PLAINS_LIFE: &str = include_str!("biome_plains.json");
/// The colour fields: the file's name, and the biome's.
pub const COLORS: [(&str, &str); 6] = [
    ("sky", "sky_color"),
    ("fog", "fog_color"),
    ("water", "water_color"),
    ("waterFog", "water_fog_color"),
    ("grass", "grass_color"),
    ("foliage", "foliage_color"),
];
pub const WATER: &str = "#3f76e4";

/// `#rrggbb` in lower case, or nothing when the field is empty or not a colour.
pub fn color(data: &Data, key: &str) -> Option<String> {
    let raw = fields::text(data, key, "").trim().to_ascii_lowercase();
    let hex = raw.strip_prefix('#').unwrap_or(&raw);
    (hex.len() == 6 && hex.bytes().all(|one| one.is_ascii_hexdigit())).then(|| format!("#{hex}"))
}

/// A sound by a studio sound's name (`monolab:studio.<name>`) or a sound id with `:`, like `song::sound`.
pub fn sound(data: &Data, key: &str) -> String {
    let raw = fields::text(data, key, "").trim().to_ascii_lowercase();
    if raw.is_empty() || raw.contains(':') {
        raw
    } else {
        format!("monolab:studio.{raw}")
    }
}

/// A particle id (`minecraft:` added when it has none).
pub fn particle(data: &Data) -> String {
    let raw = fields::text(data, "particle", "").trim().to_ascii_lowercase();
    if raw.is_empty() || raw.contains(':') {
        raw
    } else {
        format!("minecraft:{raw}")
    }
}

pub fn temperature(data: &Data) -> f64 {
    item::float(data, "temperature", 0.8, -1.0, 2.0)
}

pub fn downfall(data: &Data) -> f64 {
    item::float(data, "downfall", 0.4, 0.0, 1.0)
}

/// The vanilla biome a server shows where it can't place this one (the fork's generator for now).
pub fn like(data: &Data) -> String {
    let raw = fields::text(data, "like", "plains").trim().to_ascii_lowercase();
    let raw = raw.strip_prefix("minecraft:").unwrap_or(&raw).to_string();
    if raw.is_empty() {
        "plains".into()
    } else {
        raw
    }
}

/// The vanilla datapack JSON (Minecraft 26.x: looks are environment attributes). What both add-ons register, and
/// what tells a changed biome apart.
pub fn registry_json(data: &Data) -> String {
    let life: Value = serde_json::from_str(PLAINS_LIFE).unwrap_or(Value::Null);
    let mut attributes = Map::new();
    attributes.insert("minecraft:gameplay/natural_mob_spawns".into(), life["spawns"].clone());
    for (key, name) in [("sky", "sky_color"), ("fog", "fog_color"), ("waterFog", "water_fog_color")] {
        if let Some(one) = color(data, key) {
            attributes.insert(format!("minecraft:visual/{name}"), json!(one));
        }
    }
    let particle = particle(data);
    if !particle.is_empty() {
        let chance = item::float(data, "particleChance", 0.01, 0.0, 1.0);
        attributes.insert(
            "minecraft:visual/ambient_particles".into(),
            json!({ "argument": [{ "particle": { "type": particle }, "probability": chance }], "modifier": "append" }),
        );
    }
    let music = sound(data, "music");
    if !music.is_empty() {
        attributes.insert(
            "minecraft:audio/background_music".into(),
            json!({ "default": { "sound": music, "min_delay": 12000, "max_delay": 24000 } }),
        );
    }
    let ambient = sound(data, "ambient");
    if !ambient.is_empty() {
        attributes.insert("minecraft:audio/ambient_sounds".into(), json!({ "loop": ambient }));
    }
    let mut effects = Map::new();
    effects.insert("water_color".into(), json!(color(data, "water").unwrap_or_else(|| WATER.into())));
    for (key, name) in [("grass", "grass_color"), ("foliage", "foliage_color")] {
        if let Some(one) = color(data, key) {
            effects.insert(name.into(), json!(one));
        }
    }
    json!({
        "attributes": attributes,
        "carvers": life["carvers"].clone(),
        "downfall": downfall(data),
        "effects": effects,
        "features": life["features"].clone(),
        "has_precipitation": data.get("rain").and_then(Value::as_bool) != Some(false),
        "temperature": temperature(data),
    })
    .to_string()
}

/// `studio:<id>` in a generator → `monolab:<id>` on the server.
pub fn game_name(reference: &str) -> Option<String> {
    reference.strip_prefix("studio:").map(|id| format!("monolab:{id}"))
}

pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    for (key, _) in COLORS {
        if !fields::text(data, key, "").trim().is_empty() && color(data, key).is_none() {
            out.push(format!("\"{key}\" isn't a colour like #78a7ff."));
        }
    }
    let valid = |name: &str| {
        name.split_once(':').is_some_and(|(space, path)| {
            !space.is_empty()
                && !path.is_empty()
                && name.bytes().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || b"_-.:/".contains(&one))
        })
    };
    for key in ["music", "ambient"] {
        let one = sound(data, key);
        if !one.is_empty() && !valid(&one) {
            out.push(format!("\"{key}\" isn't a sound name: {one}."));
        }
    }
    let particle = particle(data);
    if !particle.is_empty() && !valid(&particle) {
        out.push(format!("\"particle\" isn't a particle name: {particle}."));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(text: &str) -> Data {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn biomes_carry_their_looks_and_plains_life() {
        let one = data(
            r##"{"sky":"#FF8800","fog":"c0d8ff","water":"#3a7a6a","grass":"#8db127","particle":"white_ash","particleChance":0.1,"music":"calm","rain":false,"temperature":1.5}"##,
        );
        assert!(problems(&one).is_empty());
        let json: Value = serde_json::from_str(&registry_json(&one)).unwrap();
        assert_eq!(json["attributes"]["minecraft:visual/sky_color"], "#ff8800");
        assert_eq!(json["attributes"]["minecraft:visual/fog_color"], "#c0d8ff");
        assert_eq!(json["attributes"]["minecraft:visual/ambient_particles"]["argument"][0]["particle"]["type"], "minecraft:white_ash");
        assert_eq!(json["attributes"]["minecraft:audio/background_music"]["default"]["sound"], "monolab:studio.calm");
        assert_eq!(json["effects"]["water_color"], "#3a7a6a");
        assert_eq!(json["effects"]["grass_color"], "#8db127");
        assert_eq!(json["has_precipitation"], false);
        assert_eq!(json["temperature"], 1.5);
        assert!(json["features"].as_array().is_some_and(|steps| steps.len() == 11));
        assert!(json["attributes"]["minecraft:gameplay/natural_mob_spawns"]["argument"]["spawns_by_category"]["monster"].is_array());
        let plain: Value = serde_json::from_str(&registry_json(&data("{}"))).unwrap();
        assert_eq!(plain["effects"]["water_color"], WATER);
        assert_eq!(plain["has_precipitation"], true);
        assert_eq!(problems(&data(r#"{"sky":"blue","music":"A B"}"#)).len(), 2);
        assert_eq!(like(&data(r#"{"like":"minecraft:Desert"}"#)), "desert");
        assert_eq!(game_name("studio:ash"), Some("monolab:ash".into()));
    }
}

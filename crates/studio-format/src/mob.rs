//! Studio maker M5: the `mob` kind, a vanilla mob with its own name, stats, gear, look (an M4 model riding an
//! invisible base), drops, natural spawning and skills. Shared by the app and the Pumpkin add-on; the Paper add-on keeps
//! the same rules in `StudioMobs.java`.
//!
//! ```json
//! "base": "zombie", "name": "<red>Ruby Golem", "showName": true,
//! "health": 40, "damage": 6, "speed": 0.25, "armor": 4, "toughness": 0,
//! "knockbackResistance": 0, "followRange": 32, "size": 1,            // missing = the base's own
//! "equipment": { "head": "diamond_helmet", "hand": "studio:item/ruby_sword" }, "dropEquipment": false,
//! "model": "",                                                         // a model kind id
//! "loot": "", "vanillaDrops": false, "exp": 5,
//! "ai": { "attack": true, "wander": true, "look": true, "burn": false, "pickup": false },
//! "silent": false, "glowing": false, "persistent": false,
//! "spawn": { "enabled": false, "worlds": [], "biomes": [], "lightMin": 0, "lightMax": 15, "max": 10, "every": 30, "chance": 50, "radius": 32 },
//! "skills": [{ "trigger": "attack", "every": 0, "cooldown": 2, "chance": 100, "targets": "trigger", "radius": 8, "conditions": [], "actions": [] }]
//! ```

use crate::fields::{self, Data};
use crate::item;
use serde_json::Value;

/// When a skill runs: on spawn, when the mob hits something, is hurt, dies, every few seconds, is right-clicked, or
/// picks a new target.
pub const TRIGGERS: [&str; 7] = ["spawn", "attack", "damaged", "death", "timer", "interact", "target"];
/// Who a skill's actions run on: the mob, its target, the other entity of the trigger, or players close by.
pub const TARGETS: [&str; 4] = ["self", "target", "trigger", "nearby"];
pub const SLOTS: [&str; 6] = ["head", "chest", "legs", "feet", "hand", "offhand"];
/// Simple AI switches, and what each is when the file doesn't say.
pub const AI: [&str; 5] = ["attack", "wander", "look", "burn", "pickup"];
pub const AI_DEFAULTS: [bool; 5] = [true, true, true, false, false];
/// What the `projectile` action fires.
pub const PROJECTILES: [&str; 6] = ["arrow", "snowball", "fireball", "small_fireball", "wither_skull", "trident"];
/// The actions M5 adds (usable everywhere actions are; in skills they act on each target).
pub const NEW_ACTIONS: [&str; 7] = ["damage", "heal", "potion", "launch", "particle", "projectile", "summon"];
/// Stat key, least, most. `size` is the game's scale attribute.
pub const STATS: [(&str, f64, f64); 8] = [
    ("health", 1.0, 1024.0),
    ("damage", 0.0, 2048.0),
    ("speed", 0.0, 4.0),
    ("armor", 0.0, 30.0),
    ("toughness", 0.0, 20.0),
    ("knockbackResistance", 0.0, 1.0),
    ("followRange", 1.0, 256.0),
    ("size", 0.0625, 16.0),
];
pub const BASE: &str = "zombie";
/// Living entities a mob can't be based on (not mobs, or bosses with their own rules).
pub const NOT_MOBS: [&str; 4] = ["player", "armor_stand", "ender_dragon", "wither"];
pub const MOST_SKILLS: usize = 32;
pub const MOST_EXP: i64 = 100_000;
/// Seconds: the longest timer or cooldown (a day).
pub const MOST_SECONDS: f64 = 86_400.0;
pub const MOST_ALIVE: i64 = 200;

/// One skill, with every number clamped. `conditions` and `actions` are lists, as written.
#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    pub trigger: String,
    pub every: f64,
    pub cooldown: f64,
    pub chance: f64,
    pub targets: String,
    pub radius: f64,
    pub conditions: Value,
    pub actions: Value,
}

/// Natural spawning: every `every` seconds, for each online player in one of `worlds` (empty = all), a `chance`% try
/// at one spot 16..`radius` blocks away on the surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Spawn {
    pub enabled: bool,
    pub worlds: Vec<String>,
    pub biomes: Vec<String>,
    pub light_min: i64,
    pub light_max: i64,
    pub max: i64,
    pub every: f64,
    pub chance: f64,
    pub radius: f64,
}

/// `minecraft:Zombie` and `zombie` both become `zombie`.
pub fn clean(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("minecraft:") {
        return rest.to_string();
    }
    lower
}

/// The vanilla entity it is, lower case, `zombie` when missing.
pub fn base(data: &Data) -> String {
    let one = clean(&fields::text(data, "base", ""));
    if one.is_empty() {
        BASE.to_string()
    } else {
        one
    }
}

/// The model kind id riding it, or empty.
pub fn model(data: &Data) -> String {
    fields::text(data, "model", "").trim().to_string()
}

/// The loot kind id it drops, or empty.
pub fn loot(data: &Data) -> String {
    fields::text(data, "loot", "").trim().to_string()
}

/// A stat (see [`STATS`]) clamped to its range; `None` when missing (the base's own) or not a stat.
pub fn stat(data: &Data, key: &str) -> Option<f64> {
    let (_, least, most) = STATS.iter().find(|(name, _, _)| *name == key)?;
    data.get(key).and_then(Value::as_f64).filter(|one| one.is_finite()).map(|one| one.clamp(*least, *most))
}

/// Experience dropped on death; `None` = the base's own.
pub fn exp(data: &Data) -> Option<i64> {
    data.get("exp").and_then(Value::as_f64).filter(|one| one.is_finite()).map(|one| (one.round() as i64).clamp(0, MOST_EXP))
}

/// (slot, item) for every slot with an item, in [`SLOTS`] order.
pub fn equipment(data: &Data) -> Vec<(String, String)> {
    let Some(gear) = item::part(data, "equipment") else { return Vec::new() };
    SLOTS
        .iter()
        .filter_map(|slot| {
            let item = fields::text(gear, slot, "").trim().to_string();
            (!item.is_empty()).then(|| (slot.to_string(), item))
        })
        .collect()
}

/// An AI switch (see [`AI`]); unknown names are off.
pub fn ai(data: &Data, switch: &str) -> bool {
    let Some(index) = AI.iter().position(|one| *one == switch) else { return false };
    item::part(data, "ai").and_then(|ai| ai.get(switch)).and_then(Value::as_bool).unwrap_or(AI_DEFAULTS[index])
}

pub fn skills(data: &Data) -> Vec<Skill> {
    let Some(Value::Array(list)) = data.get("skills") else { return Vec::new() };
    list.iter().filter_map(Value::as_object).take(MOST_SKILLS).map(skill).collect()
}

fn skill(one: &Data) -> Skill {
    let trigger = fields::text(one, "trigger", "").trim().to_ascii_lowercase();
    // A timer runs at most once a second.
    let least_every = if trigger == "timer" { 1.0 } else { 0.0 };
    let list = |key: &str| one.get(key).filter(|value| value.is_array()).cloned().unwrap_or_else(|| Value::Array(Vec::new()));
    Skill {
        every: item::float(one, "every", least_every, least_every, MOST_SECONDS),
        cooldown: item::float(one, "cooldown", 0.0, 0.0, MOST_SECONDS),
        chance: item::float(one, "chance", 100.0, 0.0, 100.0),
        targets: fields::choice(one, "targets", &TARGETS, "trigger"),
        radius: item::float(one, "radius", 8.0, 1.0, 64.0),
        conditions: list("conditions"),
        actions: list("actions"),
        trigger,
    }
}

pub fn spawn(data: &Data) -> Spawn {
    let empty = Data::new();
    let one = item::part(data, "spawn").unwrap_or(&empty);
    Spawn {
        enabled: one.get("enabled").and_then(Value::as_bool) == Some(true),
        worlds: fields::lines(one, "worlds").into_iter().map(|world| world.trim().to_string()).filter(|world| !world.is_empty()).collect(),
        biomes: fields::lines(one, "biomes").into_iter().map(|biome| clean(&biome)).filter(|biome| !biome.is_empty()).collect(),
        light_min: fields::number(one, "lightMin", 0, 0, 15),
        light_max: fields::number(one, "lightMax", 15, 0, 15),
        max: fields::number(one, "max", 10, 1, MOST_ALIVE),
        every: item::float(one, "every", 30.0, 1.0, 3600.0),
        chance: item::float(one, "chance", 50.0, 0.0, 100.0),
        radius: item::float(one, "radius", 32.0, 16.0, 128.0),
    }
}

/// What is wrong with the file on its own, for its status. Empty = fine. (Whether the base, model and loot exist is
/// checked by each add-on.)
pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    let base = base(data);
    if !base.bytes().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == b'_') {
        out.push("\"base\" names a vanilla mob, like zombie.".into());
    } else if NOT_MOBS.contains(&base.as_str()) {
        out.push(format!("A mob can't be based on {base}."));
    }
    if data.get("name").is_some_and(|one| !one.is_string()) {
        out.push("\"name\" must be text.".into());
    }
    for (key, least, most) in STATS {
        if data.get(key).is_some_and(|one| one.as_f64().is_none_or(|one| !(least..=most).contains(&one))) {
            out.push(format!("\"{key}\" goes from {least} to {most}."));
        }
    }
    match data.get("equipment") {
        None => {}
        Some(Value::Object(gear)) => {
            for (slot, item) in gear {
                if !SLOTS.contains(&slot.as_str()) {
                    out.push(format!("\"equipment\" has no slot called \"{slot}\"."));
                } else if !item.is_string() {
                    out.push(format!("\"equipment.{slot}\" must be an item name."));
                }
            }
        }
        Some(_) => out.push("\"equipment\" maps a slot (head, hand...) to an item.".into()),
    }
    let model = model(data);
    if !model.is_empty() && !crate::valid_id(&model) {
        out.push("\"model\" names a studio model: a-z, 0-9, - and _.".into());
    }
    let loot = loot(data);
    if !loot.is_empty() && !crate::valid_id(&loot) {
        out.push("\"loot\" names a studio loot table: a-z, 0-9, - and _.".into());
    }
    if data.get("exp").is_some_and(|one| one.as_f64().is_none_or(|one| !(0.0..=MOST_EXP as f64).contains(&one))) {
        out.push(format!("\"exp\" goes from 0 to {MOST_EXP}."));
    }
    if data.get("ai").is_some_and(|one| !one.is_object()) {
        out.push(format!("\"ai\" holds switches: {}.", AI.join(", ")));
    }
    if let Some(place) = data.get("spawn") {
        if !place.is_object() {
            out.push("\"spawn\" must be an object.".into());
        } else {
            let rules = spawn(data);
            if rules.light_min > rules.light_max {
                out.push("\"lightMin\" can't be above \"lightMax\".".into());
            }
        }
    }
    match data.get("skills") {
        None => {}
        Some(Value::Array(list)) => {
            if list.len() > MOST_SKILLS {
                out.push(format!("A mob has up to {MOST_SKILLS} skills."));
            }
            for (index, one) in list.iter().enumerate().take(MOST_SKILLS) {
                let number = index + 1;
                let Some(one) = one.as_object() else {
                    out.push(format!("Skill {number} must be an object."));
                    continue;
                };
                let trigger = fields::text(one, "trigger", "");
                if !TRIGGERS.contains(&trigger.as_str()) {
                    out.push(format!("Skill {number}: \"trigger\" must be one of: {}.", TRIGGERS.join(", ")));
                }
                if one.get("targets").is_some_and(|value| value.as_str().is_none_or(|value| !TARGETS.contains(&value))) {
                    out.push(format!("Skill {number}: \"targets\" must be one of: {}.", TARGETS.join(", ")));
                }
                for key in ["conditions", "actions"] {
                    if one.get(key).is_some_and(|value| !value.is_array()) {
                        out.push(format!("Skill {number}: \"{key}\" must be a list."));
                    }
                }
                if let Some(Value::Array(actions)) = one.get("actions") {
                    if actions.iter().any(|action| action.get("type").and_then(Value::as_str).is_none()) {
                        out.push(format!("Skill {number}: every action needs a \"type\"."));
                    }
                }
            }
        }
        Some(_) => out.push("\"skills\" must be a list.".into()),
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
    fn fields() {
        let one = data(json!({
            "base": " minecraft:Husk ", "model": " golem ", "loot": "gems", "health": 5000, "speed": 0.3, "size": 0, "exp": 12.4,
            "equipment": { "head": "diamond_helmet", "chest": " ", "hand": "studio:item/ruby_sword" },
            "ai": { "wander": false, "burn": true }
        }));
        assert_eq!(base(&one), "husk");
        assert_eq!(base(&data(json!({}))), BASE);
        assert_eq!(model(&one), "golem");
        assert_eq!(loot(&one), "gems");
        assert_eq!(stat(&one, "health"), Some(1024.0));
        assert_eq!(stat(&one, "speed"), Some(0.3));
        assert_eq!(stat(&one, "size"), Some(0.0625));
        assert_eq!(stat(&one, "armor"), None);
        assert_eq!(stat(&one, "loot"), None);
        assert_eq!(exp(&one), Some(12));
        assert_eq!(exp(&data(json!({}))), None);
        assert_eq!(
            equipment(&one),
            vec![("head".to_string(), "diamond_helmet".to_string()), ("hand".to_string(), "studio:item/ruby_sword".to_string())]
        );
        assert!(ai(&one, "attack") && !ai(&one, "wander") && ai(&one, "look") && ai(&one, "burn") && !ai(&one, "pickup"));
        assert!(!ai(&one, "fly"));
    }

    #[test]
    fn skills_are_clamped() {
        let one = data(json!({ "skills": [
            { "trigger": "timer", "every": 0, "cooldown": -3, "chance": 150, "targets": "everyone", "radius": 500, "actions": [{ "type": "heal" }] },
            { "trigger": "attack" },
            "not a skill"
        ] }));
        let found = skills(&one);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].trigger, "timer");
        assert_eq!(found[0].every, 1.0);
        assert_eq!(found[0].cooldown, 0.0);
        assert_eq!(found[0].chance, 100.0);
        assert_eq!(found[0].targets, "trigger");
        assert_eq!(found[0].radius, 64.0);
        assert_eq!(found[0].actions, json!([{ "type": "heal" }]));
        assert_eq!(found[0].conditions, json!([]));
        assert_eq!(found[1].every, 0.0);
        assert_eq!(found[1].radius, 8.0);
    }

    #[test]
    fn spawn_rules() {
        let none = spawn(&data(json!({})));
        assert!(!none.enabled);
        assert_eq!((none.light_min, none.light_max, none.max, none.every, none.chance, none.radius), (0, 15, 10, 30.0, 50.0, 32.0));
        let one = spawn(&data(
            json!({ "spawn": { "enabled": true, "worlds": ["world", " "], "biomes": ["minecraft:Plains"], "radius": 4, "max": 0 } }),
        ));
        assert!(one.enabled);
        assert_eq!(one.worlds, ["world"]);
        assert_eq!(one.biomes, ["plains"]);
        assert_eq!(one.radius, 16.0);
        assert_eq!(one.max, 1);
    }

    #[test]
    fn problems_are_named() {
        let good = data(
            json!({ "base": "zombie", "health": 40, "equipment": { "head": "diamond_helmet" }, "skills": [{ "trigger": "spawn", "targets": "self", "actions": [] }] }),
        );
        assert!(problems(&good).is_empty(), "{:?}", problems(&good));
        assert!(problems(&data(json!({}))).is_empty());
        assert!(problems(&data(json!({ "base": "player" })))[0].contains("player"));
        let bad = data(json!({
            "base": "zom bie", "health": 0, "equipment": { "back": "elytra", "head": 3 }, "model": "Golem", "exp": -1, "ai": [],
            "spawn": { "lightMin": 10, "lightMax": 2 },
            "skills": [{ "trigger": "jump", "targets": "all", "actions": [{}], "conditions": {} }, 4]
        }));
        let found = problems(&bad);
        assert_eq!(found.len(), 13, "{found:?}");
        assert!(found[1].contains("\"health\" goes from 1 to 1024"));
        assert!(found.iter().any(|one| one.contains("lightMin")));
        assert!(found.last().unwrap().contains("Skill 2"));
    }
}

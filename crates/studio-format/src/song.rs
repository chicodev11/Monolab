//! Music discs (`song` files): jukebox songs of our own, `monolab:<id>`. Both add-ons register them while the
//! server starts (Paper's `jukebox_song` registry event in its bootstrap, the fork's `register-registry-entry`), so a
//! song made or changed later works after the next restart. An item plays one with `disc` (its
//! `jukebox_playable` component); the sound itself is a studio sound in the pack.

use crate::{fields, item, mini};
use serde_json::{Map, Value, json};

type Data = Map<String, Value>;

pub const REGISTRY: &str = "jukebox_song";
pub const MOST_SECONDS: f64 = 3600.0;

/// The sound event it plays: a studio sound's file name (`assets/sounds/<name>.ogg`) is `monolab:studio.<name>`;
/// a name with `:` is used as it is (`minecraft:music_disc.cat`).
pub fn sound(data: &Data) -> String {
    let raw = fields::text(data, "sound", "").trim().to_ascii_lowercase();
    if raw.is_empty() || raw.contains(':') { raw } else { format!("monolab:studio.{raw}") }
}

pub fn seconds(data: &Data) -> f64 {
    item::float(data, "seconds", 60.0, 1.0, MOST_SECONDS)
}

/// What a comparator next to the jukebox reads while it plays, 0-15.
pub fn comparator(data: &Data) -> u8 {
    fields::number(data, "comparator", 1, 0, 15) as u8
}

/// The line under the disc's name and in the "Now playing" bar (MiniMessage).
pub fn description(data: &Data, id: &str) -> String {
    let name = fields::text(data, "name", "").trim().to_string();
    if name.is_empty() { id.replace(['_', '-'], " ") } else { name }
}

/// What both add-ons compare to tell that a song changed since the server started.
pub fn fingerprint(data: &Data, id: &str) -> String {
    format!("{}|{}|{}|{}", sound(data), seconds(data), comparator(data), description(data, id))
}

/// The vanilla datapack JSON of the song (the fork's `register-registry-entry`).
pub fn registry_json(data: &Data, id: &str) -> String {
    json!({
        "sound_event": { "sound_id": sound(data) },
        "description": mini::to_json(&description(data, id)),
        "length_in_seconds": seconds(data) as f32,
        "comparator_output": comparator(data),
    })
    .to_string()
}

pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    let sound = sound(data);
    if sound.is_empty() {
        out.push("\"sound\" is needed: a studio sound's name or a sound id.".into());
    } else if !sound.split_once(':').is_some_and(|(space, path)| !space.is_empty() && !path.is_empty() && sound.bytes().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || b"_-.:/".contains(&one))) {
        out.push(format!("\"sound\" isn't a sound name: {sound}."));
    }
    if !data.get("seconds").is_some_and(Value::is_number) {
        out.push("\"seconds\" is needed: how long the song plays.".into());
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
    fn songs_name_their_sound() {
        let one = data(r#"{"sound":"boss_theme","seconds":95,"name":"<gold>Boss theme"}"#);
        assert_eq!(sound(&one), "monolab:studio.boss_theme");
        assert!(problems(&one).is_empty());
        let json: Value = serde_json::from_str(&registry_json(&one, "boss")).unwrap();
        assert_eq!(json["sound_event"]["sound_id"], "monolab:studio.boss_theme");
        assert_eq!(json["length_in_seconds"], 95.0);
        assert_eq!(json["comparator_output"], 1);
        assert_eq!(sound(&data(r#"{"sound":"minecraft:music_disc.cat"}"#)), "minecraft:music_disc.cat");
        assert_eq!(problems(&data("{}")).len(), 2);
        assert_eq!(description(&data("{}"), "boss_theme"), "boss theme");
    }
}

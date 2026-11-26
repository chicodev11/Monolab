//! Studio maker M4: the `model` kind, a Blockbench model (`studio/assets/models/<model>.bbmodel`) shown in the world as
//! one item display per bone. Shared by the app's pack builder (`src-tauri/src/studio_pack_models.rs`) and the Pumpkin
//! add-on; the Paper add-on reads the baked rig (`studio/pack/models/<id>.json`) and keeps the field rules in
//! `StudioModels.java`.
//!
//! ```json
//! "model": "dragon", "scale": 1,
//! "animations": { "idle": "idle", "walk": "walk_cycle" },     // role → the .bbmodel's animation name
//! "hitbox": { "width": 1.5, "height": 2 },                    // an interaction entity; none when missing or 0
//! "seat": "saddle",                                           // a bone players can ride
//! "visibility": "everyone",                                    // or "permission" with "permission": "node"
//! "place": { "world": "world", "x": 0, "y": 64, "z": 0, "yaw": 90 },  // shown there by itself (optional)
//! "clickActions": []
//! ```
//! NPCs, mobs and furniture name a model by id later; the `animation` action plays one by name on placed models.

use crate::bbmodel::Model;
use crate::fields::{self, Data};
use serde_json::Value;

/// The roles other kinds ask for. Any other key is kept too (an action can play it by name).
pub const ROLES: [&str; 8] = ["idle", "walk", "run", "attack", "hurt", "death", "spawn", "interact"];
pub const VISIBILITY: [&str; 2] = ["everyone", "permission"];
pub const MOST_SCALE: f64 = 16.0;
pub const LEAST_SCALE: f64 = 0.05;
pub const MOST_HITBOX: f64 = 16.0;

pub fn asset(data: &Data) -> String {
    fields::text(data, "model", "").trim().to_string()
}

pub fn scale(data: &Data) -> f64 {
    data.get("scale").and_then(Value::as_f64).filter(|one| one.is_finite() && *one > 0.0).unwrap_or(1.0).clamp(LEAST_SCALE, MOST_SCALE)
}

/// role → animation name, as written.
pub fn roles(data: &Data) -> Vec<(String, String)> {
    let Some(Value::Object(map)) = data.get("animations") else { return Vec::new() };
    map.iter()
        .filter_map(|(role, name)| Some((role.clone(), name.as_str()?.trim().to_string())))
        .filter(|(_, name)| !name.is_empty())
        .collect()
}

/// (width, height) in blocks, or `None` for no hitbox.
pub fn hitbox(data: &Data) -> Option<(f64, f64)> {
    let Some(Value::Object(box_)) = data.get("hitbox") else { return None };
    let get = |key| box_.get(key).and_then(Value::as_f64).filter(|one| one.is_finite()).unwrap_or(0.0).clamp(0.0, MOST_HITBOX);
    let (width, height) = (get("width"), get("height"));
    (width > 0.0 && height > 0.0).then_some((width, height))
}

pub fn seat(data: &Data) -> String {
    fields::text(data, "seat", "").trim().to_string()
}

/// What is wrong with the file on its own, for its status. Empty = fine.
pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    let asset = asset(data);
    if asset.is_empty() {
        out.push("Pick a Blockbench model (.bbmodel) from the studio's models.".into());
    } else if !crate::valid_id(&asset) {
        out.push("\"model\" names an asset: a-z, 0-9, - and _.".into());
    }
    if data.get("scale").is_some_and(|one| one.as_f64().is_none_or(|one| !(LEAST_SCALE..=MOST_SCALE).contains(&one))) {
        out.push(format!("\"scale\" goes from {LEAST_SCALE} to {MOST_SCALE}."));
    }
    if data.get("animations").is_some_and(|one| !one.is_object()) {
        out.push("\"animations\" maps a role (idle, walk...) to an animation name.".into());
    }
    if data.get("hitbox").is_some_and(|one| !one.is_object()) {
        out.push("\"hitbox\" needs a width and a height.".into());
    }
    let visibility = fields::text(data, "visibility", "everyone");
    if !VISIBILITY.contains(&visibility.as_str()) {
        out.push("\"visibility\" is everyone or permission.".into());
    } else if visibility == "permission" && fields::text(data, "permission", "").trim().is_empty() {
        out.push("Name the permission that may see it.".into());
    }
    if let Some(place) = data.get("place") {
        match place.as_object() {
            Some(place) if ["x", "y", "z"].iter().all(|key| place.get(*key).and_then(Value::as_f64).is_some()) => {}
            _ => out.push("\"place\" needs a world, x, y and z.".into()),
        }
    }
    if data.get("clickActions").is_some_and(|one| !one.is_array()) {
        out.push("\"clickActions\" must be a list.".into());
    }
    out
}

/// What doesn't match the model itself: animation names and the seat bone.
pub fn problems_with(data: &Data, model: &Model) -> Vec<String> {
    let mut out = Vec::new();
    for (role, name) in roles(data) {
        if !model.animations.iter().any(|one| one.name == name) {
            out.push(format!("\"{role}\": the model has no animation called \"{name}\"."));
        }
    }
    let seat = seat(data);
    if !seat.is_empty() && !model.bones.iter().any(|one| one.name == seat) {
        out.push(format!("The model has no bone called \"{seat}\" to sit on."));
    }
    if model.bones.iter().all(|one| one.cubes.is_empty()) {
        out.push("The model has no cubes to show.".into());
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
        let one = data(
            json!({ "model": "dragon", "scale": 2, "animations": { "idle": "idle", "walk": " " }, "hitbox": { "width": 1, "height": 0 }, "seat": "saddle" }),
        );
        assert_eq!(asset(&one), "dragon");
        assert_eq!(scale(&one), 2.0);
        assert_eq!(roles(&one), vec![("idle".to_string(), "idle".to_string())]);
        assert_eq!(hitbox(&one), None);
        assert_eq!(hitbox(&data(json!({ "hitbox": { "width": 1, "height": 40 } }))), Some((1.0, 16.0)));
        assert_eq!(scale(&data(json!({ "scale": 0 }))), 1.0);
        assert!(problems(&one).is_empty(), "{:?}", problems(&one));
    }

    #[test]
    fn problems_are_named() {
        assert!(problems(&data(json!({})))[0].contains("Blockbench"));
        let bad = data(json!({ "model": "Dragon", "scale": 99, "visibility": "permission", "place": { "x": 1 }, "clickActions": {} }));
        assert_eq!(problems(&bad).len(), 5, "{:?}", problems(&bad));
        let model = crate::bbmodel::parse(r#"{"elements":[{"type":"cube","uuid":"c","from":[0,0,0],"to":[1,1,1]}],"outliner":[{"name":"body","uuid":"g","children":["c"]}],"animations":[{"name":"idle","animators":{}}]}"#).unwrap();
        let file = data(json!({ "model": "x", "animations": { "idle": "idle", "walk": "walk" }, "seat": "saddle" }));
        let found = problems_with(&file, &model);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].contains("walk"));
    }
}

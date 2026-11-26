//! Paintings (`painting` files): painting variants of our own, `monolab:<id>`, drawn from a studio texture the pack
//! copies to `textures/painting/<id>.png`. Like songs, both add-ons register them while the server starts, so a
//! painting made or changed later works after the next restart. `studio:painting/<id>` gives a painting item that
//! hangs this one.

use crate::{fields, mini};
use serde_json::{Map, Value, json};

type Data = Map<String, Value>;

pub const REGISTRY: &str = "painting_variant";
/// The game's limit for a painting's width and height, in blocks.
pub const MOST_BLOCKS: i64 = 16;

pub fn texture(data: &Data) -> String {
    fields::text(data, "texture", "").trim().to_string()
}

pub fn width(data: &Data) -> u8 {
    fields::number(data, "width", 1, 1, MOST_BLOCKS) as u8
}

pub fn height(data: &Data) -> u8 {
    fields::number(data, "height", 1, 1, MOST_BLOCKS) as u8
}

/// The title shown on the item (MiniMessage); the id when empty.
pub fn title(data: &Data, id: &str) -> String {
    let name = fields::text(data, "title", "").trim().to_string();
    if name.is_empty() { id.replace(['_', '-'], " ") } else { name }
}

pub fn author(data: &Data) -> String {
    fields::text(data, "author", "").trim().to_string()
}

pub fn fingerprint(data: &Data, id: &str) -> String {
    format!("{}|{}|{}|{}", width(data), height(data), title(data, id), author(data))
}

/// The vanilla datapack JSON (the fork's `register-registry-entry`).
pub fn registry_json(data: &Data, id: &str) -> String {
    let mut out = json!({
        "asset_id": format!("monolab:{id}"),
        "width": width(data),
        "height": height(data),
        "title": mini::to_json(&title(data, id)),
    });
    let author = author(data);
    if !author.is_empty() {
        out["author"] = mini::to_json(&author);
    }
    out.to_string()
}

/// Where the pack puts the painting's picture.
pub fn pack_path(id: &str) -> String {
    format!("assets/monolab/textures/painting/{id}.png")
}

pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    if texture(data).is_empty() {
        out.push("\"texture\" is needed: a studio picture (Assets → Textures).".into());
    }
    for key in ["width", "height"] {
        if data.get(key).and_then(Value::as_f64).is_some_and(|one| !(1.0..=MOST_BLOCKS as f64).contains(&one)) {
            out.push(format!("\"{key}\" is 1 to {MOST_BLOCKS} blocks."));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paintings_have_a_size_and_a_picture() {
        let one: Data = serde_json::from_str(r#"{"texture":"sunset","width":2,"height":1,"title":"Sunset","author":"Mina"}"#).unwrap();
        assert!(problems(&one).is_empty());
        let json: Value = serde_json::from_str(&registry_json(&one, "sunset")).unwrap();
        assert_eq!(json["asset_id"], "monolab:sunset");
        assert_eq!(json["width"], 2);
        assert!(json.get("author").is_some());
        let empty: Data = serde_json::from_str(r#"{"width":20}"#).unwrap();
        assert_eq!(problems(&empty).len(), 2);
        assert_eq!(width(&empty), 16);
    }
}

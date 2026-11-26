//! A studio advancement file as a Minecraft advancement JSON. The Paper add-on loads its own copy of this
//! (`StudioAdvancements.java`); the Pumpkin add-on registers the same JSON with the fork (`register-advancement`),
//! placing them on the tab itself with [`layout`] since the fork takes positions from the plugin.

use crate::fields::{self, Data};
use crate::mini;
use serde_json::{Value, json};

/// The criterion only the studio's `advancement` action grants.
pub const CRITERION: &str = "granted";

/// The advancement JSON for one file. `namespace` is the add-on's (`monolab`), `items` turns an item name into
/// its registry key (`diamond` → `minecraft:diamond`), or `None` when the server doesn't know it.
pub fn json(id: &str, data: &Data, namespace: &str, items: impl Fn(&str) -> Option<String>) -> Value {
    let icon = items(&fields::text(data, "icon", "diamond")).unwrap_or_else(|| "minecraft:diamond".into());
    let frame = fields::choice(data, "frame", &crate::ADVANCEMENT_FRAMES, "task");
    let parent = fields::text(data, "parent", "").trim().to_string();
    let text = |key: &str, fallback: &str| mini::to_json(&fields::text(data, key, fallback));
    let mut display = json!({
        "icon": { "id": icon },
        "title": text("title", id),
        "description": text("description", ""),
        "frame": frame,
        "show_toast": data.get("toast").and_then(Value::as_bool) != Some(false),
        "announce_to_chat": data.get("announce").and_then(Value::as_bool) != Some(false),
        "hidden": data.get("hidden").and_then(Value::as_bool) == Some(true),
    });
    if parent.is_empty() {
        display["background"] = json!(background(&fields::text(data, "background", "stone")));
    }
    let mut out = json!({ "display": display, "criteria": { CRITERION: { "trigger": "minecraft:impossible" } } });
    if !parent.is_empty() {
        out["parent"] = json!(format!("{namespace}:{parent}"));
    }
    out
}

/// The tab background: a texture path as it is, or a block name in the 1.21.5+ form (`minecraft:block/stone`).
pub fn background(given: &str) -> String {
    let name = given.trim().to_ascii_lowercase();
    if name.contains('/') {
        return name;
    }
    let block = name.strip_prefix("minecraft:").unwrap_or(&name);
    format!("minecraft:block/{}", if block.is_empty() { "stone" } else { block })
}

/// Parents first; files whose parent is missing (or that loop) are left out.
pub fn order(all: &[(String, Data)]) -> Vec<&(String, Data)> {
    fn visit<'a>(id: &str, all: &'a [(String, Data)], out: &mut Vec<&'a (String, Data)>, path: &mut Vec<String>) -> bool {
        if out.iter().any(|(done, _)| done == id) {
            return true;
        }
        if path.iter().any(|one| one == id) {
            return false;
        }
        let Some(one) = all.iter().find(|(name, _)| name == id) else { return false };
        let parent = fields::text(&one.1, "parent", "").trim().to_string();
        if !parent.is_empty() {
            path.push(id.to_string());
            let ok = visit(&parent, all, out, path);
            path.pop();
            if !ok {
                return false;
            }
        }
        out.push(one);
        true
    }
    let mut out = Vec::new();
    let mut names: Vec<&String> = all.iter().map(|(id, _)| id).collect();
    names.sort();
    for id in names {
        visit(id, all, &mut out, &mut Vec::new());
    }
    out
}

/// Where each advancement of `ordered` (parents first, as [`order`] gives) sits on its tab, like the game lays out
/// a data pack's: a child one column right of its parent, each leaf on its own row, a parent on its first child's.
pub fn layout(ordered: &[&(String, Data)]) -> std::collections::HashMap<String, (f32, f32)> {
    fn place(id: &str, column: f32, children: &std::collections::HashMap<String, Vec<String>>, row: &mut f32, out: &mut std::collections::HashMap<String, (f32, f32)>) {
        let mine = children.get(id).cloned().unwrap_or_default();
        let first = *row;
        if mine.is_empty() {
            *row += 1.0;
        }
        for child in &mine {
            place(child, column + 1.0, children, row, out);
        }
        out.insert(id.to_string(), (column, first));
    }
    let mut children: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut roots = Vec::new();
    for (id, data) in ordered {
        let parent = fields::text(data, "parent", "").trim().to_string();
        if parent.is_empty() {
            roots.push(id.clone());
        } else {
            children.entry(parent).or_default().push(id.clone());
        }
    }
    let mut out = std::collections::HashMap::new();
    for root in roots {
        // Every root is its own tab, counted from the top.
        let mut row = 0.0;
        place(&root, 0.0, &children, &mut row, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(value: Value) -> Data {
        value.as_object().cloned().unwrap()
    }

    #[test]
    fn makes_minecraft_advancements() {
        let root = json("first", &data(json!({ "title": "<gold>Hi", "icon": "oak_sapling", "frame": "GOAL", "toast": false })), "monolab", |name| Some(format!("minecraft:{name}")));
        assert_eq!(root["display"]["icon"]["id"], "minecraft:oak_sapling");
        assert_eq!(root["display"]["frame"], "goal");
        assert_eq!(root["display"]["show_toast"], false);
        assert_eq!(root["display"]["announce_to_chat"], true);
        assert_eq!(root["display"]["background"], "minecraft:block/stone");
        assert_eq!(root["criteria"]["granted"]["trigger"], "minecraft:impossible");
        assert!(root.get("parent").is_none());
        let child = json("next", &data(json!({ "parent": "first", "icon": "nope" })), "monolab", |_| None);
        assert_eq!(child["parent"], "monolab:first");
        assert_eq!(child["display"]["icon"]["id"], "minecraft:diamond");
        assert!(child["display"].get("background").is_none());
        assert_eq!(background("minecraft:textures/block/dirt.png"), "minecraft:textures/block/dirt.png");
    }

    #[test]
    fn orders_parents_first_and_drops_orphans() {
        let all = vec![
            ("b".to_string(), data(json!({ "parent": "a" }))),
            ("a".to_string(), data(json!({}))),
            ("c".to_string(), data(json!({ "parent": "missing" }))),
            ("d".to_string(), data(json!({ "parent": "e" }))),
            ("e".to_string(), data(json!({ "parent": "d" }))),
        ];
        let names: Vec<&str> = order(&all).iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn lays_out_tabs() {
        let all = vec![
            ("root".to_string(), data(json!({}))),
            ("one".to_string(), data(json!({ "parent": "root" }))),
            ("two".to_string(), data(json!({ "parent": "root" }))),
            ("deep".to_string(), data(json!({ "parent": "one" }))),
            ("other".to_string(), data(json!({}))),
        ];
        let spots = layout(&order(&all));
        assert_eq!(spots["root"], (0.0, 0.0));
        assert_eq!(spots["one"], (1.0, 0.0));
        assert_eq!(spots["deep"], (2.0, 0.0));
        assert_eq!(spots["two"], (1.0, 1.0));
        assert_eq!(spots["other"], (0.0, 0.0));
    }
}

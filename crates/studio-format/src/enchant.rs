//! Studio custom enchantments (`enchant` kind, `studio/enchant/<id>.json`). The game's own enchantments can't be added
//! while a Paper server runs, so ours live on the item: `monolab:enchants` (custom data / PDC) holds
//! `id=level;id=level`, and the item's lore starts with one line per enchantment (`<gray>Lifesteal II`), counted in
//! `monolab:enchant_lines` so the next change replaces them. Their effects are action lists:
//!
//! ```json
//! "name": "Lifesteal", "maxLevel": 3,
//! "items": ["sword", "axe"],             // what it goes on (see ITEM_GROUPS); empty = anything
//! "chance": 100,                          // percent each time
//! "hitActions": [], "mineActions": [], "hurtActions": []
//! ```
//!
//! `hitActions` run when the holder hits something, `mineActions` when they break a block, `hurtActions` when a worn
//! piece's wearer is hurt. Placeholders: `<enchant>`, `<level>` (a number), `<target>` / `<block>` like items.
//! Items get them from the item's `enchantments` (`"studio:lifesteal": 2`) or the `enchant` action (held item).
//! Same rules in the Paper add-on's `StudioEnchants.java`.

use crate::fields::{self, Data};
use serde_json::Value;

/// What an enchantment may go on: groups of item names (by the end of the name, like the game's tags).
pub const ITEM_GROUPS: [&str; 15] = ["sword", "axe", "pickaxe", "shovel", "hoe", "bow", "crossbow", "trident", "mace", "fishing_rod", "helmet", "chestplate", "leggings", "boots", "elytra"];
/// Wider groups the editor offers too.
pub const WIDE_GROUPS: [&str; 4] = ["weapon", "tool", "armor", "any"];
pub const ACTION_LISTS: [&str; 3] = ["hitActions", "mineActions", "hurtActions"];
pub const MOST_LEVEL: u32 = 255;
/// The prefix of a studio enchantment in an item's `enchantments`.
pub const PREFIX: &str = "studio:";
/// Keys on the item.
pub const KEY_ENCHANTS: &str = "enchants";
pub const KEY_LINES: &str = "enchant_lines";

pub fn max_level(data: &Data) -> u32 {
    fields::number(data, "maxLevel", 1, 1, i64::from(MOST_LEVEL)) as u32
}

pub fn chance(data: &Data) -> f64 {
    fields::number(data, "chance", 100, 0, 100) as f64 / 100.0
}

/// Whether an item (`minecraft:diamond_sword`) may carry it.
pub fn fits(data: &Data, material: &str) -> bool {
    let groups: Vec<String> = fields::lines(data, "items").iter().map(|one| one.trim().to_ascii_lowercase()).filter(|one| !one.is_empty()).collect();
    if groups.is_empty() {
        return true;
    }
    let bare = material.trim().trim_start_matches("minecraft:");
    groups.iter().any(|group| in_group(group, bare))
}

fn in_group(group: &str, bare: &str) -> bool {
    match group {
        "any" => true,
        "weapon" => ["sword", "axe", "bow", "crossbow", "trident", "mace"].iter().any(|one| in_group(one, bare)),
        "tool" => ["pickaxe", "axe", "shovel", "hoe", "fishing_rod", "shears"].iter().any(|one| in_group(one, bare)),
        "armor" => ["helmet", "chestplate", "leggings", "boots"].iter().any(|one| in_group(one, bare)) || bare == "turtle_helmet",
        "axe" => bare.ends_with("_axe"),
        "bow" => bare == "bow",
        "shears" => bare == "shears",
        other => bare == other || bare.ends_with(&format!("_{other}")),
    }
}

/// `II` for 2: roman numerals up to 10, the number past it (like the game).
pub fn numeral(level: u32) -> String {
    const ROMAN: [&str; 10] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];
    if (1..=10).contains(&level) { ROMAN[level as usize - 1].to_string() } else { level.to_string() }
}

/// The lore line for one enchantment: its name (MiniMessage) and level, grey unless the name has its own colour.
pub fn lore_line(data: &Data, id: &str, level: u32) -> String {
    let name = fields::text(data, "name", id);
    let shown = if max_level(data) == 1 && level == 1 { name.clone() } else { format!("{name} {}", numeral(level)) };
    if name.trim_start().starts_with('<') { shown } else { format!("<gray>{shown}") }
}

/// `id=level;id=level` read back, in order, levels at least 1.
pub fn parse(text: &str) -> Vec<(String, u32)> {
    text.split(';')
        .filter_map(|one| {
            let (id, level) = one.split_once('=')?;
            let level = level.trim().parse::<u32>().ok()?.clamp(1, MOST_LEVEL);
            let id = id.trim();
            crate::valid_id(id).then(|| (id.to_string(), level))
        })
        .collect()
}

pub fn write(list: &[(String, u32)]) -> String {
    list.iter().map(|(id, level)| format!("{id}={level}")).collect::<Vec<_>>().join(";")
}

/// `list` with `id` set to `level` (0 takes it off), keeping the order.
pub fn with(list: &[(String, u32)], id: &str, level: u32) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = list.iter().filter(|(one, _)| one != id).cloned().collect();
    if level > 0 {
        match list.iter().position(|(one, _)| one == id) {
            Some(at) => out.insert(at.min(out.len()), (id.to_string(), level)),
            None => out.push((id.to_string(), level)),
        }
    }
    out
}

/// The studio enchantments an item file lists in `enchantments` (`studio:<id>`).
pub fn on_item(item: &Data) -> Vec<(String, u32)> {
    let Some(Value::Object(map)) = item.get("enchantments") else { return Vec::new() };
    map.iter()
        .filter_map(|(key, level)| {
            let id = key.trim().to_ascii_lowercase().strip_prefix(PREFIX)?.to_string();
            let level = level.as_f64().unwrap_or(1.0).round().clamp(1.0, f64::from(MOST_LEVEL)) as u32;
            crate::valid_id(&id).then_some((id, level))
        })
        .collect()
}

pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    for group in fields::lines(data, "items") {
        let clean = group.trim().to_ascii_lowercase();
        if !clean.is_empty() && !ITEM_GROUPS.contains(&clean.as_str()) && !WIDE_GROUPS.contains(&clean.as_str()) {
            out.push(format!("\"items\" takes: {}, {}.", ITEM_GROUPS.join(", "), WIDE_GROUPS.join(", ")));
            break;
        }
    }
    if ACTION_LISTS.iter().all(|key| data.get(*key).and_then(Value::as_array).is_none_or(Vec::is_empty)) {
        out.push("Add actions for when it hits, mines or is hurt, or it does nothing.".into());
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
    fn rules() {
        let steal = data(json!({ "name": "Lifesteal", "maxLevel": 3, "items": ["sword", "axe"], "hitActions": [{ "type": "heal" }] }));
        assert!(problems(&steal).is_empty());
        assert!(fits(&steal, "minecraft:diamond_sword") && fits(&steal, "iron_axe") && !fits(&steal, "iron_pickaxe"));
        assert!(fits(&data(json!({ "items": ["armor"] })), "netherite_boots"));
        assert!(fits(&data(json!({ "items": ["tool"] })), "stone_pickaxe"));
        assert!(fits(&data(json!({})), "stick"));
        assert_eq!(lore_line(&steal, "lifesteal", 2), "<gray>Lifesteal II");
        assert_eq!(lore_line(&data(json!({ "name": "<red>Burn" })), "burn", 1), "<red>Burn");
        assert_eq!(numeral(12), "12");
        let list = parse("lifesteal=2;bad id=1;burn=0;x=3");
        assert_eq!(list, vec![("lifesteal".to_string(), 2), ("burn".to_string(), 1), ("x".to_string(), 3)]);
        assert_eq!(write(&with(&list, "burn", 4)), "lifesteal=2;burn=4;x=3");
        assert_eq!(write(&with(&list, "lifesteal", 0)), "burn=1;x=3");
        assert_eq!(write(&with(&list, "new", 1)), "lifesteal=2;burn=1;x=3;new=1");
        assert_eq!(on_item(&data(json!({ "enchantments": { "sharpness": 5, "studio:lifesteal": 2 } }))), vec![("lifesteal".to_string(), 2)]);
        assert_eq!(problems(&data(json!({ "items": ["hat"], "hitActions": [{}] }))).len(), 1);
        assert_eq!(problems(&data(json!({}))).len(), 1);
        assert!((chance(&data(json!({ "chance": 25 }))) - 0.25).abs() < 1e-9);
    }
}

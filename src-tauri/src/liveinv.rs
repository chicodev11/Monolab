use crate::nbt::InventoryItem;
use serde::Deserialize;

const FRESH_MS: f64 = 30_000.0;

#[derive(Deserialize)]
struct File {
    at: f64,
    #[serde(default)]
    players: Vec<Person>,
}

#[derive(Deserialize)]
struct Person {
    #[serde(default)]
    name: String,
    #[serde(default)]
    uuid: String,
    #[serde(default)]
    items: Vec<(i64, String, i64, i64)>,
    #[serde(default)]
    ender: Vec<(i64, String, i64, i64)>,
}

pub struct LiveInventory {
    pub name: String,
    pub items: Vec<InventoryItem>,
    pub ender: Vec<InventoryItem>,
}

fn plain(uuid: &str) -> String {
    uuid.chars().filter(|c| *c != '-').collect::<String>().to_ascii_lowercase()
}

fn items_of(list: Vec<(i64, String, i64, i64)>) -> Vec<InventoryItem> {
    list.into_iter()
        .filter(|(_, id, count, _)| !id.is_empty() && *count > 0)
        .map(|(slot, id, count, damage)| InventoryItem {
            id,
            count,
            slot,
            name: String::new(),
            damage: damage.max(0),
            max_damage: 0,
            enchantments: Vec::new(),
        })
        .collect()
}

pub fn find(text: &str, uuid: &str, now_ms: f64) -> Option<LiveInventory> {
    let file: File = serde_json::from_str(text).ok()?;
    if now_ms - file.at > FRESH_MS {
        return None;
    }
    let want = plain(uuid);
    let person = file.players.into_iter().find(|one| plain(&one.uuid) == want)?;
    Some(LiveInventory {
        name: person.name,
        items: items_of(person.items),
        ender: items_of(person.ender),
    })
}

pub fn merge(live: Vec<InventoryItem>, saved: &[InventoryItem]) -> Vec<InventoryItem> {
    live.into_iter()
        .map(|mut item| {
            if let Some(old) = saved.iter().find(|one| one.slot == item.slot && one.id == item.id) {
                item.name = old.name.clone();
                item.enchantments = old.enchantments.clone();
                item.max_damage = old.max_damage;
            }
            item
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = r#"{"at":100000,"players":[{"name":"Steve","uuid":"0F2A-11","items":[[0,"minecraft:diamond_sword",1,61],[102,"minecraft:iron_chestplate",1,0],[5,"minecraft:air",1,0]],"ender":[[26,"minecraft:ender_pearl",16,0]]}]}"#;

    #[test]
    fn finds_a_fresh_player_by_uuid() {
        let found = find(TEXT, "0f2a11", 110_000.0).unwrap();
        assert_eq!(found.name, "Steve");
        assert_eq!(found.items.len(), 3);
        assert_eq!((found.items[0].slot, found.items[0].damage), (0, 61));
        assert_eq!((found.ender[0].slot, found.ender[0].count), (26, 16));
        assert!(find(TEXT, "ffff", 110_000.0).is_none());
    }

    #[test]
    fn ignores_an_old_file() {
        assert!(find(TEXT, "0f2a11", 200_000.0).is_none());
        assert!(find("not json", "0f2a11", 100_000.0).is_none());
    }

    #[test]
    fn keeps_names_and_enchantments_only_for_the_same_item() {
        let live = find(TEXT, "0f2a11", 100_000.0).unwrap().items;
        let mut sword = live[0].clone();
        sword.name = "Excalibur".into();
        sword.enchantments = vec![crate::nbt::Enchantment { id: "minecraft:sharpness".into(), level: 5 }];
        let mut moved = live[1].clone();
        moved.id = "minecraft:diamond_chestplate".into();
        moved.name = "Old".into();
        let merged = merge(live, &[sword, moved]);
        assert_eq!(merged[0].name, "Excalibur");
        assert_eq!(merged[0].damage, 61);
        assert_eq!(merged[0].enchantments.len(), 1);
        assert_eq!(merged[1].name, "");
    }
}

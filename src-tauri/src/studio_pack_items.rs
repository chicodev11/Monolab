//! Studio maker M1 (`docs/studio-maker.md`): the resource pack files that give studio items (`studio/item/*.json`)
//! their look. The item's `look` names a studio texture or model asset (M0, `studio_pack.rs`); this adds what points
//! the item at it.
//!
//! - 1.21.4 and later: an item model definition `assets/monolab/items/<id>.json` (the add-on sets the item's
//!   `item_model` to `monolab:<id>`) and the model `assets/monolab/models/item/<id>.json`.
//! - Older: `custom_model_data` overrides on the base item's own model (`assets/minecraft/models/item/<material>.json`),
//!   numbers from `ids.json` (kind `cmd`, from 10000) so they never move. The add-on reads the same `ids.json`.
//! - Armor (`equip.texture` / `equip.legsTexture`, 1.21.2 and later): an equipment model `monolab:<id>` and its
//!   textures, copied from the studio texture assets ([`item_copies`]).

use crate::studio_pack::{pack_format, Ids, NAMESPACE};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use studio_format::item::{self, Look};

/// The first custom model data number studio items get.
pub const FIRST_CMD: u32 = 10000;
/// Pack format 46 = 1.21.4, where `items/` model definitions came in.
const DEFINITIONS_FROM: u32 = 46;
/// Pack format 42 = 1.21.2, where equipment models came in.
const EQUIPMENT_FROM: u32 = 42;
/// How far along a use each pulling frame starts, like the vanilla bow.
const PULL_AT: [f64; 2] = [0.65, 0.9];

type Data = Map<String, Value>;

fn asset(name: &str) -> String {
    format!("{NAMESPACE}:studio/{name}")
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec_pretty(value).unwrap_or_default()
}

fn material_of(data: &Data) -> String {
    let raw = data.get("material").and_then(Value::as_str).unwrap_or("stone").trim().to_ascii_lowercase();
    raw.strip_prefix("minecraft:").unwrap_or(&raw).to_string()
}

/// A flat (or held) item model drawing one studio texture.
fn flat(texture: &str, handheld: bool) -> Value {
    json!({ "parent": if handheld { "minecraft:item/handheld" } else { "minecraft:item/generated" }, "textures": { "layer0": asset(texture) } })
}

/// The models one item needs, by model name under `monolab:item/`: the base, then each pulling frame.
fn models(id: &str, material: &str, look: &Look) -> Vec<(String, Value)> {
    let handheld = look.handheld.unwrap_or_else(|| item::handheld_material(material));
    let base = if look.is_model() { json!({ "parent": asset(&look.model) }) } else { flat(&look.texture, handheld) };
    let mut out = vec![(id.to_string(), base)];
    for (at, frame) in look.pulling.iter().enumerate() {
        out.push((format!("{id}_pulling_{at}"), flat(frame, handheld)));
    }
    out
}

fn model_ref(name: &str) -> Value {
    json!({ "type": "minecraft:model", "model": format!("{NAMESPACE}:item/{name}") })
}

/// The 1.21.4+ item model definition: the base model, or while used the pulling frames by how long it is held.
fn definition(id: &str, look: &Look) -> Value {
    if look.pulling.is_empty() {
        return json!({ "model": model_ref(id) });
    }
    let entries: Vec<Value> = PULL_AT
        .iter()
        .zip(1..look.pulling.len())
        .map(|(threshold, at)| json!({ "threshold": threshold, "model": model_ref(&format!("{id}_pulling_{at}")) }))
        .collect();
    json!({
        "model": {
            "type": "minecraft:condition",
            "property": "minecraft:using_item",
            "on_false": model_ref(id),
            "on_true": {
                "type": "minecraft:range_dispatch",
                "property": "minecraft:use_duration",
                "scale": 0.05,
                "entries": entries,
                "fallback": model_ref(&format!("{id}_pulling_0")),
            },
        }
    })
}

/// The armor textures an item wears: (equipment layer, studio texture name).
fn armor_layers(data: &Data) -> Vec<(&'static str, String)> {
    let Some(equip) = item::part(data, "equip") else { return Vec::new() };
    let mut out = Vec::new();
    for (layer, key) in [("humanoid", "texture"), ("humanoid_leggings", "legsTexture")] {
        let name = equip.get(key).and_then(Value::as_str).unwrap_or_default().trim();
        if studio_format::valid_id(name) {
            out.push((layer, name.to_string()));
        }
    }
    out
}

/// Items worth looking at: a good id. Bad files are the add-on's to report.
fn usable(items: &[(String, Data)]) -> impl Iterator<Item = &(String, Data)> {
    items.iter().filter(|(id, data)| studio_format::valid_id(id) && studio_format::enabled(data))
}

/// The pack files for the items' looks and armor, sorted by path. Paths are inside the zip.
pub fn item_files(version: &str, items: &[(String, Data)], ids: &mut Ids) -> Vec<(String, Vec<u8>)> {
    let format = pack_format(version);
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    // Older versions: material → (number, item id, look), all overrides of one base item go in one file.
    let mut overrides: BTreeMap<String, Vec<(u32, String, Look)>> = BTreeMap::new();
    for (id, data) in usable(items) {
        if let Some(look) = item::look(data).filter(|_| item::problems(data).iter().all(|one| !one.starts_with("\"look"))) {
            let material = material_of(data);
            for (name, model) in models(id, &material, &look) {
                files.insert(format!("assets/{NAMESPACE}/models/item/{name}.json"), bytes(&model));
            }
            if format >= DEFINITIONS_FROM {
                files.insert(format!("assets/{NAMESPACE}/items/{id}.json"), bytes(&definition(id, &look)));
            } else {
                let number = ids.number("cmd", id, FIRST_CMD);
                overrides.entry(material).or_default().push((number, id.clone(), look));
            }
        }
        let layers = armor_layers(data);
        if format >= EQUIPMENT_FROM && !layers.is_empty() {
            let mut model = Map::new();
            for (layer, _) in &layers {
                model.insert((*layer).to_string(), json!([{ "texture": format!("{NAMESPACE}:{id}") }]));
            }
            let path = if format >= DEFINITIONS_FROM {
                format!("assets/{NAMESPACE}/equipment/{id}.json")
            } else {
                format!("assets/{NAMESPACE}/models/equipment/{id}.json")
            };
            files.insert(path, bytes(&json!({ "layers": model })));
        }
    }
    for (material, mut list) in overrides {
        list.sort_by_key(|(number, _, _)| *number);
        let mut entries = Vec::new();
        for (number, id, look) in &list {
            entries.push(json!({ "predicate": { "custom_model_data": number }, "model": format!("{NAMESPACE}:item/{id}") }));
            for at in 0..look.pulling.len() {
                let mut predicate = json!({ "custom_model_data": number, "pulling": 1 });
                if at > 0 {
                    predicate["pull"] = json!(PULL_AT[at - 1]);
                }
                entries.push(json!({ "predicate": predicate, "model": format!("{NAMESPACE}:item/{id}_pulling_{at}") }));
            }
        }
        // The base item keeps a plain look of its own; block items lose their 3D one (the field hint says so).
        let parent = if item::handheld_material(&material) { "minecraft:item/handheld" } else { "minecraft:item/generated" };
        let model = json!({ "parent": parent, "textures": { "layer0": format!("minecraft:item/{material}") }, "overrides": entries });
        files.insert(format!("assets/minecraft/models/item/{material}.json"), bytes(&model));
    }
    files.into_iter().collect()
}

/// Studio textures the pack needs a second copy of, where the game looks for armor: (texture asset name, path in the
/// zip). The caller reads `studio/assets/textures/<name>.png` and adds it at that path.
pub fn item_copies(version: &str, items: &[(String, Data)]) -> Vec<(String, String)> {
    if pack_format(version) < EQUIPMENT_FROM {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (id, data) in usable(items) {
        for (layer, texture) in armor_layers(data) {
            out.push((texture, format!("assets/{NAMESPACE}/textures/entity/equipment/{layer}/{id}.png")));
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(list: Value) -> Vec<(String, Data)> {
        list.as_object().unwrap().iter().map(|(id, data)| (id.clone(), data.as_object().cloned().unwrap())).collect()
    }

    fn read(files: &[(String, Vec<u8>)], path: &str) -> Value {
        let (_, bytes) = files
            .iter()
            .find(|(one, _)| one == path)
            .unwrap_or_else(|| panic!("no {path} in {:?}", files.iter().map(|one| &one.0).collect::<Vec<_>>()));
        serde_json::from_slice(bytes).unwrap()
    }

    #[test]
    fn new_versions_get_definitions() {
        let list = items(json!({
            "ruby_sword": { "material": "diamond_sword", "look": { "texture": "ruby_sword" } },
            "gem": { "material": "paper", "look": { "model": "gem" } },
            "plain": { "material": "stone" },
            "Bad": { "look": { "texture": "x" } },
            "off": { "enabled": false, "look": { "texture": "x" } },
        }));
        let mut ids = Ids::default();
        let files = item_files("1.21.4", &list, &mut ids);
        assert_eq!(files.len(), 4, "{:?}", files.iter().map(|one| &one.0).collect::<Vec<_>>());
        assert_eq!(read(&files, "assets/monolab/items/ruby_sword.json")["model"]["model"], "monolab:item/ruby_sword");
        let sword = read(&files, "assets/monolab/models/item/ruby_sword.json");
        assert_eq!(sword["parent"], "minecraft:item/handheld");
        assert_eq!(sword["textures"]["layer0"], "monolab:studio/ruby_sword");
        assert_eq!(read(&files, "assets/monolab/models/item/gem.json")["parent"], "monolab:studio/gem");
        assert!(ids.given.is_empty(), "no numbers on 1.21.4+");
        // Same input, same files.
        assert_eq!(files, item_files("1.21.4", &list, &mut ids));
    }

    #[test]
    fn pulling_frames() {
        let list = items(
            json!({ "bow": { "material": "bow", "look": { "texture": "bow", "handheld": false, "pulling": ["bow_0", "bow_1", "bow_2"] } } }),
        );
        let files = item_files("1.21.8", &list, &mut Ids::default());
        let model = read(&files, "assets/monolab/items/bow.json")["model"].clone();
        assert_eq!(model["type"], "minecraft:condition");
        assert_eq!(model["on_true"]["fallback"]["model"], "monolab:item/bow_pulling_0");
        assert_eq!(model["on_true"]["entries"][1]["threshold"], 0.9);
        assert_eq!(model["on_true"]["entries"][1]["model"]["model"], "monolab:item/bow_pulling_2");
        assert_eq!(read(&files, "assets/monolab/models/item/bow_pulling_1.json")["parent"], "minecraft:item/generated");

        let old = item_files("1.21.1", &list, &mut Ids::default());
        let overrides = read(&old, "assets/minecraft/models/item/bow.json")["overrides"].clone();
        assert_eq!(overrides.as_array().unwrap().len(), 4);
        assert_eq!(overrides[2]["predicate"]["pull"], 0.65);
    }

    #[test]
    fn old_versions_get_numbers_that_stay() {
        let list = items(json!({
            "a": { "material": "paper", "look": { "texture": "a" } },
            "b": { "material": "minecraft:paper", "look": { "texture": "b" } },
            "c": { "material": "stick", "look": { "texture": "c" } },
        }));
        let mut ids = Ids::default();
        let files = item_files("1.21.1", &list, &mut ids);
        assert!(!files.iter().any(|(path, _)| path.contains("/items/")));
        let paper = read(&files, "assets/minecraft/models/item/paper.json");
        assert_eq!(paper["textures"]["layer0"], "minecraft:item/paper");
        assert_eq!(paper["overrides"][0]["predicate"]["custom_model_data"], 10000);
        assert_eq!(paper["overrides"][1]["model"], "monolab:item/b");
        assert_eq!(read(&files, "assets/minecraft/models/item/stick.json")["parent"], "minecraft:item/handheld");
        // Removing one never moves the others or hands its number on.
        let fewer = items(
            json!({ "b": { "material": "paper", "look": { "texture": "b" } }, "d": { "material": "paper", "look": { "texture": "d" } } }),
        );
        let files = item_files("1.21.1", &fewer, &mut ids);
        let paper = read(&files, "assets/minecraft/models/item/paper.json");
        assert_eq!(paper["overrides"][0]["predicate"]["custom_model_data"], 10001);
        assert_eq!(paper["overrides"][1]["predicate"]["custom_model_data"], 10003);
    }

    #[test]
    fn armor() {
        let list = items(
            json!({ "ruby_helmet": { "material": "leather_helmet", "equip": { "slot": "head", "texture": "ruby_armor", "legsTexture": "ruby_legs" } } }),
        );
        let files = item_files("1.21.4", &list, &mut Ids::default());
        let model = read(&files, "assets/monolab/equipment/ruby_helmet.json");
        assert_eq!(model["layers"]["humanoid"][0]["texture"], "monolab:ruby_helmet");
        assert_eq!(model["layers"]["humanoid_leggings"][0]["texture"], "monolab:ruby_helmet");
        assert!(item_files("1.21.3", &list, &mut Ids::default())
            .iter()
            .any(|(path, _)| path == "assets/monolab/models/equipment/ruby_helmet.json"));
        assert!(item_files("1.21.1", &list, &mut Ids::default()).is_empty());
        assert_eq!(
            item_copies("1.21.4", &list),
            vec![
                ("ruby_armor".to_string(), "assets/monolab/textures/entity/equipment/humanoid/ruby_helmet.png".to_string()),
                ("ruby_legs".to_string(), "assets/monolab/textures/entity/equipment/humanoid_leggings/ruby_helmet.png".to_string()),
            ]
        );
        assert!(item_copies("1.21.1", &list).is_empty());
    }
}

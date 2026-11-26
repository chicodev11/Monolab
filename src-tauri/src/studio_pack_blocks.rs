//! Studio maker M3 (`docs/studio-maker.md`): the resource pack files for studio blocks (`studio/block/*.json`) and
//! furniture (`studio/furniture/*.json`).
//!
//! - Blocks borrow vanilla states (`studio_format::block::state`), numbered in `ids.json` (kinds `block`, `plant`).
//!   The pack replaces the borrowed block's blockstate file (`assets/minecraft/blockstates/<block>.json`) with one
//!   entry per state: ours point at `monolab:block/<id>`, every other state keeps its vanilla look. The files list
//!   every state by name, so no entry overlaps another.
//! - A block's own item (no `item` link) and furniture items: `assets/monolab/items/{block,furniture}/<id>.json`
//!   on 1.21.4+ (the add-on sets `item_model` to `monolab:block/<id>` / `monolab:furniture/<id>`), custom model data
//!   on the base item before (`paper` for blocks — not a block item, so the client never puts one down itself —
//!   and `flint` for furniture; numbers in `ids.json` kind `cmd`, named
//!   `block:<id>` / `furniture:<id>`).
//!
//! Before 1.21.4 an M1 item on the same base item writes the same file; [`merge`] joins their overrides.

use crate::studio_pack::{pack_format, Ids, NAMESPACE};
use crate::studio_pack_items::FIRST_CMD;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use studio_format::block::{self, Look, State};
use studio_format::{furniture, item};

/// Pack format 46 = 1.21.4, where `items/` model definitions came in.
const DEFINITIONS_FROM: u32 = 46;
/// Note block instruments past [`block::INSTRUMENTS`] (mob heads, 1.19.3 and 1.20): never ours, listed so they keep
/// their look. Every version the pack supports (1.20.5+) has them.
const MORE_INSTRUMENTS: [&str; 7] = ["zombie", "skeleton", "creeper", "dragon", "wither_skeleton", "piglin", "custom_head"];

type Data = Map<String, Value>;
type Files = BTreeMap<String, Vec<u8>>;

fn asset(name: &str) -> String {
    format!("{NAMESPACE}:studio/{name}")
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec_pretty(value).unwrap_or_default()
}

fn usable(list: &[(String, Data)]) -> impl Iterator<Item = &(String, Data)> {
    list.iter().filter(|(id, data)| studio_format::valid_id(id) && studio_format::enabled(data))
}

/// The block's model: a cube from its textures, a cross for plants, or the studio model asset.
fn block_model(shape: &str, look: &Look) -> Value {
    if !look.model.is_empty() {
        return json!({ "parent": asset(&look.model) });
    }
    if shape == "plant" {
        return json!({ "parent": "minecraft:block/cross", "textures": { "cross": asset(&look.texture), "particle": asset(&look.texture) } });
    }
    if look.top.is_empty() && look.bottom.is_empty() {
        return json!({ "parent": "minecraft:block/cube_all", "textures": { "all": asset(&look.texture) } });
    }
    let top = if look.top.is_empty() { &look.texture } else { &look.top };
    let bottom = if look.bottom.is_empty() { top } else { &look.bottom };
    json!({ "parent": "minecraft:block/cube_bottom_top", "textures": { "side": asset(&look.texture), "top": asset(top), "bottom": asset(bottom), "particle": asset(&look.texture) } })
}

/// Every state of a borrowed block as (variant key, vanilla model and y turn), in a fixed order.
fn vanilla_states(block: &str) -> Vec<(String, String, u32)> {
    match block {
        "note_block" => {
            let mut out = Vec::new();
            for instrument in block::INSTRUMENTS.iter().chain(MORE_INSTRUMENTS.iter()) {
                for note in 0..block::NOTES {
                    for powered in [false, true] {
                        out.push((
                            format!("instrument={instrument},note={note},powered={powered}"),
                            "minecraft:block/note_block".to_string(),
                            0,
                        ));
                    }
                }
            }
            out
        }
        "tripwire" => (0..128u32)
            .map(|bits| {
                let on = |name: &str| bits >> block::WIRE.iter().position(|one| *one == name).unwrap_or(0) & 1 == 1;
                let key = block::WIRE.iter().map(|name| format!("{name}={}", on(name))).collect::<Vec<_>>().join(",");
                let (model, turn) = wire_model(on("north"), on("east"), on("south"), on("west"));
                let attached = if on("attached") { "attached_" } else { "" };
                (key, format!("minecraft:block/tripwire_{attached}{model}"), turn)
            })
            .collect(),
        mushroom => (0..64u32)
            .map(|bits| {
                let key = block::FACES
                    .iter()
                    .enumerate()
                    .map(|(at, name)| format!("{name}={}", bits >> at & 1 == 1))
                    .collect::<Vec<_>>()
                    .join(",");
                (key, format!("{NAMESPACE}:block/vanilla/{mushroom}_{bits}"), 0)
            })
            .collect(),
    }
}

/// The vanilla tripwire model for which sides have string: (`n`, `ne`, `ns`, `nse`, `nsew`) and its y turn.
fn wire_model(north: bool, east: bool, south: bool, west: bool) -> (&'static str, u32) {
    match (north, east, south, west) {
        (false, false, false, false) | (true, false, true, false) => ("ns", 0),
        (false, true, false, true) => ("ns", 90),
        (true, false, false, false) => ("n", 0),
        (false, true, false, false) => ("n", 90),
        (false, false, true, false) => ("n", 180),
        (false, false, false, true) => ("n", 270),
        (true, true, false, false) => ("ne", 0),
        (false, true, true, false) => ("ne", 90),
        (false, false, true, true) => ("ne", 180),
        (true, false, false, true) => ("ne", 270),
        (true, true, true, false) => ("nse", 0),
        (false, true, true, true) => ("nse", 90),
        (true, false, true, true) => ("nse", 180),
        (true, true, false, true) => ("nse", 270),
        (true, true, true, true) => ("nsew", 0),
    }
}

/// A mushroom block's vanilla look for one face mix: skin where the face is on, the inside where it is off.
fn mushroom_model(mushroom: &str, bits: u32) -> Value {
    let mut textures = Map::new();
    for (at, face) in block::FACES.iter().enumerate() {
        let texture =
            if bits >> at & 1 == 1 { format!("minecraft:block/{mushroom}") } else { "minecraft:block/mushroom_block_inside".to_string() };
        textures.insert((*face).to_string(), json!(texture));
    }
    textures.insert("particle".into(), json!(format!("minecraft:block/{mushroom}")));
    json!({ "parent": "minecraft:block/cube", "textures": textures })
}

/// A blockstate file listing every state: ours by `ours` (variant key → our model), the rest vanilla. Written by hand
/// so the entries keep this order.
fn blockstate(block: &str, ours: &BTreeMap<String, String>) -> Vec<u8> {
    let mut out = String::from("{\n  \"variants\": {");
    for (at, (key, vanilla, turn)) in vanilla_states(block).into_iter().enumerate() {
        let entry = match ours.get(&key) {
            Some(model) => json!({ "model": model }),
            None if turn > 0 => json!({ "model": vanilla, "y": turn }),
            None => json!({ "model": vanilla }),
        };
        out.push_str(if at == 0 { "\n    " } else { ",\n    " });
        out.push_str(&serde_json::to_string(&key).unwrap_or_default());
        out.push_str(": ");
        out.push_str(&entry.to_string());
    }
    out.push_str("\n  }\n}\n");
    out.into_bytes()
}

fn model_ref(model: &str) -> Value {
    json!({ "model": { "type": "minecraft:model", "model": model } })
}

/// Old-version overrides on one base item, gathered before writing: base → (number, model).
type Overrides = BTreeMap<&'static str, Vec<(u32, String)>>;

fn write_overrides(files: &mut Files, overrides: Overrides) {
    for (base, mut list) in overrides {
        list.sort();
        let entries: Vec<Value> =
            list.iter().map(|(number, model)| json!({ "predicate": { "custom_model_data": number }, "model": model })).collect();
        let model = json!({ "parent": "minecraft:item/generated", "textures": { "layer0": format!("minecraft:item/{base}") }, "overrides": entries });
        files.insert(format!("assets/minecraft/models/item/{base}.json"), bytes(&model));
    }
}

/// The pack files for studio blocks and furniture, sorted by path. Paths are inside the zip. Numbers given out go
/// into `ids`; a block past the last free state gets a number but no state (the add-on reports it).
pub fn block_files(version: &str, blocks: &[(String, Data)], furniture_list: &[(String, Data)], ids: &mut Ids) -> Vec<(String, Vec<u8>)> {
    let format = pack_format(version);
    let mut files = Files::new();
    let mut overrides = Overrides::new();
    // Borrowed block → variant key → our model.
    let mut borrowed: BTreeMap<&'static str, BTreeMap<String, String>> = BTreeMap::new();
    for (id, data) in usable(blocks) {
        if !block::problems(data).is_empty() {
            continue;
        }
        let Some(look) = block::look(data) else { continue };
        let shape = block::shape(data);
        let model = format!("{NAMESPACE}:block/{id}");
        files.insert(format!("assets/{NAMESPACE}/models/block/{id}.json"), bytes(&block_model(&shape, &look)));
        let number = ids.number(block::ids_kind(&shape), id, block::FIRST);
        if let Some(State { block, properties }) = block::state(&shape, number) {
            let key = properties.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join(",");
            borrowed.entry(block).or_default().insert(key, model.clone());
        }
        // A crop's later stages: a model and a borrowed plant state each.
        if let Some(growth) = block::growth(data) {
            for (at, name) in growth.stages.iter().enumerate() {
                let stage = at as u32 + 1;
                let stage_look = if look.model.is_empty() {
                    Look { texture: name.clone(), ..Look::default() }
                } else {
                    Look { model: name.clone(), ..Look::default() }
                };
                files.insert(format!("assets/{NAMESPACE}/models/block/stage/{id}/{stage}.json"), bytes(&block_model(&shape, &stage_look)));
                let number = ids.number(block::IDS_PLANT, &block::stage_key(id, stage), block::FIRST);
                if let Some(State { block, properties }) = block::state(&shape, number) {
                    let key = properties.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join(",");
                    borrowed.entry(block).or_default().insert(key, format!("{NAMESPACE}:block/stage/{id}/{stage}"));
                }
            }
        }
        if !item_link(data).is_empty() {
            continue;
        }
        // Its own item: the block itself in the hand, or for a plant its picture.
        let item_model = if shape == "plant" && look.model.is_empty() {
            let flat = format!("{NAMESPACE}:item/block/{id}");
            files.insert(
                format!("assets/{NAMESPACE}/models/item/block/{id}.json"),
                bytes(&json!({ "parent": "minecraft:item/generated", "textures": { "layer0": asset(&look.texture) } })),
            );
            flat
        } else {
            model
        };
        if format >= DEFINITIONS_FROM {
            files.insert(format!("assets/{NAMESPACE}/items/block/{id}.json"), bytes(&model_ref(&item_model)));
        } else {
            let base = if shape == "plant" { block::PLANT_ITEM } else { block::CUBE_ITEM };
            overrides.entry(base).or_default().push((ids.number("cmd", &format!("block:{id}"), FIRST_CMD), item_model));
        }
    }
    for (block, ours) in &borrowed {
        files.insert(format!("assets/minecraft/blockstates/{block}.json"), blockstate(block, ours));
        if block::MUSHROOMS.contains(block) {
            for bits in 0..64 {
                files.insert(format!("assets/{NAMESPACE}/models/block/vanilla/{block}_{bits}.json"), bytes(&mushroom_model(block, bits)));
            }
        }
    }
    for (id, data) in usable(furniture_list) {
        if !furniture::problems(data).iter().all(|one| !one.starts_with("\"look") && !one.starts_with("Pick")) {
            continue;
        }
        let Some(look) = item::look(data) else { continue };
        let model = if look.is_model() {
            json!({ "parent": asset(&look.model) })
        } else {
            json!({ "parent": "minecraft:item/generated", "textures": { "layer0": asset(&look.texture) } })
        };
        let name = format!("{NAMESPACE}:furniture/{id}");
        files.insert(format!("assets/{NAMESPACE}/models/furniture/{id}.json"), bytes(&model));
        if format >= DEFINITIONS_FROM {
            files.insert(format!("assets/{NAMESPACE}/items/furniture/{id}.json"), bytes(&model_ref(&name)));
        } else {
            // The display shows this number even when the piece is placed with a linked studio item.
            overrides
                .entry(furniture::ITEM)
                .or_default()
                .push((ids.number("cmd", &format!("{}{id}", furniture::CMD_PREFIX), FIRST_CMD), name));
        }
    }
    write_overrides(&mut files, overrides);
    files.into_iter().collect()
}

/// The studio item that places a block or piece of furniture, or empty for its own item.
pub fn item_link(data: &Data) -> String {
    data.get("item").and_then(Value::as_str).unwrap_or_default().trim().to_string()
}

/// Adds `more` to `files`: a path both have whose files carry `overrides` (two features on one base item before
/// 1.21.4) gets one file with both lists, sorted by number; any other repeat keeps the first.
pub fn merge(files: &mut Vec<(String, Vec<u8>)>, more: Vec<(String, Vec<u8>)>) {
    for (path, extra) in more {
        let Some(at) = files.iter().position(|(one, _)| *one == path) else {
            files.push((path, extra));
            continue;
        };
        let (Ok(mut was), Ok(add)) = (serde_json::from_slice::<Value>(&files[at].1), serde_json::from_slice::<Value>(&extra)) else {
            continue;
        };
        let (Some(list), Some(add_list)) =
            (was.get("overrides").and_then(Value::as_array).cloned(), add.get("overrides").and_then(Value::as_array))
        else {
            continue;
        };
        let mut joined: Vec<Value> = list.into_iter().chain(add_list.iter().cloned()).collect();
        joined.sort_by_key(|one| one["predicate"]["custom_model_data"].as_u64().unwrap_or(0));
        was["overrides"] = Value::Array(joined);
        files[at].1 = bytes(&was);
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(value: Value) -> Vec<(String, Data)> {
        value.as_object().unwrap().iter().map(|(id, data)| (id.clone(), data.as_object().cloned().unwrap())).collect()
    }

    fn read(files: &[(String, Vec<u8>)], path: &str) -> Value {
        let (_, bytes) = files
            .iter()
            .find(|(one, _)| one == path)
            .unwrap_or_else(|| panic!("no {path} in {:?}", files.iter().map(|one| &one.0).collect::<Vec<_>>()));
        serde_json::from_slice(bytes).unwrap()
    }

    #[test]
    fn blocks_borrow_states() {
        let blocks = list(json!({
            "ruby_ore": { "look": { "texture": "ruby_ore" } },
            "log": { "look": { "texture": "bark", "top": "rings" }, "item": "magic_log" },
            "fern": { "shape": "plant", "look": { "texture": "fern" } },
            "broken": { "shape": "slab", "look": { "texture": "x" } },
        }));
        let mut ids = Ids::default();
        let files = block_files("1.21.4", &blocks, &[], &mut ids);
        // Numbers in file order (sorted ids): fern is the first plant, log then ruby_ore the first cubes.
        assert_eq!(ids.given["block"]["log"], 1);
        assert_eq!(ids.given["block"]["ruby_ore"], 2);
        assert_eq!(ids.given["plant"]["fern"], 1);
        let notes = read(&files, "assets/minecraft/blockstates/note_block.json");
        let variants = notes["variants"].as_object().unwrap();
        assert_eq!(variants.len(), (block::INSTRUMENTS.len() + MORE_INSTRUMENTS.len()) * 50);
        assert_eq!(variants["instrument=harp,note=0,powered=true"]["model"], "monolab:block/log");
        assert_eq!(variants["instrument=harp,note=1,powered=false"]["model"], "monolab:block/ruby_ore");
        assert_eq!(variants["instrument=harp,note=0,powered=false"]["model"], "minecraft:block/note_block");
        assert_eq!(variants["instrument=piglin,note=3,powered=true"]["model"], "minecraft:block/note_block");
        let wire = read(&files, "assets/minecraft/blockstates/tripwire.json");
        let wires = wire["variants"].as_object().unwrap();
        assert_eq!(wires.len(), 128);
        assert_eq!(
            wires["attached=true,disarmed=false,east=false,north=false,powered=false,south=false,west=false"]["model"],
            "monolab:block/fern"
        );
        assert_eq!(
            wires["attached=false,disarmed=false,east=true,north=false,powered=false,south=false,west=false"],
            json!({ "model": "minecraft:block/tripwire_n", "y": 90 })
        );
        assert_eq!(read(&files, "assets/monolab/models/block/log.json")["parent"], "minecraft:block/cube_bottom_top");
        assert_eq!(read(&files, "assets/monolab/models/block/log.json")["textures"]["bottom"], "monolab:studio/rings");
        assert_eq!(read(&files, "assets/monolab/models/block/fern.json")["parent"], "minecraft:block/cross");
        // Own items only for blocks without a linked studio item.
        assert_eq!(read(&files, "assets/monolab/items/block/ruby_ore.json")["model"]["model"], "monolab:block/ruby_ore");
        assert_eq!(read(&files, "assets/monolab/items/block/fern.json")["model"]["model"], "monolab:item/block/fern");
        assert!(!files.iter().any(|(path, _)| path.contains("items/block/log")));
        assert!(!files.iter().any(|(path, _)| path.contains("broken")));
        assert!(!files.iter().any(|(path, _)| path.contains("mushroom")), "no mushroom states until the note block runs out");
        // The variant keys keep the written order (the first is the first vanilla state).
        let text = String::from_utf8(files.iter().find(|(path, _)| path.ends_with("note_block.json")).unwrap().1.clone()).unwrap();
        assert!(text.find("note=0,powered=false").unwrap() < text.find("note=0,powered=true").unwrap());
        assert_eq!(files, block_files("1.21.4", &blocks, &[], &mut ids));
    }

    #[test]
    fn crops_borrow_a_state_per_stage() {
        let blocks = list(json!({ "wheat": { "shape": "plant", "look": { "texture": "w0" }, "growth": { "stages": ["w1", "w2"] } } }));
        let mut ids = Ids::default();
        let files = block_files("1.21.4", &blocks, &[], &mut ids);
        assert_eq!(ids.given["plant"]["wheat"], 1);
        assert_eq!(ids.given["plant"]["wheat/1"], 2);
        assert_eq!(ids.given["plant"]["wheat/2"], 3);
        assert_eq!(read(&files, "assets/monolab/models/block/stage/wheat/2.json")["textures"]["cross"], "monolab:studio/w2");
        let wires = read(&files, "assets/minecraft/blockstates/tripwire.json");
        assert_eq!(
            wires["variants"]["attached=false,disarmed=true,east=false,north=false,powered=false,south=false,west=false"]["model"],
            "monolab:block/stage/wheat/1"
        );
    }

    #[test]
    fn mushrooms_after_note_blocks() {
        let mut ids = Ids::default();
        ids.next.insert("block".into(), block::NOTE_STATES + 1);
        let files = block_files("1.21.8", &list(json!({ "cap": { "look": { "texture": "cap" } } })), &[], &mut ids);
        let state = read(&files, "assets/minecraft/blockstates/brown_mushroom_block.json");
        assert_eq!(state["variants"].as_object().unwrap().len(), 64);
        assert_eq!(state["variants"]["down=false,east=false,north=false,south=false,up=false,west=false"]["model"], "monolab:block/cap");
        assert_eq!(
            state["variants"]["down=true,east=true,north=true,south=true,up=true,west=true"]["model"],
            "monolab:block/vanilla/brown_mushroom_block_63"
        );
        let skin = read(&files, "assets/monolab/models/block/vanilla/brown_mushroom_block_1.json");
        assert_eq!(skin["textures"]["down"], "minecraft:block/brown_mushroom_block");
        assert_eq!(skin["textures"]["up"], "minecraft:block/mushroom_block_inside");
    }

    #[test]
    fn furniture_and_old_versions() {
        let blocks = list(json!({ "ruby_ore": { "look": { "texture": "ruby_ore" } } }));
        let chairs = list(
            json!({ "chair": { "look": { "model": "chair" } }, "lamp": { "look": { "texture": "lamp" }, "item": "lamp_item" }, "bad": {} }),
        );
        let files = block_files("1.21.4", &blocks, &chairs, &mut Ids::default());
        assert_eq!(read(&files, "assets/monolab/items/furniture/chair.json")["model"]["model"], "monolab:furniture/chair");
        assert_eq!(read(&files, "assets/monolab/models/furniture/chair.json")["parent"], "monolab:studio/chair");
        assert_eq!(read(&files, "assets/monolab/models/furniture/lamp.json")["parent"], "minecraft:item/generated");
        assert!(!files.iter().any(|(path, _)| path.contains("bad")));

        let mut ids = Ids::default();
        let old = block_files("1.21.1", &blocks, &chairs, &mut ids);
        assert!(!old.iter().any(|(path, _)| path.contains("/items/")));
        let notes = read(&old, "assets/minecraft/models/item/paper.json");
        assert_eq!(notes["parent"], "minecraft:item/generated");
        assert_eq!(notes["overrides"][0], json!({ "predicate": { "custom_model_data": 10000 }, "model": "monolab:block/ruby_ore" }));
        let flint = read(&old, "assets/minecraft/models/item/flint.json");
        assert_eq!(flint["overrides"].as_array().unwrap().len(), 2, "the linked lamp's display still needs its number");
        assert_eq!(ids.given["cmd"]["furniture:chair"], 10001);
    }

    #[test]
    fn merges_overrides() {
        let mut files = vec![(
            "assets/minecraft/models/item/flint.json".to_string(),
            bytes(
                &json!({ "parent": "minecraft:item/generated", "overrides": [{ "predicate": { "custom_model_data": 10002 }, "model": "a" }] }),
            ),
        )];
        let more = vec![
            (
                "assets/minecraft/models/item/flint.json".to_string(),
                bytes(&json!({ "overrides": [{ "predicate": { "custom_model_data": 10000 }, "model": "b" }] })),
            ),
            ("assets/monolab/models/block/x.json".to_string(), b"{}".to_vec()),
        ];
        merge(&mut files, more);
        assert_eq!(files.len(), 2);
        let flint = read(&files, "assets/minecraft/models/item/flint.json");
        assert_eq!(flint["overrides"][0]["model"], "b");
        assert_eq!(flint["overrides"][1]["model"], "a");
        assert_eq!(flint["parent"], "minecraft:item/generated");
    }
}

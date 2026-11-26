//! Studio maker M4 (`docs/studio-maker.md`): studio models (`studio/model/*.json`) into the resource pack, and the
//! rig the add-ons play. Each model file names a Blockbench asset (`studio/assets/models/<model>.bbmodel`):
//!
//! - every bone with cubes becomes one item model `assets/monolab/models/model/<id>/p<n>.json` and an item model
//!   definition `assets/monolab/items/model/<id>/p<n>.json` (the add-on sets `item_model` to `monolab:model/<id>/p<n>`),
//!   with the cubes given relative to the bone's pivot. Cubes turned in a way item models can't (more than one axis,
//!   or not a 22.5° step up to 45°) get a part of their own that the display entity turns instead.
//! - item model elements must stay inside -16..32, so a big bone is drawn smaller and the display scales it back up.
//! - pictures kept inside the `.bbmodel` go to `assets/monolab/textures/model/<id>/<n>.png`; a linked one is taken
//!   from the studio texture of the same name.
//! - the rig, `studio/pack/models/<id>.json` ([`RIG_FOLDER`]): per part its bone, item and a pose for the rest frame
//!   and every tick of every animation (translation, rotation, scale for the display's transformation, in blocks).
//!   The add-ons only step through frames, so the Blockbench rules live in one place (`studio_format::bbmodel`).
//!
//! Item model definitions came in 1.21.4 (pack format 46); older versions get no model files and a status line.

// TEMPORARY until studio_pack_write calls model_files (see the M4 handover); remove this line then.

use crate::studio_pack::{pack_format, Source, NAMESPACE};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use studio_format::bbmodel::{self, Model, Pose, Quat, Vec3};
use studio_format::model as rules;

/// Where rigs go, under the studio folder: `studio/pack/models/<id>.json`.
pub const RIG_FOLDER: &str = "pack/models";
/// Pack format 46 = 1.21.4.
const DEFINITIONS_FROM: u32 = 46;
/// Item model elements may reach from -16 to 32: 24 either side of the middle.
const REACH: f64 = 24.0;
/// The item model element rotations the game takes.
const ANGLES: [f64; 4] = [-45.0, -22.5, 22.5, 45.0];

type Data = Map<String, Value>;

/// What the pack builder adds: files in the zip, and rigs to write next to it (id, bytes).
#[derive(Default, Debug)]
pub struct ModelOutput {
    pub files: Vec<(String, Vec<u8>)>,
    pub rigs: Vec<(String, Vec<u8>)>,
}

/// One display entity: a bone's cubes, or one turned cube of it.
struct Part {
    bone: usize,
    /// For a turned cube: its pivot (relative to the bone's) and turn.
    own: Option<(Vec3, Quat)>,
    cubes: Vec<usize>,
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec_pretty(value).unwrap_or_default()
}

fn round(value: f64) -> f64 {
    (value * 10000.0).round() / 10000.0
}

/// The single axis and angle an item model element can take for this turn, if it can.
fn element_turn(rotation: Vec3) -> Option<Option<(&'static str, f64)>> {
    let turned: Vec<usize> = (0..3).filter(|at| rotation[*at].abs() > 1e-6).collect();
    match turned.as_slice() {
        [] => Some(None),
        [axis] if ANGLES.iter().any(|one| (one - rotation[*axis]).abs() < 1e-6) => Some(Some((["x", "y", "z"][*axis], rotation[*axis]))),
        _ => None,
    }
}

fn parts(model: &Model) -> Vec<Part> {
    let mut out = Vec::new();
    for (at, bone) in model.bones.iter().enumerate().filter(|(_, bone)| bone.visible) {
        let mut plain = Vec::new();
        for &cube in &bone.cubes {
            let one = &model.cubes[cube];
            if element_turn(one.rotation).is_some() {
                plain.push(cube);
            } else {
                let pivot = [0, 1, 2].map(|axis| one.origin[axis] - bone.origin[axis]);
                out.push(Part { bone: at, own: Some((pivot, bbmodel::euler(one.rotation))), cubes: vec![cube] });
            }
        }
        if !plain.is_empty() {
            out.push(Part { bone: at, own: None, cubes: plain });
        }
    }
    out.sort_by_key(|one| (one.bone, one.own.is_some()));
    out
}

/// The part's pivot in model pixels: the bone's, or the turned cube's own.
fn pivot(model: &Model, part: &Part) -> Vec3 {
    match part.own {
        Some(_) => model.cubes[part.cubes[0]].origin,
        None => model.bones[part.bone].origin,
    }
}

/// How much smaller the part is drawn so its cubes fit an item model (1 = as is).
fn shrink(model: &Model, part: &Part) -> f64 {
    let center = pivot(model, part);
    let reach = part.cubes.iter().flat_map(|&cube| {
        let one = &model.cubes[cube];
        (0..3).flat_map(move |axis| [(one.from[axis] - center[axis]).abs(), (one.to[axis] - center[axis]).abs()])
    });
    (reach.fold(0.0, f64::max) / REACH).max(1.0)
}

/// The item model for one part. `textures`: the model's texture index → resource name (missing = not available).
fn part_model(model: &Model, part: &Part, textures: &BTreeMap<usize, String>) -> Value {
    let center = pivot(model, part);
    let small = shrink(model, part);
    let place = |value: f64, axis: usize| round((value - center[axis]) / small + 8.0);
    let mut used = BTreeMap::new();
    let mut elements = Vec::new();
    for &cube in &part.cubes {
        let one = &model.cubes[cube];
        let mut faces = Map::new();
        for (name, face) in bbmodel::FACES.iter().zip(&one.faces) {
            let Some(face) = face else { continue };
            let Some(texture) = textures.get(&face.texture) else { continue };
            used.insert(face.texture, texture.clone());
            let size = model.textures.get(face.texture).map_or((16.0, 16.0), |one| (one.uv_width, one.uv_height));
            let uv = [face.uv[0] * 16.0 / size.0, face.uv[1] * 16.0 / size.1, face.uv[2] * 16.0 / size.0, face.uv[3] * 16.0 / size.1].map(round);
            let mut out = json!({ "uv": uv, "texture": format!("#{}", face.texture) });
            if face.rotation != 0 {
                out["rotation"] = json!(face.rotation);
            }
            faces.insert((*name).to_string(), out);
        }
        if faces.is_empty() {
            continue;
        }
        let from = [0, 1, 2].map(|axis| place(one.from[axis], axis));
        let to = [0, 1, 2].map(|axis| place(one.to[axis], axis));
        let mut element = json!({ "from": from, "to": to, "faces": faces });
        if part.own.is_none() {
            if let Some(Some((axis, angle))) = element_turn(one.rotation) {
                let origin = [0, 1, 2].map(|at| place(one.origin[at], at));
                element["rotation"] = json!({ "origin": origin, "axis": axis, "angle": angle });
            }
        }
        elements.push(element);
    }
    let mut names = Map::new();
    for (index, texture) in &used {
        names.insert(index.to_string(), json!(texture));
    }
    if let Some(first) = used.values().next() {
        names.insert("particle".into(), json!(first));
    }
    json!({ "textures": names, "elements": elements })
}

/// The pose of each part in a frame, as the display's transformation wants it (blocks, model scale applied).
fn frame(parts: &[Part], shrinks: &[f64], bones: &[Pose], scale: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(parts.len() * 10);
    for (part, small) in parts.iter().zip(shrinks) {
        let bone = bones[part.bone];
        let pose = match part.own {
            Some((offset, turn)) => bone.child(offset, turn, [1.0; 3]),
            None => bone,
        };
        out.extend(pose.translation.map(|one| one / 16.0 * scale));
        out.extend(pose.rotation);
        out.extend(pose.scale.map(|one| one * small * scale));
    }
    out.into_iter().map(round).collect()
}

/// The rig file for one model: parts, rest frame, baked animations, the seat spot, problems.
fn rig(id: &str, data: &Data, model: &Model, parts: &[Part], problems: Vec<String>) -> Value {
    let scale = rules::scale(data);
    let shrinks: Vec<f64> = parts.iter().map(|part| shrink(model, part)).collect();
    let rest_bones = bbmodel::pose(model, None);
    let mut animations = Map::new();
    for animation in &model.animations {
        let frames: Vec<Value> = (0..bbmodel::frame_count(animation))
            .map(|at| {
                let time = (at as f64 / bbmodel::FPS).min(animation.length);
                json!(frame(parts, &shrinks, &bbmodel::pose(model, Some((animation, time))), scale))
            })
            .collect();
        animations.insert(animation.name.clone(), json!({ "loop": animation.looping.name(), "length": round(animation.length), "frames": frames }));
    }
    let seat = rules::seat(data);
    let seat = model.bones.iter().position(|one| one.name == seat).map(|at| rest_bones[at].translation.map(|one| round(one / 16.0 * scale)));
    let list: Vec<Value> = parts
        .iter()
        .enumerate()
        .map(|(at, part)| json!({ "bone": model.bones[part.bone].name, "item": format!("{NAMESPACE}:model/{id}/p{at}") }))
        .collect();
    json!({
        "schema": 1,
        "id": id,
        "fps": bbmodel::FPS,
        "parts": list,
        "rest": frame(parts, &shrinks, &rest_bones, scale),
        "animations": animations,
        "seat": seat,
        "problems": problems,
    })
}

fn decode_png(source: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let data = source.strip_prefix("data:image/png;base64,")?;
    let out = base64::engine::general_purpose::STANDARD.decode(data.trim()).ok()?;
    out.starts_with(b"\x89PNG\r\n\x1a\n").then_some(out)
}

fn problem_rig(id: &str, problems: Vec<String>) -> (String, Vec<u8>) {
    (id.to_string(), bytes(&json!({ "schema": 1, "id": id, "parts": [], "rest": [], "animations": {}, "problems": problems })))
}

/// The pack files and rigs for every good model file.
pub fn model_files(version: &str, models: &[(String, Data)], sources: &[Source]) -> ModelOutput {
    let mut output = ModelOutput::default();
    let new_enough = pack_format(version) >= DEFINITIONS_FROM;
    for (id, data) in models.iter().filter(|(id, data)| studio_format::valid_id(id) && studio_format::enabled(data)) {
        let mut problems = rules::problems(data);
        if !new_enough {
            problems.insert(0, "Models need Minecraft 1.21.4 or later.".into());
            output.rigs.push(problem_rig(id, problems));
            continue;
        }
        let asset = rules::asset(data);
        let Some((_, _, source)) = sources.iter().find(|(folder, file, _)| folder == "models" && *file == format!("{asset}.bbmodel")) else {
            if !asset.is_empty() {
                problems.push(format!("There is no model asset called {asset}.bbmodel."));
            }
            output.rigs.push(problem_rig(id, problems));
            continue;
        };
        let model = match bbmodel::parse(&String::from_utf8_lossy(source)) {
            Ok(one) => one,
            Err(error) => {
                problems.push(error);
                output.rigs.push(problem_rig(id, problems));
                continue;
            }
        };
        problems.extend(model.problems.iter().cloned());
        problems.extend(rules::problems_with(data, &model));
        let mut textures = BTreeMap::new();
        for (at, texture) in model.textures.iter().enumerate() {
            if let Some(png) = decode_png(&texture.source) {
                output.files.push((format!("assets/{NAMESPACE}/textures/model/{id}/{at}.png"), png));
                textures.insert(at, format!("{NAMESPACE}:model/{id}/{at}"));
            } else if sources.iter().any(|(folder, file, _)| folder == "textures" && *file == format!("{}.png", texture.name)) {
                textures.insert(at, format!("{NAMESPACE}:studio/{}", texture.name));
            } else {
                problems.push(format!("Texture \"{}\" isn't inside the model. Add it to the studio's textures with that name.", texture.name));
            }
        }
        let parts = parts(&model);
        if parts.len() > bbmodel::MOST_BONES {
            problems.push(format!("Over {} parts; the rest are left out.", bbmodel::MOST_BONES));
        }
        let parts: Vec<Part> = parts.into_iter().take(bbmodel::MOST_BONES).collect();
        for (at, part) in parts.iter().enumerate() {
            output.files.push((format!("assets/{NAMESPACE}/models/model/{id}/p{at}.json"), bytes(&part_model(&model, part, &textures))));
            output.files.push((format!("assets/{NAMESPACE}/items/model/{id}/p{at}.json"), bytes(&json!({ "model": { "type": "minecraft:model", "model": format!("{NAMESPACE}:model/{id}/p{at}") } }))));
        }
        let rig = rig(id, data, &model, &parts, problems);
        output.rigs.push((id.clone(), serde_json::to_vec(&rig).unwrap_or_default()));
    }
    output.files.sort_by(|a, b| a.0.cmp(&b.0));
    output.rigs.sort_by(|a, b| a.0.cmp(&b.0));
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

    fn bbmodel() -> String {
        format!(
            r#"{{
            "resolution": {{"width": 16, "height": 16}},
            "elements": [
                {{"name": "body", "type": "cube", "uuid": "c1", "from": [-4, 0, -4], "to": [4, 8, 4], "origin": [0, 0, 0],
                 "faces": {{"north": {{"uv": [0, 0, 8, 8], "texture": 0}}, "south": {{"uv": [0, 0, 8, 8], "texture": 1}}}}}},
                {{"name": "tilt", "type": "cube", "uuid": "c2", "from": [0, 0, 0], "to": [2, 2, 2], "origin": [1, 1, 1], "rotation": [0, 22.5, 0],
                 "faces": {{"north": {{"uv": [0, 0, 2, 2], "texture": 0}}}}}},
                {{"name": "odd", "type": "cube", "uuid": "c3", "from": [0, 8, 0], "to": [2, 10, 2], "origin": [1, 9, 1], "rotation": [10, 30, 0],
                 "faces": {{"north": {{"uv": [0, 0, 2, 2], "texture": 0}}}}}},
                {{"name": "tail", "type": "cube", "uuid": "c4", "from": [0, 0, 0], "to": [2, 2, 80], "origin": [0, 0, 0],
                 "faces": {{"up": {{"uv": [0, 0, 16, 16], "texture": 0}}}}}}
            ],
            "outliner": [{{"name": "body", "uuid": "g1", "origin": [0, 0, 0], "children": ["c1", "c2", "c3"]}},
                         {{"name": "tail", "uuid": "g2", "origin": [0, 0, 0], "children": ["c4"]}},
                         {{"name": "saddle", "uuid": "g3", "origin": [0, 16, 0], "children": []}}],
            "textures": [{{"name": "skin", "source": "data:image/png;base64,{PNG}"}}, {{"name": "fur", "source": ""}}],
            "animations": [{{"name": "wag", "loop": "loop", "length": 0.5, "animators": {{"g2": {{"name": "tail", "keyframes": [
                {{"channel": "rotation", "time": 0, "data_points": [{{"x": 0, "y": 0, "z": 0}}]}},
                {{"channel": "rotation", "time": 0.5, "data_points": [{{"x": 0, "y": 40, "z": 0}}]}}]}}}}}}]
        }}"#
        )
    }

    fn data(value: Value) -> Vec<(String, Data)> {
        vec![("pet".to_string(), value.as_object().cloned().unwrap())]
    }

    fn find<'a>(files: &'a [(String, Vec<u8>)], path: &str) -> &'a [u8] {
        &files.iter().find(|(one, _)| one == path).unwrap_or_else(|| panic!("no {path} in {:?}", files.iter().map(|one| &one.0).collect::<Vec<_>>())).1
    }

    #[test]
    fn builds_parts_and_rig() {
        let sources = vec![("models".to_string(), "pet.bbmodel".to_string(), bbmodel().into_bytes()), ("textures".to_string(), "fur.png".to_string(), vec![])];
        let models = data(json!({ "model": "pet", "scale": 2, "seat": "saddle", "animations": { "idle": "wag" } }));
        let out = model_files("1.21.4", &models, &sources);
        // body (with the 22.5° cube inside), its odd cube on its own, the tail: 3 parts.
        assert!(out.files.iter().any(|(path, _)| path == "assets/monolab/textures/model/pet/0.png"));
        let body: Value = serde_json::from_slice(find(&out.files, "assets/monolab/models/model/pet/p0.json")).unwrap();
        assert_eq!(body["elements"].as_array().unwrap().len(), 2);
        assert_eq!(body["elements"][0]["from"], json!([4.0, 8.0, 4.0]));
        assert_eq!(body["elements"][1]["rotation"]["angle"], 22.5);
        assert_eq!(body["textures"]["0"], "monolab:model/pet/0");
        assert_eq!(body["textures"]["1"], "monolab:studio/fur", "linked texture from the studio's own");
        let odd: Value = serde_json::from_slice(find(&out.files, "assets/monolab/models/model/pet/p1.json")).unwrap();
        assert!(odd["elements"][0].get("rotation").is_none(), "the display turns it");
        let tail: Value = serde_json::from_slice(find(&out.files, "assets/monolab/models/model/pet/p2.json")).unwrap();
        let to = tail["elements"][0]["to"][2].as_f64().unwrap();
        assert!(to <= 32.0, "shrunk to fit: {to}");
        let definition: Value = serde_json::from_slice(find(&out.files, "assets/monolab/items/model/pet/p2.json")).unwrap();
        assert_eq!(definition["model"]["model"], "monolab:model/pet/p2");

        let (id, rig) = &out.rigs[0];
        assert_eq!(id, "pet");
        let rig: Value = serde_json::from_slice(rig).unwrap();
        assert_eq!(rig["parts"].as_array().unwrap().len(), 3);
        assert_eq!(rig["parts"][1]["bone"], "body");
        assert_eq!(rig["rest"].as_array().unwrap().len(), 30);
        // The tail is drawn at 24/80 size, so its display scales back up by 80/24, times the model's 2.
        let tail_scale = rig["rest"][27].as_f64().unwrap();
        assert!((tail_scale - 2.0 * 80.0 / 24.0).abs() < 1e-3, "{tail_scale}");
        assert_eq!(rig["seat"], json!([0.0, 2.0, 0.0]));
        let wag = &rig["animations"]["wag"];
        assert_eq!(wag["loop"], "loop");
        assert_eq!(wag["frames"].as_array().unwrap().len(), 11);
        assert_ne!(wag["frames"][0][24], wag["frames"][10][24], "the tail turns");
        assert!(rig["problems"].as_array().unwrap().is_empty(), "{:?}", rig["problems"]);
        // Same input, same output.
        let again = model_files("1.21.4", &models, &sources);
        assert_eq!(out.files, again.files);
    }

    #[test]
    fn problems_land_in_the_rig() {
        let models = data(json!({ "model": "missing" }));
        let out = model_files("1.21.4", &models, &[]);
        let rig: Value = serde_json::from_slice(&out.rigs[0].1).unwrap();
        assert!(rig["problems"][0].as_str().unwrap().contains("missing.bbmodel"));
        let old = model_files("1.21.1", &models, &[]);
        let rig: Value = serde_json::from_slice(&old.rigs[0].1).unwrap();
        assert!(rig["problems"][0].as_str().unwrap().contains("1.21.4"));
        assert!(old.files.is_empty());
        let sources = vec![("models".to_string(), "missing.bbmodel".to_string(), b"{\"elements\":[],\"textures\":[{\"name\":\"gone\"}]}".to_vec())];
        let out = model_files("1.21.4", &models, &sources);
        let rig: Value = serde_json::from_slice(&out.rigs[0].1).unwrap();
        let text = rig["problems"].to_string();
        assert!(text.contains("gone") && text.contains("no cubes"), "{text}");
    }
}

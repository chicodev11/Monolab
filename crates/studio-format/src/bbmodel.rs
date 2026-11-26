//! Studio maker M4 (`docs/studio-maker.md`): Blockbench `.bbmodel` files (a public JSON format), read into a bone
//! tree with cubes and keyframe animations, and posed at any time. The app's pack builder
//! (`src-tauri/src/studio_pack_models.rs`) turns each bone into one item model and bakes the poses into a rig file the
//! add-ons play back (one item display per bone), so the Paper add-on never reads `.bbmodel` itself.
//!
//! Units are Blockbench pixels (16 = one block), Y up. A bone's pose maps its cubes, given relative to the bone's
//! pivot, into model space: `point → translation + rotation * (scale ⊙ point)`. Scales multiply down the tree per
//! axis (exact for even scales, close enough for the rest).

use serde_json::{Map, Value};

/// Bones (with the parts rotated cubes need) a model may have; more is a model nobody should send as entities.
pub const MOST_BONES: usize = 128;
/// Animations are baked at the game's tick rate, and no longer than this.
pub const FPS: f64 = 20.0;
pub const MOST_SECONDS: f64 = 30.0;

type Data = Map<String, Value>;
pub type Vec3 = [f64; 3];
/// x, y, z, w
pub type Quat = [f64; 4];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Texture {
    pub name: String,
    /// `data:image/png;base64,...` when Blockbench kept the picture inside the file (the usual case). Empty = linked.
    pub source: String,
    /// The size face UVs are given in.
    pub uv_width: f64,
    pub uv_height: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// u1, v1, u2, v2 in the texture's UV size.
    pub uv: [f64; 4],
    pub texture: usize,
    /// 0, 90, 180 or 270
    pub rotation: i32,
}

/// Face order: north, east, south, west, up, down (the game's names).
pub const FACES: [&str; 6] = ["north", "east", "south", "west", "up", "down"];

#[derive(Clone, Debug, PartialEq)]
pub struct Cube {
    pub name: String,
    pub from: Vec3,
    pub to: Vec3,
    pub origin: Vec3,
    pub rotation: Vec3,
    pub faces: [Option<Face>; 6],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Bone {
    pub name: String,
    pub parent: Option<usize>,
    pub origin: Vec3,
    pub rotation: Vec3,
    pub cubes: Vec<usize>,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Rotation,
    Position,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interp {
    Linear,
    CatmullRom,
    Step,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub time: f64,
    pub value: Vec3,
    pub interp: Interp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loop {
    /// Plays once and goes back to the rest pose.
    Once,
    Loop,
    /// Plays once and stays on the last frame.
    Hold,
}

impl Loop {
    pub fn name(self) -> &'static str {
        match self {
            Loop::Once => "once",
            Loop::Loop => "loop",
            Loop::Hold => "hold",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub bone: usize,
    pub channel: Channel,
    pub keys: Vec<Key>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Animation {
    pub name: String,
    pub length: f64,
    pub looping: Loop,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub textures: Vec<Texture>,
    pub bones: Vec<Bone>,
    pub cubes: Vec<Cube>,
    pub animations: Vec<Animation>,
    /// Things read but not used (meshes, expressions in keyframes...), for the file's status.
    pub problems: Vec<String>,
}

fn num(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(one) => one.as_f64(),
        // Keyframe values are often strings ("12.5"); expressions don't parse and count as missing.
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|one| one.is_finite())
}

fn vec3(value: Option<&Value>, fallback: f64) -> Vec3 {
    let Some(Value::Array(list)) = value else { return [fallback; 3] };
    [0, 1, 2].map(|at| num(list.get(at)).unwrap_or(fallback))
}

fn text<'a>(data: &'a Data, key: &str) -> &'a str {
    data.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// Reads a `.bbmodel`. Fails only when it isn't one at all; odd parts land in [`Model::problems`].
pub fn parse(source: &str) -> Result<Model, String> {
    let root: Value = serde_json::from_str(source).map_err(|error| format!("Not a Blockbench model: {error}"))?;
    let Value::Object(root) = root else { return Err("Not a Blockbench model.".into()) };
    if !root.contains_key("elements") && !root.contains_key("outliner") {
        return Err("Not a Blockbench model (no elements).".into());
    }
    let mut model = Model::default();
    let resolution = root.get("resolution").and_then(Value::as_object);
    let width = resolution.and_then(|one| num(one.get("width"))).filter(|one| *one > 0.0).unwrap_or(16.0);
    let height = resolution.and_then(|one| num(one.get("height"))).filter(|one| *one > 0.0).unwrap_or(16.0);
    let mut texture_ids: Vec<String> = Vec::new();
    for raw in root.get("textures").and_then(Value::as_array).into_iter().flatten() {
        let Some(one) = raw.as_object() else { continue };
        texture_ids.push(text(one, "uuid").to_string());
        let source = text(one, "source");
        model.textures.push(Texture {
            name: text(one, "name").trim_end_matches(".png").to_string(),
            source: if source.starts_with("data:image/png;base64,") { source.to_string() } else { String::new() },
            uv_width: num(one.get("uv_width")).filter(|one| *one > 0.0).unwrap_or(width),
            uv_height: num(one.get("uv_height")).filter(|one| *one > 0.0).unwrap_or(height),
        });
    }

    // Cubes by uuid; meshes and locators have no item-model shape.
    let mut cube_ids: Vec<(String, usize)> = Vec::new();
    let mut skipped_meshes = 0;
    for raw in root.get("elements").and_then(Value::as_array).into_iter().flatten() {
        let Some(one) = raw.as_object() else { continue };
        let kind = text(one, "type");
        if !kind.is_empty() && kind != "cube" {
            if kind == "mesh" {
                skipped_meshes += 1;
            }
            continue;
        }
        if one.get("visibility").and_then(Value::as_bool) == Some(false) {
            cube_ids.push((text(one, "uuid").to_string(), usize::MAX));
            continue;
        }
        let inflate = num(one.get("inflate")).unwrap_or(0.0);
        let from = vec3(one.get("from"), 0.0);
        let to = vec3(one.get("to"), 0.0);
        let faces = FACES.map(|name| {
            let face = one.get("faces")?.get(name)?.as_object()?;
            let texture = match face.get("texture") {
                Some(Value::Number(at)) => at.as_u64().map(|at| at as usize),
                // Newer files may name the texture by uuid.
                Some(Value::String(id)) => texture_ids.iter().position(|one| one == id),
                _ => None,
            }?;
            let uv = match face.get("uv") {
                Some(Value::Array(list)) if list.len() == 4 => [0, 1, 2, 3].map(|at| num(list.get(at)).unwrap_or(0.0)),
                _ => return None,
            };
            let rotation = (num(face.get("rotation")).unwrap_or(0.0) as i32).rem_euclid(360) / 90 * 90;
            Some(Face { uv, texture, rotation })
        });
        cube_ids.push((text(one, "uuid").to_string(), model.cubes.len()));
        model.cubes.push(Cube {
            name: text(one, "name").to_string(),
            from: [0, 1, 2].map(|at| from[at].min(to[at]) - inflate),
            to: [0, 1, 2].map(|at| from[at].max(to[at]) + inflate),
            origin: vec3(one.get("origin"), 0.0),
            rotation: vec3(one.get("rotation"), 0.0),
            faces,
        });
    }
    if skipped_meshes > 0 {
        model.problems.push(format!("{skipped_meshes} mesh element(s) left out: only cubes can be item models."));
    }

    // Groups: older files keep them whole in the outliner, newer ones in "groups" with the outliner naming uuids.
    let groups: Vec<&Data> = root.get("groups").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object).collect();
    let mut loose = Vec::new();
    for child in root.get("outliner").and_then(Value::as_array).into_iter().flatten() {
        walk(child, None, &groups, &cube_ids, &mut model, &mut loose);
    }
    // Cubes outside any group (or files without an outliner) hang off a root bone.
    let placed: Vec<usize> = model.bones.iter().flat_map(|one| one.cubes.iter().copied()).chain(loose.iter().copied()).collect();
    let orphans: Vec<usize> = (0..model.cubes.len()).filter(|at| !placed.contains(at)).collect();
    loose.extend(orphans);
    let root_added = !loose.is_empty();
    if root_added {
        model
            .bones
            .insert(0, Bone { name: "root".into(), parent: None, origin: [0.0; 3], rotation: [0.0; 3], cubes: loose, visible: true });
        for bone in model.bones.iter_mut().skip(1) {
            bone.parent = bone.parent.map(|at| at + 1);
        }
    }
    if model.bones.len() > MOST_BONES {
        model.problems.push(format!("Only the first {MOST_BONES} bones are used."));
        model.bones.truncate(MOST_BONES);
    }

    let bone_ids: Vec<String> = bone_uuids(&root, &groups);
    let offset = usize::from(root_added);
    for raw in root.get("animations").and_then(Value::as_array).into_iter().flatten() {
        let Some(one) = raw.as_object() else { continue };
        let mut animation = Animation {
            name: text(one, "name").to_string(),
            length: num(one.get("length")).unwrap_or(0.0).clamp(0.0, MOST_SECONDS),
            looping: match text(one, "loop") {
                "loop" => Loop::Loop,
                "hold" => Loop::Hold,
                _ => Loop::Once,
            },
            tracks: Vec::new(),
        };
        let mut expressions = 0;
        for (uuid, animator) in one.get("animators").and_then(Value::as_object).into_iter().flatten() {
            let Some(animator) = animator.as_object() else { continue };
            let by_uuid = bone_ids.iter().position(|one| one == uuid).map(|at| at + offset);
            let by_name = model.bones.iter().position(|one| one.name == text(animator, "name"));
            let Some(bone) = by_uuid.filter(|at| *at < model.bones.len()).or(by_name) else { continue };
            for channel in [Channel::Rotation, Channel::Position, Channel::Scale] {
                let name = match channel {
                    Channel::Rotation => "rotation",
                    Channel::Position => "position",
                    Channel::Scale => "scale",
                };
                let mut keys = Vec::new();
                for raw in animator.get("keyframes").and_then(Value::as_array).into_iter().flatten() {
                    let Some(key) = raw.as_object().filter(|key| text(key, "channel") == name) else { continue };
                    let point = key.get("data_points").and_then(Value::as_array).and_then(|list| list.first()).and_then(Value::as_object);
                    let fallback = if channel == Channel::Scale { 1.0 } else { 0.0 };
                    let value = ["x", "y", "z"].map(|axis| {
                        let raw = point.and_then(|one| one.get(axis));
                        num(raw).unwrap_or_else(|| {
                            if raw.and_then(Value::as_str).is_some_and(|one| !one.trim().is_empty()) {
                                expressions += 1;
                            }
                            fallback
                        })
                    });
                    let interp = match text(key, "interpolation") {
                        "catmullrom" => Interp::CatmullRom,
                        "step" => Interp::Step,
                        _ => Interp::Linear,
                    };
                    keys.push(Key { time: num(key.get("time")).unwrap_or(0.0).max(0.0), value, interp });
                }
                if !keys.is_empty() {
                    keys.sort_by(|a, b| a.time.total_cmp(&b.time));
                    animation.tracks.push(Track { bone, channel, keys });
                }
            }
        }
        if expressions > 0 {
            model.problems.push(format!("\"{}\": {expressions} keyframe value(s) are expressions, read as 0.", animation.name));
        }
        if animation.length <= 0.0 {
            animation.length =
                animation.tracks.iter().flat_map(|one| one.keys.iter().map(|key| key.time)).fold(0.0, f64::max).min(MOST_SECONDS);
        }
        model.animations.push(animation);
    }
    Ok(model)
}

/// Group uuids in the order [`walk`] adds bones.
fn bone_uuids(root: &Data, groups: &[&Data]) -> Vec<String> {
    fn visit(node: &Value, groups: &[&Data], out: &mut Vec<String>) {
        let Some(node) = node.as_object() else { return };
        let uuid = text(node, "uuid").to_string();
        out.push(uuid.clone());
        let children = node
            .get("children")
            .and_then(Value::as_array)
            .or_else(|| groups.iter().find(|one| text(one, "uuid") == uuid).and_then(|one| one.get("children")).and_then(Value::as_array));
        for child in children.into_iter().flatten() {
            visit(child, groups, out);
        }
    }
    let mut out = Vec::new();
    for child in root.get("outliner").and_then(Value::as_array).into_iter().flatten() {
        visit(child, groups, &mut out);
    }
    out
}

fn walk(node: &Value, parent: Option<usize>, groups: &[&Data], cubes: &[(String, usize)], model: &mut Model, loose: &mut Vec<usize>) {
    match node {
        Value::String(uuid) => {
            if let Some((_, at)) = cubes.iter().find(|(one, _)| one == uuid).filter(|(_, at)| *at != usize::MAX) {
                match parent {
                    Some(bone) => model.bones[bone].cubes.push(*at),
                    None => loose.push(*at),
                }
            }
        }
        Value::Object(node) => {
            let uuid = text(node, "uuid");
            let full = groups.iter().find(|one| text(one, "uuid") == uuid).copied();
            let info = full.unwrap_or(node);
            let at = model.bones.len();
            let parent_visible = parent.is_none_or(|one| model.bones[one].visible);
            model.bones.push(Bone {
                name: text(info, "name").to_string(),
                parent,
                origin: vec3(info.get("origin"), 0.0),
                rotation: vec3(info.get("rotation"), 0.0),
                cubes: Vec::new(),
                visible: parent_visible && info.get("visibility").and_then(Value::as_bool) != Some(false),
            });
            let children = node
                .get("children")
                .and_then(Value::as_array)
                .or_else(|| full.and_then(|one| one.get("children")).and_then(Value::as_array));
            for child in children.into_iter().flatten() {
                walk(child, Some(at), groups, cubes, model, loose);
            }
        }
        _ => {}
    }
}

// ---------- sampling ----------

fn lerp(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    [0, 1, 2].map(|at| a[at] + (b[at] - a[at]) * t)
}

fn catmull(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f64) -> Vec3 {
    let (t2, t3) = (t * t, t * t * t);
    [0, 1, 2].map(|at| {
        0.5 * (2.0 * p1[at]
            + (p2[at] - p0[at]) * t
            + (2.0 * p0[at] - 5.0 * p1[at] + 4.0 * p2[at] - p3[at]) * t2
            + (3.0 * p1[at] - p0[at] - 3.0 * p2[at] + p3[at]) * t3)
    })
}

/// A track's value at `time` seconds: held before the first key and after the last, step / linear / Catmull-Rom
/// between (Catmull-Rom when either side asks for it, like Blockbench).
pub fn sample(keys: &[Key], time: f64) -> Option<Vec3> {
    let first = keys.first()?;
    if time <= first.time || keys.len() == 1 {
        return Some(first.value);
    }
    let after = keys.iter().position(|key| key.time > time);
    let Some(next) = after else { return keys.last().map(|one| one.value) };
    let (a, b) = (&keys[next - 1], &keys[next]);
    if a.interp == Interp::Step {
        return Some(a.value);
    }
    let span = b.time - a.time;
    let t = if span <= 0.0 { 1.0 } else { (time - a.time) / span };
    if a.interp == Interp::CatmullRom || b.interp == Interp::CatmullRom {
        let before = if next >= 2 { keys[next - 2].value } else { a.value };
        let later = keys.get(next + 1).map_or(b.value, |one| one.value);
        return Some(catmull(before, a.value, b.value, later, t));
    }
    Some(lerp(a.value, b.value, t))
}

/// Euler degrees in Blockbench's order (Z·Y·X, as three.js `ZYX`) as a quaternion.
pub fn euler(degrees: Vec3) -> Quat {
    let half = degrees.map(|one| one.to_radians() / 2.0);
    let (sx, cx) = half[0].sin_cos();
    let (sy, cy) = half[1].sin_cos();
    let (sz, cz) = half[2].sin_cos();
    // q = qz * qy * qx
    [sx * cy * cz - cx * sy * sz, cx * sy * cz + sx * cy * sz, cx * cy * sz - sx * sy * cz, cx * cy * cz + sx * sy * sz]
}

pub fn quat_mul(a: Quat, b: Quat) -> Quat {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

pub fn rotate(q: Quat, v: Vec3) -> Vec3 {
    let u = [q[0], q[1], q[2]];
    let cross = |a: Vec3, b: Vec3| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let t = cross(u, v).map(|one| one * 2.0);
    let c = cross(u, t);
    [0, 1, 2].map(|at| v[at] + q[3] * t[at] + c[at])
}

/// Where a bone (or part) sits: `point relative to its pivot → translation + rotation * (scale ⊙ point)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Pose {
    pub const ROOT: Pose = Pose { translation: [0.0; 3], rotation: [0.0, 0.0, 0.0, 1.0], scale: [1.0; 3] };

    /// A child placed at `offset` (relative to this pose's pivot, before this pose's rotation), turned by `rotation`
    /// and sized by `scale`.
    pub fn child(&self, offset: Vec3, rotation: Quat, scale: Vec3) -> Pose {
        let moved = rotate(self.rotation, [0, 1, 2].map(|at| offset[at] * self.scale[at]));
        Pose {
            translation: [0, 1, 2].map(|at| self.translation[at] + moved[at]),
            rotation: quat_mul(self.rotation, rotation),
            scale: [0, 1, 2].map(|at| self.scale[at] * scale[at]),
        }
    }
}

/// Every bone's pose, at rest (`None`) or `time` seconds into an animation. Animation values follow Blockbench:
/// rotation X and Y and position X are mirrored, rotations add to the bone's own, scales multiply.
pub fn pose(model: &Model, animation: Option<(&Animation, f64)>) -> Vec<Pose> {
    let mut out: Vec<Pose> = Vec::with_capacity(model.bones.len());
    for (at, bone) in model.bones.iter().enumerate() {
        let (mut rotation, mut position, mut scale) = (bone.rotation, [0.0; 3], [1.0; 3]);
        if let Some((animation, time)) = animation {
            for track in animation.tracks.iter().filter(|one| one.bone == at) {
                let Some(value) = sample(&track.keys, time) else { continue };
                match track.channel {
                    Channel::Rotation => rotation = [rotation[0] - value[0], rotation[1] - value[1], rotation[2] + value[2]],
                    Channel::Position => position = [-value[0], value[1], value[2]],
                    Channel::Scale => scale = value,
                }
            }
        }
        let (parent, parent_origin) = match bone.parent.filter(|one| *one < out.len()) {
            Some(one) => (out[one], model.bones[one].origin),
            None => (Pose::ROOT, [0.0; 3]),
        };
        let offset = [0, 1, 2].map(|axis| position[axis] + bone.origin[axis] - parent_origin[axis]);
        out.push(parent.child(offset, euler(rotation), scale));
    }
    out
}

/// The animation time for `elapsed` seconds of play: wrapped when it loops, `None` once a `once` animation is over.
pub fn play_time(animation: &Animation, elapsed: f64) -> Option<f64> {
    if animation.length <= 0.0 {
        return (animation.looping != Loop::Once).then_some(0.0);
    }
    match animation.looping {
        Loop::Loop => Some(elapsed.rem_euclid(animation.length)),
        Loop::Hold => Some(elapsed.min(animation.length)),
        Loop::Once => (elapsed <= animation.length).then_some(elapsed),
    }
}

/// How many baked frames an animation has (one per tick, both ends).
pub fn frame_count(animation: &Animation) -> usize {
    ((animation.length.min(MOST_SECONDS) * FPS).ceil() as usize).max(1) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "meta": {"format_version": "4.10", "model_format": "free"},
        "resolution": {"width": 32, "height": 32},
        "elements": [
            {"name": "body", "type": "cube", "uuid": "c1", "from": [-4, 0, -4], "to": [4, 8, 4], "origin": [0, 0, 0],
             "faces": {"north": {"uv": [0, 0, 8, 8], "texture": 0}, "up": {"uv": [8, 0, 16, 8], "texture": 0, "rotation": 90}, "down": {"uv": [0, 0, 0, 0], "texture": null}}},
            {"name": "head", "type": "cube", "uuid": "c2", "from": [-2, 8, -2], "to": [2, 12, 2], "origin": [0, 8, 0], "rotation": [0, 30, 0],
             "faces": {"north": {"uv": [0, 16, 4, 20], "texture": 0}}},
            {"name": "loose", "type": "cube", "uuid": "c3", "from": [0, 0, 0], "to": [1, 1, 1], "faces": {}},
            {"name": "mesh", "type": "mesh", "uuid": "m1"}
        ],
        "outliner": [
            {"name": "body", "uuid": "g1", "origin": [0, 0, 0], "children": ["c1",
                {"name": "head", "uuid": "g2", "origin": [0, 8, 0], "rotation": [10, 0, 0], "children": ["c2"]}]},
            "c3"
        ],
        "textures": [{"name": "skin.png", "uuid": "t1", "source": "data:image/png;base64,AAAA"}],
        "animations": [
            {"name": "nod", "loop": "loop", "length": 1, "animators": {
                "g2": {"name": "head", "type": "bone", "keyframes": [
                    {"channel": "rotation", "time": 0, "interpolation": "linear", "data_points": [{"x": "0", "y": "0", "z": "0"}]},
                    {"channel": "rotation", "time": 0.5, "interpolation": "linear", "data_points": [{"x": "-20", "y": "0", "z": "0"}]},
                    {"channel": "position", "time": 0, "data_points": [{"x": 0, "y": "math.sin(q.anim_time)", "z": 0}]}
                ]}
            }},
            {"name": "pop", "loop": "once", "animators": {"g1": {"name": "body", "keyframes": [
                {"channel": "scale", "time": 0.25, "data_points": [{"x": 2, "y": 2, "z": 2}]}
            ]}}}
        ]
    }"#;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn reads_bones_cubes_and_animations() {
        let model = parse(SAMPLE).unwrap();
        let names: Vec<&str> = model.bones.iter().map(|one| one.name.as_str()).collect();
        assert_eq!(names, ["root", "body", "head"]);
        assert_eq!(model.bones[2].parent, Some(1));
        assert_eq!(model.bones[0].cubes, vec![2]);
        assert_eq!(model.bones[1].cubes, vec![0]);
        assert_eq!(model.cubes[0].faces[0].as_ref().unwrap().uv, [0.0, 0.0, 8.0, 8.0]);
        assert_eq!(model.cubes[0].faces[4].as_ref().unwrap().rotation, 90);
        assert!(model.cubes[0].faces[5].is_none(), "no texture = no face");
        assert_eq!(model.textures[0].name, "skin");
        assert_eq!(model.textures[0].uv_width, 32.0);
        assert_eq!(model.animations.len(), 2);
        let nod = &model.animations[0];
        assert_eq!(nod.looping, Loop::Loop);
        assert_eq!(nod.tracks.len(), 2);
        assert!(nod.tracks.iter().all(|one| one.bone == 2));
        assert_eq!(model.animations[1].length, 0.25, "length from the last key when missing");
        assert!(model.problems.iter().any(|one| one.contains("mesh")));
        assert!(model.problems.iter().any(|one| one.contains("expressions")));
        assert!(parse("[]").is_err());
        assert!(parse("{}").is_err());
    }

    #[test]
    fn newer_files_keep_groups_apart() {
        let text = r#"{"elements":[{"type":"cube","uuid":"c1","from":[0,0,0],"to":[1,1,1]}],
            "groups":[{"uuid":"g1","name":"arm","origin":[1,2,3]}],
            "outliner":[{"uuid":"g1","children":["c1"]}]}"#;
        let model = parse(text).unwrap();
        assert_eq!(model.bones.len(), 1);
        assert_eq!(model.bones[0].name, "arm");
        assert_eq!(model.bones[0].origin, [1.0, 2.0, 3.0]);
        assert_eq!(model.bones[0].cubes, vec![0]);
    }

    #[test]
    fn samples_between_keys() {
        let key = |time, x, interp| Key { time, value: [x, 0.0, 0.0], interp };
        let keys = vec![key(0.0, 0.0, Interp::Linear), key(1.0, 10.0, Interp::Step), key(2.0, 20.0, Interp::Linear)];
        assert_eq!(sample(&keys, -1.0).unwrap()[0], 0.0);
        assert_eq!(sample(&keys, 0.5).unwrap()[0], 5.0);
        assert_eq!(sample(&keys, 1.5).unwrap()[0], 10.0, "step holds");
        assert_eq!(sample(&keys, 9.0).unwrap()[0], 20.0);
        let smooth = vec![key(0.0, 0.0, Interp::CatmullRom), key(1.0, 10.0, Interp::CatmullRom), key(2.0, 20.0, Interp::CatmullRom)];
        assert!(close(sample(&smooth, 0.5).unwrap()[0], 5.0 - 0.625), "{:?}", sample(&smooth, 0.5));
        assert!(close(sample(&smooth, 1.0).unwrap()[0], 10.0));
        assert!(sample(&[], 0.0).is_none());
    }

    #[test]
    fn poses_follow_the_tree() {
        let model = parse(SAMPLE).unwrap();
        let rest = pose(&model, None);
        // The head's pivot sits 8 px up, turned 10° on X.
        assert!(close(rest[2].translation[1], 8.0));
        let turned = rotate(rest[2].rotation, [0.0, 0.0, 1.0]);
        assert!(close(turned[1], -(10f64.to_radians().sin())), "{turned:?}");
        // Half way through "nod": the head is at 10 - (-10)... X is mirrored, so 10 + 10 = 20°.
        let nod = &model.animations[0];
        let mid = pose(&model, Some((nod, 0.25)));
        let turned = rotate(mid[2].rotation, [0.0, 0.0, 1.0]);
        assert!(close(turned[1], -(20f64.to_radians().sin())), "{turned:?}");
        // A parent's scale moves and sizes its children.
        let pop = &model.animations[1];
        let big = pose(&model, Some((pop, 0.25)));
        assert!(close(big[2].translation[1], 16.0));
        assert!(close(big[2].scale[0], 2.0));
    }

    #[test]
    fn euler_order_is_zyx() {
        let q = euler([90.0, 90.0, 0.0]);
        // X first, then Y: +Y goes to +Z under X, then to +X... no: Y-axis (0,1,0) → X 90° → (0,0,1) → Y 90° → (1,0,0).
        let v = rotate(q, [0.0, 1.0, 0.0]);
        assert!(close(v[0], 1.0) && close(v[1], 0.0) && close(v[2], 0.0), "{v:?}");
    }

    #[test]
    fn play_times() {
        let mut animation = Animation { name: "a".into(), length: 2.0, looping: Loop::Loop, tracks: Vec::new() };
        assert_eq!(play_time(&animation, 5.0), Some(1.0));
        animation.looping = Loop::Hold;
        assert_eq!(play_time(&animation, 5.0), Some(2.0));
        animation.looping = Loop::Once;
        assert_eq!(play_time(&animation, 5.0), None);
        assert_eq!(frame_count(&animation), 41);
    }
}

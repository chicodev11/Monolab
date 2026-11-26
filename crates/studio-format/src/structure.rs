//! Structures (`structure` files): a build brought in from a `.schem` (WorldEdit) or `.nbt` (structure block) file.
//! The app turns the file into a plain block list kept next to the studio files, in `structures/<id>.json`
//! (`{"size": [x, y, z], "palette": ["oak_planks", "oak_stairs[facing=east,half=bottom]", ...], "blocks": [x, y, z, i, ...]}`,
//! air left out), so both add-ons place it without reading NBT. The `structure` action puts it down (a custom
//! world's `structure` feature is planned; `MOST_FEATURE_SIZE` is for it). Block entities (chest contents, sign text) aren't brought along.

use crate::fields;
use serde_json::{json, Map, Value};

type Data = Map<String, Value>;

/// Where the block lists live, inside the studio folder.
pub const FOLDER: &str = "structures";
pub const ROTATIONS: [&str; 5] = ["0", "90", "180", "270", "random"];
/// The most blocks one structure may hold.
pub const MOST_BLOCKS: usize = 1_000_000;
/// A world feature's structure must fit in this many blocks across (it is cut at the chunks it reaches).
pub const MOST_FEATURE_SIZE: i32 = 32;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Structure {
    pub size: (i32, i32, i32),
    pub palette: Vec<String>,
    /// (x, y, z, palette index), from the structure's low corner.
    pub blocks: Vec<(i32, i32, i32, u32)>,
}

impl Structure {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|error| format!("The structure's block list is broken: {error}"))?;
        let size = value
            .get("size")
            .and_then(Value::as_array)
            .map(|list| list.iter().map(|one| one.as_i64().unwrap_or(0) as i32).collect::<Vec<_>>())
            .unwrap_or_default();
        if size.len() != 3 {
            return Err("The structure's block list has no size.".into());
        }
        let palette: Vec<String> = value
            .get("palette")
            .and_then(Value::as_array)
            .map(|list| list.iter().map(|one| one.as_str().unwrap_or("air").to_string()).collect())
            .unwrap_or_default();
        let flat: Vec<i64> = value
            .get("blocks")
            .and_then(Value::as_array)
            .map(|list| list.iter().map(|one| one.as_i64().unwrap_or(0)).collect())
            .unwrap_or_default();
        let blocks = flat
            .as_chunks::<4>()
            .0
            .iter()
            .take(MOST_BLOCKS)
            .filter(|one| (one[3] as usize) < palette.len())
            .map(|one| (one[0] as i32, one[1] as i32, one[2] as i32, one[3] as u32))
            .collect();
        Ok(Self { size: (size[0], size[1], size[2]), palette, blocks })
    }

    pub fn to_json(&self) -> String {
        let flat: Vec<i64> =
            self.blocks.iter().flat_map(|(x, y, z, i)| [i64::from(*x), i64::from(*y), i64::from(*z), i64::from(*i)]).collect();
        json!({ "size": [self.size.0, self.size.1, self.size.2], "palette": self.palette, "blocks": flat }).to_string()
    }

    /// Every block turned `quarter` times clockwise (seen from above) around the low corner, so it still starts
    /// at (0, 0, 0): (dx, dy, dz, block state).
    pub fn placements(&self, quarter: u8) -> Vec<(i32, i32, i32, String)> {
        let turned: Vec<String> = self.palette.iter().map(|state| rotate_state(state, quarter)).collect();
        let (sx, _, sz) = self.size;
        self.blocks
            .iter()
            .map(|&(x, y, z, index)| {
                let (nx, nz) = match quarter % 4 {
                    1 => (sz - 1 - z, x),
                    2 => (sx - 1 - x, sz - 1 - z),
                    3 => (z, sx - 1 - x),
                    _ => (x, z),
                };
                (nx, y, nz, turned[index as usize].clone())
            })
            .collect()
    }

    /// Width and depth after turning.
    pub fn footprint(&self, quarter: u8) -> (i32, i32) {
        if quarter % 2 == 1 {
            (self.size.2, self.size.0)
        } else {
            (self.size.0, self.size.2)
        }
    }
}

/// `oak_stairs[facing=east,half=bottom]` → (`minecraft:oak_stairs`, [(facing, east), (half, bottom)]).
pub fn split_state(state: &str) -> (String, Vec<(String, String)>) {
    let (name, rest) = match state.split_once('[') {
        Some((name, rest)) => (name, rest.trim_end_matches(']')),
        None => (state, ""),
    };
    let name = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
    let props = rest
        .split(',')
        .filter_map(|one| one.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect();
    (name, props)
}

const SIDES: [&str; 4] = ["north", "east", "south", "west"];

fn turn_side(side: &str, quarter: u8) -> Option<&'static str> {
    SIDES.iter().position(|one| *one == side).map(|at| SIDES[(at + quarter as usize) % 4])
}

/// The state turned `quarter` times clockwise: `facing`, `axis`, sign and banner `rotation`, and fence/wall/pane
/// sides (`north=true`...).
pub fn rotate_state(state: &str, quarter: u8) -> String {
    let quarter = quarter % 4;
    if quarter == 0 || !state.contains('[') {
        return state.to_string();
    }
    let (name, props) = state.split_once('[').map(|(name, rest)| (name, rest.trim_end_matches(']'))).unwrap_or((state, ""));
    let turned: Vec<String> = props
        .split(',')
        .filter_map(|one| one.split_once('='))
        .map(|(key, value)| {
            let (key, value) = (key.trim(), value.trim());
            match key {
                "facing" => format!("facing={}", turn_side(value, quarter).unwrap_or(value)),
                "axis" if quarter % 2 == 1 && (value == "x" || value == "z") => format!("axis={}", if value == "x" { "z" } else { "x" }),
                "rotation" => value
                    .parse::<u8>()
                    .map(|turn| format!("rotation={}", (turn + quarter * 4) % 16))
                    .unwrap_or_else(|_| format!("{key}={value}")),
                side if SIDES.contains(&side) => format!("{}={value}", turn_side(side, quarter).unwrap_or(side)),
                _ => format!("{key}={value}"),
            }
        })
        .collect();
    format!("{name}[{}]", turned.join(","))
}

/// How many quarter turns a file's or action's `rotation` means; `random` takes one from `roll` (0..4).
pub fn quarter_of(rotation: &str, roll: u8) -> u8 {
    match rotation.trim() {
        "90" => 1,
        "180" => 2,
        "270" => 3,
        "random" => roll % 4,
        _ => 0,
    }
}

pub fn rotation(data: &Data) -> String {
    fields::choice(data, "rotation", &ROTATIONS, "0")
}

/// Whether it is placed with its middle (x, z) on the spot rather than its corner.
pub fn centered(data: &Data) -> bool {
    data.get("centered").and_then(Value::as_bool) != Some(false)
}

/// Lifts it by this many blocks (negative sinks it into the ground).
pub fn lift(data: &Data) -> i32 {
    fields::number(data, "lift", 0, -64, 64) as i32
}

pub fn data_path(id: &str) -> String {
    format!("{FOLDER}/{id}.json")
}

/// The spot of a structure's low corner when it is put at (x, y, z).
pub fn corner(data: &Data, structure: &Structure, quarter: u8, at: (i32, i32, i32)) -> (i32, i32, i32) {
    let (w, d) = structure.footprint(quarter);
    let (x, z) = if centered(data) { (at.0 - w / 2, at.2 - d / 2) } else { (at.0, at.2) };
    (x, at.1 + lift(data), z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_and_round_trips() {
        let built = Structure {
            size: (3, 1, 2),
            palette: vec!["stone".into(), "oak_stairs[facing=north,half=bottom]".into()],
            blocks: vec![(0, 0, 0, 0), (2, 0, 1, 1)],
        };
        let back = Structure::from_json(&built.to_json()).unwrap();
        assert_eq!(back, built);
        let turned = built.placements(1);
        assert_eq!(turned[0], (1, 0, 0, "stone".into()));
        assert_eq!(turned[1], (0, 0, 2, "oak_stairs[facing=east,half=bottom]".into()));
        assert_eq!(built.footprint(1), (2, 3));
        assert_eq!(rotate_state("oak_log[axis=x]", 1), "oak_log[axis=z]");
        assert_eq!(rotate_state("oak_sign[rotation=14]", 1), "oak_sign[rotation=2]");
        assert_eq!(rotate_state("oak_fence[east=true,north=false]", 2), "oak_fence[west=true,south=false]");
        assert_eq!(split_state("oak_stairs[facing=east]"), ("minecraft:oak_stairs".into(), vec![("facing".into(), "east".into())]));
        assert_eq!(quarter_of("random", 7), 3);
        let data: Data = serde_json::from_str(r#"{"lift": -1}"#).unwrap();
        assert_eq!(corner(&data, &built, 0, (10, 64, 10)), (9, 63, 9));
    }
}

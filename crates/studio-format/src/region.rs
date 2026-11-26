//! Copied areas for the `region` rule step (`studio/data/regions/<name>.json`), the same on Paper and Pumpkin:
//! `{"size": [w, h, d], "palette": ["minecraft:stone", ...], "blocks": [0, 1, ...]}`. Blocks go x first, then z, then
//! y; each is a place in the palette. Pasting puts the lowest corner at the spot, turned a quarter clockwise
//! (seen from above) as many times as asked.

use serde_json::{json, Value};

/// A fill may change this many blocks at most; a copied area may hold this many.
pub const MOST_FILL: u64 = 32_768;
pub const MOST_REGION: u64 = 262_144;

/// A region's name: lower-case letters, digits, `_` and `-`, at most 64.
pub fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == b'_' || one == b'-')
}

/// Where the file of a region lives, under the studio folder.
pub fn data_path(name: &str) -> String {
    format!("data/regions/{name}.json")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub size: (u32, u32, u32),
    pub palette: Vec<String>,
    pub blocks: Vec<u32>,
}

impl Region {
    /// Builds one from block states in file order, sharing the palette.
    pub fn from_states(size: (u32, u32, u32), states: &[String]) -> Self {
        let mut palette: Vec<String> = Vec::new();
        let mut blocks = Vec::with_capacity(states.len());
        let mut index: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
        for state in states {
            let at = *index.entry(state.as_str()).or_insert_with(|| {
                palette.push(state.clone());
                (palette.len() - 1) as u32
            });
            blocks.push(at);
        }
        Self { size, palette, blocks }
    }

    pub fn to_json(&self) -> String {
        json!({ "schema": 1, "size": [self.size.0, self.size.1, self.size.2], "palette": self.palette, "blocks": self.blocks }).to_string()
    }

    pub fn from_json(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        let size = value.get("size")?.as_array()?;
        let part = |at: usize| size.get(at).and_then(Value::as_u64).filter(|one| *one > 0 && *one <= 4096).map(|one| one as u32);
        let size = (part(0)?, part(1)?, part(2)?);
        let palette: Vec<String> =
            value.get("palette")?.as_array()?.iter().map(|one| one.as_str().unwrap_or("minecraft:air").to_string()).collect();
        let blocks: Vec<u32> = value.get("blocks")?.as_array()?.iter().map(|one| one.as_u64().unwrap_or(0) as u32).collect();
        let volume = u64::from(size.0) * u64::from(size.1) * u64::from(size.2);
        if volume != blocks.len() as u64 || volume > MOST_REGION || blocks.iter().any(|one| *one as usize >= palette.len()) {
            return None;
        }
        Some(Self { size, palette, blocks })
    }

    /// The palette place of the block at (x, y, z) inside the area.
    pub fn at(&self, x: u32, y: u32, z: u32) -> u32 {
        let (width, _, depth) = self.size;
        self.blocks[((y * depth + z) * width + x) as usize]
    }
}

/// Where (x, z) inside a width × depth area lands after turning it a quarter clockwise `quarter` times.
pub fn turn(x: u32, z: u32, width: u32, depth: u32, quarter: u32) -> (u32, u32) {
    match quarter % 4 {
        1 => (depth - 1 - z, x),
        2 => (width - 1 - x, depth - 1 - z),
        3 => (z, width - 1 - x),
        _ => (x, z),
    }
}

const SIDES: [&str; 4] = ["north", "east", "south", "west"];

fn turn_side(side: &str, quarter: u32) -> Option<&'static str> {
    let at = SIDES.iter().position(|one| *one == side)?;
    Some(SIDES[(at + quarter as usize) % 4])
}

/// A block state (`minecraft:oak_stairs[facing=north,half=bottom]`) turned a quarter clockwise `quarter` times:
/// `facing`, `axis`, `rotation` (signs, heads) and side keys (fences, walls, panes) turn; other properties stay.
pub fn turn_state(state: &str, quarter: u32) -> String {
    let quarter = quarter % 4;
    if quarter == 0 {
        return state.to_string();
    }
    let Some((name, rest)) = state.split_once('[') else { return state.to_string() };
    let props = rest.trim_end_matches(']');
    let mut out: Vec<(String, String)> = Vec::new();
    for pair in props.split(',').filter(|one| !one.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let (key, value) = match key {
            "facing" => (key.to_string(), turn_side(value, quarter).unwrap_or(value).to_string()),
            "axis" if quarter % 2 == 1 => (
                key.to_string(),
                match value {
                    "x" => "z".to_string(),
                    "z" => "x".to_string(),
                    other => other.to_string(),
                },
            ),
            "rotation" => (
                key.to_string(),
                value.parse::<u32>().map(|one| ((one + 4 * quarter) % 16).to_string()).unwrap_or_else(|_| value.to_string()),
            ),
            side if SIDES.contains(&side) => (turn_side(side, quarter).unwrap_or(side).to_string(), value.to_string()),
            _ => (key.to_string(), value.to_string()),
        };
        out.push((key, value));
    }
    out.sort();
    format!("{name}[{}]", out.iter().map(|(key, value)| format!("{key}={value}")).collect::<Vec<_>>().join(","))
}

/// One axis of a spot as a rule writes it: a number, `~` (the base) or `~3` (three past it). `None` when unreadable.
pub fn axis(text: &str, base: i32) -> Option<i32> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix('~') {
        let rest = rest.trim();
        return if rest.is_empty() { Some(base) } else { rest.parse::<f64>().ok().map(|one| base + one.floor() as i32) };
    }
    text.parse::<f64>().ok().map(|one| one.floor() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_turns() {
        let states: Vec<String> =
            ["minecraft:stone", "minecraft:air", "minecraft:stone", "minecraft:dirt"].iter().map(|one| one.to_string()).collect();
        let region = Region::from_states((2, 1, 2), &states);
        assert_eq!(region.palette, vec!["minecraft:stone", "minecraft:air", "minecraft:dirt"]);
        assert_eq!(Region::from_json(&region.to_json()), Some(region.clone()));
        assert_eq!(region.at(1, 0, 1), 2);
        assert_eq!(turn(0, 0, 3, 2, 1), (1, 0));
        assert_eq!(turn(2, 1, 3, 2, 2), (0, 0));
        assert_eq!(turn(0, 0, 3, 2, 3), (0, 2));
        assert!(Region::from_json(r#"{"size":[2,1,1],"palette":["a"],"blocks":[0]}"#).is_none(), "wrong count");
    }

    #[test]
    fn turns_states() {
        assert_eq!(turn_state("minecraft:oak_stairs[facing=north,half=bottom]", 1), "minecraft:oak_stairs[facing=east,half=bottom]");
        assert_eq!(turn_state("minecraft:oak_log[axis=x]", 1), "minecraft:oak_log[axis=z]");
        assert_eq!(turn_state("minecraft:oak_sign[rotation=14]", 1), "minecraft:oak_sign[rotation=2]");
        assert_eq!(
            turn_state("minecraft:oak_fence[east=false,north=true,south=false,west=false]", 1),
            "minecraft:oak_fence[east=true,north=false,south=false,west=false]"
        );
        assert_eq!(turn_state("minecraft:stone", 3), "minecraft:stone");
    }

    #[test]
    fn reads_axes() {
        assert_eq!(axis("~", 5), Some(5));
        assert_eq!(axis("~-2", 5), Some(3));
        assert_eq!(axis("12.7", 5), Some(12));
        assert_eq!(axis("x", 5), None);
        assert!(name_ok("arena_1") && !name_ok("Arena") && !name_ok("../x"));
    }
}

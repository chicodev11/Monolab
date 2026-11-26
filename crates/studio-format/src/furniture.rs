//! Studio maker M3: furniture (`furniture` kind). A piece of furniture is an item display showing the studio
//! texture or model (the M1 look), an interaction entity players click and hit, and, when it is solid, barrier
//! blocks on its cells. It turns to face the player who placed it (snapped to 4 or 8 ways), may take more than one
//! block, may be sat on, and runs actions on right click and when it breaks. Shared by the app's pack builder
//! (`src-tauri/src/studio_pack_blocks.rs`) and the Pumpkin add-on; Paper keeps the same rules in `StudioFurniture.java`.
//!
//! ```json
//! "look": { "texture": "chair" },                    // or { "model": "chair" } — like an item's
//! "display": "fixed", "scale": 1, "y": 0.5,          // how the item display draws it
//! "turns": 4,                                        // 1 (never turns), 4 or 8
//! "hitbox": { "width": 1, "height": 1 },
//! "solid": true,                                     // barrier blocks on every cell
//! "cells": [{ "x": 0, "y": 0, "z": 0 }, { "x": 1, "y": 0, "z": 0 }],
//! "seat": { "height": 0.5 },                         // right click sits; leave out for none
//! "light": 0,                                        // light blocks on its cells (not when solid)
//! "drops": true, "item": "", "name": "Chair", "lore": [],
//! "useActions": [], "breakActions": []
//! ```

use crate::fields::{self, Data};
use crate::item;
use serde_json::Value;

/// How the item display draws the item, as the game names it.
pub const DISPLAYS: [&str; 4] = ["fixed", "none", "head", "ground"];
pub const TURNS: [i64; 3] = [1, 4, 8];
pub const ACTION_LISTS: [&str; 2] = ["useActions", "breakActions"];
/// `ids.json` kind for furniture items before 1.21.4 (custom model data) is the items' `cmd`, named `furniture:<id>`.
pub const CMD_PREFIX: &str = "furniture:";
/// The item furniture is placed with when it has no studio item of its own, and its base before 1.21.4.
pub const ITEM: &str = "flint";
pub const MOST_CELLS: usize = 27;
/// Cells reach at most this far from the one it was placed on.
pub const MOST_REACH: i64 = 4;

/// One block a piece of furniture takes, from where it was placed, before turning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cell {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

pub fn cells(data: &Data) -> Vec<Cell> {
    let mut out = Vec::new();
    if let Some(Value::Array(list)) = data.get("cells") {
        for one in list.iter().filter_map(Value::as_object) {
            let read = |key: &str| fields::number(one, key, 0, -MOST_REACH, MOST_REACH);
            let cell = Cell { x: read("x"), y: read("y"), z: read("z") };
            if !out.contains(&cell) {
                out.push(cell);
            }
            if out.len() == MOST_CELLS {
                break;
            }
        }
    }
    if out.is_empty() {
        out.push(Cell { x: 0, y: 0, z: 0 });
    }
    out
}

pub fn turns(data: &Data) -> i64 {
    let one = fields::number(data, "turns", 4, 1, 8);
    if TURNS.contains(&one) { one } else { 4 }
}

/// The yaw a piece placed by a player looking at `player_yaw` gets: facing the player, snapped to `turns` ways
/// (0 = south, like the game). Always in `[0, 360)`.
pub fn snap_yaw(player_yaw: f64, turns: i64) -> f64 {
    let facing = (player_yaw + 180.0).rem_euclid(360.0);
    if turns <= 1 {
        return 0.0;
    }
    let step = 360.0 / turns as f64;
    ((facing / step).round() * step).rem_euclid(360.0)
}

/// Where a cell lands once the piece is turned to `yaw`: cells turn by the nearest quarter (an 8-way piece's cells
/// follow its nearest straight direction).
pub fn turn_cell(cell: Cell, yaw: f64) -> Cell {
    let quarter = ((yaw.rem_euclid(360.0) / 90.0).round() as i64).rem_euclid(4);
    // The game's yaw turns clockwise seen from above: south (+z) → west (-x) → north → east.
    let (x, z) = match quarter {
        1 => (-cell.z, cell.x),
        2 => (-cell.x, -cell.z),
        3 => (cell.z, -cell.x),
        _ => (cell.x, cell.z),
    };
    Cell { x, y: cell.y, z }
}

pub fn display(data: &Data) -> String {
    let one = fields::text(data, "display", "fixed");
    if DISPLAYS.contains(&one.as_str()) { one } else { "fixed".into() }
}

pub fn scale(data: &Data) -> f64 {
    item::float(data, "scale", 1.0, 0.05, 8.0)
}

/// How high above the block's bottom the display sits.
pub fn lift(data: &Data) -> f64 {
    item::float(data, "y", 0.5, -4.0, 8.0)
}

/// The hitbox: (width, height).
pub fn hitbox(data: &Data) -> (f64, f64) {
    let empty = Data::new();
    let box_ = item::part(data, "hitbox").unwrap_or(&empty);
    (item::float(box_, "width", 1.0, 0.1, 8.0), item::float(box_, "height", 1.0, 0.1, 8.0))
}

pub fn solid(data: &Data) -> bool {
    data.get("solid").and_then(Value::as_bool) == Some(true)
}

/// How high a player sits above the block's bottom, or `None` when it isn't a seat.
pub fn seat(data: &Data) -> Option<f64> {
    item::part(data, "seat").map(|seat| item::float(seat, "height", 0.5, -1.0, 4.0))
}

/// Light level on its cells; a solid piece can't have light (both need the cell's block).
pub fn light(data: &Data) -> u8 {
    if solid(data) { 0 } else { fields::number(data, "light", 0, 0, 15) as u8 }
}

pub fn drops(data: &Data) -> bool {
    data.get("drops").and_then(Value::as_bool) != Some(false)
}

/// What is wrong with a furniture file, for its status. Empty = fine.
pub fn problems(data: &Data) -> Vec<String> {
    let mut out: Vec<String> = item::problems(data).into_iter().filter(|one| one.starts_with("\"look")).collect();
    if item::look(data).is_none() && out.is_empty() {
        out.push("Pick a texture or a model in \"look\".".into());
    }
    let raw = fields::text(data, "display", "fixed");
    if !DISPLAYS.contains(&raw.as_str()) {
        out.push(format!("\"display\" must be one of: {}.", DISPLAYS.join(", ")));
    }
    if data.get("turns").is_some_and(|one| one.as_i64().is_none_or(|turns| !TURNS.contains(&turns))) {
        out.push("\"turns\" must be 1, 4 or 8.".into());
    }
    if let Some(Value::Array(list)) = data.get("cells") {
        if list.len() > MOST_CELLS {
            out.push(format!("A piece of furniture takes up to {MOST_CELLS} cells."));
        }
        let far = list.iter().filter_map(Value::as_object).any(|one| ["x", "y", "z"].iter().any(|key| one.get(*key).and_then(Value::as_f64).is_some_and(|at| at.abs() > MOST_REACH as f64)));
        if far {
            out.push(format!("Cells reach at most {MOST_REACH} blocks from the first."));
        }
    }
    if solid(data) && fields::number(data, "light", 0, 0, 15) > 0 {
        out.push("A solid piece can't give light (both need its blocks); \"light\" is ignored.".into());
    }
    let item = fields::text(data, "item", "");
    if !item.trim().is_empty() && !crate::valid_id(item.trim()) {
        out.push("\"item\" must be a studio item's file name.".into());
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
    fn snaps_and_turns() {
        // A player looking north (yaw 180) places a piece facing south (0), toward them.
        assert_eq!(snap_yaw(180.0, 4), 0.0);
        assert_eq!(snap_yaw(0.0, 4), 180.0);
        assert_eq!(snap_yaw(-90.0, 4), 90.0);
        assert_eq!(snap_yaw(10.0, 8), 180.0);
        assert_eq!(snap_yaw(-200.0, 8), 0.0);
        assert_eq!(snap_yaw(-130.0, 8), 45.0);
        assert_eq!(snap_yaw(123.0, 1), 0.0);
        let east = Cell { x: 1, y: 0, z: 0 };
        assert_eq!(turn_cell(east, 0.0), east);
        assert_eq!(turn_cell(east, 90.0), Cell { x: 0, y: 0, z: 1 });
        assert_eq!(turn_cell(east, 180.0), Cell { x: -1, y: 0, z: 0 });
        assert_eq!(turn_cell(east, 270.0), Cell { x: 0, y: 0, z: -1 });
        assert_eq!(turn_cell(east, 45.0), Cell { x: 0, y: 0, z: 1 }, "45 rounds to the quarter at 90");
    }

    #[test]
    fn reads_fields() {
        let one = data(json!({ "look": { "texture": "chair" }, "cells": [{ "x": 0 }, { "x": 1 }, { "x": 1 }, { "x": 9 }], "seat": {}, "light": 7 }));
        assert_eq!(cells(&one), vec![Cell { x: 0, y: 0, z: 0 }, Cell { x: 1, y: 0, z: 0 }, Cell { x: 4, y: 0, z: 0 }]);
        assert_eq!(seat(&one), Some(0.5));
        assert_eq!(light(&one), 7);
        assert_eq!(turns(&one), 4);
        assert_eq!(hitbox(&one), (1.0, 1.0));
        assert!(drops(&one));
        assert_eq!(cells(&data(json!({}))), vec![Cell { x: 0, y: 0, z: 0 }]);
        assert_eq!(light(&data(json!({ "solid": true, "light": 9 }))), 0);
        assert_eq!(display(&data(json!({ "display": "weird" }))), "fixed");
    }

    #[test]
    fn finds_problems() {
        assert!(problems(&data(json!({ "look": { "model": "sofa" }, "turns": 8, "solid": true }))).is_empty());
        assert_eq!(problems(&data(json!({}))).len(), 1);
        assert_eq!(problems(&data(json!({ "look": { "texture": "a" }, "turns": 3 }))).len(), 1);
        assert_eq!(problems(&data(json!({ "look": { "texture": "a" }, "cells": [{ "x": 5 }] }))).len(), 1);
        assert_eq!(problems(&data(json!({ "look": { "texture": "a" }, "solid": true, "light": 3 }))).len(), 1);
        assert_eq!(problems(&data(json!({ "look": { "texture": "a", "model": "b" } }))).len(), 1);
    }
}

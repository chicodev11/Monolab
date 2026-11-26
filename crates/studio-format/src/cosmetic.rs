//! Cosmetics (`cosmetic` files): a hat or a back piece (a studio or vanilla item shown on an item display riding
//! the player) or a trail (a particle where the player walks). A player wears at most one per slot; the `cosmetic`
//! action puts one on or takes it off, and what they wear is kept on the player (`monolab:cosmetics`), so it is
//! there again after a rejoin or a restart.

use crate::{fields, item};
use serde_json::{Map, Value};

type Data = Map<String, Value>;

pub const SLOTS: [&str; 3] = ["hat", "back", "trail"];
/// What the action does: `toggle` (the default, for menu buttons), `wear` or `remove`.
pub const MODES: [&str; 3] = ["toggle", "wear", "remove"];
/// The player's data key that holds `slot=id;...`.
pub const KEY: &str = "cosmetics";
/// Ticks between cosmetic updates (displays follow the body's turn, trails puff).
pub const EVERY_TICKS: u64 = 2;

pub fn slot(data: &Data) -> String {
    fields::choice(data, "slot", &SLOTS, "hat")
}

/// The shown item for a hat or back piece: `studio:<id>` or a vanilla material.
pub fn item(data: &Data) -> String {
    fields::text(data, "item", "").trim().to_string()
}

pub fn scale(data: &Data) -> f32 {
    item::float(data, "scale", 1.0, 0.1, 4.0) as f32
}

/// Extra lift (up) and push (back, for back pieces) in blocks.
pub fn offset(data: &Data) -> (f32, f32) {
    (item::float(data, "up", 0.0, -2.0, 2.0) as f32, item::float(data, "back", 0.0, -2.0, 2.0) as f32)
}

/// Where the item sits in the display, from the rider's spot on top of the player's head: a hat just over the head, a
/// back piece between the shoulders and behind.
pub fn translation(data: &Data) -> (f32, f32, f32) {
    let (up, back) = offset(data);
    match slot(data).as_str() {
        "back" => (0.0, -0.75 + up, -0.3 - back),
        _ => (0.0, -0.2 + up, 0.0 - back),
    }
}

pub fn particle(data: &Data) -> String {
    fields::text(data, "particle", "").trim().trim_start_matches("minecraft:").to_ascii_lowercase()
}

pub fn count(data: &Data) -> u32 {
    fields::number(data, "count", 2, 1, 20) as u32
}

pub fn permission(data: &Data) -> String {
    fields::text(data, "permission", "").trim().to_string()
}

/// `slot=id;...` as kept on the player.
pub fn parse(text: &str) -> Vec<(String, String)> {
    text.split(';')
        .filter_map(|one| one.split_once('='))
        .filter(|(slot, id)| SLOTS.contains(slot) && crate::valid_id(id))
        .map(|(slot, id)| (slot.to_string(), id.to_string()))
        .collect()
}

pub fn write(list: &[(String, String)]) -> String {
    list.iter().map(|(slot, id)| format!("{slot}={id}")).collect::<Vec<_>>().join(";")
}

/// The worn list after the action: `wear` replaces that slot's piece, `remove` takes this one off, `toggle` does
/// whichever fits.
pub fn after(list: &[(String, String)], slot: &str, id: &str, mode: &str) -> Vec<(String, String)> {
    let wearing = list.iter().any(|(one, worn)| one == slot && worn == id);
    let wear = match mode {
        "wear" => true,
        "remove" => false,
        _ => !wearing,
    };
    let mut out: Vec<(String, String)> = list.iter().filter(|(one, worn)| one != slot && worn != id).cloned().collect();
    if wear {
        out.push((slot.to_string(), id.to_string()));
    } else if !wearing {
        out = list.to_vec();
    }
    out.sort();
    out
}

pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    let raw = fields::text(data, "slot", "");
    if !raw.is_empty() && !SLOTS.contains(&raw.as_str()) {
        out.push(format!("\"slot\" must be one of: {}.", SLOTS.join(", ")));
    }
    if slot(data) == "trail" {
        if particle(data).is_empty() {
            out.push("A trail needs a \"particle\" (flame, heart, end_rod...).".into());
        }
    } else if item(data).is_empty() {
        out.push("A hat or back piece needs an \"item\": studio:<id> or a vanilla item.".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn one_piece_per_slot() {
        let worn = parse("hat=crown;trail=hearts;bad=x;back=");
        assert_eq!(worn, pairs(&[("hat", "crown"), ("trail", "hearts")]));
        assert_eq!(write(&worn), "hat=crown;trail=hearts");
        assert_eq!(after(&worn, "hat", "halo", "toggle"), pairs(&[("hat", "halo"), ("trail", "hearts")]));
        assert_eq!(after(&worn, "hat", "crown", "toggle"), pairs(&[("trail", "hearts")]));
        assert_eq!(after(&worn, "hat", "crown", "wear"), pairs(&[("hat", "crown"), ("trail", "hearts")]));
        assert_eq!(after(&worn, "back", "wings", "remove"), worn, "taking off what isn't worn changes nothing");
        let data: Data = serde_json::from_str(r#"{"slot":"trail"}"#).unwrap();
        assert_eq!(problems(&data).len(), 1);
        let back: Data = serde_json::from_str(r#"{"slot":"back","item":"studio:wings","back":0.1}"#).unwrap();
        assert!(problems(&back).is_empty());
        assert!((translation(&back).2 + 0.4).abs() < 1e-6);
    }
}

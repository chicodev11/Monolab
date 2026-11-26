//! Item quick edit (same files as the Paper add-on's `StudioHeldItems`): the app writes `live/item-ask.json` to read a
//! player's main-hand item or `live/item-set.json` (with `nbtHex`, item NBT as the fork's `to-nbt` makes it) to put
//! one there; the answer goes to `live/item.json` with the same `nonce`, as `nbtHex` or `error`. Only the app writes
//! these, when the user presses its buttons.

use super::Studio;
use pumpkin_plugin_api::common::Hand;
use pumpkin_plugin_api::{ItemStack, Player};
use serde_json::{Value, json};
use std::fs;

impl Studio {
    pub(super) async fn held_requests(&mut self, players: &[&Player]) {
        for (file, set) in [("item-ask.json", false), ("item-set.json", true)] {
            let path = self.root.join("live").join(file);
            let Ok(text) = fs::read_to_string(&path) else { continue };
            let _ = fs::remove_file(&path);
            let Ok(request) = serde_json::from_str::<Value>(&text) else { continue };
            let nonce = request.get("nonce").and_then(Value::as_u64).unwrap_or(0);
            let name = request.get("player").and_then(Value::as_str).unwrap_or_default();
            let answer = match players.iter().find(|one| one.get_name() == name) {
                None => Err(format!("{name} isn't online.")),
                Some(player) => held_answer(player, set.then_some(&request)).await,
            };
            let out = match answer {
                Ok(bytes) => json!({ "nonce": nonce, "nbtHex": hex(&bytes) }),
                Err(error) => json!({ "nonce": nonce, "error": error }),
            };
            let live = self.root.join("live");
            let _ = fs::create_dir_all(&live);
            let temp = live.join("item.json.tmp");
            if fs::write(&temp, out.to_string()).is_ok() {
                let _ = fs::rename(&temp, live.join("item.json"));
            }
        }
    }
}

async fn held_answer(player: &Player, set: Option<&Value>) -> Result<Vec<u8>, String> {
    if let Some(request) = set {
        let bytes = unhex(request.get("nbtHex").and_then(Value::as_str).ok_or("No item was sent.")?)?;
        let stack = ItemStack::from_nbt(&bytes).ok_or("The game didn't take that item.")?;
        player.set_item_in_hand(Hand::Right, Some(stack)).await;
    }
    let held = player.get_item_in_hand(Hand::Right).ok_or("Their main hand is empty.")?;
    Ok(held.to_nbt().await)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|one| format!("{one:02x}")).collect()
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) {
        return Err("The item bytes are cut short.".into());
    }
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).map_err(|_| "The item bytes aren't hex.".to_string())).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn hex_goes_both_ways() {
        assert_eq!(super::unhex(&super::hex(&[0, 10, 255])).unwrap(), vec![0, 10, 255]);
        assert!(super::unhex("abc").is_err());
    }
}

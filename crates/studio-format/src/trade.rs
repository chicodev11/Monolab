//! Trade lists (`trade` files): a villager-style trade screen of our own offers, opened by the `trade` action
//! (from an NPC, a command, a menu button...). Each offer takes one or two items (vanilla or `studio:<id>`) for one.
//! `uses` limits how often each player may take it until `restockMinutes` pass (0 = never). Pumpkin has no trade
//! screen for plugins yet (`docs/pumpkin-fork-requests.md`).

use crate::fields;
use serde_json::{Map, Value};

type Data = Map<String, Value>;

pub const MOST_OFFERS: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub buy: String,
    pub buy_amount: u32,
    pub buy2: String,
    pub buy2_amount: u32,
    pub sell: String,
    pub sell_amount: u32,
    /// Times each player may take it, 0 = no limit.
    pub uses: u32,
}

pub fn offers(data: &Data) -> Vec<Offer> {
    let Some(list) = data.get("offers").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(Value::as_object)
        .take(MOST_OFFERS)
        .map(|one| Offer {
            buy: fields::text(one, "buy", "").trim().to_string(),
            buy_amount: fields::number(one, "buyAmount", 1, 1, 64) as u32,
            buy2: fields::text(one, "buy2", "").trim().to_string(),
            buy2_amount: fields::number(one, "buy2Amount", 1, 1, 64) as u32,
            sell: fields::text(one, "sell", "").trim().to_string(),
            sell_amount: fields::number(one, "sellAmount", 1, 1, 64) as u32,
            uses: fields::number(one, "uses", 0, 0, 100_000) as u32,
        })
        .collect()
}

pub fn restock_minutes(data: &Data) -> u64 {
    fields::number(data, "restockMinutes", 0, 0, 60 * 24 * 30) as u64
}

pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    let list = offers(data);
    if list.is_empty() {
        out.push("A trade list needs \"offers\": what players give and get.".into());
    }
    if data.get("offers").and_then(Value::as_array).is_some_and(|all| all.len() > MOST_OFFERS) {
        out.push(format!("Up to {MOST_OFFERS} offers; the rest are left out."));
    }
    for (at, one) in list.iter().enumerate() {
        if one.buy.is_empty() || one.sell.is_empty() {
            out.push(format!("Offer {} needs an item to give (\"buy\") and one to get (\"sell\").", at + 1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_read_with_amounts() {
        let data: Data =
            serde_json::from_str(r#"{"offers":[{"buy":"emerald","buyAmount":3,"sell":"studio:ruby","uses":5},{"buy":"","sell":"stone"}]}"#)
                .unwrap();
        let list = offers(&data);
        assert_eq!(list[0].buy_amount, 3);
        assert_eq!(list[0].sell_amount, 1);
        assert_eq!(list[0].uses, 5);
        assert_eq!(problems(&data), vec!["Offer 2 needs an item to give (\"buy\") and one to get (\"sell\").".to_string()]);
        assert_eq!(problems(&Data::new()).len(), 1);
    }
}

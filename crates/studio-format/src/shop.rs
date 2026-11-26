//! Shops (`shop` files): a chest screen of goods, opened by the `shop` action (from an NPC, a command, a menu
//! button...). Left click buys `amount` of a good for `buy`, right click sells that many for `sell`, shift + right
//! click sells every one the player has. 0 means it can't be bought (or sold). Money is a player variable
//! (`currency: "variable"`, the `variable` named) on both add-ons, or the Vault economy (`currency: "vault"`, Paper
//! only). The same rules are in the Paper add-on's `StudioShops`.

use crate::fields;
use serde_json::{Map, Value};

type Data = Map<String, Value>;

pub const CURRENCIES: [&str; 2] = ["variable", "vault"];
/// A double chest.
pub const MOST_GOODS: usize = 54;

#[derive(Debug, Clone, PartialEq)]
pub struct Good {
    pub item: String,
    pub amount: u32,
    pub buy: f64,
    pub sell: f64,
}

pub fn goods(data: &Data) -> Vec<Good> {
    let Some(list) = data.get("goods").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(Value::as_object)
        .take(MOST_GOODS)
        .map(|one| Good {
            item: fields::text(one, "item", "").trim().to_string(),
            amount: fields::number(one, "amount", 1, 1, 64) as u32,
            buy: price(one, "buy"),
            sell: price(one, "sell"),
        })
        .collect()
}

fn price(data: &Data, key: &str) -> f64 {
    let value = data.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    if value.is_finite() { value.clamp(0.0, 1e12) } else { 0.0 }
}

pub fn currency(data: &Data) -> String {
    let one = fields::text(data, "currency", "variable");
    if CURRENCIES.contains(&one.as_str()) { one } else { "variable".into() }
}

/// The player variable that holds the money (`coins` when not set).
pub fn variable(data: &Data) -> String {
    let one = fields::text(data, "variable", "coins").trim().to_string();
    if one.is_empty() { "coins".into() } else { one }
}

/// What the money is called in the screen's lines.
pub fn money_name(data: &Data) -> String {
    let one = fields::text(data, "moneyName", "").trim().to_string();
    if one.is_empty() { "coins".into() } else { one }
}

/// Rows of the chest: as set (1-6), or just enough for the goods.
pub fn rows(data: &Data) -> u32 {
    let set = fields::number(data, "rows", 0, 0, 6) as u32;
    if set > 0 {
        return set;
    }
    (goods(data).len().div_ceil(9) as u32).clamp(1, 6)
}

/// `12` for whole amounts, `12.50` otherwise.
pub fn money_text(amount: f64) -> String {
    if (amount - amount.round()).abs() < 1e-9 { format!("{}", amount.round() as i64) } else { format!("{amount:.2}") }
}

/// The lines under a good in the screen (MiniMessage), the same on both add-ons.
pub fn lore(good: &Good, name: &str, balance: f64) -> Vec<String> {
    let mut out = Vec::new();
    if good.buy > 0.0 {
        out.push(format!("<gray>Left click: buy for <white>{} {name}</white>", money_text(good.buy)));
    }
    if good.sell > 0.0 {
        out.push(format!("<gray>Right click: sell for <white>{} {name}</white>", money_text(good.sell)));
        out.push("<dark_gray>Shift + right click: sell all".into());
    }
    out.push(format!("<dark_gray>You have {} {name}", money_text(balance)));
    out
}

pub fn problems(data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    let list = goods(data);
    if list.is_empty() {
        out.push("A shop needs \"goods\": what it buys and sells.".into());
    }
    if data.get("goods").and_then(Value::as_array).is_some_and(|all| all.len() > MOST_GOODS) {
        out.push(format!("Up to {MOST_GOODS} goods; the rest are left out."));
    }
    let wanted = fields::text(data, "currency", "variable");
    if !CURRENCIES.contains(&wanted.as_str()) {
        out.push(format!("\"currency\" must be one of: {}.", CURRENCIES.join(", ")));
    }
    for (at, one) in list.iter().enumerate() {
        if one.item.is_empty() {
            out.push(format!("Good {} needs an \"item\".", at + 1));
        } else if one.buy <= 0.0 && one.sell <= 0.0 {
            out.push(format!("Good {} has no price: set \"buy\", \"sell\" or both.", at + 1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goods_and_money_read() {
        let data: Data = serde_json::from_str(r#"{"currency":"vault","goods":[{"item":"diamond","amount":2,"buy":100,"sell":40.5},{"item":"stone"},{"item":"","buy":1}]}"#).unwrap();
        let list = goods(&data);
        assert_eq!(list[0], Good { item: "diamond".into(), amount: 2, buy: 100.0, sell: 40.5 });
        assert_eq!(currency(&data), "vault");
        assert_eq!(variable(&data), "coins");
        assert_eq!(rows(&data), 1);
        assert_eq!(money_text(40.5), "40.50");
        assert_eq!(money_text(100.0), "100");
        assert_eq!(lore(&list[0], "coins", 7.0).len(), 4);
        assert_eq!(problems(&data), vec!["Good 2 has no price: set \"buy\", \"sell\" or both.".to_string(), "Good 3 needs an \"item\".".to_string()]);
        assert_eq!(problems(&Data::new()).len(), 1);
    }
}

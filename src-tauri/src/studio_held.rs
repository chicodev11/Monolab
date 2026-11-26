//! In-game item quick edit: the app asks the add-on for a player's main-hand item and puts an edited one back,
//! through files in `<studio>/live/` (both add-ons, local or over SFTP): `item-ask.json` / `item-set.json` go in,
//! `item.json` comes back with the same `nonce`. Paper answers in the game's item JSON (`json`); Pumpkin in item
//! NBT as hex (`nbtHex`), which is turned into the same JSON shape here and back.

use serde_json::{Map, Number, Value};

pub const ASK_FILE: &str = "live/item-ask.json";
pub const SET_FILE: &str = "live/item-set.json";
pub const ANSWER_FILE: &str = "live/item.json";
/// The test tool's request and answer (both add-ons' `StudioTests`).
pub const FIRE_FILE: &str = "live/fire.json";
pub const FIRED_FILE: &str = "live/fired.json";

/// What one rule did in a test event: `matched` is false when its "when" settings (blocks, message…) didn't take
/// the event; then `passed` and `ran` are empty.
#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct TestRun {
    pub rule: String,
    pub matched: bool,
    pub passed: bool,
    pub ran: Vec<String>,
}

/// The test answer's runs, or why there are none.
pub fn read_runs(answer: &Value) -> Result<Vec<TestRun>, String> {
    if let Some(problem) = answer.get("error").and_then(Value::as_str) {
        return Err(problem.to_string());
    }
    let runs = answer.get("runs").and_then(Value::as_array).ok_or("The add-on answered without runs.")?;
    Ok(runs
        .iter()
        .map(|one| TestRun {
            rule: one.get("rule").and_then(Value::as_str).unwrap_or_default().to_string(),
            matched: one.get("matched").and_then(Value::as_bool).unwrap_or(false),
            passed: one.get("passed").and_then(Value::as_bool).unwrap_or(false),
            ran: one.get("ran").and_then(Value::as_array).map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default(),
        })
        .collect())
}

/// Item NBT (as the fork's `to-nbt` gives it) as JSON: numbers lose their NBT width, which the game's item reader
/// doesn't mind (it reads any number type), arrays become lists.
pub fn json_of_nbt(value: &fastnbt::Value) -> Value {
    use fastnbt::Value as Nbt;
    match value {
        Nbt::Byte(one) => Value::from(*one),
        Nbt::Short(one) => Value::from(*one),
        Nbt::Int(one) => Value::from(*one),
        Nbt::Long(one) => Value::from(*one),
        Nbt::Float(one) => Number::from_f64(f64::from(*one)).map_or(Value::Null, Value::Number),
        Nbt::Double(one) => Number::from_f64(*one).map_or(Value::Null, Value::Number),
        Nbt::String(one) => Value::String(one.clone()),
        Nbt::ByteArray(list) => Value::Array(list.iter().map(|one| Value::from(*one)).collect()),
        Nbt::IntArray(list) => Value::Array(list.iter().map(|one| Value::from(*one)).collect()),
        Nbt::LongArray(list) => Value::Array(list.iter().map(|one| Value::from(*one)).collect()),
        Nbt::List(list) => Value::Array(list.iter().map(json_of_nbt).collect()),
        Nbt::Compound(map) => Value::Object(map.iter().map(|(key, one)| (key.clone(), json_of_nbt(one))).collect()),
    }
}

/// JSON back to NBT for the fork's `from-nbt`: whole numbers are ints (longs when too big), others doubles, true and
/// false bytes. A list with any fraction is all doubles, since NBT lists hold one type.
pub fn nbt_of_json(value: &Value) -> Result<fastnbt::Value, String> {
    use fastnbt::Value as Nbt;
    Ok(match value {
        Value::Null => return Err("null isn't allowed in an item.".into()),
        Value::Bool(one) => Nbt::Byte(i8::from(*one)),
        Value::Number(number) => number_of(number, false),
        Value::String(text) => Nbt::String(text.clone()),
        Value::Array(list) => {
            let fractions = list.iter().any(|one| one.as_number().is_some_and(|number| number.as_i64().is_none()));
            let mut out = Vec::new();
            for one in list {
                out.push(match one {
                    Value::Number(number) => number_of(number, fractions),
                    other => nbt_of_json(other)?,
                });
            }
            Nbt::List(out)
        }
        Value::Object(map) => {
            let mut out = std::collections::HashMap::new();
            for (key, one) in map {
                out.insert(key.clone(), nbt_of_json(one)?);
            }
            Nbt::Compound(out)
        }
    })
}

fn number_of(number: &Number, as_double: bool) -> fastnbt::Value {
    use fastnbt::Value as Nbt;
    match number.as_i64() {
        Some(whole) if !as_double => i32::try_from(whole).map_or(Nbt::Long(whole), Nbt::Int),
        _ => Nbt::Double(number.as_f64().unwrap_or(0.0)),
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|one| format!("{one:02x}")).collect()
}

pub fn unhex(text: &str) -> Result<Vec<u8>, String> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) {
        return Err("The item bytes are cut short.".into());
    }
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).map_err(|_| "The item bytes aren't hex.".to_string())).collect()
}

/// The answer file's item as pretty JSON, or why there is none.
pub fn read_answer(answer: &Value) -> Result<String, String> {
    if let Some(problem) = answer.get("error").and_then(Value::as_str) {
        return Err(problem.to_string());
    }
    let item = if let Some(json) = answer.get("json") {
        match json {
            Value::String(text) => serde_json::from_str(text).map_err(|error| format!("The add-on's item isn't JSON: {error}"))?,
            other => other.clone(),
        }
    } else if let Some(bytes) = answer.get("nbtHex").and_then(Value::as_str) {
        json_of_nbt(&crate::nbt::decode_nbt(unhex(bytes)?)?)
    } else {
        return Err("The add-on answered without an item.".into());
    };
    serde_json::to_string_pretty(&item).map_err(|error| error.to_string())
}

/// The request to put `text` (item JSON) in the player's hand: JSON for Paper, NBT hex for Pumpkin.
pub fn set_request(player: &str, nonce: u64, text: &str, pumpkin: bool) -> Result<Value, String> {
    let item: Value = serde_json::from_str(text).map_err(|error| format!("That isn't JSON: {error}"))?;
    if !item.get("id").is_some_and(Value::is_string) {
        return Err("An item needs an \"id\", like \"minecraft:diamond_sword\".".into());
    }
    let mut out = Map::new();
    out.insert("player".into(), Value::String(player.to_string()));
    out.insert("nonce".into(), Value::from(nonce));
    if pumpkin {
        let bytes = fastnbt::to_bytes(&nbt_of_json(&item)?).map_err(|error| format!("Could not write NBT: {error}"))?;
        out.insert("nbtHex".into(), Value::String(hex(&bytes)));
    } else {
        out.insert("json".into(), item);
    }
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_go_through_nbt_and_back() {
        let item = serde_json::json!({ "id": "minecraft:diamond_sword", "count": 1, "components": { "minecraft:damage": 3, "minecraft:custom_model_data": { "floats": [1.5, 2] }, "minecraft:unbreakable": {} } });
        let nbt = nbt_of_json(&item).unwrap();
        let bytes = fastnbt::to_bytes(&nbt).unwrap();
        let back = read_answer(&serde_json::json!({ "nbtHex": hex(&bytes) })).unwrap();
        let read: Value = serde_json::from_str(&back).unwrap();
        assert_eq!(read["id"], "minecraft:diamond_sword");
        assert_eq!(read["components"]["minecraft:damage"], 3);
        assert_eq!(read["components"]["minecraft:custom_model_data"]["floats"], serde_json::json!([1.5, 2.0]));
        assert_eq!(unhex("0aff").unwrap(), vec![10, 255]);
        assert!(unhex("0").is_err());
        assert_eq!(read_answer(&serde_json::json!({ "error": "Their main hand is empty." })).unwrap_err(), "Their main hand is empty.");
        let paper = set_request("Mina", 7, r#"{"id":"minecraft:stone","count":2}"#, false).unwrap();
        assert_eq!(paper["json"]["count"], 2);
        assert!(set_request("Mina", 7, r#"{"count":2}"#, true).is_err());
    }

    #[test]
    fn test_runs_are_read() {
        let runs = read_runs(&serde_json::json!({ "nonce": 1, "runs": [{ "rule": "welcome", "matched": true, "passed": true, "ran": ["message"] }, { "rule": "ores", "matched": false }] })).unwrap();
        assert_eq!(runs[0], TestRun { rule: "welcome".into(), matched: true, passed: true, ran: vec!["message".into()] });
        assert!(!runs[1].matched && runs[1].ran.is_empty());
        assert_eq!(read_runs(&serde_json::json!({ "error": "Mina isn't online." })).unwrap_err(), "Mina isn't online.");
    }
}

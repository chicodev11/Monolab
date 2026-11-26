//! In-game test tool (same files as the Paper add-on's `StudioTests`): the app writes `live/fire.json` (`nonce`,
//! `event`, `rule`, `player`, `vars`) to make a rule event happen without anyone doing it. The rules run as for the
//! real event (cooldowns skipped); the answer goes to `live/fired.json` with the same `nonce`, as `runs` or `error`.
//! Only the app writes the request, when the user presses its button.

use super::Studio;
use super::play::RULE_EVENTS;
use pumpkin_plugin_api::{Player, Server};
use serde_json::{Value, json};
use std::fs;

impl Studio {
    pub(super) async fn test_requests(&mut self, server: &Server, players: &[&Player]) {
        let path = self.root.join("live").join("fire.json");
        let Ok(text) = fs::read_to_string(&path) else { return };
        let _ = fs::remove_file(&path);
        let Ok(request) = serde_json::from_str::<Value>(&text) else { return };
        let nonce = request.get("nonce").and_then(Value::as_u64).unwrap_or(0);
        let answer = self.test_answer(server, players, &request).await;
        let out = match answer {
            Ok(runs) => json!({ "nonce": nonce, "runs": runs }),
            Err(error) => json!({ "nonce": nonce, "error": error }),
        };
        let live = self.root.join("live");
        let _ = fs::create_dir_all(&live);
        let temp = live.join("fired.json.tmp");
        if fs::write(&temp, out.to_string()).is_ok() {
            let _ = fs::rename(&temp, live.join("fired.json"));
        }
    }

    async fn test_answer(&mut self, server: &Server, players: &[&Player], request: &Value) -> Result<Vec<Value>, String> {
        let text = |key: &str| request.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_string();
        let (event, rule, name) = (text("event"), text("rule"), text("player"));
        if event.contains(':') {
            return Err("Pumpkin plugins can't add studio triggers yet, so plugin events can't be tested here.".into());
        }
        if !RULE_EVENTS.contains(&event.as_str()) {
            return Err(format!("There's no event called {event}."));
        }
        let extra = vars_of(request);
        if name.is_empty() {
            if !matches!(event.as_str(), "timer" | "schedule") {
                return Err("This event happens to a player. Pick one who is online.".into());
            }
            return Ok(self.fire_test(server, &event, &rule, None, extra).await);
        }
        let player = players.iter().find(|one| one.get_name() == name).ok_or_else(|| format!("{name} isn't online."))?;
        Ok(self.fire_test(server, &event, &rule, Some(player), extra).await)
    }
}

/// The request's placeholders; block names lose `minecraft:` like the real events give them.
fn vars_of(request: &Value) -> Vec<(String, String)> {
    let Some(vars) = request.get("vars").and_then(Value::as_object) else { return Vec::new() };
    vars.iter()
        .map(|(key, one)| {
            let text = one.as_str().map_or_else(|| one.to_string(), str::to_string);
            let text = if key == "block" { text.trim().to_ascii_lowercase().trim_start_matches("minecraft:").to_string() } else { text };
            (key.clone(), text)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn vars_are_read_and_blocks_cleaned() {
        let request = serde_json::json!({ "vars": { "block": "minecraft:Stone", "message": "hi", "count": 3 } });
        let mut vars = super::vars_of(&request);
        vars.sort();
        assert_eq!(vars, vec![("block".into(), "stone".into()), ("count".into(), "3".into()), ("message".into(), "hi".into())]);
        assert!(super::vars_of(&serde_json::json!({})).is_empty());
    }
}

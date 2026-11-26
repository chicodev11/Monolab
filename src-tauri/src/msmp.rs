//! Minecraft Server Management Protocol (MSMP, 1.21.9+): JSON-RPC 2.0 over a WebSocket the server itself opens.
//! Lets the app change server settings and game rules while the server runs, without a restart or a console
//! command. Shapes come from the server's own schema (`json-rpc-api-schema.json`, API 3.1.0 on 26.3).
//! Monolab only talks to it on the server's own machine, without TLS, so `setup` turns TLS off and binds it to localhost:
//! on this computer directly, on an SSH server through an SSH tunnel to that machine's localhost.

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

const TIMEOUT: Duration = Duration::from_secs(8);

/// Where the management server listens: this computer's localhost, or an SSH server's (reached through a tunnel).
#[derive(Clone)]
pub enum Link {
    Local(u16),
    Ssh(std::sync::Arc<crate::remote::Conn>, u16),
}

/// Every `minecraft:serversettings/<key>` with its setter's parameter name and value kind.
pub const SETTINGS: &[(&str, &str, &str)] = &[
    ("difficulty", "difficulty", "difficulty"),
    ("game_mode", "mode", "game_mode"),
    ("force_game_mode", "force", "boolean"),
    ("max_players", "max", "integer"),
    ("motd", "message", "string"),
    ("view_distance", "distance", "integer"),
    ("simulation_distance", "distance", "integer"),
    ("allow_flight", "allow", "boolean"),
    ("spawn_protection_radius", "radius", "integer"),
    ("player_idle_timeout", "seconds", "integer"),
    ("pause_when_empty_seconds", "seconds", "integer"),
    ("autosave", "enable", "boolean"),
    ("use_allowlist", "use", "boolean"),
    ("enforce_allowlist", "enforce", "boolean"),
    ("accept_transfers", "accept", "boolean"),
    ("hide_online_players", "hide", "boolean"),
    ("status_replies", "enable", "boolean"),
    ("status_heartbeat_interval", "seconds", "integer"),
    ("entity_broadcast_range", "percentage_points", "integer"),
    ("operator_user_permission_level", "level", "integer"),
];

pub const DIFFICULTIES: [&str; 4] = ["peaceful", "easy", "normal", "hard"];
pub const GAME_MODES: [&str; 4] = ["survival", "creative", "adventure", "spectator"];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub secret: String,
    pub tls: bool,
}

pub fn read_config(properties: &str) -> Config {
    let read = |key: &str| crate::proxy::property(properties, key).unwrap_or_default();
    Config {
        enabled: read("management-server-enabled").trim() == "true",
        host: read("management-server-host").trim().to_string(),
        port: read("management-server-port").trim().parse().unwrap_or(0),
        secret: read("management-server-secret").trim().to_string(),
        // On unless set to false, like the server.
        tls: read("management-server-tls-enabled").trim() != "false",
    }
}

impl Config {
    /// Monolab can connect: on, a fixed port, a secret, no TLS, and only on this computer.
    pub fn ready(&self) -> bool {
        self.enabled && !self.tls && self.port > 0 && !self.secret.is_empty() && matches!(self.host.as_str(), "" | "localhost" | "127.0.0.1")
    }
}

/// server.properties with the management server on, local only, without TLS, on `port` with `secret`.
/// A port or secret that is already set is kept.
pub fn setup_properties(properties: &str, port: u16, secret: &str) -> String {
    let now = read_config(properties);
    let mut text = crate::proxy::set_property(properties, "management-server-enabled", "true");
    text = crate::proxy::set_property(&text, "management-server-host", "localhost");
    text = crate::proxy::set_property(&text, "management-server-tls-enabled", "false");
    if now.port == 0 {
        text = crate::proxy::set_property(&text, "management-server-port", &port.to_string());
    }
    if now.secret.is_empty() {
        text = crate::proxy::set_property(&text, "management-server-secret", secret);
    }
    text
}

/// 40 letters and digits, as the server wants.
pub fn new_secret() -> String {
    crate::web::make_key().chars().take(40).collect()
}

/// Versions with MSMP: 1.21.9 and newer (the 26.x numbering included).
pub fn version_has_msmp(version: &str) -> bool {
    let parts: Vec<u32> = version.split(['.', '-', '+', ' ']).map_while(|one| one.parse().ok()).collect();
    match parts.as_slice() {
        [1, minor, rest @ ..] => *minor > 21 || (*minor == 21 && rest.first().copied().unwrap_or(0) >= 9),
        [year, ..] => *year >= 26,
        [] => false,
    }
}

pub fn setting_param(key: &str) -> Option<(&'static str, &'static str)> {
    SETTINGS.iter().find(|(name, _, _)| *name == key).map(|(_, param, kind)| (*param, *kind))
}

/// Checks a value against its setting's kind before it goes to the server.
pub fn check_setting(kind: &str, value: &Value) -> Result<(), String> {
    let fine = match kind {
        "boolean" => value.is_boolean(),
        "integer" => value.as_i64().is_some(),
        "string" => value.is_string(),
        "difficulty" => value.as_str().is_some_and(|one| DIFFICULTIES.contains(&one)),
        "game_mode" => value.as_str().is_some_and(|one| GAME_MODES.contains(&one)),
        _ => false,
    };
    if fine { Ok(()) } else { Err("That value doesn't fit this setting.".into()) }
}

/// Before API 2.0.0 game rule values were sent as text.
pub fn rule_value(api_version: &str, value: &Value) -> Value {
    let major: u32 = api_version.split('.').next().and_then(|one| one.parse().ok()).unwrap_or(3);
    if major < 2 {
        match value {
            Value::Bool(one) => json!(one.to_string()),
            Value::Number(one) => json!(one.to_string()),
            other => other.clone(),
        }
    } else {
        value.clone()
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MsmpSetting {
    pub key: String,
    /// `boolean`, `integer`, `string`, `difficulty` or `game_mode`.
    pub kind: String,
    /// JSON text: true/false, a number or a quoted word.
    pub value: String,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MsmpRule {
    pub key: String,
    /// `boolean` or `integer`.
    pub kind: String,
    /// JSON text: true/false, a number or a quoted word.
    pub value: String,
}

pub fn rules_from(result: &Value) -> Vec<MsmpRule> {
    let mut rules: Vec<MsmpRule> = result
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|one| {
            let key = one.get("key")?.as_str()?.to_string();
            let value = one.get("value").cloned().unwrap_or(Value::Null);
            // Old servers send text; read it back into a boolean or number.
            let value = match &value {
                Value::String(text) if text == "true" || text == "false" => json!(text == "true"),
                Value::String(text) => text.parse::<i64>().map(|number| json!(number)).unwrap_or(value),
                _ => value,
            };
            let kind = one
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| if value.is_boolean() { "boolean".into() } else { "integer".into() });
            Some(MsmpRule { key, kind, value: value.to_string() })
        })
        .collect();
    rules.sort_by(|a, b| a.key.cmp(&b.key));
    rules
}

/// Sends several requests over one connection and returns each answer in order.
pub async fn call_many(at: &Link, secret: &str, calls: &[(String, Value)]) -> Result<Vec<Result<Value, String>>, String> {
    let port = match at {
        Link::Local(port) | Link::Ssh(_, port) => *port,
    };
    let mut request = format!("ws://127.0.0.1:{port}")
        .into_client_request()
        .map_err(|error| error.to_string())?;
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {secret}").parse().map_err(|_| "The management secret has characters that can't be sent.".to_string())?);
    match at {
        Link::Local(_) => {
            let stream = tokio::time::timeout(TIMEOUT, tokio::net::TcpStream::connect(("127.0.0.1", port)))
                .await
                .map_err(|_| "The server's management connection didn't answer.".to_string())?
                .map_err(|error| format!("Could not reach the server's management connection: {error}"))?;
            talk(stream, request, calls).await
        }
        Link::Ssh(conn, _) => {
            let stream = tokio::time::timeout(TIMEOUT, conn.forward("127.0.0.1", port))
                .await
                .map_err(|_| "The SSH server didn't open a tunnel to the management connection.".to_string())??;
            talk(stream, request, calls).await
        }
    }
}

/// The WebSocket conversation itself, over any connection to the management server.
async fn talk<S>(stream: S, request: tokio_tungstenite::tungstenite::handshake::client::Request, calls: &[(String, Value)]) -> Result<Vec<Result<Value, String>>, String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut socket, _) = tokio::time::timeout(TIMEOUT, tokio_tungstenite::client_async(request, stream))
        .await
        .map_err(|_| "The server's management connection didn't answer.".to_string())?
        .map_err(|error| match error {
            tokio_tungstenite::tungstenite::Error::Http(response) if response.status() == 401 => {
                "The server turned down the management secret. Restart the server after changing it.".to_string()
            }
            other => format!("Could not reach the server's management connection: {other}"),
        })?;
    for (at, (method, params)) in calls.iter().enumerate() {
        // Methods without parameters take none at all: an empty object is "Invalid params".
        let mut message = json!({ "jsonrpc": "2.0", "id": at + 1, "method": method });
        if !params.is_null() {
            message["params"] = params.clone();
        }
        socket.send(Message::text(message.to_string())).await.map_err(|error| error.to_string())?;
    }
    let mut answers: Vec<Option<Result<Value, String>>> = vec![None; calls.len()];
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while answers.iter().any(Option::is_none) {
        let next = tokio::time::timeout_at(deadline, socket.next())
            .await
            .map_err(|_| "The server took too long to answer.".to_string())?;
        let Some(frame) = next else { break };
        let Ok(Message::Text(text)) = frame.map_err(|error| error.to_string()) else { continue };
        let Ok(value) = serde_json::from_str::<Value>(text.as_str()) else { continue };
        // Notifications have no id; only answers to our requests count.
        let Some(id) = value.get("id").and_then(Value::as_u64) else { continue };
        let Some(slot) = answers.get_mut(id.saturating_sub(1) as usize) else { continue };
        *slot = Some(match value.get("error") {
            Some(error) => Err(error.get("message").and_then(Value::as_str).unwrap_or("The server reported an error.").to_string()),
            None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
        });
    }
    let _ = socket.close(None).await;
    Ok(answers.into_iter().map(|one| one.unwrap_or_else(|| Err("No answer.".into()))).collect())
}

pub async fn call(at: &Link, secret: &str, method: &str, params: Value) -> Result<Value, String> {
    call_many(at, secret, &[(method.to_string(), params)]).await?.pop().unwrap_or_else(|| Err("No answer.".into()))
}

pub async fn api_version(at: &Link, secret: &str) -> Result<String, String> {
    let schema = call(at, secret, "rpc.discover", Value::Null).await?;
    Ok(schema.pointer("/info/version").and_then(Value::as_str).unwrap_or_default().to_string())
}

pub async fn settings(at: &Link, secret: &str) -> Result<Vec<MsmpSetting>, String> {
    let calls: Vec<(String, Value)> = SETTINGS.iter().map(|(key, _, _)| (format!("minecraft:serversettings/{key}"), Value::Null)).collect();
    let answers = call_many(at, secret, &calls).await?;
    Ok(SETTINGS
        .iter()
        .zip(answers)
        .filter_map(|((key, _, kind), answer)| answer.ok().map(|value| MsmpSetting { key: key.to_string(), kind: kind.to_string(), value: value.to_string() }))
        .collect())
}

pub async fn set_setting(at: &Link, secret: &str, key: &str, value: Value) -> Result<Value, String> {
    let (param, kind) = setting_param(key).ok_or("Unknown server setting.")?;
    check_setting(kind, &value)?;
    call(at, secret, &format!("minecraft:serversettings/{key}/set"), json!({ param: value })).await
}

pub async fn rules(at: &Link, secret: &str) -> Result<Vec<MsmpRule>, String> {
    Ok(rules_from(&call(at, secret, "minecraft:gamerules", Value::Null).await?))
}

pub async fn set_rule(at: &Link, secret: &str, api: &str, key: &str, value: Value) -> Result<(), String> {
    if !(value.is_boolean() || value.as_i64().is_some()) {
        return Err("A game rule is on/off or a whole number.".into());
    }
    if key.is_empty() || key.len() > 80 || !key.chars().all(|one| one.is_ascii_alphanumeric() || matches!(one, '_' | ':' | '.' | '/')) {
        return Err("Unknown game rule.".into());
    }
    call(at, secret, "minecraft:gamerules/update", json!({ "gamerule": { "key": key, "value": rule_value(api, &value) } })).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_sets_up_the_properties() {
        let fresh = "motd=hi\nmanagement-server-enabled=false\nmanagement-server-port=0\nmanagement-server-secret=\n";
        let config = read_config(fresh);
        assert!(!config.enabled && config.tls && config.port == 0 && !config.ready());
        let done = setup_properties(fresh, 25585, "abc");
        let config = read_config(&done);
        assert!(config.ready(), "{done}");
        assert_eq!((config.port, config.secret.as_str(), config.host.as_str()), (25585, "abc", "localhost"));
        assert!(done.starts_with("motd=hi\n"));
        let kept = setup_properties("management-server-port=30000\nmanagement-server-secret=keep\n", 1, "new");
        let config = read_config(&kept);
        assert_eq!((config.port, config.secret.as_str()), (30000, "keep"));
        assert!(!read_config("management-server-enabled=true\nmanagement-server-host=0.0.0.0\nmanagement-server-port=1\nmanagement-server-secret=x\nmanagement-server-tls-enabled=false").ready());
    }

    #[test]
    fn knows_which_versions_have_it() {
        assert!(version_has_msmp("1.21.9"));
        assert!(version_has_msmp("1.21.11"));
        assert!(version_has_msmp("26.3"));
        assert!(!version_has_msmp("1.21.8"));
        assert!(!version_has_msmp("1.20.6"));
        assert!(!version_has_msmp("1.21"));
        assert!(!version_has_msmp("snapshot"));
    }

    #[test]
    fn checks_values_before_sending() {
        assert!(check_setting("boolean", &json!(true)).is_ok());
        assert!(check_setting("boolean", &json!("true")).is_err());
        assert!(check_setting("integer", &json!(10)).is_ok());
        assert!(check_setting("difficulty", &json!("hard")).is_ok());
        assert!(check_setting("difficulty", &json!("insane")).is_err());
        assert!(check_setting("game_mode", &json!("creative")).is_ok());
        assert_eq!(setting_param("max_players"), Some(("max", "integer")));
        assert_eq!(setting_param("nope"), None);
        assert_eq!(new_secret().len(), 40);
    }

    #[test]
    fn game_rules_old_and_new() {
        assert_eq!(rule_value("1.1.0", &json!(true)), json!("true"));
        assert_eq!(rule_value("3.1.0", &json!(5)), json!(5));
        let rules = rules_from(&json!([
            { "type": "integer", "key": "random_tick_speed", "value": 3 },
            { "type": "boolean", "key": "keep_inventory", "value": false },
            { "key": "old_style", "value": "true" },
        ]));
        assert_eq!(rules.iter().map(|one| one.key.as_str()).collect::<Vec<_>>(), ["keep_inventory", "old_style", "random_tick_speed"]);
        assert_eq!(rules[1].value, "true");
        assert_eq!(rules[1].kind, "boolean");
    }

    /// Talks to a real server: `MSMP_PORT=<port> MSMP_SECRET=<secret> cargo test real_msmp -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs a running server with the management server on"]
    fn real_msmp() {
        let port = Link::Local(std::env::var("MSMP_PORT").expect("MSMP_PORT").parse().unwrap());
        let port = &port;
        let secret = std::env::var("MSMP_SECRET").expect("MSMP_SECRET");
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let api = api_version(port, &secret).await.expect("discover");
            eprintln!("api {api}");
            let list = settings(port, &secret).await.expect("settings");
            eprintln!("settings {}: {:?}", list.len(), list.iter().map(|one| format!("{}={}", one.key, one.value)).collect::<Vec<_>>());
            assert_eq!(list.len(), SETTINGS.len());
            let before: Value = serde_json::from_str(&list.iter().find(|one| one.key == "max_players").unwrap().value).unwrap();
            let after = set_setting(port, &secret, "max_players", json!(37)).await.expect("set max_players");
            eprintln!("max_players {before} -> {after}");
            assert_eq!(after, json!(37));
            set_setting(port, &secret, "difficulty", json!("hard")).await.expect("set difficulty");
            set_setting(port, &secret, "max_players", before).await.expect("put back");
            let rules = rules(port, &secret).await.expect("rules");
            eprintln!("rules {}: first {:?}", rules.len(), rules.first());
            let keep = rules.iter().find(|one| one.key.contains("keep_inventory")).expect("keep_inventory").clone();
            set_rule(port, &secret, &api, &keep.key, json!(keep.value != "true")).await.expect("set rule");
            let again = rules_from(&call(port, &secret, "minecraft:gamerules", Value::Null).await.unwrap());
            let now = again.iter().find(|one| one.key == keep.key).unwrap();
            eprintln!("{} {} -> {}", keep.key, keep.value, now.value);
            assert_ne!(now.value, keep.value);
            assert!(call(port, "wrong-secret", "rpc.discover", Value::Null).await.is_err());
        });
    }
}

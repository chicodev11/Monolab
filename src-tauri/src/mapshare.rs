//! The world map's web page runs inside the server's Monolab add-on, on a port of its own (like BlueMap), so friends
//! reach it through a router port forward. The app only writes the add-on's `web.json` and reads `web-status.json`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_PORT: u16 = 8100;
/// The first add-on version that carries the web map.
pub const FIRST_ADDON: u32 = 28;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub port: u16,
    /// A secret first part of the address; empty means anyone who finds the port can look.
    pub token: String,
    pub players: bool,
    pub name: String,
    pub labels: HashMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Status {
    pub at: u64,
    pub running: bool,
    pub port: u16,
    pub error: Option<String>,
}

pub fn read_settings(text: &str) -> Settings {
    let mut one: Settings = serde_json::from_str(text).unwrap_or_default();
    if !port_ok(one.port) {
        one.port = DEFAULT_PORT;
    }
    one.token = clean_token(&one.token);
    one
}

pub fn read_status(text: &str) -> Option<Status> {
    serde_json::from_str(text).ok()
}

pub fn port_ok(port: u16) -> bool {
    port >= 1024
}

pub fn clean_token(token: &str) -> String {
    token.chars().filter(char::is_ascii_alphanumeric).take(64).collect()
}

pub fn make_token() -> String {
    crate::web::make_key().chars().take(32).collect()
}

/// What follows `host:port` in the address friends open.
pub fn path_of(settings: &Settings) -> String {
    if settings.token.is_empty() {
        "/".to_string()
    } else {
        format!("/{}/", settings.token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_the_addon_expects() {
        let mut one = Settings { enabled: true, port: 8100, token: "abc".into(), players: true, name: "Home".into(), ..Default::default() };
        one.labels.insert("night".into(), "밤".into());
        let text = serde_json::to_string(&one).unwrap();
        for key in ["\"enabled\":true", "\"port\":8100", "\"token\":\"abc\"", "\"players\":true", "\"name\":\"Home\""] {
            assert!(text.contains(key), "{key} missing from {text}");
        }
        let back = read_settings(&text);
        assert_eq!(back.port, 8100);
        assert_eq!(back.labels.get("night").map(String::as_str), Some("밤"));
    }

    #[test]
    fn falls_back_on_broken_settings() {
        assert_eq!(read_settings("not json").port, DEFAULT_PORT);
        assert_eq!(read_settings("{\"port\":80}").port, DEFAULT_PORT, "ports below 1024 need admin rights");
        assert!(!read_settings("").enabled);
    }

    #[test]
    fn keeps_tokens_url_safe() {
        assert_eq!(clean_token("ab/../c d"), "abcd");
        let made = make_token();
        assert_eq!(made.len(), 32);
        assert_eq!(clean_token(&made), made);
        assert_ne!(make_token(), made);
    }

    #[test]
    fn builds_the_path_friends_open() {
        let mut one = Settings::default();
        assert_eq!(path_of(&one), "/");
        one.token = "abc".into();
        assert_eq!(path_of(&one), "/abc/");
    }

    #[test]
    fn reads_the_addons_status() {
        let status = read_status("{\"at\":5,\"running\":true,\"port\":8100}").unwrap();
        assert!(status.running && status.port == 8100 && status.error.is_none());
        let failed = read_status("{\"at\":5,\"running\":false,\"port\":8100,\"error\":\"Address already in use\"}").unwrap();
        assert_eq!(failed.error.as_deref(), Some("Address already in use"));
        assert!(read_status("nope").is_none());
    }
}

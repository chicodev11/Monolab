use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

static LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    pub token: String,
    pub server_id: String,
    pub created_at: u64,
    #[serde(default)]
    pub labels: HashMap<String, String>,
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|dir| dir.join("map-shares.json"))
}

fn load(app: &AppHandle) -> Vec<Share> {
    file(app)
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, shares: &[Share]) -> Result<(), String> {
    let path = file(app).ok_or("The app data folder is unavailable.")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(shares).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())
}

pub fn of_server(app: &AppHandle, server: &str) -> Option<Share> {
    load(app).into_iter().find(|one| one.server_id == server)
}

pub fn create(app: &AppHandle, server: &str, labels: HashMap<String, String>, now: u64) -> Result<Share, String> {
    let _held = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut shares = load(app);
    shares.retain(|one| one.server_id != server);
    let share = Share { token: crate::web::make_key(), server_id: server.to_string(), created_at: now, labels };
    shares.push(share.clone());
    save(app, &shares)?;
    Ok(share)
}

pub fn remove(app: &AppHandle, server: &str) -> Result<(), String> {
    let _held = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut shares = load(app);
    shares.retain(|one| one.server_id != server);
    save(app, &shares)
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The share a link token opens, if any.
pub fn find(app: &AppHandle, token: &str) -> Option<Share> {
    if token.len() < 32 || !token.bytes().all(|one| one.is_ascii_hexdigit()) {
        return None;
    }
    load(app).into_iter().find(|one| same(&one.token, token))
}

/// Splits `/m/<token>/<rest>` into the token and what follows.
pub fn split(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("/m/")?;
    Some(match rest.split_once('/') {
        Some((token, tail)) => (token, tail),
        None => (rest, ""),
    })
}

pub const PAGE: &str = include_str!("web/mapshare.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_share_paths() {
        assert_eq!(split("/m/abc"), Some(("abc", "")));
        assert_eq!(split("/m/abc/tile"), Some(("abc", "tile")));
        assert_eq!(split("/m/abc/"), Some(("abc", "")));
        assert_eq!(split("/api/list"), None);
        assert!(same("abcd", "abcd"));
        assert!(!same("abcd", "abce"));
        assert!(!same("abcd", "abc"));
    }
}

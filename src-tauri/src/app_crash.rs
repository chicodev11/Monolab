//! Opt-in crash reports. When Monolab itself breaks (a Rust panic, or a screen that can't draw), the
//! report is kept on this PC. Next time the app asks, showing exactly what would be sent; nothing goes
//! anywhere unless the user says so. The account server keeps it without any account or address.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// What the account server takes (relay/src/accounts/http.rs).
pub const MESSAGE_MAX: usize = 1_000;
pub const DETAIL_MAX: usize = 12_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingCrash {
    /// "panic" (the app's core) or "ui" (a screen).
    pub kind: String,
    pub message: String,
    pub detail: String,
    pub version: String,
    pub os: String,
    /// Unix seconds.
    pub at: u64,
}

/// Where the report waits: next to the app's crash log.
pub fn folder() -> Option<PathBuf> {
    Some(crate::data_root::current()?.join("logs"))
}

fn file() -> Option<PathBuf> {
    folder().map(|folder| folder.join("pending-crash.json"))
}

fn cut(text: &str, most: usize) -> String {
    text.chars().take(most).collect()
}

/// The user's own folder names don't go into a report.
pub fn scrub(text: &str) -> String {
    let mut text = text.to_string();
    for variable in ["USERPROFILE", "HOME", "APPDATA", "LOCALAPPDATA"] {
        if let Ok(value) = std::env::var(variable) {
            if value.len() > 3 {
                text = text.replace(&value, &format!("%{variable}%"));
            }
        }
    }
    if let Ok(name) = std::env::var("USERNAME").or_else(|_| std::env::var("USER")) {
        if name.len() > 2 {
            text = text.replace(&format!("\\{name}\\"), "\\%USERNAME%\\").replace(&format!("/{name}/"), "/%USERNAME%/");
        }
    }
    text.chars().map(|letter| if letter.is_control() && letter != '\n' && letter != '\t' { ' ' } else { letter }).collect()
}

pub fn make(kind: &str, message: &str, detail: &str, at: u64) -> PendingCrash {
    PendingCrash {
        kind: if kind == "ui" { "ui".into() } else { "panic".into() },
        message: cut(scrub(message).replace(['\n', '\t'], " ").trim(), MESSAGE_MAX),
        detail: cut(&scrub(detail), DETAIL_MAX),
        version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        at,
    }
}

/// Keeps a report for the next start. The first one waiting wins: it's usually the cause.
pub fn keep(report: &PendingCrash) {
    let Some(path) = file() else { return };
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(report) {
        let _ = fs::write(path, text);
    }
}

pub fn pending() -> Option<PendingCrash> {
    let text = fs::read_to_string(file()?).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn forget() {
    if let Some(path) = file() {
        let _ = fs::remove_file(path);
    }
}

/// Sends the waiting report to the account server, then forgets it.
pub async fn send(server: &str) -> Result<(), String> {
    let report = pending().ok_or("There is no crash report to send.")?;
    let body = serde_json::json!({
        "version": report.version,
        "os": report.os,
        "kind": report.kind,
        "message": if report.message.is_empty() { "(no message)".to_string() } else { report.message.clone() },
        "detail": report.detail,
    });
    let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent("Monolab").build().map_err(|error| error.to_string())?;
    let answer = client.post(format!("{server}/v2/crash")).json(&body).send().await.map_err(|error| format!("Could not send the report: {error}"))?;
    if answer.status().as_u16() == 429 {
        return Err("Too many reports from this network right now. Try again later.".into());
    }
    answer.error_for_status().map_err(|error| format!("The server didn't take the report: {error}"))?;
    forget();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_fit_what_the_server_takes() {
        let report = make("panic", &"m".repeat(5_000), &"d".repeat(50_000), 1);
        assert_eq!(report.message.chars().count(), MESSAGE_MAX);
        assert_eq!(report.detail.chars().count(), DETAIL_MAX);
        assert_eq!(make("weird", "x", "", 1).kind, "panic");
        assert_eq!(make("ui", "x", "", 1).kind, "ui");
    }

    #[test]
    fn messages_are_one_line_without_control_letters() {
        let report = make("ui", "a\nb\u{7}c", "line\nnext\u{1b}", 1);
        assert_eq!(report.message, "a b c");
        assert_eq!(report.detail, "line\nnext ");
    }
}

//! Discord webhook alerts (TODO 1-1 I): a crash, lag, players joining or leaving, a task's alert, sent to a
//! Discord channel. The webhook address is a secret (anyone who has it can post there), so it lives sealed in
//! `discord-webhook.json` like other passwords and the screen only ever sees whether one is set.

use crate::data_root::MonolabDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// What can be sent: the app's own alert kinds ("Crash", "Task", "Server" from `announce`) plus lag and players.
pub const EVENTS: [&str; 6] = ["crash", "lag", "join", "leave", "task", "server"];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    url: String,
    events: Vec<String>,
    /// Server ids to send for; empty = every server.
    #[serde(default)]
    servers: Vec<String>,
}

/// What the screen sees.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WebhookInfo {
    pub set: bool,
    /// The channel's webhook id (the number in the address), so the user can tell which one it is.
    pub hint: String,
    pub events: Vec<String>,
    pub servers: Vec<String>,
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir.join("discord-webhook.json"))
}

fn load(app: &AppHandle) -> Saved {
    path(app).ok().and_then(|path| crate::store::read_secret(&path).ok().flatten()).unwrap_or_default()
}

/// Only Discord's own webhook addresses, so the alerts can't be pointed anywhere else.
pub fn valid_url(url: &str) -> bool {
    let Some(rest) = [
        "https://discord.com/api/webhooks/",
        "https://discordapp.com/api/webhooks/",
        "https://canary.discord.com/api/webhooks/",
        "https://ptb.discord.com/api/webhooks/",
    ]
    .iter()
    .find_map(|start| url.strip_prefix(start)) else {
        return false;
    };
    let mut parts = rest.split('/');
    let id = parts.next().unwrap_or_default();
    let token = parts.next().unwrap_or_default();
    !id.is_empty()
        && id.bytes().all(|one| one.is_ascii_digit())
        && !token.is_empty()
        && token.bytes().all(|one| one.is_ascii_alphanumeric() || one == b'-' || one == b'_')
        && parts.next().is_none()
}

fn hint_of(url: &str) -> String {
    url.split("/api/webhooks/").nth(1).and_then(|rest| rest.split('/').next()).unwrap_or_default().to_string()
}

pub fn info(app: &AppHandle) -> WebhookInfo {
    let saved = load(app);
    WebhookInfo { set: !saved.url.is_empty(), hint: hint_of(&saved.url), events: saved.events, servers: saved.servers }
}

/// `url`: `None` keeps the saved one, `Some("")` forgets it.
pub fn save(app: &AppHandle, url: Option<String>, events: Vec<String>, servers: Vec<String>) -> Result<WebhookInfo, String> {
    let mut saved = load(app);
    if let Some(url) = url {
        let url = url.trim().to_string();
        if !url.is_empty() && !valid_url(&url) {
            return Err(
                "That isn't a Discord webhook address. In Discord: channel settings > Integrations > Webhooks > Copy Webhook URL.".into()
            );
        }
        saved.url = url;
    }
    saved.events = events.into_iter().filter(|one| EVENTS.contains(&one.as_str())).collect();
    saved.servers = servers;
    let text = serde_json::to_string_pretty(&saved).map_err(|error| error.to_string())?;
    crate::store::write_secret(&path(app)?, &text).map_err(|error| format!("Could not save the webhook: {error}"))?;
    Ok(info(app))
}

/// One message, as Discord takes it. `allowed_mentions` is empty so a player called "@everyone" pings nobody.
pub fn body(server: &str, event: &str, message: &str) -> serde_json::Value {
    let title = match event {
        "crash" => "Crash",
        "lag" => "Lag",
        "join" => "Joined",
        "leave" => "Left",
        "task" => "Task",
        _ => "Server",
    };
    let line = format!("**{}** · {title}\n{}", clean(server), clean(message));
    serde_json::json!({ "username": "Monolab", "content": line.chars().take(1900).collect::<String>(), "allowed_mentions": { "parse": [] } })
}

/// Server and player names are typed by people: keep them from turning into Discord markup or links.
fn clean(text: &str) -> String {
    // A zero-width space after the colon keeps Discord from making a link.
    let mut out = String::new();
    for one in text.chars() {
        if matches!(one, '\\' | '*' | '_' | '`' | '~' | '|' | '>' | '#' | '[' | ']') {
            out.push('\\');
        }
        out.push(one);
    }
    out.replace("://", ":\u{200b}//")
}

fn post(url: &str, body: &serde_json::Value) -> Result<(), String> {
    let client = crate::net::builder()
        .user_agent(crate::net::USER_AGENT)
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| error.to_string())?;
    let answer = client.post(url).json(body).send().map_err(|error| format!("Could not reach Discord: {error}"))?;
    if answer.status().is_success() {
        Ok(())
    } else if answer.status().as_u16() == 404 {
        Err("Discord doesn't know this webhook any more. It may have been deleted; paste a new address.".into())
    } else {
        Err(format!("Discord refused the message ({}).", answer.status()))
    }
}

/// Sends a test message now, and says what went wrong.
pub fn test(app: &AppHandle) -> Result<(), String> {
    let saved = load(app);
    if saved.url.is_empty() {
        return Err("Paste a webhook address first.".into());
    }
    post(&saved.url, &body("Monolab", "server", "Alerts from Monolab will show up here."))
}

/// Sends an alert in the background when the user chose this event (and this server). At most 20 a minute, so a
/// crash loop or a busy server can't flood the channel (Discord would block the webhook).
pub fn send(app: &AppHandle, server_id: &str, server: &str, event: &str, message: &str) {
    let saved = load(app);
    if saved.url.is_empty()
        || !saved.events.iter().any(|one| one == event)
        || (!saved.servers.is_empty() && !saved.servers.iter().any(|one| one == server_id))
    {
        return;
    }
    static SENT: Mutex<Vec<u64>> = Mutex::new(Vec::new());
    let now = crate::unix_now();
    {
        let mut sent = SENT.lock().expect("webhook state poisoned");
        sent.retain(|at| now.saturating_sub(*at) < 60);
        if sent.len() >= 20 {
            return;
        }
        sent.push(now);
    }
    let payload = body(server, event, message);
    std::thread::spawn(move || {
        if let Err(error) = post(&saved.url, &payload) {
            eprintln!("[webhook] {error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_discord_webhooks() {
        assert!(valid_url("https://discord.com/api/webhooks/123456/abc-DEF_9"));
        assert!(valid_url("https://ptb.discord.com/api/webhooks/1/x"));
        assert!(!valid_url("http://discord.com/api/webhooks/1/x"));
        assert!(!valid_url("https://discord.com.evil.example/api/webhooks/1/x"));
        assert!(!valid_url("https://discord.com/api/webhooks/abc/x"));
        assert!(!valid_url("https://discord.com/api/webhooks/1/x/extra"));
        assert!(!valid_url("https://discord.com/api/webhooks/1/"));
        assert_eq!(hint_of("https://discord.com/api/webhooks/123/abc"), "123");
    }

    #[test]
    fn messages_ping_nobody_and_show_no_markup() {
        let sent = body("My **server**", "join", "@everyone joined from https://x.example");
        let text = sent["content"].as_str().unwrap();
        assert!(text.starts_with(r"**My \*\*server\*\*** · Joined"), "{text}");
        assert!(body("a", "join", "Steve_1")["content"].as_str().unwrap().ends_with(r"Steve\_1"));
        assert!(!text.contains("https://"), "{text}");
        assert_eq!(sent["allowed_mentions"]["parse"].as_array().unwrap().len(), 0);
        assert!(body("a", "crash", &"x".repeat(5000))["content"].as_str().unwrap().chars().count() <= 1900);
    }
}

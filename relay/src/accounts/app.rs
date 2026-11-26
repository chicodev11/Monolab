//! `/v2/app`: one WebSocket per running Monolab app, signed in or not, for update notices and
//! anonymous usage counts. Section 3 of `docs/updates-and-stats.md`.
//!
//! app → server: `{"type":"hello","app":"1.0.2","addon":41,"fork":0,"os":"windows-x86_64","install":"<32 hex>"|null}`
//! first, then `{"type":"ping"}` every 60 seconds.
//! server → app: `{"type":"updates","addon":N,"fork":M}` right after the hello and whenever the release
//! list changes, `pong`, `error {code}`.
//!
//! IP addresses are only held in memory while a connection is open (for the per-address caps) and in
//! the rate limiter's short windows; they are never written anywhere. `install` is null when the person
//! turned usage counts off: then only the in-memory connection count goes up.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::Shared;
use super::store::InstallSeen;
use super::util::now;

/// Where the signed release list lives. The server only reads the two numbers; apps check the signature.
pub const UPDATES_URL: &str = "https://github.com/chicodev11/Monolab/releases/download/updates/updates.json";
/// How often the release list is read.
pub const UPDATES_EVERY: Duration = Duration::from_secs(5 * 60);

/// Open app connections in all, and from one address (a household or a school shares one).
const MOST: usize = 50_000;
const PER_IP: u32 = 16;
/// New connections one address may open per minute.
pub const CONNECTS_PER_MINUTE: u32 = 20;
const HELLO_WITHIN: Duration = Duration::from_secs(10);
/// Apps ping every 60 seconds; two missed pings and a bit close the connection.
const IDLE: Duration = Duration::from_secs(150);
/// Messages per minute an app may send after its hello.
const PER_MINUTE: u32 = 10;
const QUEUE: usize = 16;
/// Longest version or OS text accepted in a hello.
pub const TEXT_MAX: usize = 32;
/// Biggest add-on or fork number accepted (they're small counters).
const NUMBER_MAX: u64 = 1_000_000_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Hello {
    pub app: String,
    pub addon: u64,
    pub fork: u64,
    pub os: String,
    /// The random install id, or None when usage counts are off.
    pub install: Option<String>,
}

fn short_text(value: &Value) -> Option<String> {
    let text = value.as_str()?.trim();
    let ok = !text.is_empty()
        && text.len() <= TEXT_MAX
        && text.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'));
    ok.then(|| text.to_string())
}

fn small_number(value: &Value) -> Option<u64> {
    match value {
        Value::Null => Some(0),
        other => other.as_u64().filter(|number| *number <= NUMBER_MAX),
    }
}

/// An install id: exactly 32 lowercase hex digits.
pub fn valid_install(text: &str) -> bool {
    text.len() == 32 && text.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Reads and checks the first message. `install` must be 32 lowercase hex digits or null; the versions and
/// OS are short plain texts; add-on and fork are whole numbers (missing counts as 0).
pub fn parse_hello(text: &str) -> Result<Hello, &'static str> {
    let value: Value = serde_json::from_str(text).map_err(|_| "bad_hello")?;
    if value["type"] != "hello" {
        return Err("hello_first");
    }
    let app = short_text(&value["app"]).ok_or("bad_hello")?;
    let os = short_text(&value["os"]).ok_or("bad_hello")?;
    let addon = small_number(&value["addon"]).ok_or("bad_hello")?;
    let fork = small_number(&value["fork"]).ok_or("bad_hello")?;
    let install = match &value["install"] {
        Value::Null => None,
        Value::String(id) if valid_install(id) => Some(id.clone()),
        _ => return Err("bad_hello"),
    };
    Ok(Hello { app, addon, fork, os, install })
}

/// The add-on version and fork build in the release list (`updates.json`). A number that's missing or
/// isn't a whole number keeps the one from before (0 at first); a list that doesn't read keeps both.
pub fn manifest_numbers(text: &str, before: (u64, u64)) -> (u64, u64) {
    let Ok(value) = serde_json::from_str::<Value>(text) else { return before };
    let addon = value["addon"]["version"].as_u64().unwrap_or(before.0);
    let fork = value["fork"]["build"].as_u64().unwrap_or(before.1);
    (addon, fork)
}

pub fn updates_message(numbers: (u64, u64)) -> String {
    json!({ "type": "updates", "addon": numbers.0, "fork": numbers.1 }).to_string()
}

/// Open app connections, kept in memory only.
#[derive(Default)]
pub struct AppHub {
    connections: Mutex<HashMap<u64, mpsc::Sender<String>>>,
    per_ip: Mutex<HashMap<String, u32>>,
    open: AtomicU64,
    anonymous: AtomicU64,
    next: AtomicU64,
    /// The last add-on version and fork build read from the release list.
    updates: Mutex<(u64, u64)>,
}

/// A place for one connection from one address. Gives the place back when dropped.
pub(crate) struct Slot {
    hub: Arc<Shared>,
    ip: String,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let hub = &self.hub.apps;
        hub.open.fetch_sub(1, Ordering::Relaxed);
        let mut per_ip = hub.per_ip.lock().expect("app hub poisoned");
        if let Some(count) = per_ip.get_mut(&self.ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                per_ip.remove(&self.ip);
            }
        }
    }
}

impl AppHub {
    /// Takes a place for a new connection, or None when this address or the whole server is full.
    pub(crate) fn reserve(shared: &Arc<Shared>, ip: &str) -> Option<Slot> {
        let hub = &shared.apps;
        let mut per_ip = hub.per_ip.lock().expect("app hub poisoned");
        if hub.open.load(Ordering::Relaxed) as usize >= MOST {
            return None;
        }
        let count = per_ip.entry(ip.to_string()).or_insert(0);
        if *count >= PER_IP {
            return None;
        }
        *count += 1;
        hub.open.fetch_add(1, Ordering::Relaxed);
        Some(Slot { hub: shared.clone(), ip: ip.to_string() })
    }

    /// Apps connected now that said hello, and how many of them send no usage counts.
    pub fn connected(&self) -> (u64, u64) {
        (self.connections.lock().expect("app hub poisoned").len() as u64, self.anonymous.load(Ordering::Relaxed))
    }

    pub fn updates(&self) -> (u64, u64) {
        *self.updates.lock().expect("app hub poisoned")
    }

    /// Takes the numbers from a freshly read release list; tells every app when either changed.
    pub fn set_updates(&self, numbers: (u64, u64)) -> bool {
        {
            let mut current = self.updates.lock().expect("app hub poisoned");
            if *current == numbers {
                return false;
            }
            *current = numbers;
        }
        let text = updates_message(numbers);
        for outbox in self.connections.lock().expect("app hub poisoned").values() {
            // A full queue is a stuck app; it gets the numbers again when it reconnects.
            let _ = outbox.try_send(text.clone());
        }
        true
    }

    fn join(&self, outbox: mpsc::Sender<String>) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.connections.lock().expect("app hub poisoned").insert(id, outbox);
        id
    }

    fn leave(&self, id: u64) {
        self.connections.lock().expect("app hub poisoned").remove(&id);
    }
}

async fn send(socket: &mut WebSocket, text: String) -> bool {
    socket.send(Message::Text(text.into())).await.is_ok()
}

async fn refuse(socket: &mut WebSocket, code: &str) {
    let _ = send(socket, json!({ "type": "error", "code": code }).to_string()).await;
    let _ = socket.send(Message::Close(None)).await;
}

pub(crate) async fn run(shared: Arc<Shared>, mut socket: WebSocket, slot: Slot) {
    let first = tokio::time::timeout(HELLO_WITHIN, socket.recv()).await;
    let hello = match first {
        Ok(Some(Ok(Message::Text(text)))) => parse_hello(&text),
        _ => Err("hello_first"),
    };
    let hello = match hello {
        Ok(hello) => hello,
        Err(code) => return refuse(&mut socket, code).await,
    };
    match &hello.install {
        Some(id) => {
            let seen = InstallSeen { id: id.clone(), app: hello.app.clone(), addon: hello.addon, fork: hello.fork, os: hello.os.clone() };
            if let Err(error) = shared.store.install_seen(&seen, now()) {
                eprintln!("[accounts] usage counts: {error}");
            }
        }
        None => {
            shared.apps.anonymous.fetch_add(1, Ordering::Relaxed);
        }
    }

    let (outbox, mut inbox) = mpsc::channel::<String>(QUEUE);
    let id = shared.apps.join(outbox);
    let mut alive = send(&mut socket, updates_message(shared.apps.updates())).await;
    let mut window = tokio::time::Instant::now();
    let mut count = 0u32;
    while alive {
        tokio::select! {
            incoming = tokio::time::timeout(IDLE, socket.recv()) => {
                let Ok(Some(Ok(message))) = incoming else { break };
                let text = match message {
                    Message::Text(text) => text,
                    Message::Ping(_) | Message::Pong(_) => continue,
                    _ => break,
                };
                if window.elapsed() > Duration::from_secs(60) {
                    window = tokio::time::Instant::now();
                    count = 0;
                }
                count += 1;
                if count > PER_MINUTE {
                    let _ = send(&mut socket, json!({ "type": "error", "code": "slow_down" }).to_string()).await;
                    break;
                }
                let kind = serde_json::from_str::<Value>(&text).ok().and_then(|value| value["type"].as_str().map(str::to_string));
                if kind.as_deref() == Some("ping") {
                    alive = send(&mut socket, json!({ "type": "pong" }).to_string()).await;
                }
            }
            outgoing = inbox.recv() => {
                let Some(text) = outgoing else { break };
                alive = send(&mut socket, text).await;
            }
        }
    }
    let _ = socket.send(Message::Close(None)).await;
    shared.apps.leave(id);
    if hello.install.is_none() {
        shared.apps.anonymous.fetch_sub(1, Ordering::Relaxed);
    }
    drop(slot);
}

/// Reads the release list now and then (and right away when told), and tells apps when it changed.
pub(crate) async fn watch_updates(shared: Arc<Shared>, url: String) {
    loop {
        match fetch(&shared.http, &url).await {
            Ok(text) => {
                let numbers = manifest_numbers(&text, shared.apps.updates());
                if shared.apps.set_updates(numbers) {
                    println!("[accounts] release list: add-on {} fork {}", numbers.0, numbers.1);
                }
            }
            Err(error) => eprintln!("[accounts] release list: {error}"),
        }
        tokio::select! {
            _ = tokio::time::sleep(UPDATES_EVERY) => {}
            _ = shared.updates_now.notified() => {}
        }
    }
}

async fn fetch(http: &reqwest::Client, url: &str) -> Result<String, String> {
    let response = http
        .get(url)
        .header(reqwest::header::USER_AGENT, "monolab-relay")
        .send()
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    if response.content_length().is_some_and(|size| size > 1024 * 1024) {
        return Err("too big".into());
    }
    let text = response.text().await.map_err(|error| error.to_string())?;
    if text.len() > 1024 * 1024 {
        return Err("too big".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_is_checked() {
        let id = "0123456789abcdef0123456789abcdef";
        let ok = parse_hello(&format!(r#"{{"type":"hello","app":"1.0.2","addon":41,"fork":0,"os":"windows-x86_64","install":"{id}"}}"#))
            .unwrap();
        assert_eq!(ok, Hello { app: "1.0.2".into(), addon: 41, fork: 0, os: "windows-x86_64".into(), install: Some(id.into()) });
        let anonymous = parse_hello(r#"{"type":"hello","app":"1.0.2","addon":41,"fork":3,"os":"linux-aarch64","install":null}"#).unwrap();
        assert_eq!(anonymous.install, None);
        // Missing numbers count as 0, a missing install as null.
        let bare = parse_hello(r#"{"type":"hello","app":"1.0.2","os":"macos-aarch64"}"#).unwrap();
        assert_eq!((bare.addon, bare.fork, bare.install), (0, 0, None));

        let hello = |install: &str| {
            parse_hello(&format!(r#"{{"type":"hello","app":"1.0.2","addon":41,"fork":0,"os":"windows-x86_64","install":{install}}}"#))
        };
        assert_eq!(hello(r#""0123456789ABCDEF0123456789ABCDEF""#), Err("bad_hello"));
        assert_eq!(hello(r#""0123456789abcdef0123456789abcde""#), Err("bad_hello"));
        assert_eq!(hello(r#""0123456789abcdef0123456789abcdef0""#), Err("bad_hello"));
        assert_eq!(hello(r#""0123456789abcdef0123456789abcdeg""#), Err("bad_hello"));
        assert_eq!(hello("42"), Err("bad_hello"));
        assert!(valid_install(id) && !valid_install(""));

        let long = "1".repeat(TEXT_MAX + 1);
        assert_eq!(parse_hello(&format!(r#"{{"type":"hello","app":"{long}","os":"windows-x86_64"}}"#)), Err("bad_hello"));
        assert_eq!(parse_hello(&format!(r#"{{"type":"hello","app":"1.0.2","os":"{long}"}}"#)), Err("bad_hello"));
        let longest = "1".repeat(TEXT_MAX);
        assert!(parse_hello(&format!(r#"{{"type":"hello","app":"{longest}","os":"x"}}"#)).is_ok());
        assert_eq!(parse_hello(r#"{"type":"hello","app":"","os":"x"}"#), Err("bad_hello"));
        assert_eq!(parse_hello(r#"{"type":"hello","app":"1.0 <b>","os":"x"}"#), Err("bad_hello"));
        assert_eq!(parse_hello(r#"{"type":"hello","app":"1.0.2","addon":-1,"os":"x"}"#), Err("bad_hello"));
        assert_eq!(parse_hello(r#"{"type":"hello","app":"1.0.2","addon":"41","os":"x"}"#), Err("bad_hello"));
        assert_eq!(parse_hello(r#"{"type":"hello","app":"1.0.2","fork":1.5,"os":"x"}"#), Err("bad_hello"));
        assert_eq!(parse_hello(r#"{"type":"ping"}"#), Err("hello_first"));
        assert_eq!(parse_hello("not json"), Err("bad_hello"));
    }

    #[test]
    fn manifest_numbers_are_read() {
        let full = r#"{"schema":1,"addon":{"version":42,"files":{}},"fork":{"build":3,"files":{}}}"#;
        assert_eq!(manifest_numbers(full, (0, 0)), (42, 3));
        // Missing parts keep what was there before.
        assert_eq!(manifest_numbers(r#"{"schema":1,"addon":{"version":43}}"#, (42, 3)), (43, 3));
        assert_eq!(manifest_numbers(r#"{"schema":1,"fork":{"build":4}}"#, (0, 0)), (0, 4));
        assert_eq!(manifest_numbers(r#"{"addon":{"version":"44"},"fork":{"build":-1}}"#, (42, 3)), (42, 3));
        assert_eq!(manifest_numbers("<html>not found</html>", (42, 3)), (42, 3));
        assert_eq!(serde_json::from_str::<Value>(&updates_message((42, 3))).unwrap(), json!({ "type": "updates", "addon": 42, "fork": 3 }));
    }
}

//! `/v2/ws`: one WebSocket per signed-in app for who is online, team changes, notices and new
//! activity. JSON text messages. The app signs in with its session token as the first message.
//!
//! app → server: `{"type":"auth","token":"…"}` first, then `{"type":"ping"}` every 30 seconds.
//! server → app: `ready`, `presence {user, online}`, `team_changed {team, version}`,
//! `notice {kind, data}`, `activity {team, item}`, `pong`, `error {code}`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::util::{now, sha256};
use super::Shared;

/// Connections one person may hold at once (a few devices, plus reconnects).
const PER_USER: usize = 12;
const IDLE: Duration = Duration::from_secs(90);
const AUTH_WITHIN: Duration = Duration::from_secs(10);
const QUEUE: usize = 256;
/// Messages per minute an app may send (pings mostly).
const PER_MINUTE: u32 = 30;

struct Connection {
    id: u64,
    device: String,
    outbox: mpsc::Sender<String>,
}

#[derive(Default)]
pub struct Hub {
    online: Mutex<HashMap<String, Vec<Connection>>>,
    next: AtomicU64,
}

impl Hub {
    pub fn send(&self, people: &[String], message: &Value) {
        let text = message.to_string();
        let online = self.online.lock().expect("hub poisoned");
        let mut seen = std::collections::HashSet::new();
        for person in people {
            if !seen.insert(person) {
                continue;
            }
            for connection in online.get(person).into_iter().flatten() {
                // A full queue means a stuck app; it misses this one and catches up by reading again.
                let _ = connection.outbox.try_send(text.clone());
            }
        }
    }

    pub fn is_online(&self, person: &str) -> bool {
        self.online.lock().expect("hub poisoned").contains_key(person)
    }

    /// Closes every connection of a device (signed out or taken away). Gives the people who are now offline.
    pub fn kick_device(&self, device: &str) -> Vec<String> {
        let mut online = self.online.lock().expect("hub poisoned");
        let mut gone = Vec::new();
        for (person, connections) in online.iter_mut() {
            let before = connections.len();
            connections.retain(|one| one.device != device);
            if before > 0 && connections.is_empty() {
                gone.push(person.clone());
            }
        }
        online.retain(|_, connections| !connections.is_empty());
        gone
    }

    /// Adds a connection. Gives its id and whether it's the person's first, or None when they have too many.
    fn join(&self, person: &str, device: &str, outbox: mpsc::Sender<String>) -> Option<(u64, bool)> {
        let mut online = self.online.lock().expect("hub poisoned");
        let connections = online.entry(person.to_string()).or_default();
        if connections.len() >= PER_USER {
            return None;
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        connections.push(Connection { id, device: device.to_string(), outbox });
        Some((id, connections.len() == 1))
    }

    /// Removes a connection. Gives true when that was the person's last.
    fn leave(&self, person: &str, id: u64) -> bool {
        let mut online = self.online.lock().expect("hub poisoned");
        let Some(connections) = online.get_mut(person) else { return false };
        let before = connections.len();
        connections.retain(|one| one.id != id);
        if connections.len() == before {
            return false;
        }
        if connections.is_empty() {
            online.remove(person);
            return true;
        }
        false
    }
}

pub async fn run(shared: Arc<Shared>, mut socket: WebSocket) {
    let first = tokio::time::timeout(AUTH_WITHIN, socket.recv()).await;
    let token = match first {
        Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str::<Value>(&text)
            .ok()
            .filter(|value| value["type"] == "auth")
            .and_then(|value| value["token"].as_str().map(str::to_string)),
        _ => None,
    };
    let Some(token) = token else {
        let _ = socket.send(Message::Text(json!({ "type": "error", "code": "sign_in" }).to_string().into())).await;
        return;
    };
    let Ok(Some((user, device))) = shared.store.session(&sha256(&token), now()) else {
        let _ = socket.send(Message::Text(json!({ "type": "error", "code": "signed_out" }).to_string().into())).await;
        return;
    };

    let (outbox, mut inbox) = mpsc::channel::<String>(QUEUE);
    let Some((id, first)) = shared.hub.join(&user.id, &device, outbox) else {
        let _ = socket.send(Message::Text(json!({ "type": "error", "code": "too_many" }).to_string().into())).await;
        return;
    };

    let teammates = shared.store.teammates(&user.id).unwrap_or_default();
    let mut greeting = vec![json!({ "type": "ready", "user": { "id": user.id, "handle": user.handle } })];
    for other in teammates.iter().filter(|one| **one != user.id) {
        if shared.hub.is_online(other) {
            greeting.push(json!({ "type": "presence", "user": other, "online": true }));
        }
    }
    let mut alive = true;
    for message in greeting {
        if socket.send(Message::Text(message.to_string().into())).await.is_err() {
            alive = false;
            break;
        }
    }
    if !alive {
        if shared.hub.leave(&user.id, id) {
            shared.hub.send(&teammates, &json!({ "type": "presence", "user": user.id, "online": false }));
        }
        return;
    }
    if first {
        shared.hub.send(&teammates, &json!({ "type": "presence", "user": user.id, "online": true }));
    }

    let mut window = tokio::time::Instant::now();
    let mut count = 0u32;
    loop {
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
                    let _ = socket.send(Message::Text(json!({ "type": "error", "code": "slow_down" }).to_string().into())).await;
                    break;
                }
                let kind = serde_json::from_str::<Value>(&text).ok().and_then(|value| value["type"].as_str().map(str::to_string));
                if kind.as_deref() == Some("ping")
                    && socket.send(Message::Text(json!({ "type": "pong" }).to_string().into())).await.is_err()
                {
                    break;
                }
            }
            outgoing = inbox.recv() => {
                // None: the hub dropped this connection (signed out, device taken away).
                let Some(text) = outgoing else { break };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    }
    let _ = socket.send(Message::Close(None)).await;
    if shared.hub.leave(&user.id, id) {
        let teammates = shared.store.teammates(&user.id).unwrap_or_default();
        shared.hub.send(&teammates, &json!({ "type": "presence", "user": user.id, "online": false }));
    }
}

//! What a teammate's window hears without asking again and again (`__events`, the app's `wait_for_events`):
//! console lines as they are written (`log:batch`) and servers starting and stopping (`server-status`).
//! A watcher thread reads the end of each server's log and asks tmux which sessions are there once a
//! second; the window long-polls with the `at` of the last answer.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::config::ServerConfig;
use crate::console::{ServerLogEvent, Tmux};

pub const LOG: &str = "log:batch";
pub const STATUS: &str = "server-status";
/// Events kept for windows that ask late.
const KEEP: usize = 2000;
/// Most of a log read in one go (a server that prints faster than this loses the middle, not the end).
const READ_MOST: u64 = 256 * 1024;

#[derive(Default)]
struct Queue {
    items: Vec<(u64, String, Value)>,
    last: u64,
}

/// The events of this node, newest last. `at` numbers start from the time the node started (ms), so a
/// window that knew an earlier run of the node still gets the new events.
pub struct Events {
    queue: Mutex<Queue>,
    woke: Condvar,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_millis() as u64).unwrap_or(0)
}

impl Default for Events {
    fn default() -> Self {
        Events { queue: Mutex::new(Queue { items: Vec::new(), last: now_ms() }), woke: Condvar::new() }
    }
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Whether an event's payload is about one of `servers`; an array payload keeps only those items.
fn keep_for(payload: &Value, servers: &[String]) -> Option<Value> {
    let ours = |item: &Value| item.get("serverId").and_then(Value::as_str).is_some_and(|id| servers.iter().any(|mine| mine == id));
    match payload {
        Value::Array(items) => {
            let kept: Vec<Value> = items.iter().filter(|one| ours(one)).cloned().collect();
            (!kept.is_empty()).then_some(Value::Array(kept))
        }
        other => ours(other).then(|| other.clone()),
    }
}

impl Events {
    pub fn push(&self, event: &str, payload: Value) {
        let mut queue = locked(&self.queue);
        queue.last += 1;
        let at = queue.last;
        queue.items.push((at, event.to_string(), payload));
        let extra = queue.items.len().saturating_sub(KEEP);
        queue.items.drain(..extra);
        drop(queue);
        self.woke.notify_all();
    }

    pub fn status(&self, server: &str, status: &str) {
        self.push(STATUS, json!({ "serverId": server, "status": status }));
    }

    /// The events after `after` about `servers`, waiting up to `most` for one: `{events:[{event,payload}], at}`.
    pub fn wait(&self, after: u64, servers: &[String], most: Duration) -> Value {
        let until = Instant::now() + most;
        let mut queue = locked(&self.queue);
        loop {
            let found: Vec<Value> = queue
                .items
                .iter()
                .filter(|(at, _, _)| *at > after)
                .filter_map(|(_, event, payload)| keep_for(payload, servers).map(|payload| json!({ "event": event, "payload": payload })))
                .collect();
            let newest = queue.last;
            // Something new for this window, or the wait is over (then `at` moves on past events about other servers).
            if !found.is_empty() || Instant::now() >= until {
                return json!({ "events": found, "at": newest });
            }
            // Events about other servers move `at` on without waking this window.
            let left = until.saturating_duration_since(Instant::now());
            queue = self.woke.wait_timeout(queue, left).map(|(guard, _)| guard).unwrap_or_else(|poison| poison.into_inner().0);
        }
    }
}

/// Where the watcher is in one server's log.
struct Seen {
    offset: u64,
    /// The start of a line that wasn't finished yet.
    partial: String,
    running: bool,
}

/// Reads what the log gained since `seen.offset`, as whole lines. The first look only notes where the
/// log ends (the window gets the earlier lines with `get_console_logs`).
fn new_lines(server: &ServerConfig, seen: &mut Option<Seen>, running: bool) -> Vec<String> {
    let Ok(path) = crate::files::resolve_in_server(&server.path, &server.log) else {
        if let Some(seen) = seen.as_mut() {
            seen.offset = 0;
            seen.partial.clear();
        }
        return Vec::new();
    };
    let size = std::fs::metadata(&path).map(|one| one.len()).unwrap_or(0);
    let Some(state) = seen.as_mut() else {
        *seen = Some(Seen { offset: size, partial: String::new(), running });
        return Vec::new();
    };
    if size < state.offset {
        // A new log (the server started again and rolled the old one away).
        state.offset = 0;
        state.partial.clear();
    }
    if size == state.offset {
        return Vec::new();
    }
    let from = state.offset.max(size.saturating_sub(READ_MOST));
    let mut bytes = Vec::new();
    let read = std::fs::File::open(&path).and_then(|mut file| {
        file.seek(SeekFrom::Start(from))?;
        file.take(size - from).read_to_end(&mut bytes)
    });
    if read.is_err() {
        return Vec::new();
    }
    state.offset = from + bytes.len() as u64;
    let mut text = std::mem::take(&mut state.partial);
    text.push_str(&String::from_utf8_lossy(&bytes));
    let mut lines: Vec<String> = text.split('\n').map(|one| one.trim_end_matches('\r').to_string()).collect();
    state.partial = lines.pop().unwrap_or_default();
    lines
}

/// The watcher: once a second, new console lines and sessions that came or went. `stopping` holds the
/// servers the node is stopping (their status stays `stopping` until the session is gone).
pub fn watch(servers: Vec<ServerConfig>, tmux: Tmux, events: Arc<Events>) {
    std::thread::spawn(move || {
        let mut seen: HashMap<String, Option<Seen>> = HashMap::new();
        loop {
            for server in &servers {
                let running = server.path.is_dir() && tmux.running(server.session());
                let state = seen.entry(server.id.clone()).or_insert(None);
                let was = state.as_ref().map(|one| one.running);
                let lines = new_lines(server, state, running);
                if !lines.is_empty() {
                    let batch: Vec<ServerLogEvent> = lines.into_iter().map(|line| crate::console::log_event(&server.id, &line)).collect();
                    events.push(LOG, serde_json::to_value(batch).unwrap_or_default());
                }
                if let Some(state) = state.as_mut() {
                    state.running = running;
                }
                if was.is_some_and(|was| was != running) {
                    events.status(&server.id, if running { "running" } else { "stopped" });
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_gets_only_its_servers() {
        let events = Events::default();
        events.status("a", "running");
        events.status("b", "running");
        events.push(LOG, json!([{ "serverId": "a", "line": "one" }, { "serverId": "b", "line": "two" }]));
        let mine = ["a".to_string()];
        let first = events.wait(0, &mine, Duration::from_millis(10));
        let list = first["events"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1]["payload"], json!([{ "serverId": "a", "line": "one" }]));
        // Nothing new: waits, then moves on.
        let at = first["at"].as_u64().unwrap();
        let began = Instant::now();
        let next = events.wait(at, &mine, Duration::from_millis(120));
        assert!(next["events"].as_array().unwrap().is_empty());
        assert!(began.elapsed() >= Duration::from_millis(100));
        // Only about another server: still nothing, but `at` moves past it.
        events.status("b", "stopped");
        let other = events.wait(at, &mine, Duration::from_millis(20));
        assert!(other["events"].as_array().unwrap().is_empty());
        assert!(other["at"].as_u64().unwrap() > at);
    }

    #[test]
    fn a_waiting_window_wakes_up() {
        let events = Arc::new(Events::default());
        let at = events.wait(0, &[], Duration::ZERO)["at"].as_u64().unwrap();
        let later = events.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            later.status("a", "stopped");
        });
        let began = Instant::now();
        let woke = events.wait(at, &["a".to_string()], Duration::from_secs(5));
        assert_eq!(woke["events"][0]["payload"]["status"], "stopped");
        assert!(began.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn the_log_is_read_as_it_grows() {
        let root = crate::files::tests::sandbox("events-log");
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::write(root.join("logs/latest.log"), "old line\n").unwrap();
        let server = ServerConfig { id: "s".into(), name: "S".into(), path: root.clone(), tmux: String::new(), log: "logs/latest.log".into(), start: String::new(), stop: "stop".into() };
        let mut seen = None;
        assert!(new_lines(&server, &mut seen, false).is_empty(), "the first look only notes the end");
        let append = |text: &str| {
            use std::io::Write;
            std::fs::OpenOptions::new().append(true).open(root.join("logs/latest.log")).unwrap().write_all(text.as_bytes()).unwrap();
        };
        append("new one\nhalf");
        assert_eq!(new_lines(&server, &mut seen, false), ["new one"]);
        append(" done\r\n");
        assert_eq!(new_lines(&server, &mut seen, false), ["half done"]);
        // Rolled over: a shorter file starts again from its beginning.
        std::fs::write(root.join("logs/latest.log"), "fresh\n").unwrap();
        assert_eq!(new_lines(&server, &mut seen, false), ["fresh"]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}

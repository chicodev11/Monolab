//! Progress and cancelling for long commands. A command opens a [`Task`] under its own name and the
//! server it works on; the app's job card for that call shows the steps and can cancel it.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// The error a cancelled command returns; the app shows no error for it.
pub const CANCELLED: &str = "Cancelled.";

const EVERY: Duration = Duration::from_millis(120);

fn table() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static TABLE: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

fn key(kind: &str, server: &str) -> String {
    format!("{kind}\n{server}")
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Step<'a> {
    kind: &'a str,
    server_id: &'a str,
    done: u64,
    total: u64,
    /// "bytes" or "items".
    unit: &'a str,
    note: &'a str,
}

/// Sends one step to the app. Kept behind a box so code that never follows a task (tests) doesn't link the app.
type Say = Box<dyn Fn(&Step) + Send + Sync>;

pub struct Task {
    say: Option<Say>,
    kind: &'static str,
    server: String,
    stop: Arc<AtomicBool>,
    said: Mutex<Option<Instant>>,
}

/// Starts a task that the app can follow and cancel. `kind` is the command's name as the app calls it.
pub fn begin(app: &AppHandle, kind: &'static str, server: &str) -> Task {
    let stop = Arc::new(AtomicBool::new(false));
    table().lock().unwrap_or_else(|poison| poison.into_inner()).insert(key(kind, server), stop.clone());
    let app = app.clone();
    let say: Say = Box::new(move |step: &Step| {
        let _ = app.emit("task-progress", step);
    });
    Task { say: Some(say), kind, server: server.to_string(), stop, said: Mutex::new(None) }
}

/// Asks a running task to stop. False when nothing by that name runs.
pub fn cancel(kind: &str, server: &str) -> bool {
    let table = table().lock().unwrap_or_else(|poison| poison.into_inner());
    match table.get(&key(kind, server)) {
        Some(flag) => {
            flag.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

impl Task {
    /// A task nobody follows (tests, callers without an app).
    pub fn quiet() -> Task {
        Task { say: None, kind: "", server: String::new(), stop: Arc::new(AtomicBool::new(false)), said: Mutex::new(None) }
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// The flag worker threads can check.
    pub fn flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }

    pub fn check(&self) -> Result<(), String> {
        if self.stopped() {
            Err(CANCELLED.into())
        } else {
            Ok(())
        }
    }

    /// Reports how far along it is, at most every 120 ms (the last step always goes out).
    pub fn step(&self, done: u64, total: u64, unit: &str, note: &str) {
        let Some(say) = &self.say else { return };
        let mut said = self.said.lock().unwrap_or_else(|poison| poison.into_inner());
        let now = Instant::now();
        if done < total && said.is_some_and(|then| now.duration_since(then) < EVERY) {
            return;
        }
        *said = Some(now);
        say(&Step { kind: self.kind, server_id: &self.server, done, total, unit, note });
    }

    /// Copies everything, counting into `done` and stopping when cancelled.
    pub fn copy(&self, from: &mut impl Read, to: &mut impl Write, done: &mut u64, total: u64, note: &str) -> Result<u64, String> {
        let mut buffer = vec![0_u8; 256 * 1024];
        let mut copied = 0;
        loop {
            self.check()?;
            let read = match from.read(&mut buffer) {
                Ok(0) => return Ok(copied),
                Ok(read) => read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.to_string()),
            };
            to.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
            copied += read as u64;
            *done += read as u64;
            self.step(*done, total, "bytes", note);
        }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        if self.say.is_none() {
            return;
        }
        let mut table = table().lock().unwrap_or_else(|poison| poison.into_inner());
        let name = key(self.kind, &self.server);
        if table.get(&name).is_some_and(|flag| Arc::ptr_eq(flag, &self.stop)) {
            table.remove(&name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_stops_when_cancelled() {
        let task = Task::quiet();
        let mut done = 0;
        let mut out = Vec::new();
        assert_eq!(task.copy(&mut &b"hello"[..], &mut out, &mut done, 5, "").unwrap(), 5);
        assert_eq!(done, 5);
        task.stop.store(true, Ordering::Relaxed);
        assert_eq!(task.copy(&mut &b"again"[..], &mut out, &mut done, 5, ""), Err(CANCELLED.to_string()));
        assert_eq!(out, b"hello");
    }

    #[test]
    fn cancel_without_a_task_says_so() {
        assert!(!cancel("nothing", "here"));
    }
}

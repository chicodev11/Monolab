//! Who has a file open (F2): editors say so every 30 seconds while a file is open (`file_editing` from the host's
//! window, `__editing` from a teammate's), and the host forgets them a minute after the last word. Each change goes
//! out as `file-editing { serverId, path, people }`, to the host's window and, filtered by server, to teammates.
//! Memory only, and not in the teammate log.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// How long an open file counts after the editor last said so.
const OPEN_FOR: Duration = Duration::from_secs(60);

type Key = (String, String);
static OPEN: Mutex<Option<HashMap<Key, HashMap<String, Instant>>>> = Mutex::new(None);

/// One open file and who has it open.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenFile {
    pub path: String,
    pub people: Vec<String>,
}

/// [`people`] for the screens.
pub fn open_files(server: &str) -> Vec<OpenFile> {
    people(server).into_iter().map(|(path, people)| OpenFile { path, people }).collect()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Changed<'a> {
    server_id: &'a str,
    path: &'a str,
    people: Vec<String>,
}

fn clean(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches('/').to_string()
}

/// Updates the table and gives the people on `path` now, or None when nothing changed for it.
fn update(server: &str, path: &str, who: &str, closed: bool, now: Instant) -> Option<Vec<String>> {
    let mut held = OPEN.lock().unwrap_or_else(|poison| poison.into_inner());
    let table = held.get_or_insert_with(HashMap::new);
    table.retain(|_, people| {
        people.retain(|_, at| now.duration_since(*at) < OPEN_FOR);
        !people.is_empty()
    });
    let key = (server.to_string(), clean(path));
    let before = names(table.get(&key));
    if closed {
        if let Some(people) = table.get_mut(&key) {
            people.remove(who);
            if people.is_empty() {
                table.remove(&key);
            }
        }
    } else {
        table.entry(key.clone()).or_default().insert(who.to_string(), now);
    }
    let after = names(table.get(&key));
    (before != after).then_some(after)
}

fn names(people: Option<&HashMap<String, Instant>>) -> Vec<String> {
    let mut out: Vec<String> = people.map(|one| one.keys().cloned().collect()).unwrap_or_default();
    out.sort();
    out
}

/// Someone opened (or still has open) or closed `path` on `server`. `who` is `host` for the host's window,
/// `@handle` for a teammate.
pub fn mark(app: &AppHandle, server: &str, path: &str, who: &str, closed: bool) {
    if server.is_empty() || path.trim().is_empty() || who.is_empty() {
        return;
    }
    let path = clean(path);
    if let Some(people) = update(server, &path, who, closed, Instant::now()) {
        let event = Changed { server_id: server, path: &path, people };
        let _ = app.emit("file-editing", &event);
        crate::remember_for_web(app, "file-editing", &event);
    }
}

/// Everyone with a file of `server` open now, per path (for a window that just opened, and for F4 notices).
pub fn people(server: &str) -> Vec<(String, Vec<String>)> {
    let now = Instant::now();
    let held = OPEN.lock().unwrap_or_else(|poison| poison.into_inner());
    let mut out: Vec<(String, Vec<String>)> = held
        .as_ref()
        .map(|table| {
            table
                .iter()
                .filter(|((one, _), _)| one == server)
                .map(|((_, path), people)| {
                    let mut live: Vec<String> = people.iter().filter(|(_, at)| now.duration_since(**at) < OPEN_FOR).map(|(name, _)| name.clone()).collect();
                    live.sort();
                    (path.clone(), live)
                })
                .filter(|(_, live)| !live.is_empty())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgets_after_a_minute() {
        let start = Instant::now();
        assert_eq!(update("ed-a", "a.yml", "@amy", false, start), Some(vec!["@amy".into()]));
        // Still there: nothing changed.
        assert_eq!(update("ed-a", "a.yml", "@amy", false, start + Duration::from_secs(30)), None);
        // Bob comes after Amy's last word is over a minute old: only Bob is left.
        assert_eq!(update("ed-a", "a.yml", "@bob", false, start + Duration::from_secs(95)), Some(vec!["@bob".into()]));
    }

    #[test]
    fn closing_removes_only_that_person() {
        let now = Instant::now();
        update("ed-b", "a.yml", "@amy", false, now);
        update("ed-b", "a.yml", "host", false, now);
        assert_eq!(update("ed-b", "a.yml", "@amy", true, now), Some(vec!["host".into()]));
    }

    #[test]
    fn lists_people_per_path() {
        let now = Instant::now();
        update("ed-c", "a.yml", "@amy", false, now);
        update("ed-c", "/b.yml", "@bob", false, now);
        update("ed-other", "a.yml", "@cat", false, now);
        assert_eq!(people("ed-c"), vec![("a.yml".to_string(), vec!["@amy".to_string()]), ("b.yml".to_string(), vec!["@bob".to_string()])]);
    }
}

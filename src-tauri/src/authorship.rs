//! Who changed a file (F2): the host remembers, for a few minutes, who last wrote each path of a server, so the
//! Rewind history can say `@amy`, `@amy's AI`, `host` or `AI` next to a change it finds. Memory only; the names
//! go into the Rewind log with the change. Writes reach the server through `monolink_team::serve` (teammates and
//! their agents), the host's own window and the host's own agent (`mcp.rs`), and each notes its writer here.

use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// How long a write counts for the change Rewind finds next (its watcher looks every few seconds).
const RECENT_SECS: u64 = 10 * 60;
/// The most writes remembered.
const MOST: usize = 2000;

struct Note {
    server: String,
    path: String,
    who: String,
    at: u64,
}

static NOTES: Mutex<VecDeque<Note>> = Mutex::new(VecDeque::new());

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|time| time.as_secs()).unwrap_or(0)
}

fn clean(path: &str) -> String {
    path.replace('\\', "/").trim_start_matches("./").trim_start_matches('/').to_string()
}

/// Remembers that `who` just wrote `path` (relative to the server folder) on `server`. The latest note wins: the
/// host's command notes `host`, then the web panel, a teammate or an agent names itself.
pub fn note(server: &str, path: &str, who: &str) {
    note_at(server, path, who, now());
}

fn note_at(server: &str, path: &str, who: &str, at: u64) {
    let path = clean(path);
    if server.is_empty() || path.is_empty() || who.is_empty() {
        return;
    }
    let mut notes = NOTES.lock().unwrap_or_else(|poison| poison.into_inner());
    notes.push_back(Note { server: server.to_string(), path, who: who.to_string(), at });
    while notes.len() > MOST {
        notes.pop_front();
    }
}

/// Who last wrote `path` on `server` up to `at_secs`, if it was within the last ten minutes.
pub fn who_changed(server: &str, path: &str, at_secs: u64) -> Option<String> {
    let path = clean(path);
    let notes = NOTES.lock().unwrap_or_else(|poison| poison.into_inner());
    notes
        .iter()
        .rev()
        .find(|one| one.server == server && one.path == path && one.at <= at_secs && at_secs - one.at <= RECENT_SECS)
        .map(|one| one.who.clone())
}

/// The server-folder paths a request writes, without the studio root (`studio/...`; see [`with_studio_root`]).
pub fn paths_of(command: &str, body: &Value) -> Vec<String> {
    let text = |key: &str| body.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let joined = |dir: String, name: String| if dir.trim_matches('/').is_empty() { name } else { format!("{}/{name}", dir.trim_end_matches('/')) };
    let path = match command {
        "save_editable_file" | "trash_server_path" => text("path"),
        "create_server_file" => joined(text("path"), text("name")),
        "studio_save" => format!("studio/{}/{}.json", text("kind"), text("name")),
        "studio_asset_add" | "studio_asset_add_upload" | "studio_asset_remove" => format!("studio/assets/{}/{}", text("folder"), text("file")),
        _ => String::new(),
    };
    let path = clean(&path);
    if path.is_empty() || path.contains("//") || path.ends_with("/.json") { Vec::new() } else { vec![path] }
}

/// `studio/...` from [`paths_of`] under the server's real studio folder (`plugins/Monolab/studio` or Pumpkin's).
pub fn with_studio_root(path: &str, studio_root: Option<&str>) -> String {
    match (path.strip_prefix("studio/"), studio_root) {
        (Some(rest), Some(root)) => format!("{root}/{rest}"),
        _ => path.to_string(),
    }
}

/// The name shown for a writer: `@amy`, `@amy's AI` for a teammate's agent.
pub fn teammate(handle: &str, agent: bool) -> String {
    if agent { format!("@{handle}'s AI") } else { format!("@{handle}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paths_of_known_commands() {
        assert_eq!(paths_of("save_editable_file", &json!({ "id": "s", "path": "/config/a.yml" })), ["config/a.yml"]);
        assert_eq!(paths_of("create_server_file", &json!({ "path": "plugins", "name": "x.yml" })), ["plugins/x.yml"]);
        assert_eq!(paths_of("create_server_file", &json!({ "path": "", "name": "x.yml" })), ["x.yml"]);
        assert_eq!(paths_of("studio_save", &json!({ "kind": "mob", "name": "golem" })), ["studio/mob/golem.json"]);
        assert_eq!(paths_of("studio_asset_add_upload", &json!({ "folder": "models", "file": "bot.bbmodel" })), ["studio/assets/models/bot.bbmodel"]);
        assert_eq!(paths_of("trash_server_path", &json!({ "path": "old.txt" })), ["old.txt"]);
        assert!(paths_of("set_server_memo", &json!({ "id": "s" })).is_empty());
        assert!(paths_of("studio_save", &json!({})).is_empty());
        assert_eq!(with_studio_root("studio/mob/golem.json", Some("plugins/Monolab/studio")), "plugins/Monolab/studio/mob/golem.json");
        assert_eq!(with_studio_root("config/a.yml", Some("plugins/Monolab/studio")), "config/a.yml");
    }

    #[test]
    fn author_only_for_same_server_path_and_recent() {
        let at = 1_000_000;
        note_at("srv-a", "config/a.yml", "@amy", at - 60);
        assert_eq!(who_changed("srv-a", "config/a.yml", at).as_deref(), Some("@amy"));
        assert_eq!(who_changed("srv-b", "config/a.yml", at), None);
        assert_eq!(who_changed("srv-a", "config/b.yml", at), None);
        note_at("srv-a", "old.yml", "@bob", at - 11 * 60);
        assert_eq!(who_changed("srv-a", "old.yml", at), None);
    }

    #[test]
    fn the_latest_writer_wins() {
        let at = 2_000_000;
        note_at("srv-k", "a.yml", "host", at);
        note_at("srv-k", "a.yml", "@amy", at);
        assert_eq!(who_changed("srv-k", "a.yml", at).as_deref(), Some("@amy"));
    }
}

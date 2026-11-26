//! The node's record of every teammate request: one JSON line per request in
//! `<data>/team-log/<YYYY-MM>.jsonl`, in the app's `TeamLogEntry` shape (`src-tauri/src/teamlog.rs`)
//! so the host app can read it later. Never trimmed; only root on this machine can clear it (the folder is
//! 0700, the files 0600). The team's Owner reads it through `__team_log` (F7), nobody else.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Argument names whose values are never written down (the app's list).
const SECRET: &[&str] = &["password", "secret", "token", "apiKey", "api_key", "privateKey", "pin", "data", "bytes"];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamLogEntry {
    /// Milliseconds since 1970.
    pub at: f64,
    pub who: String,
    pub via: String,
    pub device: String,
    pub server: String,
    pub command: String,
    pub detail: String,
    pub ok: bool,
    pub error: String,
}

pub struct TeamLog {
    folder: PathBuf,
    lock: Mutex<()>,
}

/// The arguments as they should be kept: secrets out, long strings cut (with their length).
pub fn redact(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(name, one)| {
                    let hidden = SECRET.iter().any(|secret| {
                        name.eq_ignore_ascii_case(secret) || name.to_ascii_lowercase().ends_with(&secret.to_ascii_lowercase())
                    });
                    (name.clone(), if hidden && !one.is_null() { Value::String("(hidden)".into()) } else { redact(one) })
                })
                .collect(),
        ),
        Value::Array(list) if list.len() > 50 => Value::String(format!("({} items)", list.len())),
        Value::Array(list) => Value::Array(list.iter().map(redact).collect()),
        Value::String(text) if text.chars().count() > 400 => {
            let start: String = text.chars().take(300).collect();
            Value::String(format!("{start}… ({} characters)", text.chars().count()))
        }
        other => other.clone(),
    }
}

impl TeamLog {
    pub fn new(data: &std::path::Path) -> TeamLog {
        TeamLog { folder: data.join("team-log"), lock: Mutex::new(()) }
    }

    /// The entry for one request (what [`TeamLog::record`] writes).
    pub fn entry(who: &str, via: &str, device: &str, command: &str, body: &Value, result: &Result<Value, String>) -> TeamLogEntry {
        let mut detail = redact(body).to_string();
        if detail.chars().count() > 4000 {
            detail = format!("{}…", detail.chars().take(4000).collect::<String>());
        }
        TeamLogEntry {
            at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_millis() as f64).unwrap_or(0.0),
            who: who.to_string(),
            via: via.to_string(),
            device: device.to_string(),
            server: body.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
            command: command.to_string(),
            detail,
            ok: result.is_ok(),
            error: result.as_ref().err().map(|one| one.chars().take(500).collect()).unwrap_or_default(),
        }
    }

    /// Writes down one request. `via` is like `team · developer`. A failed write is reported, not fatal.
    pub fn record(&self, who: &str, via: &str, device: &str, command: &str, body: &Value, result: &Result<Value, String>) {
        let entry = TeamLog::entry(who, via, device, command, body, result);
        if let Err(error) = self.append(&entry) {
            eprintln!("monolab-node: could not write the team log: {error}");
        }
    }

    /// Entries newest first, at most `limit`, older than `before` (ms) when given, narrowed by server, who and
    /// text in the command or detail.
    pub fn read(&self, server: &str, who: &str, text: &str, before: Option<f64>, limit: usize) -> Vec<TeamLogEntry> {
        let mut months: Vec<PathBuf> = fs::read_dir(&self.folder)
            .map(|dir| dir.flatten().map(|one| one.path()).filter(|path| path.extension().is_some_and(|ext| ext == "jsonl")).collect())
            .unwrap_or_default();
        months.sort();
        let text = text.to_lowercase();
        let mut out = Vec::new();
        for month in months.iter().rev() {
            let Ok(body) = fs::read_to_string(month) else { continue };
            for line in body.lines().rev() {
                let Ok(entry) = serde_json::from_str::<TeamLogEntry>(line) else { continue };
                if before.is_some_and(|at| entry.at >= at)
                    || (!server.is_empty() && entry.server != server)
                    || (!who.is_empty() && entry.who != who)
                    || (!text.is_empty() && !format!("{} {}", entry.command, entry.detail).to_lowercase().contains(&text))
                {
                    continue;
                }
                out.push(entry);
                if out.len() >= limit {
                    return out;
                }
            }
        }
        out
    }

    fn append(&self, entry: &TeamLogEntry) -> Result<(), String> {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&self.folder).map_err(|error| error.to_string())?;
        let month = chrono::Local::now().format("%Y-%m").to_string();
        let line = serde_json::to_string(entry).map_err(|error| error.to_string())?;
        let _held = self.lock.lock().unwrap_or_else(|poison| poison.into_inner());
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(self.folder.join(format!("{month}.jsonl")))
            .map_err(|error| error.to_string())?;
        writeln!(file, "{line}").map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn secrets_and_long_text_are_left_out() {
        let kept = redact(
            &json!({ "id": "s1", "password": "hunter2", "dbPassword": "x", "apiKey": "k", "rule": "keepInventory", "text": "a".repeat(1000) }),
        );
        assert_eq!(kept["id"], "s1");
        assert_eq!(kept["password"], "(hidden)");
        assert_eq!(kept["dbPassword"], "(hidden)");
        assert_eq!(kept["apiKey"], "(hidden)");
        assert_eq!(kept["rule"], "keepInventory");
        assert!(kept["text"].as_str().unwrap().ends_with("(1000 characters)"));
    }

    #[test]
    fn one_line_per_request_in_the_app_shape() {
        let data = crate::files::tests::sandbox("teamlog");
        let log = TeamLog::new(&data);
        log.record(
            "@bob",
            "team · viewer",
            "DEV",
            "save_editable_file",
            &json!({ "id": "s", "path": "a.yml", "content": "x" }),
            &Err("no".into()),
        );
        log.record("@bob", "team · viewer", "DEV", "list_instances", &json!({}), &Ok(json!([])));
        let file = fs::read_dir(data.join("team-log")).unwrap().next().unwrap().unwrap().path();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        let text = fs::read_to_string(&file).unwrap();
        let lines: Vec<Value> = text.lines().map(|one| serde_json::from_str(one).unwrap()).collect();
        assert_eq!(lines.len(), 2);
        let first = &lines[0];
        for field in ["at", "who", "via", "device", "server", "command", "detail", "ok", "error"] {
            assert!(first.get(field).is_some(), "{field}");
        }
        assert_eq!(first["server"], "s");
        assert_eq!(first["ok"], false);
        assert_eq!(first["error"], "no");
        assert_eq!(lines[1]["ok"], true);
        fs::remove_dir_all(&data).unwrap();
    }
}

//! The host's record of everything other people did on its servers: Monolink teammates and
//! web panel accounts. One JSON line per request in
//! `team-log/<YYYY-MM>.jsonl` (app data), never trimmed: when something breaks — a world setting
//! changed, a file deleted, a plugin added — the host can see who did it. Only the host can read or
//! clear it: neither command is on the web allow list, and Monolink windows can't call them.
//!
//! A screen left open asks the same thing again and again, so the same person reading the same thing
//! with the same arguments within a minute stays one line with a count. Writes and refusals always get
//! their own line.

use crate::data_root::MonolabDirs;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

const FOLDER: &str = "team-log";

/// Asked over and over by the screens; they say nothing about what someone did.
const QUIET: &[&str] = &["__events", "__whoami", "__upload_part", "__presence", "__editing", "__download_part"];

/// Argument names whose values are never written down.
// (Not plain `key`: setting edits name the setting that way, like `"key": "difficulty"`.)
const SECRET: &[&str] = &["password", "secret", "token", "apiKey", "api_key", "privateKey", "pin", "data", "bytes"];

/// A repeat of the same read within this long joins the line before.
const SAME_READ_MS: f64 = 60_000.0;

/// How far back from the end of the month file a repeat is looked for.
const TAIL_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TeamLogEntry {
    /// Milliseconds since 1970.
    pub at: f64,
    /// `@handle` for a teammate, the web account's name otherwise.
    pub who: String,
    /// `team` or `web`, with the role: `team · developer`, `web · Manager`.
    pub via: String,
    /// The device the request came from (Monolink), empty for the web panel.
    pub device: String,
    /// The server it was about, empty for requests about no server.
    pub server: String,
    pub command: String,
    /// The arguments, with secrets left out and long text shortened.
    pub detail: String,
    pub ok: bool,
    /// Why it was refused or failed.
    pub error: String,
    /// How many times this same read came in a row (1 for most lines).
    #[serde(default = "one")]
    pub count: u32,
    /// When the last of them came: the same as `at` for a single request.
    #[serde(default)]
    pub last_at: f64,
}

fn one() -> u32 {
    1
}

impl TeamLogEntry {
    fn from_line(line: &str) -> Option<TeamLogEntry> {
        let mut entry: TeamLogEntry = serde_json::from_str(line).ok()?;
        entry.count = entry.count.max(1);
        entry.last_at = entry.last_at.max(entry.at);
        Some(entry)
    }

    /// Only successful reads fold together.
    fn folds(&self) -> bool {
        self.ok && self.error.is_empty() && crate::web::api::is_read(&self.command)
    }

    fn same_request(&self, other: &TeamLogEntry) -> bool {
        self.folds()
            && other.folds()
            && self.who == other.who
            && self.via == other.via
            && self.device == other.device
            && self.server == other.server
            && self.command == other.command
            && self.detail == other.detail
    }
}

/// What to show and export: one server (or all), one person (or everyone), a command or its
/// arguments containing some text, refused requests only.
#[derive(Clone, Debug, Default)]
struct Filter {
    server: String,
    who: String,
    text: String,
    refused_only: bool,
}

impl Filter {
    fn new(server: String, who: &str, text: &str, refused_only: bool) -> Filter {
        Filter { server, who: who.trim().to_lowercase(), text: text.trim().to_lowercase(), refused_only }
    }

    fn keeps(&self, entry: &TeamLogEntry) -> bool {
        (self.server.is_empty() || entry.server == self.server)
            && (self.who.is_empty() || entry.who.to_lowercase().contains(&self.who))
            && (self.text.is_empty()
                || entry.command.to_lowercase().contains(&self.text)
                || entry.detail.to_lowercase().contains(&self.text))
            && (!self.refused_only || !entry.ok)
    }
}

fn folder(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().monolab_data_dir().map_err(|error| error.to_string())?.join(FOLDER))
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poison| poison.into_inner())
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

/// Writes down one request. `via` is like `team · developer`.
#[allow(clippy::too_many_arguments)]
pub fn record(app: &AppHandle, who: &str, via: &str, device: &str, command: &str, body: &Value, result: &Result<Value, String>) {
    if QUIET.contains(&command) {
        return;
    }
    let server = body
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| crate::web_servers_of(app, command, body).into_iter().next())
        .unwrap_or_default();
    let mut detail = redact(body).to_string();
    if detail.chars().count() > 4000 {
        detail = format!("{}…", detail.chars().take(4000).collect::<String>());
    }
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_millis() as f64).unwrap_or(0.0);
    let entry = TeamLogEntry {
        at,
        who: who.to_string(),
        via: via.to_string(),
        device: device.to_string(),
        server,
        command: command.to_string(),
        detail,
        ok: result.is_ok(),
        error: result.as_ref().err().map(|one| one.chars().take(500).collect()).unwrap_or_default(),
        count: 1,
        last_at: at,
    };
    let _ = append(app, &entry);
}

fn append(app: &AppHandle, entry: &TeamLogEntry) -> Result<(), String> {
    let dir = folder(app)?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let month = chrono::Local::now().format("%Y-%m").to_string();
    let path = dir.join(format!("{month}.jsonl"));
    let line = serde_json::to_string(entry).map_err(|error| error.to_string())?;
    let _held = lock();
    if entry.folds() && fold(&path, entry).unwrap_or(false) {
        return Ok(());
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path).map_err(|error| error.to_string())?;
    writeln!(file, "{line}").map_err(|error| error.to_string())
}

/// Adds `entry` to the same read near the end of the file, if one came within a minute. Only the
/// lines from that one on are written again; nothing is dropped.
fn fold(path: &Path, entry: &TeamLogEntry) -> std::io::Result<bool> {
    let Ok(mut file) = OpenOptions::new().read(true).write(true).open(path) else { return Ok(false) };
    let length = file.metadata()?.len();
    let from = length.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(from))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    // Start at a whole line.
    let skip = if from == 0 {
        0
    } else {
        match bytes.iter().position(|byte| *byte == b'\n') {
            Some(at) => at + 1,
            None => return Ok(false),
        }
    };
    let Ok(tail) = std::str::from_utf8(&bytes[skip..]) else { return Ok(false) };
    let Some((offset, text)) = fold_into(tail, entry) else { return Ok(false) };
    let start = from + (skip + offset) as u64;
    file.seek(SeekFrom::Start(start))?;
    file.write_all(text.as_bytes())?;
    file.set_len(start + text.len() as u64)?;
    Ok(true)
}

/// Looks for the same read in `tail` (whole lines). Returns where the rewrite starts in `tail` and
/// the text from there on: that line with the count raised, then every line after it as it was.
fn fold_into(tail: &str, entry: &TeamLogEntry) -> Option<(usize, String)> {
    let mut lines = Vec::new();
    let mut offset = 0;
    for raw in tail.split_inclusive('\n') {
        lines.push((offset, raw));
        offset += raw.len();
    }
    for (offset, raw) in lines.into_iter().rev() {
        let Some(mut earlier) = TeamLogEntry::from_line(raw.trim_end()) else { continue };
        if (entry.at - earlier.last_at).abs() > SAME_READ_MS || !earlier.same_request(entry) {
            continue;
        }
        earlier.count = earlier.count.saturating_add(entry.count.max(1));
        earlier.last_at = earlier.last_at.max(entry.last_at.max(entry.at));
        let mut text = serde_json::to_string(&earlier).ok()?;
        text.push('\n');
        text.push_str(&tail[offset + raw.len()..]);
        return Some((offset, text));
    }
    None
}

/// The month files, newest first.
fn months(app: &AppHandle) -> Vec<PathBuf> {
    let Ok(dir) = folder(app) else { return Vec::new() };
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|list| {
            list.filter_map(|one| one.ok().map(|one| one.path())).filter(|one| one.extension().is_some_and(|ext| ext == "jsonl")).collect()
        })
        .unwrap_or_default();
    files.sort();
    files.reverse();
    files
}

/// What other people did, newest first: for one server (or all, with `server` empty), one person
/// (or everyone), a command or arguments holding `text`, only refused ones, before a time (for
/// paging), at most `limit`.
#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub fn team_log(
    app: AppHandle,
    server: String,
    who: String,
    text: String,
    refused_only: bool,
    before: Option<f64>,
    limit: u32,
) -> Vec<TeamLogEntry> {
    let limit = limit.clamp(1, 2000) as usize;
    let filter = Filter::new(server, &who, &text, refused_only);
    let mut out = Vec::new();
    let _held = lock();
    for file in months(&app) {
        let Ok(text) = fs::read_to_string(&file) else { continue };
        for line in text.lines().rev() {
            let Some(entry) = TeamLogEntry::from_line(line) else { continue };
            if before.is_some_and(|before| entry.at >= before) || !filter.keeps(&entry) {
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

/// Saves what the filters show as a CSV file, oldest first (this PC only: not on the web allow list,
/// refused in Monolink windows). Returns how many lines were written.
#[tauri::command(async)]
#[specta::specta]
pub fn export_team_log(
    app: AppHandle,
    target: String,
    server: String,
    who: String,
    text: String,
    refused_only: bool,
) -> Result<u32, String> {
    let destination = PathBuf::from(target.trim());
    if destination.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case("csv")) {
        return Err("The export file name must end with .csv.".into());
    }
    let filter = Filter::new(server, &who, &text, refused_only);
    let mut out = String::from("\u{feff}");
    out.push_str(&csv_row(&["Time", "Last time", "Count", "Person", "Via", "Device", "Server", "Command", "Arguments", "Result", "Error"]));
    let mut written = 0u32;
    {
        let _held = lock();
        for file in months(&app).iter().rev() {
            let Ok(text) = fs::read_to_string(file) else { continue };
            for entry in text.lines().filter_map(TeamLogEntry::from_line).filter(|entry| filter.keeps(entry)) {
                out.push_str(&csv_line(&entry, &local_time));
                written += 1;
            }
        }
    }
    fs::write(&destination, out).map_err(|error| format!("Could not save the log: {error}"))?;
    Ok(written)
}

fn local_time(at: f64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(at as i64).single().map(|time| time.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default()
}

fn csv_line(entry: &TeamLogEntry, time: &dyn Fn(f64) -> String) -> String {
    let last = if entry.count > 1 { time(entry.last_at) } else { String::new() };
    let result = if entry.ok { "ok" } else { "refused" };
    csv_row(&[
        &time(entry.at),
        &last,
        &entry.count.to_string(),
        &entry.who,
        &entry.via,
        &entry.device,
        &entry.server,
        &entry.command,
        &entry.detail,
        result,
        &entry.error,
    ])
}

fn csv_row(fields: &[&str]) -> String {
    let mut line = fields.iter().map(|one| csv_field(one)).collect::<Vec<_>>().join(",");
    line.push_str("\r\n");
    line
}

/// One CSV cell. Quotes, commas and line breaks are quoted; text a spreadsheet would run as a formula
/// (teammates choose what they send) starts with `'` so it stays text.
fn csv_field(value: &str) -> String {
    let guarded = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{value}") } else { value.to_string() };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// Clears the log (this PC only: not on the web allow list, refused in Monolink windows). The
/// clearing itself is the first line of the new log.
#[tauri::command(async)]
#[specta::specta]
pub fn clear_team_log(app: AppHandle) -> Result<u32, String> {
    let files = months(&app);
    let mut gone = 0u32;
    {
        let _held = lock();
        for file in &files {
            gone += fs::read_to_string(file).map(|text| text.lines().count() as u32).unwrap_or(0);
            fs::remove_file(file).map_err(|error| format!("Could not clear the log: {error}"))?;
        }
    }
    record(&app, "host", "this PC", "", "clear_team_log", &serde_json::json!({ "entries": gone }), &Ok(Value::Null));
    Ok(gone)
}

#[cfg(test)]
mod tests {
    use super::{csv_field, csv_line, fold_into, Filter, TeamLogEntry};
    use serde_json::json;

    fn read(who: &str, command: &str, detail: &str, at: f64) -> TeamLogEntry {
        TeamLogEntry {
            at,
            who: who.into(),
            via: "team · developer".into(),
            device: "d1".into(),
            server: "s1".into(),
            command: command.into(),
            detail: detail.into(),
            ok: true,
            error: String::new(),
            count: 1,
            last_at: at,
        }
    }

    fn lines(list: &[TeamLogEntry]) -> String {
        list.iter().map(|one| serde_json::to_string(one).unwrap() + "\n").collect()
    }

    /// Writes `entry` the way `append` does, on a file held as text.
    fn append(file: &mut String, entry: &TeamLogEntry) {
        if entry.folds() {
            if let Some((offset, text)) = fold_into(file, entry) {
                file.truncate(offset);
                file.push_str(&text);
                return;
            }
        }
        file.push_str(&lines(std::slice::from_ref(entry)));
    }

    fn parsed(file: &str) -> Vec<TeamLogEntry> {
        file.lines().filter_map(TeamLogEntry::from_line).collect()
    }

    #[test]
    fn the_same_read_within_a_minute_is_one_line() {
        let mut file = String::new();
        append(&mut file, &read("@bob", "get_console_logs", r#"{"id":"s1"}"#, 1_000.0));
        append(&mut file, &read("@amy", "list_players", r#"{"id":"s1"}"#, 2_000.0));
        append(&mut file, &read("@bob", "get_console_logs", r#"{"id":"s1"}"#, 30_000.0));
        append(&mut file, &read("@bob", "get_console_logs", r#"{"id":"s1"}"#, 85_000.0));
        let all = parsed(&file);
        assert_eq!(all.len(), 2, "{file}");
        assert_eq!(all[0].who, "@bob");
        assert_eq!(all[0].count, 3);
        assert_eq!(all[0].at, 1_000.0);
        assert_eq!(all[0].last_at, 85_000.0);
        assert_eq!(all[1].who, "@amy", "the lines after it stay as they were");
    }

    #[test]
    fn a_minute_apart_or_different_is_a_new_line() {
        let mut file = String::new();
        append(&mut file, &read("@bob", "get_console_logs", r#"{"id":"s1"}"#, 0.0));
        append(&mut file, &read("@bob", "get_console_logs", r#"{"id":"s1"}"#, 61_000.0));
        append(&mut file, &read("@bob", "get_console_logs", r#"{"id":"s2"}"#, 62_000.0));
        append(&mut file, &read("@amy", "get_console_logs", r#"{"id":"s1"}"#, 63_000.0));
        assert_eq!(parsed(&file).len(), 4);
    }

    #[test]
    fn writes_and_refusals_are_never_folded() {
        let mut file = String::new();
        let write = read("@bob", "send_console_command", r#"{"id":"s1","command":"list"}"#, 0.0);
        append(&mut file, &write);
        append(&mut file, &read("@bob", "send_console_command", r#"{"id":"s1","command":"list"}"#, 1_000.0));
        let mut refused = read("@bob", "get_console_logs", r#"{"id":"s1"}"#, 2_000.0);
        refused.ok = false;
        refused.error = "You don't have permission for this server.".into();
        append(&mut file, &refused);
        append(&mut file, &refused.clone());
        append(&mut file, &read("@bob", "get_console_logs", r#"{"id":"s1"}"#, 3_000.0));
        let all = parsed(&file);
        assert_eq!(all.len(), 5);
        assert!(all.iter().all(|one| one.count == 1));
    }

    #[test]
    fn old_lines_read_as_single_requests() {
        let old = r#"{"at":5.0,"who":"@bob","via":"team · viewer","device":"","server":"s1","command":"list_players","detail":"{}","ok":true,"error":""}"#;
        let entry = TeamLogEntry::from_line(old).unwrap();
        assert_eq!(entry.count, 1);
        assert_eq!(entry.last_at, 5.0);
    }

    #[test]
    fn filters_by_text_and_refusal() {
        let mut refused = read("@bob", "save_world_settings", r#"{"difficulty":"hard"}"#, 0.0);
        refused.ok = false;
        let fine = read("@amy", "send_console_command", r#"{"command":"op Amy"}"#, 0.0);
        let only_refused = Filter::new(String::new(), "", "", true);
        assert!(only_refused.keeps(&refused) && !only_refused.keeps(&fine));
        let by_text = Filter::new(String::new(), "", "OP amy", false);
        assert!(by_text.keeps(&fine) && !by_text.keeps(&refused));
        let by_command = Filter::new("s1".into(), "BOB", "world", false);
        assert!(by_command.keeps(&refused) && !by_command.keeps(&fine));
    }

    #[test]
    fn csv_cells_are_escaped() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field(r#"{"command":"list"}"#), r#""{""command"":""list""}""#);
        assert_eq!(csv_field("two\nlines"), "\"two\nlines\"");
        assert_eq!(csv_field("=HYPERLINK(\"x\")"), "\"'=HYPERLINK(\"\"x\"\")\"");
        assert_eq!(csv_field("@bob"), "'@bob");
        assert_eq!(csv_field("-1"), "'-1");
        assert_eq!(csv_field(""), "");
    }

    #[test]
    fn a_csv_line_has_every_column() {
        let mut entry = read("@bob", "get_console_logs", r#"{"id":"s1"}"#, 1.0);
        entry.count = 4;
        entry.last_at = 2.0;
        let line = csv_line(&entry, &|at| format!("t{at}"));
        assert_eq!(line, "t1,t2,4,'@bob,team · developer,d1,s1,get_console_logs,\"{\"\"id\"\":\"\"s1\"\"}\",ok,\r\n");
    }

    #[test]
    fn secrets_and_long_text_are_left_out() {
        let kept = super::redact(&json!({
            "id": "s1",
            "password": "hunter2",
            "dbPassword": "x",
            "apiKey": "k",
            "data": "QUJD",
            "rule": "keepInventory",
            "value": true,
            "text": "a".repeat(1000),
        }));
        assert_eq!(kept["id"], "s1");
        assert_eq!(kept["password"], "(hidden)");
        assert_eq!(kept["dbPassword"], "(hidden)");
        assert_eq!(kept["apiKey"], "(hidden)");
        assert_eq!(kept["data"], "(hidden)");
        assert_eq!(kept["rule"], "keepInventory");
        assert_eq!(kept["value"], true);
        assert!(kept["text"].as_str().unwrap().ends_with("(1000 characters)"));
    }
}

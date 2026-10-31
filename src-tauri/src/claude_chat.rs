
use serde::Serialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

use crate::claude_agent;

const HISTORY_LINES: usize = 600;
/// Chats that keep running in the background per server; the oldest idle one is closed past this.
const LIVE_CHATS: usize = 4;

struct Chat {
    server: String,
    session: String,
    stdin: ChildStdin,
    child: Child,
    alive: bool,
    busy: bool,
    pending: Vec<(String, String)>,
}

fn chats() -> &'static Mutex<HashMap<String, Chat>> {
    static ALL: OnceLock<Mutex<HashMap<String, Chat>>> = OnceLock::new();
    ALL.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Serialize, Clone, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatLine {
    pub id: String,
    pub line: String,
    pub exited: bool,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatState {
    pub id: String,
    pub pending: Vec<String>,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub updated: f64,
    pub chat: String,
    pub busy: bool,
}

fn emit(app: &AppHandle, id: &str, line: String, exited: bool) {
    let _ = app.emit("claude-chat", ChatLine { id: id.to_string(), line, exited });
}

pub fn state(server: &str, session: &str) -> ChatState {
    let all = chats().lock().expect("chat poisoned");
    all.iter()
        .filter(|(_, one)| one.server == server && one.alive && (session.is_empty() || one.session == session))
        .max_by(|a, b| a.0.cmp(b.0))
        .map(|(id, one)| ChatState { id: id.clone(), pending: one.pending.iter().map(|(_, line)| line.clone()).collect() })
        .unwrap_or(ChatState { id: String::new(), pending: Vec::new() })
}

fn is_session(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|one| one.is_ascii_hexdigit() || one == '-')
}

/// `session`: empty starts a new chat, "last" continues the newest one, otherwise resumes that session.
pub fn open(app: &AppHandle, server: &str, session: &str, mode: &str, model: &str) -> Result<String, String> {
    let mode = permission_mode(mode)?;
    let model = model_name(model)?;
    let session = session.trim();
    let named = is_session(session);
    if !session.is_empty() && session != "last" && !named {
        return Err("Unknown conversation.".into());
    }
    if named {
        let all = chats().lock().expect("chat poisoned");
        if let Some((id, _)) = all.iter().find(|(_, one)| one.server == server && one.alive && one.session == session) {
            return Ok(id.clone());
        }
    }
    let claude = claude_agent::find_claude(app)
        .ok_or("Claude Code isn't installed on this computer. Install it as shown, then try again.")?;
    let instance = crate::instance_by_id(app, server)?;
    let dir = claude_agent::workspace(app, server)?;
    let config = claude_agent::mcp_config(app, server)?;
    let resume = session == "last" && claude_agent::has_history(app, &dir);

    make_room(server);

    let is_cmd = claude.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("cmd"));
    let mut command = if is_cmd {
        let mut one = crate::hidden_command("cmd");
        one.arg("/c").arg(&claude);
        one
    } else {
        crate::hidden_command(&claude)
    };
    command.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
    ]);
    command.arg("--permission-mode").arg(mode);
    command.arg("--allow-dangerously-skip-permissions");
    if !model.is_empty() {
        command.arg("--model").arg(model);
    }
    if named {
        command.arg("--resume").arg(session);
    } else if resume {
        command.arg("--continue");
    }
    command.args(claude_agent::DENIED_TOOLS);
    command.arg("--mcp-config").arg(&config).arg("--strict-mcp-config");
    command.arg("--append-system-prompt").arg(claude_agent::briefing(&instance, &dir));
    command.current_dir(&dir);
    if let Some(account) = crate::claude_accounts::config_for_server(app, server) {
        command.env("CLAUDE_CONFIG_DIR", account);
    }
    if std::env::var_os("CLAUDE_CODE_AUTO_COMPACT_WINDOW").is_none() {
        // Long chats re-read their whole history every turn; compact well before the 1M window fills.
        command.env("CLAUDE_CODE_AUTO_COMPACT_WINDOW", "120000");
    }
    // No side calls (titles, tips, update checks) that spend the plan without the user asking.
    command.env("DISABLE_NON_ESSENTIAL_MODEL_CALLS", "1");
    command.env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1");
    command.env_remove("CLAUDECODE");
    command.env_remove("CLAUDE_CODE_ENTRYPOINT");
    command.env_remove("CLAUDE_CODE_CHILD_SESSION");
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| format!("Could not start Claude Code: {error}"))?;
    let stdin = child.stdin.take().ok_or("Could not open Claude Code's input.")?;
    let stdout = child.stdout.take().ok_or("Could not open Claude Code's output.")?;
    let stderr = child.stderr.take();

    let id = format!(
        "chat{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_millis()).unwrap_or(0)
    );
    chats().lock().expect("chat poisoned").insert(
        id.clone(),
        Chat {
            server: server.to_string(),
            session: if named { session.to_string() } else { String::new() },
            stdin,
            child,
            alive: true,
            busy: false,
            pending: Vec::new(),
        },
    );

    if let Some(stderr) = stderr {
        let app = app.clone();
        let key = id.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let text = line.trim().to_string();
                if text.is_empty() {
                    continue;
                }
                let wrapped = serde_json::json!({ "type": "monolab_stderr", "text": text }).to_string();
                emit(&app, &key, wrapped, false);
            }
        });
    }

    let app = app.clone();
    let key = id.clone();
    std::thread::spawn(move || {
        let mut known = named;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            let maybe_result = line.contains("\"type\":\"result\"");
            if maybe_result || (!known && line.contains("\"session_id\"")) {
                let value = serde_json::from_str::<serde_json::Value>(&line).ok();
                let finished = value.as_ref().is_some_and(|value| value.get("type").and_then(|one| one.as_str()) == Some("result"));
                let found = value
                    .and_then(|value| value.get("session_id").and_then(|one| one.as_str()).map(str::to_string))
                    .filter(|one| is_session(one));
                if let Some(one) = chats().lock().expect("chat poisoned").get_mut(&key) {
                    if let Some(found) = found.filter(|_| !known) {
                        one.session = found;
                        known = true;
                    }
                    if finished {
                        one.busy = false;
                    }
                }
            }
            if let Some(request) = permission_request(&line) {
                if let Some(one) = chats().lock().expect("chat poisoned").get_mut(&key) {
                    one.pending.push((request, line.clone()));
                }
            }
            emit(&app, &key, line, false);
        }
        if let Some(one) = chats().lock().expect("chat poisoned").get_mut(&key) {
            one.alive = false;
            one.pending.clear();
            let _ = one.child.wait();
        }
        emit(&app, &key, String::new(), true);
    });
    Ok(id)
}

fn permission_request(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("type")?.as_str()? != "control_request" {
        return None;
    }
    if value.get("request")?.get("subtype")?.as_str()? != "can_use_tool" {
        return None;
    }
    Some(value.get("request_id")?.as_str()?.to_string())
}

fn write_line(id: &str, value: &serde_json::Value) -> Result<(), String> {
    let mut all = chats().lock().expect("chat poisoned");
    let one = all.get_mut(id).ok_or("The conversation has ended. Please open it again.")?;
    if !one.alive {
        return Err("The conversation has ended. Please open it again.".to_string());
    }
    let mut text = value.to_string();
    text.push('\n');
    one.stdin.write_all(text.as_bytes()).map_err(|error| format!("Could not send to Claude Code: {error}"))?;
    one.stdin.flush().map_err(|error| error.to_string())
}

pub fn send(app: &AppHandle, id: &str, content: &str) -> Result<(), String> {
    let content: serde_json::Value =
        serde_json::from_str(content).map_err(|error| format!("The content to send is invalid: {error}"))?;
    if !(content.is_string() || content.is_array()) {
        return Err("The content to send is invalid.".to_string());
    }
    let message = serde_json::json!({ "type": "user", "message": { "role": "user", "content": content } });
    write_line(id, &message)?;
    if let Some(one) = chats().lock().expect("chat poisoned").get_mut(id) {
        one.busy = true;
    }
    emit(app, id, message.to_string(), false);
    Ok(())
}

pub fn answer(
    id: &str,
    request_id: &str,
    allow: bool,
    input: Option<String>,
    permissions: Option<String>,
    message: Option<String>,
) -> Result<(), String> {
    let response = if allow {
        let input: serde_json::Value = match input {
            Some(text) => serde_json::from_str(&text).map_err(|error| error.to_string())?,
            None => serde_json::json!({}),
        };
        let mut body = serde_json::json!({ "behavior": "allow", "updatedInput": input });
        if let Some(text) = permissions {
            let rules: serde_json::Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
            body["updatedPermissions"] = rules;
        }
        body
    } else {
        serde_json::json!({
            "behavior": "deny",
            "message": message.unwrap_or_else(|| "The user said no.".to_string()),
        })
    };
    let line = serde_json::json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": request_id, "response": response },
    });
    write_line(id, &line)?;
    if let Some(one) = chats().lock().expect("chat poisoned").get_mut(id) {
        one.pending.retain(|(request, _)| request != request_id);
    }
    Ok(())
}

fn permission_mode(mode: &str) -> Result<&'static str, String> {
    match mode {
        "" | "default" => Ok("default"),
        "acceptEdits" => Ok("acceptEdits"),
        "bypassPermissions" => Ok("bypassPermissions"),
        _ => Err("Unknown permission mode.".into()),
    }
}

fn model_name(model: &str) -> Result<&str, String> {
    let model = model.trim();
    let fine = model.is_empty()
        || matches!(model, "default" | "opus" | "sonnet" | "haiku" | "fable" | "opusplan")
        || (model.starts_with("claude-") && model.len() < 64 && model.chars().all(|one| one.is_ascii_alphanumeric() || matches!(one, '-' | '.' | '[' | ']')));
    if !fine {
        return Err("Unknown model.".into());
    }
    Ok(if model == "default" { "" } else { model })
}

pub fn set_model(id: &str, model: &str) -> Result<(), String> {
    let model = model_name(model)?;
    let line = serde_json::json!({
        "type": "control_request",
        "request_id": request_id("model"),
        "request": { "subtype": "set_model", "model": if model.is_empty() { serde_json::Value::Null } else { model.into() } },
    });
    write_line(id, &line)
}

fn request_id(prefix: &str) -> String {
    format!(
        "{prefix}{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_nanos()).unwrap_or(0)
    )
}

pub fn set_mode(id: &str, mode: &str) -> Result<(), String> {
    let mode = permission_mode(mode)?;
    let request = request_id("mode");
    let line = serde_json::json!({
        "type": "control_request",
        "request_id": request,
        "request": { "subtype": "set_permission_mode", "mode": mode },
    });
    write_line(id, &line)
}

pub fn interrupt(id: &str) -> Result<(), String> {
    let request = format!(
        "stop{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_millis()).unwrap_or(0)
    );
    let line = serde_json::json!({
        "type": "control_request",
        "request_id": request,
        "request": { "subtype": "interrupt" },
    });
    write_line(id, &line)?;
    if let Some(one) = chats().lock().expect("chat poisoned").get_mut(id) {
        one.pending.clear();
        one.busy = false;
    }
    Ok(())
}

fn make_room(server: &str) {
    let mut live: Vec<(String, bool)> = chats()
        .lock()
        .expect("chat poisoned")
        .iter()
        .filter(|(_, one)| one.server == server)
        .map(|(id, one)| (id.clone(), one.busy && one.alive))
        .collect();
    live.sort();
    while live.len() >= LIVE_CHATS {
        let at = live.iter().position(|(_, busy)| !busy).unwrap_or(0);
        close(&live.remove(at).0);
    }
}

fn titles_file(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    Ok(app.path().app_data_dir().map_err(|error| error.to_string())?.join("claude-chat-titles.json"))
}

fn titles(app: &AppHandle) -> HashMap<String, String> {
    titles_file(app).ok().and_then(|file| crate::store::read(&file).ok().flatten()).unwrap_or_default()
}

/// The first thing the user typed, as the chat's name.
fn first_words(path: &std::path::Path) -> String {
    static SEEN: OnceLock<Mutex<HashMap<std::path::PathBuf, String>>> = OnceLock::new();
    let seen = SEEN.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(one) = seen.lock().expect("titles poisoned").get(path) {
        return one.clone();
    }
    let Ok(file) = std::fs::File::open(path) else { return String::new() };
    let mut found = String::new();
    for line in BufReader::new(file).lines().map_while(Result::ok).take(400) {
        if !line.contains("\"type\":\"user\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        let flag = |key: &str| value.get(key).and_then(|one| one.as_bool()).unwrap_or(false);
        if value.get("type").and_then(|one| one.as_str()) != Some("user") || flag("isMeta") || flag("isSidechain") || flag("isCompactSummary") {
            continue;
        }
        let content = value.get("message").and_then(|one| one.get("content"));
        let text = match content {
            Some(serde_json::Value::String(text)) => text.clone(),
            Some(serde_json::Value::Array(blocks)) => blocks
                .iter()
                .filter(|block| block.get("type").and_then(|one| one.as_str()) == Some("text"))
                .filter_map(|block| block.get("text").and_then(|one| one.as_str()))
                .collect::<Vec<_>>()
                .join(" "),
            _ => String::new(),
        };
        let text = text.split("Attached files (relative").next().unwrap_or("").trim().to_string();
        if text.is_empty() || text.starts_with('<') || text.starts_with("This session is being continued") {
            continue;
        }
        let line = text.lines().next().unwrap_or("").trim();
        found = line.chars().take(80).collect();
        break;
    }
    if !found.is_empty() {
        seen.lock().expect("titles poisoned").insert(path.to_path_buf(), found.clone());
    }
    found
}

pub fn sessions(app: &AppHandle, server: &str) -> Result<Vec<ChatSession>, String> {
    let dir = claude_agent::workspace(app, server)?;
    let Some(place) = claude_agent::history_dir(app, &dir) else { return Ok(Vec::new()) };
    let names = titles(app);
    let live: HashMap<String, (String, bool)> = chats()
        .lock()
        .expect("chat poisoned")
        .iter()
        .filter(|(_, one)| one.server == server && one.alive && !one.session.is_empty())
        .map(|(id, one)| (one.session.clone(), (id.clone(), one.busy)))
        .collect();
    let mut list: Vec<ChatSession> = std::fs::read_dir(place)
        .map(|entries| entries.flatten().collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "jsonl") {
                return None;
            }
            let id = path.file_stem()?.to_string_lossy().into_owned();
            if !is_session(&id) {
                return None;
            }
            let updated = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|at| at.as_millis() as f64)
                .unwrap_or(0.0);
            let title = names.get(&id).cloned().unwrap_or_else(|| first_words(&path));
            let (chat, busy) = live.get(&id).cloned().unwrap_or_default();
            if title.is_empty() && chat.is_empty() {
                return None;
            }
            Some(ChatSession { id, title, updated, chat, busy })
        })
        .collect();
    list.sort_by(|a, b| b.updated.total_cmp(&a.updated));
    Ok(list)
}

pub fn rename(app: &AppHandle, session: &str, title: &str) -> Result<(), String> {
    if !is_session(session) {
        return Err("Unknown conversation.".into());
    }
    let file = titles_file(app)?;
    let mut names = titles(app);
    let title: String = title.trim().chars().take(100).collect();
    if title.is_empty() {
        names.remove(session);
    } else {
        names.insert(session.to_string(), title);
    }
    let text = serde_json::to_string_pretty(&names).map_err(|error| error.to_string())?;
    crate::store::write(&file, &text).map_err(|error| format!("Could not save the chat name: {error}"))
}

pub fn delete(app: &AppHandle, server: &str, session: &str) -> Result<(), String> {
    if !is_session(session) {
        return Err("Unknown conversation.".into());
    }
    let ids: Vec<String> = chats()
        .lock()
        .expect("chat poisoned")
        .iter()
        .filter(|(_, one)| one.server == server && one.session == session)
        .map(|(id, _)| id.clone())
        .collect();
    for id in ids {
        close(&id);
    }
    let dir = claude_agent::workspace(app, server)?;
    let place = claude_agent::history_dir(app, &dir).ok_or("Could not find the chat history.")?;
    let file = place.join(format!("{session}.jsonl"));
    if file.exists() {
        std::fs::remove_file(&file).map_err(|error| format!("Could not delete the conversation: {error}"))?;
    }
    let extra = place.join(session);
    if extra.is_dir() {
        let _ = std::fs::remove_dir_all(extra);
    }
    if titles(app).contains_key(session) {
        rename(app, session, "")?;
    }
    Ok(())
}

pub fn history(app: &AppHandle, server: &str, session: &str) -> Result<Vec<String>, String> {
    let dir = claude_agent::workspace(app, server)?;
    let Some(place) = claude_agent::history_dir(app, &dir) else { return Ok(Vec::new()) };
    if is_session(session) {
        let text = std::fs::read_to_string(place.join(format!("{session}.jsonl"))).unwrap_or_default();
        return Ok(conversation_lines(&text));
    }
    let newest = std::fs::read_dir(place)
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
                .max_by_key(|entry| entry.metadata().and_then(|meta| meta.modified()).ok())
        })
        .ok()
        .flatten();
    let Some(file) = newest else { return Ok(Vec::new()) };
    let text = std::fs::read_to_string(file.path()).map_err(|error| error.to_string())?;
    Ok(conversation_lines(&text))
}

fn conversation_lines(text: &str) -> Vec<String> {
    // From the end, so a long conversation only parses the part that is shown.
    let mut lines: Vec<String> = text
        .lines()
        .rev()
        .filter(|line| {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { return false };
            let kind = value.get("type").and_then(|one| one.as_str()).unwrap_or("");
            let meta = value.get("isMeta").and_then(|one| one.as_bool()).unwrap_or(false);
            let side = value.get("isSidechain").and_then(|one| one.as_bool()).unwrap_or(false);
            (kind == "user" || kind == "assistant") && !meta && !side
        })
        .take(HISTORY_LINES)
        .map(str::to_string)
        .collect();
    lines.reverse();
    lines
}

pub fn close(id: &str) {
    if let Some(mut one) = chats().lock().expect("chat poisoned").remove(id) {
        let _ = one.child.kill();
    }
}

pub fn close_server(server: &str) {
    let ids: Vec<String> = chats()
        .lock()
        .expect("chat poisoned")
        .iter()
        .filter(|(_, one)| one.server == server)
        .map(|(id, _)| id.clone())
        .collect();
    for id in ids {
        close(&id);
    }
}

pub fn close_all() {
    let mut all = chats().lock().expect("chat poisoned");
    for (_, mut one) in all.drain() {
        let _ = one.child.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_session_ids() {
        assert!(is_session("0b5d2f1c-6a0e-4c7e-9a49-3f1b2c0d9e88"));
        assert!(!is_session(""));
        assert!(!is_session("last"));
        assert!(!is_session("../x"));
    }

    #[test]
    fn spots_a_permission_request() {
        let line = r#"{"type":"control_request","request_id":"abc","request":{"subtype":"can_use_tool","tool_name":"Write","input":{}}}"#;
        assert_eq!(permission_request(line).as_deref(), Some("abc"));
        assert_eq!(permission_request(r#"{"type":"assistant","message":{}}"#), None);
        assert_eq!(permission_request(r#"{"type":"control_request","request_id":"x","request":{"subtype":"interrupt"}}"#), None);
        assert_eq!(permission_request("not json"), None);
    }

    #[test]
    fn keeps_only_the_conversation_from_a_transcript() {
        let text = [
            r#"{"type":"summary","summary":"x"}"#,
            r#"{"type":"user","message":{"role":"user","content":"hi"}}"#,
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"meta"}}"#,
            r#"{"type":"assistant","isSidechain":true,"message":{"content":[]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hello"}]}}"#,
            "broken",
        ]
        .join("\n");
        let kept = conversation_lines(&text);
        assert_eq!(kept.len(), 2);
        assert!(kept[0].contains("\"hi\""));
        assert!(kept[1].contains("hello"));
    }
}

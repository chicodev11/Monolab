
use serde::Serialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter};

use crate::claude_agent;

const HISTORY_LINES: usize = 600;

struct Chat {
    server: String,
    stdin: ChildStdin,
    child: Child,
    alive: bool,
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

fn emit(app: &AppHandle, id: &str, line: String, exited: bool) {
    let _ = app.emit("claude-chat", ChatLine { id: id.to_string(), line, exited });
}

pub fn state(server: &str) -> ChatState {
    let all = chats().lock().expect("chat poisoned");
    all.iter()
        .find(|(_, one)| one.server == server && one.alive)
        .map(|(id, one)| ChatState { id: id.clone(), pending: one.pending.iter().map(|(_, line)| line.clone()).collect() })
        .unwrap_or(ChatState { id: String::new(), pending: Vec::new() })
}

pub fn open(app: &AppHandle, server: &str, fresh: bool, mode: &str, model: &str) -> Result<String, String> {
    let mode = permission_mode(mode)?;
    let model = model_name(model)?;
    let claude = claude_agent::find_claude(app)
        .ok_or("Claude Code isn't installed on this computer. Install it as shown, then try again.")?;
    let instance = crate::instance_by_id(app, server)?;
    let dir = claude_agent::workspace(app, server)?;
    let config = claude_agent::mcp_config(app, server)?;
    let resume = !fresh && claude_agent::has_history(app, &dir);

    close_server(server);

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
    if resume {
        command.arg("--continue");
    }
    command.args(claude_agent::DENIED_DELETES);
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
        Chat { server: server.to_string(), stdin, child, alive: true, pending: Vec::new() },
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
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
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
    }
    Ok(())
}

pub fn history(app: &AppHandle, server: &str) -> Result<Vec<String>, String> {
    let dir = claude_agent::workspace(app, server)?;
    let Some(place) = claude_agent::history_dir(app, &dir) else { return Ok(Vec::new()) };
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
    let lines: Vec<String> = text
        .lines()
        .filter(|line| {
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { return false };
            let kind = value.get("type").and_then(|one| one.as_str()).unwrap_or("");
            let meta = value.get("isMeta").and_then(|one| one.as_bool()).unwrap_or(false);
            let side = value.get("isSidechain").and_then(|one| one.as_bool()).unwrap_or(false);
            (kind == "user" || kind == "assistant") && !meta && !side
        })
        .map(str::to_string)
        .collect();
    let start = lines.len().saturating_sub(HISTORY_LINES);
    lines[start..].to_vec()
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

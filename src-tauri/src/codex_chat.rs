use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};

use crate::claude_agent;
use crate::claude_chat::{ChatLine, ChatState};

const HISTORY_LINES: usize = 600;
const TOKEN_VAR: &str = "MONOLAB_MCP_TOKEN";

struct Session {
    server: String,
    mode: String,
    model: String,
    turn: Option<Child>,
}

fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    static ALL: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
    ALL.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn is_codex(id: &str) -> bool {
    id.starts_with("codex")
}

pub fn find_codex(app: &AppHandle) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["codex.exe", "codex.cmd"] } else { &["codex"] };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in names {
                let one = dir.join(name);
                if one.is_file() {
                    return Some(one);
                }
            }
        }
    }
    if cfg!(windows) {
        if let Ok(roaming) = app.path().data_dir() {
            let npm = roaming.join("npm").join("codex.cmd");
            if npm.is_file() {
                return Some(npm);
            }
        }
        if let Ok(local) = app.path().local_data_dir() {
            let bundled = std::fs::read_dir(local.join("OpenAI").join("Codex").join("bin"))
                .ok()?
                .flatten()
                .map(|entry| entry.path().join("codex.exe"))
                .filter(|one| one.is_file())
                .max_by_key(|one| one.metadata().and_then(|meta| meta.modified()).ok());
            if bundled.is_some() {
                return bundled;
            }
        }
    }
    None
}

fn codex_home(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?.join("codex");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn with_home(app: &AppHandle, command: &mut std::process::Command) {
    if let Some(dir) = codex_home(app) {
        command.env("CODEX_HOME", dir);
    }
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CodexModel {
    pub slug: String,
    pub name: String,
    pub description: String,
}

pub fn models(app: &AppHandle) -> Vec<CodexModel> {
    let Some(text) = codex_home(app).and_then(|dir| std::fs::read_to_string(dir.join("models_cache.json")).ok()) else {
        return Vec::new();
    };
    let Ok(doc) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    let mut list: Vec<(i64, CodexModel)> = doc
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|one| one.get("visibility").and_then(Value::as_str).unwrap_or("list") == "list")
        .filter_map(|one| {
            let slug = one.get("slug").and_then(Value::as_str)?.to_string();
            Some((
                one.get("priority").and_then(Value::as_i64).unwrap_or(1000),
                CodexModel {
                    name: one.get("display_name").and_then(Value::as_str).unwrap_or(&slug).to_string(),
                    description: one.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                    slug,
                },
            ))
        })
        .collect();
    list.sort_by_key(|one| one.0);
    list.into_iter().map(|one| one.1).collect()
}

fn configured_model(app: &AppHandle) -> Option<String> {
    let text = std::fs::read_to_string(codex_home(app)?.join("config.toml")).ok()?;
    text.lines()
        .map(str::trim)
        .take_while(|line| !line.starts_with('['))
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "model").then(|| value.trim().trim_matches(['"', '\'']).to_string())
        })
}

fn usable_model(app: &AppHandle, chosen: &str) -> String {
    if !chosen.is_empty() {
        return chosen.to_string();
    }
    let list = models(app);
    match (configured_model(app), list.first()) {
        (Some(set), _) if list.iter().any(|one| one.slug == set) => String::new(),
        (_, Some(best)) => best.slug.clone(),
        _ => String::new(),
    }
}

pub fn logged_in(app: &AppHandle) -> bool {
    codex_home(app).is_some_and(|dir| dir.join("auth.json").is_file())
}

fn command_for(program: &Path) -> std::process::Command {
    if program.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("cmd")) {
        let mut one = crate::hidden_command("cmd");
        one.arg("/c").arg(program);
        one
    } else {
        crate::hidden_command(program)
    }
}

pub fn login(app: &AppHandle) -> Result<(), String> {
    let codex = find_codex(app).ok_or("Codex is not installed on this computer. Install it as shown, then try again.")?;
    let mut command = command_for(&codex);
    with_home(app, &mut command);
    command.arg("login");
    let before = codex_home(app).and_then(|dir| dir.join("auth.json").metadata().ok()?.modified().ok());
    crate::claude_accounts::drive_login(app, command, &|| {
        let now = codex_home(app).and_then(|dir| dir.join("auth.json").metadata().ok()?.modified().ok());
        now.is_some() && now != before
    })
}

fn emit(app: &AppHandle, id: &str, line: String, exited: bool) {
    let _ = app.emit("claude-chat", ChatLine { id: id.to_string(), line, exited });
}

fn store_file(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|error| error.to_string())?.join(name))
}

fn threads(app: &AppHandle) -> HashMap<String, String> {
    store_file(app, "codex-threads-app.json")
        .ok()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn set_thread(app: &AppHandle, server: &str, thread: Option<&str>) {
    let mut all = threads(app);
    match thread {
        Some(one) => all.insert(server.to_string(), one.to_string()),
        None => all.remove(server),
    };
    if let (Ok(file), Ok(text)) = (store_file(app, "codex-threads-app.json"), serde_json::to_string_pretty(&all)) {
        let _ = std::fs::write(file, text);
    }
}

fn transcript(app: &AppHandle, server: &str) -> Result<PathBuf, String> {
    Ok(crate::instance_data_dir(app, server)?.join("codex-chat.jsonl"))
}

fn remember(app: &AppHandle, server: &str, line: &str) {
    let Ok(file) = transcript(app, server) else { return };
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(out, "{line}");
    }
}

fn sandbox(mode: &str) -> Result<&'static str, String> {
    match mode {
        "" | "default" => Ok("read-only"),
        "acceptEdits" => Ok("workspace-write"),
        "bypassPermissions" => Ok("danger-full-access"),
        _ => Err("Unknown permission mode.".into()),
    }
}

fn model_name(model: &str) -> Result<String, String> {
    let model = model.trim();
    if model.is_empty() || model == "default" {
        return Ok(String::new());
    }
    if model.len() < 64 && model.chars().all(|one| one.is_ascii_alphanumeric() || matches!(one, '-' | '.' | '_')) {
        Ok(model.to_string())
    } else {
        Err("Unknown model.".into())
    }
}

pub fn state(server: &str) -> ChatState {
    let all = sessions().lock().expect("codex poisoned");
    let id = all.iter().find(|(_, one)| one.server == server).map(|(id, _)| id.clone()).unwrap_or_default();
    ChatState { id, pending: Vec::new() }
}

pub fn open(app: &AppHandle, server: &str, fresh: bool, mode: &str, model: &str) -> Result<String, String> {
    sandbox(mode)?;
    let model = model_name(model)?;
    find_codex(app).ok_or("Codex is not installed on this computer. Install it as shown, then try again.")?;
    claude_agent::workspace(app, server)?;
    close_server(server);
    crate::claude_chat::close_server(server);
    if fresh {
        set_thread(app, server, None);
        if let Ok(file) = transcript(app, server) {
            let _ = std::fs::remove_file(file);
        }
    }
    let id = format!(
        "codex{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_millis()).unwrap_or(0)
    );
    sessions().lock().expect("codex poisoned").insert(
        id.clone(),
        Session { server: server.to_string(), mode: mode.to_string(), model, turn: None },
    );
    Ok(id)
}

pub fn set_model(id: &str, model: &str) -> Result<(), String> {
    let model = model_name(model)?;
    let mut all = sessions().lock().expect("codex poisoned");
    all.get_mut(id).ok_or("The conversation has ended. Please open it again.")?.model = model;
    Ok(())
}

pub fn set_mode(id: &str, mode: &str) -> Result<(), String> {
    sandbox(mode)?;
    let mut all = sessions().lock().expect("codex poisoned");
    all.get_mut(id).ok_or("The conversation has ended. Please open it again.")?.mode = mode.to_string();
    Ok(())
}

fn toml_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

/// Splits what the chat screen sends (a string, or Claude-style content blocks) into text and pictures.
fn prompt_parts(app: &AppHandle, server: &str, content: &Value) -> Result<(String, Vec<PathBuf>), String> {
    use base64::Engine;
    if let Some(text) = content.as_str() {
        return Ok((text.to_string(), Vec::new()));
    }
    let mut words = Vec::new();
    let mut images = Vec::new();
    let workspace = claude_agent::workspace(app, server)?;
    for block in content.as_array().ok_or("The content to send is invalid.")? {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => words.push(block.get("text").and_then(Value::as_str).unwrap_or("").to_string()),
            Some("image") => {
                let source = block.get("source").cloned().unwrap_or_default();
                let data = source.get("data").and_then(Value::as_str).unwrap_or("");
                let kind = source.get("media_type").and_then(Value::as_str).unwrap_or("image/png");
                let ext = kind.rsplit('/').next().unwrap_or("png");
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data.as_bytes())
                    .map_err(|_| "Could not read the attached picture.".to_string())?;
                let saved = claude_agent::attach(app, server, &format!("image.{ext}"), &bytes)?;
                images.push(workspace.join(saved));
            }
            _ => {}
        }
    }
    Ok((words.join("\n"), images))
}

pub fn send(app: &AppHandle, id: &str, content: &str) -> Result<(), String> {
    let content: Value = serde_json::from_str(content).map_err(|error| format!("The content to send is invalid: {error}"))?;
    let (server, mode, model) = {
        let all = sessions().lock().expect("codex poisoned");
        let one = all.get(id).ok_or("The conversation has ended. Please open it again.")?;
        if one.turn.is_some() {
            return Err("Codex is still answering. Send it when it finishes.".into());
        }
        (one.server.clone(), one.mode.clone(), one.model.clone())
    };
    let codex = find_codex(app).ok_or("Codex is not installed on this computer. Install it as shown, then try again.")?;
    let instance = crate::instance_by_id(app, &server)?;
    let dir = claude_agent::workspace(app, &server)?;
    let (url, token) = crate::mcp::ensure_running(app)?;
    let (prompt, images) = prompt_parts(app, &server, &content)?;
    let thread = threads(app).get(&server).cloned();

    let mut command = command_for(&codex);
    with_home(app, &mut command);
    command.args(["exec", "--json", "--skip-git-repo-check"]);
    command.arg("-s").arg(sandbox(&mode)?);
    command.arg("-c").arg("approval_policy=\"never\"");
    let model = usable_model(app, &model);
    if !model.is_empty() {
        command.arg("-m").arg(&model);
    }
    command.arg("-c").arg(format!("mcp_servers.monolab.url={}", toml_string(&format!("{url}?server={server}"))));
    command.arg("-c").arg(format!("mcp_servers.monolab.bearer_token_env_var={}", toml_string(TOKEN_VAR)));
    command.arg("-c").arg("mcp_servers.monolab.default_tools_approval_mode=\"approve\"");
    command.arg("-c").arg(format!("developer_instructions={}", toml_string(&claude_agent::briefing(&instance, &dir))));
    for image in &images {
        command.arg("-i").arg(image);
    }
    if let Some(thread) = &thread {
        command.arg("resume").arg(thread);
    }
    command.arg("-");
    command.current_dir(&dir).env(TOKEN_VAR, token);
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| format!("Could not start Codex: {error}"))?;
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(prompt.as_bytes());
    }
    let stdout = child.stdout.take().ok_or("Could not read Codex output.")?;
    let stderr = child.stderr.take();

    let user = json!({ "type": "user", "message": { "role": "user", "content": content } }).to_string();
    remember(app, &server, &user);
    emit(app, id, user, false);
    if let Some(one) = sessions().lock().expect("codex poisoned").get_mut(id) {
        one.turn = Some(child);
    }

    let errors = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
    if let Some(stderr) = stderr {
        let errors = errors.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let text = line.trim().to_string();
                if !text.is_empty() && !text.starts_with("Reading additional input") {
                    errors.lock().expect("codex poisoned").push(text);
                }
            }
        });
    }

    let app = app.clone();
    let key = id.to_string();
    std::thread::spawn(move || {
        let mut finished = false;
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
            if event.get("type").and_then(Value::as_str) == Some("thread.started") {
                if let Some(thread) = event.get("thread_id").and_then(Value::as_str) {
                    set_thread(&app, &server, Some(thread));
                }
            }
            for out in translate(&event) {
                if out.get("type").and_then(Value::as_str) == Some("result") {
                    finished = true;
                }
                let text = out.to_string();
                if !matches!(out.get("type").and_then(Value::as_str), Some("stream_event")) {
                    remember(&app, &server, &text);
                }
                emit(&app, &key, text, false);
            }
        }
        let child = sessions().lock().expect("codex poisoned").get_mut(&key).and_then(|one| one.turn.take());
        let failed = child.map(|mut one| one.wait().map(|status| !status.success()).unwrap_or(true)).unwrap_or(false);
        if !finished {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let said = errors.lock().expect("codex poisoned").join("\n");
            let result = if failed && !said.is_empty() {
                json!({ "type": "result", "subtype": "error", "is_error": true, "result": said })
            } else {
                json!({ "type": "result", "subtype": "success" })
            };
            emit(&app, &key, result.to_string(), false);
        }
    });
    Ok(())
}

fn tool_use(id: &str, name: &str, input: Value) -> Value {
    json!({ "type": "assistant", "message": { "role": "assistant", "content": [{ "type": "tool_use", "id": id, "name": name, "input": input }] } })
}

fn tool_result(id: &str, content: &str, failed: bool) -> Value {
    json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": id, "content": content, "is_error": failed }] } })
}

fn mcp_text(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| blocks.iter().filter_map(|one| one.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

/// Turns one `codex exec --json` event into the Claude stream-json lines the chat screen already draws.
fn translate(event: &Value) -> Vec<Value> {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
    let item = event.get("item").cloned().unwrap_or_default();
    let item_id = item.get("id").and_then(Value::as_str).unwrap_or("item");
    let item_kind = item.get("type").and_then(Value::as_str).unwrap_or("");
    let s = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    match (kind, item_kind) {
        ("thread.started", _) => vec![json!({ "type": "system", "subtype": "init", "model": "Codex" })],
        ("item.completed", "agent_message") => {
            vec![json!({ "type": "assistant", "message": { "role": "assistant", "content": [{ "type": "text", "text": s("text") }] } })]
        }
        ("item.started", "command_execution") => vec![tool_use(item_id, "Bash", json!({ "command": s("command") }))],
        ("item.completed", "command_execution") => {
            let failed = item.get("exit_code").and_then(Value::as_i64).is_some_and(|code| code != 0);
            vec![
                tool_use(item_id, "Bash", json!({ "command": s("command") })),
                tool_result(item_id, &s("aggregated_output"), failed),
            ]
        }
        ("item.started", "mcp_tool_call") => {
            let name = format!("mcp__{}__{}", s("server"), s("tool"));
            vec![tool_use(item_id, &name, item.get("arguments").cloned().unwrap_or_else(|| json!({})))]
        }
        ("item.completed", "mcp_tool_call") => {
            let name = format!("mcp__{}__{}", s("server"), s("tool"));
            let error = item.get("error").and_then(|one| one.get("message").and_then(Value::as_str).or(one.as_str())).unwrap_or("");
            let output = item.get("result").map(mcp_text).unwrap_or_default();
            let flagged = item
                .get("result")
                .is_some_and(|result| [result.get("isError"), result.get("is_error")].into_iter().flatten().any(|one| one.as_bool() == Some(true)));
            vec![
                tool_use(item_id, &name, item.get("arguments").cloned().unwrap_or_else(|| json!({}))),
                tool_result(item_id, if error.is_empty() { &output } else { error }, flagged || !error.is_empty() || s("status") == "failed"),
            ]
        }
        ("item.completed", "file_change") => {
            let files: Vec<String> = item
                .get("changes")
                .and_then(Value::as_array)
                .map(|all| {
                    all.iter()
                        .map(|one| format!("{} {}", one.get("kind").and_then(Value::as_str).unwrap_or("update"), one.get("path").and_then(Value::as_str).unwrap_or("")))
                        .collect()
                })
                .unwrap_or_default();
            let failed = s("status") == "failed";
            vec![
                tool_use(item_id, "Edit", json!({ "file_path": files.first().cloned().unwrap_or_default() })),
                tool_result(item_id, &files.join("\n"), failed),
            ]
        }
        ("item.completed", "web_search") => {
            vec![tool_use(item_id, "WebSearch", json!({ "query": s("query") })), tool_result(item_id, "", false)]
        }
        ("item.completed", "error") if s("message").contains("is not advertised as supported") => Vec::new(),
        ("item.completed", "error") => vec![json!({ "type": "monolab_stderr", "text": s("message") })],
        ("turn.completed", _) => vec![json!({ "type": "result", "subtype": "success" })],
        ("turn.failed", _) => {
            let said = event.get("error").and_then(|one| one.get("message")).and_then(Value::as_str).unwrap_or("Codex stopped.");
            vec![json!({ "type": "result", "subtype": "error", "is_error": true, "result": said })]
        }
        ("error", _) => {
            let said = event.get("message").and_then(Value::as_str).unwrap_or("");
            if said.is_empty() {
                Vec::new()
            } else {
                vec![json!({ "type": "monolab_stderr", "text": said })]
            }
        }
        _ => Vec::new(),
    }
}

pub fn interrupt(app: &AppHandle, id: &str) -> Result<(), String> {
    let mut all = sessions().lock().expect("codex poisoned");
    let one = all.get_mut(id).ok_or("The conversation has ended. Please open it again.")?;
    if let Some(child) = one.turn.as_mut() {
        crate::kill_process_tree(child.id());
        let _ = child.kill();
    }
    drop(all);
    emit(app, id, json!({ "type": "result", "subtype": "success" }).to_string(), false);
    Ok(())
}

pub fn history(app: &AppHandle, server: &str) -> Result<Vec<String>, String> {
    let Ok(text) = std::fs::read_to_string(transcript(app, server)?) else { return Ok(Vec::new()) };
    let lines: Vec<String> = text.lines().filter(|line| !line.trim().is_empty()).map(str::to_string).collect();
    let start = lines.len().saturating_sub(HISTORY_LINES);
    Ok(lines[start..].to_vec())
}

pub fn has_history(app: &AppHandle, server: &str) -> bool {
    threads(app).contains_key(server)
}

pub fn close(id: &str) {
    if let Some(mut one) = sessions().lock().expect("codex poisoned").remove(id) {
        if let Some(child) = one.turn.as_mut() {
            crate::kill_process_tree(child.id());
            let _ = child.kill();
        }
    }
}

pub fn close_server(server: &str) {
    let ids: Vec<String> = sessions()
        .lock()
        .expect("codex poisoned")
        .iter()
        .filter(|(_, one)| one.server == server)
        .map(|(id, _)| id.clone())
        .collect();
    for id in ids {
        close(&id);
    }
}

pub fn close_all() {
    let ids: Vec<String> = sessions().lock().expect("codex poisoned").keys().cloned().collect();
    for id in ids {
        close(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_codex_events_into_chat_lines() {
        let message = translate(&json!({ "type": "item.completed", "item": { "id": "item_0", "type": "agent_message", "text": "hi" } }));
        assert_eq!(message[0]["message"]["content"][0]["text"], "hi");

        let run = translate(&json!({ "type": "item.completed", "item": {
            "id": "item_1", "type": "command_execution", "command": "echo hello", "aggregated_output": "hello", "exit_code": 1 } }));
        assert_eq!(run[0]["message"]["content"][0]["name"], "Bash");
        assert_eq!(run[1]["message"]["content"][0]["tool_use_id"], "item_1");
        assert_eq!(run[1]["message"]["content"][0]["is_error"], true);

        let tool = translate(&json!({ "type": "item.completed", "item": {
            "id": "item_2", "type": "mcp_tool_call", "server": "monolab", "tool": "read_file", "arguments": { "path": "a" },
            "result": { "content": [{ "type": "text", "text": "body" }] } } }));
        assert_eq!(tool[0]["message"]["content"][0]["name"], "mcp__monolab__read_file");
        assert_eq!(tool[1]["message"]["content"][0]["content"], "body");

        assert_eq!(translate(&json!({ "type": "turn.completed", "usage": {} }))[0]["type"], "result");
        assert_eq!(translate(&json!({ "type": "turn.failed", "error": { "message": "nope" } }))[0]["result"], "nope");
        assert!(translate(&json!({ "type": "item.completed", "item": { "type": "reasoning", "text": "x" } })).is_empty());
    }

    #[test]
    fn maps_modes_to_sandboxes() {
        assert_eq!(sandbox("default").unwrap(), "read-only");
        assert_eq!(sandbox("acceptEdits").unwrap(), "workspace-write");
        assert_eq!(sandbox("bypassPermissions").unwrap(), "danger-full-access");
        assert!(sandbox("rm -rf").is_err());
        assert!(model_name("gpt-5.5").is_ok());
        assert!(model_name("a b").is_err());
    }
}

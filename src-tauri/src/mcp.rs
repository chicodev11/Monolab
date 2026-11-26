
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

pub const PORT: u16 = 25570;

fn stop_flag() -> &'static Mutex<Option<Arc<AtomicBool>>> {
    static FLAG: OnceLock<Mutex<Option<Arc<AtomicBool>>>> = OnceLock::new();
    FLAG.get_or_init(|| Mutex::new(None))
}

fn last_error() -> &'static Mutex<Option<String>> {
    static ERROR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    ERROR.get_or_init(|| Mutex::new(None))
}

fn token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(crate::web::make_key)
}

pub fn ensure_running(app: &AppHandle) -> Result<(String, String), String> {
    start(app, token());
    if stop_flag().lock().expect("mcp poisoned").is_none() {
        return Err(last_error().lock().expect("mcp poisoned").clone().unwrap_or_else(|| "Could not start the server tools.".into()));
    }
    Ok((format!("http://127.0.0.1:{PORT}/mcp"), token().to_string()))
}

fn start(app: &AppHandle, token: &str) {
    if stop_flag().lock().expect("mcp poisoned").is_some() {
        return;
    }
    let listener = match TcpListener::bind(("127.0.0.1", PORT)) {
        Ok(listener) => listener,
        Err(error) => {
            *last_error().lock().expect("mcp poisoned") =
                Some(format!("Could not open port {PORT} — check whether another program is using it. ({error})"));
            return;
        }
    };
    *last_error().lock().expect("mcp poisoned") = None;
    let flag = Arc::new(AtomicBool::new(false));
    *stop_flag().lock().expect("mcp poisoned") = Some(flag.clone());
    let app = app.clone();
    let token = token.to_string();
    let busy = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if flag.load(Ordering::Relaxed) {
                break;
            }
            let Ok(stream) = stream else { continue };
            if busy.load(Ordering::Relaxed) >= 8 {
                continue;
            }
            busy.fetch_add(1, Ordering::Relaxed);
            let (app, token, busy) = (app.clone(), token.clone(), busy.clone());
            std::thread::spawn(move || {
                let _ = serve(&app, &token, stream);
                busy.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
}

fn reply(stream: &mut TcpStream, status: u16, kind: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Method Not Allowed",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn serve(app: &AppHandle, token: &str, mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let (mut length, mut auth, mut origin) = (0_usize, String::new(), String::new());
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim().to_string();
        match name.to_ascii_lowercase().as_str() {
            "content-length" => length = value.parse().unwrap_or(0),
            "authorization" => auth = value,
            "origin" => origin = value,
            _ => {}
        }
    }
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default();
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let path = path.to_string();
    let locked: Option<String> = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("server="))
        .map(str::to_string)
        .filter(|id| !id.is_empty());
    if path != "/mcp" {
        return reply(&mut stream, 404, "text/plain", b"not found");
    }
    let local = |origin: &str| {
        ["http://127.0.0.1", "http://localhost", "https://127.0.0.1", "https://localhost"]
            .iter()
            .any(|base| origin == *base || origin.starts_with(&format!("{base}:")))
    };
    if !origin.is_empty() && !local(&origin) {
        return reply(&mut stream, 403, "text/plain", b"origin");
    }
    if !same(auth.strip_prefix("Bearer ").unwrap_or_default().trim(), token) {
        return reply(&mut stream, 401, "application/json", br#"{"error":"unauthorized"}"#);
    }
    if method != "POST" {
        return reply(&mut stream, 405, "text/plain", b"method");
    }
    if length > 16 * 1024 * 1024 {
        return reply(&mut stream, 400, "text/plain", b"too large");
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    let Ok(message) = serde_json::from_slice::<Value>(&body) else {
        let error = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}});
        return reply(&mut stream, 400, "application/json", error.to_string().as_bytes());
    };
    let answers: Vec<Value> = match &message {
        Value::Array(list) => list.iter().filter_map(|one| handle(app, one, locked.as_deref())).collect(),
        one => handle(app, one, locked.as_deref()).into_iter().collect(),
    };
    if answers.is_empty() {
        return reply(&mut stream, 202, "application/json", b"");
    }
    let out = if message.is_array() { Value::Array(answers) } else { answers.into_iter().next().unwrap_or(Value::Null) };
    reply(&mut stream, 200, "application/json", out.to_string().as_bytes())
}

const INSTRUCTIONS: &str = "Monolab is the user's Minecraft server manager app. These tools act on servers registered in it \
(local, SSH home server, or Hydrodactyl panel). Start with list_servers to get ids. Before changing things \
(starting/stopping, writing files, installing or removing mods/plugins, changing server.properties) tell the user \
what you are about to do and why. For crashes read list_crash_reports / read_console / search_logs first. \
Paths are relative to the server folder and cannot leave it. \
live_settings / set_gamerule change a running server without a restart; rewind_history / rewind_restore undo file changes. \
Plugin servers (Paper/Purpur/Folia) take plugins, Fabric/NeoForge/Forge take mods.";

/// Tools that act on this PC's own copy of a server, so they can't reach a teammate's server.
const LOCAL_ONLY: &[&str] = &["read_memo", "update_todo", "list_core_versions", "create_server"];

/// Tools that work across servers: a server-locked agent doesn't get them.
const APP_WIDE: &[&str] = &["list_servers", "list_core_versions", "create_server"];

thread_local! {
    /// The shared server (team reference) this thread's tool call is for, if any.
    static ON_HOST: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

fn on_host<T>(server: &str, run: impl FnOnce() -> T) -> T {
    ON_HOST.with(|one| *one.borrow_mut() = Some(server.to_string()));
    let out = run();
    ON_HOST.with(|one| *one.borrow_mut() = None);
    out
}

fn handle(app: &AppHandle, message: &Value, locked: Option<&str>) -> Option<Value> {
    let id = message.get("id").cloned()?;
    // An agent for a server shared through a Monolink team (`team_agent`).
    let shared = locked.and_then(crate::team_agent::parse);
    let method = message.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18");
            Ok(json!({
                "protocolVersion": asked,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "monolab", "version": env!("CARGO_PKG_VERSION") },
                "instructions": match locked {
                    Some(_) => "Tools act on this agent's one Minecraft server.".to_string(),
                    None => INSTRUCTIONS.to_string(),
                },
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_list(locked.is_some()).into_iter().filter(|tool| !(shared.is_some() && LOCAL_ONLY.contains(&tool["name"].as_str().unwrap_or_default()))).collect::<Vec<_>>() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let ran = match &shared {
                Some(_) if LOCAL_ONLY.contains(&name) => Err(format!("`{name}` doesn't work on a teammate's server.")),
                // Every request of this call goes to the host, with this user's team role.
                Some(target) => on_host(locked.unwrap_or_default(), || call(app, name, &args, Some(&target.instance))),
                None => call(app, name, &args, locked),
            };
            Ok(match ran {
                Ok(Value::String(data)) if name == "studio_view_texture" && data.starts_with("data:image/png;base64,") => {
                    json!({ "content": [{ "type": "image", "mimeType": "image/png", "data": &data["data:image/png;base64,".len()..] }] })
                }
                Ok(value) => json!({ "content": [{ "type": "text", "text": clip(&pretty(value)) }] }),
                Err(error) => json!({ "content": [{ "type": "text", "text": error }], "isError": true }),
            })
        }
        _ => Err(json!({ "code": -32601, "message": format!("unknown method {method}") })),
    };
    Some(match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
    })
}

fn pretty(value: Value) -> String {
    match value {
        Value::String(text) => text,
        other => serde_json::to_string(&slim(other)).unwrap_or_default(),
    }
}

fn cursors() -> &'static Mutex<std::collections::HashMap<String, String>> {
    static SEEN: OnceLock<Mutex<std::collections::HashMap<String, String>>> = OnceLock::new();
    SEEN.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn shorten(line: &str, max: usize) -> String {
    if line.chars().count() <= max {
        return line.to_string();
    }
    let cut: String = line.chars().take(max).collect();
    format!("{cut}…")
}

fn squeeze(lines: &[String], max: usize) -> Vec<String> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for line in lines {
        let line = shorten(line, max);
        match out.last_mut() {
            Some((last, count)) if *last == line => *count += 1,
            _ => out.push((line, 1)),
        }
    }
    out.into_iter().map(|(line, count)| if count > 1 { format!("{line}  (x{count})") } else { line }).collect()
}

fn console_lines(app: &AppHandle, id: &str) -> Result<Vec<(String, bool)>, String> {
    let all = invoke(app, "get_console_logs", json!({ "id": id }))?;
    Ok(all
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|one| {
            let line = one.get("line").and_then(Value::as_str)?.to_string();
            Some((line, one.get("noise").and_then(Value::as_bool).unwrap_or(false)))
        })
        .collect())
}

fn mark_read(id: &str, lines: &[(String, bool)]) {
    if let (Ok(mut marks), Some(last)) = (cursors().lock(), lines.last()) {
        marks.insert(id.to_string(), last.0.clone());
    }
}

fn after_mark(lines: &[(String, bool)], mark: Option<&str>) -> usize {
    mark.and_then(|last| lines.iter().rposition(|(line, _)| line == last)).map(|at| at + 1).unwrap_or(0)
}

fn is_error(line: &str) -> bool {
    line.contains("ERROR") || line.contains("FATAL") || line.contains("Exception")
}

fn is_warning(line: &str) -> bool {
    line.contains("WARN")
}

fn wait_ready(app: &AppHandle, id: &str, before: Option<String>) -> String {
    let began = std::time::Instant::now();
    let limit = Duration::from_secs(300);
    // On a teammate's shared server each check is a call to the host (and a line in its teammate log): ask
    // less often, and give up once the host stops answering instead of failing every few seconds for minutes.
    let shared = ON_HOST.with(|one| one.borrow().is_some());
    let mut failed = 0;
    let outcome = loop {
        std::thread::sleep(poll_gap(shared, began.elapsed()));
        let list = match invoke(app, "list_instances", json!({})) {
            Ok(list) => {
                failed = 0;
                list
            }
            Err(error) => {
                failed += 1;
                if shared && failed >= 3 {
                    break format!("unknown: the host stopped answering ({})", shorten(&error, 160));
                }
                Value::Null
            }
        };
        let view = list.as_array().and_then(|all| all.iter().find(|one| one.get("id").and_then(Value::as_str) == Some(id)).cloned());
        let status = view.as_ref().and_then(|one| one.get("status")).and_then(Value::as_str).unwrap_or("").to_string();
        let ready = view.as_ref().and_then(|one| one.get("ready")).and_then(Value::as_bool).unwrap_or(false);
        if status == "running" && ready {
            break "ready".to_string();
        }
        if status == "crashed" {
            let why = view.as_ref().and_then(|one| one.get("crashReason")).and_then(Value::as_str).unwrap_or("").to_string();
            break if why.is_empty() { "crashed".to_string() } else { format!("crashed ({})", shorten(&why, 200)) };
        }
        if status == "stopped" && began.elapsed() > Duration::from_secs(20) {
            break "stopped".to_string();
        }
        if began.elapsed() > limit {
            break format!("still starting after {} s", limit.as_secs());
        }
    };
    let secs = began.elapsed().as_secs();
    let lines = console_lines(app, id).unwrap_or_default();
    let from = after_mark(&lines, before.as_deref());
    let fresh: Vec<&str> = lines[from..].iter().filter(|(_, noise)| !noise).map(|(line, _)| line.as_str()).collect();
    mark_read(id, &lines);
    let errors: Vec<String> = fresh.iter().filter(|line| is_error(line)).map(|line| line.to_string()).collect();
    let warnings: Vec<String> = fresh.iter().filter(|line| is_warning(line)).map(|line| line.to_string()).collect();
    let mut out = format!("Server is {outcome} after about {secs} s. New console lines: {}, errors: {}, warnings: {}.", fresh.len(), errors.len(), warnings.len());
    if let Some(done) = fresh.iter().rev().find(|line| line.contains("Done (")) {
        out.push_str(&format!("\n{}", shorten(done, 200)));
    }
    if !errors.is_empty() {
        out.push_str("\nLast errors:");
        for line in squeeze(&errors[errors.len().saturating_sub(8)..], 240) {
            out.push_str(&format!("\n{line}"));
        }
    } else if !warnings.is_empty() {
        out.push_str("\nLast warnings:");
        for line in squeeze(&warnings[warnings.len().saturating_sub(4)..], 240) {
            out.push_str(&format!("\n{line}"));
        }
    }
    if outcome != "ready" {
        out.push_str("\nLast lines:");
        for line in squeeze(&fresh[fresh.len().saturating_sub(12)..].iter().map(|line| line.to_string()).collect::<Vec<_>>(), 240) {
            out.push_str(&format!("\n{line}"));
        }
    }
    out.push_str("\nThis summary already covers startup - only call read_console or search_logs if you need more detail.");
    out
}

/// Biggest plugin file an agent sends to a teammate's host, and the piece size (the host takes up to 4 MB).
const HOST_UPLOAD_MAX: u64 = 256 * 1024 * 1024;
const HOST_UPLOAD_PART: usize = 1024 * 1024;

/// Sends a file on this PC to the host of the shared server this call is for, in pieces (`__upload_*`, the
/// same steps a teammate's window uses), and gives the host's name for the upload.
fn upload_to_host(app: &AppHandle, path: &std::path::Path) -> Result<String, String> {
    use base64::Engine;
    let name = path.file_name().and_then(|one| one.to_str()).ok_or("Could not read the file name.")?.to_string();
    let size = std::fs::metadata(path).map_err(|error| error.to_string())?.len();
    if size > HOST_UPLOAD_MAX {
        return Err(format!("{name} is larger than 256 MB; too big to send to the host."));
    }
    let mut file = std::fs::File::open(path).map_err(|error| format!("Could not read {name}: {error}"))?;
    let begun = invoke(app, "__upload_begin", json!({ "name": name, "size": size }))?;
    let upload = begun.get("upload").and_then(Value::as_str).ok_or("The host didn't open the upload.")?.to_string();
    let mut piece = vec![0u8; HOST_UPLOAD_PART];
    let mut at = 0u64;
    loop {
        let read = file.read(&mut piece).map_err(|error| format!("Could not read {name}: {error}"))?;
        if read == 0 {
            break;
        }
        let data = base64::engine::general_purpose::STANDARD.encode(&piece[..read]);
        invoke(app, "__upload_part", json!({ "upload": upload, "at": at, "data": data }))?;
        at += read as u64;
    }
    invoke(app, "__upload_end", json!({ "upload": upload }))?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "The host didn't take the file.".to_string())
}

/// How long to wait before the next "is it up yet?" check: every 2 s on this PC; on a shared server 5 s, then
/// 10 s after the first half minute and 15 s after two minutes.
fn poll_gap(shared: bool, waited: Duration) -> Duration {
    if !shared {
        return Duration::from_secs(2);
    }
    Duration::from_secs(match waited.as_secs() {
        0..30 => 5,
        30..120 => 10,
        _ => 15,
    })
}

fn last_line(app: &AppHandle, id: &str) -> Option<String> {
    console_lines(app, id).ok().and_then(|lines| lines.last().map(|(line, _)| line.clone()))
}

fn human_size(bytes: u64) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1_048_575 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn span_of(list: Option<&Value>) -> Value {
    let all: Vec<&str> = list.and_then(Value::as_array).map(|one| one.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    match all.len() {
        0 => Value::Null,
        1..=3 => Value::String(all.join(", ")),
        n => Value::String(format!("{} ~ {} ({n} versions)", all[0], all[n - 1])),
    }
}

fn clip(text: &str) -> String {
    const MAX: usize = 24_000;
    if text.len() <= MAX {
        return text.to_string();
    }
    let mut cut = MAX;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n… (truncated: {} characters total)", &text[..cut], text.len())
}

/// Lines start..start+count (from 1) under a header, so a big file can be read in parts.
fn line_window(content: &str, start: usize, count: Option<usize>) -> String {
    let all: Vec<&str> = content.lines().collect();
    let from = start.clamp(1, all.len().max(1)) - 1;
    let to = count.map_or(all.len(), |count| (from + count.max(1)).min(all.len()));
    format!("(lines {}-{} of {})\n{}", from + 1, to, all.len(), all[from..to].join("\n"))
}

/// Agents send \n even when the file uses CRLF, so their text is matched in the file's own line breaks.
fn replace_once(content: &str, old: &str, new: &str) -> Result<String, String> {
    let crlf = content.contains("\r\n") && old.contains('\n') && !old.contains("\r\n");
    let (old, new) = if crlf {
        (old.replace('\n', "\r\n"), new.replace("\r\n", "\n").replace('\n', "\r\n"))
    } else {
        (old.to_string(), new.to_string())
    };
    match content.matches(old.as_str()).count() {
        1 => Ok(content.replacen(old.as_str(), &new, 1)),
        0 => Err("oldText was not found. Read the file again and copy the text exactly.".into()),
        many => Err(format!("oldText appears {many} times. Include more surrounding lines so it appears once.")),
    }
}

/// A tool's `value` as the JSON text MSMP takes: numbers and true/false stay, a word gets quotes.
fn json_text(value: Option<&Value>) -> Result<String, String> {
    match value {
        None | Some(Value::Null) => Err("`value` is missing.".into()),
        Some(Value::String(word)) => {
            let word = word.trim();
            if word.is_empty() {
                return Err("`value` is missing.".into());
            }
            Ok(match serde_json::from_str::<Value>(word) {
                Ok(parsed) if !parsed.is_object() && !parsed.is_array() => parsed.to_string(),
                _ => Value::String(word.to_string()).to_string(),
            })
        }
        Some(other) => Ok(other.to_string()),
    }
}

fn field_of(one: &Value, key: &str) -> String {
    match one.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// "5 min ago" for a Unix second.
fn ago(at: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_secs()).unwrap_or(at);
    match now.saturating_sub(at) {
        0..60 => "just now".into(),
        secs @ 60..3600 => format!("{} min ago", secs / 60),
        secs @ 3600..86_400 => format!("{} h ago", secs / 3600),
        secs => format!("{} d ago", secs / 86_400),
    }
}

fn slim(value: Value) -> Value {
    match value {
        Value::String(text) if text.starts_with("data:") => Value::String("[image]".into()),
        Value::Array(list) => Value::Array(list.into_iter().map(slim).collect()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(key, value)| (key, slim(value))).collect()),
        other => other,
    }
}

struct Tool {
    name: &'static str,
    description: &'static str,
    args: &'static [(&'static str, &'static str, &'static str, bool)],
    read_only: bool,
    destructive: bool,
}

/// The studio's asset folders (`studio_pack::check_upload`).
const STUDIO_FOLDERS: [&str; 3] = ["textures", "models", "sounds"];

const SERVER: (&str, &str, &str, bool) = ("server", "string", "Server id or name from list_servers (optional when only one server exists).", false);

const TOOLS: &[Tool] = &[
    Tool { name: "list_core_versions", description: "Minecraft versions a new server can use for one core, newest first. Cores: Vanilla, Paper, Purpur, Leaf, Folia, Fabric, Forge, NeoForge, Velocity, Pumpkin.", args: &[("core", "string", "Core name, e.g. Paper.", true), ("snapshots", "boolean", "Include snapshots (Vanilla/Fabric only, default false).", false)], read_only: true, destructive: false },
    Tool { name: "create_server", description: "Make a new server on this computer: downloads the core, accepts the Minecraft EULA (https://aka.ms/MinecraftEULA) and picks a free port. Before calling, tell the user the core, version and name and that this accepts the EULA for them, and only go ahead once they agree. It is not started; call start_server afterwards if wanted. Takes a while for big downloads.", args: &[("core", "string", "Core name from list_core_versions (Paper, Fabric, Velocity…).", true), ("version", "string", "Minecraft version from list_core_versions (default: newest).", false), ("name", "string", "Server name (default: the next free \"Server N\").", false), ("memoryMb", "integer", "Most memory in MB (512-65536, default 4096).", false), ("eulaAccepted", "boolean", "true once the user agreed to the Minecraft EULA.", true)], read_only: false, destructive: false },
    Tool { name: "list_servers", description: "List all servers registered in Monolab with id, core, version, status, port and where they run.", args: &[], read_only: true, destructive: false },
    Tool { name: "server_status", description: "Status of one server: running state, CPU, memory, TPS and online players when available.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "read_console", description: "New live console lines since your last read_console call (or since start_server/restart_server summarized startup). Harmless noise is hidden, repeats are folded and long lines shortened. Do not poll this in a loop - start_server and restart_server already wait for the server and summarize.", args: &[SERVER, ("lines", "integer", "Most lines to return (default 40, max 500).", false), ("all", "boolean", "true = the last lines regardless of what you already read.", false)], read_only: true, destructive: false },
    Tool { name: "list_log_files", description: "List log files (latest.log and older .log.gz) of a server.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "search_logs", description: "Read or search a log file. Empty query returns recent entries.", args: &[SERVER, ("query", "string", "Text to search for (optional).", false), ("file", "string", "Log file name, default latest.log.", false), ("level", "string", "Filter: All (default), Error, Warning.", false), ("limit", "integer", "Most matching lines to return, newest last (default 50, max 300).", false)], read_only: true, destructive: false },
    Tool { name: "list_crash_reports", description: "Crash reports with description, exception and suspected mods/plugins.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "diagnose_lag", description: "Lag diagnosis: TPS, entity hotspots, memory, view distance hints (best with the Monolab addon installed).", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "precheck", description: "Check common start problems before starting: server jar, EULA, port in use, Java version, memory.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "list_files", description: "List a folder inside the server folder, one line per entry (folders end with /).", args: &[SERVER, ("path", "string", "Folder path relative to the server folder, empty for the top.", false)], read_only: true, destructive: false },
    Tool { name: "read_file", description: "Read a text file (config, yml, json, properties, toml, txt…) inside the server folder. For a big file, read only the part you need with start and lines.", args: &[SERVER, ("path", "string", "File path relative to the server folder.", true), ("start", "integer", "First line to return, from 1 (optional).", false), ("lines", "integer", "How many lines to return from start (optional).", false)], read_only: true, destructive: false },
    Tool { name: "edit_file", description: "Change part of a text file inside the server folder: oldText (copied exactly, with enough surrounding lines to appear only once) becomes newText. Use this instead of write_file for small changes.", args: &[SERVER, ("path", "string", "File path relative to the server folder.", true), ("oldText", "string", "Exact text to replace.", true), ("newText", "string", "Text to put in its place (empty removes it).", true)], read_only: false, destructive: true },
    Tool { name: "write_file", description: "Write a text file inside the server folder (creates it if missing, replaces the whole content). Read it first and keep what you do not intend to change. For small changes to an existing file use edit_file.", args: &[SERVER, ("path", "string", "File path relative to the server folder.", true), ("content", "string", "Full new file content.", true)], read_only: false, destructive: true },
    Tool { name: "delete_file", description: "Delete a file or folder inside the server folder. It is NOT erased: it goes to the server trash, where the user can restore it from the Files tab. Always use this instead of shell commands (rm, del, Remove-Item) for anything in the server folder.", args: &[SERVER, ("path", "string", "File or folder path relative to the server folder.", true)], read_only: false, destructive: true },
    Tool { name: "make_folder", description: "Create a folder inside the server folder, including any missing parent folders (e.g. world/datapacks/mypack/data/mypack/function). Fine if it already exists.", args: &[SERVER, ("path", "string", "Folder path relative to the server folder.", true)], read_only: false, destructive: false },
    Tool { name: "add_addon_file", description: "Put a mod/plugin file that exists on THIS computer (e.g. a jar you just built with Gradle) into the server's mods or plugins folder. Works for local, SSH and panel servers. If a local server is running it is queued for the next start; SSH/panel servers must be stopped first. Restart the server afterwards so it loads.", args: &[SERVER, ("localPath", "string", "Absolute path of the .jar (or .wasm for Pumpkin) on this computer.", true)], read_only: false, destructive: false },
    Tool { name: "read_properties", description: "Read server.properties (or pumpkin.toml for Pumpkin) as key/value list.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "set_properties", description: "Change some server.properties values; other keys stay as they are. Applied on next start.", args: &[SERVER, ("values", "object", "Map of key to new value, e.g. {\"view-distance\":\"8\"}.", true)], read_only: false, destructive: false },
    Tool { name: "list_addons", description: "Installed mods and plugins, one line each: file name, version, and notes if disabled or not compatible. Use query to narrow it down.", args: &[SERVER, ("query", "string", "Only list files whose name contains this text (optional).", false)], read_only: true, destructive: false },
    Tool { name: "search_modrinth", description: "Search Modrinth for mods/plugins that fit this server's core and Minecraft version.", args: &[SERVER, ("query", "string", "Search words (empty = popular).", false), ("page", "integer", "Page number from 0.", false)], read_only: true, destructive: false },
    Tool { name: "list_addon_versions", description: "Versions of a Modrinth project with which ones fit this server.", args: &[SERVER, ("projectId", "string", "Modrinth project id.", true)], read_only: true, destructive: false },
    Tool { name: "install_addon", description: "Download and install a Modrinth mod/plugin (hash checked). Omit versionId for the newest fitting version. If the server is running it applies on next start.", args: &[SERVER, ("projectId", "string", "Modrinth project id.", true), ("versionId", "string", "Specific version id (optional).", false)], read_only: false, destructive: false },
    Tool { name: "toggle_addon", description: "Enable or disable an installed mod/plugin by file name (server should be stopped).", args: &[SERVER, ("fileName", "string", "File name from list_addons.", true)], read_only: false, destructive: false },
    Tool { name: "delete_addon", description: "Remove an installed mod/plugin (moved to the app's trash, restorable).", args: &[SERVER, ("fileName", "string", "File name from list_addons.", true)], read_only: false, destructive: true },
    Tool { name: "monolab_addon", description: "The Monolab mod/plugin that feeds this app (monitor TPS, loaded chunks, world map, live players). action \"status\" shows whether it is installed and its version vs the newest one bundled with the app; \"install\" installs or updates it to the bundled version (replaces an old one); \"remove\" uninstalls it. Restart the server afterwards so it loads.", args: &[SERVER, ("action", "string", "status, install or remove.", true)], read_only: false, destructive: false },
    Tool { name: "start_server", description: "Start a server and wait until it is ready (or crashes), then return a short startup summary with errors.", args: &[SERVER], read_only: false, destructive: false },
    Tool { name: "stop_server", description: "Stop a server safely (saves the world). Players get disconnected.", args: &[SERVER], read_only: false, destructive: true },
    Tool { name: "restart_server", description: "Restart a server and wait until it is ready again, then return a short startup summary with errors. Players get disconnected.", args: &[SERVER], read_only: false, destructive: true },
    Tool { name: "send_command", description: "Send a console command to a running server (without leading slash), e.g. \"list\" or \"say hello\".", args: &[SERVER, ("command", "string", "Console command.", true)], read_only: false, destructive: true },
    Tool { name: "list_players", description: "Whitelist, ops, bans and known players of a server.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "read_memo", description: "The user's Memo tab for this server: to-dos with ids and done state, and memos with their text. Attached pictures come as file paths you can open to look at. Read it when the user mentions their memo, notes or to-dos.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "update_todo", description: "Change the to-do list (board) in the user's Memo tab: action 'add' with text (and assignee), 'done' / 'open' with a to-do id from read_memo, 'status' with status todo, doing or done, or 'assign' with assignee (@handle, or empty to free it). Move a to-do to doing when you start it and done when you have finished it.", args: &[SERVER, ("action", "string", "add, done, open, status or assign.", true), ("id", "string", "To-do id from read_memo (all but add).", false), ("text", "string", "To-do text (add).", false), ("status", "string", "todo, doing or done (status).", false), ("assignee", "string", "@handle of a teammate (assign, or add).", false)], read_only: false, destructive: false },
    Tool { name: "studio_overview", description: "The server's studio (custom items, mobs, models, menus… that the Monolab add-on runs): every file by kind, whether it is on, and its problems. Read studio/AGENTS.md in the server folder first: it lists every kind and action.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "studio_read", description: "One studio file as JSON, with its hash (pass the hash back to studio_write so you don't overwrite someone else's change).", args: &[SERVER, ("kind", "string", "Kind, e.g. mob, model, item, menu (see studio_overview).", true), ("name", "string", "File name without .json.", true)], read_only: true, destructive: false },
    Tool { name: "studio_write", description: "Save a studio file (the whole JSON). The app checks it the way the add-on will and answers with any problems; fix them and save again. For a new file leave hash empty; for an existing one pass the hash from studio_read. Common order for a mob with a model: studio_upload_asset the .bbmodel (models) → studio_write a model that names it → studio_write the mob with \"model\" set → studio_build_pack.", args: &[SERVER, ("kind", "string", "Kind, e.g. mob.", true), ("name", "string", "File name without .json (a-z, 0-9, - and _).", true), ("json", "string", "The whole file as JSON text.", true), ("hash", "string", "Hash from studio_read; empty for a new file.", false)], read_only: false, destructive: true },
    Tool { name: "studio_new", description: "The starting JSON for a new studio file of a kind (the same template the app's New button uses).", args: &[SERVER, ("kind", "string", "Kind, e.g. mob.", true)], read_only: true, destructive: false },
    Tool { name: "studio_assets", description: "Pictures (textures), models (.bbmodel/.json) and sounds (.ogg) in the studio's assets, and the resource pack's state.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "studio_upload_asset", description: "Put a picture, Blockbench model or sound that is on THIS computer into the studio's assets, then the pack is built again. folder: textures (.png), models (.bbmodel or .json), sounds (.ogg). Names use a-z, 0-9, - and _.", args: &[SERVER, ("localPath", "string", "Absolute path of the file on this computer.", true), ("folder", "string", "textures, models or sounds.", true), ("name", "string", "File name to use in the studio (default: the file's own name).", false)], read_only: false, destructive: false },
    Tool { name: "studio_view_texture", description: "Look at one picture from the studio's textures (a .png name from studio_assets). Comes back as an image.", args: &[SERVER, ("file", "string", "Picture file name, e.g. ruby.png.", true)], read_only: true, destructive: false },
    Tool { name: "studio_remove_asset", description: "Remove a picture, model or sound from the studio's assets, then the pack is built again. Check with studio_overview first that nothing still names it.", args: &[SERVER, ("folder", "string", "textures, models or sounds.", true), ("file", "string", "File name from studio_assets.", true)], read_only: false, destructive: true },
    Tool { name: "studio_build_pack", description: "Build the studio's resource pack again and report its size, format and problems. Players get it on their next join (or as the host set it up).", args: &[SERVER], read_only: false, destructive: false },
    Tool { name: "storage_list", description: "The team's Storage on the host: files kept for later (models, packs, plugin jars, settings) with their versions. Only for servers shared with a team.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "storage_put_in_server", description: "Copy a file from the team's Storage into this server, into a folder inside the server folder (e.g. plugins, or plugins/Monolab/studio/assets/models). Version 0 = the newest.", args: &[SERVER, ("item", "string", "Storage item id from storage_list.", true), ("folder", "string", "Folder inside the server, empty for the top.", true), ("version", "integer", "Version number (default: the newest).", false)], read_only: false, destructive: false },
    Tool { name: "live_settings", description: "Settings and game rules the running server can change right now without a restart (Minecraft 1.21.9+ management server). Says why not when it can't: too old, not set up (setup_live_settings), or not running.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "setup_live_settings", description: "Turn on the management server in server.properties (this computer or SSH, never panels) so live_settings works. Applies at the next start: restart_server afterwards if the user agrees.", args: &[SERVER], read_only: false, destructive: false },
    Tool { name: "set_live_setting", description: "Change one live setting on the running server now (key from live_settings, e.g. difficulty, max_players, view_distance). The server keeps it in server.properties too.", args: &[SERVER, ("key", "string", "Setting key from live_settings.", true), ("value", "string", "New value: true/false, a number or a word (e.g. hard).", true)], read_only: false, destructive: false },
    Tool { name: "set_gamerule", description: "Change one game rule on the running server now (key from live_settings, as the server names it).", args: &[SERVER, ("rule", "string", "Game rule key from live_settings.", true), ("value", "string", "true/false or a number.", true)], read_only: false, destructive: false },
    Tool { name: "rewind_history", description: "Rewind: recent changes to the server's text files (configs, studio files…), newest first, with who made them. Servers on this computer only (a teammate's server: the host's computer). Use an entry id with rewind_show or rewind_restore.", args: &[SERVER, ("path", "string", "Only this file (relative to the server folder).", false), ("limit", "integer", "How many (default 30).", false)], read_only: true, destructive: false },
    Tool { name: "rewind_show", description: "One Rewind change: the file before it, after it, and as it is now.", args: &[SERVER, ("entry", "string", "Entry id from rewind_history.", true)], read_only: true, destructive: false },
    Tool { name: "rewind_restore", description: "Put a file back as it was before (undo) or after a Rewind change. The file's current text is kept in Rewind first, so this can be undone too. Tell the user which file you put back.", args: &[SERVER, ("entry", "string", "Entry id from rewind_history.", true), ("side", "string", "before (undo the change, default) or after.", false)], read_only: false, destructive: true },
    Tool { name: "list_backups", description: "Backups of a server.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "create_backup", description: "Make a backup of the whole server folder now. Good idea before risky changes.", args: &[SERVER, ("note", "string", "Short label (optional).", false)], read_only: false, destructive: false },
];

/// A locked agent gets no `list_servers` and no `server` argument: every tool schema is sent with every turn.
fn tool_list(locked: bool) -> Vec<Value> {
    TOOLS
        .iter()
        .filter(|tool| !(locked && APP_WIDE.contains(&tool.name)))
        .map(|tool| {
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();
            for (name, kind, description, needed) in tool.args {
                if locked && *name == "server" {
                    continue;
                }
                properties.insert((*name).to_string(), json!({ "type": kind, "description": description }));
                if *needed {
                    required.push(*name);
                }
            }
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": { "type": "object", "properties": properties, "required": required },
                "annotations": { "readOnlyHint": tool.read_only, "destructiveHint": tool.destructive },
            })
        })
        .collect()
}

fn invoke(app: &AppHandle, command: &str, body: Value) -> Result<Value, String> {
    if let Some(server) = ON_HOST.with(|one| one.borrow().clone()) {
        return crate::team_agent::call(app, &server, command, body);
    }
    let value = crate::web_invoke(app, command, &body)?;
    // This PC's agent wrote it (F2, Rewind "who"); a shared server's host notes the teammate's agent itself.
    crate::note_writes(app, body.get("id").and_then(Value::as_str).unwrap_or_default(), command, &body, "AI");
    Ok(value)
}

fn text<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name).and_then(Value::as_str).map(str::trim).filter(|value| !value.is_empty())
}

fn need<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    text(args, name).ok_or_else(|| format!("`{name}` is required."))
}

fn ensure_folder(app: &AppHandle, with: &dyn Fn(Value) -> Value, path: &str) -> Result<String, String> {
    let parts: Vec<&str> = path
        .split(['/', '\\'])
        .map(str::trim)
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    if parts.is_empty() {
        return Err("`path` is empty.".into());
    }
    for at in 0..parts.len() {
        let parent = parts[..at].join("/");
        let name = parts[at];
        if let Err(first) = invoke(app, "create_server_folder", with(json!({ "path": parent, "name": name }))) {
            let page = invoke(app, "browse_server_files", with(json!({ "path": parent, "offset": null, "limit": null })))
                .map_err(|_| first.clone())?;
            let is_folder = page
                .get("entries")
                .and_then(Value::as_array)
                .map(|entries| {
                    entries.iter().any(|one| {
                        one.get("name").and_then(Value::as_str) == Some(name)
                            && one.get("isDir").and_then(Value::as_bool) == Some(true)
                    })
                })
                .unwrap_or(false);
            if !is_folder {
                return Err(first);
            }
        }
    }
    Ok(parts.join("/"))
}

fn pick(app: &AppHandle, args: &Value, locked: Option<&str>) -> Result<Value, String> {
    let list = invoke(app, "list_instances", json!({}))?;
    let list = list.as_array().cloned().unwrap_or_default();
    let field = |one: &Value, key: &str| one.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    if let Some(own) = locked {
        let mine = list
            .iter()
            .find(|one| field(one, "id") == own)
            .cloned()
            .ok_or_else(|| "This agent's server is no longer registered in the app.".to_string())?;
        if let Some(wanted) = text(args, "server") {
            if wanted != own && !field(&mine, "name").eq_ignore_ascii_case(wanted) {
                return Err(format!("This agent only works on server `{}` (id {own}). Leave out `server`.", field(&mine, "name")));
            }
        }
        return Ok(mine);
    }
    match text(args, "server") {
        Some(wanted) => list
            .iter()
            .find(|one| field(one, "id") == wanted)
            .or_else(|| list.iter().find(|one| field(one, "name").eq_ignore_ascii_case(wanted)))
            .cloned()
            .ok_or_else(|| format!("No server named or with id `{wanted}`. Call list_servers.")),
        None if list.len() == 1 => Ok(list[0].clone()),
        None => Err("Several servers exist — pass `server` (id or name from list_servers).".into()),
    }
}

/// The core's proper name (any case), or an error listing the ones that work.
fn known_core(wanted: &str) -> Result<&'static str, String> {
    let cores = crate::kinds::creatable();
    cores
        .iter()
        .find(|one| one.eq_ignore_ascii_case(wanted.trim()))
        .copied()
        .ok_or_else(|| format!("Unknown core `{wanted}`. Use one of: {}.", cores.join(", ")))
}

fn where_of(one: &Value) -> String {
    let get = |key: &str| one.get(key).and_then(Value::as_str).unwrap_or_default();
    if !get("panelHost").is_empty() {
        format!("panel:{}", get("panelHost"))
    } else if !get("remoteHost").is_empty() {
        format!("ssh:{}", get("remoteHost"))
    } else {
        "local".into()
    }
}

fn addon_kind(core: &str) -> &'static str {
    crate::kinds::addon_word(core)
}

fn call(app: &AppHandle, name: &str, args: &Value, locked: Option<&str>) -> Result<Value, String> {
    if locked.is_some() && matches!(name, "list_core_versions" | "create_server") {
        return Err("This agent works on one server only and can't make new servers.".into());
    }
    if name == "list_core_versions" {
        let core = known_core(need(args, "core")?)?;
        let snapshots = args.get("snapshots").and_then(Value::as_bool).unwrap_or(false);
        let list = invoke(app, "list_core_versions", json!({ "core": core, "snapshots": snapshots }))?;
        let all: Vec<Value> = list.as_array().cloned().unwrap_or_default();
        let shown: Vec<Value> = all.iter().take(40).cloned().collect();
        return Ok(json!({ "core": core, "versions": shown, "more": all.len().saturating_sub(shown.len()) }));
    }
    if name == "create_server" {
        if args.get("eulaAccepted").and_then(Value::as_bool) != Some(true) {
            return Err("Ask the user to agree to the Minecraft EULA (https://aka.ms/MinecraftEULA) first, then call again with eulaAccepted: true.".into());
        }
        let core = known_core(need(args, "core")?)?;
        let version = match text(args, "version") {
            Some(version) => version.to_string(),
            None => invoke(app, "list_core_versions", json!({ "core": core, "snapshots": false }))?
                .as_array()
                .and_then(|list| list.first())
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("No versions found for {core}."))?,
        };
        let memory = args.get("memoryMb").and_then(Value::as_u64).unwrap_or(4096).clamp(512, 65536);
        let made = invoke(
            app,
            "create_instance",
            json!({ "input": {
                "name": text(args, "name").unwrap_or(""), "core": core, "version": version,
                "maxMemoryMib": memory, "minMemoryMib": memory, "eulaAccepted": true,
            } }),
        )?;
        // The server list refreshes on any status message.
        let _ = app.emit("server-status", json!({ "serverId": made.get("id"), "status": "stopped" }));
        return Ok(json!({
            "id": made.get("id"), "name": made.get("name"), "core": made.get("core"), "version": made.get("version"),
            "port": made.get("port"), "path": made.get("path"),
            "note": "Created, not started. Use start_server to start it.",
        }));
    }
    if name == "list_servers" {
        let list = invoke(app, "list_instances", json!({}))?;
        return Ok(Value::Array(
            list.as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter(|one| locked.is_none_or(|own| one.get("id").and_then(Value::as_str) == Some(own)))
                .map(|one| {
                    json!({
                        "id": one.get("id"), "name": one.get("name"), "core": one.get("core"),
                        "version": one.get("version"), "status": one.get("status"), "port": one.get("port"),
                        "where": where_of(one), "path": one.get("path"),
                    })
                })
                .collect(),
        ));
    }
    let server = pick(app, args, locked)?;
    let id = server.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
    let core = server.get("core").and_then(Value::as_str).unwrap_or_default().to_string();
    let with = |extra: Value| {
        let mut body = json!({ "id": id });
        if let (Some(target), Some(more)) = (body.as_object_mut(), extra.as_object()) {
            target.extend(more.clone());
        }
        body
    };
    match name {
        "server_status" => {
            let resources = invoke(app, "get_resource_snapshot", with(json!({}))).unwrap_or(Value::Null);
            let tps = invoke(app, "get_tps_snapshot", with(json!({}))).unwrap_or(Value::Null);
            let mut live = invoke(app, "read_live", with(json!({}))).unwrap_or(Value::Null);
            if let Some(map) = live.as_object_mut() {
                for (key, keep) in [("entityKinds", 8), ("hotspots", 3), ("players", 20)] {
                    if let Some(list) = map.get_mut(key).and_then(Value::as_array_mut) {
                        list.truncate(keep);
                    }
                }
            }
            Ok(json!({
                "name": server.get("name"), "core": core, "version": server.get("version"),
                "status": server.get("status"), "ready": server.get("ready"), "crashReason": server.get("crashReason"),
                "where": where_of(&server), "resources": resources, "tps": tps, "live": live,
            }))
        }
        "read_console" => {
            let most = args.get("lines").and_then(Value::as_u64).unwrap_or(40).clamp(1, 500) as usize;
            let everything = args.get("all").and_then(Value::as_bool).unwrap_or(false);
            let all = console_lines(app, &id)?;
            if all.is_empty() {
                return Ok(Value::String("(console is empty — the server may not have run since the app started; try search_logs)".into()));
            }
            let mark = if everything { None } else { cursors().lock().ok().and_then(|marks| marks.get(&id).cloned()) };
            let from = after_mark(&all, mark.as_deref());
            mark_read(&id, &all);
            let fresh = &all[from..];
            if fresh.is_empty() {
                return Ok(Value::String("(no new console lines since your last read)".into()));
            }
            let kept: Vec<String> = fresh.iter().filter(|(_, noise)| !noise).map(|(line, _)| line.clone()).collect();
            let hidden = fresh.len() - kept.len();
            let skipped = kept.len().saturating_sub(most);
            let mut out = String::new();
            if skipped > 0 {
                out.push_str(&format!("… {skipped} earlier lines not shown (use search_logs to find something specific)\n"));
            }
            out.push_str(&squeeze(&kept[skipped..], 300).join("\n"));
            if hidden > 0 {
                out.push_str(&format!("\n({hidden} harmless noise lines hidden)"));
            }
            Ok(Value::String(out))
        }
        "list_log_files" => invoke(app, "list_log_files", with(json!({}))),
        "search_logs" => {
            let all = invoke(
                app,
                "list_log_entries",
                with(json!({
                    "query": text(args, "query").unwrap_or(""),
                    "level": text(args, "level").unwrap_or("All"),
                    "fileName": text(args, "file").unwrap_or("latest.log"),
                })),
            )?;
            let lines: Vec<String> = all
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|one| one.get("line").and_then(Value::as_str).map(str::to_string))
                .collect();
            if lines.is_empty() {
                return Ok(Value::String("(no matching lines)".into()));
            }
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50).clamp(1, 300) as usize;
            let from = lines.len().saturating_sub(limit);
            let mut out = String::new();
            if from > 0 {
                out.push_str(&format!("… showing the last {limit} of {} matching lines\n", lines.len()));
            }
            out.push_str(&squeeze(&lines[from..], 300).join("\n"));
            Ok(Value::String(out))
        }
        "list_crash_reports" => invoke(app, "list_crash_reports", with(json!({}))),
        "diagnose_lag" => invoke(app, "diagnose_lag", with(json!({}))),
        "precheck" => invoke(app, "precheck_instance", with(json!({}))),
        "list_files" => {
            let page = invoke(app, "browse_server_files", with(json!({ "path": text(args, "path").unwrap_or(""), "offset": null, "limit": null })))?;
            let entries = page.get("entries").and_then(Value::as_array).cloned().unwrap_or_default();
            let total = page.get("total").and_then(Value::as_u64).unwrap_or(entries.len() as u64);
            let mut lines: Vec<String> = entries
                .iter()
                .map(|one| {
                    let name = one.get("name").and_then(Value::as_str).unwrap_or("");
                    if one.get("isDir").and_then(Value::as_bool).unwrap_or(false) {
                        format!("{name}/")
                    } else {
                        let size = one.get("sizeBytes").and_then(Value::as_u64).unwrap_or(0);
                        format!("{name}  {}", human_size(size))
                    }
                })
                .collect();
            if total as usize > entries.len() {
                lines.push(format!("… {} more not shown", total as usize - entries.len()));
            }
            Ok(Value::String(if lines.is_empty() { "(empty folder)".into() } else { lines.join("\n") }))
        }
        "read_file" => {
            let content = invoke(app, "read_editable_file", with(json!({ "path": need(args, "path")? })))?;
            let start = args.get("start").and_then(Value::as_u64).map(|one| one as usize);
            let lines = args.get("lines").and_then(Value::as_u64).map(|one| one as usize);
            match content {
                Value::String(content) if start.is_some() || lines.is_some() => Ok(Value::String(line_window(&content, start.unwrap_or(1), lines))),
                other => Ok(other),
            }
        }
        "edit_file" => {
            let path = need(args, "path")?.trim_start_matches('/').to_string();
            let old = args.get("oldText").and_then(Value::as_str).filter(|one| !one.is_empty()).ok_or("`oldText` is required.")?;
            let new = args.get("newText").and_then(Value::as_str).ok_or("`newText` is required.")?;
            let Value::String(content) = invoke(app, "read_editable_file", with(json!({ "path": path })))? else {
                return Err(format!("Could not read {path} as text."));
            };
            let changed = replace_once(&content, old, new)?;
            invoke(app, "save_editable_file", with(json!({ "path": path, "content": changed })))?;
            Ok(Value::String(format!("Edited {path}.")))
        }
        "write_file" => {
            let path = need(args, "path")?.trim_start_matches('/').to_string();
            let content = args.get("content").and_then(Value::as_str).ok_or("`content` is required.")?;
            let save = |app: &AppHandle| invoke(app, "save_editable_file", with(json!({ "path": path, "content": content })));
            match save(app) {
                Ok(_) => Ok(Value::String(format!("Saved {path}."))),
                Err(first) => {
                    let (dir, file) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
                    if !dir.is_empty() {
                        ensure_folder(app, &with, dir).map_err(|_| first.clone())?;
                    }
                    invoke(app, "create_server_file", with(json!({ "path": dir, "name": file }))).map_err(|_| first)?;
                    save(app)?;
                    Ok(Value::String(format!("Created {path}.")))
                }
            }
        }
        "delete_file" => {
            let path = need(args, "path")?.trim_start_matches('/').to_string();
            if path.is_empty() {
                return Err("Refusing to trash the whole server folder.".into());
            }
            invoke(app, "trash_server_path", with(json!({ "path": path })))?;
            Ok(Value::String(format!("Moved {path} to the server trash (restorable from the Files tab).")))
        }
        "make_folder" => {
            let path = need(args, "path")?;
            let made = ensure_folder(app, &with, path)?;
            Ok(Value::String(format!("Folder {made} is ready.")))
        }
        "add_addon_file" => {
            let local = need(args, "localPath")?;
            let path = std::path::Path::new(local);
            if !path.is_file() {
                return Err(format!("No file at {local} on this computer."));
            }
            if ON_HOST.with(|one| one.borrow().is_some()) {
                // A teammate's server: the file goes to the host first, then the host adds that upload.
                let token = upload_to_host(app, path)?;
                return invoke(app, "add_addon_files", with(json!({ "paths": [token] })));
            }
            use tauri::Manager;
            tauri::async_runtime::block_on(crate::add_addon_files(app.clone(), app.state(), id.clone(), vec![local.to_string()]))
                .and_then(|done| serde_json::to_value(done).map_err(|error| error.to_string()))
        }
        "read_properties" => {
            let all = invoke(app, "read_server_properties", with(json!({})))?;
            Ok(Value::Array(
                all.as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|one| json!({ "key": one.get("key"), "value": one.get("value") }))
                    .collect(),
            ))
        }
        "set_properties" => {
            let values = args.get("values").and_then(Value::as_object).ok_or("`values` must be an object.")?;
            let current = invoke(app, "read_server_properties", with(json!({})))?;
            let mut entries: Vec<(String, String)> = current
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|one| {
                    (
                        one.get("key").and_then(Value::as_str).unwrap_or_default().to_string(),
                        one.get("value").and_then(Value::as_str).unwrap_or_default().to_string(),
                    )
                })
                .collect();
            for (key, value) in values {
                let value = match value {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                match entries.iter_mut().find(|(have, _)| have == key) {
                    Some(entry) => entry.1 = value,
                    None => entries.push((key.clone(), value)),
                }
            }
            let properties: Vec<Value> = entries.into_iter().map(|(key, value)| json!({ "key": key, "value": value })).collect();
            invoke(app, "save_server_properties", with(json!({ "properties": properties })))?;
            Ok(Value::String("Saved. Takes effect on next start.".into()))
        }
        "list_addons" => {
            let all = invoke(app, "scan_addons", with(json!({})))?;
            let needle = text(args, "query").unwrap_or("").to_lowercase();
            let list = all.as_array().cloned().unwrap_or_default();
            let lines: Vec<String> = list
                .iter()
                .filter(|one| {
                    needle.is_empty()
                        || one.get("fileName").and_then(Value::as_str).unwrap_or("").to_lowercase().contains(&needle)
                        || one.get("name").and_then(Value::as_str).unwrap_or("").to_lowercase().contains(&needle)
                })
                .map(|one| {
                    let file = one.get("fileName").and_then(Value::as_str).unwrap_or("");
                    let version = one.get("version").and_then(Value::as_str).unwrap_or("");
                    let mut line = if version.is_empty() { file.to_string() } else { format!("{file} · {version}") };
                    if !one.get("enabled").and_then(Value::as_bool).unwrap_or(true) {
                        line.push_str(" · disabled");
                    }
                    let fit = one.get("compatibility").and_then(Value::as_str).unwrap_or("");
                    if !fit.is_empty() && !fit.eq_ignore_ascii_case("ok") && !fit.eq_ignore_ascii_case("compatible") {
                        line.push_str(&format!(" · {fit}"));
                    }
                    line
                })
                .collect();
            Ok(Value::String(format!("{} of {} installed\n{}", lines.len(), list.len(), lines.join("\n"))))
        }
        "search_modrinth" => {
            let page = invoke(
                app,
                "search_addon_catalog",
                with(json!({
                    "query": text(args, "query").unwrap_or(""),
                    "page": args.get("page").and_then(Value::as_u64),
                    "categories": null, "environment": null, "versions": null, "loader": null,
                })),
            )?;
            let hits = page.get("hits").and_then(Value::as_array).cloned().unwrap_or_default();
            Ok(json!({
                "hasMore": page.get("hasMore"),
                "hits": hits.iter().map(|one| json!({
                    "projectId": one.get("projectId"), "title": one.get("title"), "description": one.get("description"),
                    "author": one.get("author"), "downloads": one.get("downloads"), "loaders": one.get("loaders"),
                    "gameVersions": one.get("versions").and_then(Value::as_array).map(|list| list.iter().rev().take(6).cloned().collect::<Vec<_>>()),
                })).collect::<Vec<_>>(),
            }))
        }
        "list_addon_versions" => {
            let all = invoke(app, "list_addon_versions", with(json!({ "projectId": need(args, "projectId")?, "projectType": addon_kind(&core) })))?;
            Ok(Value::Array(
                all.as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .take(30)
                    .map(|one| json!({
                        "versionId": one.get("id"), "name": one.get("name"), "fits": one.get("fits"), "channel": one.get("channel"),
                        "gameVersions": span_of(one.get("gameVersions")), "loaders": one.get("loaders"),
                    }))
                    .collect(),
            ))
        }
        "install_addon" => {
            invoke(
                app,
                "install_modrinth",
                with(json!({ "projectId": need(args, "projectId")?, "projectType": addon_kind(&core), "versionId": text(args, "versionId") })),
            )?;
            Ok(Value::String("Installed. If the server is running it loads on next start.".into()))
        }
        "toggle_addon" | "delete_addon" => {
            let file = need(args, "fileName")?;
            let all = invoke(app, "scan_addons", with(json!({})))?;
            let found = all
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .find(|one| one.get("fileName").and_then(Value::as_str) == Some(file))
                .ok_or_else(|| format!("No installed file `{file}`. Call list_addons."))?;
            let kind = found.get("kind").and_then(Value::as_str).unwrap_or("mod").to_string();
            invoke(app, name, with(json!({ "fileName": file, "kind": kind })))?;
            Ok(Value::String(if name == "toggle_addon" { format!("Toggled {file}.") } else { format!("Removed {file} (in the app trash).") }))
        }
        "monolab_addon" => match need(args, "action")? {
            "status" => invoke(app, "addon_status", with(json!({}))),
            "install" => invoke(app, "install_addon", with(json!({}))).map(|_| Value::String("Installed the bundled Monolab addon. Restart the server so it loads.".into())),
            "remove" => invoke(app, "remove_addon", with(json!({}))).map(|_| Value::String("Removed the Monolab addon. Restart the server to unload it.".into())),
            _ => Err("action must be status, install or remove.".into()),
        },
        "start_server" => {
            let before = last_line(app, &id);
            invoke(app, "start_instance", with(json!({})))?;
            Ok(Value::String(wait_ready(app, &id, before)))
        }
        "stop_server" => invoke(app, "stop_instance", with(json!({}))).map(|_| Value::String("Stopping.".into())),
        "restart_server" => {
            let before = last_line(app, &id);
            invoke(app, "restart_instance", with(json!({})))?;
            Ok(Value::String(wait_ready(app, &id, before)))
        }
        "send_command" => {
            let command = need(args, "command")?.trim_start_matches('/');
            invoke(app, "send_console_command", with(json!({ "command": command })))?;
            std::thread::sleep(Duration::from_millis(1200));
            let tail = call(app, "read_console", &json!({ "server": id, "lines": 15 }), locked).unwrap_or(Value::Null);
            Ok(Value::String(format!("Sent `{command}`. New console lines:\n{}", pretty(tail))))
        }
        "list_players" => invoke(app, "list_players", with(json!({}))),
        "list_backups" => invoke(app, "list_backups", with(json!({}))),
        "studio_overview" => invoke(app, "studio_overview", with(json!({}))),
        "studio_read" => invoke(app, "studio_read", with(json!({ "kind": need(args, "kind")?, "name": need(args, "name")? }))),
        "studio_write" => invoke(app, "studio_save", with(json!({ "kind": need(args, "kind")?, "name": need(args, "name")?, "text": need(args, "json")?, "hash": text(args, "hash").unwrap_or_default() }))),
        "studio_new" => invoke(app, "studio_template", with(json!({ "kind": need(args, "kind")?, "name": "new" }))),
        "studio_assets" => invoke(app, "studio_asset_list", with(json!({}))),
        "studio_upload_asset" => {
            let local = need(args, "localPath")?;
            let folder = need(args, "folder")?;
            if !STUDIO_FOLDERS.contains(&folder) {
                return Err("`folder` must be textures, models or sounds.".into());
            }
            let path = std::path::Path::new(local);
            if !path.is_file() {
                return Err(format!("No file at {local} on this computer."));
            }
            let file = text(args, "name").map(str::to_string).or_else(|| path.file_name().and_then(|one| one.to_str()).map(str::to_string)).ok_or("Could not read the file name.")?;
            if ON_HOST.with(|one| one.borrow().is_some()) {
                let upload = upload_to_host(app, path)?;
                return invoke(app, "studio_asset_add_upload", with(json!({ "folder": folder, "file": file, "upload": upload })));
            }
            use base64::Engine;
            let bytes = std::fs::read(path).map_err(|error| format!("Could not read {local}: {error}"))?;
            invoke(app, "studio_asset_add", with(json!({ "folder": folder, "file": file, "data": base64::engine::general_purpose::STANDARD.encode(bytes) })))
        }
        "studio_build_pack" => invoke(app, "studio_pack_build", with(json!({}))),
        "studio_view_texture" => invoke(app, "studio_asset_image", with(json!({ "file": need(args, "file")? }))),
        "studio_remove_asset" => {
            let folder = need(args, "folder")?;
            if !STUDIO_FOLDERS.contains(&folder) {
                return Err("`folder` must be textures, models or sounds.".into());
            }
            invoke(app, "studio_asset_remove", with(json!({ "folder": folder, "file": need(args, "file")? })))
        }
        "live_settings" => {
            let state = invoke(app, "msmp_state", with(json!({})))?;
            if state["connected"] != true {
                let why = if state["supported"] != true {
                    state["reason"].as_str().unwrap_or("This server has no live settings.").to_string()
                } else if state["ready"] != true {
                    "Live settings aren't set up yet: call setup_live_settings, then restart the server.".into()
                } else if state["running"] != true {
                    "The server isn't running. Start it to use live settings (set_properties still works for the next start).".into()
                } else {
                    format!("Could not reach the server's management port: {}", state["error"].as_str().unwrap_or("no answer"))
                };
                return Ok(Value::String(why));
            }
            let lines = |list: Value| -> Vec<String> {
                list.as_array().into_iter().flatten().map(|one| format!("{} = {} ({})", field_of(one, "key"), field_of(one, "value"), field_of(one, "kind"))).collect()
            };
            let settings = lines(invoke(app, "msmp_settings", with(json!({})))?);
            let rules = lines(invoke(app, "msmp_rules", with(json!({})))?);
            Ok(Value::String(format!("Settings (set_live_setting):\n{}\n\nGame rules (set_gamerule):\n{}", settings.join("\n"), rules.join("\n"))))
        }
        "setup_live_settings" => invoke(app, "msmp_setup", with(json!({}))).map(|_| Value::String("Live settings are set up. They work after the next start.".into())),
        "set_live_setting" => {
            let key = need(args, "key")?;
            let now = invoke(app, "msmp_set_setting", with(json!({ "key": key, "value": json_text(args.get("value"))? })))?;
            Ok(Value::String(format!("{key} is now {}.", pretty(now))))
        }
        "set_gamerule" => {
            let rule = need(args, "rule")?;
            let value = json_text(args.get("value"))?;
            invoke(app, "msmp_set_rule", with(json!({ "key": rule, "value": value })))?;
            Ok(Value::String(format!("{rule} is now {value}.")))
        }
        "rewind_history" => {
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(30).clamp(1, 200);
            // Catch up first, so a change the agent just made is there.
            let _ = invoke(app, "rewind_scan", with(json!({})));
            let list = invoke(app, "rewind_list", with(json!({ "path": text(args, "path").filter(|one| !one.is_empty()), "limit": limit })))?;
            let rows: Vec<String> = list
                .as_array()
                .into_iter()
                .flatten()
                .map(|one| {
                    let by = one.get("by").and_then(Value::as_str).map(|who| format!(" by {who}")).unwrap_or_default();
                    format!("{}  {}  {} {}{by}", field_of(one, "id"), ago(one.get("at").and_then(Value::as_u64).unwrap_or(0)), field_of(one, "kind"), field_of(one, "path"))
                })
                .collect();
            Ok(Value::String(if rows.is_empty() { "No changes kept yet.".into() } else { rows.join("\n") }))
        }
        "rewind_show" => {
            let versions = invoke(app, "rewind_versions", with(json!({ "entry": need(args, "entry")? })))?;
            let part = |key: &str| versions.get(key).and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| "(no file)".into());
            Ok(Value::String(format!("--- before\n{}\n--- after\n{}\n--- now\n{}", part("before"), part("after"), part("current"))))
        }
        "rewind_restore" => {
            let side = text(args, "side").filter(|one| !one.is_empty()).unwrap_or("before");
            if side != "before" && side != "after" {
                return Err("`side` must be before or after.".into());
            }
            let done = invoke(app, "rewind_restore", with(json!({ "entry": need(args, "entry")?, "side": side })))?;
            Ok(Value::String(format!("Put {} back as it was {side} that change.", field_of(&done, "path"))))
        }
        // A shared server's memo is on the host: ask it (F3).
        "storage_list" if ON_HOST.with(|one| one.borrow().is_some()) => invoke(app, "__storage_view", with(json!({}))),
        "storage_list" => {
            let team = crate::monolink_team::team_of(app, &id).ok_or("This server isn't shared with a team, so it has no Storage.")?;
            serde_json::to_value(crate::team_storage_of(app, &team)?.view()).map_err(|error| error.to_string())
        }
        "storage_put_in_server" => {
            let item = need(args, "item")?.to_string();
            let folder = text(args, "folder").unwrap_or_default().to_string();
            let version = args.get("version").and_then(Value::as_u64).unwrap_or(0);
            if ON_HOST.with(|one| one.borrow().is_some()) {
                return invoke(app, "__storage_to_server", with(json!({ "item": item, "folder": folder, "version": version })));
            }
            let team = crate::monolink_team::team_of(app, &id).ok_or("This server isn't shared with a team, so it has no Storage.")?;
            let path = tauri::async_runtime::block_on(crate::storage_into_server(app, &team, &item, version as u32, &id, &folder))?;
            crate::note_writes(app, &id, "save_editable_file", &json!({ "path": path }), "AI");
            Ok(Value::String(format!("Put it in {path}.")))
        }
        "read_memo" if ON_HOST.with(|one| one.borrow().is_some()) => invoke(app, "agent_read_memo", with(json!({}))),
        "read_memo" => crate::memo_for_agent(app, &id).map(Value::String),
        "update_todo" if ON_HOST.with(|one| one.borrow().is_some()) => invoke(
            app,
            "agent_update_todo",
            with(json!({ "action": need(args, "action")?, "todo": text(args, "id"), "text": text(args, "text"), "status": text(args, "status"), "assignee": text(args, "assignee") })),
        ),
        "update_todo" => crate::update_todo(
            app,
            &id,
            &crate::memo::TodoChange { action: need(args, "action")?, todo: text(args, "id"), words: text(args, "text"), status: text(args, "status"), assignee: text(args, "assignee"), by: "AI" },
        )
        .map(Value::String),
        "create_backup" => invoke(app, "create_backup", with(json!({ "note": text(args, "note").unwrap_or("Claude"), "worldsOnly": false }))),
        other => Err(format!("Unknown tool `{other}`.")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_tool_has_a_schema_and_unique_name() {
        let list = super::tool_list(false);
        assert_eq!(list.len(), super::TOOLS.len());
        let mut names: Vec<&str> = super::TOOLS.iter().map(|tool| tool.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), super::TOOLS.len());
        for tool in &list {
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn studio_tools_are_listed() {
        let names = ["studio_overview", "studio_read", "studio_write", "studio_new", "studio_assets", "studio_upload_asset", "studio_build_pack", "storage_list", "storage_put_in_server"];
        for name in names {
            let tool = super::TOOLS.iter().find(|tool| tool.name == name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(tool.read_only, matches!(name, "studio_overview" | "studio_read" | "studio_new" | "studio_assets" | "storage_list"), "{name}");
        }
    }

    #[test]
    fn values_become_json_text() {
        use serde_json::json;
        assert_eq!(super::json_text(Some(&json!("true"))).unwrap(), "true");
        assert_eq!(super::json_text(Some(&json!(" 8 "))).unwrap(), "8");
        assert_eq!(super::json_text(Some(&json!("hard"))).unwrap(), "\"hard\"");
        assert_eq!(super::json_text(Some(&json!("\"peaceful\""))).unwrap(), "\"peaceful\"");
        assert_eq!(super::json_text(Some(&json!(12))).unwrap(), "12");
        assert_eq!(super::json_text(Some(&json!(false))).unwrap(), "false");
        assert!(super::json_text(Some(&json!(""))).is_err());
        assert!(super::json_text(None).is_err());
    }

    #[test]
    fn live_and_rewind_tools_are_listed() {
        for (name, read_only) in [
            ("live_settings", true),
            ("setup_live_settings", false),
            ("set_live_setting", false),
            ("set_gamerule", false),
            ("rewind_history", true),
            ("rewind_show", true),
            ("rewind_restore", false),
            ("studio_view_texture", true),
            ("studio_remove_asset", false),
        ] {
            let tool = super::TOOLS.iter().find(|tool| tool.name == name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(tool.read_only, read_only, "{name}");
        }
    }

    #[test]
    fn upload_folder_must_be_known() {
        for bad in ["../x", "plugins", ""] {
            assert!(!super::STUDIO_FOLDERS.contains(&bad), "{bad}");
        }
        assert!(super::STUDIO_FOLDERS.contains(&"models"));
    }

    #[test]
    fn edits_one_place_in_the_file_line_breaks() {
        let file = "a: 1\r\nb: 2\r\nc: 3\r\n";
        assert_eq!(super::replace_once(file, "a: 1\nb: 2", "a: 1\nb: 5").unwrap(), "a: 1\r\nb: 5\r\nc: 3\r\n");
        assert!(super::replace_once(file, "z", "y").is_err());
        assert!(super::replace_once("x x", "x", "y").is_err());
        assert_eq!(super::line_window("1\n2\n3\n4", 2, Some(2)), "(lines 2-3 of 4)\n2\n3");
        assert_eq!(super::line_window("1\n2", 9, None), "(lines 2-2 of 2)\n2");
        assert_eq!(super::line_window("", 1, Some(5)), "(lines 1-0 of 0)\n");
    }

    #[test]
    fn shared_servers_are_asked_less_often() {
        use std::time::Duration;
        assert_eq!(super::poll_gap(false, Duration::from_secs(200)), Duration::from_secs(2));
        assert_eq!(super::poll_gap(true, Duration::from_secs(0)), Duration::from_secs(5));
        assert_eq!(super::poll_gap(true, Duration::from_secs(45)), Duration::from_secs(10));
        assert_eq!(super::poll_gap(true, Duration::from_secs(150)), Duration::from_secs(15));
    }

    #[test]
    fn compares_tokens_fully() {
        assert!(super::same("abc", "abc"));
        assert!(!super::same("abc", "abd"));
        assert!(!super::same("abc", "ab"));
    }

    #[test]
    fn strips_pictures_and_clips() {
        let slim = super::slim(serde_json::json!({ "icon": "data:image/png;base64,AAAA", "name": "x" }));
        assert_eq!(slim["icon"], "[image]");
        assert!(super::clip(&"\u{AC00}".repeat(40_000)).contains("truncated"));
    }

    #[test]
    fn folds_repeats_and_finds_new_lines() {
        let lines: Vec<String> = ["a", "b", "b", "b", "c"].iter().map(|one| one.to_string()).collect();
        assert_eq!(super::squeeze(&lines, 300), vec!["a", "b  (x3)", "c"]);
        let seen: Vec<(String, bool)> = ["x", "y", "z"].iter().map(|one| (one.to_string(), false)).collect();
        assert_eq!(super::after_mark(&seen, Some("y")), 2);
        assert_eq!(super::after_mark(&seen, Some("gone")), 0);
        assert_eq!(super::after_mark(&seen, None), 0);
        assert!(super::shorten(&"k".repeat(400), 300).ends_with('…'));
    }
}

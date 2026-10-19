
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::AppHandle;

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
Paths are relative to the server folder and cannot leave it. Messages from the app are in Korean. \
Plugin servers (Paper/Purpur/Folia) take plugins, Fabric/NeoForge/Forge take mods.";

fn handle(app: &AppHandle, message: &Value, locked: Option<&str>) -> Option<Value> {
    let id = message.get("id").cloned()?;
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
                    Some(_) => format!("{INSTRUCTIONS} This connection is locked to ONE server: every tool acts on it and the `server` argument can be left out."),
                    None => INSTRUCTIONS.to_string(),
                },
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tool_list() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            Ok(match call(app, name, &args, locked) {
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
    let outcome = loop {
        std::thread::sleep(Duration::from_secs(2));
        let list = invoke(app, "list_instances", json!({})).unwrap_or(Value::Null);
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

const SERVER: (&str, &str, &str, bool) = ("server", "string", "Server id or name from list_servers (optional when only one server exists).", false);

const TOOLS: &[Tool] = &[
    Tool { name: "list_servers", description: "List all servers registered in Monolab with id, core, version, status, port and where they run.", args: &[], read_only: true, destructive: false },
    Tool { name: "server_status", description: "Status of one server: running state, CPU, memory, TPS and online players when available.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "read_console", description: "New live console lines since your last read_console call (or since start_server/restart_server summarized startup). Harmless noise is hidden, repeats are folded and long lines shortened. Do not poll this in a loop - start_server and restart_server already wait for the server and summarize.", args: &[SERVER, ("lines", "integer", "Most lines to return (default 60, max 500).", false), ("all", "boolean", "true = the last lines regardless of what you already read.", false)], read_only: true, destructive: false },
    Tool { name: "list_log_files", description: "List log files (latest.log and older .log.gz) of a server.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "search_logs", description: "Read or search a log file. Empty query returns recent entries.", args: &[SERVER, ("query", "string", "Text to search for (optional).", false), ("file", "string", "Log file name, default latest.log.", false), ("level", "string", "Filter: All (default), Error, Warning.", false), ("limit", "integer", "Most matching lines to return, newest last (default 80, max 300).", false)], read_only: true, destructive: false },
    Tool { name: "list_crash_reports", description: "Crash reports with description, exception and suspected mods/plugins.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "diagnose_lag", description: "Lag diagnosis: TPS, entity hotspots, memory, view distance hints (best with the Monolab addon installed).", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "precheck", description: "Check common start problems before starting: server jar, EULA, port in use, Java version, memory.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "list_files", description: "List a folder inside the server folder, one line per entry (folders end with /).", args: &[SERVER, ("path", "string", "Folder path relative to the server folder, empty for the top.", false)], read_only: true, destructive: false },
    Tool { name: "read_file", description: "Read a text file (config, yml, json, properties, toml, txt…) inside the server folder.", args: &[SERVER, ("path", "string", "File path relative to the server folder.", true)], read_only: true, destructive: false },
    Tool { name: "write_file", description: "Write a text file inside the server folder (creates it if missing, replaces the whole content). Read it first and keep what you do not intend to change.", args: &[SERVER, ("path", "string", "File path relative to the server folder.", true), ("content", "string", "Full new file content.", true)], read_only: false, destructive: true },
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
    Tool { name: "list_backups", description: "Backups of a server.", args: &[SERVER], read_only: true, destructive: false },
    Tool { name: "create_backup", description: "Make a backup of the whole server folder now. Good idea before risky changes.", args: &[SERVER, ("note", "string", "Short label (optional).", false)], read_only: false, destructive: false },
];

fn tool_list() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|tool| {
            let mut properties = serde_json::Map::new();
            let mut required = Vec::new();
            for (name, kind, description, needed) in tool.args {
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
    crate::web_invoke(app, command, &body)
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
    match core {
        "Paper" | "Purpur" | "Folia" | "Velocity" | "BungeeCord" => "plugin",
        _ => "mod",
    }
}

fn call(app: &AppHandle, name: &str, args: &Value, locked: Option<&str>) -> Result<Value, String> {
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
            let most = args.get("lines").and_then(Value::as_u64).unwrap_or(60).clamp(1, 500) as usize;
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
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(80).clamp(1, 300) as usize;
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
        "read_file" => invoke(app, "read_editable_file", with(json!({ "path": need(args, "path")? }))),
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
            if !std::path::Path::new(local).is_file() {
                return Err(format!("No file at {local} on this computer."));
            }
            invoke(app, "add_addon_files", with(json!({ "paths": [local] })))
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
        "create_backup" => invoke(app, "create_backup", with(json!({ "note": text(args, "note").unwrap_or("Claude") }))),
        other => Err(format!("Unknown tool `{other}`.")),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_tool_has_a_schema_and_unique_name() {
        let list = super::tool_list();
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

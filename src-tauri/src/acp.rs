//! Agent Client Protocol (v1) client: runs any ACP agent from the public registry as a child process and talks
//! newline-delimited JSON-RPC 2.0 over its stdin/stdout. The agent reaches the server through the Monolab MCP tools.

use crate::data_root::MonolabDirs;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::claude_agent;

pub const PROTOCOL_VERSION: u64 = 1;
const REGISTRY_URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
const REGISTRY_FRESH: Duration = Duration::from_secs(24 * 60 * 60);
/// The first `npx` run downloads the agent, so starting up may take a while.
const START_TIMEOUT: Duration = Duration::from_secs(300);
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Agents that keep running in the background per server; the oldest idle one is closed past this.
const LIVE_AGENTS: usize = 3;
const AUTH_REQUIRED: i64 = -32000;
const METHOD_NOT_FOUND: i64 = -32601;
const INTERNAL_ERROR: i64 = -32603;

// ---------- registry ----------

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AcpAgent {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub icon: String,
    pub repository: String,
    pub license: String,
    /// `npx`, `uvx` or `binary`: how it would be started on this computer.
    pub launch: String,
    /// The launcher is on this computer (or the download is already here), so it can start right away.
    pub ready: bool,
    /// The registry has a download of it for this computer; Monolab can fetch it when the user asks.
    pub download: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Launch {
    program: PathBuf,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

fn registry_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("acp-registry.json"))
}

/// The registry, from a day-old copy when there is one, otherwise downloaded; an old copy beats none offline.
fn registry(app: &AppHandle, refresh: bool) -> Result<Value, String> {
    let file = registry_file(app)?;
    let cached = std::fs::read_to_string(&file).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let fresh = std::fs::metadata(&file)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|at| at.elapsed().ok())
        .is_some_and(|age| age < REGISTRY_FRESH);
    if let Some(value) = cached.as_ref().filter(|_| fresh && !refresh) {
        return Ok(value.clone());
    }
    let downloaded = crate::addons::client().and_then(|client| {
        let text = client
            .get(REGISTRY_URL)
            .send()
            .and_then(|response| response.error_for_status())
            .and_then(|response| response.text())
            .map_err(|error| format!("Could not load the agent list: {error}"))?;
        let value: Value = serde_json::from_str(&text).map_err(|error| format!("The agent list is invalid: {error}"))?;
        if !value.get("agents").is_some_and(Value::is_array) {
            return Err("The agent list is invalid.".to_string());
        }
        Ok((text, value))
    });
    match downloaded {
        Ok((text, value)) => {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&file, text);
            Ok(value)
        }
        Err(error) => cached.ok_or(error),
    }
}

fn on_path(names: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| names.iter().map(|name| dir.join(name)).find(|one| one.is_file()))
}

fn npx() -> Option<PathBuf> {
    on_path(if cfg!(windows) { &["npx.cmd", "npx.exe"] } else { &["npx"] })
}

fn uvx() -> Option<PathBuf> {
    on_path(if cfg!(windows) { &["uvx.exe"] } else { &["uvx"] })
}

pub(crate) fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "windows-aarch64",
        ("windows", _) => "windows-x86_64",
        ("macos", "aarch64") => "darwin-aarch64",
        ("macos", _) => "darwin-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        _ => "linux-x86_64",
    }
}

fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
}

fn env_pairs(value: Option<&Value>) -> Vec<(String, String)> {
    value
        .and_then(Value::as_object)
        .map(|map| map.iter().filter_map(|(key, one)| Some((key.clone(), one.as_str()?.to_string()))).collect())
        .unwrap_or_default()
}

fn has_download(entry: &Value) -> bool {
    entry.pointer("/distribution/binary").and_then(|one| one.get(platform())).is_some()
}

fn downloaded(entry: &Value, root: Option<&Path>) -> bool {
    root.is_some_and(|root| crate::acp_binary::installed(root, &text(entry, "id"), &text(entry, "version")))
}

/// The ways an agent can start, best first: a package runner we already have beats a download.
fn launch_kinds(entry: &Value, has_npx: bool, has_uvx: bool, has_binary: bool) -> Vec<(&'static str, bool)> {
    let distribution = entry.get("distribution");
    let has = |key: &str| distribution.and_then(|one| one.get(key)).is_some();
    let binary = has_download(entry);
    let mut kinds = Vec::new();
    if has("npx") {
        kinds.push(("npx", has_npx));
    }
    if has("uvx") {
        kinds.push(("uvx", has_uvx));
    }
    if binary {
        // Ready once the user has downloaded it.
        kinds.push(("binary", has_binary));
    }
    kinds.sort_by_key(|(_, ready)| !*ready);
    kinds
}

fn agent_list(registry: &Value, has_npx: bool, has_uvx: bool, root: Option<&Path>) -> Vec<AcpAgent> {
    registry
        .get("agents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let (launch, ready) = *launch_kinds(entry, has_npx, has_uvx, downloaded(entry, root)).first()?;
            Some(AcpAgent {
                id: text(entry, "id"),
                name: text(entry, "name"),
                version: text(entry, "version"),
                description: text(entry, "description"),
                icon: text(entry, "icon"),
                repository: text(entry, "repository"),
                license: text(entry, "license"),
                launch: launch.to_string(),
                ready,
                download: has_download(entry),
            })
        })
        .filter(|agent| !agent.id.is_empty())
        .collect()
}

fn launch_for(registry: &Value, id: &str, npx: Option<&Path>, uvx: Option<&Path>, root: Option<&Path>) -> Result<Launch, String> {
    let entry = registry
        .get("agents")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().find(|one| one.get("id").and_then(Value::as_str) == Some(id)))
        .ok_or("That agent isn't in the agent list.")?;
    let distribution = entry.get("distribution").cloned().unwrap_or(Value::Null);
    for (kind, ready) in launch_kinds(entry, npx.is_some(), uvx.is_some(), downloaded(entry, root)) {
        if !ready {
            continue;
        }
        if kind == "binary" {
            let (Some(root), Some(build)) = (root, distribution.pointer("/binary").and_then(|one| one.get(platform()))) else { continue };
            let dir = crate::acp_binary::folder(root, id, &text(entry, "version"));
            let program = crate::acp_binary::program(&dir, &text(build, "cmd"))?;
            return Ok(Launch { program, args: strings(build.get("args")), env: env_pairs(build.get("env")) });
        }
        let package = &distribution[kind];
        let name = text(package, "package");
        if name.is_empty() || name.starts_with('-') {
            continue;
        }
        let program = if kind == "npx" { npx } else { uvx };
        let Some(program) = program else { continue };
        let mut args = if kind == "npx" { vec!["--yes".to_string(), name] } else { vec![name] };
        args.extend(strings(package.get("args")));
        return Ok(Launch { program: program.to_path_buf(), args, env: env_pairs(package.get("env")) });
    }
    let kinds = launch_kinds(entry, false, false, false);
    Err(match kinds.first().map(|(kind, _)| *kind) {
        Some("npx") => "This agent needs Node.js. Install Node.js, then try again.".into(),
        Some("uvx") => "This agent needs uv (uvx). Install uv, then try again.".into(),
        Some("binary") => "Download this agent first.".into(),
        _ => "This agent can't run on this computer.".into(),
    })
}

pub fn agents(app: &AppHandle, refresh: bool) -> Result<Vec<AcpAgent>, String> {
    let root = crate::acp_binary::root(app).ok();
    Ok(agent_list(&registry(app, refresh)?, npx().is_some(), uvx().is_some(), root.as_deref()))
}

/// Downloads a registry agent that ships as a program for this computer (the user pressed Download).
pub fn install(app: &AppHandle, agent: &str) -> Result<(), String> {
    let registry = registry(app, false)?;
    crate::acp_binary::install(app, &registry, agent)
}

// ---------- JSON-RPC connection ----------

#[derive(Debug, Clone, PartialEq)]
struct RpcError {
    code: i64,
    message: String,
}

impl From<RpcError> for String {
    fn from(error: RpcError) -> String {
        error.message
    }
}

/// What a line from the agent is.
#[derive(Debug, PartialEq)]
enum Incoming {
    Response { id: u64, result: Result<Value, RpcError> },
    Request { id: Value, method: String, params: Value },
    Notification { method: String, params: Value },
    Junk,
}

fn classify(line: &str) -> Incoming {
    let Ok(value) = serde_json::from_str::<Value>(line) else { return Incoming::Junk };
    let method = value.get("method").and_then(Value::as_str).map(str::to_string);
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    match (value.get("id").filter(|one| !one.is_null()), method) {
        (Some(id), Some(method)) => Incoming::Request { id: id.clone(), method, params },
        (None, Some(method)) => Incoming::Notification { method, params },
        (Some(id), None) => {
            // Our ids are numbers; anything else isn't an answer to us.
            let Some(id) = id.as_u64() else { return Incoming::Junk };
            let result = match value.get("error") {
                Some(error) => Err(RpcError {
                    code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                    message: error.get("message").and_then(Value::as_str).unwrap_or("The agent reported an error.").to_string(),
                }),
                None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
            };
            Incoming::Response { id, result }
        }
        (None, None) => Incoming::Junk,
    }
}

type Waiters = Mutex<HashMap<(String, u64), Sender<Result<Value, RpcError>>>>;

fn waiters() -> &'static Waiters {
    static ALL: OnceLock<Waiters> = OnceLock::new();
    ALL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A fresh number for ids made elsewhere (terminals).
pub(crate) fn next_number() -> u64 {
    next_id()
}

/// Where an agent's events go: (connection id, kind, data). The app sends them to the screen; tests collect them.
type Sink = Arc<dyn Fn(&str, &str, Value) + Send + Sync>;

/// Builds the `mcpServers` list from what the agent said it can take.
type McpList = Box<dyn Fn(&Value) -> Result<Value, String> + Send + Sync>;

/// What a conversation is set up with, kept so signing in later can start one the same way.
struct Setup {
    dir: PathBuf,
    /// Folders the agent may read, write and run commands in: its workspace, and the server folder when it's on this computer.
    roots: Vec<PathBuf>,
    briefing: String,
    mcp: McpList,
}

struct Agent {
    server: String,
    agent: String,
    session: String,
    stdin: Arc<Mutex<ChildStdin>>,
    child: Child,
    alive: bool,
    busy: bool,
    /// Permission requests the user hasn't answered: (key, JSON-RPC id, line for the screen).
    pending: Vec<(String, Value, String)>,
    capabilities: Value,
    /// Sent ahead of the first prompt when the agent can't take a system prompt.
    briefing: Option<String>,
    sink: Sink,
    setup: Arc<Setup>,
}

fn live() -> &'static Mutex<HashMap<String, Agent>> {
    static ALL: OnceLock<Mutex<HashMap<String, Agent>>> = OnceLock::new();
    ALL.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Serialize, Clone, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AcpEvent {
    pub id: String,
    /// `update` (a `session/update`), `permission`, `done` (a prompt ended), `stderr`, `exited`.
    pub kind: String,
    /// JSON text.
    pub data: String,
}

fn app_sink(app: &AppHandle) -> Sink {
    let app = app.clone();
    Arc::new(move |id: &str, kind: &str, data: Value| {
        let _ = app.emit("acp-event", AcpEvent { id: id.to_string(), kind: kind.to_string(), data: data.to_string() });
    })
}

fn sink_of(id: &str) -> Option<Sink> {
    live().lock().expect("acp poisoned").get(id).map(|one| one.sink.clone())
}

fn write_to(stdin: &Mutex<ChildStdin>, message: &Value) -> Result<(), String> {
    let mut line = message.to_string();
    line.push('\n');
    let mut stdin = stdin.lock().expect("acp stdin poisoned");
    stdin.write_all(line.as_bytes()).map_err(|error| format!("Could not send to the agent: {error}"))?;
    stdin.flush().map_err(|error| error.to_string())
}

fn stdin_of(id: &str) -> Result<Arc<Mutex<ChildStdin>>, String> {
    let all = live().lock().expect("acp poisoned");
    let one = all.get(id).filter(|one| one.alive).ok_or("The agent has stopped. Please open it again.")?;
    Ok(one.stdin.clone())
}

fn send(id: &str, message: &Value) -> Result<(), String> {
    let stdin = stdin_of(id)?;
    write_to(&stdin, message)
}

fn notify(id: &str, method: &str, params: Value) -> Result<(), String> {
    send(id, &json!({ "jsonrpc": "2.0", "method": method, "params": params }))
}

fn request(id: &str, method: &str, params: Value, timeout: Option<Duration>) -> Result<Value, RpcError> {
    let failed = |message: String| RpcError { code: 0, message };
    let stdin = stdin_of(id).map_err(failed)?;
    let number = next_id();
    let (tell, hear) = channel();
    waiters().lock().expect("acp waiters poisoned").insert((id.to_string(), number), tell);
    let sent = write_to(&stdin, &json!({ "jsonrpc": "2.0", "id": number, "method": method, "params": params }));
    let answer = match sent {
        Err(error) => Err(failed(error)),
        Ok(()) => match timeout {
            Some(limit) => hear.recv_timeout(limit).map_err(|error| match error {
                std::sync::mpsc::RecvTimeoutError::Timeout => failed(format!("The agent didn't answer {method} in time.")),
                std::sync::mpsc::RecvTimeoutError::Disconnected => failed("The agent has stopped.".into()),
            }),
            None => hear.recv().map_err(|_| failed("The agent has stopped.".into())),
        }
        .and_then(|result| result),
    };
    waiters().lock().expect("acp waiters poisoned").remove(&(id.to_string(), number));
    answer
}

fn reply(id: &str, request: &Value, result: Result<Value, (i64, &str)>) -> Result<(), String> {
    let message = match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": request, "result": value }),
        Err((code, text)) => json!({ "jsonrpc": "2.0", "id": request, "error": { "code": code, "message": text } }),
    };
    send(id, &message)
}

fn request_key(id: &Value) -> String {
    id.to_string()
}

/// Handles one line from the agent: answers go to whoever waits, updates and questions go to the screen.
fn handle_line(sink: &Sink, key: &str, line: &str) {
    match classify(line) {
        Incoming::Response { id, result } => {
            if let Some(tell) = waiters().lock().expect("acp waiters poisoned").remove(&(key.to_string(), id)) {
                let _ = tell.send(result);
            }
        }
        Incoming::Notification { method, params } => {
            if method == "session/update" {
                sink(key, "update", params);
            }
        }
        Incoming::Request { id, method, params } => {
            if method == "session/request_permission" {
                let shown = json!({ "requestId": request_key(&id), "request": params });
                if let Some(one) = live().lock().expect("acp poisoned").get_mut(key) {
                    one.pending.push((request_key(&id), id, shown.to_string()));
                }
                sink(key, "permission", shown);
            } else if method.starts_with("fs/") || method.starts_with("terminal/") {
                // Files and terminals run off the reading thread: waiting for a command must not stall the agent's other lines.
                let roots = live().lock().expect("acp poisoned").get(key).map(|one| one.setup.roots.clone()).unwrap_or_default();
                let key = key.to_string();
                std::thread::spawn(move || {
                    let answer = crate::acp_host::handle(&key, &roots, &method, &params)
                        .unwrap_or_else(|| Err("Monolab doesn't support this.".into()));
                    let _ = match answer {
                        Ok(value) => reply(&key, &id, Ok(value)),
                        Err(error) => reply(&key, &id, Err((INTERNAL_ERROR, error.as_str()))),
                    };
                });
            } else {
                let _ = reply(key, &id, Err((METHOD_NOT_FOUND, "Monolab doesn't support this.")));
            }
        }
        Incoming::Junk => {}
    }
}

fn spawn(sink: Sink, setup: Setup, server: &str, agent: &str, launch: &Launch, claude_config: Option<PathBuf>) -> Result<String, String> {
    let dir = setup.dir.clone();
    let is_cmd = launch.program.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"));
    let mut command = if is_cmd {
        let mut one = crate::hidden_command("cmd");
        one.arg("/c").arg(&launch.program);
        one
    } else {
        crate::hidden_command(&launch.program)
    };
    command.args(&launch.args);
    for (name, value) in &launch.env {
        command.env(name, value);
    }
    command.current_dir(&dir);
    command.env_remove("CLAUDECODE");
    command.env_remove("CLAUDE_CODE_ENTRYPOINT");
    command.env_remove("CLAUDE_CODE_CHILD_SESSION");
    if let Some(account) = claude_config {
        command.env("CLAUDE_CONFIG_DIR", account);
    }
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| format!("Could not start the agent: {error}"))?;
    let stdin = child.stdin.take().ok_or("Could not open the agent's input.")?;
    let stdout = child.stdout.take().ok_or("Could not open the agent's output.")?;
    let stderr = child.stderr.take();

    let id = format!("acp{}", next_id());
    live().lock().expect("acp poisoned").insert(
        id.clone(),
        Agent {
            server: server.to_string(),
            agent: agent.to_string(),
            session: String::new(),
            stdin: Arc::new(Mutex::new(stdin)),
            child,
            alive: true,
            busy: false,
            pending: Vec::new(),
            capabilities: Value::Null,
            briefing: None,
            sink: sink.clone(),
            setup: Arc::new(setup),
        },
    );
    if let Some(stderr) = stderr {
        let (sink, key) = (sink.clone(), id.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let text = line.trim();
                if !text.is_empty() {
                    sink(&key, "stderr", json!(text));
                }
            }
        });
    }
    let key = id.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if !line.trim().is_empty() {
                handle_line(&sink, &key, &line);
            }
        }
        if let Some(one) = live().lock().expect("acp poisoned").get_mut(&key) {
            one.alive = false;
            one.busy = false;
            one.pending.clear();
            let _ = one.child.wait();
        }
        // Dropping the senders wakes everyone still waiting on this agent.
        waiters().lock().expect("acp waiters poisoned").retain(|(owner, _), _| owner != &key);
        sink(&key, "exited", Value::Null);
    });
    Ok(id)
}

// ---------- sessions ----------

#[derive(Serialize, Clone, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AcpOpened {
    pub id: String,
    /// Empty when the agent wants a sign-in first; pick one of `auth_methods` and call authenticate.
    pub session_id: String,
    /// The agent's `agentInfo`, modes and config options, as JSON text.
    pub agent_info: String,
    pub modes: String,
    pub config_options: String,
    /// JSON array of the agent's `authMethods`.
    pub auth_methods: String,
    pub can_load: bool,
}

#[derive(Serialize, Clone, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AcpLive {
    pub id: String,
    pub agent: String,
    pub session_id: String,
    pub busy: bool,
    pub pending: Vec<String>,
}

/// The Monolab tools for this server: over http when the agent takes it, otherwise through the app's own stdio bridge.
fn mcp_servers(url: &str, token: &str, exe: &Path, capabilities: &Value) -> Value {
    let http = capabilities.pointer("/mcpCapabilities/http").and_then(Value::as_bool).unwrap_or(false);
    if http {
        return json!([{ "type": "http", "name": "monolab", "url": url,
            "headers": [{ "name": "Authorization", "value": format!("Bearer {token}") }] }]);
    }
    json!([{ "name": "monolab", "command": exe.to_string_lossy(), "args": [MCP_STDIO_FLAG, url],
        "env": [{ "name": MCP_TOKEN_ENV, "value": token }] }])
}

fn session_params(setup: &Setup, capabilities: &Value) -> Result<(Value, bool), String> {
    let mut params = json!({ "cwd": setup.dir.to_string_lossy(), "mcpServers": (setup.mcp)(capabilities)? });
    let takes_prompt = capabilities.pointer("/sessionCapabilities/systemPrompt").is_some_and(|one| !one.is_null());
    if takes_prompt {
        params["systemPrompt"] = json!({ "mode": "append", "content": [{ "type": "text", "text": setup.briefing }] });
    }
    Ok((params, takes_prompt))
}

fn make_room(server: &str) {
    let mut ids: Vec<(String, bool)> = live()
        .lock()
        .expect("acp poisoned")
        .iter()
        .filter(|(_, one)| one.server == server)
        .map(|(id, one)| (id.clone(), one.alive && one.busy))
        .collect();
    ids.sort_by_key(|(id, _)| id.trim_start_matches("acp").parse::<u64>().unwrap_or(0));
    while ids.len() >= LIVE_AGENTS {
        let at = ids.iter().position(|(_, busy)| !busy).unwrap_or(0);
        close(&ids.remove(at).0);
    }
}

/// Starts `agent` for `server`. `session` empty starts a new conversation, otherwise loads that one when the agent can.
pub fn open(app: &AppHandle, server: &str, agent: &str, session: &str) -> Result<AcpOpened, String> {
    let instance = crate::team_agent::instance_for(app, server)?;
    let dir = claude_agent::workspace(app, server)?;
    if session.trim().is_empty() {
        crate::team_agent::new_chat(server);
    }
    let binaries = crate::acp_binary::root(app).ok();
    let launch = launch_for(&registry(app, false)?, agent, npx().as_deref(), uvx().as_deref(), binaries.as_deref())?;
    let (url, token) = crate::mcp::ensure_running(app)?;
    let url = format!("{url}?server={server}");
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    // A server on this computer: its folder too (Rewind keeps what the agent changes there).
    let local = crate::team_agent::place_of(&instance.id).is_none() && instance.remote_host.is_empty() && instance.panel_host.is_empty();
    let mut roots = vec![dir.clone()];
    if local && !instance.path.is_empty() && Path::new(&instance.path).is_dir() {
        roots.push(PathBuf::from(&instance.path));
    }
    let setup = Setup {
        briefing: claude_agent::briefing_for(app, &instance, &dir),
        dir,
        roots,
        mcp: Box::new(move |capabilities| Ok(mcp_servers(&url, &token, &exe, capabilities))),
    };
    make_room(server);
    let claude_config = crate::claude_accounts::config_for_server(app, server);
    let id = spawn(app_sink(app), setup, server, agent, &launch, claude_config)?;
    match start_session(&id, session.trim()) {
        Ok(opened) => Ok(opened),
        Err(error) => {
            close(&id);
            Err(error)
        }
    }
}

fn setup_of(id: &str) -> Result<(Arc<Setup>, Value), String> {
    let all = live().lock().expect("acp poisoned");
    let one = all.get(id).filter(|one| one.alive).ok_or("The agent has stopped. Please open it again.")?;
    Ok((one.setup.clone(), one.capabilities.clone()))
}

fn start_session(id: &str, session: &str) -> Result<AcpOpened, String> {
    let hello = request(
        id,
        "initialize",
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "clientCapabilities": { "fs": { "readTextFile": true, "writeTextFile": true }, "terminal": true },
            "clientInfo": { "name": "monolab", "title": "Monolab", "version": env!("CARGO_PKG_VERSION") },
        }),
        Some(START_TIMEOUT),
    )?;
    let version = hello.get("protocolVersion").and_then(Value::as_u64).unwrap_or(0);
    if version != PROTOCOL_VERSION {
        return Err(format!("This agent speaks ACP version {version}, but Monolab supports version {PROTOCOL_VERSION}."));
    }
    let capabilities = hello.get("agentCapabilities").cloned().unwrap_or_else(|| json!({}));
    let can_load = capabilities.get("loadSession").and_then(Value::as_bool).unwrap_or(false);
    let auth_methods = hello.get("authMethods").cloned().unwrap_or_else(|| json!([]));
    if let Some(one) = live().lock().expect("acp poisoned").get_mut(id) {
        one.capabilities = capabilities.clone();
    }
    let mut opened = AcpOpened {
        id: id.to_string(),
        session_id: String::new(),
        agent_info: hello.get("agentInfo").cloned().unwrap_or(Value::Null).to_string(),
        modes: "null".into(),
        config_options: "null".into(),
        auth_methods: auth_methods.to_string(),
        can_load,
    };
    let wanted = if can_load { session } else { "" };
    let first = new_session(id, wanted);
    // The agent may not have the old conversation any more (another folder, cleared, expired): it answers
    // "Resource not found". Start a new one; the screen keeps the saved messages and says it starts fresh.
    let started = match first {
        Err(error) if !wanted.is_empty() && error.code != AUTH_REQUIRED => new_session(id, ""),
        other => other,
    };
    match started {
        Ok(started) => {
            opened.session_id = started.0;
            opened.modes = started.1.to_string();
            opened.config_options = started.2.to_string();
            Ok(opened)
        }
        // Signing in is the user's call; the screen offers the agent's methods and then calls authenticate.
        Err(error) if error.code == AUTH_REQUIRED => Ok(opened),
        Err(error) => Err(error.into()),
    }
}

fn new_session(id: &str, session: &str) -> Result<(String, Value, Value), RpcError> {
    let failed = |message: String| RpcError { code: 0, message };
    let (setup, capabilities) = setup_of(id).map_err(failed)?;
    let (mut params, takes_prompt) = session_params(&setup, &capabilities).map_err(failed)?;
    let answer = if session.is_empty() {
        request(id, "session/new", params, Some(START_TIMEOUT))?
    } else {
        params["sessionId"] = json!(session);
        request(id, "session/load", params, Some(START_TIMEOUT))?
    };
    let session_id = if session.is_empty() { text(&answer, "sessionId") } else { session.to_string() };
    if session_id.is_empty() {
        return Err(failed("The agent didn't start a conversation.".into()));
    }
    if let Some(one) = live().lock().expect("acp poisoned").get_mut(id) {
        one.session = session_id.clone();
        one.briefing = (!takes_prompt && session.is_empty()).then(|| setup.briefing.clone());
    }
    let modes = answer.get("modes").cloned().unwrap_or(Value::Null);
    let options = answer.get("configOptions").cloned().unwrap_or(Value::Null);
    Ok((session_id, modes, options))
}

/// Signs in with one of the agent's `type`-less (agent-handled) methods, then starts the conversation.
pub fn authenticate(id: &str, method: &str) -> Result<AcpOpened, String> {
    let (_, capabilities) = setup_of(id)?;
    request(id, "authenticate", json!({ "methodId": method }), Some(START_TIMEOUT))?;
    let (session_id, modes, options) = new_session(id, "")?;
    Ok(AcpOpened {
        id: id.to_string(),
        session_id,
        agent_info: "null".into(),
        modes: modes.to_string(),
        config_options: options.to_string(),
        auth_methods: "[]".into(),
        can_load: capabilities.get("loadSession").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn session_of(id: &str) -> Result<String, String> {
    let all = live().lock().expect("acp poisoned");
    let one = all.get(id).filter(|one| one.alive).ok_or("The agent has stopped. Please open it again.")?;
    if one.session.is_empty() {
        return Err("Sign in to the agent first.".into());
    }
    Ok(one.session.clone())
}

/// `content` is a JSON array of ACP content blocks. Returns at once; the end of the turn arrives as a `done` event.
pub fn prompt(id: &str, content: &str) -> Result<(), String> {
    let mut blocks: Vec<Value> = serde_json::from_str(content).map_err(|error| format!("The content to send is invalid: {error}"))?;
    if blocks.is_empty() || !blocks.iter().all(|one| one.get("type").and_then(Value::as_str).is_some()) {
        return Err("The content to send is invalid.".into());
    }
    let session = session_of(id)?;
    {
        let mut all = live().lock().expect("acp poisoned");
        let one = all.get_mut(id).ok_or("The agent has stopped. Please open it again.")?;
        if one.busy {
            return Err("The agent is still working. Wait for it, or stop it first.".into());
        }
        if let Some(briefing) = one.briefing.take() {
            blocks.insert(0, json!({ "type": "text", "text": briefing }));
        }
        one.busy = true;
    }
    let sink = sink_of(id).ok_or("The agent has stopped. Please open it again.")?;
    let key = id.to_string();
    std::thread::spawn(move || {
        let answer = request(&key, "session/prompt", json!({ "sessionId": session, "prompt": blocks }), None);
        if let Some(one) = live().lock().expect("acp poisoned").get_mut(&key) {
            one.busy = false;
        }
        let data = match answer {
            Ok(result) => json!({ "stopReason": text(&result, "stopReason") }),
            Err(error) => json!({ "error": error.message }),
        };
        sink(&key, "done", data);
    });
    Ok(())
}

/// Stops the current turn. Unanswered permission requests are answered `cancelled`, as the protocol asks.
pub fn cancel(id: &str) -> Result<(), String> {
    let session = session_of(id)?;
    let waiting: Vec<Value> = live()
        .lock()
        .expect("acp poisoned")
        .get_mut(id)
        .map(|one| one.pending.drain(..).map(|(_, request, _)| request).collect())
        .unwrap_or_default();
    for request in waiting {
        let _ = reply(id, &request, Ok(json!({ "outcome": { "outcome": "cancelled" } })));
    }
    notify(id, "session/cancel", json!({ "sessionId": session }))
}

/// Answers a permission request with one of its `optionId`s, or `None` to dismiss it.
pub fn answer(id: &str, request_id: &str, option: Option<String>) -> Result<(), String> {
    let request = {
        let mut all = live().lock().expect("acp poisoned");
        let one = all.get_mut(id).ok_or("The agent has stopped. Please open it again.")?;
        let at = one.pending.iter().position(|(key, _, _)| key == request_id).ok_or("That question was already answered.")?;
        one.pending.remove(at).1
    };
    let outcome = match option {
        Some(option) => json!({ "outcome": "selected", "optionId": option }),
        None => json!({ "outcome": "cancelled" }),
    };
    reply(id, &request, Ok(json!({ "outcome": outcome })))
}

pub fn set_mode(id: &str, mode: &str) -> Result<(), String> {
    let session = session_of(id)?;
    request(id, "session/set_mode", json!({ "sessionId": session, "modeId": mode }), Some(CALL_TIMEOUT))?;
    Ok(())
}

/// `value` is JSON: a string picks a select option, a boolean flips a toggle. Returns the agent's new options as JSON.
pub fn set_config(id: &str, config: &str, value: &str) -> Result<String, String> {
    let session = session_of(id)?;
    let value: Value = serde_json::from_str(value).map_err(|error| error.to_string())?;
    let mut params = json!({ "sessionId": session, "configId": config, "value": value });
    match value {
        Value::Bool(_) => params["type"] = json!("boolean"),
        Value::String(_) => {}
        _ => return Err("Unknown setting value.".into()),
    }
    let answer = request(id, "session/set_config_option", params, Some(CALL_TIMEOUT))?;
    Ok(answer.get("configOptions").cloned().unwrap_or(Value::Null).to_string())
}

pub fn state(server: &str) -> Vec<AcpLive> {
    let all = live().lock().expect("acp poisoned");
    let mut list: Vec<AcpLive> = all
        .iter()
        .filter(|(_, one)| one.server == server && one.alive)
        .map(|(id, one)| AcpLive {
            id: id.clone(),
            agent: one.agent.clone(),
            session_id: one.session.clone(),
            busy: one.busy,
            pending: one.pending.iter().map(|(_, _, line)| line.clone()).collect(),
        })
        .collect();
    list.sort_by(|a, b| a.id.cmp(&b.id));
    list
}

pub fn close(id: &str) {
    crate::acp_host::release_all(id);
    if let Some(mut one) = live().lock().expect("acp poisoned").remove(id) {
        // npx runs the agent under cmd and node, so take the whole tree down.
        crate::kill_process_tree(one.child.id());
        let _ = one.child.kill();
    }
}

pub fn close_server(server: &str) {
    let ids: Vec<String> =
        live().lock().expect("acp poisoned").iter().filter(|(_, one)| one.server == server).map(|(id, _)| id.clone()).collect();
    for id in ids {
        close(&id);
    }
}

pub fn close_all() {
    let ids: Vec<String> = live().lock().expect("acp poisoned").keys().cloned().collect();
    for id in ids {
        close(&id);
    }
}

// ---------- MCP stdio bridge ----------

/// `monolab.exe --mcp-stdio <url>` relays MCP messages between stdio and the app's local MCP endpoint,
/// for agents that only take stdio MCP servers. The token comes in the environment, never on the command line.
pub const MCP_STDIO_FLAG: &str = "--mcp-stdio";
pub const MCP_TOKEN_ENV: &str = "MONOLAB_MCP_TOKEN";

fn bridge_url_ok(url: &str) -> bool {
    reqwest::Url::parse(url)
        .is_ok_and(|url| url.scheme() == "http" && url.host_str() == Some("127.0.0.1") && url.username().is_empty() && url.path() == "/mcp")
}

pub fn mcp_stdio_bridge(url: &str) -> i32 {
    if !bridge_url_ok(url) {
        eprintln!("monolab: the MCP bridge only talks to this computer");
        return 2;
    }
    let token = std::env::var(MCP_TOKEN_ENV).unwrap_or_default();
    let Ok(client) = reqwest::blocking::Client::builder().no_proxy().timeout(Duration::from_secs(600)).build() else { return 1 };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let answer = client
            .post(url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(line.to_string())
            .send()
            .and_then(|response| response.text());
        let body = match answer {
            Ok(body) => body,
            Err(error) => {
                eprintln!("monolab: {error}");
                // Requests still deserve an answer so the agent doesn't hang.
                let id = serde_json::from_str::<Value>(line).ok().and_then(|value| value.get("id").cloned());
                match id {
                    Some(id) if !id.is_null() => {
                        json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": "Monolab isn't running." } }).to_string()
                    }
                    _ => continue,
                }
            }
        };
        let body = body.trim();
        if body.is_empty() {
            continue;
        }
        // One line per message: compact whatever came back.
        let compact = serde_json::from_str::<Value>(body).map(|value| value.to_string()).unwrap_or_else(|_| body.replace('\n', " "));
        if writeln!(stdout, "{compact}").and_then(|()| stdout.flush()).is_err() {
            break;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({ "version": "1.0.0", "agents": [
            { "id": "gemini", "name": "Gemini CLI", "version": "0.68.0", "description": "d",
              "distribution": { "npx": { "package": "@google/gemini-cli@0.68.0", "args": ["--acp"] } } },
            { "id": "both", "name": "Both", "version": "1",
              "distribution": { "npx": { "package": "both@1", "env": { "A": "b" } },
                "binary": { (platform()): { "archive": "https://x/y.zip", "cmd": "./both.exe" } } } },
            { "id": "bin", "name": "Bin", "version": "1",
              "distribution": { "binary": { (platform()): { "archive": "https://x/y.zip", "cmd": "./bin.exe" } } } },
            { "id": "elsewhere", "name": "Elsewhere", "version": "1",
              "distribution": { "binary": { "plan9-mips": { "archive": "https://x", "cmd": "./x" } } } },
            { "id": "py", "name": "Py", "version": "1", "distribution": { "uvx": { "package": "py-acp==1" } } },
        ] })
    }

    #[test]
    fn lists_agents_that_could_run_here() {
        let list = agent_list(&sample(), true, false, None);
        let ids: Vec<&str> = list.iter().map(|one| one.id.as_str()).collect();
        assert_eq!(ids, ["gemini", "both", "bin", "py"]);
        assert!(list[0].ready && list[0].launch == "npx");
        assert!(!list[2].ready && list[2].launch == "binary");
        assert!(!list[3].ready && list[3].launch == "uvx");
        assert!(agent_list(&sample(), false, false, None).iter().all(|one| !one.ready));
        assert!(list[2].download && list[1].download && !list[0].download);
    }

    #[test]
    fn builds_the_launch_from_the_registry_only() {
        let npx = PathBuf::from("C:/node/npx.cmd");
        let launch = launch_for(&sample(), "gemini", Some(&npx), None, None).unwrap();
        assert_eq!(launch.program, npx);
        assert_eq!(launch.args, ["--yes", "@google/gemini-cli@0.68.0", "--acp"]);
        let both = launch_for(&sample(), "both", Some(&npx), None, None).unwrap();
        assert_eq!(both.env, [("A".to_string(), "b".to_string())]);
        assert!(launch_for(&sample(), "gemini", None, None, None).unwrap_err().contains("Node.js"));
        assert!(launch_for(&sample(), "bin", Some(&npx), None, None).unwrap_err().contains("Download"));
        assert!(launch_for(&sample(), "nope", Some(&npx), None, None).is_err());
        let uv = PathBuf::from("C:/uv/uvx.exe");
        assert_eq!(launch_for(&sample(), "py", None, Some(&uv), None).unwrap().args, ["py-acp==1"]);
    }

    #[test]
    fn tells_messages_apart() {
        assert_eq!(
            classify(r#"{"jsonrpc":"2.0","id":3,"result":{"sessionId":"s"}}"#),
            Incoming::Response { id: 3, result: Ok(json!({ "sessionId": "s" })) }
        );
        assert_eq!(
            classify(r#"{"jsonrpc":"2.0","id":4,"error":{"code":-32000,"message":"auth"}}"#),
            Incoming::Response { id: 4, result: Err(RpcError { code: -32000, message: "auth".into() }) }
        );
        assert!(matches!(
            classify(r#"{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{}}"#),
            Incoming::Request { method, .. } if method == "session/request_permission"
        ));
        assert!(matches!(
            classify(r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s"}}"#),
            Incoming::Notification { method, .. } if method == "session/update"
        ));
        assert_eq!(classify(r#"{"jsonrpc":"2.0","id":"x","result":1}"#), Incoming::Junk);
        assert_eq!(classify("npm WARN something"), Incoming::Junk);
    }

    /// Runs a real registry agent (downloads it with npx) through the same code the app uses, without MCP tools:
    /// `ACP_AGENT=gemini [ACP_AUTH=<methodId>] cargo test real_agent -- --ignored --nocapture`
    #[test]
    #[ignore = "starts a real agent from npm"]
    fn real_agent_round_trip() {
        let agent = std::env::var("ACP_AGENT").unwrap_or_else(|_| "gemini".into());
        let text_of = |url: &str| reqwest::blocking::get(url).and_then(|one| one.text()).expect("registry");
        let registry: Value = serde_json::from_str(&text_of(REGISTRY_URL)).expect("registry json");
        let launch = launch_for(&registry, &agent, npx().as_deref(), uvx().as_deref(), None).expect("launch");
        eprintln!("launch: {} {:?}", launch.program.display(), launch.args);
        let dir = std::env::temp_dir().join(format!("monolab-acp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let events: Arc<Mutex<Vec<(String, Value)>>> = Arc::default();
        let seen = events.clone();
        let sink: Sink = Arc::new(move |_, kind: &str, data: Value| {
            let shown: String = data.to_string().chars().take(300).collect();
            eprintln!("[{kind}] {shown}");
            seen.lock().unwrap().push((kind.to_string(), data));
        });
        let setup = Setup {
            dir: dir.clone(),
            roots: vec![dir.clone()],
            briefing: "You are being tested. Keep answers short.".into(),
            mcp: Box::new(|_| Ok(json!([]))),
        };
        let id = spawn(sink, setup, "test", &agent, &launch, None).expect("spawn");
        let mut opened = start_session(&id, "").expect("start");
        eprintln!("agentInfo {} | authMethods {} | session {:?}", opened.agent_info, opened.auth_methods, opened.session_id);
        eprintln!("modes {} | configOptions {}", opened.modes, opened.config_options);
        if opened.session_id.is_empty() {
            let Ok(method) = std::env::var("ACP_AUTH") else {
                eprintln!("SIGN-IN NEEDED: set ACP_AUTH to one of the methods above");
                close(&id);
                return;
            };
            opened = authenticate(&id, &method).expect("authenticate");
            eprintln!("signed in, session {}", opened.session_id);
        }
        prompt(&id, r#"[{"type":"text","text":"Reply with exactly the word: pong"}]"#).expect("prompt");
        let started = std::time::Instant::now();
        let done = loop {
            if let Some((_, data)) = events.lock().unwrap().iter().find(|(kind, _)| kind == "done") {
                break data.clone();
            }
            assert!(started.elapsed() < Duration::from_secs(240), "no answer in time");
            std::thread::sleep(Duration::from_millis(200));
        };
        let reply: String = events
            .lock()
            .unwrap()
            .iter()
            .filter(|(kind, data)| {
                kind == "update" && data.pointer("/update/sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk")
            })
            .filter_map(|(_, data)| data.pointer("/update/content/text").and_then(Value::as_str).map(str::to_string))
            .collect();
        eprintln!("done {done} | reply {reply:?}");
        close(&id);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(done["stopReason"], "end_turn");
        assert!(reply.to_lowercase().contains("pong"));
    }

    #[test]
    fn bridge_only_reaches_this_computer() {
        assert!(bridge_url_ok("http://127.0.0.1:25599/mcp?server=a"));
        assert!(!bridge_url_ok("http://example.com/mcp"));
        assert!(!bridge_url_ok("https://127.0.0.1:1/mcp"));
        assert!(!bridge_url_ok("http://127.0.0.1:1@example.com/mcp"));
    }
}

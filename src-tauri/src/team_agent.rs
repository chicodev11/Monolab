//! Teammates' agents and the server's shared `AGENTS.md` (TODO sections 22 and 23-B).
//!
//! **Teammate side.** A teammate's agent runs on their own PC with their own Claude, Codex or ACP
//! sign-in. For a server shared through a Monolink team it works on a server reference
//! (`team~<team>~<host device>~<server id>`) instead of a local server id; its Monolab tools reach the
//! host through `direct::call_team` with the team's role grant, marked `agent: true`. So it can only
//! do what the teammate's role allows, and the host decides the rest.
//!
//! **Host side.** Agent requests pass [`decide`]: refused unless the host let teammates' agents in for
//! that server (all teammates, or listed ones), refused when the role doesn't allow the command, read
//! commands go straight through, and anything that changes something waits for the host to answer in
//! the main window (Allow, Deny, or Allow for this chat).
//!
//! **Shared rules.** `AGENTS.md` in the server folder holds the team's rules for that server. Everyone
//! with access reads it ([`read_server_agents_md`]); only the host, web panel admins and team Admins or
//! the Owner change it. Every agent, the host's own and teammates', gets it at the start of a chat.

use std::collections::HashMap;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use relay_proto::grant::{Grant, Role};
use relay_proto::DeviceId;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

/// How a server reference starts.
const PREFIX: &str = "team~";
/// The file name, in the top of the server folder.
pub const RULES_FILE: &str = "AGENTS.md";
/// Longest shared AGENTS.md that is saved, and the most of it an agent is handed.
const RULES_MAX: usize = 64 * 1024;
const RULES_IN_BRIEFING: usize = 16 * 1024;
/// How long the host has to answer an agent's request.
const ASK_WAIT: Duration = Duration::from_secs(5 * 60);
/// How long a teammate's app waits for an agent request (the host may be deciding).
const CALL_WAIT: Duration = Duration::from_secs(6 * 60);
/// "Allow for this chat" lasts at most this long.
const CHAT_ALLOW_FOR: Duration = Duration::from_secs(12 * 60 * 60);
/// What a teammate's app remembers about a shared server before asking the host again.
const INFO_FRESH: Duration = Duration::from_secs(5 * 60);

// ── server references ──────────────────────────────────────────────────────

/// A server on a teammate's PC, reached through a Monolink team.
#[derive(Clone, Debug, PartialEq)]
pub struct TeamRef {
    pub team: String,
    pub host: DeviceId,
    pub instance: String,
}

fn plain(part: &str) -> bool {
    !part.is_empty() && part.len() <= 64 && part.chars().all(|one| one.is_ascii_alphanumeric() || one == '-' || one == '_')
}

/// The reference for a shared server, or None for a local server id.
pub fn parse(server: &str) -> Option<TeamRef> {
    let rest = server.strip_prefix(PREFIX)?;
    let mut parts = rest.split('~');
    let (team, host, instance) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || !plain(team) || !plain(host) || !plain(instance) {
        return None;
    }
    Some(TeamRef { team: team.into(), host: DeviceId::parse(host)?, instance: instance.into() })
}

/// The reference text for a shared server (the screens build the same, in `teamServers.ts`).
#[cfg(test)]
pub fn make(team: &str, host: &DeviceId, instance: &str) -> String {
    format!("{PREFIX}{team}~{}~{instance}", host.text())
}

// ── teammate side ──────────────────────────────────────────────────────────

/// The current chat of each shared server on this PC: "Allow for this chat" on the host is tied to it.
fn chats() -> &'static Mutex<HashMap<String, String>> {
    static CHATS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    CHATS.get_or_init(Default::default)
}

fn nonce() -> String {
    let mut bytes = [0u8; 12];
    let _ = getrandom::fill(&mut bytes);
    relay_proto::hex(&bytes)
}

fn chat_of(server: &str) -> String {
    chats().lock().unwrap_or_else(|poison| poison.into_inner()).entry(server.to_string()).or_insert_with(nonce).clone()
}

/// A new conversation started on `server`: what the host allowed for the last one doesn't carry over.
pub fn new_chat(server: &str) {
    if parse(server).is_some() {
        chats().lock().unwrap_or_else(|poison| poison.into_inner()).insert(server.to_string(), nonce());
    }
}

/// The body an agent request carries: the arguments plus the agent marker and the chat.
fn marked(body: Value, chat: &str) -> Value {
    let mut body = match body {
        Value::Object(map) => Value::Object(map),
        _ => json!({}),
    };
    body["agent"] = json!(true);
    body["chat"] = json!(chat);
    body
}

/// Runs a future to the end from any thread, async runtime or not.
fn block<T: Send>(work: impl std::future::Future<Output = T> + Send) -> T {
    std::thread::scope(|scope| scope.spawn(|| tauri::async_runtime::block_on(work)).join().expect("team agent call panicked"))
}

/// One Monolab tool request from an agent on this PC to the host of a shared server.
pub fn call(app: &AppHandle, server: &str, command: &str, body: Value) -> Result<Value, String> {
    let target = parse(server).ok_or("Unknown shared server.")?;
    if !crate::monolink_account::is_signed_in(app) {
        return Err("Sign in to Monolink first (Monolink page, Account).".into());
    }
    let body = marked(body, &chat_of(server));
    block(crate::direct::call_team(app, &target.team, target.host, command, &body, CALL_WAIT))
}

/// When this PC last heard about a shared server, and its name, core and version.
type Heard = (Instant, String, String, String);

fn known() -> &'static Mutex<HashMap<String, Heard>> {
    static KNOWN: OnceLock<Mutex<HashMap<String, Heard>>> = OnceLock::new();
    KNOWN.get_or_init(Default::default)
}

/// A local server, or a shared one described the way agent screens and briefings need it.
pub(crate) fn instance_for(app: &AppHandle, server: &str) -> Result<crate::ServerInstance, String> {
    let Some(target) = parse(server) else { return crate::instance_by_id(app, server) };
    let cached = known().lock().unwrap_or_else(|poison| poison.into_inner()).get(server).cloned();
    let (name, core, version) = match cached {
        Some((at, name, core, version)) if at.elapsed() < INFO_FRESH => (name, core, version),
        _ => match call(app, server, "list_instances", json!({})) {
            Ok(list) => {
                let one = list
                    .as_array()
                    .and_then(|all| all.iter().find(|one| one["id"] == target.instance.as_str()).cloned())
                    .ok_or("This server isn't shared with you any more.")?;
                let field = |key: &str| one[key].as_str().unwrap_or_default().to_string();
                let found = (field("name"), field("core"), field("version"));
                known().lock().unwrap_or_else(|poison| poison.into_inner()).insert(server.to_string(), (Instant::now(), found.0.clone(), found.1.clone(), found.2.clone()));
                found
            }
            // The host is away: what was heard before still names the chat folder.
            Err(error) => match cached {
                Some((_, name, core, version)) => (name, core, version),
                None => return Err(error),
            },
        },
    };
    shared_instance(server, &name, &core, &version)
}

fn shared_instance(server: &str, name: &str, core: &str, version: &str) -> Result<crate::ServerInstance, String> {
    serde_json::from_value(json!({
        "id": server,
        "name": if name.trim().is_empty() { "Team server" } else { name },
        "path": "",
        "core": core,
        "version": version,
        "serverJar": null,
        "javaPath": "",
        "maxMemoryMib": 0,
        "jvmArgs": "",
        "port": 0,
        "createdAt": 0,
    }))
    .map_err(|error| error.to_string())
}

/// What the briefing says about where a shared server is and what the agent may do there.
pub fn place_of(server: &str) -> Option<String> {
    parse(server).map(|_| {
        "a server on a teammate's PC, shared with this user through a Monolink team (its files live on the host's PC, not on this computer). \
The `monolab` tools reach it through the host with this user's team role: some tools may be refused for that role, and every change waits until the host allows it, \
so say what you want to change and why before you call a tool that changes something. add_addon_file sends the file from this computer to the host first; read_memo and update_todo don't work here"
            .to_string()
    })
}

/// The server's shared rules (its `AGENTS.md`) for the start of a chat, or "" when there are none or
/// they can't be read right now.
pub fn rules_note(app: &AppHandle, server: &str) -> String {
    let text = match parse(server) {
        Some(_) => call(app, server, "read_server_agents_md", json!({ "id": parse(server).map(|one| one.instance).unwrap_or_default() })).ok().and_then(|value| value.as_str().map(str::to_string)),
        None => read_rules(app, server).ok(),
    };
    briefing_rules(&text.unwrap_or_default())
}

/// The rules as the agent is told them.
pub fn briefing_rules(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    let mut shown: String = text.chars().take(RULES_IN_BRIEFING).collect();
    if shown.len() < text.len() {
        shown.push_str("\n… (cut short; read the rest with read_file AGENTS.md)");
    }
    format!(
        "\n\nThe team's rules for this server, from AGENTS.md in the server folder (shared by everyone who works on it). Follow them in this chat. \
They are the server's own file, not your notes: don't change it unless the user asks.\n<server-rules>\n{shown}\n</server-rules>"
    )
}

// ── host side: the gate ────────────────────────────────────────────────────

/// What happens to one agent request on the host.
#[derive(Debug, PartialEq)]
pub enum Gate {
    Pass,
    Ask,
    Refuse(String),
}

/// Whether a request says it comes from an agent.
pub fn is_agent(body: &Value) -> bool {
    body.get("agent").and_then(Value::as_bool).unwrap_or(false)
}

/// The request without the agent marker, and the chat it names.
pub fn unmark(body: &Value) -> (Value, String) {
    let mut body = body.clone();
    let mut chat = String::new();
    if let Some(map) = body.as_object_mut() {
        map.remove("agent");
        chat = map.remove("chat").and_then(|one| one.as_str().map(str::to_string)).unwrap_or_default();
    }
    (body, chat.chars().take(64).collect())
}

/// Commands an agent may use without the host answering: they only look.
pub fn is_read(command: &str) -> bool {
    command == "read_server_agents_md" || crate::web::api::is_read(command)
}

/// Whether the host let `handle`'s agent work on a server.
pub fn allowed(link: &crate::monolink_team::TeamServerLink, handle: &str) -> bool {
    link.agents || link.agent_people.iter().any(|one| one.eq_ignore_ascii_case(handle))
}

/// The host's decision for one agent request. `allowed` is the host's switch for this teammate,
/// `admin_only` the server's "commands only from admins", `chat_allowed` whether the host already
/// allowed changes for this chat.
pub fn decide(allowed: bool, admin_only: bool, role: Role, command: &str, body: &Value, chat_allowed: bool) -> Gate {
    if !allowed {
        return Gate::Refuse("The host hasn't let teammates' agents work on this server.".into());
    }
    if let Some(why) = crate::monolink_team::role_refusal(role, command, body, admin_only) {
        return Gate::Refuse(why.into());
    }
    if is_read(command) {
        return Gate::Pass;
    }
    if role == Role::Viewer {
        return Gate::Refuse("A viewer's agent can only look, not change anything.".into());
    }
    // Sending a file in pieces changes nothing by itself (it waits in the uploads folder, swept after a day);
    // the host is asked when the agent uses it (add_addon_files), not once per piece.
    if command.starts_with("__upload_") {
        return Gate::Pass;
    }
    if chat_allowed {
        Gate::Pass
    } else {
        Gate::Ask
    }
}

/// One agent request waiting for the host.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentAsk {
    pub id: String,
    /// `@handle`.
    pub who: String,
    pub role: String,
    pub team: String,
    pub server: String,
    pub server_name: String,
    pub command: String,
    /// The file or folder it is about, if any.
    pub path: String,
    /// A console command or similar short text, if any.
    pub text: String,
    /// The arguments, with secrets left out.
    pub detail: String,
    /// Milliseconds since 1970.
    pub at: f64,
    /// The chat can be allowed as a whole.
    pub can_chat: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Answer {
    Allow,
    Chat,
    Deny,
}

type ChatKey = (String, String, String, String);

#[derive(Default)]
struct Asks {
    waiting: Vec<(AgentAsk, Sender<Answer>, ChatKey)>,
    chats: HashMap<ChatKey, Instant>,
    next: u64,
}

fn asks() -> &'static Mutex<Asks> {
    static ASKS: OnceLock<Mutex<Asks>> = OnceLock::new();
    ASKS.get_or_init(Default::default)
}

fn held() -> std::sync::MutexGuard<'static, Asks> {
    asks().lock().unwrap_or_else(|poison| poison.into_inner())
}

fn chat_allowed(key: &ChatKey) -> bool {
    !key.3.is_empty() && held().chats.get(key).is_some_and(|at| at.elapsed() < CHAT_ALLOW_FOR)
}

fn waiting_list() -> Vec<AgentAsk> {
    held().waiting.iter().map(|(ask, _, _)| ask.clone()).collect()
}

/// A question waits for the host: when the main window is minimized or behind other windows, its taskbar
/// button flashes until the host looks (the question itself shows in the window).
fn call_attention(app: &AppHandle) {
    use tauri::Manager;
    let Some(window) = app.get_window("main") else { return };
    let away = window.is_minimized().unwrap_or(false) || !window.is_focused().unwrap_or(true);
    if away {
        let _ = window.request_user_attention(Some(tauri::UserAttentionType::Critical));
    }
}

fn tell_main(app: &AppHandle) {
    // The main window only: these never go to web panel or Monolink guests.
    let _ = app.emit_to("main", "team-agent-asks", waiting_list());
}

/// Checks an agent request from a teammate and, for a change, waits for the host. Ok means run it.
pub fn gate(app: &AppHandle, grant: &Grant, command: &str, body: &Value, chat: &str) -> Result<(), String> {
    let links = crate::monolink_team::server_links(app, &grant.team);
    let reach = crate::monolink_team::reach(app, grant);
    let named: Vec<String> = body
        .get("id")
        .and_then(Value::as_str)
        .map(|one| vec![one.to_string()])
        .unwrap_or_else(|| crate::web_servers_of(app, command, body))
        .into_iter()
        .filter(|one| !one.is_empty())
        .collect();
    let link = |id: &str| links.iter().find(|one| one.instance == id && reach.iter().any(|mine| mine == id));
    let (is_allowed, admin_only) = if named.is_empty() {
        // About no server (like the server list): fine when any server lets this teammate's agent in.
        (reach.iter().any(|id| link(id).is_some_and(|one| allowed(one, &grant.handle))), false)
    } else {
        (
            named.iter().all(|id| link(id).is_some_and(|one| allowed(one, &grant.handle))),
            named.iter().any(|id| link(id).is_some_and(|one| one.admin_only)),
        )
    };
    let server = named.first().cloned().unwrap_or_default();
    let key: ChatKey = (grant.team.clone(), grant.handle.to_ascii_lowercase(), server.clone(), chat.to_string());
    match decide(is_allowed, admin_only, grant.role, command, body, chat_allowed(&key)) {
        Gate::Pass => Ok(()),
        Gate::Refuse(why) => Err(why),
        Gate::Ask => ask_host(app, grant, command, body, key),
    }
}

fn ask_host(app: &AppHandle, grant: &Grant, command: &str, body: &Value, key: ChatKey) -> Result<(), String> {
    let text = |name: &str| body.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    let server_name = crate::load_instances(app).unwrap_or_default().into_iter().find(|one| one.id == key.2).map(|one| one.name).unwrap_or_default();
    let mut path = text("path");
    if !text("name").is_empty() && matches!(command, "create_server_file" | "create_server_folder") {
        path = if path.is_empty() { text("name") } else { format!("{}/{}", path.trim_end_matches('/'), text("name")) };
    }
    if path.is_empty() {
        path = text("fileName");
    }
    let shown = match command {
        "send_console_command" | "send_rcon_command" => text("command"),
        _ => String::new(),
    };
    let mut detail = crate::teamlog::redact(body).to_string();
    if detail.chars().count() > 1500 {
        detail = format!("{}…", detail.chars().take(1500).collect::<String>());
    }
    let (sender, answer) = channel();
    let id = {
        let mut asks = held();
        asks.next += 1;
        let id = format!("ask{}", asks.next);
        let ask = AgentAsk {
            id: id.clone(),
            who: format!("@{}", grant.handle),
            role: grant.role.as_str().to_string(),
            team: grant.team.clone(),
            server: key.2.clone(),
            server_name,
            command: command.to_string(),
            path,
            text: shown,
            detail,
            at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_millis() as f64).unwrap_or(0.0),
            can_chat: !key.3.is_empty(),
        };
        asks.waiting.push((ask, sender, key.clone()));
        id
    };
    tell_main(app);
    call_attention(app);
    let got = answer.recv_timeout(ASK_WAIT);
    held().waiting.retain(|(one, _, _)| one.id != id);
    tell_main(app);
    match got {
        Ok(Answer::Allow) => Ok(()),
        Ok(Answer::Chat) => {
            held().chats.insert(key, Instant::now());
            Ok(())
        }
        Ok(Answer::Deny) => Err("The host said no.".into()),
        Err(RecvTimeoutError::Timeout) => Err("The host didn't answer in time.".into()),
        Err(RecvTimeoutError::Disconnected) => Err("The host didn't answer.".into()),
    }
}

/// Agent requests waiting for this PC's answer (main window only: not on the web allow list, and
/// Monolink windows can't call app commands).
#[tauri::command]
#[specta::specta]
pub fn team_agent_asks() -> Vec<AgentAsk> {
    waiting_list()
}

/// Answers an agent request: `allow`, `chat` (allow this and the rest of that chat's changes) or `deny`.
#[tauri::command]
#[specta::specta]
pub fn team_agent_answer(id: String, answer: String) -> Result<(), String> {
    let answer = match answer.as_str() {
        "allow" => Answer::Allow,
        "chat" => Answer::Chat,
        "deny" => Answer::Deny,
        _ => return Err("Unknown answer.".into()),
    };
    let asks = held();
    let (_, sender, _) = asks.waiting.iter().find(|(one, _, _)| one.id == id).ok_or("That request isn't waiting any more.")?;
    sender.send(answer).map_err(|_| "That request isn't waiting any more.".to_string())
}

/// Forgets every "Allow for this chat" (the host turned an agent switch off).
pub fn forget_chats(team: &str) {
    held().chats.retain(|key, _| key.0 != team);
}

// ── the shared AGENTS.md ───────────────────────────────────────────────────

fn missing(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    ["not found", "no such file", "cannot find", "can't find", "does not exist", "os error 2", "404"].iter().any(|one| error.contains(one))
}

/// The text of a local server folder's AGENTS.md, "" when there is none.
pub fn read_rules_in(folder: &std::path::Path) -> Result<String, String> {
    match std::fs::read(folder.join(RULES_FILE)) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            Ok(text.chars().take(RULES_MAX).collect())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(format!("Could not read AGENTS.md: {error}")),
    }
}

fn read_rules(app: &AppHandle, id: &str) -> Result<String, String> {
    let instance = crate::instance_by_id(app, id)?;
    if instance.panel_host.is_empty() && instance.remote_host.is_empty() {
        return read_rules_in(std::path::Path::new(&instance.path));
    }
    // Opening a chat reaches here from an async command; a bare block_on inside the runtime panics
    // ("Cannot start a runtime from within a runtime"), so wait on a thread of its own.
    let (app, id) = (app.clone(), id.to_string());
    match crate::off_runtime(move || tauri::async_runtime::block_on(crate::read_editable_file(app, id, RULES_FILE.into()))) {
        Ok(text) => Ok(text.chars().take(RULES_MAX).collect()),
        Err(error) if missing(&error) => Ok(String::new()),
        Err(error) => Err(error),
    }
}

/// The server's shared AGENTS.md, "" when it has none.
#[tauri::command(async)]
#[specta::specta]
pub fn read_server_agents_md(app: AppHandle, id: String) -> Result<String, String> {
    read_rules(&app, &id)
}

/// Creates or replaces the server's shared AGENTS.md. Web panel admins only on the web; a team Admin
/// or Owner through Monolink (checked in `monolink_team`). Recorded in the server's audit log, and
/// in the teammate log when a teammate did it.
#[tauri::command(async)]
#[specta::specta]
pub fn save_server_agents_md(app: AppHandle, id: String, content: String) -> Result<(), String> {
    if content.len() > RULES_MAX {
        return Err("AGENTS.md can be at most 64 KB.".into());
    }
    let instance = crate::instance_by_id(&app, &id)?;
    if instance.panel_host.is_empty() && instance.remote_host.is_empty() {
        std::fs::write(std::path::Path::new(&instance.path).join(RULES_FILE), &content).map_err(|error| format!("Could not save AGENTS.md: {error}"))?;
    } else {
        // Same as reading: this command runs on a runtime thread, so the wait happens on a thread of its own.
        let (app, id, content) = (app.clone(), id.clone(), content.clone());
        crate::off_runtime(move || tauri::async_runtime::block_on(crate::save_editable_file(app.clone(), tauri::Manager::state(&app), id, RULES_FILE.into(), content, None)))?;
    }
    crate::record_audit(&app, &id, "File", "Shared AGENTS.md saved", &format!("{} characters", content.chars().count()));
    Ok(())
}

/// Whether a file request would create, change, move or delete the shared AGENTS.md.
pub fn touches_rules(command: &str, body: &Value) -> bool {
    const FILE_CHANGES: &[&str] = &[
        "save_editable_file",
        "create_server_file",
        "create_server_folder",
        "rename_server_path",
        "trash_server_path",
        "restore_server_trash",
        "unzip_server_path",
        "add_server_uploads",
    ];
    if command == "save_server_agents_md" {
        return true;
    }
    if !FILE_CHANGES.contains(&command) {
        return false;
    }
    let text = |name: &str| body.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    let is_rules = |path: &str| {
        let path = path.replace('\\', "/");
        let path = path.trim().trim_start_matches("./").trim_matches('/');
        path.eq_ignore_ascii_case(RULES_FILE)
    };
    let path = text("path");
    let folder = path.rsplit_once(['/', '\\']).map(|(head, _)| head.to_string()).unwrap_or_default();
    let mut candidates = vec![path.clone(), format!("{path}/{}", text("name")), format!("{folder}/{}", text("newName"))];
    if matches!(command, "restore_server_trash" | "unzip_server_path" | "add_server_uploads") {
        // These can land a file anywhere in the folder they name; at the top that may be AGENTS.md.
        candidates.push(if is_rules(&path) || path.trim_matches('/').is_empty() { RULES_FILE.into() } else { String::new() });
    }
    if let Some(list) = body.get("paths").and_then(Value::as_array) {
        candidates.extend(list.iter().filter_map(Value::as_str).map(str::to_string));
    }
    candidates.iter().any(|one| is_rules(one))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> DeviceId {
        DeviceId([7; 16])
    }

    #[test]
    fn references_round_trip_and_reject_odd_text() {
        let text = make("01JTEAM", &device(), "srv-1");
        assert_eq!(parse(&text), Some(TeamRef { team: "01JTEAM".into(), host: device(), instance: "srv-1".into() }));
        assert_eq!(parse("srv-1"), None);
        assert_eq!(parse("team~a~b"), None);
        assert_eq!(parse(&format!("{text}~more")), None);
        assert_eq!(parse(&format!("team~a/b~{}~c", device().text())), None);
        assert_eq!(parse(&format!("team~a~{}~..", device().text())), None);
    }

    #[test]
    fn the_marker_and_chat_come_off_before_running() {
        let body = marked(json!({ "id": "s", "path": "a.yml" }), "c1");
        assert!(is_agent(&body));
        let (plain, chat) = unmark(&body);
        assert_eq!(plain, json!({ "id": "s", "path": "a.yml" }));
        assert_eq!(chat, "c1");
        assert!(!is_agent(&json!({ "id": "s" })));
        assert!(!is_agent(&json!({ "agent": "yes" })));
    }

    #[test]
    fn switch_off_refuses_everything() {
        for command in ["list_instances", "read_editable_file", "save_editable_file"] {
            assert!(matches!(decide(false, false, Role::Owner, command, &json!({}), true), Gate::Refuse(_)), "{command}");
        }
    }

    #[test]
    fn reads_pass_and_writes_wait_for_the_host() {
        let body = json!({ "id": "s", "path": "server.properties" });
        for read in ["list_instances", "get_console_logs", "read_editable_file", "browse_server_files", "scan_addons", "read_server_agents_md"] {
            assert_eq!(decide(true, false, Role::Developer, read, &body, false), Gate::Pass, "{read}");
        }
        for write in ["save_editable_file", "send_console_command", "install_addon", "toggle_addon", "delete_addon", "save_world_settings", "save_server_properties", "start_instance", "trash_server_path"] {
            assert_eq!(decide(true, false, Role::Developer, write, &body, false), Gate::Ask, "{write}");
        }
        // Commands both lists name (rewind) change things, so they wait too.
        assert_eq!(decide(true, false, Role::Developer, "rewind_restore", &body, false), Gate::Ask);
        // Sending a plugin file in pieces doesn't ask each time; adding it to the server does. A viewer can't.
        for piece in ["__upload_begin", "__upload_part", "__upload_end"] {
            assert_eq!(decide(true, false, Role::Developer, piece, &json!({}), false), Gate::Pass, "{piece}");
            assert!(matches!(decide(true, false, Role::Viewer, piece, &json!({}), false), Gate::Refuse(_)), "{piece}");
        }
        assert_eq!(decide(true, false, Role::Developer, "add_addon_files", &body, false), Gate::Ask);
    }

    #[test]
    fn allowing_a_chat_lets_its_changes_through() {
        let body = json!({ "id": "s", "path": "a.yml" });
        assert_eq!(decide(true, false, Role::Developer, "save_editable_file", &body, true), Gate::Pass);
        // But never past the role.
        assert!(matches!(decide(true, false, Role::Viewer, "save_editable_file", &body, true), Gate::Refuse(_)));
    }

    #[test]
    fn agents_stay_inside_the_teammates_role() {
        let body = json!({ "id": "s" });
        // A viewer's agent reads, and nothing else.
        assert_eq!(decide(true, false, Role::Viewer, "read_editable_file", &body, false), Gate::Pass);
        for write in ["save_editable_file", "send_console_command", "install_addon", "save_world_settings"] {
            assert!(matches!(decide(true, false, Role::Viewer, write, &body, false), Gate::Refuse(_)), "{write}");
        }
        // Admin-only and owner-only commands follow the role, before the host is ever asked.
        for command in crate::web::api::SERVER_ADMINS {
            assert!(matches!(decide(true, false, Role::Developer, command, &body, false), Gate::Refuse(_)), "{command}");
            assert_eq!(decide(true, false, Role::Admin, command, &body, false), Gate::Ask, "{command}");
        }
        for command in crate::web::api::SERVER_OWNER {
            assert!(matches!(decide(true, false, Role::Admin, command, &body, false), Gate::Refuse(_)), "{command}");
            assert_eq!(decide(true, false, Role::Owner, command, &body, false), Gate::Ask, "{command}");
        }
        // "Commands: admins only" holds for agents too.
        assert!(matches!(decide(true, true, Role::Developer, "send_console_command", &body, false), Gate::Refuse(_)));
        assert_eq!(decide(true, true, Role::Admin, "send_console_command", &body, false), Gate::Ask);
        // The shared AGENTS.md: everyone's agent reads it, only an admin's may change it.
        assert_eq!(decide(true, false, Role::Viewer, "read_server_agents_md", &body, false), Gate::Pass);
        assert!(matches!(decide(true, false, Role::Developer, "save_server_agents_md", &body, false), Gate::Refuse(_)));
        assert!(matches!(decide(true, false, Role::Developer, "save_editable_file", &json!({ "id": "s", "path": "/AGENTS.md" }), true), Gate::Refuse(_)));
        assert_eq!(decide(true, false, Role::Admin, "save_server_agents_md", &body, false), Gate::Ask);
    }

    #[test]
    fn agent_switch_is_per_server_and_per_teammate() {
        let mut link = crate::monolink_team::TeamServerLink { instance: "s".into(), id: "x".into(), ..Default::default() };
        assert!(!allowed(&link, "bob"), "off by default");
        link.agent_people = vec!["Bob".into()];
        assert!(allowed(&link, "bob"));
        assert!(!allowed(&link, "carol"));
        link.agents = true;
        assert!(allowed(&link, "carol"));
    }

    #[test]
    fn requests_that_would_change_the_shared_rules() {
        let yes = [
            ("save_editable_file", json!({ "path": "AGENTS.md" })),
            ("save_editable_file", json!({ "path": "./agents.md" })),
            ("create_server_file", json!({ "path": "", "name": "AGENTS.md" })),
            ("rename_server_path", json!({ "path": "notes.md", "newName": "AGENTS.md" })),
            ("rename_server_path", json!({ "path": "AGENTS.md", "newName": "old.md" })),
            ("trash_server_path", json!({ "path": "\\AGENTS.md" })),
            ("add_server_uploads", json!({ "path": "" })),
            ("save_server_agents_md", json!({})),
        ];
        for (command, body) in yes {
            assert!(touches_rules(command, &body), "{command} {body}");
        }
        let no = [
            ("save_editable_file", json!({ "path": "plugins/x/AGENTS.md" })),
            ("create_server_file", json!({ "path": "plugins", "name": "AGENTS.md" })),
            ("read_editable_file", json!({ "path": "AGENTS.md" })),
            ("add_server_uploads", json!({ "path": "plugins" })),
        ];
        for (command, body) in no {
            assert!(!touches_rules(command, &body), "{command} {body}");
        }
    }

    #[test]
    fn reads_the_shared_rules_from_the_server_folder() {
        let dir = std::env::temp_dir().join(format!("monolab-rules-{}", nonce()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(read_rules_in(&dir).unwrap(), "", "no file is no rules");
        assert_eq!(briefing_rules(""), "");
        std::fs::write(dir.join(RULES_FILE), "# Rules\n- Never touch world/\n").unwrap();
        let text = read_rules_in(&dir).unwrap();
        assert!(text.contains("Never touch world/"));
        let note = briefing_rules(&text);
        assert!(note.contains("<server-rules>") && note.contains("Never touch world/"));
        let long = "x".repeat(RULES_IN_BRIEFING + 10);
        assert!(briefing_rules(&long).contains("cut short"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_hosts_answer_reaches_the_waiting_request() {
        let (sender, answer) = channel();
        let key: ChatKey = ("t".into(), "bob".into(), "s".into(), "c".into());
        let ask = AgentAsk {
            id: "ask-test".into(),
            who: "@bob".into(),
            role: "developer".into(),
            team: "t".into(),
            server: "s".into(),
            server_name: String::new(),
            command: "save_editable_file".into(),
            path: "a.yml".into(),
            text: String::new(),
            detail: String::new(),
            at: 0.0,
            can_chat: true,
        };
        held().waiting.push((ask, sender, key.clone()));
        assert!(team_agent_asks().iter().any(|one| one.id == "ask-test"));
        assert!(team_agent_answer("ask-test".into(), "maybe".into()).is_err());
        team_agent_answer("ask-test".into(), "chat".into()).unwrap();
        assert_eq!(answer.recv_timeout(Duration::from_secs(1)), Ok(Answer::Chat));
        held().waiting.retain(|(one, _, _)| one.id != "ask-test");
        assert!(team_agent_answer("ask-test".into(), "allow".into()).is_err(), "answered requests are gone");
        // A chat the host allowed lets its later changes through, until the switch is narrowed.
        held().chats.insert(key.clone(), Instant::now());
        assert!(chat_allowed(&key));
        assert!(!chat_allowed(&(key.0.clone(), key.1.clone(), key.2.clone(), String::new())), "no chat, no blanket allow");
        forget_chats("t");
        assert!(!chat_allowed(&key));
    }

    #[test]
    fn teammates_without_an_agent_follow_the_same_rules_file_limit() {
        use crate::monolink_team::role_refusal;
        let body = json!({ "id": "s", "path": "AGENTS.md" });
        assert!(role_refusal(Role::Developer, "save_editable_file", &body, false).is_some());
        assert!(role_refusal(Role::Admin, "save_editable_file", &body, false).is_none());
        assert!(role_refusal(Role::Developer, "save_editable_file", &json!({ "id": "s", "path": "config.yml" }), false).is_none());
        assert!(role_refusal(Role::Viewer, "read_server_agents_md", &json!({ "id": "s" }), false).is_none());
    }

    #[test]
    fn a_shared_server_becomes_an_instance_for_agent_screens() {
        let text = make("T", &device(), "s");
        let one = shared_instance(&text, "Survival", "paper", "1.21.4").unwrap();
        assert_eq!(one.id, text);
        assert_eq!(one.name, "Survival");
        assert!(one.panel_host.is_empty() && one.remote_host.is_empty());
        assert!(place_of(&text).is_some());
        assert!(place_of("s").is_none());
    }
}

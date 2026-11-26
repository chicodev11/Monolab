//! Monolink account: Discord sign-in through the browser, the session token (sealed with DPAPI like
//! the device key), calls to the account server (`mnlab.cc`) and its live connection for who is
//! online and team changes. See `docs/monolink.md` sections 4 and 9.
//!
//! The token never reaches the web view: the screens call `account_api` with a path and Rust adds it.

use crate::data_root::MonolabDirs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use ed25519_dalek::Signer;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio_tungstenite::tungstenite::Message;

const DEFAULT_SERVER: &str = "https://mnlab.cc";
const SESSION_FILE: &str = "monolink-session.bin";
/// Events the screens listen to: the account changed (signed in/out), and each live message.
pub const CHANGED: &str = "account-changed";
pub const LIVE: &str = "account-live";
const SIGN_IN_WAIT: Duration = Duration::from_secs(10 * 60);

/// The account server. `MONOLAB_ACCOUNTS_URL` points a test build at a local one.
pub fn server() -> String {
    std::env::var("MONOLAB_ACCOUNTS_URL").ok().filter(|one| !one.trim().is_empty()).unwrap_or_else(|| DEFAULT_SERVER.into()).trim_end_matches('/').to_string()
}

/// `fake` only for a local test server started with `--fake-sign-in`.
fn provider() -> &'static str {
    if std::env::var("MONOLAB_FAKE_SIGN_IN").is_ok_and(|one| one == "1") { "fake" } else { "discord" }
}

#[derive(Default)]
struct State {
    token: Option<String>,
    user: Option<Value>,
    /// Handed out by a sign-in that needs a handle; used once by `account_finish`.
    ticket: Option<String>,
    sign_in: Option<Arc<AtomicBool>>,
    live: Option<tauri::async_runtime::JoinHandle<()>>,
    live_status: &'static str,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State { live_status: "off", ..State::default() }))
}

fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| reqwest::Client::builder().timeout(Duration::from_secs(20)).user_agent("Monolab").build().expect("http client"))
}

// ── the stored session ─────────────────────────────────────────────────────

fn session_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().monolab_data_dir().map_err(|error| error.to_string())?.join(SESSION_FILE))
}

/// Whether this PC is signed in to a Monolab account. Monolink (teams, both sides) only
/// works signed in.
pub fn is_signed_in(app: &AppHandle) -> bool {
    load_token(app).is_some()
}

fn load_token(app: &AppHandle) -> Option<String> {
    let mut state = state().lock().expect("account poisoned");
    if state.token.is_none() {
        let sealed = std::fs::read(session_path(app).ok()?).ok()?;
        state.token = String::from_utf8(crate::device::unseal(&sealed)?).ok();
    }
    state.token.clone()
}

fn save_token(app: &AppHandle, token: &str) -> Result<(), String> {
    let sealed = crate::device::seal(token.as_bytes()).ok_or("Windows couldn't protect the Monolink session.")?;
    let path = session_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, &sealed).and_then(|()| std::fs::rename(&temp, &path)).map_err(|error| format!("Could not save the Monolink session: {error}"))?;
    state().lock().expect("account poisoned").token = Some(token.to_string());
    Ok(())
}

/// Forgets the session here (the server side is ended by `account_sign_out`).
fn forget(app: &AppHandle) {
    if let Ok(path) = session_path(app) {
        let _ = std::fs::remove_file(path);
    }
    let live = {
        let mut state = state().lock().expect("account poisoned");
        state.token = None;
        state.user = None;
        state.live_status = "off";
        state.live.take()
    };
    if let Some(live) = live {
        live.abort();
    }
    // Monolink needs an account: signing out ends every connection.
    crate::direct::close_all();
    let _ = app.emit(CHANGED, ());
}

fn signed_in(app: &AppHandle, token: &str, user: Value) -> Result<(), String> {
    save_token(app, token)?;
    state().lock().expect("account poisoned").user = Some(user);
    start_live(app);
    let _ = app.emit(CHANGED, ());
    Ok(())
}

// ── views ──────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    /// `signed_out`, `signed_in`, or `offline` (signed in but the server can't be reached).
    pub status: String,
    /// The account as the server sends it (`id`, `handle`, `avatar`, `handle_change_after`), JSON.
    pub user: Option<String>,
    pub server: String,
    /// The live connection: `off`, `connecting`, `online`, `waiting`.
    pub live: String,
    pub error: Option<String>,
}

fn view(status: &str, error: Option<String>) -> AccountView {
    let state = state().lock().expect("account poisoned");
    AccountView {
        status: status.into(),
        user: state.user.as_ref().map(Value::to_string),
        // Where the account server is stays in the app.
        server: String::new(),
        live: state.live_status.into(),
        error,
    }
}

#[derive(Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SignInResult {
    /// `signed_in`, `needs_handle`, `failed` or `cancelled`.
    pub state: String,
    pub suggested: Option<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HandleCheck {
    pub ok: bool,
    pub handle: Option<String>,
    pub reason: Option<String>,
}

/// The `error` code from a server answer, or the whole text when it isn't one.
fn error_code(text: &str) -> String {
    serde_json::from_str::<Value>(text).ok().and_then(|value| value["error"].as_str().map(str::to_string)).unwrap_or_else(|| text.trim().to_string())
}

fn unreachable(error: reqwest::Error) -> String {
    format!("unreachable: {error}")
}

// ── commands ───────────────────────────────────────────────────────────────

/// Where the account stands. Checks the session with the server when there is one.
#[tauri::command(async)]
#[specta::specta]
pub async fn account_state(app: AppHandle) -> AccountView {
    let Some(token) = load_token(&app) else { return view("signed_out", None) };
    match http().get(format!("{}/v2/me", server())).bearer_auth(&token).send().await {
        Err(error) => view("offline", Some(unreachable(error))),
        Ok(response) if response.status() == reqwest::StatusCode::UNAUTHORIZED => {
            forget(&app);
            view("signed_out", None)
        }
        Ok(response) if !response.status().is_success() => {
            let text = response.text().await.unwrap_or_default();
            view("offline", Some(error_code(&text)))
        }
        Ok(response) => {
            let body: Value = response.json().await.unwrap_or(Value::Null);
            state().lock().expect("account poisoned").user = Some(body["user"].clone());
            start_live(&app);
            view("signed_in", None)
        }
    }
}

/// Signs in with Discord: opens the browser and waits (up to 10 minutes) for the answer.
#[tauri::command(async)]
#[specta::specta]
pub async fn account_sign_in(app: AppHandle) -> Result<SignInResult, String> {
    let key = crate::device::device_key(&app)?;
    let provider = provider();
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_secs()).unwrap_or(0);
    let signature = key.sign(format!("monolab-login-v1\n{provider}\n{at}").as_bytes());
    let name = crate::relay_name_or_default(&crate::read_app_settings(&app).relay_name);
    // After Discord the browser comes back here with a proof; only with it does polling give the
    // session. So a sign-in link someone sends you can't sign *their* app in as you.
    let back = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|error| format!("Couldn't get ready to sign in: {error}"))?;
    let port = back.local_addr().map_err(|error| error.to_string())?.port();
    let response = http()
        .post(format!("{}/v2/auth/begin", server()))
        .json(&json!({
            "provider": provider,
            "public_key": hex(&key.verifying_key().to_bytes()),
            "device_name": name,
            "at": at,
            "signature": hex(&signature.to_bytes()),
            "port": port,
        }))
        .send()
        .await
        .map_err(unreachable)?;
    let ok = response.status().is_success();
    let text = response.text().await.unwrap_or_default();
    if !ok {
        return Err(error_code(&text));
    }
    let begin: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let (Some(login_id), Some(poll_secret), Some(url)) = (begin["login_id"].as_str(), begin["poll_secret"].as_str(), begin["url"].as_str()) else {
        return Err("bad_answer".into());
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    if let Some(old) = state().lock().expect("account poisoned").sign_in.replace(cancelled.clone()) {
        old.store(true, Ordering::Relaxed);
    }
    let proof = Arc::new(Mutex::new(String::new()));
    let catcher = tokio::spawn(catch_proof(back, login_id.to_string(), proof.clone()));
    if provider == "fake" {
        // Test server only: finish the pretend Discord step here instead of opening a browser.
        let subject = std::env::var("MONOLAB_FAKE_SUBJECT").unwrap_or_else(|_| "1".into());
        let _ = http().get(format!("{url}&subject={subject}&name=tester{subject}")).send().await;
    } else {
        use tauri_plugin_opener::OpenerExt;
        app.opener().open_url(url, None::<&str>).map_err(|error| format!("Couldn't open the browser: {error}"))?;
    }
    let started = std::time::Instant::now();
    let result = loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if cancelled.load(Ordering::Relaxed) {
            break SignInResult { state: "cancelled".into(), suggested: None, reason: None };
        }
        if started.elapsed() > SIGN_IN_WAIT {
            break SignInResult { state: "failed".into(), suggested: None, reason: Some("expired".into()) };
        }
        let proof = proof.lock().expect("proof poisoned").clone();
        let Ok(response) = http()
            .post(format!("{}/v2/auth/poll", server()))
            .json(&json!({ "login_id": login_id, "poll_secret": poll_secret, "proof": proof }))
            .send()
            .await
        else {
            continue;
        };
        let Ok(answer) = response.json::<Value>().await else { continue };
        match answer["state"].as_str() {
            Some("waiting") | None => continue,
            Some("signed_in") => {
                let token = answer["token"].as_str().unwrap_or_default();
                signed_in(&app, token, answer["user"].clone())?;
                break SignInResult { state: "signed_in".into(), suggested: None, reason: None };
            }
            Some("needs_handle") => {
                state().lock().expect("account poisoned").ticket = answer["ticket"].as_str().map(str::to_string);
                break SignInResult { state: "needs_handle".into(), suggested: answer["suggested"].as_str().map(str::to_string), reason: None };
            }
            Some(_) => break SignInResult { state: "failed".into(), suggested: None, reason: answer["reason"].as_str().map(str::to_string) },
        }
    };
    catcher.abort();
    state().lock().expect("account poisoned").sign_in = None;
    Ok(result)
}

/// Waits on 127.0.0.1 for the browser to come back from the account server with this sign-in's
/// proof, then sends the browser on to the server's "go back to the app" page.
async fn catch_proof(listener: tokio::net::TcpListener, login_id: String, proof: Arc<Mutex<String>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    loop {
        let Ok((mut stream, _)) = listener.accept().await else { return };
        let mut buffer = vec![0u8; 4096];
        let read = match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer)).await {
            Ok(Ok(read)) => read,
            _ => continue,
        };
        let request = String::from_utf8_lossy(&buffer[..read]);
        let target = request.lines().next().and_then(|line| line.strip_prefix("GET ")).and_then(|rest| rest.split(' ').next()).unwrap_or_default();
        let found = target.strip_prefix("/monolab-login?").and_then(|query| {
            let pairs: Vec<(String, String)> = tauri::Url::parse(&format!("http://x/?{query}")).ok()?.query_pairs().into_owned().collect();
            let get = |key: &str| pairs.iter().find(|(name, _)| name == key).map(|(_, value)| value.clone());
            (get("login")? == login_id).then_some(get("proof")?)
        });
        let answer = match found {
            Some(value) if !value.is_empty() => {
                *proof.lock().expect("proof poisoned") = value;
                format!("HTTP/1.1 303 See Other\r\nLocation: {}/auth/done\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", server())
            }
            _ => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
        };
        let _ = stream.write_all(answer.as_bytes()).await;
        let _ = stream.shutdown().await;
    }
}

#[tauri::command]
#[specta::specta]
pub fn account_cancel_sign_in() {
    if let Some(flag) = state().lock().expect("account poisoned").sign_in.take() {
        flag.store(true, Ordering::Relaxed);
    }
}

/// Whether a handle can be used (format and not taken).
#[tauri::command(async)]
#[specta::specta]
pub async fn account_check_handle(handle: String) -> Result<HandleCheck, String> {
    let clean: String = handle.trim().trim_start_matches('@').chars().filter(|one| one.is_ascii_alphanumeric() || *one == '_').collect();
    if clean.is_empty() {
        return Ok(HandleCheck { ok: false, handle: None, reason: Some("too_short".into()) });
    }
    let response = http().get(format!("{}/v2/handles/{clean}", server())).send().await.map_err(unreachable)?;
    let answer: Value = response.json().await.map_err(|error| error.to_string())?;
    if answer.get("error").is_some() {
        return Err(error_code(&answer.to_string()));
    }
    // What the person typed may have had letters the server doesn't allow; report that, not the cleaned one.
    let typed = handle.trim().trim_start_matches('@').to_ascii_lowercase();
    if typed != clean.to_ascii_lowercase() {
        return Ok(HandleCheck { ok: false, handle: None, reason: Some("bad_letters".into()) });
    }
    Ok(HandleCheck {
        ok: answer["ok"].as_bool().unwrap_or(false),
        handle: answer["handle"].as_str().map(str::to_string),
        reason: answer["reason"].as_str().map(str::to_string),
    })
}

/// Picks the handle for a new account and finishes signing in.
#[tauri::command(async)]
#[specta::specta]
pub async fn account_finish(app: AppHandle, handle: String) -> Result<AccountView, String> {
    let ticket = state().lock().expect("account poisoned").ticket.clone().ok_or("expired")?;
    let response = http().post(format!("{}/v2/auth/finish", server())).json(&json!({ "ticket": ticket, "handle": handle })).send().await.map_err(unreachable)?;
    let ok = response.status().is_success();
    let text = response.text().await.unwrap_or_default();
    if !ok {
        return Err(error_code(&text));
    }
    let answer: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    state().lock().expect("account poisoned").ticket = None;
    signed_in(&app, answer["token"].as_str().unwrap_or_default(), answer["user"].clone())?;
    Ok(view("signed_in", None))
}

#[tauri::command(async)]
#[specta::specta]
pub async fn account_sign_out(app: AppHandle) {
    if let Some(token) = load_token(&app) {
        let _ = http().post(format!("{}/v2/auth/logout", server())).bearer_auth(token).send().await;
    }
    forget(&app);
}

/// Calls the account server as this person. Only `/v2/` paths outside sign-in. Gives the answer's
/// JSON text (`null` when there is none); a refusal comes back as the server's error JSON.
#[tauri::command(async)]
#[specta::specta]
pub async fn account_api(app: AppHandle, method: String, path: String, body: Option<String>) -> Result<String, String> {
    // Grants and the host address are this app's business, not the screens'.
    if path.starts_with("/v2/grants") || path.starts_with("/v2/me/addr") {
        return Err(json!({ "error": "bad_path" }).to_string());
    }
    request(&app, &method, &path, body).await
}

/// Calls the account server from Rust. Gives the answer as JSON (`null` when there is none).
pub async fn api(app: &AppHandle, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let text = request(app, method, path, body.map(|one| one.to_string())).await?;
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

async fn request(app: &AppHandle, method: &str, path: &str, body: Option<String>) -> Result<String, String> {
    let app = app.clone();
    let (method, path) = (method.to_string(), path.to_string());
    if !path.starts_with("/v2/") || path.starts_with("/v2/auth/") || path.contains("..") {
        return Err(json!({ "error": "bad_path" }).to_string());
    }
    let token = load_token(&app).ok_or_else(|| json!({ "error": "signed_out" }).to_string())?;
    let url = format!("{}{path}", server());
    let request = match method.as_str() {
        "GET" => http().get(url),
        "POST" => http().post(url),
        "PUT" => http().put(url),
        "PATCH" => http().patch(url),
        "DELETE" => http().delete(url),
        _ => return Err(json!({ "error": "bad_method" }).to_string()),
    }
    .bearer_auth(token);
    let request = match body {
        Some(body) => request.header("content-type", "application/json").body(body),
        None => request,
    };
    let response = request.send().await.map_err(|error| json!({ "error": "unreachable", "detail": error.to_string() }).to_string())?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        forget(&app);
    }
    if !status.is_success() {
        return Err(if text.trim().starts_with('{') { text } else { json!({ "error": "server", "detail": text }).to_string() });
    }
    if path == "/v2/me" && method == "PATCH" {
        if let Ok(answer) = serde_json::from_str::<Value>(&text) {
            state().lock().expect("account poisoned").user = Some(answer["user"].clone());
            let _ = app.emit(CHANGED, ());
        }
    }
    Ok(if text.trim().is_empty() { "null".into() } else { text })
}

// ── role grants and the host address (Monolink connections) ────────────

/// What a teammate needs to connect to one host for one team.
#[derive(Clone, Debug)]
pub struct Pass {
    pub grant: String,
    /// The host's iroh address, when it said where it is lately.
    pub addr: Option<Value>,
    pub exp: u64,
}

fn grants() -> &'static Mutex<std::collections::HashMap<(String, String), Pass>> {
    static GRANTS: OnceLock<Mutex<std::collections::HashMap<(String, String), Pass>>> = OnceLock::new();
    GRANTS.get_or_init(Default::default)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_secs()).unwrap_or(0)
}

/// A grant for `host` in `team`: kept while it has a few minutes left, else fetched (with every other
/// host's in that team). `fresh` always fetches.
pub async fn grant_for(app: &AppHandle, team: &str, host: &relay_proto::DeviceId, fresh: bool) -> Result<Pass, String> {
    let key = (team.to_string(), host.text());
    if !fresh {
        if let Some(pass) = grants().lock().expect("grants poisoned").get(&key).filter(|one| one.exp > unix_now() + 4 * 60) {
            return Ok(pass.clone());
        }
    }
    let answer = api(app, "POST", "/v2/grants", Some(json!({ "team": team }))).await.map_err(|error| grant_error(&error))?;
    let mut held = grants().lock().expect("grants poisoned");
    held.retain(|(one, _), _| one != team);
    for item in answer["grants"].as_array().into_iter().flatten() {
        let (Some(host), Some(grant)) = (item["host_device"].as_str(), item["grant"].as_str()) else { continue };
        let addr = Some(item["addr"].clone()).filter(|one| !one.is_null());
        held.insert((team.to_string(), host.to_string()), Pass { grant: grant.to_string(), addr, exp: item["exp"].as_u64().unwrap_or(0) });
    }
    held.get(&key).cloned().ok_or_else(|| "This server isn't shared with the team any more.".to_string())
}

fn grant_error(error: &str) -> String {
    if error.contains("signed_out") {
        "Sign in to Monolink again.".into()
    } else if error.contains("not_found") {
        "You're not in this team any more.".into()
    } else if error.contains("unreachable") {
        "Couldn't reach Monolink. Check your internet connection.".into()
    } else {
        "Couldn't get access from Monolink. Try again.".into()
    }
}

/// Tells the account server where this PC's iroh endpoint is, when it hosts servers for a team.
/// Teammates get it only with a grant for this PC.
pub fn publish_addr(app: &AppHandle) {
    if crate::read_app_settings(app).monolink_teams.is_empty() || load_token(app).is_none() {
        return;
    }
    crate::direct::start(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Right after start the endpoint may still be opening.
        for _ in 0..50 {
            if let Ok(addr) = crate::direct::address() {
                let _ = api(&app, "PUT", "/v2/me/addr", Some(json!({ "addr": addr }))).await;
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
}

/// A live message about teams: keep grants, hosted servers and open connections in step.
fn on_team_message(app: &AppHandle, value: &Value) {
    let team = value["team"].as_str().or_else(|| value["data"]["team"].as_str()).unwrap_or_default().to_string();
    if team.is_empty() {
        return;
    }
    grants().lock().expect("grants poisoned").retain(|(one, _), _| *one != team);
    let gone = value["type"] == "notice" && matches!(value["kind"].as_str(), Some("removed" | "team_deleted"));
    if gone {
        crate::monolink_team::prune(app, &team, None);
        crate::direct::close_team(&team);
        return;
    }
    if value["type"] != "team_changed" {
        return;
    }
    crate::direct::team_changed(&team, value["version"].as_u64().unwrap_or(0));
    // A host checks its servers are still attached there (someone may have taken one out).
    if crate::read_app_settings(app).monolink_teams.iter().any(|one| one.team == team) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            match api(&app, "GET", &format!("/v2/teams/{team}"), None).await {
                Ok(answer) => {
                    let ids: Vec<String> = answer["servers"].as_array().into_iter().flatten().filter_map(|one| one["id"].as_str().map(str::to_string)).collect();
                    crate::monolink_team::prune(&app, &team, Some(&ids));
                }
                Err(error) if error.contains("not_found") => crate::monolink_team::prune(&app, &team, None),
                Err(_) => {}
            }
        });
    }
}

// ── invite links (monolab://invite/<code>) ─────────────────────────────────

fn pending_invite() -> &'static Mutex<Option<String>> {
    static PENDING: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(None))
}

/// The invite code in a `monolab://invite/<code>` or `https://mnlab.cc/invite/<code>` link.
pub fn invite_code_of(link: &str) -> Option<String> {
    let link = link.trim().trim_end_matches('/');
    let rest = link.strip_prefix("monolab://invite/").or_else(|| link.split_once("/invite/").map(|(_, code)| code))?;
    let code: String = rest.chars().take_while(|one| one.is_ascii_alphanumeric()).collect();
    (code.len() >= 20 && code.len() <= 40).then_some(code)
}

/// A link arrived (first launch or a second launch passed it on): keep it and tell the screens.
pub fn open_link(app: &AppHandle, link: &str) {
    if let Some(code) = invite_code_of(link) {
        *pending_invite().lock().expect("invite poisoned") = Some(code.clone());
        let _ = app.emit("monolink-invite", code);
        if let Some(window) = app.get_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

/// The invite link the app was opened with, once.
#[tauri::command]
#[specta::specta]
pub fn take_pending_invite() -> Option<String> {
    pending_invite().lock().expect("invite poisoned").take()
}

// ── live connection ────────────────────────────────────────────────────────

static LIVE_GENERATION: AtomicU64 = AtomicU64::new(0);

fn set_live(app: &AppHandle, status: &'static str) {
    state().lock().expect("account poisoned").live_status = status;
    let _ = app.emit(LIVE, json!({ "type": "status", "status": status }));
}

/// Keeps the live connection up while signed in. Does nothing when it already runs.
pub fn start_live(app: &AppHandle) {
    let mut state = state().lock().expect("account poisoned");
    if state.live.as_ref().is_some_and(|one| !one.inner().is_finished()) || state.token.is_none() {
        return;
    }
    let generation = LIVE_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let app = app.clone();
    state.live = Some(tauri::async_runtime::spawn(async move {
        let mut wait = 1u64;
        loop {
            if LIVE_GENERATION.load(Ordering::Relaxed) != generation {
                return;
            }
            set_live(&app, "connecting");
            let started = std::time::Instant::now();
            match live_session(&app).await {
                Ok(()) => {}
                Err(stop) if stop == "signed_out" => {
                    forget(&app);
                    return;
                }
                Err(_) => {}
            }
            set_live(&app, "waiting");
            if started.elapsed() > Duration::from_secs(60) {
                wait = 1;
            }
            tokio::time::sleep(Duration::from_secs(wait)).await;
            wait = (wait * 2).min(60);
        }
    }));
}

async fn live_session(app: &AppHandle) -> Result<(), String> {
    let token = load_token(app).ok_or("signed_out")?;
    let base = server();
    let url = if let Some(rest) = base.strip_prefix("https://") { format!("wss://{rest}/v2/ws") } else { format!("ws://{}/v2/ws", base.trim_start_matches("http://")) };
    let connector = if url.starts_with("wss://") {
        use rustls_platform_verifier::BuilderVerifierExt;
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|error| error.to_string())?
            .with_platform_verifier()
            .map_err(|error| error.to_string())?
            .with_no_client_auth();
        tokio_tungstenite::Connector::Rustls(Arc::new(config))
    } else {
        tokio_tungstenite::Connector::Plain
    };
    let (socket, _) = tokio::time::timeout(Duration::from_secs(15), tokio_tungstenite::connect_async_tls_with_config(url, None, false, Some(connector)))
        .await
        .map_err(|_| "timeout".to_string())?
        .map_err(|error| error.to_string())?;
    let (mut sink, mut source) = socket.split();
    sink.send(Message::Text(json!({ "type": "auth", "token": token }).to_string().into())).await.map_err(|error| error.to_string())?;
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    ping.tick().await;
    let mut pings = 0u32;
    loop {
        tokio::select! {
            incoming = tokio::time::timeout(Duration::from_secs(90), source.next()) => {
                let Ok(Some(Ok(message))) = incoming else { return Err("closed".into()) };
                let Message::Text(text) = message else { continue };
                let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };
                match value["type"].as_str() {
                    Some("ready") => {
                        set_live(app, "online");
                        publish_addr(app);
                    }
                    Some("pong") => {}
                    Some("error") => {
                        let code = value["code"].as_str().unwrap_or_default();
                        return Err(if code == "signed_out" { "signed_out".into() } else { code.to_string() });
                    }
                    _ => {
                        if matches!(value["type"].as_str(), Some("team_changed" | "notice")) {
                            on_team_message(app, &value);
                        }
                        let _ = app.emit(LIVE, value);
                    }
                }
            }
            _ = ping.tick() => {
                if sink.send(Message::Text(json!({ "type": "ping" }).to_string().into())).await.is_err() {
                    return Err("closed".into());
                }
                // The address can change (new network); say it again every five minutes.
                pings += 1;
                if pings.is_multiple_of(10) {
                    publish_addr(app);
                }
            }
        }
    }
}

/// Stops the live connection (app closing).
pub fn stop(_app: &AppHandle) {
    LIVE_GENERATION.fetch_add(1, Ordering::Relaxed);
    if let Some(live) = state().lock().expect("account poisoned").live.take() {
        live.abort();
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::invite_code_of;

    #[test]
    fn invite_links() {
        let code = "abcdefghjkmnpqrstvwxyz0123";
        assert_eq!(invite_code_of(&format!("monolab://invite/{code}")).as_deref(), Some(code));
        assert_eq!(invite_code_of(&format!("monolab://invite/{code}/")).as_deref(), Some(code));
        assert_eq!(invite_code_of(&format!("https://mnlab.cc/invite/{code}")).as_deref(), Some(code));
        assert_eq!(invite_code_of("monolab://invite/short"), None);
        assert_eq!(invite_code_of("monolab://other/abcdefghjkmnpqrstvwxyz0123"), None);
    }
}

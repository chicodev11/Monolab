
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub const DEFAULT: &str = "default";
const SHARED: [&str; 2] = ["projects", "file-history"];

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    accounts: Vec<Stored>,
    #[serde(default)]
    servers: HashMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Stored {
    id: String,
    name: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccount {
    pub id: String,
    pub name: String,
    pub logged_in: bool,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeAccounts {
    pub accounts: Vec<ClaudeAccount>,
    pub current: String,
}

fn store_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|error| error.to_string())?.join("claude-accounts.json"))
}

fn read(app: &AppHandle) -> Store {
    store_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write(app: &AppHandle, store: &Store) -> Result<(), String> {
    let path = store_path(app)?;
    let text = serde_json::to_string_pretty(store).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| format!("Could not save the account list: {error}"))
}

fn home_claude(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().home_dir().map_err(|error| error.to_string())?.join(".claude"))
}

fn account_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|error| error.to_string())?.join("claude-accounts").join(id))
}

fn config_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    if id == DEFAULT {
        home_claude(app)
    } else {
        account_dir(app, id)
    }
}

pub fn config_for_server(app: &AppHandle, server: &str) -> Option<PathBuf> {
    let store = read(app);
    let id = store.servers.get(server)?;
    if id == DEFAULT || !store.accounts.iter().any(|one| &one.id == id) {
        return None;
    }
    let dir = account_dir(app, id).ok()?;
    let _ = prepare(app, &dir);
    Some(dir)
}

fn prepare(app: &AppHandle, dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| format!("Could not create the account folder: {error}"))?;
    let home = home_claude(app)?;
    for name in SHARED {
        let target = home.join(name);
        fs::create_dir_all(&target).map_err(|error| error.to_string())?;
        let link = dir.join(name);
        if fs::symlink_metadata(&link).is_ok() {
            continue;
        }
        link_folder(&link, &target)?;
    }
    Ok(())
}

#[cfg(windows)]
fn link_folder(link: &Path, target: &Path) -> Result<(), String> {
    let done = crate::hidden_command("cmd")
        .arg("/c")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .map_err(|error| error.to_string())?;
    if done.status.success() {
        Ok(())
    } else {
        Err(format!("Could not link the conversation history folder: {}", String::from_utf8_lossy(&done.stderr).trim()))
    }
}

#[cfg(not(windows))]
fn link_folder(link: &Path, target: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|error| error.to_string())
}

fn logged_in(app: &AppHandle, id: &str) -> bool {
    config_dir(app, id).is_ok_and(|dir| dir.join(".credentials.json").is_file())
}

pub fn list(app: &AppHandle, server: &str) -> ClaudeAccounts {
    let store = read(app);
    let mut accounts = vec![ClaudeAccount { id: DEFAULT.into(), name: "Default account".into(), logged_in: logged_in(app, DEFAULT) }];
    for one in &store.accounts {
        accounts.push(ClaudeAccount { id: one.id.clone(), name: one.name.clone(), logged_in: logged_in(app, &one.id) });
    }
    let current = store
        .servers
        .get(server)
        .filter(|id| store.accounts.iter().any(|one| &one.id == *id))
        .cloned()
        .unwrap_or_else(|| DEFAULT.into());
    ClaudeAccounts { accounts, current }
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("Account names must be 1–40 characters.".into());
    }
    Ok(name.to_string())
}

pub fn add(app: &AppHandle, name: &str) -> Result<String, String> {
    let name = clean_name(name)?;
    let mut store = read(app);
    let id = format!(
        "acct{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_millis()).unwrap_or(0)
    );
    prepare(app, &account_dir(app, &id)?)?;
    store.accounts.push(Stored { id: id.clone(), name });
    write(app, &store)?;
    Ok(id)
}

pub fn rename(app: &AppHandle, id: &str, name: &str) -> Result<(), String> {
    let name = clean_name(name)?;
    let mut store = read(app);
    let one = store.accounts.iter_mut().find(|one| one.id == id).ok_or("That account wasn't found.")?;
    one.name = name;
    write(app, &store)
}

pub fn remove(app: &AppHandle, id: &str) -> Result<(), String> {
    if id == DEFAULT {
        return Err("The default account can't be deleted.".into());
    }
    let mut store = read(app);
    if !store.accounts.iter().any(|one| one.id == id) {
        return Err("That account wasn't found.".into());
    }
    let dir = account_dir(app, id)?;
    for name in SHARED {
        let link = dir.join(name);
        if fs::symlink_metadata(&link).is_ok() {
            fs::remove_dir(&link).map_err(|error| format!("Could not detach the conversation history link: {error}"))?;
        }
    }
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|error| format!("Could not delete the account folder: {error}"))?;
    }
    store.accounts.retain(|one| one.id != id);
    store.servers.retain(|_, account| account != id);
    write(app, &store)
}

pub fn use_for(app: &AppHandle, server: &str, id: &str) -> Result<(), String> {
    let mut store = read(app);
    if id == DEFAULT {
        store.servers.remove(server);
    } else {
        if !store.accounts.iter().any(|one| one.id == id) {
            return Err("That account wasn't found.".into());
        }
        store.servers.insert(server.to_string(), id.to_string());
    }
    write(app, &store)
}

static LOGIN_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LOGIN_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn credentials_time(app: &AppHandle, id: &str) -> Option<std::time::SystemTime> {
    config_dir(app, id).ok()?.join(".credentials.json").metadata().ok()?.modified().ok()
}

fn login_input() -> &'static std::sync::Mutex<Option<std::process::ChildStdin>> {
    static INPUT: std::sync::OnceLock<std::sync::Mutex<Option<std::process::ChildStdin>>> = std::sync::OnceLock::new();
    INPUT.get_or_init(|| std::sync::Mutex::new(None))
}

pub fn login(app: &AppHandle, id: &str) -> Result<(), String> {
    let before = credentials_time(app, id);
    let claude = crate::claude_agent::find_claude(app)
        .ok_or("Claude Code isn't installed on this computer. Install it as shown, then try again.")?;
    let mut command = if claude.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("cmd")) {
        let mut one = crate::hidden_command("cmd");
        one.arg("/c").arg(&claude);
        one
    } else {
        crate::hidden_command(&claude)
    };
    command.args(["auth", "login"]);
    if id != DEFAULT {
        let dir = account_dir(app, id)?;
        prepare(app, &dir)?;
        command.env("CLAUDE_CONFIG_DIR", dir);
    }
    command.env_remove("CLAUDECODE").env_remove("CLAUDE_CODE_ENTRYPOINT").env_remove("CLAUDE_CODE_CHILD_SESSION");
    drive_login(app, command, &|| logged_in(app, id) && credentials_time(app, id) != before)
}

/// Runs a CLI sign-in, streaming its output as `claude-login` events. One at a time across Claude and Codex.
pub(crate) fn drive_login(app: &AppHandle, command: std::process::Command, signed_in: &dyn Fn() -> bool) -> Result<(), String> {
    if LOGIN_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("Another account is still signing in. Finish that one first.".into());
    }
    let result = run_login(app, command, signed_in);
    LOGIN_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
    result
}

fn run_login(app: &AppHandle, mut command: std::process::Command, signed_in: &dyn Fn() -> bool) -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    use tauri::Emitter;
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start sign-in: {error}"))?;
    *login_input().lock().map_err(|_| "Could not read the sign-in state.")? = child.stdin.take();
    let mut readers = Vec::new();
    for stream in [child.stdout.take().map(|one| Box::new(one) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|one| Box::new(one) as Box<dyn std::io::Read + Send>)].into_iter().flatten() {
        let app = app.clone();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let _ = app.emit("claude-login", line);
            }
        }));
    }
    LOGIN_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break Some(status);
        }
        if LOGIN_CANCEL.load(std::sync::atomic::Ordering::SeqCst) || started.elapsed() > std::time::Duration::from_secs(600) {
            let _ = child.kill();
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    };
    if let Ok(mut input) = login_input().lock() {
        *input = None;
    }
    for one in readers {
        let _ = one.join();
    }
    match status {
        Some(status) if status.success() => Ok(()),
        _ if signed_in() => Ok(()),
        Some(_) => Err("Sign-in failed. Try again.".into()),
        None if LOGIN_CANCEL.load(std::sync::atomic::Ordering::SeqCst) => Err("Sign-in cancelled.".into()),
        None => Err("Sign-in did not finish within 10 minutes, so it was stopped.".into()),
    }
}

pub fn login_code(code: &str) -> Result<(), String> {
    use std::io::Write;
    let mut input = login_input().lock().map_err(|_| "Could not read the sign-in state.")?;
    let pipe = input.as_mut().ok_or("No sign-in is in progress.")?;
    pipe.write_all(format!("{}\n", code.trim()).as_bytes()).map_err(|error| error.to_string())?;
    pipe.flush().map_err(|error| error.to_string())
}

pub fn login_cancel() {
    LOGIN_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut input) = login_input().lock() {
        *input = None;
    }
}

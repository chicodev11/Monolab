
use crate::{hidden_command, kill_process_tree, load_instances, off_runtime, web};
use serde::Serialize;
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Stdio},
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

const VERSION: &str = "1.0.10";
const DOWNLOAD: &str =
    "https://github.com/playit-cloud/playit-agent/releases/download/v1.0.10/playit-windows-x86_64-signed.exe";
const API: &str = "https://api.playit.gg";
const PIPE: &str = r"\\.\pipe\monolab-playitd";
const CLAIM_WAIT: Duration = Duration::from_secs(600);
/// Don't ask the API for the tunnel list more often than this.
const TUNNELS_FRESH: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub has_secret: bool,
    pub running: bool,
    pub claim: Option<Claim>,
    pub tunnels: Vec<Tunnel>,
    pub error: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Claim {
    pub url: String,
    pub state: String,
    pub error: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Tunnel {
    pub id: String,
    pub name: String,
    pub local_port: u16,
    pub address: String,
    pub disabled: bool,
}

struct Live {
    child: Option<Child>,
    claim: Option<Claim>,
    error: String,
    tunnels: Vec<Tunnel>,
    agent_id: String,
    fetched: Option<Instant>,
    generation: u64,
    log: VecDeque<String>,
}

fn live() -> &'static Mutex<Live> {
    static LIVE: OnceLock<Mutex<Live>> = OnceLock::new();
    LIVE.get_or_init(|| {
        Mutex::new(Live {
            child: None,
            claim: None,
            error: String::new(),
            tunnels: Vec::new(),
            agent_id: String::new(),
            fetched: None,
            generation: 0,
            log: VecDeque::new(),
        })
    })
}

fn lock() -> MutexGuard<'static, Live> {
    live().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn folder(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not find the app data folder: {error}"))?
        .join("playit");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn secret_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(folder(app)?.join("playit.toml"))
}

pub fn read_secret(app: &AppHandle) -> Option<String> {
    let text = std::fs::read_to_string(secret_path(app).ok()?).ok()?;
    let trimmed = text.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(trimmed.to_string());
    }
    let key = trimmed
        .lines()
        .find_map(|line| line.trim().strip_prefix("secret_key"))?
        .trim()
        .strip_prefix('=')?
        .trim()
        .trim_matches('"')
        .to_string();
    (!key.is_empty()).then_some(key)
}

fn write_secret(app: &AppHandle, secret: &str) -> Result<(), String> {
    std::fs::write(secret_path(app)?, format!("secret_key = \"{secret}\"\n"))
        .map_err(|error| format!("Could not save the playit secret key: {error}"))
}

fn ensure_exe(app: &AppHandle) -> Result<PathBuf, String> {
    let path = folder(app)?.join(format!("playitd-{VERSION}.exe"));
    if path.is_file() {
        return Ok(path);
    }
    let client = crate::net::builder()
        .user_agent("Monolab (local Windows desktop app)")
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|error| error.to_string())?;
    let body = client
        .get(DOWNLOAD)
        .send()
        .and_then(|answer| answer.error_for_status())
        .map_err(|error| format!("Could not download playit: {error}"))?
        .bytes()
        .map_err(|error| error.to_string())?;
    if body.len() < 1024 * 1024 || !body.starts_with(b"MZ") {
        return Err("The downloaded playit file is not valid. Please try again in a moment.".into());
    }
    let staging = path.with_extension("part");
    std::fs::write(&staging, &body)
        .and_then(|_| std::fs::rename(&staging, &path))
        .map_err(|error| format!("Could not save playit: {error}"))?;
    Ok(path)
}

fn call(
    secret: Option<&str>,
    path: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let client = crate::net::builder()
        .user_agent(format!("Monolab {}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client.post(format!("{API}{path}")).json(&body);
    if let Some(secret) = secret {
        request = request.header("Authorization", format!("Agent-Key {}", secret.trim()));
    }
    let answer = request
        .send()
        .map_err(|error| format!("Could not reach playit: {error}"))?;
    if answer.status().as_u16() == 429 {
        return Err("playit is rate-limiting requests. Please try again shortly.".into());
    }
    let text = answer.text().map_err(|error| error.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
        format!(
            "Could not read the playit response: {}",
            text.chars().take(160).collect::<String>()
        )
    })?;
    let data = value.get("data").cloned().unwrap_or(serde_json::Value::Null);
    match value.get("status").and_then(|status| status.as_str()) {
        Some("success") => Ok(data),
        Some("fail") => Err(format!("playit refused: {}", brief(&data))),
        Some("error") => Err(format!("playit error: {}", brief(&data))),
        _ => Err(format!("Unrecognized playit response: {}", brief(&value))),
    }
}

fn brief(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Object(map) => {
            if let Some(message) = map.get("message").and_then(|m| m.as_str()) {
                return match map.get("type").and_then(|t| t.as_str()) {
                    Some(kind) => format!("{message} ({kind})"),
                    None => message.to_string(),
                };
            }
            if map.len() == 1 {
                let (key, inner) = map.iter().next().expect("one found");
                return format!("{key} · {}", brief(inner));
            }
            value.to_string().chars().take(160).collect()
        }
        other => other.to_string().chars().take(160).collect(),
    }
}

fn set_claim(generation: u64, state: &str, error: impl Into<String>) {
    let mut live = lock();
    if live.generation != generation {
        return;
    }
    if let Some(claim) = live.claim.as_mut() {
        claim.state = state.to_string();
        claim.error = error.into();
    }
}

pub fn begin_claim(app: &AppHandle) -> Result<String, String> {
    let code: String = web::make_key().chars().take(10).collect();
    let url = format!("https://playit.gg/claim/{code}");
    let generation = {
        let mut live = lock();
        live.generation += 1;
        live.claim = Some(Claim {
            url: url.clone(),
            state: "waiting".into(),
            error: String::new(),
        });
        live.generation
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let began = Instant::now();
        loop {
            if lock().generation != generation {
                return;
            }
            if began.elapsed() > CLAIM_WAIT {
                set_claim(generation, "expired", "");
                return;
            }
            let asked = call(
                None,
                "/claim/setup",
                serde_json::json!({
                    "code": code,
                    "agent_type": "self-managed",
                    "version": format!("Monolab {}", env!("CARGO_PKG_VERSION")),
                }),
            );
            match asked {
                Ok(state) => match state.as_str() {
                    Some("UserAccepted") => break,
                    Some("UserRejected") => {
                        set_claim(generation, "rejected", "");
                        return;
                    }
                    _ => {}
                },
                Err(error) if error.contains("CodeExpired") || error.contains("InvalidCode") => {
                    set_claim(generation, "expired", error);
                    return;
                }
                Err(error) => set_claim(generation, "waiting", error),
            }
            std::thread::sleep(Duration::from_secs(2));
        }
        let secret = loop {
            match call(None, "/claim/exchange", serde_json::json!({ "code": code })) {
                Ok(data) => match data.get("secret_key").and_then(|key| key.as_str()) {
                    Some(key) => break key.to_string(),
                    None => {
                        set_claim(generation, "error", "playit did not return a secret key.");
                        return;
                    }
                },
                Err(error) if error.contains("NotAccepted") && began.elapsed() < CLAIM_WAIT => {
                    std::thread::sleep(Duration::from_secs(2));
                }
                Err(error) => {
                    set_claim(generation, "error", error);
                    return;
                }
            }
        };
        if let Err(error) = write_secret(&app, &secret) {
            set_claim(generation, "error", error);
            return;
        }
        match start(&app) {
            Ok(()) => set_claim(generation, "accepted", ""),
            Err(error) => set_claim(generation, "accepted", error),
        }
        let _ = refresh(&app, true);
    });
    Ok(url)
}

pub fn start(app: &AppHandle) -> Result<(), String> {
    let path = secret_path(app)?;
    if read_secret(app).is_none() {
        return Err("Link a playit account first.".into());
    }
    stop();
    let exe = off_runtime(|| ensure_exe(app))?;
    let mut child = hidden_command(&exe)
        .arg("--secret-path")
        .arg(&path)
        .args(["--socket-path", PIPE])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not run playit: {error}"))?;
    if let Some(err) = child.stderr.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let mut live = lock();
                if live.log.len() >= 40 {
                    live.log.pop_front();
                }
                let lower = line.to_ascii_lowercase();
                if lower.contains("error") || lower.contains("invalid") || lower.contains("failed") {
                    live.error = line.trim().to_string();
                }
                live.log.push_back(line);
            }
        });
    }
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            for _ in BufReader::new(out).lines().map_while(Result::ok) {}
        });
    }
    let mut live = lock();
    live.child = Some(child);
    live.error.clear();
    Ok(())
}

pub fn stop() {
    let Some(mut child) = lock().child.take() else {
        return;
    };
    let pid = child.id();
    let _ = child.kill();
    kill_process_tree(pid);
    let _ = child.wait();
}

fn running() -> bool {
    let mut live = lock();
    match live.child.as_mut().map(|child| child.try_wait()) {
        Some(Ok(None)) => true,
        Some(_) => {
            live.child = None;
            false
        }
        None => false,
    }
}

fn refresh(app: &AppHandle, force: bool) -> Result<(), String> {
    let Some(secret) = read_secret(app) else {
        let mut live = lock();
        live.tunnels.clear();
        live.agent_id.clear();
        live.fetched = None;
        return Ok(());
    };
    if !force {
        if let Some(at) = lock().fetched {
            if at.elapsed() < TUNNELS_FRESH {
                return Ok(());
            }
        }
    }
    let data = off_runtime(|| call(Some(&secret), "/agents/rundata", serde_json::json!({})))?;
    let tunnels = data
        .get("tunnels")
        .and_then(|list| list.as_array())
        .map(|list| list.iter().map(tunnel_of).collect::<Vec<_>>())
        .unwrap_or_default();
    let pending = data
        .get("pending")
        .and_then(|list| list.as_array())
        .map(|list| {
            list.iter()
                .map(|one| Tunnel {
                    id: text(one, "id"),
                    name: text(one, "name"),
                    local_port: 0,
                    address: String::new(),
                    disabled: one.get("is_disabled").and_then(|v| v.as_bool()).unwrap_or(false),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut live = lock();
    live.agent_id = text(&data, "agent_id");
    live.tunnels = tunnels.into_iter().chain(pending).collect();
    live.fetched = Some(Instant::now());
    Ok(())
}

fn text(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|inner| inner.as_str())
        .unwrap_or_default()
        .to_string()
}

fn tunnel_of(one: &serde_json::Value) -> Tunnel {
    let port = one
        .get("port")
        .and_then(|range| range.get("from"))
        .and_then(|from| from.as_u64())
        .unwrap_or(0) as u16;
    let host = {
        let custom = text(one, "custom_domain");
        if custom.is_empty() {
            text(one, "assigned_domain")
        } else {
            custom
        }
    };
    let address = if host.is_empty() {
        String::new()
    } else if port == 25565 || port == 0 {
        host
    } else {
        format!("{host}:{port}")
    };
    Tunnel {
        id: text(one, "id"),
        name: text(one, "name"),
        local_port: one
            .get("local_port")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u16,
        address,
        disabled: one.get("disabled").is_some_and(|value| !value.is_null()),
    }
}

pub fn status(app: &AppHandle) -> Status {
    let has_secret = read_secret(app).is_some();
    let running = running();
    let mut error = String::new();
    if has_secret {
        if let Err(cause) = refresh(app, false) {
            error = cause;
        }
    }
    let live = lock();
    Status {
        has_secret,
        running,
        claim: live.claim.clone(),
        tunnels: live.tunnels.clone(),
        error: if error.is_empty() {
            live.error.clone()
        } else {
            error
        },
    }
}

pub fn tunnel_port_allowed(port: u16, web_panel_port: u16) -> Result<(), String> {
    if port == 0 {
        return Err("The server port is not set.".into());
    }
    if port == web_panel_port {
        return Err("The web panel port cannot be opened through playit. Only the Minecraft port can.".into());
    }
    Ok(())
}

pub fn expose(app: &AppHandle, id: &str) -> Result<Status, String> {
    let Some(secret) = read_secret(app) else {
        return Err("Link a playit account first.".into());
    };
    let instance = load_instances(app)?
        .into_iter()
        .find(|one| one.id == id)
        .ok_or_else(|| "Server not found.".to_string())?;
    tunnel_port_allowed(instance.port, crate::read_app_settings(app).web_panel_port)?;
    if !running() {
        start(app)?;
    }
    refresh(app, true)?;
    let (agent_id, already) = {
        let live = lock();
        (
            live.agent_id.clone(),
            live.tunnels
                .iter()
                .any(|one| one.local_port == instance.port),
        )
    };
    if already {
        return Ok(status(app));
    }
    if agent_id.is_empty() {
        return Err("playit did not return an agent id. Please try again in a moment.".into());
    }
    let body = |name: Option<&str>| {
        serde_json::json!({
            "name": name,
            "tunnel_type": "minecraft-java",
            "port_type": "tcp",
            "port_count": 1,
            "origin": {
                "type": "agent",
                "data": {
                    "agent_id": agent_id,
                    "local_ip": "127.0.0.1",
                    "local_port": instance.port,
                },
            },
            "enabled": true,
            "alloc": null,
            "firewall_id": null,
            "proxy_protocol": null,
        })
    };
    let made = off_runtime(|| call(Some(&secret), "/tunnels/create", body(Some(&instance.name))));
    if let Err(error) = made {
        if error.contains("InvalidTunnelName") {
            off_runtime(|| call(Some(&secret), "/tunnels/create", body(None)))?;
        } else if error.contains("NotAllowedWithReadOnly") || error.contains("AgentNotSelfManaged") {
            return Err("This playit link is read-only. Press 'Unlink playit' and link again.".into());
        } else {
            return Err(error);
        }
    }
    refresh(app, true)?;
    Ok(status(app))
}

pub fn unexpose(app: &AppHandle, id: &str) -> Result<Status, String> {
    let Some(secret) = read_secret(app) else {
        return Err("Link a playit account first.".into());
    };
    let instance = load_instances(app)?
        .into_iter()
        .find(|one| one.id == id)
        .ok_or_else(|| "Server not found.".to_string())?;
    refresh(app, true)?;
    let ids: Vec<String> = lock()
        .tunnels
        .iter()
        .filter(|one| one.local_port == instance.port)
        .map(|one| one.id.clone())
        .collect();
    for tunnel_id in ids {
        off_runtime(|| {
            call(
                Some(&secret),
                "/tunnels/delete",
                serde_json::json!({ "tunnel_id": tunnel_id }),
            )
        })?;
    }
    refresh(app, true)?;
    Ok(status(app))
}

pub fn forget(app: &AppHandle) -> Result<Status, String> {
    stop();
    let path = secret_path(app)?;
    if path.is_file() {
        std::fs::remove_file(&path).map_err(|error| format!("Could not delete the secret key: {error}"))?;
    }
    let mut live = lock();
    live.generation += 1;
    live.claim = None;
    live.tunnels.clear();
    live.agent_id.clear();
    live.fetched = None;
    live.error.clear();
    live.log.clear();
    drop(live);
    Ok(status(app))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_an_address_the_way_minecraft_wants_it() {
        let one = serde_json::json!({
            "id": "t1", "name": "survival", "local_port": 25565,
            "assigned_domain": "cold-frost.ply.gg", "custom_domain": null,
            "port": { "from": 12345, "to": 12345 }, "disabled": null
        });
        let tunnel = tunnel_of(&one);
        assert_eq!(tunnel.address, "cold-frost.ply.gg:12345");
        assert_eq!(tunnel.local_port, 25565);
        assert!(!tunnel.disabled);
        let two = serde_json::json!({
            "id": "t2", "local_port": 25566, "assigned_domain": "a.ply.gg",
            "custom_domain": "mc.example.com", "port": { "from": 25565, "to": 25565 },
            "disabled": "ByUser"
        });
        let tunnel = tunnel_of(&two);
        assert_eq!(tunnel.address, "mc.example.com");
        assert!(tunnel.disabled);
    }

    #[test]
    fn briefs_an_error() {
        assert_eq!(brief(&serde_json::json!("NotAccepted")), "NotAccepted");
        assert_eq!(
            brief(&serde_json::json!({ "auth": "InvalidAgentKey" })),
            "auth · InvalidAgentKey"
        );
        assert_eq!(
            brief(&serde_json::json!({ "message": "NotAllowedWithReadOnly", "type": "auth" })),
            "NotAllowedWithReadOnly (auth)"
        );
    }

    #[test]
    #[ignore]
    fn talks_to_the_real_playit_api() {
        let code: String = web::make_key().chars().take(10).collect();
        let answer = call(
            None,
            "/claim/setup",
            serde_json::json!({ "code": code, "agent_type": "assignable", "version": "Monolab test" }),
        );
        println!("{answer:?}");
        let state = answer.expect("the API should answer");
        assert!(
            matches!(state.as_str(), Some("WaitingForUserVisit") | Some("WaitingForUser")),
            "{state}"
        );
        let refused = call(None, "/claim/exchange", serde_json::json!({ "code": code }))
            .expect_err("should be refused before approval");
        println!("{refused}");
        assert!(
            ["CodeNotFound", "NotAccepted", "NotSetup"].iter().any(|word| refused.contains(word)),
            "{refused}"
        );
    }

    #[test]
    #[ignore]
    fn runs_the_real_daemon() {
        use std::io::Write as _;
        let dir = std::env::temp_dir().join("se-playit-test");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(format!("playitd-{VERSION}.exe"));
        if !exe.is_file() {
            println!("Downloading playit… (about 5 MB)");
            let client = crate::net::builder()
                .user_agent("Monolab test")
                .timeout(Duration::from_secs(300))
                .build()
                .unwrap();
            let body = client.get(DOWNLOAD).send().unwrap().error_for_status().unwrap().bytes().unwrap();
            assert!(body.starts_with(b"MZ"), "the download is not an executable");
            assert!(body.len() > 1024 * 1024, "the download is too small");
            std::fs::write(&exe, &body).unwrap();
        }
        println!("playitd {} bytes", std::fs::metadata(&exe).unwrap().len());
        let secret = dir.join("playit.toml");
        let mut file = std::fs::File::create(&secret).unwrap();
        writeln!(file, "secret_key = \"{}\"", "ab".repeat(32)).unwrap();
        drop(file);

        let mut child = std::process::Command::new(&exe)
            .arg("--secret-path")
            .arg(&secret)
            .args(["--socket-path", r"\\.\pipe\monolab-playitd-test"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("playitd should start");
        let (sender, receiver) = std::sync::mpsc::channel();
        let err = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let _ = sender.send(line);
            }
        });
        let began = Instant::now();
        let mut lines = Vec::new();
        while began.elapsed() < Duration::from_secs(12) {
            match receiver.recv_timeout(Duration::from_millis(500)) {
                Ok(line) => {
                    println!("  [playitd] {line}");
                    lines.push(line);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break,
            }
        }
        let pid = child.id();
        let _ = child.kill();
        crate::kill_process_tree(pid);
        let _ = child.wait();
        let all = lines.join("\n").to_ascii_lowercase();
        assert!(!all.contains("unexpected argument"), "wrong argument name");
        assert!(all.contains("starting playitd"), "The daemon did not start:\n{all}");
    }

    #[test]
    #[ignore]
    fn reads_real_tunnels() {
        let secret = std::env::var("SE_PLAYIT_SECRET").expect("SE_PLAYIT_SECRET is required");
        let data = call(Some(&secret), "/agents/rundata", serde_json::json!({})).unwrap();
        println!("{}", serde_json::to_string_pretty(&data).unwrap());
        for one in data["tunnels"].as_array().unwrap_or(&Vec::new()) {
            let tunnel = tunnel_of(one);
            println!("{} → local {} · {}", tunnel.address, tunnel.local_port, tunnel.name);
        }
    }
}

#[cfg(test)]
mod scope_tests {
    #[test]
    fn never_tunnels_the_web_panel_port() {
        assert!(super::tunnel_port_allowed(25565, 25580).is_ok());
        assert!(super::tunnel_port_allowed(25580, 25580).is_err());
        assert!(super::tunnel_port_allowed(0, 25580).is_err());
    }
}

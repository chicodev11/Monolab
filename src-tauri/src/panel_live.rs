
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tauri::{AppHandle, Manager};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

use crate::panel::{self, PanelStats, Target};
use crate::{AppState, ServerInstance, ServerLogEvent};

const KEEP_BLIND: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct PanelLive {
    pub stats: PanelStats,
    pub at: Instant,
}

pub fn target_of(instance: &ServerInstance) -> Target {
    Target { ident: instance.panel_server.clone(), daemon: instance.panel_daemon.clone() }
}

pub fn start_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let kick = app.state::<AppState>().panel_kick.clone();
        let mut first = true;
        loop {
            if !first {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                    _ = kick.notified() => {}
                }
            }
            first = false;
            let instances: Vec<ServerInstance> = crate::load_instances(&app)
                .unwrap_or_default()
                .into_iter()
                .filter(|one| !one.panel_host.is_empty())
                .collect();
            if instances.is_empty() {
                continue;
            }
            let mut apis: HashMap<String, Option<Arc<panel::Api>>> = HashMap::new();
            let mut asks = Vec::new();
            for one in &instances {
                let api = apis
                    .entry(one.panel_host.clone())
                    .or_insert_with(|| panel::api(&app, &one.panel_host).ok().map(Arc::new))
                    .clone();
                let target = target_of(one);
                let id = one.id.clone();
                asks.push(async move {
                    let got = match api {
                        Some(api) if api.has_account_key() => api.stats(&target).await,
                        Some(_) => Err("No account key".to_string()),
                        None => Err("No key".to_string()),
                    };
                    (id, got)
                });
            }
            let results = futures_util::future::join_all(asks).await;
            let state = app.state::<AppState>();
            let mut wanted_sockets = Vec::new();
            {
                let mut live = state.panel_live.lock().expect("panel state poisoned");
                let mut checked = state.panel_checked.lock().expect("panel state poisoned");
                for (id, got) in results {
                    checked.insert(id.clone());
                    match got {
                        Ok(stats) => {
                            if stats.state != "offline" {
                                wanted_sockets.push(id.clone());
                            }
                            live.insert(id, PanelLive { stats, at: Instant::now() });
                        }
                        Err(_) => {
                            if live.get(&id).is_some_and(|one| one.at.elapsed() > KEEP_BLIND) {
                                live.remove(&id);
                            }
                        }
                    }
                }
                live.retain(|id, _| instances.iter().any(|one| &one.id == id));
            }
            for id in wanted_sockets {
                if let Some(one) = instances.iter().find(|one| one.id == id) {
                    ensure_console(&app, one);
                }
            }
        }
    });
}

pub fn ensure_console(app: &AppHandle, instance: &ServerInstance) {
    let state = app.state::<AppState>();
    if !state.panel_sockets.lock().expect("panel state poisoned").insert(instance.id.clone()) {
        return;
    }
    let app = app.clone();
    let instance = instance.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_console(&app, &instance).await;
        app.state::<AppState>()
            .panel_sockets
            .lock()
            .expect("panel state poisoned")
            .remove(&instance.id);
        if let Err(error) = result {
            eprintln!("[panel] {} console disconnected: {error}", instance.name);
        }
    });
}

async fn run_console(app: &AppHandle, instance: &ServerInstance) -> Result<(), String> {
    let host = panel::host(app, &instance.panel_host)?;
    let api = panel::api(app, &instance.panel_host)?;
    let target = target_of(instance);
    let (socket, token) = api.socket(&target).await?;
    let mut request = socket
        .as_str()
        .into_client_request()
        .map_err(|error| format!("The console address is invalid: {error}"))?;
    request.headers_mut().insert(
        "Origin",
        host.url.parse().map_err(|_| "Could not put the panel address in the request header.".to_string())?,
    );
    let connector = if socket.starts_with("wss://") {
        use rustls_platform_verifier::BuilderVerifierExt;
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|error| error.to_string())?
        .with_platform_verifier()
        .map_err(|error| error.to_string())?
        .with_no_client_auth();
        Some(tokio_tungstenite::Connector::Rustls(Arc::new(config)))
    } else {
        let url = reqwest::Url::parse(&socket).map_err(|_| "Could not read the console address.".to_string())?;
        if !url.host_str().is_some_and(crate::net::is_private_host) {
            return Err("The node returned an unencrypted address (ws://). Nodes outside your network need wss.".into());
        }
        Some(tokio_tungstenite::Connector::Plain)
    };
    let (mut stream, _) = tokio::time::timeout(
        Duration::from_secs(15),
        tokio_tungstenite::connect_async_tls_with_config(request, None, false, connector),
    )
    .await
    .map_err(|_| "Timed out connecting to the console.".to_string())?
    .map_err(|error| format!("Could not connect to the console: {error}"))?;
    let say = |event: &str, args: serde_json::Value| {
        Message::text(serde_json::json!({ "event": event, "args": args }).to_string())
    };
    stream
        .send(say("auth", serde_json::json!([token])))
        .await
        .map_err(|error| error.to_string())?;
    let state = app.state::<AppState>();
    let logs = state.logs.clone();
    let mut checkup = tokio::time::interval(Duration::from_secs(30));
    loop {
        let message = tokio::select! {
            next = stream.next() => next,
            _ = checkup.tick() => {
                let still = crate::load_instances(app)
                    .unwrap_or_default()
                    .iter()
                    .any(|one| one.id == instance.id);
                if !still {
                    return Ok(());
                }
                continue;
            }
        };
        let Some(message) = message else { return Ok(()) };
        let message = message.map_err(|error| error.to_string())?;
        let Message::Text(text) = message else {
            if let Message::Close(_) = message {
                return Ok(());
            }
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text.as_str()) else { continue };
        let event = value.get("event").and_then(|one| one.as_str()).unwrap_or("");
        let first_arg = value
            .pointer("/args/0")
            .and_then(|one| one.as_str())
            .unwrap_or("")
            .to_string();
        match event {
            "auth success" => {
                let first = state
                    .panel_backlog
                    .lock()
                    .expect("panel state poisoned")
                    .insert(instance.id.clone());
                if first {
                    let _ = stream.send(say("send logs", serde_json::json!([null]))).await;
                }
            }
            "console output" | "install output" | "daemon message" => {
                for piece in first_arg.split('\n') {
                    let raw = piece.trim_end_matches('\r');
                    if raw.is_empty() {
                        continue;
                    }
                    crate::publish_log(
                        app,
                        &logs,
                        ServerLogEvent {
                            server_id: instance.id.clone(),
                            line: crate::strip_ansi(raw),
                            stream: "stdout".into(),
                            noise: false,
                            ansi: raw.to_string(),
                        },
                    );
                }
            }
            "status" => {
                let mut live = state.panel_live.lock().expect("panel state poisoned");
                let one = live.entry(instance.id.clone()).or_insert(PanelLive {
                    stats: PanelStats::default(),
                    at: Instant::now(),
                });
                one.stats.state = first_arg.clone();
                one.at = Instant::now();
            }
            "stats" => {
                if let Ok(stats) = serde_json::from_str::<serde_json::Value>(&first_arg) {
                    let mut live = state.panel_live.lock().expect("panel state poisoned");
                    if let Some(one) = live.get_mut(&instance.id) {
                        let number = |key: &str| stats.get(key).and_then(|one| one.as_f64()).unwrap_or(0.0);
                        one.stats.cpu = number("cpu_absolute");
                        one.stats.memory_bytes = number("memory_bytes");
                        one.stats.memory_limit_bytes = number("memory_limit_bytes");
                        one.stats.disk_bytes = number("disk_bytes");
                        one.stats.uptime_ms = number("uptime");
                        if let Some(state_now) = stats.get("state").and_then(|one| one.as_str()) {
                            one.stats.state = state_now.to_string();
                        }
                        one.at = Instant::now();
                    }
                }
            }
            "token expiring" => {
                let (_, fresh) = api.socket(&target).await?;
                stream
                    .send(say("auth", serde_json::json!([fresh])))
                    .await
                    .map_err(|error| error.to_string())?;
            }
            "token expired" | "jwt error" => return Err(format!("token expired ({event})")),
            _ => {}
        }
    }
}

pub async fn state_now(app: &AppHandle, instance: &ServerInstance) -> Result<String, String> {
    let api = panel::api(app, &instance.panel_host)?;
    let stats = api.stats(&target_of(instance)).await?;
    let state = stats.state.clone();
    app.state::<AppState>()
        .panel_live
        .lock()
        .expect("panel state poisoned")
        .insert(instance.id.clone(), PanelLive { stats, at: Instant::now() });
    Ok(state)
}

#[cfg(test)]
mod real_tests {
    use super::*;

    #[test]
    #[ignore]
    fn reads_a_real_panel() {
        let dir = std::path::PathBuf::from(std::env::var("APPDATA").unwrap()).join("com.monolab.desktop");
        let hosts: Vec<panel::PanelHost> =
            serde_json::from_str(&std::fs::read_to_string(dir.join("panel-hosts.json")).unwrap()).unwrap();
        let secrets: HashMap<String, String> =
            serde_json::from_str(&std::fs::read_to_string(dir.join("panel-secrets.json")).unwrap()).unwrap();
        let host = hosts.first().expect("registered panel");
        let api = panel::Api::new(
            &host.url,
            secrets.get(&host.id).map(String::as_str),
            secrets.get(&format!("{}#app", host.id)).map(String::as_str),
            host.role,
        )
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let account = api.account().await;
            println!("account: {:?}", account.as_ref().map(|one| (&one.username, one.admin)));
            let servers = api.servers().await.expect("server list");
            println!("{} servers", servers.len());
            for one in &servers {
                println!("  - {} [{}] node={} port={} mem={}MiB", one.name, one.identifier, one.node, one.port, one.memory_limit_mib);
            }
            let Some(first) = servers.first() else { return };
            let daemon = api.daemon(&first.identifier).await.unwrap_or_default();
            println!("daemon: {daemon}");
            let perms = api.permissions(&first.identifier).await.unwrap_or_default();
            println!("{} permissions", perms.len());
            let target = Target { ident: first.identifier.clone(), daemon };
            println!("state: {:?}", api.stats(&target).await);
            match api.read_head(&target, "logs/latest.log", 128 * 1024).await {
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    println!("log head {} bytes", bytes.len());
                    for line in text.lines().filter(|line| line.to_lowercase().contains("version")).take(8) {
                        println!("  | {line}");
                    }
                }
                Err(error) => println!("log head failed: {error}"),
            }
            match api.list(&target, "").await {
                Ok(rows) => println!("{} top-level files: {:?}", rows.len(), rows.iter().take(12).map(|one| &one.name).collect::<Vec<_>>()),
                Err(error) => println!("file list failed: {error}"),
            }
            match api.read_text(&target, "server.properties").await {
                Ok(text) => println!("server.properties {} lines", text.lines().count()),
                Err(error) => println!("server.properties failed: {error}"),
            }
            match api.startup(&target).await {
                Ok(one) => println!("start command: {} / {} variables / image {}", one.command, one.variables.len(), one.docker_image),
                Err(error) => println!("startup variables failed: {error}"),
            }
            match api.backups(&target).await {
                Ok(rows) => println!("{} backups", rows.len()),
                Err(error) => println!("backup failed: {error}"),
            }
            match api.read_bytes(&target, "server.properties", 1024 * 1024).await {
                Ok(bytes) => println!("signed download: {} bytes", bytes.len()),
                Err(error) => println!("signed download failed: {error}"),
            }
            if let Some(app_key) = secrets.get(&format!("{}#app", host.id)) {
                let only = panel::Api::new(&host.url, None, Some(app_key), panel::PanelRole::Admin).unwrap();
                println!("server list with only the Application key: {:?}", only.servers().await.map(|one| one.len()));
                println!("create options with only the Application key: {:?}", only.create_options().await.map(|one| one.eggs.len()));
            }
            if host.role == panel::PanelRole::Admin {
                match api.create_options().await {
                    Ok(one) => println!("create options: users {} · nodes {} · eggs {}", one.users.len(), one.nodes.len(), one.eggs.len()),
                    Err(error) => println!("create options failed: {error}"),
                }
            }
            let (socket, token) = match api.socket(&target).await {
                Ok(one) => one,
                Err(error) => return println!("websocket token failed: {error}"),
            };
            println!("websocket address: {socket}");
            let mut request = socket.as_str().into_client_request().unwrap();
            request.headers_mut().insert("Origin", host.url.parse().unwrap());
            let connector = if socket.starts_with("wss://") {
                use rustls_platform_verifier::BuilderVerifierExt;
                let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
                    .with_safe_default_protocol_versions().unwrap()
                    .with_platform_verifier().unwrap()
                    .with_no_client_auth();
                Some(tokio_tungstenite::Connector::Rustls(Arc::new(config)))
            } else {
                Some(tokio_tungstenite::Connector::Plain)
            };
            let (mut stream, _) = match tokio_tungstenite::connect_async_tls_with_config(request, None, false, connector).await {
                Ok(one) => one,
                Err(error) => return println!("websocket connect failed: {error}"),
            };
            stream.send(Message::text(serde_json::json!({"event":"auth","args":[token]}).to_string())).await.unwrap();
            let until = Instant::now() + Duration::from_secs(8);
            let mut shown = 0;
            while Instant::now() < until {
                let Ok(Some(Ok(message))) = tokio::time::timeout(Duration::from_secs(2), stream.next()).await else { continue };
                if let Message::Text(text) = message {
                    let value: serde_json::Value = serde_json::from_str(text.as_str()).unwrap_or_default();
                    let event = value.get("event").and_then(|one| one.as_str()).unwrap_or("").to_string();
                    if event == "auth success" {
                        let _ = stream.send(Message::text(serde_json::json!({"event":"send logs","args":[null]}).to_string())).await;
                    }
                    if shown < 12 {
                        let arg = value.pointer("/args/0").and_then(|one| one.as_str()).unwrap_or("");
                        println!("  ws: {event} {}", arg.chars().take(90).collect::<String>());
                        shown += 1;
                    }
                }
            }
        });
    }
}

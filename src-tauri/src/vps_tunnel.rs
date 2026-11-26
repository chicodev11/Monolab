//! Port forwarding through your own VPS (instead of playit): the app keeps an SSH connection to one of your saved
//! SSH servers and asks it to listen on a port (`ssh -R 0.0.0.0:<port>:127.0.0.1:<server port>`). Players join
//! `<VPS address>:<port>`. Java edition only (TCP). The tunnel runs while the app is open and comes back by itself
//! when the connection drops. The choice (which VPS, which port) is remembered per server in `vps-tunnels.json`.

use crate::data_root::MonolabDirs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Seconds between tries when the VPS can't be reached.
const RETRY_SECS: u64 = 10;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    host: String,
    port: u16,
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VpsTunnel {
    /// The saved SSH server, empty when none was picked yet.
    pub host: String,
    pub port: u16,
    /// Started (and trying to stay connected).
    pub running: bool,
    /// Connected right now: players can join `address`.
    pub connected: bool,
    pub address: String,
    pub error: String,
}

struct Live {
    task: tauri::async_runtime::JoinHandle<()>,
    status: Arc<Mutex<VpsTunnel>>,
}

fn live() -> &'static Mutex<HashMap<String, Live>> {
    static LIVE: OnceLock<Mutex<HashMap<String, Live>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn file(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("vps-tunnels.json"))
}

fn load(app: &AppHandle) -> HashMap<String, Saved> {
    file(app).ok().and_then(|path| std::fs::read_to_string(path).ok()).and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn store(app: &AppHandle, all: &HashMap<String, Saved>) -> Result<(), String> {
    let path = file(app)?;
    let text = serde_json::to_string_pretty(all).map_err(|error| error.to_string())?;
    std::fs::write(path, text).map_err(|error| format!("Could not save the tunnel settings: {error}"))
}

/// What the Network tab shows for a server.
pub fn state(app: &AppHandle, server: &str) -> VpsTunnel {
    if let Some(one) = live().lock().expect("tunnel poisoned").get(server) {
        return one.status.lock().expect("tunnel poisoned").clone();
    }
    let saved = load(app).remove(server).unwrap_or_default();
    VpsTunnel { host: saved.host, port: saved.port, ..Default::default() }
}

/// Starts (or restarts) the tunnel: `host` is a saved SSH server id, `port` the port players use on it.
pub fn start(app: &AppHandle, server: &str, host: &str, port: u16, local_port: u16) -> Result<VpsTunnel, String> {
    if port < 1024 {
        return Err("Pick a port from 1024 up (lower ones need root on the VPS).".into());
    }
    let one = crate::remote::host(app, host)?;
    stop(server);
    let mut all = load(app);
    all.insert(server.to_string(), Saved { host: host.to_string(), port });
    store(app, &all)?;
    let status = Arc::new(Mutex::new(VpsTunnel {
        host: host.to_string(),
        port,
        running: true,
        connected: false,
        address: format!("{}:{port}", one.address),
        error: String::new(),
    }));
    let task = tauri::async_runtime::spawn(keep(app.clone(), host.to_string(), port, local_port, status.clone()));
    let now = status.lock().expect("tunnel poisoned").clone();
    live().lock().expect("tunnel poisoned").insert(server.to_string(), Live { task, status });
    Ok(now)
}

async fn keep(app: AppHandle, host: String, port: u16, local_port: u16, status: Arc<Mutex<VpsTunnel>>) {
    loop {
        match crate::remote::reverse_tunnel(&app, &host, port, local_port).await {
            Ok(conn) => {
                {
                    let mut now = status.lock().expect("tunnel poisoned");
                    now.connected = true;
                    now.error.clear();
                }
                let guard = Closer(Some(conn.clone()));
                while !conn.is_closed() {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
                drop(guard);
                let mut now = status.lock().expect("tunnel poisoned");
                now.connected = false;
                now.error = "The connection to the VPS dropped. Trying again…".into();
            }
            Err(error) => {
                let mut now = status.lock().expect("tunnel poisoned");
                now.connected = false;
                now.error = error;
            }
        }
        tokio::time::sleep(Duration::from_secs(RETRY_SECS)).await;
    }
}

/// Closes the SSH connection when the tunnel task ends (stopped or aborted).
struct Closer(Option<Arc<crate::remote::Conn>>);

impl Drop for Closer {
    fn drop(&mut self) {
        if let Some(conn) = self.0.take() {
            tauri::async_runtime::spawn(async move { conn.close().await });
        }
    }
}

/// Stops the tunnel (the saved choice stays for next time).
pub fn stop(server: &str) {
    if let Some(one) = live().lock().expect("tunnel poisoned").remove(server) {
        one.task.abort();
    }
}

/// Stops and forgets the choice.
pub fn forget(app: &AppHandle, server: &str) -> Result<(), String> {
    stop(server);
    let mut all = load(app);
    if all.remove(server).is_some() {
        store(app, &all)?;
    }
    Ok(())
}

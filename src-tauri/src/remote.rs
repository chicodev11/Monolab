
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use base64::Engine;
use russh::client::{self, Handle};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::{ChannelMsg, Disconnect};
use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

const CONNECT_SECS: u64 = 12;
const EXEC_SECS: u64 = 30;
const LONG_SECS: u64 = 30 * 60;

#[derive(Clone, Debug, Default, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SshHost {
    pub id: String,
    pub name: String,
    pub address: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub auth: String,
    #[serde(default)]
    pub key_path: String,
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default)]
    pub root: String,
}

fn default_port() -> u16 {
    22
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SshHostView {
    #[serde(flatten)]
    pub host: SshHost,
    pub has_secret: bool,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HostKey {
    pub algorithm: String,
    pub fingerprint: String,
    pub changed: bool,
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    #[allow(dead_code)]
    pub fn ok(self) -> Result<String, String> {
        if self.code == 0 {
            return Ok(self.stdout);
        }
        let why = if self.stderr.trim().is_empty() {
            self.stdout.trim()
        } else {
            self.stderr.trim()
        };
        Err(format!("A command failed on the SSH server ({}): {why}", self.code))
    }
}

fn folder(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(dir)
}

fn hosts_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(folder(app)?.join("ssh-hosts.json"))
}

fn secrets_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(folder(app)?.join("ssh-secrets.json"))
}

pub fn load_hosts(app: &AppHandle) -> Result<Vec<SshHost>, String> {
    Ok(crate::store::read(&hosts_path(app)?)?.unwrap_or_default())
}

fn save_hosts(app: &AppHandle, hosts: &[SshHost]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(hosts).map_err(|error| error.to_string())?;
    crate::store::write(&hosts_path(app)?, &text)
        .map_err(|error| format!("Could not write ssh-hosts.json: {error}"))
}

fn load_secrets(app: &AppHandle) -> HashMap<String, String> {
    secrets_path(app)
        .ok()
        .and_then(|path| crate::store::read(&path).ok().flatten())
        .unwrap_or_default()
}

fn save_secrets(app: &AppHandle, secrets: &HashMap<String, String>) -> Result<(), String> {
    let text = serde_json::to_string_pretty(secrets).map_err(|error| error.to_string())?;
    crate::store::write(&secrets_path(app)?, &text)
        .map_err(|error| format!("Could not write ssh-secrets.json: {error}"))
}

pub fn list_hosts(app: &AppHandle) -> Result<Vec<SshHostView>, String> {
    let secrets = load_secrets(app);
    Ok(load_hosts(app)?
        .into_iter()
        .map(|host| SshHostView {
            has_secret: secrets.get(&host.id).is_some_and(|one| !one.is_empty()),
            host,
        })
        .collect())
}

pub fn host(app: &AppHandle, id: &str) -> Result<SshHost, String> {
    load_hosts(app)?
        .into_iter()
        .find(|one| one.id == id)
        .ok_or_else(|| "Registered SSH server not found.".to_string())
}

pub fn save_host(app: &AppHandle, one: SshHost, secret: Option<String>) -> Result<(), String> {
    if one.address.trim().is_empty() || one.username.trim().is_empty() {
        return Err("Please enter an address and username.".into());
    }
    if one.fingerprint.trim().is_empty() {
        return Err("Please confirm the host key fingerprint first.".into());
    }
    let mut hosts = load_hosts(app)?;
    match hosts.iter_mut().find(|have| have.id == one.id) {
        Some(have) => *have = one.clone(),
        None => hosts.push(one.clone()),
    }
    save_hosts(app, &hosts)?;
    if let Some(secret) = secret {
        let mut secrets = load_secrets(app);
        if secret.is_empty() {
            secrets.remove(&one.id);
        } else {
            secrets.insert(one.id.clone(), secret);
        }
        save_secrets(app, &secrets)?;
    }
    forget(&one.id);
    Ok(())
}

pub fn delete_host(app: &AppHandle, id: &str) -> Result<(), String> {
    let mut hosts = load_hosts(app)?;
    hosts.retain(|one| one.id != id);
    save_hosts(app, &hosts)?;
    let mut secrets = load_secrets(app);
    secrets.remove(id);
    save_secrets(app, &secrets)?;
    forget(id);
    Ok(())
}

fn fingerprint_of(key: &russh::keys::PublicKey) -> (String, String) {
    let bytes = russh::keys::PublicKeyBase64::public_key_bytes(key);
    let digest = Sha256::digest(&bytes);
    let text = base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest);
    (key.algorithm().to_string(), format!("SHA256:{text}"))
}

struct Client {
    expect: Option<String>,
    seen: Arc<Mutex<Option<(String, String)>>>,
}

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            return Ok(false);
        };
        let pair = fingerprint_of(key);
        let ok = match &self.expect {
            Some(want) => &pair.1 == want,
            None => true,
        };
        *self.seen.lock().await = Some(pair);
        Ok(ok)
    }
}

fn config() -> Arc<client::Config> {
    Arc::new(client::Config {
        inactivity_timeout: Some(Duration::from_secs(120)),
        keepalive_interval: Some(Duration::from_secs(20)),
        ..Default::default()
    })
}

pub async fn probe(app: &AppHandle, address: &str, port: u16) -> Result<HostKey, String> {
    let seen = Arc::new(Mutex::new(None));
    let handler = Client {
        expect: None,
        seen: seen.clone(),
    };
    let connect = client::connect(config(), (address.to_string(), port), handler);
    let handle = tokio::time::timeout(Duration::from_secs(CONNECT_SECS), connect)
        .await
        .map_err(|_| format!("Could not connect to {address}:{port} (timed out). Check the address and port."))?
        .map_err(|error| format!("Could not connect to {address}:{port}: {error}"))?;
    let pair = seen.lock().await.clone();
    let _ = handle
        .disconnect(Disconnect::ByApplication, "", "English")
        .await;
    let (algorithm, fingerprint) = pair.ok_or("The SSH server didn't send a host key.")?;
    let changed = load_hosts(app)?.iter().any(|one| {
        one.address == address && one.port == port && !one.fingerprint.is_empty() && one.fingerprint != fingerprint
    });
    Ok(HostKey {
        algorithm,
        fingerprint,
        changed,
    })
}

pub struct Conn {
    handle: Handle<Client>,
    shared: Mutex<Option<Arc<Sftp>>>,
    channels: Arc<tokio::sync::Semaphore>,
}

pub struct Sftp {
    inner: Option<SftpSession>,
    turn: Option<tokio::sync::OwnedSemaphorePermit>,
}

impl std::ops::Deref for Sftp {
    type Target = SftpSession;
    fn deref(&self) -> &SftpSession {
        self.inner.as_ref().expect("closed SFTP session")
    }
}

impl Drop for Sftp {
    fn drop(&mut self) {
        let (Some(inner), turn) = (self.inner.take(), self.turn.take()) else { return };
        tauri::async_runtime::spawn(async move {
            let _ = tokio::time::timeout(Duration::from_secs(5), inner.close()).await;
            drop(turn);
        });
    }
}

type Pool = Mutex<HashMap<String, Arc<Conn>>>;

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(Default::default)
}

pub fn forget(id: &str) {
    let id = id.to_string();
    tauri::async_runtime::spawn(async move {
        pool().lock().await.remove(&id);
    });
}

async fn dial(app: &AppHandle, one: &SshHost) -> Result<Arc<Conn>, String> {
    dial_with(one, load_secrets(app).get(&one.id).cloned()).await
}

async fn dial_with(one: &SshHost, secret: Option<String>) -> Result<Arc<Conn>, String> {
    if one.fingerprint.trim().is_empty() {
        return Err("This SSH server was registered without confirming its host key. Please register it again.".into());
    }
    let seen = Arc::new(Mutex::new(None));
    let handler = Client {
        expect: Some(one.fingerprint.clone()),
        seen,
    };
    let connect = client::connect(config(), (one.address.clone(), one.port), handler);
    let mut handle = tokio::time::timeout(Duration::from_secs(CONNECT_SECS), connect)
        .await
        .map_err(|_| format!("Could not connect to {} (timed out).", one.address))?
        .map_err(|error| {
            format!(
                "Could not connect to {}: {error}\nThis happens when the host key differs from the one registered. If the SSH server was reinstalled, remove it and register it again.",
                one.address
            )
        })?;

    let result = if one.auth == "password" {
        let password = secret.ok_or("No password saved. Enter it again in the SSH server settings.")?;
        handle
            .authenticate_password(&one.username, password)
            .await
            .map_err(|error| format!("Login failed: {error}"))?
    } else {
        let path = one.key_path.trim();
        if path.is_empty() {
            return Err("Please choose an SSH key file.".into());
        }
        let key = load_secret_key(path, secret.as_deref().filter(|one| !one.is_empty()))
            .map_err(|error| format!("Could not read the SSH key file: {error}"))?;
        handle
            .authenticate_publickey(
                &one.username,
                PrivateKeyWithHashAlg::new(Arc::new(key), None),
            )
            .await
            .map_err(|error| format!("Could not log in with the key: {error}"))?
    };
    if !matches!(result, russh::client::AuthResult::Success) {
        return Err(if one.auth == "password" {
            "Wrong username or password.".into()
        } else {
            "This key can't log in. Check that it's in `~/.ssh/authorized_keys` on the SSH server."
                .to_string()
        });
    }
    Ok(Arc::new(Conn {
        handle,
        shared: Mutex::new(None),
        channels: Arc::new(tokio::sync::Semaphore::new(4)),
    }))
}

pub async fn connect(app: &AppHandle, host_id: &str) -> Result<Arc<Conn>, String> {
    if let Some(have) = pool().lock().await.get(host_id) {
        if !have.handle.is_closed() {
            return Ok(have.clone());
        }
    }
    let one = host(app, host_id)?;
    let fresh = dial(app, &one).await?;
    pool()
        .lock()
        .await
        .insert(host_id.to_string(), fresh.clone());
    Ok(fresh)
}

impl Conn {
    pub async fn shared_sftp(&self) -> Result<Arc<Sftp>, String> {
        let mut held = self.shared.lock().await;
        if let Some(one) = held.as_ref() {
            return Ok(one.clone());
        }
        let turn = Arc::new(tokio::sync::Semaphore::new(1))
            .acquire_owned()
            .await
            .map_err(|_| "The SSH connection closed.".to_string())?;
        let one = Arc::new(self.open_sftp(turn).await?);
        *held = Some(one.clone());
        Ok(one)
    }

    pub async fn drop_shared_sftp(&self) {
        self.shared.lock().await.take();
    }

    async fn healed<T>(&self, result: Result<T, String>) -> Result<T, String> {
        if let Err(error) = &result {
            if !is_missing(error) {
                self.drop_shared_sftp().await;
            }
        }
        result
    }

    pub async fn run(&self, command: &str) -> Result<Output, String> {
        self.run_for(command, EXEC_SECS).await
    }

    pub async fn run_long(&self, command: &str) -> Result<Output, String> {
        self.run_for(command, LONG_SECS).await
    }

    async fn run_for(&self, command: &str, secs: u64) -> Result<Output, String> {
        let work = async {
            let mut channel = self
                .handle
                .channel_open_session()
                .await
                .map_err(|error| format!("Could not open an SSH channel: {error}"))?;
            channel
                .exec(true, command)
                .await
                .map_err(|error| format!("Could not send the command: {error}"))?;
            let mut out = Output::default();
            let mut stdout: Vec<u8> = Vec::new();
            let mut stderr: Vec<u8> = Vec::new();
            while let Some(message) = channel.wait().await {
                match message {
                    ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                    ChannelMsg::ExtendedData { data, .. } => stderr.extend_from_slice(&data),
                    ChannelMsg::ExitStatus { exit_status } => out.code = exit_status as i32,
                    ChannelMsg::Close | ChannelMsg::Eof => {}
                    _ => {}
                }
            }
            out.stdout = String::from_utf8_lossy(&stdout).into_owned();
            out.stderr = String::from_utf8_lossy(&stderr).into_owned();
            Ok::<Output, String>(out)
        };
        tokio::time::timeout(Duration::from_secs(secs), work)
            .await
            .map_err(|_| format!("The SSH server didn't respond within {secs} seconds."))?
    }

    #[allow(dead_code)]
    pub async fn sftp(&self) -> Result<Sftp, String> {
        let turn = self
            .channels
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "The SSH connection closed.".to_string())?;
        self.open_sftp(turn).await
    }

    async fn open_sftp(&self, turn: tokio::sync::OwnedSemaphorePermit) -> Result<Sftp, String> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .map_err(|error| format!("Could not open an SSH channel: {error}"))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|error| format!("Could not open SFTP: {error}"))?;
        let inner = SftpSession::new(channel.into_stream())
            .await
            .map_err(|error| format!("Could not start SFTP: {error}"))?;
        Ok(Sftp { inner: Some(inner), turn: Some(turn) })
    }
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BoxInfo {
    pub os: String,
    pub java: String,
    pub screen: bool,
    pub tmux: bool,
    pub servers: Vec<FoundServer>,
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FoundServer {
    pub pid: u32,
    pub user: String,
    pub folder: String,
    pub command: String,
}

pub async fn survey(app: &AppHandle, host_id: &str) -> Result<BoxInfo, String> {
    let conn = connect(app, host_id).await?;

    let os = conn
        .run(". /etc/os-release 2>/dev/null && printf %s \"$PRETTY_NAME\" || uname -sr")
        .await?
        .stdout;

    let java = conn
        .run("bash -lc 'java -version 2>&1 | head -1; command -v java' 2>&1")
        .await?
        .stdout;
    let mut java_lines = java.lines().map(str::trim).filter(|line| !line.is_empty());
    let java = match (java_lines.next(), java_lines.next()) {
        (Some(version), Some(path)) if path.starts_with('/') => format!("{version} · {path}"),
        (Some(version), _) if !version.contains("not found") => version.to_string(),
        _ => String::new(),
    };

    let tools = conn
        .run("command -v screen >/dev/null && echo screen; command -v tmux >/dev/null && echo tmux")
        .await?
        .stdout;

    Ok(BoxInfo {
        os: os.trim().to_string(),
        java: java.trim().to_string(),
        screen: tools.contains("screen"),
        tmux: tools.contains("tmux"),
        servers: running_servers(&conn).await?,
    })
}

async fn running_servers(conn: &Conn) -> Result<Vec<FoundServer>, String> {
    let listing = conn
        .run("ps -eo pid=,user=,args= | grep -i java | grep -viE 'grep -|ps -eo'")
        .await?
        .stdout;
    let mut found = Vec::new();
    for line in listing.lines() {
        let Some((pid, user, command)) = split_ps_line(line) else {
            continue;
        };
        if !looks_like_minecraft(&command) {
            continue;
        }
        let folder = conn
            .run(&format!("readlink /proc/{pid}/cwd 2>/dev/null"))
            .await?
            .stdout
            .trim()
            .to_string();
        if !folder.is_empty() && found.iter().any(|one: &FoundServer| one.folder == folder) {
            continue;
        }
        found.push(FoundServer {
            pid,
            user,
            folder,
            command,
        });
    }
    Ok(found)
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub directory: bool,
    pub looks_like_server: bool,
}

pub async fn browse(app: &AppHandle, host_id: &str, path: &str) -> Result<Vec<RemoteEntry>, String> {
    let conn = connect(app, host_id).await?;
    let path = path.trim();
    let root = if path.is_empty() {
        conn.run("printf %s \"$HOME\"").await?.stdout
    } else {
        path.to_string()
    };
    let root = root.trim_end_matches('/').to_string();
    let root = if root.is_empty() { "/".to_string() } else { root };

    let sftp = conn.shared_sftp().await?;
    let mut entries = Vec::new();
    let listing = conn
        .healed(
            sftp.read_dir(&root)
                .await
                .map_err(|error| format!("Cannot open {root}: {error}")),
        )
        .await?;
    for item in listing {
        let name = item.file_name();
        if name.starts_with('.') || !item.file_type().is_dir() {
            continue;
        }
        let full = format!("{root}/{name}");
        entries.push(RemoteEntry {
            looks_like_server: sftp.metadata(format!("{full}/server.properties")).await.is_ok()
                || sftp.metadata(format!("{full}/pumpkin.toml")).await.is_ok(),
            name,
            path: full,
            directory: true,
        });
    }
    entries.sort_by_key(|one| one.name.to_lowercase());
    Ok(entries)
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Detected {
    pub core: String,
    pub version: String,
    pub port: u16,
    pub jar: String,
}

const EVIDENCE: &str = "echo '@jars'; ls -1 *.jar 2>/dev/null | head -20\n\
     echo '@marker'; { [ -f server.properties ] || [ -f eula.txt ]; } && echo yes\n\
     echo '@pumpkin'; [ -f pumpkin.toml ] && echo yes\n\
     echo '@neoforge'; ls -1 libraries/net/neoforged/neoforge 2>/dev/null | head -1\n\
     echo '@forge'; ls -1 libraries/net/minecraftforge/forge 2>/dev/null | head -1\n\
     echo '@fabric'; { [ -d .fabric ] || [ -f fabric-server-launch.properties ]; } && echo yes\n\
     echo '@plugins'; [ -d plugins ] && echo yes\n\
     echo '@mods'; [ -d mods ] && echo yes\n\
     echo '@port'; grep -E '^server-port=' server.properties 2>/dev/null | head -1\n\
     echo '@history'; head -c 400 version_history.json 2>/dev/null; echo\n\
     echo '@log'; head -40 logs/latest.log 2>/dev/null";

pub async fn detect(app: &AppHandle, host_id: &str, folder: &str) -> Result<Detected, String> {
    let conn = connect(app, host_id).await?;
    let script = format!(
        "cd {} 2>/dev/null || {{ echo MISSING; exit 0; }}\n{}\n",
        quote(folder),
        EVIDENCE
    );
    let text = conn.run(&script).await?.stdout;
    if text.contains("MISSING") {
        return Err(format!("Could not open the folder {folder}."));
    }
    Ok(read_evidence(&text))
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FoundFolder {
    pub name: String,
    pub path: String,
    #[serde(flatten)]
    pub found: Detected,
}

pub async fn scan(app: &AppHandle, host_id: &str) -> Result<Vec<FoundFolder>, String> {
    let one = host(app, host_id)?;
    let conn = connect(app, host_id).await?;
    let root = if one.root.trim().is_empty() {
        server_root(app, host_id).await?
    } else {
        one.root.trim().trim_end_matches('/').to_string()
    };
    if root.is_empty() {
        return Err("Please choose the server folder location.".into());
    }
    let script = format!(
        "cd {} 2>/dev/null || {{ echo NOROOT; exit 0; }}\n\
         for d in */; do\n\
           d=${{d%/}}\n\
           [ -d \"$d\" ] || continue\n\
           echo \"@@folder $d\"\n\
           (cd \"$d\" || exit 0\n{}\n)\n\
         done\n",
        quote(&root),
        EVIDENCE
    );
    let text = conn.run(&script).await?.stdout;
    if text.contains("NOROOT") {
        return Err(format!("Could not open the folder {root}. Check the server folder location."));
    }
    Ok(read_scan(&text, &root))
}

fn read_scan(text: &str, root: &str) -> Vec<FoundFolder> {
    let mut found = Vec::new();
    for chunk in text.split("@@folder ").skip(1) {
        let mut lines = chunk.lines();
        let Some(name) = lines.next().map(str::trim) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let rest: String = lines.collect::<Vec<_>>().join("\n");
        if !is_server_folder(&rest) {
            continue;
        }
        let seen = read_evidence(&rest);
        found.push(FoundFolder {
            path: format!("{root}/{name}"),
            name: name.to_string(),
            found: seen,
        });
    }
    found
}

fn is_server_folder(text: &str) -> bool {
    let has = |key: &str| {
        let mut on = false;
        for line in text.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix('@') {
                on = name == key;
            } else if on && !line.is_empty() {
                return true;
            }
        }
        false
    };
    if has("pumpkin") || has("neoforge") || has("forge") {
        return true;
    }
    let seen = read_evidence(text);
    has("marker") && (seen.core != crate::sniff::UNKNOWN || !seen.jar.is_empty())
}

fn read_evidence(text: &str) -> Detected {
    let mut part = String::new();
    let mut bag: HashMap<String, Vec<String>> = HashMap::new();
    for line in text.lines() {
        if let Some(name) = line.trim().strip_prefix('@') {
            part = name.to_string();
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        bag.entry(part.clone()).or_default().push(line.trim().into());
    }
    let first = |key: &str| bag.get(key).and_then(|lines| lines.first()).cloned();
    let jars = bag.get("jars").cloned().unwrap_or_default();

    let mut core = jars.iter().find_map(|name| crate::sniff::core_in_name(name));
    let mut version = jars.iter().find_map(|name| version_in_name(name));

    if core.is_none() {
        if first("pumpkin").is_some() {
            core = Some("Pumpkin".into());
        } else if let Some(build) = first("neoforge") {
            core = Some("NeoForge".into());
            version = version.or_else(|| crate::cores::neoforge_game_version(&build));
        } else if let Some(build) = first("forge") {
            core = Some("Forge".into());
            version = version.or_else(|| build.split('-').next().map(str::to_owned));
        } else if first("fabric").is_some() {
            core = Some("Fabric".into());
        } else if first("plugins").is_some() && first("mods").is_none() {
            core = Some("Paper".into());
        }
    }

    if version.is_none() {
        version = bag
            .get("history")
            .into_iter()
            .flatten()
            .find_map(|line| version_in_history(line));
    }

    if version.is_none() {
        version = bag
            .get("log")
            .into_iter()
            .flatten()
            .find_map(|line| version_in_log(line));
    }

    Detected {
        core: core.unwrap_or_else(|| crate::sniff::UNKNOWN.into()),
        version: version.unwrap_or_default(),
        port: first("port")
            .and_then(|line| line.split('=').nth(1).map(str::to_owned))
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(25565),
        jar: jars.first().cloned().unwrap_or_default(),
    }
}

fn version_in_name(name: &str) -> Option<String> {
    name.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|bit| bit.trim_matches('.'))
        .find(|bit| bit.matches('.').count() >= 1 && bit.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_owned)
}

pub(crate) fn version_in_log(line: &str) -> Option<String> {
    if let Some(found) = mc_in_parens(line) {
        return Some(found);
    }
    let lower = line.to_lowercase();
    let at = lower.find("version ").map(|at| at + "version ".len()).or_else(|| {
        lower
            .find("starting pumpkin ")
            .map(|at| at + "starting pumpkin ".len())
    })?;
    let rest = line.get(at..)?.trim();
    let word = rest.split_whitespace().next()?;
    (!word.is_empty()).then(|| word.to_string())
}

fn version_in_history(line: &str) -> Option<String> {
    mc_in_parens(line)
}

fn mc_in_parens(line: &str) -> Option<String> {
    let at = line.find("(MC:")? + "(MC:".len();
    let rest = line.get(at..)?;
    let end = rest.find(')')?;
    let word = rest.get(..end)?.trim();
    (!word.is_empty() && word.starts_with(|c: char| c.is_ascii_digit())).then(|| word.to_string())
}

fn split_ps_line(line: &str) -> Option<(u32, String, String)> {
    let line = line.trim();
    let mut bits = line.splitn(3, char::is_whitespace);
    let pid = bits.next()?.parse::<u32>().ok()?;
    let user = bits.next()?.trim().to_string();
    let command = bits.next()?.trim().to_string();
    if command.is_empty() {
        return None;
    }
    Some((pid, user, command))
}

fn looks_like_minecraft(command: &str) -> bool {
    let lower = command.to_lowercase();
    lower.contains("-jar")
        || [
            "paper", "purpur", "leaf", "folia", "fabric", "forge", "neoforge", "spigot", "minecraft",
        ]
        .iter()
        .any(|name| lower.contains(name))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    Tmux,
    Screen,
}

impl Holder {
    pub fn session(id: &str) -> String {
        let safe: String = id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        format!("se-{safe}")
    }

    pub fn start(self, name: &str, folder: &str, command: &str) -> String {
        match self {
            Holder::Tmux => format!(
                "tmux new-session -d -s {} -c {} {}",
                quote(name),
                quote(folder),
                quote(command)
            ),
            Holder::Screen => format!(
                "cd {} && screen -dmS {} sh -c {}",
                quote(folder),
                quote(name),
                quote(command)
            ),
        }
    }

    pub fn alive(self, name: &str) -> String {
        match self {
            Holder::Tmux => format!("tmux has-session -t {} 2>/dev/null", quote(name)),
            Holder::Screen => format!(
                "screen -ls | grep -q {}",
                quote(&format!("\\.{name}\\s"))
            ),
        }
    }

    pub fn kill(self, name: &str) -> String {
        match self {
            Holder::Tmux => format!("tmux kill-session -t {}", quote(name)),
            Holder::Screen => format!("screen -S {} -X quit", quote(name)),
        }
    }

    pub fn send(self, name: &str, line: &str) -> String {
        match self {
            Holder::Tmux => format!(
                "tmux send-keys -t {} -l {} && tmux send-keys -t {} Enter",
                quote(name),
                quote(line),
                quote(name)
            ),
            Holder::Screen => format!(
                "screen -S {} -X stuff {}",
                quote(name),
                quote(&format!("{line}\n"))
            ),
        }
    }
}

pub fn launch_line(
    custom: &str,
    java: &str,
    launch: &Launch,
    min_mib: u32,
    max_mib: u32,
    jvm_args: &str,
) -> Result<String, String> {
    let custom = custom.trim();
    if !custom.is_empty() {
        return Ok(custom.to_string());
    }
    let jar = launch.jar.as_str();
    if jar.trim().is_empty() && launch.args.is_empty() {
        return Err("Found nothing to start in the server folder — no jar and no NeoForge/Forge args file (libraries/…/unix_args.txt). Check that the server folder is correct.".into());
    }
    let mut bits = vec![
        java_word(java)?,
        "-Dstdout.encoding=UTF-8".into(),
        "-Dstderr.encoding=UTF-8".into(),
        "-Duser.language=en".into(),
        "-Duser.country=US".into(),
    ];
    if !launch.args.is_empty() && launch.user_args {
        bits.push("@user_jvm_args.txt".into());
    }
    bits.push(format!("-Xms{min_mib}M"));
    bits.push(format!("-Xmx{max_mib}M"));
    for arg in jvm_args.split_whitespace() {
        bits.push(arg.to_string());
    }
    if launch.args.is_empty() {
        bits.push("-jar".into());
        bits.push(quote(jar.trim()));
    } else {
        bits.push(quote(&format!("@{}", launch.args)));
    }
    bits.push("nogui".into());
    Ok(bits.join(" "))
}

pub async fn holder_of(conn: &Conn) -> Result<Holder, String> {
    let seen = conn
        .run("command -v tmux >/dev/null && echo tmux; command -v screen >/dev/null && echo screen")
        .await?
        .stdout;
    if seen.contains("tmux") {
        Ok(Holder::Tmux)
    } else if seen.contains("screen") {
        Ok(Holder::Screen)
    } else {
        Err("Neither tmux nor screen is installed. `sudo apt install tmux` is all it takes.".into())
    }
}

pub async fn start(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    id: &str,
    command: &str,
    restart: bool,
) -> Result<(), String> {
    let conn = connect(app, host_id).await?;
    let holder = holder_of(&conn).await?;
    let name = Holder::session(id);
    if conn.run(&holder.alive(&name)).await?.code == 0 {
        return Err("This server is already running.".into());
    }
    let _ = conn.run(&format!("rm -f {}", quote(&launch_log(id)))).await;
    conn.run(&holder.start(&name, folder, &launch_wrapper(id, command, restart)))
        .await?
        .ok()?;
    Ok(())
}

pub async fn confirm_started(app: &AppHandle, host_id: &str, id: &str) -> Result<(), String> {
    let conn = connect(app, host_id).await?;
    let holder = holder_of(&conn).await?;
    let name = Holder::session(id);
    tokio::time::sleep(Duration::from_secs(3)).await;
    if conn.run(&holder.alive(&name)).await?.code != 0 {
        let why = conn
            .run(&format!("tail -n 6 {} 2>/dev/null", quote(&launch_log(id))))
            .await
            .map(|out| out.stdout.trim().to_string())
            .unwrap_or_default();
        return Err(if why.is_empty() {
            "The server stopped right after starting. Check latest.log in Logs.".into()
        } else {
            format!("The server stopped right after starting: {why}")
        });
    }
    Ok(())
}

fn java_word(java: &str) -> Result<String, String> {
    let java = java.trim();
    if java.is_empty() || java == "java" {
        return Ok("java".into());
    }
    check_java_path(java)?;
    Ok(quote(java))
}

pub fn check_java_path(path: &str) -> Result<(), String> {
    let path = path.trim();
    if !path.starts_with('/') {
        return Err("Java on the SSH server must be a full path starting with /.".into());
    }
    if path.chars().any(|c| c.is_control()) || path.split('/').any(|bit| bit == "..") {
        return Err("The Java path contains characters that aren't allowed.".into());
    }
    if path.rsplit('/').next() != Some("java") {
        return Err("The Java executable must be named java (…/bin/java).".into());
    }
    Ok(())
}

pub(crate) async fn javas(app: &AppHandle, host_id: &str) -> Result<Vec<crate::java::JavaRuntime>, String> {
    let conn = connect(app, host_id).await?;
    let script = r#"seen=""
emit() {
  j="$1"; src="$2"
  [ -x "$j" ] || return 0
  real=$(readlink -f "$j" 2>/dev/null || echo "$j")
  case " $seen " in *" $real "*) return 0;; esac
  seen="$seen $real"
  ver=$("$j" -version 2>&1 | head -n 1)
  printf 'J\t%s\t%s\t%s\n' "$j" "$src" "$ver"
}
d=$(bash -lc 'command -v java' 2>/dev/null | tail -n 1)
[ -n "$d" ] && emit "$d" path
for j in $(update-alternatives --list java 2>/dev/null); do emit "$j" alternatives; done
for j in /usr/lib/jvm/*/bin/java /opt/*/bin/java /opt/*/*/bin/java "$HOME"/.sdkman/candidates/java/*/bin/java "$HOME"/.jdks/*/bin/java "$HOME"/java/*/bin/java; do emit "$j" folder; done
"#;
    let text = conn.run(script).await?.stdout;
    Ok(read_javas(&text))
}

fn read_javas(text: &str) -> Vec<crate::java::JavaRuntime> {
    let mut found = Vec::new();
    for line in text.lines() {
        let mut bits = line.trim_start().splitn(4, '\t');
        if bits.next() != Some("J") {
            continue;
        }
        let (Some(path), Some(source), Some(version)): (Option<&str>, Option<&str>, Option<&str>) =
            (bits.next(), bits.next(), bits.next())
        else {
            continue;
        };
        let Some(major) = crate::java::java_major_from_output(version.as_bytes()) else {
            continue;
        };
        let source = match source {
            "path" => "PATH default".to_string(),
            "alternatives" => "update-alternatives".to_string(),
            _ => path
                .trim_end_matches("/bin/java")
                .rsplit('/')
                .next()
                .unwrap_or(path)
                .to_string(),
        };
        found.push(crate::java::JavaRuntime {
            path: path.to_string(),
            major,
            source,
            managed: false,
        });
    }
    found
}

pub async fn precheck(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    jar: &str,
    java: &str,
    needs_java: u32,
) -> Result<Vec<crate::doctor::Finding>, String> {
    use crate::doctor::Finding;
    let conn = connect(app, host_id).await?;
    let jar = jar.trim();
    let java_word = java_word(java)?;
    let script = format!(
        "cd {folder} || exit 3\n\
         {jar_test}\n\
         [ -f eula.txt ] && echo eulafile\n\
         grep -qi '^eula=true' eula.txt 2>/dev/null && echo eula\n\
         echo \"javaver:$(bash -lc {java_test} 2>&1 | head -n 1)\"\n",
        folder = quote(folder),
        java_test = quote(&format!("{java_word} -version")),
        jar_test = if jar.is_empty() {
            "echo nojar".to_string()
        } else {
            format!("[ -f {} ] && echo jar", quote(jar))
        },
    );
    let out = conn.run(&script).await?;
    if out.code == 3 {
        return Ok(vec![Finding::stop(
            "Server folder not found",
            &format!("The folder {folder} doesn't exist on the SSH server. If you moved it, import the server again."),
        )]);
    }
    let seen = |word: &str| out.stdout.lines().any(|line| line.trim() == word);
    let mut found = Vec::new();
    if !jar.is_empty() && !seen("jar") {
        found.push(Finding::stop(
            "Server file is missing",
            &format!("{jar} is missing from the server folder on the SSH server. Please check in Files."),
        ));
    }
    if seen("eulafile") && !seen("eula") {
        found.push(Finding::stop(
            "The Minecraft EULA has not been accepted",
            "Open eula.txt in the server folder and change eula=false to eula=true.",
        ));
    }
    let version_line = out
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("javaver:"))
        .unwrap_or("");
    match crate::java::java_major_from_output(version_line.as_bytes()) {
        None => found.push(Finding::stop(
            "Java was not found on the SSH server",
            if java_word == "java" {
                "Java isn't installed on Ubuntu or isn't on PATH. Install it in a terminal with `sudo apt install openjdk-21-jre-headless` (use 25 for 26.x)."
            } else {
                "There's no Java at the chosen path. Pick Java again in Overview."
            },
        )),
        Some(major) if major < needs_java => found.push(Finding::stop(
            &format!("Java {major} on the SSH server can't run this server"),
            &format!("This Minecraft version needs Java {needs_java} or newer. Please install a newer Java on Ubuntu."),
        )),
        Some(_) => {}
    }
    Ok(found)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Launch {
    pub jar: String,
    pub args: String,
    pub user_args: bool,
    pub from_win: String,
}

pub async fn resolve_launch(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    jar: &str,
    core: &str,
) -> Result<Launch, String> {
    let conn = connect(app, host_id).await?;
    let script = format!(
        "cd {} 2>/dev/null || {{ echo @nofolder; exit 0; }}\n\
         ls -1 *.jar 2>/dev/null\n\
         echo @args\n\
         grep -Eho '@[^ ]*(unix|win)_args[.]txt' run.sh run.bat 2>/dev/null | head -n 1\n\
         find libraries -maxdepth 8 \\( -name unix_args.txt -o -name win_args.txt \\) 2>/dev/null\n\
         echo @user\n\
         [ -f user_jvm_args.txt ] && echo yes\n",
        quote(folder)
    );
    let out = conn.run(&script).await?;
    if out.stdout.lines().any(|line| line.trim() == "@nofolder") {
        return Err(format!(
            "The server folder ({folder}) doesn't exist on the SSH server. Check whether it was moved or renamed."
        ));
    }
    let launch = read_launch(&out.stdout, jar, core);
    if !launch.from_win.is_empty() {
        let made = conn
            .run(&format!(
                "cd {} && sed -e 's/;/:/g' -e 's#\\\\#/#g' {} > {}",
                quote(folder),
                quote(&launch.from_win),
                quote(&launch.args)
            ))
            .await?;
        if made.code != 0 {
            return Err(format!(
                "Couldn't convert {} for Linux: {}",
                launch.from_win,
                made.stderr.trim()
            ));
        }
    }
    Ok(launch)
}

pub fn read_launch(text: &str, wanted: &str, core: &str) -> Launch {
    let mut part = "jars";
    let (mut jars, mut named, mut listed, mut user_args) = (Vec::new(), None, Vec::new(), false);
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        match line {
            "@args" => part = "args",
            "@user" => part = "user",
            "@nofolder" => {}
            _ => match part {
                "jars" => {
                    if !line.to_ascii_lowercase().contains("installer") {
                        jars.push(line.to_string());
                    }
                }
                "args" => {
                    let path = line.replace('\\', "/");
                    let path = path.trim_start_matches("./").to_string();
                    if let Some(path) = path.strip_prefix('@') {
                        named.get_or_insert_with(|| path.to_string());
                    } else {
                        listed.push(path);
                    }
                }
                _ => user_args |= line == "yes",
            },
        }
    }
    let folder_of = |path: &str| path.rsplit_once('/').map(|(dir, _)| dir.to_string()).unwrap_or_default();
    let mut dirs: Vec<String> = listed.iter().map(|path| folder_of(path)).collect();
    dirs.sort();
    dirs.dedup();
    let dir = named
        .map(|path| folder_of(&path))
        .filter(|dir| dirs.contains(dir))
        .or_else(|| {
            dirs.iter()
                .max_by_key(|dir| {
                    dir.rsplit('/')
                        .next()
                        .unwrap_or("")
                        .split(|c: char| !c.is_ascii_digit())
                        .filter(|part| !part.is_empty())
                        .map(|part| part.parse::<u64>().unwrap_or(0))
                        .collect::<Vec<_>>()
                })
                .cloned()
        });
    let installed = matches!(core, "Forge" | "NeoForge");
    match dir {
        Some(dir) if installed || jars.is_empty() => {
            let unix = format!("{dir}/unix_args.txt");
            let win = format!("{dir}/win_args.txt");
            let from_win = if listed.contains(&unix) { String::new() } else { win };
            Launch {
                jar: String::new(),
                args: unix,
                user_args,
                from_win,
            }
        }
        _ => Launch {
            jar: pick_jar(wanted, &jars),
            args: String::new(),
            user_args,
            from_win: String::new(),
        },
    }
}

pub fn pick_jar(wanted: &str, jars: &[String]) -> String {
    let wanted = wanted.trim();
    if jars.is_empty() || jars.iter().any(|one| one == wanted) {
        return wanted.to_string();
    }
    jars.iter()
        .find(|one| one.eq_ignore_ascii_case("server.jar"))
        .or_else(|| jars.iter().find(|one| crate::sniff::core_in_name(one).is_some()))
        .or_else(|| jars.first())
        .cloned()
        .unwrap_or_else(|| wanted.to_string())
}

pub fn launch_log(id: &str) -> String {
    format!("/tmp/monolab-{}.log", Holder::session(id))
}

const RESTART_LOOP: &str = r#"tries=()
while :; do
began=$(date +%s)
{ @COMMAND@
} 2> >(tee -a @LOG@ >&2); code=$?; wait $!
tail -n 400 logs/latest.log 2>/dev/null | grep -q 'Stopping the server' && exit $code
now=$(date +%s)
if [ $((now - began)) -lt 30 ]; then echo '[Monolab] Stopped within 30 seconds of starting. Not restarting.'; exit $code; fi
kept=(); for t in "${tries[@]}"; do [ $((now - t)) -lt 600 ] && kept+=("$t"); done; tries=("${kept[@]}")
if [ ${#tries[@]} -ge 3 ]; then echo '[Monolab] Crashed three times within 10 minutes. Not restarting.'; exit $code; fi
tries+=("$now")
echo "[Monolab] The server stopped unexpectedly (exit $code). Restarting in 5 seconds (${#tries[@]}/3)."
sleep 5
done"#;

pub fn launch_wrapper(id: &str, command: &str, restart: bool) -> String {
    let log = quote(&launch_log(id));
    let inner = if restart {
        RESTART_LOOP.replace("@LOG@", &log).replace("@COMMAND@", command)
    } else {
        format!("{{ {command}\n}} 2> >(tee {log} >&2); code=$?; wait $!; exit $code")
    };
    format!("bash -lc {}", quote(&inner))
}

pub async fn send_line(
    app: &AppHandle,
    host_id: &str,
    id: &str,
    line: &str,
    session: Option<String>,
) -> Result<(), String> {
    let conn = connect(app, host_id).await?;
    let holder = holder_of(&conn).await?;
    let name = session.unwrap_or_else(|| Holder::session(id));
    if conn.run(&holder.alive(&name)).await?.code != 0 {
        return Err("This server isn't running.".into());
    }
    conn.run(&holder.send(&name, line)).await?.ok()?;
    Ok(())
}

pub async fn force_stop(
    app: &AppHandle,
    host_id: &str,
    id: &str,
    session: Option<String>,
    pid: u32,
) -> Result<(), String> {
    let conn = connect(app, host_id).await?;
    let name = session.unwrap_or_else(|| Holder::session(id));
    let mut cut = false;
    if let Ok(holder) = holder_of(&conn).await {
        if conn.run(&holder.alive(&name)).await.map(|one| one.code == 0).unwrap_or(false) {
            conn.run(&holder.kill(&name)).await?.ok()?;
            cut = true;
        }
    }
    if pid > 1 {
        let _ = conn.run(&format!("kill -9 {pid}")).await;
        cut = true;
    }
    if !cut {
        return Err("Nothing to kill — it seems to have stopped already.".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Live {
    pub pid: u32,
    pub session: String,
    pub ours: bool,
}

pub async fn live_state(
    app: &AppHandle,
    host_id: &str,
    servers: &[(String, String)],
) -> HashMap<String, Live> {
    live_state_checked(app, host_id, servers).await.unwrap_or_default()
}

pub async fn live_state_checked(
    app: &AppHandle,
    host_id: &str,
    servers: &[(String, String)],
) -> Result<HashMap<String, Live>, String> {
    let conn = connect(app, host_id).await?;
    let script = "echo '@sessions'\n\
         tmux list-panes -a -F '#{session_name} #{pane_pid}' 2>/dev/null\n\
         screen -ls 2>/dev/null | sed -n 's/^[[:space:]]*\\([0-9]\\{1,\\}\\)\\.\\([^[:space:]]\\{1,\\}\\).*/\\2 \\1/p'\n\
         echo '@procs'\n\
         for p in /proc/[0-9]*; do\n\
           pid=${p#/proc/}\n\
           exe=$(readlink \"$p/exe\" 2>/dev/null) || continue\n\
           case \"$exe\" in */java) ;; *) continue;; esac\n\
           printf '%s %s\\n' \"$pid\" \"$(readlink \"$p/cwd\" 2>/dev/null)\"\n\
         done\n\
         echo '@tree'\n\
         for p in /proc/[0-9]*; do\n\
           pid=${p#/proc/}\n\
           printf '%s %s\\n' \"$pid\" \"$(awk '{print $4}' \"$p/stat\" 2>/dev/null)\"\n\
         done\n";
    let result = conn.run(script).await?;
    Ok(read_live_state(&result.stdout, servers))
}

fn read_live_state(text: &str, servers: &[(String, String)]) -> HashMap<String, Live> {
    let mut part = "";
    let mut panes: HashMap<u32, String> = HashMap::new();
    let mut javas: Vec<(u32, String)> = Vec::new();
    let mut parent: HashMap<u32, u32> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('@') {
            part = name;
            continue;
        }
        if line.is_empty() {
            continue;
        }
        let mut bits = line.split_whitespace();
        match part {
            "sessions" => {
                if let (Some(name), Some(pid)) = (bits.next(), bits.next()) {
                    if let Ok(pid) = pid.parse() {
                        panes.insert(pid, name.to_string());
                    }
                }
            }
            "procs" => {
                if let (Some(pid), Some(folder)) = (bits.next(), bits.next()) {
                    if let Ok(pid) = pid.parse() {
                        javas.push((pid, folder.trim_end_matches('/').to_string()));
                    }
                }
            }
            "tree" => {
                if let (Some(pid), Some(up)) = (bits.next(), bits.next()) {
                    if let (Ok(pid), Ok(up)) = (pid.parse(), up.parse()) {
                        parent.insert(pid, up);
                    }
                }
            }
            _ => {}
        }
    }

    let mut out = HashMap::new();
    for (id, folder) in servers {
        let want = folder.trim_end_matches('/');
        let Some((pid, _)) = javas.iter().find(|(_, at)| at == want) else {
            continue;
        };
        let mut at = *pid;
        let mut session = String::new();
        for _ in 0..30 {
            if let Some(name) = panes.get(&at) {
                session = name.clone();
                break;
            }
            match parent.get(&at) {
                Some(up) if *up > 1 => at = *up,
                _ => break,
            }
        }
        out.insert(
            id.clone(),
            Live {
                pid: *pid,
                ours: session == Holder::session(id),
                session,
            },
        );
    }
    out
}

pub async fn tail_log<F>(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    mut on_line: impl FnMut(String),
    keep: F,
) -> Result<(), String>
where
    F: Fn() -> bool,
{
    let conn = connect(app, host_id).await?;
    let mut channel = conn
        .handle
        .channel_open_session()
        .await
        .map_err(|error| format!("Could not open an SSH channel: {error}"))?;
    let command = format!("tail -n 200 -F {}/logs/latest.log 2>/dev/null", quote(folder));
    channel
        .exec(true, command.as_str())
        .await
        .map_err(|error| format!("Could not read the log: {error}"))?;

    let mut rest = Vec::new();
    while let Some(message) = channel.wait().await {
        if !keep() {
            break;
        }
        let ChannelMsg::Data { data } = message else {
            continue;
        };
        rest.extend_from_slice(&data);
        while let Some(at) = rest.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = rest.drain(..=at).collect();
            let text = String::from_utf8_lossy(&line).trim_end().to_string();
            if !text.is_empty() {
                on_line(text);
            }
        }
    }
    let _ = channel.close().await;
    Ok(())
}

fn inside(folder: &str, relative: &str) -> Result<String, String> {
    let relative = relative.trim().replace('\\', "/");
    let relative = relative.trim_start_matches('/');
    let base = folder.trim_end_matches('/');
    if relative.is_empty() {
        return Ok(if base.is_empty() { "/".to_string() } else { base.to_string() });
    }
    crate::paths::check_relative(relative)?;
    Ok(format!("{base}/{relative}"))
}

pub async fn list_dir(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    relative: &str,
) -> Result<Vec<FileRow>, String> {
    let at = inside(folder, relative)?;
    let conn = connect(app, host_id).await?;
    let sftp = conn.shared_sftp().await?;
    let listing = conn
        .healed(
            sftp.read_dir(&at)
                .await
                .map_err(|error| format!("Cannot open {at}: {error}")),
        )
        .await?;
    let mut rows: Vec<FileRow> = listing
        .into_iter()
        .map(|one| {
            let meta = one.metadata();
            FileRow {
                name: one.file_name(),
                directory: one.file_type().is_dir(),
                size_bytes: meta.size.unwrap_or(0),
                modified_at: meta.mtime.unwrap_or(0) as u64,
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(rows)
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FileRow {
    pub name: String,
    pub directory: bool,
    pub size_bytes: u64,
    pub modified_at: u64,
}

pub async fn read_text(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    relative: &str,
) -> Result<String, String> {
    const LIMIT: u64 = 8 * 1024 * 1024;
    let at = inside(folder, relative)?;
    let conn = connect(app, host_id).await?;
    let sftp = conn.shared_sftp().await?;
    let meta = conn
        .healed(
            sftp.metadata(&at)
                .await
                .map_err(|error| format!("Cannot open {at}: {error}")),
        )
        .await?;
    if meta.size.unwrap_or(0) > LIMIT {
        return Err("This file is too large to open in the app (over 8 MB).".into());
    }
    let bytes = conn
        .healed(
            sftp.read(&at)
                .await
                .map_err(|error| format!("Could not read {at}: {error}")),
        )
        .await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn read_turn(host_id: &str) -> tokio::sync::OwnedSemaphorePermit {
    static TURNS: OnceLock<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>> = OnceLock::new();
    let gate = TURNS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entry(host_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(8)))
        .clone();
    gate.acquire_owned().await.expect("semaphore is never closed")
}

pub fn is_missing(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("no such file") || lower.contains("not found") || lower.contains("404")
}

pub async fn read_bytes(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    relative: &str,
    limit: u64,
) -> Result<Vec<u8>, String> {
    let at = inside(folder, relative)?;
    let _turn = read_turn(host_id).await;
    let conn = connect(app, host_id).await?;
    let mut last = String::new();
    for _ in 0..2 {
        let sftp = conn.shared_sftp().await?;
        match read_with(&sftp, &at, limit).await {
            Ok(bytes) => return Ok(bytes),
            Err(error) if is_missing(&error) || error.contains("too large") => return Err(error),
            Err(error) => {
                conn.drop_shared_sftp().await;
                last = error;
            }
        }
    }
    Err(last)
}

async fn read_with(sftp: &SftpSession, at: &str, limit: u64) -> Result<Vec<u8>, String> {
    let meta = sftp
        .metadata(at)
        .await
        .map_err(|error| format!("Cannot open {at}: {error}"))?;
    if meta.size.unwrap_or(0) > limit {
        return Err(format!(
            "This image is too large to open in the app (over {} MB).",
            limit / (1024 * 1024)
        ));
    }
    sftp.read(at)
        .await
        .map_err(|error| format!("Could not read {at}: {error}"))
}

async fn put(sftp: &SftpSession, path: &str, bytes: &[u8]) -> Result<(), String> {
    put_watched(sftp, path, bytes, &|_, _| {}).await
}

pub async fn put_watched(
    sftp: &SftpSession,
    path: &str,
    bytes: &[u8],
    tick: &(dyn Fn(u64, u64) + Sync),
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let mut file = sftp
        .create(path)
        .await
        .map_err(|error| format!("Could not create {path}: {error}"))?;
    let total = bytes.len() as u64;
    let mut sent = 0u64;
    tick(0, total);
    for chunk in bytes.chunks(256 * 1024) {
        file.write_all(chunk)
            .await
            .map_err(|error| format!("Could not write to {path}: {error}"))?;
        sent += chunk.len() as u64;
        tick(sent, total);
    }
    file.shutdown()
        .await
        .map_err(|error| format!("Could not close {path}: {error}"))?;
    Ok(())
}

pub async fn write_text(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    relative: &str,
    text: &str,
) -> Result<(), String> {
    let at = inside(folder, relative)?;
    let conn = connect(app, host_id).await?;
    let sftp = conn.sftp().await?;
    let temp = match at.rsplit_once('/') {
        Some((folder, name)) => format!("{folder}/.{name}.uploading"),
        None => format!(".{at}.uploading"),
    };
    put(&sftp, &temp, text.as_bytes()).await?;
    conn.run(&format!("mv -f {} {}", quote(&temp), quote(&at)))
        .await?
        .ok()?;
    Ok(())
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|gap| gap.as_secs())
        .unwrap_or(0)
}

pub async fn move_path(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    from: &str,
    to: &str,
) -> Result<(), String> {
    let from = inside(folder, from)?;
    let to = inside(folder, to)?;
    let conn = connect(app, host_id).await?;
    conn.run(&format!("mv -n {} {}", quote(&from), quote(&to)))
        .await?
        .ok()?;
    Ok(())
}

pub async fn trash_path(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    relative: &str,
) -> Result<(), String> {
    let at = inside(folder, relative)?;
    let conn = connect(app, host_id).await?;
    let bin = format!("{}/.monolab-trash", folder.trim_end_matches('/'));
    let name = relative.rsplit('/').next().unwrap_or("File");
    let stored = format!("{}-{name}", unix_secs());
    conn.run(&format!(
        "mkdir -p {} && mv {} {}",
        quote(&bin),
        quote(&at),
        quote(&format!("{bin}/{stored}"))
    ))
    .await?
    .ok()?;
    let from = relative.trim_matches('/').to_string();
    edit_trash_notes(app, host_id, folder, |note| {
        note.insert(stored, from);
    })
    .await;
    Ok(())
}

pub const TRASH_NOTE: &str = "index.json";

async fn trash_notes(app: &AppHandle, host_id: &str, folder: &str) -> HashMap<String, String> {
    read_trash_notes(app, host_id, folder).await.unwrap_or_default()
}

async fn read_trash_notes(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
) -> Option<HashMap<String, String>> {
    let at = format!(".monolab-trash/{TRASH_NOTE}");
    match read_text(app, host_id, folder, &at).await {
        Ok(text) => serde_json::from_str(&text).ok(),
        Err(error) if is_missing(&error) => Some(HashMap::new()),
        Err(_) => None,
    }
}

async fn edit_trash_notes(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    change: impl FnOnce(&mut HashMap<String, String>),
) {
    if let Some(mut note) = read_trash_notes(app, host_id, folder).await {
        change(&mut note);
        write_trash_notes(app, host_id, folder, &note).await;
    }
}

async fn write_trash_notes(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    note: &HashMap<String, String>,
) {
    let Ok(text) = serde_json::to_string_pretty(note) else {
        return;
    };
    let at = format!(".monolab-trash/{TRASH_NOTE}");
    let _ = write_text(app, host_id, folder, &at, &text).await;
}

pub async fn trash_list(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
) -> Result<Vec<(FileRow, String)>, String> {
    let rows = match list_dir(app, host_id, folder, ".monolab-trash").await {
        Ok(rows) => rows,
        Err(_) => return Ok(Vec::new()),
    };
    let note = trash_notes(app, host_id, folder).await;
    Ok(rows
        .into_iter()
        .filter(|one| one.name != TRASH_NOTE)
        .map(|one| {
            let came = note
                .get(&one.name)
                .cloned()
                .unwrap_or_else(|| plain_trash_name(&one.name));
            (one, came)
        })
        .collect())
}

pub fn plain_trash_name(stored: &str) -> String {
    match stored.split_once('-') {
        Some((head, rest)) if !head.is_empty() && head.chars().all(|one| one.is_ascii_digit()) => {
            rest.to_string()
        }
        _ => stored.to_string(),
    }
}

pub async fn trash_restore(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    stored: &str,
) -> Result<String, String> {
    crate::paths::check_relative(stored)?;
    let note = read_trash_notes(app, host_id, folder)
        .await
        .ok_or("Could not read the trash list. Please try again shortly.")?;
    let back = note
        .get(stored)
        .cloned()
        .unwrap_or_else(|| plain_trash_name(stored));
    let to = inside(folder, &back)?;
    let from = inside(folder, &format!(".monolab-trash/{stored}"))?;
    let conn = connect(app, host_id).await?;
    let out = conn
        .run(&format!(
            "test -e {0} && exit 17; mkdir -p \"$(dirname {0})\" && mv {1} {0}",
            quote(&to),
            quote(&from)
        ))
        .await?;
    if out.code == 17 {
        return Err(format!("Something is already at {back}. Move it out of the way first."));
    }
    out.ok()?;
    edit_trash_notes(app, host_id, folder, |note| {
        note.remove(stored);
    })
    .await;
    Ok(back)
}

pub async fn trash_delete(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    stored: &str,
) -> Result<(), String> {
    crate::paths::check_relative(stored)?;
    let at = inside(folder, &format!(".monolab-trash/{stored}"))?;
    let conn = connect(app, host_id).await?;
    conn.run(&format!("rm -rf {}", quote(&at))).await?.ok()?;
    edit_trash_notes(app, host_id, folder, |note| {
        note.remove(stored);
    })
    .await;
    Ok(())
}

pub async fn trash_empty(app: &AppHandle, host_id: &str, folder: &str) -> Result<u32, String> {
    let count = trash_list(app, host_id, folder).await?.len() as u32;
    let bin = inside(folder, ".monolab-trash")?;
    let conn = connect(app, host_id).await?;
    conn.run(&format!("rm -rf {}/* {}/.[!.]*", quote(&bin), quote(&bin)))
        .await?;
    Ok(count)
}

pub async fn make_dir(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    relative: &str,
) -> Result<(), String> {
    let at = inside(folder, relative)?;
    let conn = connect(app, host_id).await?;
    let out = conn
        .run(&format!("test -e {0} && exit 17; mkdir -p {0}", quote(&at)))
        .await?;
    if out.code == 17 {
        return Err("That name already exists.".into());
    }
    out.ok()?;
    Ok(())
}

pub async fn log_files(app: &AppHandle, host_id: &str, folder: &str) -> Vec<FileRow> {
    let mut rows = list_dir(app, host_id, folder, "logs").await.unwrap_or_default();
    rows.retain(|one| {
        !one.directory && (one.name.ends_with(".log") || one.name.ends_with(".log.gz"))
    });
    rows.sort_by_key(|one| std::cmp::Reverse(one.modified_at));
    rows
}

pub async fn read_log(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    name: &str,
    lines: u32,
) -> Result<String, String> {
    crate::paths::check_relative(name)?;
    let at = format!("{}/logs/{name}", folder.trim_end_matches('/'));
    let conn = connect(app, host_id).await?;
    let reader = if name.ends_with(".gz") { "zcat" } else { "cat" };
    Ok(conn
        .run(&format!(
            "{reader} {} 2>/dev/null | tail -n {lines}",
            quote(&at)
        ))
        .await?
        .stdout)
}

pub async fn resource(app: &AppHandle, host_id: &str, pid: u32) -> Option<(f64, u64)> {
    let conn = connect(app, host_id).await.ok()?;
    let script = format!(
        "p=/proc/{pid}\n\
         [ -r $p/stat ] || exit 1\n\
         u1=$(awk '{{print $14+$15}}' $p/stat); t1=$(awk '/^cpu /{{print $2+$3+$4+$5+$6+$7+$8}}' /proc/stat)\n\
         sleep 0.4\n\
         u2=$(awk '{{print $14+$15}}' $p/stat); t2=$(awk '/^cpu /{{print $2+$3+$4+$5+$6+$7+$8}}' /proc/stat)\n\
         rss=$(awk '/^VmRSS/{{print $2}}' $p/status 2>/dev/null)\n\
         echo \"$u1 $u2 $t1 $t2 ${{rss:-0}} $(nproc)\""
    );
    let out = conn.run(&script).await.ok()?;
    let bits: Vec<u64> = out
        .stdout
        .split_whitespace()
        .filter_map(|one| one.parse().ok())
        .collect();
    if bits.len() < 6 {
        return None;
    }
    let used = bits[1].saturating_sub(bits[0]);
    let total = bits[3].saturating_sub(bits[2]);
    let cpu = if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64) * 100.0
    };
    Some((cpu, bits[4] / 1024))
}

pub fn backup_vault(folder: &str) -> String {
    let folder = folder.trim_end_matches('/');
    let own = folder.rsplit('/').next().unwrap_or(folder);
    format!("{folder}/../.monolab-backups/{own}")
}

/// Packs the server folder, or only the folders in `only` (top-level names) when it isn't empty.
pub async fn make_backup(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    label: &str,
    only: &[String],
) -> Result<String, String> {
    let folder = folder.trim_end_matches('/');
    let vault = backup_vault(folder);
    let name = format!("{}-{label}.tar.gz", unix_secs());
    let what = if only.is_empty() {
        ".".to_string()
    } else {
        only.iter().map(|one| quote(&format!("./{one}"))).collect::<Vec<_>>().join(" ")
    };
    let conn = connect(app, host_id).await?;
    conn.run_long(&format!(
        "mkdir -p {vault} && {{ tar -czf {vault}/{name}.part -C {here} --warning=no-file-changed --ignore-failed-read \
         --exclude=./.monolab-trash --exclude=session.lock {what} ; c=$?; [ $c -le 1 ] || {{ rm -f {vault}/{name}.part; exit $c; }}; }} \
         && mv {vault}/{name}.part {vault}/{name}",
        vault = quote(&vault),
        here = quote(folder),
        name = name
    ))
    .await?
    .ok()?;
    Ok(name)
}

pub async fn sweep_backups(app: &AppHandle, host_id: &str, folder: &str, days: u32) -> Result<(), String> {
    let vault = backup_vault(folder);
    let old = if days == 0 {
        String::new()
    } else {
        format!(" -o -name '*.tar.gz' -mtime +{days}")
    };
    let conn = connect(app, host_id).await?;
    conn.run(&format!(
        "test -d {vault} && find {vault} -maxdepth 1 -type f \\( -name '*.tar.gz.part' -mmin +1440{old} \\) -delete; true",
        vault = quote(&vault)
    ))
    .await?
    .ok()?;
    Ok(())
}

pub async fn restore_backup(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    file_name: &str,
) -> Result<String, String> {
    let name = file_name.trim();
    if name.is_empty() || name.contains(['/', '\\']) || !name.ends_with(".tar.gz") {
        return Err("The backup name is malformed.".into());
    }
    let folder = folder.trim_end_matches('/');
    let archive = format!("{}/{name}", backup_vault(folder));
    let conn = connect(app, host_id).await?;
    conn.run(&format!("test -f {}", quote(&archive)))
        .await?
        .ok()
        .map_err(|_| "That backup was not found.".to_string())?;
    let safety = make_backup(app, host_id, folder, "before-restore", &[]).await?;
    if crate::backups::is_worlds_only(name) {
        let listing = conn.run_long(&format!("tar -tzf {}", quote(&archive))).await?.ok()?;
        let mut tops: Vec<String> = listing
            .lines()
            .filter_map(|line| line.trim_start_matches("./").split_once('/').map(|(top, _)| top.to_string()))
            .filter(|top| !top.is_empty() && top != "." && top != ".." && !top.contains('\\'))
            .collect();
        tops.sort();
        tops.dedup();
        for top in tops {
            conn.run(&format!("rm -rf {}", quote(&format!("{folder}/{top}")))).await?.ok()?;
        }
    }
    conn.run_long(&format!(
        "tar -xzf {archive} -C {here}",
        archive = quote(&archive),
        here = quote(folder)
    ))
    .await?
    .ok()
    .map_err(|error| format!("Could not extract the backup: {error}"))?;
    Ok(safety)
}

fn backup_archive(folder: &str, file_name: &str) -> Result<String, String> {
    let name = file_name.trim();
    if name.is_empty() || name.contains(['/', '\\']) || !name.ends_with(".tar.gz") {
        return Err("The backup name is malformed.".into());
    }
    Ok(format!("{}/{name}", backup_vault(folder.trim_end_matches('/'))))
}

/// Regular files in a `tar -tzv` listing with their sizes, without the leading `./`.
pub fn tar_files(listing: &str) -> Vec<(String, u64)> {
    listing
        .lines()
        .filter(|line| line.starts_with('-'))
        .filter_map(|line| {
            let mut rest = line;
            let mut fields = Vec::new();
            for _ in 0..5 {
                rest = rest.trim_start();
                let end = rest.find(char::is_whitespace)?;
                fields.push(&rest[..end]);
                rest = &rest[end..];
            }
            let name = rest.trim_start().trim_start_matches("./");
            (!name.is_empty()).then(|| Some((name.to_string(), fields[2].parse().ok()?)))?
        })
        .collect()
}

/// A backup's files and the server's files now, both with sizes.
pub async fn backup_listing(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    file_name: &str,
) -> Result<(Vec<(String, u64)>, std::collections::HashMap<String, u64>), String> {
    let archive = quote(&backup_archive(folder, file_name)?);
    let conn = connect(app, host_id).await?;
    let listing = conn
        .run_long(&format!("tar --quoting-style=literal -tzvf {archive} 2>/dev/null || tar -tzvf {archive}"))
        .await?
        .ok()
        .map_err(|error| format!("Could not read the backup: {error}"))?;
    let now = conn
        .run_long(&format!(
            "cd {} && find . -path ./.monolab-trash -prune -o -type f -printf '%s %P\\n'",
            quote(folder.trim_end_matches('/'))
        ))
        .await?
        .ok()?;
    let now = now
        .lines()
        .filter_map(|line| {
            let (size, name) = line.split_once(' ')?;
            Some((name.to_string(), size.parse().ok()?))
        })
        .collect();
    Ok((tar_files(&listing), now))
}

pub async fn backup_entry_text(app: &AppHandle, host_id: &str, folder: &str, file_name: &str, inside: &str) -> Result<Option<String>, String> {
    let archive = backup_archive(folder, file_name)?;
    let conn = connect(app, host_id).await?;
    let out = conn
        .run(&format!("tar -xzOf {} {}", quote(&archive), quote(&format!("./{inside}"))))
        .await?;
    if out.code != 0 {
        return Err("That file isn't in the backup.".into());
    }
    let text = out.stdout;
    let binary = text.len() as u64 > crate::backups::TEXT_LIMIT || text.contains('\0') || text.contains('\u{FFFD}');
    Ok((!binary).then_some(text))
}

/// Puts back only the picked paths from a backup; the current versions go into a backup first.
pub async fn restore_backup_paths(app: &AppHandle, host_id: &str, folder: &str, file_name: &str, picks: &[String]) -> Result<String, String> {
    let archive = backup_archive(folder, file_name)?;
    let folder = folder.trim_end_matches('/');
    let vault = backup_vault(folder);
    let conn = connect(app, host_id).await?;
    conn.run(&format!("test -f {}", quote(&archive)))
        .await?
        .ok()
        .map_err(|_| "That backup was not found.".to_string())?;
    let name = format!("{}-before-partial-restore.tar.gz", unix_secs());
    let list = picks.iter().map(|one| quote(&format!("./{one}"))).collect::<Vec<_>>().join(" ");
    conn.run_long(&format!(
        "cd {here} && mkdir -p {vault} && set -- && for p in {list}; do [ -e \"$p\" ] && set -- \"$@\" \"$p\"; done; \
         if [ $# -gt 0 ]; then tar -czf {vault}/{name}.part -- \"$@\" && mv {vault}/{name}.part {vault}/{name}; fi",
        here = quote(folder),
        vault = quote(&vault),
    ))
    .await?
    .ok()
    .map_err(|error| format!("Could not back up the current files: {error}"))?;
    conn.run_long(&format!("tar -xzf {} -C {} -- {list}", quote(&archive), quote(folder)))
        .await?
        .ok()
        .map_err(|error| format!("Could not extract the backup: {error}"))?;
    Ok(name)
}

pub async fn download_to(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    relative: &str,
    to: &Path,
    tick: impl Fn(u64, u64),
) -> Result<u64, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let at = inside(folder, relative)?;
    let conn = connect(app, host_id).await?;
    let sftp = conn.sftp().await?;
    let whole = sftp.metadata(&at).await.ok().and_then(|one| one.size).unwrap_or(0);
    let mut from = sftp
        .open(&at)
        .await
        .map_err(|error| format!("Cannot open {at}: {error}"))?;
    let temp = to.with_extension("monolab-part");
    let mut out = tokio::fs::File::create(&temp)
        .await
        .map_err(|error| format!("Could not create the save location: {error}"))?;
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let read = from
            .read(&mut buffer)
            .await
            .map_err(|error| format!("Could not read {at}: {error}"))?;
        if read == 0 {
            break;
        }
        out.write_all(&buffer[..read])
            .await
            .map_err(|error| format!("Could not save the file: {error}"))?;
        total += read as u64;
        tick(total, whole);
    }
    out.flush()
        .await
        .map_err(|error| format!("Could not save the file: {error}"))?;
    drop(out);
    let _ = tokio::fs::remove_file(to).await;
    tokio::fs::rename(&temp, to)
        .await
        .map_err(|error| format!("Could not save the file: {error}"))?;
    Ok(total)
}

pub async fn pack_for_download(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    relative: &str,
) -> Result<String, String> {
    let at = inside(folder, relative)?;
    let name = relative.trim_end_matches('/').rsplit('/').next().unwrap_or("Folder");
    let kept = format!(".monolab-trash/{}-{name}.tar.gz", unix_secs());
    let to = inside(folder, &kept)?;
    let parent = at.rsplit_once('/').map(|(head, _)| head).unwrap_or(".");
    let conn = connect(app, host_id).await?;
    let bin = format!("{}/.monolab-trash", folder.trim_end_matches('/'));
    conn.run_long(&format!(
        "mkdir -p {bin} && tar -czf {to} -C {parent} {leaf}",
        bin = quote(&bin),
        to = quote(&to),
        parent = quote(parent),
        leaf = quote(name)
    ))
    .await?
    .ok()?;
    Ok(kept)
}

pub async fn is_dir_checked(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    relative: &str,
) -> Result<bool, String> {
    let at = inside(folder, relative)?;
    let conn = connect(app, host_id).await?;
    let sftp = conn.shared_sftp().await?;
    match conn.healed(sftp.metadata(&at).await.map_err(|error| error.to_string())).await {
        Ok(one) => Ok(one.is_dir()),
        Err(error) if is_missing(&error) => Ok(false),
        Err(error) => Err(format!("Could not check {at}: {error}")),
    }
}

pub async fn server_root(app: &AppHandle, host_id: &str) -> Result<String, String> {
    let mut one = host(app, host_id)?;
    let root = one.root.trim().trim_end_matches('/').to_string();
    if !root.is_empty() {
        return Ok(root);
    }
    let conn = connect(app, host_id).await?;
    let made = conn
        .run("mkdir -p \"$HOME/monolab\" && printf %s \"$HOME/monolab\"")
        .await?;
    let made = made.stdout.trim().to_string();
    if made.is_empty() || !made.starts_with('/') {
        return Err("Could not create the monolab folder on the SSH server. Pick the server folder yourself.".into());
    }
    one.root = made.clone();
    save_host(app, one, None)?;
    Ok(made)
}

fn plain_folder(name: &str) -> Result<String, String> {
    let name = name.trim().trim_matches('/');
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.starts_with('.')
        || name == ".."
    {
        return Err("The folder name is invalid.".into());
    }
    Ok(name.to_string())
}

pub async fn unpack_zip(
    app: &AppHandle,
    host_id: &str,
    parent: &str,
    name: &str,
    bytes: &[u8],
    tick: &(dyn Fn(u64, u64) + Sync),
) -> Result<String, String> {
    let name = plain_folder(name)?;
    let parent = parent.trim().trim_end_matches('/').to_string();
    if parent.is_empty() {
        return Err("Please choose a folder to extract into.".into());
    }
    let dest = format!("{parent}/{name}");
    let conn = connect(app, host_id).await?;
    let taken = conn
        .run(&format!("[ -e {} ] && echo YES", quote(&dest)))
        .await?
        .stdout;
    if taken.contains("YES") {
        return Err(format!("Something already exists at {dest}. Choose another name."));
    }
    let temp = format!("{parent}/.monolab-upload-{}.zip", unix_secs());
    conn.run(&format!("mkdir -p {}", quote(&parent))).await?.ok()?;
    let sftp = conn.sftp().await?;
    put_watched(&sftp, &temp, bytes, tick).await?;
    let script = format!(
        "set -e\n\
         mkdir -p {dest}\n\
         if command -v unzip >/dev/null 2>&1; then unzip -q -o {temp} -d {dest}\n\
         elif command -v python3 >/dev/null 2>&1; then python3 -m zipfile -e {temp} {dest}\n\
         elif command -v bsdtar >/dev/null 2>&1; then bsdtar -xf {temp} -C {dest}\n\
         else echo NOUNZIP; exit 3; fi\n\
         cd {dest}\n\
         # Lift the contents out of a zip wrapped in one folder.\n\
         count=$(ls -A | wc -l)\n\
         only=$(ls -A)\n\
         if [ \"$count\" = 1 ] && [ -d \"$only\" ]; then\n\
           mv \"$only\"/* . 2>/dev/null || true\n\
           mv \"$only\"/.[!.]* . 2>/dev/null || true\n\
           rmdir \"$only\" 2>/dev/null || true\n\
         fi\n",
        dest = quote(&dest),
        temp = quote(&temp)
    );
    let unpacked = conn.run_long(&script).await?;
    let _ = conn.run(&format!("rm -f {}", quote(&temp))).await;
    if unpacked.stdout.contains("NOUNZIP") {
        let _ = conn.run(&format!("rmdir {} 2>/dev/null", quote(&dest))).await;
        return Err(
            "The server machine has no unzip tool. Install one with `sudo apt install unzip`.".into(),
        );
    }
    if let Err(error) = unpacked.ok() {
        let _ = conn.run(&format!("rm -rf {}", quote(&dest))).await;
        return Err(error);
    }
    Ok(dest)
}

pub async fn drop_path(app: &AppHandle, host_id: &str, folder: &str, relative: &str) {
    let Ok(at) = inside(folder, relative) else {
        return;
    };
    let Ok(conn) = connect(app, host_id).await else {
        return;
    };
    let _ = conn.run(&format!("rm -f {}", quote(&at))).await;
}

pub async fn swap_core(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    jar: &str,
    bytes: &[u8],
    stamp: u64,
) -> Result<String, String> {
    let jar = jar.trim();
    if jar.is_empty() || jar.contains(['/', '\\']) {
        return Err("The server jar to replace was not found.".into());
    }
    let folder = folder.trim_end_matches('/');
    let vault = backup_vault(folder);
    let current = format!("{folder}/{jar}");
    let previous = format!("{vault}/previous-{stamp}-{jar}");
    let conn = connect(app, host_id).await?;
    conn.run(&format!(
        "test -f {current} && mkdir -p {vault} && mv -f {current} {previous}",
        current = quote(&current),
        vault = quote(&vault),
        previous = quote(&previous)
    ))
    .await?
    .ok()
    .map_err(|_| "The server jar to replace was not found.".to_string())?;
    if let Err(error) = upload(app, host_id, folder, jar, bytes).await {
        let _ = conn
            .run(&format!("mv -f {} {}", quote(&previous), quote(&current)))
            .await;
        return Err(format!(
            "Could not swap in the new jar, so the old one was restored: {error}"
        ));
    }
    Ok(previous)
}

pub async fn restore_core(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    jar: &str,
    previous: &str,
    stamp: u64,
) -> Result<(), String> {
    let jar = jar.trim();
    if jar.is_empty() || jar.contains(['/', '\\']) {
        return Err("The server jar file name is invalid.".into());
    }
    let folder = folder.trim_end_matches('/');
    let vault = backup_vault(folder);
    let tail = previous
        .strip_prefix(&format!("{vault}/"))
        .or_else(|| previous.strip_prefix(&format!("{folder}/../.monolab-backups/")))
        .filter(|rest| !rest.is_empty() && !rest.contains('/'))
        .ok_or("The previous jar path is malformed.")?;
    let current = format!("{folder}/{jar}");
    let kept = format!("{vault}/rollback-{stamp}-{jar}");
    let conn = connect(app, host_id).await?;
    conn.run(&format!("test -f {}", quote(previous)))
        .await?
        .ok()
        .map_err(|_| format!("The previous jar file ({tail}) wasn't found."))?;
    conn.run(&format!(
        "if [ -f {current} ]; then mv -f {current} {kept}; fi && mv -f {previous} {current}",
        current = quote(&current),
        kept = quote(&kept),
        previous = quote(previous)
    ))
    .await?
    .ok()
    .map_err(|error| format!("Could not restore the previous jar: {error}"))?;
    Ok(())
}

pub async fn backups(app: &AppHandle, host_id: &str, folder: &str) -> Vec<FileRow> {
    let vault = backup_vault(folder);
    let Ok(conn) = connect(app, host_id).await else {
        return Vec::new();
    };
    let Ok(sftp) = conn.shared_sftp().await else {
        return Vec::new();
    };
    let seen = sftp.read_dir(&vault).await.map_err(|error| error.to_string());
    let Ok(listing) = conn.healed(seen).await else {
        return Vec::new();
    };
    let mut rows: Vec<FileRow> = listing
        .into_iter()
        .filter(|one| one.file_name().ends_with(".tar.gz"))
        .map(|one| {
            let meta = one.metadata();
            FileRow {
                name: one.file_name(),
                directory: false,
                size_bytes: meta.size.unwrap_or(0),
                modified_at: meta.mtime.unwrap_or(0) as u64,
            }
        })
        .collect();
    rows.sort_by_key(|one| std::cmp::Reverse(one.modified_at));
    rows
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AddonFile {
    pub name: String,
    pub hash: String,
    pub size: u64,
}

pub async fn addon_files(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    sub: &str,
) -> Result<Vec<AddonFile>, String> {
    let at = inside(folder, sub)?;
    let conn = connect(app, host_id).await?;
    let out = conn
        .run(&format!(
            "cd {} 2>/dev/null || exit 0\n\
             for f in *.jar *.jar.disabled; do\n\
               [ -e \"$f\" ] || continue\n\
               printf '%s\\t%s\\t%s\\n' \"$(sha512sum \"$f\" | cut -d' ' -f1)\" \"$(stat -c %s \"$f\")\" \"$f\"\n\
             done",
            quote(&at)
        ))
        .await?
        .stdout;
    Ok(out
        .lines()
        .filter_map(|line| {
            let mut bits = line.splitn(3, '\t');
            let hash = bits.next()?.trim().to_string();
            let size = bits.next()?.trim().parse().ok()?;
            let name = bits.next()?.trim().to_string();
            (!name.is_empty()).then_some(AddonFile { name, hash, size })
        })
        .collect())
}

pub struct RemoteWorld {
    pub name: String,
    pub kib: u64,
    pub at: u64,
    pub level: bool,
    pub realm: String,
    pub world: String,
    pub inside: bool,
}

pub async fn worlds(app: &AppHandle, host_id: &str, folder: &str) -> Vec<RemoteWorld> {
    let Ok(conn) = connect(app, host_id).await else {
        return Vec::new();
    };
    let out = conn
        .run(&format!(
            "cd {} 2>/dev/null || exit 0\n\
             row() {{ printf '%s\\t%s\\t%s\\t%s\\t%s\\t%s\\t%s\\n' \"$1\" \"$(du -sk \"$2\" | cut -f1)\" \"$(stat -c %Y \"$2\")\" \"$3\" \"$4\" \"$5\" \"$6\"; }}\n\
             for d in */; do\n\
               d=${{d%/}}\n\
               [ -d \"$d\" ] || continue\n\
               case \"$d\" in .*|logs|plugins|mods|config|libraries|cache|versions|crash-reports) continue;; esac\n\
               lvl=0; [ -f \"$d/level.dat\" ] && lvl=1\n\
               [ $lvl = 1 ] || [ -d \"$d/region\" ] || continue\n\
               realm=overworld; owner=$d\n\
               case \"$d\" in *_nether) realm=nether; owner=${{d%_nether}};; *_the_end) realm=end; owner=${{d%_the_end}};; esac\n\
               row \"$d\" \"$d\" $lvl $realm \"$owner\" 0\n\
               [ $lvl = 1 ] || continue\n\
               if [ -d \"$d/dimensions/minecraft/the_nether/region\" ]; then row \"$d (nether)\" \"$d/dimensions/minecraft/the_nether\" 0 nether \"$d\" 1\n\
               elif [ -d \"$d/DIM-1/region\" ]; then row \"$d (nether)\" \"$d/DIM-1\" 0 nether \"$d\" 1; fi\n\
               if [ -d \"$d/dimensions/minecraft/the_end/region\" ]; then row \"$d (end)\" \"$d/dimensions/minecraft/the_end\" 0 end \"$d\" 1\n\
               elif [ -d \"$d/DIM1/region\" ]; then row \"$d (end)\" \"$d/DIM1\" 0 end \"$d\" 1; fi\n\
               [ -d \"$d/dimensions\" ] || continue\n\
               find \"$d/dimensions\" -mindepth 3 -maxdepth 6 -type d -name region 2>/dev/null | while IFS= read -r r; do\n\
                 dim=${{r%/region}}; rel=${{dim#\"$d/dimensions/\"}}\n\
                 case \"$rel\" in minecraft/overworld|minecraft/the_nether|minecraft/the_end) continue;; esac\n\
                 key=$(printf '%s' \"$rel\" | tr 'A-Z/' 'a-z.' | tr -c 'a-z0-9_.\\n-' '_')\n\
                 row \"$d ($key)\" \"$dim\" 0 \"$key\" \"$d\" 1\n\
               done\n\
             done",
            quote(folder.trim_end_matches('/'))
        ))
        .await
        .map(|one| one.stdout)
        .unwrap_or_default();
    out.lines()
        .filter_map(|line| {
            let mut bits = line.splitn(7, '\t');
            let name = bits.next()?.trim().to_string();
            let kib: u64 = bits.next()?.trim().parse().ok()?;
            let at: u64 = bits.next()?.trim().parse().ok()?;
            let level = bits.next()?.trim() == "1";
            let realm = crate::worldmap::realm_key(bits.next()?.trim());
            let world = bits.next()?.trim().to_string();
            let inside = bits.next()?.trim() == "1";
            Some(RemoteWorld { name, kib, at, level, realm, world, inside })
        })
        .collect()
}

pub async fn upload(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    relative: &str,
    bytes: &[u8],
) -> Result<(), String> {
    upload_watched(app, host_id, folder, relative, bytes, &|_, _| {}).await
}

pub async fn upload_watched(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    relative: &str,
    bytes: &[u8],
    tick: &(dyn Fn(u64, u64) + Sync),
) -> Result<(), String> {
    let at = inside(folder, relative)?;
    let conn = connect(app, host_id).await?;
    if let Some((parent, _)) = at.rsplit_once('/') {
        conn.run(&format!("mkdir -p {}", quote(parent))).await?.ok()?;
    }
    let sftp = conn.sftp().await?;
    let temp = match at.rsplit_once('/') {
        Some((folder, name)) => format!("{folder}/.{name}.uploading"),
        None => format!(".{at}.uploading"),
    };
    put_watched(&sftp, &temp, bytes, tick).await?;
    conn.run(&format!("mv -f {} {}", quote(&temp), quote(&at)))
        .await?
        .ok()?;
    Ok(())
}

fn walk_tree(
    base: &std::path::Path,
    here: &std::path::Path,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(here).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir() {
            walk_tree(base, &path, out)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(base)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, path));
        }
    }
    Ok(())
}

pub async fn upload_tree(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    local: &std::path::Path,
) -> Result<usize, String> {
    let mut files = Vec::new();
    walk_tree(local, local, &mut files)?;
    let mut targets = Vec::with_capacity(files.len());
    for (relative, path) in files {
        targets.push((inside(folder, &relative)?, path));
    }
    upload_targets(app, host_id, targets, &|_, _, _| {}).await
}

pub async fn upload_folder(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    into: &str,
    local: &std::path::Path,
    tick: &(dyn Fn(usize, usize, &str) + Sync),
) -> Result<usize, String> {
    let mut files = Vec::new();
    walk_tree(local, local, &mut files)?;
    let into = into.trim_matches('/');
    let mut targets = Vec::with_capacity(files.len());
    for (relative, path) in files {
        let at = if into.is_empty() { relative } else { format!("{into}/{relative}") };
        targets.push((inside(folder, &at)?, path));
    }
    upload_targets(app, host_id, targets, tick).await
}

async fn upload_targets(
    app: &AppHandle,
    host_id: &str,
    targets: Vec<(String, PathBuf)>,
    tick: &(dyn Fn(usize, usize, &str) + Sync),
) -> Result<usize, String> {
    let mut parents = std::collections::BTreeSet::new();
    for (at, _) in &targets {
        if let Some((parent, _)) = at.rsplit_once('/') {
            parents.insert(parent.to_string());
        }
    }
    let conn = connect(app, host_id).await?;
    let parents: Vec<String> = parents.into_iter().collect();
    for chunk in parents.chunks(100) {
        let list: Vec<String> = chunk.iter().map(|one| quote(one)).collect();
        conn.run(&format!("mkdir -p {}", list.join(" "))).await?.ok()?;
    }
    let sftp = conn.sftp().await?;
    let whole = targets.len();
    for (at, path) in targets.iter().enumerate() {
        let (to, from) = path;
        let name = from
            .file_name()
            .map(|one| one.to_string_lossy().to_string())
            .unwrap_or_default();
        let bytes = std::fs::read(from).map_err(|error| error.to_string())?;
        put(&sftp, to, &bytes).await?;
        tick(at + 1, whole, &name);
    }
    Ok(whole)
}

pub async fn disk_free_mib(app: &AppHandle, host_id: &str, folder: &str) -> (u64, u64) {
    let Ok(conn) = connect(app, host_id).await else {
        return (0, 0);
    };
    let out = conn
        .run(&format!(
            "df -Pk {} 2>/dev/null | awk 'NR==2{{print $4, $2}}'",
            quote(folder)
        ))
        .await
        .map(|one| one.stdout)
        .unwrap_or_default();
    let bits: Vec<u64> = out
        .split_whitespace()
        .filter_map(|one| one.parse().ok())
        .collect();
    if bits.len() < 2 {
        return (0, 0);
    }
    (bits[0] / 1024, bits[1] / 1024)
}

pub async fn zip_paths(
    app: &AppHandle,
    host_id: &str,
    folder: &str,
    paths: &[String],
    name: &str,
) -> Result<(), String> {
    if paths.is_empty() {
        return Err("Select something to zip.".into());
    }
    let conn = connect(app, host_id).await?;
    let here = folder.trim_end_matches('/');
    let mut bits = Vec::new();
    for one in paths {
        inside(folder, one)?;
        bits.push(quote(one.trim_start_matches('/')));
    }
    let target = inside(folder, name)?;
    conn.run_long(&format!(
        "cd {here} && if command -v zip >/dev/null; then zip -qr {out} {list}; \
         else tar -czf {out} {list}; fi",
        here = quote(here),
        out = quote(&target),
        list = bits.join(" ")
    ))
    .await?
    .ok()?;
    Ok(())
}

#[allow(dead_code)]
/// Copies a server folder next to itself (`name-copy`, `name-copy-2` …) and returns where it went.
pub async fn copy_folder(app: &AppHandle, host_id: &str, folder: &str) -> Result<String, String> {
    let folder = folder.trim_end_matches('/');
    let base = format!("{folder}-copy");
    let conn = connect(app, host_id).await?;
    let made = conn
        .run_long(&format!(
            "dst={base}; n=2; while [ -e \"$dst\" ]; do dst={base}-$n; n=$((n+1)); done;              cp -a {from} \"$dst\" && rm -rf \"$dst/.monolab-trash\" \"$dst/session.lock\" && printf %s \"$dst\"",
            base = quote(&base),
            from = quote(folder),
        ))
        .await?
        .ok()?;
    let made = made.trim().to_string();
    if made.is_empty() {
        return Err("Could not create a folder name for the copy.".into());
    }
    Ok(made)
}

pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_server_keeps_its_own_backups() {
        assert_eq!(backup_vault("/srv/mc/aero/"), "/srv/mc/aero/../.monolab-backups/aero");
        assert_ne!(backup_vault("/srv/mc/aero"), backup_vault("/srv/mc/lobby"));
    }

    #[test]
    fn reads_files_and_sizes_from_a_tar_listing() {
        let listing = [
            "drwxr-xr-x mc/mc         0 2026-10-28 12:00 ./",
            "-rw-r--r-- mc/mc        42 2026-10-28 12:00 ./server.properties",
            "-rw-r--r-- mc/mc   1048576 2026-10-28 12:01 ./world/region/r.0.0.mca",
            "-rw-r--r-- mc/mc         7 2026-10-28 12:01 ./notes/my  file.txt",
            "lrwxrwxrwx mc/mc         0 2026-10-28 12:01 ./link -> server.properties",
        ]
        .join("\n");
        assert_eq!(
            tar_files(&listing),
            vec![
                ("server.properties".to_string(), 42),
                ("world/region/r.0.0.mca".to_string(), 1_048_576),
                ("notes/my  file.txt".to_string(), 7),
            ]
        );
    }

    #[test]
    #[ignore]
    fn runs_on_the_real_host() {
        let data = std::path::PathBuf::from(std::env::var("APPDATA").unwrap())
            .join("com.monolab.desktop");
        let hosts: Vec<SshHost> =
            serde_json::from_str(&std::fs::read_to_string(data.join("ssh-hosts.json")).unwrap())
                .unwrap();
        let secrets: HashMap<String, String> = serde_json::from_str(
            &std::fs::read_to_string(data.join("ssh-secrets.json")).unwrap_or_default(),
        )
        .unwrap_or_default();
        let one = hosts.first().expect("no SSH server registered");
        let command = std::env::var("SE_SSH_RUN").unwrap_or_else(|_| "uname -a".into());
        let out = tauri::async_runtime::block_on(async {
            let conn = dial_with(one, secrets.get(&one.id).cloned()).await?;
            conn.run(&command).await
        })
        .unwrap();
        println!("code {}\n{}\n{}", out.code, out.stdout, out.stderr);
    }

    #[test]
    #[ignore]
    fn opens_many_sftp_at_once() {
        let data = std::path::PathBuf::from(std::env::var("APPDATA").unwrap())
            .join("com.monolab.desktop");
        let hosts: Vec<SshHost> =
            serde_json::from_str(&std::fs::read_to_string(data.join("ssh-hosts.json")).unwrap())
                .unwrap();
        let secrets: HashMap<String, String> = serde_json::from_str(
            &std::fs::read_to_string(data.join("ssh-secrets.json")).unwrap_or_default(),
        )
        .unwrap_or_default();
        let one = hosts.first().expect("no SSH server registered");
        tauri::async_runtime::block_on(async {
            let conn = dial_with(one, secrets.get(&one.id).cloned()).await.unwrap();
            if std::env::var("SE_SEQ").is_ok() {
                for i in 0..12 {
                    let got = conn.sftp().await;
                    println!("seq {i}: {}", got.as_ref().map(|_| "ok".to_string()).unwrap_or_else(|e| e.clone()));
                    drop(got);
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                return;
            }
            let jobs = (0..12).map(|_| {
                let conn = conn.clone();
                async move {
                    let sftp = conn.sftp().await?;
                    let found = sftp.read_dir(".").await.map_err(|e| e.to_string())?;
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    Ok::<usize, String>(found.into_iter().count())
                }
            });
            let results = futures_util::future::join_all(jobs).await;
            let failed: Vec<_> = results.iter().filter(|one| one.is_err()).collect();
            println!("ok {} · failed {:?}", results.len() - failed.len(), failed);
            assert!(failed.is_empty());
        });
    }

    #[test]
    #[ignore]
    fn pulls_real_map_tiles() {
        let data = std::path::PathBuf::from(std::env::var("APPDATA").unwrap())
            .join("com.monolab.desktop");
        let hosts: Vec<SshHost> =
            serde_json::from_str(&std::fs::read_to_string(data.join("ssh-hosts.json")).unwrap())
                .unwrap();
        let secrets: HashMap<String, String> = serde_json::from_str(
            &std::fs::read_to_string(data.join("ssh-secrets.json")).unwrap_or_default(),
        )
        .unwrap_or_default();
        let one = hosts.first().expect("no SSH server registered");
        let src = std::env::var("SE_MAP_SRC").unwrap();
        let out = std::path::PathBuf::from(std::env::var("SE_MAP_DIR").unwrap());
        let realms = std::env::var("SE_MAP_REALMS").unwrap();
        tauri::async_runtime::block_on(async {
            let conn = dial_with(one, secrets.get(&one.id).cloned()).await.unwrap();
            let sftp = conn.sftp().await.unwrap();
            std::fs::create_dir_all(&out).unwrap();
            for name in ["palette.json", "grass.png", "foliage.png"] {
                if let Ok(bytes) = sftp.read(format!("{src}/{name}")).await {
                    std::fs::write(out.join(name), bytes).unwrap();
                }
            }
            for realm in realms.split(',') {
                std::fs::create_dir_all(out.join(realm)).unwrap();
                let mut count = 0;
                for item in sftp.read_dir(format!("{src}/{realm}")).await.unwrap() {
                    let bytes = sftp.read(format!("{src}/{realm}/{}", item.file_name())).await.unwrap();
                    std::fs::write(out.join(realm).join(item.file_name()), bytes).unwrap();
                    count += 1;
                }
                println!("{realm}: {count}");
            }
        });
    }

    #[test]
    fn finds_servers_running_outside_the_app() {
        let text = "@sessions\n\
                    mc 100\n\
                    se-b 400\n\
                    @procs\n\
                    300 /home/u/server/paperserver\n\
                    500 /home/u/server/fabric\n\
                    700 /home/u/server/vanilla\n\
                    @tree\n\
                    300 200\n200 100\n100 1\n500 400\n400 1\n700 1\n";
        let servers = [
            ("a".to_string(), "/home/u/server/paperserver".to_string()),
            ("b".to_string(), "/home/u/server/fabric".to_string()),
            ("c".to_string(), "/home/u/server/vanilla".to_string()),
            ("d".to_string(), "/home/u/server/none".to_string()),
        ];
        let seen = read_live_state(text, &servers);

        assert_eq!(seen["a"].pid, 300);
        assert_eq!(seen["a"].session, "mc");
        assert!(!seen["a"].ours);

        assert_eq!(seen["b"].session, "se-b");
        assert!(seen["b"].ours);

        assert_eq!(seen["c"].pid, 700);
        assert_eq!(seen["c"].session, "");

        assert!(!seen.contains_key("d"));
    }

    #[test]
    fn uses_the_java_you_picked() {
        let line = launch_line("", "/usr/lib/jvm/java-21 openjdk/bin/java", &jar("a.jar"), 1, 2, "").unwrap();
        assert!(line.starts_with("'/usr/lib/jvm/java-21 openjdk/bin/java' "));
        assert!(launch_line("", "/bin/sh", &jar("a.jar"), 1, 2, "").is_err());
        assert!(launch_line("", "bin/java", &jar("a.jar"), 1, 2, "").is_err());
        assert!(launch_line("", "/opt/../bin/java", &jar("a.jar"), 1, 2, "").is_err());
    }

    #[test]
    fn reads_the_javas_on_a_box() {
        let found = read_javas(
            "J\t/usr/bin/java\tpath\topenjdk version \"25.0.4.1\" 2026-07-15\n             J\t/usr/lib/jvm/java-21-openjdk-amd64/bin/java\tfolder\topenjdk version \"21.0.8\" 2025-07-15\n             J\t/opt/broken/bin/java\tfolder\t/opt/broken/bin/java: cannot execute\n             stray line\n",
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].major, 25);
        assert_eq!(found[0].source, "PATH default");
        assert_eq!(found[1].major, 21);
        assert_eq!(found[1].source, "java-21-openjdk-amd64");
    }

    fn jar(name: &str) -> Launch {
        Launch { jar: name.into(), ..Launch::default() }
    }

    #[test]
    fn launches_an_installed_server_by_its_args_file() {
        let text = "neoforge-21.1.251-installer.jar\n@args\n\
                    @libraries/net/neoforged/neoforge/21.1.251/unix_args.txt\n\
                    libraries/net/neoforged/neoforge/21.1.77/unix_args.txt\n\
                    libraries/net/neoforged/neoforge/21.1.251/unix_args.txt\n@user\nyes\n";
        let launch = read_launch(text, "", "Custom");
        assert_eq!(launch.jar, "");
        assert_eq!(launch.args, "libraries/net/neoforged/neoforge/21.1.251/unix_args.txt");
        assert!(launch.user_args);
        let line = launch_line("", "java", &launch, 1024, 4096, "").unwrap();
        assert!(
            line.ends_with("@user_jvm_args.txt -Xms1024M -Xmx4096M '@libraries/net/neoforged/neoforge/21.1.251/unix_args.txt' nogui"),
            "{line}"
        );
        assert!(!line.contains("-jar"), "{line}");

        let bare = read_launch(
            "@args\nlibraries/net/neoforged/neoforge/21.1.77/unix_args.txt\n\
             libraries/net/neoforged/neoforge/21.1.251/unix_args.txt\n@user\n",
            "",
            "NeoForge",
        );
        assert!(bare.args.contains("21.1.251"));
        assert!(!launch_line("", "java", &bare, 1, 2, "").unwrap().contains("user_jvm_args"));

        let paper = read_launch("paper.jar\n@args\n@user\n", "paper.jar", "Paper");
        assert_eq!(paper, jar("paper.jar"));
    }

    #[test]
    fn launches_a_server_copied_from_windows() {
        let text = "@args\n@libraries/net/neoforged/neoforge/21.1.251/win_args.txt\n\
                    libraries/net/neoforged/neoforge/21.1.251/win_args.txt\n@user\nyes\n";
        let launch = read_launch(text, "", "Custom");
        assert_eq!(launch.args, "libraries/net/neoforged/neoforge/21.1.251/unix_args.txt");
        assert_eq!(launch.from_win, "libraries/net/neoforged/neoforge/21.1.251/win_args.txt");

        let both = read_launch(
            "@args\nlibraries/net/neoforged/forge/1.20.1-47.1.106/win_args.txt\n\
             libraries/net/neoforged/forge/1.20.1-47.1.106/unix_args.txt\n@user\n",
            "",
            "NeoForge",
        );
        assert_eq!(both.args, "libraries/net/neoforged/forge/1.20.1-47.1.106/unix_args.txt");
        assert_eq!(both.from_win, "");

        let none = read_launch("@args\n@user\n", "", "NeoForge");
        assert!(launch_line("", "java", &none, 1, 2, "").unwrap_err().contains("unix_args"));
    }

    #[test]
    fn builds_a_launch_line() {
        let line = launch_line("", "java", &jar("paper-1.21.8.jar"), 2048, 4096, "-XX:+UseG1GC").unwrap();
        assert!(line.starts_with("java "), "{line}");
        assert!(line.contains("-Xms2048M -Xmx4096M"), "{line}");
        assert!(line.contains("-XX:+UseG1GC"), "{line}");
        assert!(line.contains("-jar 'paper-1.21.8.jar' nogui"), "{line}");
        assert_eq!(launch_line("java -jar mine.jar", "java", &jar("x.jar"), 1, 2, "").unwrap(), "java -jar mine.jar");
        assert!(launch_line("", "java", &jar("  "), 1, 2, "").is_err());
    }

    #[test]
    fn makes_a_session_name_tmux_can_hold() {
        assert_eq!(Holder::session("instance-a1b2"), "se-instance-a1b2");
        assert_eq!(Holder::session("a.b/c"), "se-a-b-c");
    }

    #[test]
    fn sends_a_console_line_as_one_piece() {
        let line = Holder::Tmux.send("se-x", "say hi; rm -rf /");
        assert!(line.contains("'say hi; rm -rf /'"), "{line}");
        assert!(line.contains("send-keys -t 'se-x' -l"), "{line}");
        assert!(line.contains("Enter"), "{line}");

        let screen = Holder::Screen.send("se-x", "stop");
        assert!(screen.contains("stuff 'stop\n'"), "{screen}");
    }

    #[test]
    fn starts_in_the_server_folder() {
        let tmux = Holder::Tmux.start("se-x", "/home/u/mc", "java -jar server.jar nogui");
        assert!(tmux.contains("-c '/home/u/mc'"), "{tmux}");
        let screen = Holder::Screen.start("se-x", "/home/u/mc", "java -jar server.jar nogui");
        assert!(screen.starts_with("cd '/home/u/mc' &&"), "{screen}");
    }

    #[test]
    fn picks_the_jar_that_is_really_there() {
        let jars = |names: &[&str]| names.iter().map(|one| one.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_jar("paper.jar", &jars(&["server.jar"])), "server.jar");
        assert_eq!(pick_jar("paper.jar", &jars(&["paper.jar", "server.jar"])), "paper.jar");
        assert_eq!(pick_jar("paper.jar", &jars(&["lib.jar", "server.jar"])), "server.jar");
        assert_eq!(pick_jar("x.jar", &jars(&["helper.jar", "purpur-1.21.jar"])), "purpur-1.21.jar");
        assert_eq!(pick_jar("paper.jar", &[]), "paper.jar");
        assert_eq!(pick_jar("", &jars(&["server.jar"])), "server.jar");
    }

    #[test]
    fn launches_through_a_login_shell() {
        let line = launch_wrapper("abc.1", "cd x && java -jar 'my server.jar' nogui", false);
        assert!(line.starts_with("bash -lc '"), "{line}");
        assert!(line.contains("/tmp/monolab-se-abc-1.log"), "{line}");
        assert!(!line.contains("exec "), "{line}");
        assert!(line.contains("my server.jar"), "{line}");
    }

    #[test]
    fn wraps_nasty_names_as_one_word() {
        assert_eq!(quote("my server"), "'my server'");
        assert_eq!(quote("a; rm -rf /"), "'a; rm -rf /'");
        assert_eq!(quote("$(whoami)"), "'$(whoami)'");
        assert_eq!(quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn nonzero_exit_becomes_an_error() {
        let failed = Output {
            code: 2,
            stdout: String::new(),
            stderr: "No such file or directory".into(),
        };
        let message = failed.ok().unwrap_err();
        assert!(message.contains("No such file"), "{message}");
        assert!(message.contains('2'), "{message}");
        let quiet = Output {
            code: 1,
            stdout: "still says something".into(),
            stderr: "   ".into(),
        };
        assert!(quiet.ok().unwrap_err().contains("still"));
    }

    #[test]
    fn reads_a_paper_folder() {
        let seen = read_evidence(
            "@jars\npaper-1.21.8-40.jar\n@pumpkin\n@neoforge\n@forge\n@fabric\n\
             @plugins\nyes\n@mods\n@port\nserver-port=25566\n@log\n\
             [10:00:00 INFO]: Starting minecraft server version 1.21.8\n",
        );
        assert_eq!(seen.core, "Paper");
        assert_eq!(seen.version, "1.21.8");
        assert_eq!(seen.port, 25566);
        assert_eq!(seen.jar, "paper-1.21.8-40.jar");
    }

    #[test]
    fn reads_a_renamed_neoforge_folder() {
        let seen = read_evidence(
            "@jars\n@pumpkin\n@neoforge\n21.1.209\n@forge\n@fabric\n@plugins\n@mods\nyes\n\
             @port\n@log\n",
        );
        assert_eq!(seen.core, "NeoForge");
        assert_eq!(seen.version, "1.21.1");
        assert_eq!(seen.port, 25565);
    }

    #[test]
    fn reads_the_version_paper_writes_down() {
        let seen = read_evidence(
            "@jars\nserver.jar\n@pumpkin\n@neoforge\n@forge\n@fabric\n@plugins\nyes\n@mods\n\
             @port\nserver-port=47820\n\
             @history\n{\"currentVersion\": \"26.3-12-main (MC: 26.3)\"}\n@log\n",
        );
        assert_eq!(seen.core, "Paper");
        assert_eq!(seen.version, "26.3");
        assert_eq!(seen.port, 47820);
    }

    #[test]
    fn prefers_the_game_version_over_the_core_build() {
        assert_eq!(
            version_in_log("[10:00:02 INFO]: This server is running Paper version 1.21.8-40-main (MC: 1.21.8)"),
            Some("1.21.8".into())
        );
        assert_eq!(
            version_in_log("[10:00:00 INFO]: Starting minecraft server version 1.21.8"),
            Some("1.21.8".into())
        );
    }

    #[test]
    fn says_it_does_not_know() {
        let seen = read_evidence("@jars\n@pumpkin\n@neoforge\n@forge\n@fabric\n@plugins\n@mods\n@port\n@log\n");
        assert_eq!(seen.core, crate::sniff::UNKNOWN);
        assert_eq!(seen.version, "");
    }

    #[test]
    fn keeps_only_the_folders_that_hold_a_server() {
        let seen = read_scan(
            "@@folder paperserver\n@jars\npaper-1.21.8-40.jar\n@marker\nyes\n@pumpkin\n@neoforge\n@forge\n\
             @fabric\n@plugins\nyes\n@mods\n@port\nserver-port=25565\n@log\n\
             @@folder backups\n@jars\n@marker\n@pumpkin\n@neoforge\n@forge\n@fabric\n@plugins\n@mods\n@port\n@log\n\
             @@folder tools\n@jars\nBuildTools.jar\n@marker\n@pumpkin\n@neoforge\n@forge\n@fabric\n@plugins\n@mods\n@port\n@log\n\
             @@folder fabricserver\n@jars\n@marker\nyes\n@pumpkin\n@neoforge\n@forge\n@fabric\nyes\n@plugins\n@mods\nyes\n@port\nserver-port=25566\n@log\n\
             @@folder neo\n@jars\n@marker\n@pumpkin\n@neoforge\n21.1.248\n@forge\n@fabric\n@plugins\n@mods\n@port\n@log\n",
            "/home/ubuntu/server",
        );
        let names: Vec<&str> = seen.iter().map(|one| one.name.as_str()).collect();
        assert_eq!(names, ["paperserver", "fabricserver", "neo"]);
        assert_eq!(seen[0].path, "/home/ubuntu/server/paperserver");
        assert_eq!(seen[0].found.core, "Paper");
        assert_eq!(seen[1].found.core, "Fabric");
        assert_eq!(seen[1].found.port, 25566);
    }

    #[test]
    fn pulls_the_version_out_of_names_and_logs() {
        assert_eq!(version_in_name("paper-1.21.8-40.jar").as_deref(), Some("1.21.8"));
        assert_eq!(
            version_in_name("fabric-server-mc.1.21.4-loader.0.16.9.jar").as_deref(),
            Some("1.21.4")
        );
        assert_eq!(version_in_name("server.jar"), None);
        assert_eq!(
            version_in_log("[10:00:00 INFO]: Starting minecraft server version 1.21.8").as_deref(),
            Some("1.21.8")
        );
        assert_eq!(
            version_in_log("[INFO] Starting Pumpkin 0.1.0-dev+26.2-26.45").as_deref(),
            Some("0.1.0-dev+26.2-26.45")
        );
        assert_eq!(
            version_in_log("23:39:57  INFO Starting Pumpkin 0.2.1+26.3-26.51 Java Minecraft (Protocol 777) | 1.26.51 Bedrock (Protocol 2193)").as_deref(),
            Some("0.2.1+26.3-26.51")
        );
        assert_eq!(version_in_log("just some line"), None);
    }

    #[test]
    fn reads_a_real_ps_line() {
        let line = "   1234 mc       java -Xmx4G -jar paper-1.21.8.jar nogui";
        let (pid, user, command) = split_ps_line(line).unwrap();
        assert_eq!(pid, 1234);
        assert_eq!(user, "mc");
        assert!(command.starts_with("java -Xmx4G"));
        assert!(split_ps_line("").is_none());
        assert!(split_ps_line("PID USER ARGS").is_none());
    }

    #[test]
    fn tells_minecraft_from_other_java() {
        assert!(looks_like_minecraft("java -Xmx4G -jar paper-1.21.8.jar nogui"));
        assert!(looks_like_minecraft(
            "/usr/bin/java @user_jvm_args.txt @libraries/net/neoforged/.../unix_args.txt"
        ));
        assert!(!looks_like_minecraft(
            "java -Des.path.home=/usr/share/elasticsearch org.elasticsearch.bootstrap.Elasticsearch"
        ));
    }

    #[test]
    fn zero_exit_hands_back_stdout() {
        let fine = Output {
            code: 0,
            stdout: "ok\n".into(),
            stderr: String::new(),
        };
        assert_eq!(fine.ok().unwrap(), "ok\n");
    }
}

#[cfg(test)]
mod inside_tests {
    #[test]
    fn keeps_the_root_slash() {
        assert_eq!(super::inside("/", "/").unwrap(), "/");
        assert_eq!(super::inside("/", "").unwrap(), "/");
        assert_eq!(super::inside("/", "home").unwrap(), "/home");
        assert_eq!(super::inside("/srv/mc/", "/plugins").unwrap(), "/srv/mc/plugins");
    }
}

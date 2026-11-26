mod acp;
mod acp_binary;
mod acp_chats;
mod acp_host;
mod addons;
mod app_crash;
mod authorship;
mod backups;
mod catalog;
mod cfpack;
mod child_job;
mod chunkstats;
mod chunky;
mod claude_accounts;
mod claude_agent;
mod claude_chat;
mod client_content;
mod client_launch;
mod clients;
mod cloud;
mod codex_chat;
mod cores;
mod crash;
mod curseforge;
mod data_root;
mod databases;
mod db_tls;
mod device;
mod direct;
mod discord;
mod disk;
mod doctor;
mod editing;
mod events;
mod extensions;
mod forks;
mod geyser;
mod gitlink;
mod hashes;
mod integrations;
mod java;
mod kinds;
mod lag;
mod lang;
mod launch;
mod liveinv;
mod logbatch;
mod logs;
mod mapshare;
mod markdown;
mod market;
mod mcp;
mod memo;
mod mojang;
mod monolink_account;
mod monolink_team;
mod msmp;
mod nbt;
mod nbt_edit;
mod net;
mod panel;
mod panel_live;
mod paths;
mod pending;
mod pfs;
mod ping;
mod player_info;
mod playit;
mod progress;
mod properties;
mod proposals;
mod proxy;
mod pumpkin;
mod rcon;
mod remote;
mod rewind;
mod runlog;
mod sftp;
mod shell;
mod skills;
mod sniff;
mod spark;
mod store;
mod studio;
mod studio_bundle;
mod studio_expansions;
mod studio_held;
mod studio_pack;
mod studio_pack_blocks;
mod studio_pack_fonts;
mod studio_pack_items;
mod studio_pack_models;
mod studio_structure;
mod tasks;
mod team_agent;
mod team_notice;
mod team_storage;
mod teamlog;
mod textures;
mod tidy;
mod tray;
mod trim;
mod updates;
mod viewer;
mod vps_tunnel;
mod web;
mod webhook;
mod worldmap;
mod zips;

use crate::data_root::MonolabDirs;
use cores::download_server_jar;
use hashes::{verify_java_archive, verify_md5, verify_sha1, verify_sha256, verify_sha512};
use java::resolve_java_runtime;
use ping::query_server_ping;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::{Disks, Pid, System};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static INSTANCE_STORE: Mutex<()> = Mutex::new(());

fn lock_instance_store() -> std::sync::MutexGuard<'static, ()> {
    INSTANCE_STORE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn in_parallel<T, R, F>(items: Vec<T>, work: F) -> Vec<R>
where
    T: Send,
    R: Send,
    F: Fn(T) -> R + Send + Sync,
{
    let lanes = std::thread::available_parallelism().map(|one| one.get()).unwrap_or(4).clamp(1, 8);
    if lanes == 1 || items.len() < 2 {
        return items.into_iter().map(work).collect();
    }
    let chunk = items.len().div_ceil(lanes);
    let work = &work;
    let mut parts: Vec<Vec<T>> = Vec::new();
    let mut rest = items;
    while !rest.is_empty() {
        let take = chunk.min(rest.len());
        parts.push(rest.drain(..take).collect());
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = parts.into_iter().map(|part| scope.spawn(move || part.into_iter().map(work).collect::<Vec<R>>())).collect();
        handles.into_iter().filter_map(|handle| handle.join().ok()).flatten().collect()
    })
}

fn off_runtime<T, F>(work: F) -> T
where
    F: FnOnce() -> T + Send,
    T: Send,
{
    let run =
        || std::thread::scope(|scope| scope.spawn(work).join().unwrap_or_else(|_| panic!("The network worker thread ended unexpectedly.")));
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => tokio::task::block_in_place(run),
        _ => run(),
    }
}

fn plain_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy().to_string();
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            if !rest.starts_with("UNC\\") {
                return PathBuf::from(rest);
            }
        }
    }
    path
}

pub(crate) fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = Command::new(program);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

fn kill_process_tree(pid: u32) {
    #[cfg(windows)]
    {
        let _ = hidden_command("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).status();
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ServerInstance {
    id: String,
    name: String,
    path: String,
    core: String,
    version: String,
    #[serde(default = "default_launch_mode")]
    launch_mode: String,
    server_jar: Option<String>,
    java_path: String,
    max_memory_mib: u32,
    #[serde(default)]
    min_memory_mib: u32,
    jvm_args: String,
    port: u16,
    #[serde(default)]
    sort_order: u32,
    /// Home screen group, empty when the server is in none.
    #[serde(default)]
    group: String,
    #[serde(default)]
    start_with_app: bool,
    #[serde(default)]
    crash_restart: bool,
    #[serde(default)]
    core_build: String,
    created_at: u64,
    #[serde(default)]
    last_run_at: u64,
    #[serde(default)]
    custom_command: String,
    #[serde(default)]
    start_script: String,
    #[serde(default)]
    start_script_hash: String,
    #[serde(default)]
    remote_host: String,
    #[serde(default)]
    linked: bool,
    #[serde(default)]
    panel_host: String,
    #[serde(default)]
    panel_server: String,
    #[serde(default)]
    panel_daemon: String,
    /// The user picked this name (created with it or renamed). Otherwise it is the default "Server N",
    /// which the app shows in its own language.
    #[serde(default)]
    named: bool,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct CreateInstanceInput {
    name: String,
    core: String,
    version: String,
    #[serde(default)]
    core_build: String,
    #[serde(default = "default_memory_mib")]
    max_memory_mib: u32,
    #[serde(default)]
    min_memory_mib: u32,
    eula_accepted: bool,
    #[serde(default)]
    operation_id: String,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct LaunchSettingsInput {
    id: String,
    max_memory_mib: u32,
    #[serde(default)]
    min_memory_mib: u32,
    jvm_args: String,
    #[serde(default = "default_java_path")]
    java_path: String,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct UpgradeInput {
    id: String,
    target_version: String,
    #[serde(default)]
    target_build: Option<String>,
    #[serde(default)]
    skip_backup: bool,
    /// Another core to switch to (Paper, Purpur, Leaf, Folia; from Vanilla too); empty keeps the current one.
    #[serde(default)]
    target_core: Option<String>,
}

const PLUGIN_CORES: &[&str] = &["Paper", "Purpur", "Leaf", "Folia"];

/// The core an upgrade ends on: the requested one when worlds and plugins carry over to it.
fn core_after(current: &str, wanted: Option<&str>) -> Result<String, String> {
    let wanted = wanted.map(str::trim).filter(|one| !one.is_empty() && *one != current);
    let Some(next) = wanted else { return Ok(current.to_string()) };
    let carries = PLUGIN_CORES.contains(&next) && (PLUGIN_CORES.contains(&current) || current == "Vanilla");
    if carries {
        Ok(next.to_string())
    } else {
        Err(format!("Switching from {current} to {next} isn't supported: worlds or add-ons don't carry over."))
    }
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct RenameInstanceInput {
    id: String,
    name: String,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct MrpackImportInput {
    id: String,
    path: String,
    include_client_only: bool,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct MrpackFileEntry {
    path: String,
    compatibility: String,
    download_count: usize,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct PlayerAccessResult {
    via_console: bool,
    added: bool,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct PlayerAccessInput {
    id: String,
    name: String,
    uuid: String,
    action: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct InstanceView {
    #[serde(flatten)]
    instance: ServerInstance,
    status: String,
    uptime_seconds: Option<u64>,
    crash_reason: Option<String>,
    ready: bool,
    #[serde(default)]
    checking: bool,
    #[serde(default)]
    unreachable: bool,
    /// A server on this computer whose folder is gone (moved, renamed, deleted or on an unplugged drive).
    #[serde(default)]
    missing: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct TrashEntry {
    id: String,
    name: String,
    original_path: String,
    trash_path: String,
    deleted_at: u64,
    #[serde(default)]
    instance: Option<ServerInstance>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ServerLogEvent {
    server_id: String,
    line: String,
    stream: String,
    #[serde(default)]
    noise: bool,
    #[serde(default)]
    ansi: String,
    /// For a sent command (`input`): the Monolink teammate who sent it (their handle), or "" for this PC.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[specta(optional)]
    by: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ParsedLogEntry {
    line: String,
    level: String,
    event: String,
    source: String,
}

#[derive(Clone, Debug)]
pub(crate) struct DownloadSource {
    pub url: String,
    pub sha1: Option<String>,
    pub sha256: Option<String>,
    pub md5: Option<String>,
}

impl DownloadSource {
    pub fn unchecked(url: impl Into<String>) -> Self {
        Self { url: url.into(), sha1: None, sha256: None, md5: None }
    }
}

struct ManagedProcess {
    child: Child,
    pid: u32,
    started_at: SystemTime,
    stop_requested_at: Option<SystemTime>,
    ready: bool,
    stop_when_ready: bool,
    stop_from_log: bool,
    crash_seen: bool,
}

impl ManagedProcess {
    /// Asks the server to stop with its console command.
    fn send_stop(&mut self) -> Result<(), String> {
        let stdin = self.child.stdin.as_mut().ok_or("The server's console is closed.")?;
        stdin.write_all(b"stop\n").map_err(|error| format!("Could not send the stop command: {error}"))?;
        stdin.flush().map_err(|error| error.to_string())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct PropertyEntry {
    key: String,
    value: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ResourceSnapshot {
    cpu_percent: f32,
    memory_mib: u64,
    disk_free_mib: u64,
    disk_total_mib: u64,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct TpsSnapshot {
    tps: Option<f32>,
    mspt: Option<f32>,
    updated_at: u64,
    heap_mib: Option<u64>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PingSnapshot {
    pub online: u32,
    pub max: u32,
    pub names: Vec<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct PlayerEntry {
    name: String,
    uuid: String,
    whitelisted: bool,
    operator: bool,
    banned: bool,
    source: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct PlayerActivityEntry {
    name: String,
    sessions: u32,
    last_seen_at: u64,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AddonEntry {
    file_name: String,
    name: String,
    kind: String,
    enabled: bool,
    metadata: String,
    compatibility: String,
    version: String,
    icon: Option<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct WorldEntry {
    name: String,
    size_mib: f64,
    modified_at: u64,
    has_level_data: bool,
    realm: String,
    world: String,
    inside_parent: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AutomationHistoryEntry {
    server_id: String,
    action: String,
    detail: String,
    created_at: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AuditEntry {
    id: String,
    server_id: String,
    category: String,
    action: String,
    detail: String,
    created_at: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct UpgradeHistoryEntry {
    server_id: String,
    from_version: String,
    to_version: String,
    previous_jar: String,
    created_at: u64,
    /// Set when the upgrade also switched the core, so a rollback switches it back.
    from_core: Option<String>,
    to_core: Option<String>,
}

type RegionKey = (String, String, i32, i32);

#[derive(Clone, Debug, Default)]
struct ChunkFolder {
    folder: String,
    regions: HashMap<String, (u64, u64)>,
    entities: HashMap<String, (u64, u64)>,
}
type TileKey = (String, String, i32, i32, i32);
/// Server, layer, region, shrink and the region file's stamp, so a redrawn region never hits an old entry.
type CoarseKey = (String, String, i32, i32, usize, Option<(u64, u64)>);

/// Drops the least recently used entries once past `cap`, so browsing a big map can't grow memory without end.
struct Lru<K, V> {
    map: HashMap<K, (V, u64)>,
    clock: u64,
    cap: usize,
}

impl<K: std::hash::Hash + Eq, V> Lru<K, V> {
    fn new(cap: usize) -> Self {
        Lru { map: HashMap::new(), clock: 0, cap }
    }

    fn get(&mut self, key: &K) -> Option<&V> {
        self.clock += 1;
        let clock = self.clock;
        self.map.get_mut(key).map(|(value, used)| {
            *used = clock;
            &*value
        })
    }

    fn insert(&mut self, key: K, value: V) {
        self.clock += 1;
        self.map.insert(key, (value, self.clock));
        if self.map.len() > self.cap {
            let mut ages: Vec<u64> = self.map.values().map(|(_, used)| *used).collect();
            let gone = self.map.len() - self.cap * 3 / 4;
            let cut = *ages.select_nth_unstable(gone - 1).1;
            self.map.retain(|_, (_, used)| *used > cut);
        }
    }

    fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool) {
        self.map.retain(|key, (value, _)| keep(key, value));
    }

    fn clear(&mut self) {
        self.map.clear();
    }
}
type RegionStamps = HashMap<(i32, i32), (u64, u64)>;
type MapIndex = HashMap<(String, String), Arc<RegionStamps>>;

struct AppState {
    processes: Arc<Mutex<HashMap<String, ManagedProcess>>>,
    crashed: Arc<Mutex<HashMap<String, String>>>,
    logs: Arc<Mutex<HashMap<String, Vec<ServerLogEvent>>>>,
    tps: Arc<Mutex<HashMap<String, TpsSnapshot>>>,
    cancelled_downloads: Arc<Mutex<HashSet<String>>>,
    remote_tailing: Arc<Mutex<HashSet<String>>>,
    remote_checked: Arc<Mutex<HashSet<String>>>,
    remote_unreachable: Arc<Mutex<HashSet<String>>>,
    remote_failures: Arc<Mutex<HashMap<String, u32>>>,
    remote_running: Arc<Mutex<HashMap<String, remote::Live>>>,
    remote_starting: Arc<Mutex<HashMap<String, std::time::Instant>>>,
    remote_kick: Arc<tokio::sync::Notify>,
    panel_live: Arc<Mutex<HashMap<String, panel_live::PanelLive>>>,
    panel_checked: Arc<Mutex<HashSet<String>>>,
    panel_unreachable: Arc<Mutex<HashSet<String>>>,
    panel_sockets: Arc<Mutex<HashSet<String>>>,
    panel_backlog: Arc<Mutex<HashSet<String>>>,
    panel_kick: Arc<tokio::sync::Notify>,
    system: Arc<Mutex<System>>,
    map_cells: Arc<Mutex<Lru<RegionKey, Arc<worldmap::Tile>>>>,
    map_coarse: Arc<Mutex<Lru<CoarseKey, Arc<worldmap::Tile>>>>,
    map_tiles: Arc<Mutex<Lru<TileKey, Arc<Vec<u8>>>>>,
    map_palettes: Arc<Mutex<HashMap<String, Arc<worldmap::Palette>>>>,
    map_index: Arc<Mutex<MapIndex>>,
    resource_history: Arc<Mutex<HashMap<String, VecDeque<ResourceSample>>>>,
    spark_reports: Arc<Mutex<HashMap<String, spark::SparkReport>>>,
    chunky: Arc<Mutex<HashMap<String, chunky::ChunkyProgress>>>,
    chunk_stats: Arc<Mutex<HashMap<RegionKey, Arc<chunkstats::RegionStats>>>>,
    chunk_entities: Arc<Mutex<HashMap<RegionKey, Arc<chunkstats::Grid>>>>,
    chunk_folders: Arc<Mutex<HashMap<(String, String), ChunkFolder>>>,
}

#[derive(Clone, Copy, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ResourceSample {
    at: u64,
    cpu_percent: f32,
    memory_mib: Option<u64>,
    tps: Option<f32>,
}

fn instances_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&data_dir).map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(data_dir.join("instances.json"))
}

fn trash_entries_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&data_dir).map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(data_dir.join("instance-trash.json"))
}

fn load_trash_entries(app: &AppHandle) -> Result<Vec<TrashEntry>, String> {
    Ok(store::read(&trash_entries_path(app)?)?.unwrap_or_default())
}

fn save_trash_entries(app: &AppHandle, entries: &[TrashEntry]) -> Result<(), String> {
    store::write(&trash_entries_path(app)?, &serde_json::to_string_pretty(entries).map_err(|error| error.to_string())?)
        .map_err(|error| format!("Could not save the trash record: {error}"))
}

fn announce(app: &AppHandle, server_id: &str, kind: &str, message: &str) -> Result<(), String> {
    static LAST: Mutex<Option<HashMap<(String, String), u64>>> = Mutex::new(None);
    let now = unix_now();
    {
        let mut last = LAST.lock().expect("announce state poisoned");
        let last = last.get_or_insert_with(HashMap::new);
        let key = (server_id.to_string(), kind.to_string());
        if last.get(&key).is_some_and(|at| now.saturating_sub(*at) < 60) {
            return Ok(());
        }
        last.insert(key, now);
    }
    let alert = serde_json::json!({ "serverId": server_id, "kind": kind, "message": message });
    remember_for_web(app, "server-alert", &alert);
    let _ = app.emit("server-alert", alert);
    let name = instance_by_id(app, server_id).map(|one| one.name).unwrap_or_default();
    webhook::send(app, server_id, &name, &kind.to_ascii_lowercase(), message);
    Ok(())
}

fn automation_history_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&data_dir).map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(data_dir.join("automation-history.json"))
}

fn audit_log_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&data_dir).map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(data_dir.join("audit-log.json"))
}

fn upgrade_history_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&data_dir).map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(data_dir.join("upgrade-history.json"))
}

fn read_upgrade_history(app: &AppHandle) -> Vec<UpgradeHistoryEntry> {
    upgrade_history_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_upgrade_history(app: &AppHandle, mut entries: Vec<UpgradeHistoryEntry>) {
    if entries.len() > 100 {
        entries.drain(..entries.len() - 100);
    }
    let Some(path) = upgrade_history_path(app).ok() else {
        return;
    };
    let _ = serde_json::to_string_pretty(&entries)
        .map_err(|error| error.to_string())
        .and_then(|text| fs::write(path, text).map_err(|error| error.to_string()));
}

fn record_automation(app: &AppHandle, server_id: &str, action: &str, detail: &str) {
    let Ok(path) = automation_history_path(app) else {
        return;
    };
    let mut entries =
        fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Vec<AutomationHistoryEntry>>(&text).ok()).unwrap_or_default();
    entries.push(AutomationHistoryEntry {
        server_id: server_id.into(),
        action: action.into(),
        detail: detail.into(),
        created_at: unix_now(),
    });
    if entries.len() > 500 {
        entries.drain(..entries.len() - 500);
    }
    let _ = serde_json::to_string_pretty(&entries)
        .map_err(|error| error.to_string())
        .and_then(|text| fs::write(path, text).map_err(|error| error.to_string()));
}

/// F2: whether the file changed since an editor opened it. `opened` is the SHA-256 of the text the editor got
/// (`studio::hash`); `None` means the editor didn't say, and the save goes ahead as before. A file that is gone now
/// counts as changed.
fn saved_since(now: Option<&str>, opened: Option<&str>) -> bool {
    match (now, opened) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(text), Some(want)) => studio::hash(text) != want,
    }
}

/// F2: remembers who wrote the paths a request writes (see `authorship`), for the Rewind history. The latest note
/// wins, so the host's own command notes `host` first and the web panel, a teammate or an agent then names itself.
pub(crate) fn note_writes(app: &AppHandle, server_id: &str, command: &str, body: &serde_json::Value, who: &str) {
    let paths = authorship::paths_of(command, body);
    if paths.is_empty() {
        return;
    }
    let root = instance_by_id(app, server_id).ok().and_then(|one| studio_root(&one));
    for path in paths {
        authorship::note(server_id, &authorship::with_studio_root(&path, root), who);
    }
}

fn record_audit(app: &AppHandle, server_id: &str, category: &str, action: &str, detail: &str) {
    // A plugin added to a server shared with a team shows in the team's activity (names only).
    for name in monolink_team::added_plugins(category, action, detail) {
        monolink_team::report(app, server_id, "plugin_added", serde_json::json!({ "name": name }));
    }
    let Ok(path) = audit_log_path(app) else {
        return;
    };
    let mut entries =
        fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Vec<AuditEntry>>(&text).ok()).unwrap_or_default();
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    entries.push(AuditEntry {
        id: format!("audit-{millis}"),
        server_id: server_id.into(),
        category: category.into(),
        action: action.into(),
        detail: detail.chars().take(500).collect(),
        created_at: unix_now(),
    });
    if entries.len() > 1_000 {
        entries.drain(..entries.len() - 1_000);
    }
    let _ = serde_json::to_string_pretty(&entries)
        .map_err(|error| error.to_string())
        .and_then(|text| fs::write(path, text).map_err(|error| error.to_string()));
}

fn load_instances(app: &AppHandle) -> Result<Vec<ServerInstance>, String> {
    let path = instances_path(app)?;
    let mut instances: Vec<ServerInstance> =
        store::read(&path).map_err(|error| format!("Could not read the server list: {error}"))?.unwrap_or_default();
    for instance in &mut instances {
        instance.path = plain_path(PathBuf::from(&instance.path)).to_string_lossy().to_string();
        if let Some(jar) = &instance.server_jar {
            instance.server_jar = Some(plain_path(PathBuf::from(jar)).to_string_lossy().to_string());
        }
    }
    let homes = app_server_homes(app);
    for instance in &mut instances {
        instance.linked = !instance.remote_host.is_empty() || !inside_any(&instance.path, &homes);
    }
    Ok(instances)
}

fn app_server_homes(app: &AppHandle) -> Vec<String> {
    let mut homes = Vec::new();
    for root in [managed_servers_root(app).ok(), default_servers_root(app).ok()].into_iter().flatten() {
        for one in [Some(root.clone()), root.canonicalize().ok().map(plain_path)].into_iter().flatten() {
            let text = one.to_string_lossy().replace('\\', "/").to_lowercase();
            let text = text.trim_end_matches('/').to_string();
            if !text.is_empty() && !homes.contains(&text) {
                homes.push(text);
            }
        }
    }
    homes
}

fn inside_any(path: &str, homes: &[String]) -> bool {
    let path = path.replace('\\', "/").to_lowercase();
    homes.iter().any(|home| path.strip_prefix(home.as_str()).is_some_and(|rest| rest.starts_with('/')))
}

fn save_instances(app: &AppHandle, instances: &[ServerInstance]) -> Result<(), String> {
    let path = instances_path(app)?;
    let text = serde_json::to_string_pretty(instances).map_err(|error| error.to_string())?;
    store::write(&path, &text).map_err(|error| format!("Could not save the server list: {error}"))
}

fn mark_last_run(app: &AppHandle, id: &str) {
    let _store = lock_instance_store();
    let Ok(mut instances) = load_instances(app) else {
        return;
    };
    if let Some(found) = instances.iter_mut().find(|instance| instance.id == id) {
        found.last_run_at = unix_now();
        let _ = save_instances(app, &instances);
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn peek_next_server_name(app: &AppHandle) -> String {
    let taken: HashSet<String> = load_instances(app).unwrap_or_default().into_iter().map(|instance| instance.name).collect();
    (1_u32..).map(|number| format!("Server {number}")).find(|name| !taken.contains(name)).unwrap_or_else(|| "New server".to_string())
}

#[tauri::command(async)]
#[specta::specta]
fn suggest_server_name(app: AppHandle) -> Result<String, String> {
    Ok(peek_next_server_name(&app))
}

fn instance_id() -> String {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    format!("instance-{millis}")
}

fn sane_min_memory(min: u32, max: u32) -> u32 {
    if min == 0 {
        return max;
    }
    min.clamp(256, max)
}

fn default_memory_mib() -> u32 {
    4096
}
fn default_java_path() -> String {
    "java".into()
}
fn default_launch_mode() -> String {
    "jar".into()
}
fn normalized_existing_path(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value.trim());
    if !path.is_dir() {
        return Err("Server folder not found. Check the path.".into());
    }
    path.canonicalize().map(plain_path).map_err(|error| format!("Could not verify the server folder: {error}"))
}

fn servers_root_setting_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&data_dir).map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(data_dir.join("servers-root.txt"))
}

fn default_servers_root(app: &AppHandle) -> Result<PathBuf, String> {
    // Servers made by older versions stay where they are; new installs keep them in the data folder.
    if let Ok(appdata) = std::env::var("APPDATA") {
        let path = PathBuf::from(appdata).join("Monolab").join("servers");
        if path.is_dir() {
            return Ok(path);
        }
    }
    let fallback = app.path().monolab_data_dir().or_else(|_| app.path().document_dir()).map_err(|error| error.to_string())?;
    Ok(fallback.join("servers"))
}

fn managed_servers_root(app: &AppHandle) -> Result<PathBuf, String> {
    let chosen = servers_root_setting_path(app)
        .ok()
        .filter(|path| path.is_file())
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|text| PathBuf::from(text.trim()))
        .filter(|path| !path.as_os_str().is_empty() && path.is_dir());
    let root = match chosen {
        Some(path) => path,
        None => default_servers_root(app)?,
    };
    fs::create_dir_all(&root).map_err(|error| format!("Could not create the default server folder: {error}"))?;
    Ok(root)
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
struct AppSettings {
    auto_backup_on_save: bool,
    open_console_on_start: bool,
    default_memory_mib: u32,
    keep_backup_days: u32,
    #[serde(default)]
    keep_auto_backups: u32,
    console_line_limit: u32,
    #[serde(default)]
    backup_root: String,
    #[serde(default)]
    backup_worlds_only: bool,
    #[serde(default)]
    backup_include_git: bool,
    #[serde(default)]
    web_panel: bool,
    #[serde(default = "default_web_port")]
    web_panel_port: u16,
    #[serde(default)]
    web_direct: bool,
    /// The web panel speaks HTTPS with this app's own certificate. Off: plain HTTP.
    #[serde(default)]
    web_https: bool,
    /// Where the web panel listens: "" every network, "127.0.0.1" this PC only, or one network card's IPv4.
    #[serde(default)]
    web_bind: String,
    #[serde(default)]
    web_accounts: Vec<web::auth::Account>,
    /// Open invite links; only digests of their tokens are stored.
    #[serde(default)]
    web_invites: Vec<web::auth::Invite>,
    /// Roles the host made, with the permissions they ticked. Web accounts and
    /// teammates (on this PC's servers) can be given them next to the built-in ones.
    #[serde(default)]
    custom_roles: Vec<web::auth::CustomRole>,
    /// What teammates see this device as (empty: the PC's name).
    #[serde(default)]
    relay_name: String,
    /// This PC's servers attached to Monolink teams. A grant opens only servers listed here.
    #[serde(default)]
    monolink_teams: Vec<monolink_team::TeamLink>,
    /// The main window's X hides Monolab to the tray (servers keep running) instead of quitting.
    #[serde(default = "default_true")]
    close_to_tray: bool,
    /// The "still running in the tray" notice was shown once already.
    #[serde(default)]
    tray_hint_shown: bool,
    /// Show in Discord what Monolab is doing (never server addresses). Off by default.
    #[serde(default)]
    discord_presence: bool,
    /// "stable" or "beta": beta also reads the beta update manifest.
    #[serde(default = "default_update_channel")]
    update_channel: String,
    /// Send anonymous usage counts (an install id, versions, OS) on the live connection to mnlab.cc.
    #[serde(default = "default_true")]
    anonymous_stats: bool,
    /// The first-run notice about anonymous counts was shown and closed.
    #[serde(default)]
    stats_notice_seen: bool,
}

fn default_update_channel() -> String {
    "stable".into()
}

fn default_true() -> bool {
    true
}

fn default_web_port() -> u16 {
    25566
}
const OLD_WEB_PORT: u16 = 25580;

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            auto_backup_on_save: false,
            open_console_on_start: true,
            default_memory_mib: 4096,
            keep_backup_days: 30,
            keep_auto_backups: 0,
            console_line_limit: 2000,
            backup_root: String::new(),
            backup_worlds_only: false,
            backup_include_git: false,
            web_panel: false,
            web_panel_port: default_web_port(),
            web_direct: false,
            web_https: false,
            web_bind: String::new(),
            web_accounts: Vec::new(),
            web_invites: Vec::new(),
            custom_roles: Vec::new(),
            relay_name: String::new(),
            monolink_teams: Vec::new(),
            close_to_tray: true,
            tray_hint_shown: false,
            discord_presence: false,
            update_channel: default_update_channel(),
            anonymous_stats: true,
            stats_notice_seen: false,
        }
    }
}

fn app_settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&data_dir).map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(data_dir.join("settings.json"))
}

pub(crate) fn read_app_settings(app: &AppHandle) -> AppSettings {
    let mut settings: AppSettings = match app_settings_path(app) {
        Ok(path) => match store::read(&path) {
            Ok(found) => found.unwrap_or_default(),
            Err(_) => {
                store::set_aside(&path);
                AppSettings::default()
            }
        },
        Err(_) => AppSettings::default(),
    };
    if settings.web_panel_port == OLD_WEB_PORT {
        settings.web_panel_port = default_web_port();
    }
    backups::INCLUDE_GIT.store(settings.backup_include_git, std::sync::atomic::Ordering::Relaxed);
    settings
}

impl AppSettings {
    fn without_secrets(mut self) -> Self {
        for account in &mut self.web_accounts {
            account.hash.clear();
        }
        for invite in &mut self.web_invites {
            invite.digest.clear();
        }
        self
    }
}

#[tauri::command(async)]
#[specta::specta]
fn get_app_settings(app: AppHandle) -> Result<AppSettings, String> {
    Ok(read_app_settings(&app).without_secrets())
}

#[tauri::command(async)]
#[specta::specta]
fn set_app_settings(app: AppHandle, settings: AppSettings) -> Result<AppSettings, String> {
    let saved = read_app_settings(&app);
    let settings = AppSettings {
        default_memory_mib: settings.default_memory_mib.clamp(512, 64 * 1024),
        keep_backup_days: settings.keep_backup_days.min(3650),
        keep_auto_backups: settings.keep_auto_backups.min(1000),
        console_line_limit: settings.console_line_limit.clamp(200, 20_000),
        update_channel: if settings.update_channel == "beta" { "beta".into() } else { default_update_channel() },
        web_accounts: saved.web_accounts,
        web_invites: saved.web_invites,
        // Changed only through the roles commands, which keep people's roles valid.
        custom_roles: saved.custom_roles,
        relay_name: saved.relay_name,
        monolink_teams: saved.monolink_teams,
        tray_hint_shown: saved.tray_hint_shown,
        // Changed only through set_web_https / set_web_bind, which also restart the panel.
        web_https: saved.web_https,
        web_bind: saved.web_bind,
        ..settings
    };
    store::write(&app_settings_path(&app)?, &serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?)
        .map_err(|error| format!("Could not save settings: {error}"))?;
    if !settings.discord_presence {
        discord::set(None);
    }
    if settings.anonymous_stats != saved.anonymous_stats {
        updates::reconnect(&app);
    }
    Ok(settings.without_secrets())
}

/// Whether this build can show a Discord status (it needs a Discord application id).
#[tauri::command]
#[specta::specta]
fn discord_presence_available() -> bool {
    discord::available()
}

/// Shows what the app is doing in Discord, or clears it. Does nothing while the setting is off.
#[tauri::command(async)]
#[specta::specta]
fn set_discord_presence(app: AppHandle, presence: Option<discord::DiscordPresence>) {
    discord::set(presence.filter(|_| read_app_settings(&app).discord_presence));
}

fn relay_name_or_default(name: &str) -> String {
    let name = relay_proto::clean_name(name);
    if name != "Monolab" || std::env::var("COMPUTERNAME").is_err() {
        return name;
    }
    let computer = std::env::var("COMPUTERNAME").unwrap_or_default();
    // A second profile on the same PC needs a name of its own, or both devices look the same.
    match dev_profile() {
        Some(profile) => relay_proto::clean_name(&format!("{computer} ({profile})")),
        None => relay_proto::clean_name(&computer),
    }
}

/// A role's name for logs: the custom role's name, or the built-in name as it is.
pub(crate) fn role_name(role: &str, custom: &[web::auth::CustomRole]) -> String {
    custom.iter().find(|one| one.id == role).map(|one| one.name.clone()).unwrap_or_else(|| role.to_string())
}

/// The host's own roles.
pub(crate) fn custom_roles(app: &AppHandle) -> Vec<web::auth::CustomRole> {
    read_app_settings(app).custom_roles
}

#[cfg(test)]
mod portable_settings_tests {
    #[test]
    fn secrets_and_web_switches_stay_home() {
        let settings = super::AppSettings { web_panel: true, relay_name: "pc".into(), default_memory_mib: 8192, ..Default::default() };
        let portable = super::portable_settings(&settings);
        assert_eq!(portable.get("defaultMemoryMib"), Some(&serde_json::json!(8192)));
        for key in ["webPanel", "webAccounts", "webInvites", "webDirect", "customRoles", "monolinkTeams", "relayName"] {
            assert!(!portable.contains_key(key), "{key} must not travel");
        }
    }

    #[test]
    fn import_takes_only_portable_keys_and_lists_changes() {
        let current = super::AppSettings::default();
        let incoming = serde_json::json!({ "defaultMemoryMib": 2048, "webPanel": true, "webDirect": true, "closeToTray": true });
        let merged = super::merged_settings(&current, &incoming).unwrap();
        assert_eq!(merged.default_memory_mib, 2048);
        assert!(!merged.web_panel && !merged.web_direct);
        let changes = super::setting_changes(&current, &incoming).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].key, "defaultMemoryMib");
        assert_eq!((changes[0].from.as_str(), changes[0].to.as_str()), ("4096", "2048"));
    }
}

#[cfg(test)]
mod role_tests {
    use crate::web::auth::CustomRole;

    #[test]
    fn role_names_for_logs() {
        let custom = vec![CustomRole { id: "role:x".into(), name: "Helper".into(), perms: vec![] }];
        assert_eq!(super::role_name("role:x", &custom), "Helper");
        assert_eq!(super::role_name("Viewer", &custom), "Viewer");
    }

    #[test]
    fn moving_people_off_a_role_reaches_everyone() {
        let mut settings = super::AppSettings {
            custom_roles: vec![CustomRole { id: "role:x".into(), name: "Helper".into(), perms: vec![] }],
            ..Default::default()
        };
        settings.web_accounts.push(crate::web::auth::Account { name: "a".into(), role: "role:x".into(), ..Default::default() });
        settings.monolink_teams.push(crate::monolink_team::TeamLink {
            team: "t".into(),
            servers: vec![],
            people: vec![crate::monolink_team::TeamPersonRole { handle: "c".into(), role: "role:x".into() }],
        });
        assert_eq!(super::role_users(&settings, "role:x"), 2);
        super::move_role(&mut settings, "role:x", "Viewer");
        assert_eq!(super::role_users(&settings, "role:x"), 0);
        assert_eq!(settings.web_accounts[0].role, "Viewer");
        assert_eq!(settings.monolink_teams[0].people[0].role, "Viewer");
    }
}

/// A file a teammate is sending in pieces.
struct MonolinkUpload {
    peer: relay_proto::DeviceId,
    slot: PathBuf,
    size: u64,
    written: u64,
    touched: std::time::Instant,
}

/// Largest file a guest may send, the same as through the web panel.
const MONOLINK_UPLOAD_MAX: u64 = 4 * 1024 * 1024 * 1024;
/// A piece is at most this much (before base64).
const MONOLINK_UPLOAD_PART: usize = 4 * 1024 * 1024;

fn monolink_uploads() -> &'static Mutex<HashMap<String, MonolinkUpload>> {
    static UPLOADS: std::sync::OnceLock<Mutex<HashMap<String, MonolinkUpload>>> = std::sync::OnceLock::new();
    UPLOADS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Receives a file from a guest in order: begin (name and size) → parts → end, which gives the same
/// token the web panel's upload gives, for the commands that take uploaded files.
fn monolink_upload(
    app: &AppHandle,
    peer: &relay_proto::DeviceId,
    command: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    use base64::Engine;
    use std::io::Write;
    let text = |name: &str| body.get(name).and_then(serde_json::Value::as_str).unwrap_or_default().to_string();
    let mut uploads = monolink_uploads().lock().expect("uploads poisoned");
    // Forget uploads nobody finished within an hour.
    uploads.retain(|_, one| {
        let keep = one.touched.elapsed() < Duration::from_secs(3600);
        if !keep {
            let _ = fs::remove_file(&one.slot);
        }
        keep
    });
    match command {
        "__upload_begin" => {
            let size = body.get("size").and_then(serde_json::Value::as_u64).ok_or("Say how big the file is.")?;
            if size > MONOLINK_UPLOAD_MAX {
                return Err("Files larger than 4 GB can't be uploaded.".into());
            }
            if uploads.values().filter(|one| one.peer == *peer).count() >= 4 {
                return Err("Too many uploads at once. Wait for the others to finish.".into());
            }
            let slot = stash_slot(app, &text("name"))?;
            fs::File::create(&slot).map_err(|error| error.to_string())?;
            let mut random = [0u8; 16];
            getrandom::fill(&mut random).map_err(|error| error.to_string())?;
            let id = relay_proto::hex(&random);
            uploads.insert(id.clone(), MonolinkUpload { peer: *peer, slot, size, written: 0, touched: std::time::Instant::now() });
            Ok(serde_json::json!({ "upload": id }))
        }
        "__upload_part" => {
            let id = text("upload");
            let upload = uploads.get_mut(&id).filter(|one| one.peer == *peer).ok_or("That upload isn't open any more.")?;
            let at = body.get("at").and_then(serde_json::Value::as_u64).ok_or("Say where this piece goes.")?;
            if at != upload.written {
                return Err("A piece of the file went missing. Try again.".into());
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(text("data"))
                .map_err(|_| "A piece of the file was damaged.".to_string())?;
            if bytes.len() > MONOLINK_UPLOAD_PART || upload.written + bytes.len() as u64 > upload.size {
                return Err("The file is bigger than it said.".into());
            }
            let mut file = fs::OpenOptions::new().append(true).open(&upload.slot).map_err(|error| error.to_string())?;
            file.write_all(&bytes).map_err(|error| error.to_string())?;
            upload.written += bytes.len() as u64;
            upload.touched = std::time::Instant::now();
            Ok(serde_json::json!(upload.written))
        }
        _ => {
            let id = text("upload");
            let upload = uploads.remove(&id).filter(|one| one.peer == *peer).ok_or("That upload isn't open any more.")?;
            if upload.written != upload.size {
                let _ = fs::remove_file(&upload.slot);
                return Err("The file didn't arrive whole. Try again.".into());
            }
            Ok(serde_json::Value::String(upload_token(&upload.slot)))
        }
    }
}

/// How this PC reaches a team host right now: "direct", "relayed" or "none".
#[tauri::command]
#[specta::specta]
fn monolink_team_route(team: String, host: String) -> String {
    relay_proto::DeviceId::parse(&host).map(|host| direct::team_route(&team, &host)).unwrap_or("none").to_string()
}

/// Runs a web panel command on a teammate's host through Monolink, with this team's grant. Used by host windows.
/// `body` and the answer are JSON text (a JSON value type in the signature trips the bindings generator).
#[tauri::command]
#[specta::specta]
async fn monolink_call(app: AppHandle, peer: String, command: String, body: String, team: String) -> Result<String, String> {
    if !monolink_account::is_signed_in(&app) {
        return Err("Sign in to Monolink first (Monolink page, Account).".into());
    }
    let peer = relay_proto::DeviceId::parse(&peer).ok_or("Unknown device.")?;
    let body: serde_json::Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    // Events are a long wait on the host; everything else should answer quickly.
    let wait = std::time::Duration::from_secs(if command == "__events" { 40 } else { 90 });
    if team.is_empty() {
        return Err("Open the server from its team.".into());
    }
    Ok(direct::call_team(&app, &team, peer, &command, &body, wait).await?.to_string())
}

/// The label of the team view that shows `device`'s servers through `team`: a webview of its own
/// inside the Monolab window. Its label is what [`monolink_guard`] trusts, so the host and team are in it.
fn monolink_label(device: &relay_proto::DeviceId, team: &str) -> String {
    let mut label = format!("monolink-{}-", device.text().to_ascii_lowercase());
    label.extend(team.bytes().map(|byte| format!("{byte:02x}")));
    label
}

/// The only app commands a team view may call. Everything else it shows comes from the host
/// through `monolink_call`, so a host that slips a script into the page can't reach this PC.
const MONOLINK_WINDOW_COMMANDS: &[&str] = &["monolink_call", "monolink_team_route", "monolink_close"];

/// Commands the main window may send to a team host on its own (the team log); opening their
/// servers happens in a Monolink window.
const MAIN_WINDOW_MONOLINK: &[&str] = &["__team_log"];

/// Checks an app command against the window that sent it. `None` lets it through.
fn monolink_guard(label: &str, command: &str, args: Option<&serde_json::Value>) -> Option<&'static str> {
    let text = |key: &str| args.and_then(|one| one.get(key)).and_then(|one| one.as_str()).unwrap_or_default().to_string();
    if !label.starts_with("monolink-") {
        if command == "monolink_call" && !MAIN_WINDOW_MONOLINK.contains(&text("command").as_str()) {
            return Some("Open their servers in a Monolink window.");
        }
        return None;
    }
    if !MONOLINK_WINDOW_COMMANDS.contains(&command) {
        return Some("A Monolink window can't use this.");
    }
    // A team view talks only to the host (and through the team) it was opened for, and closes only itself.
    let peer = if command == "monolink_team_route" { text("host") } else { text("peer") };
    let team = text("team");
    let Some(device) = relay_proto::DeviceId::parse(&peer) else {
        return Some("Unknown device.");
    };
    if team.is_empty() || monolink_label(&device, &team) != label {
        return Some("This window is for another device.");
    }
    None
}

/// Opens a teammate's servers through Monolink as a team view over the whole Monolab window (its own
/// webview, so [`monolink_guard`] still limits it). One team view at a time. Async: making a webview
/// from a sync command blocks the main thread on Windows.
#[tauri::command]
#[specta::specta]
async fn monolink_open(app: AppHandle, peer: String, name: String, team: String, server: Option<String>) -> Result<(), String> {
    let device = relay_proto::DeviceId::parse(&peer).ok_or("Unknown device.")?;
    if team.is_empty() {
        return Err("Open the server from its team.".into());
    }
    let label = monolink_label(&device, &team);
    let main = app.get_window("main").ok_or("The Monolab window isn't open.")?;
    let _ = main.unminimize();
    let _ = main.show();
    if let Some(open) = app.get_webview(&label) {
        let _ = open.show();
        return open.set_focus().map_err(|error| error.to_string());
    }
    for other in main.webviews().into_iter().filter(|one| one.label().starts_with("monolink-")) {
        let _ = other.close();
    }
    let name = relay_proto::clean_name(&name);
    let mut url = tauri::Url::parse("http://monolab/index.html").map_err(|error| error.to_string())?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("monolink", &device.text()).append_pair("name", &name).append_pair("team", &team);
        if let Some(server) = server.filter(|one| !one.is_empty()) {
            query.append_pair("server", &server);
        }
    }
    let url = format!("index.html?{}", url.query().unwrap_or_default());
    let size: tauri::Size = match main.inner_size() {
        Ok(size) if size.width > 0 && size.height > 0 => size.into(),
        _ => tauri::LogicalSize::new(1600.0, 1000.0).into(),
    };
    let builder = tauri::webview::WebviewBuilder::new(label, tauri::WebviewUrl::App(url.into())).auto_resize();
    let view = main
        .add_child(builder, tauri::PhysicalPosition::new(0, 0), size)
        .map_err(|error| format!("Could not open the team view: {error}"))?;
    let _ = view.set_focus();
    Ok(())
}

/// Closes the team view for `peer` through `team` and goes back to Monolab. The guard lets a team view
/// close only itself.
#[tauri::command]
#[specta::specta]
async fn monolink_close(app: AppHandle, peer: String, team: String) -> Result<(), String> {
    let device = relay_proto::DeviceId::parse(&peer).ok_or("Unknown device.")?;
    if let Some(view) = app.get_webview(&monolink_label(&device, &team)) {
        view.close().map_err(|error| error.to_string())?;
    }
    if let Some(main) = app.get_webview("main") {
        let _ = main.set_focus();
    }
    Ok(())
}

/// Opens the window at 1600 × 1000 only where that fits comfortably: on a smaller or scaled screen
/// (a 1920 × 1080 laptop at 125–150%) it takes about four fifths of the free area, centered.
fn fit_main_to_screen(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let monitor = match window.current_monitor() {
        Ok(Some(one)) => one,
        _ => match window.primary_monitor() {
            Ok(Some(one)) => one,
            _ => return,
        },
    };
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let free_width = f64::from(area.size.width) / scale;
    let free_height = f64::from(area.size.height) / scale;
    let width = (free_width * 0.8).clamp(960.0, 1600.0).min(free_width);
    let height = (free_height * 0.85).clamp(600.0, 1000.0).min(free_height);
    if width >= 1600.0 && height >= 1000.0 {
        return;
    }
    let _ = window.set_size(tauri::LogicalSize::new(width, height));
    let _ = window.center();
}

fn run_due_tasks(app: &AppHandle) {
    let mut list = read_tasks(app);
    if list.is_empty() {
        return;
    }
    let now = unix_now();
    let minutes_today = local_minutes_of_day();
    let weekday = {
        use chrono::Datelike;
        chrono::Local::now().weekday().num_days_from_monday() as u8
    };
    let mut due = Vec::new();
    for task in &mut list {
        if tasks::is_due(task, now, minutes_today, weekday) {
            task.last_run_at = now;
            due.push(task.clone());
        }
    }
    if due.is_empty() {
        return;
    }
    let _ = write_tasks(app, &list);
    for task in due {
        let app = app.clone();
        thread::spawn(move || {
            let state = app.state::<AppState>();
            let result = do_task(&app, &state, &task);
            finish_task(&app, &task, &result);
        });
    }
}

fn finish_task(app: &AppHandle, task: &tasks::Task, result: &Result<String, String>) {
    let (ok, detail) = match result {
        Ok(message) => (true, message.clone()),
        Err(error) => (false, format!("Failed: {error}")),
    };
    record_automation(app, &task.server_id, &task.name, &detail);
    let mut list = read_tasks(app);
    if let Some(saved) = list.iter_mut().find(|one| one.id == task.id) {
        saved.last_ok = ok;
        saved.last_result = detail;
        let _ = write_tasks(app, &list);
    }
}

fn local_minutes_of_day() -> u32 {
    let now = chrono::Local::now();
    use chrono::Timelike;
    now.hour() * 60 + now.minute()
}

fn tasks_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&data_dir).map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(data_dir.join("tasks.json"))
}

fn read_tasks(app: &AppHandle) -> Vec<tasks::Task> {
    tasks_path(app)
        .ok()
        .filter(|path| path.is_file())
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_tasks(app: &AppHandle, list: &[tasks::Task]) -> Result<(), String> {
    store::write(&tasks_path(app)?, &serde_json::to_string_pretty(list).map_err(|error| error.to_string())?)
        .map_err(|error| format!("Could not save the task: {error}"))
}

#[tauri::command(async)]
#[specta::specta]
fn list_tasks(app: AppHandle) -> Result<Vec<tasks::Task>, String> {
    Ok(read_tasks(&app))
}

#[tauri::command(async)]
#[specta::specta]
fn save_task(app: AppHandle, task: tasks::Task) -> Result<Vec<tasks::Task>, String> {
    let mut task = tasks::tidy(task)?;
    let mut list = read_tasks(&app);
    match list.iter_mut().find(|item| item.id == task.id) {
        Some(existing) => {
            task.last_run_at = existing.last_run_at;
            task.last_result = std::mem::take(&mut existing.last_result);
            task.last_ok = existing.last_ok;
            *existing = task;
        }
        None => list.push(task),
    }
    write_tasks(&app, &list)?;
    Ok(list)
}

#[tauri::command(async)]
#[specta::specta]
fn delete_task(app: AppHandle, id: String) -> Result<Vec<tasks::Task>, String> {
    let mut list = read_tasks(&app);
    list.retain(|item| item.id != id);
    write_tasks(&app, &list)?;
    Ok(list)
}

#[tauri::command(async)]
#[specta::specta]
fn run_task_now(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<String, String> {
    let task = read_tasks(&app).into_iter().find(|item| item.id == id).ok_or("Task not found.")?;
    let marks = tasks::warn_marks(&task);
    if !marks.is_empty() && matches!(task.kind.as_str(), "restart" | "stop") {
        let worker = app.clone();
        let minutes = marks[0].div_ceil(60);
        thread::spawn(move || {
            let state = worker.state::<AppState>();
            let result = do_task(&worker, &state, &task);
            finish_task(&worker, &task, &result);
        });
        return Ok(format!("Warning players, then doing it in {minutes} min."));
    }
    let result = do_task(&app, &state, &task);
    finish_task(&app, &task, &result);
    result
}

fn server_on(app: &AppHandle, state: &AppState, id: &str) -> Result<bool, String> {
    let instance = instance_by_id(app, id)?;
    Ok(instance_view(instance, state).status == "running")
}

fn say_to(app: &AppHandle, state: &AppState, id: &str, line: &str) {
    let Ok(instance) = instance_by_id(app, id) else { return };
    if is_far(&instance) {
        let app = app.clone();
        let line = line.to_string();
        let _ = off_runtime(move || {
            tauri::async_runtime::block_on(async {
                if !instance.panel_host.is_empty() {
                    if let Ok((api, target)) = panel_of(&app, &instance) {
                        let _ = api.command(&target, &line).await;
                    }
                } else {
                    let session = session_of(&app.state::<AppState>(), &instance.id);
                    let _ = remote::send_line(&app, &instance.remote_host, &instance.id, &line, session).await;
                }
            });
            Ok::<(), String>(())
        });
    } else {
        let _ = send_process_command(state, id, line);
    }
}

fn warn_players(app: &AppHandle, state: &AppState, task: &tasks::Task) {
    let marks = tasks::warn_marks(task);
    for (index, seconds) in marks.iter().enumerate() {
        let line = tasks::announce_line(&task.warn_style, &tasks::warn_text(task, *seconds));
        say_to(app, state, &task.server_id, &line);
        let next = marks.get(index + 1).copied().unwrap_or(0);
        thread::sleep(Duration::from_secs(u64::from(seconds - next)));
    }
    if !task.final_message.is_empty() {
        say_to(app, state, &task.server_id, &tasks::announce_line(&task.warn_style, &task.final_message));
        thread::sleep(Duration::from_secs(1));
    }
}

fn power_task(app: &AppHandle, state: &AppState, task: &tasks::Task) -> Result<String, String> {
    let on = server_on(app, state, &task.server_id)?;
    match task.kind.as_str() {
        "start" if on => return Ok("Already running, skipped.".into()),
        "restart" | "stop" if !on => return Ok("Skipped because the server is off.".into()),
        _ => {}
    }
    if task.kind != "start" {
        warn_players(app, state, task);
        if !server_on(app, state, &task.server_id)? {
            return Ok("Skipped because the server stopped during the warning.".into());
        }
    }
    let (app_for, id, kind) = (app.clone(), task.server_id.clone(), task.kind.clone());
    off_runtime(move || {
        tauri::async_runtime::block_on(async {
            let state = app_for.state::<AppState>();
            match kind.as_str() {
                "start" => start_instance_as(app_for.clone(), &state, id, false).await,
                "restart" => restart_instance_as(app_for.clone(), &state, id, false).await,
                _ => stop_instance_as(app_for.clone(), &state, id).await,
            }
        })
    })?;
    Ok(match task.kind.as_str() {
        "start" => "Started the server.".to_string(),
        "restart" => "Restarted the server.".to_string(),
        _ => "Stopped the server.".to_string(),
    })
}

fn do_task(app: &AppHandle, state: &AppState, task: &tasks::Task) -> Result<String, String> {
    if matches!(task.kind.as_str(), "restart" | "start" | "stop") {
        return power_task(app, state, task);
    }
    if task.kind == "update" {
        let (app, id) = (app.clone(), task.server_id.clone());
        return off_runtime(move || tauri::async_runtime::block_on(update_task(&app, &id)));
    }
    if task.kind != "notify" {
        if let Ok(instance) = instance_by_id(app, &task.server_id) {
            if is_far(&instance) {
                let (app, task) = (app.clone(), task.clone());
                return off_runtime(move || tauri::async_runtime::block_on(far_task(&app, &instance, &task)));
            }
        }
    }
    match task.kind.as_str() {
        "command" => {
            let done = send_process_command(state, &task.server_id, &task.command)?;
            if done {
                Ok(format!("Command sent: {}", task.command))
            } else {
                Ok("Skipped because the server is off.".into())
            }
        }
        "notify" => {
            announce(app, &task.server_id, "Task", &task.message)?;
            Ok(format!("Notified: {}", task.message))
        }
        "backup" => backup_for_task(app, state, &task.server_id),
        _ => Err("Unknown task type.".into()),
    }
}

fn backup_for_task(app: &AppHandle, state: &AppState, id: &str) -> Result<String, String> {
    let instance = instance_by_id(app, id)?;
    let running = send_process_command(state, id, "save-off").unwrap_or(false);
    if running {
        let _ = send_process_command(state, id, "save-all flush");
        thread::sleep(Duration::from_secs(5));
    }

    let folder = instance.path.clone();
    let set = read_app_settings(app);
    let vault = backup_folder(app, &instance)?;
    let worlds = set.backup_worlds_only;
    let made = {
        let vault = vault.clone();
        off_runtime(move || backups::create(Path::new(&folder), "Auto", &vault, worlds))
    };

    if running {
        let _ = send_process_command(state, id, "save-on");
    }

    let made = made?;
    record_audit(app, id, "Server", "Scheduled backup", &made.file_name);
    cloud::copy_after_backup(app, vault.join(&made.file_name), &instance.name);
    let (keep, count) = (set.keep_backup_days, set.keep_auto_backups);
    off_runtime(move || backups::sweep(&vault, keep, count));
    Ok(if running { format!("Paused saving and backed up: {}", made.file_name) } else { format!("Backed up: {}", made.file_name) })
}

/// A scheduled update: every plugin and mod with a newer version that has no known problem (those are left for
/// the user to look at). On a running server the files are downloaded now and swapped in on the next start.
async fn update_task(app: &AppHandle, id: &str) -> Result<String, String> {
    let updates = check_addon_updates(app.clone(), id.to_string()).await?;
    let (safe, risky): (Vec<_>, Vec<_>) = updates.into_iter().partition(|one| one.issues.is_empty());
    let skipped = if risky.is_empty() {
        String::new()
    } else {
        format!(" Skipped {} that may not work with this server; update those by hand.", risky.len())
    };
    if safe.is_empty() {
        return Ok(format!("Everything is up to date.{skipped}"));
    }
    let names: Vec<String> = safe.iter().map(|one| one.file_name.clone()).collect();
    let result = apply_addon_updates(app.clone(), app.state::<AppState>(), id.to_string(), names.clone()).await?;
    record_audit(app, id, "Plugins", "Scheduled update", &names.join(", "));
    Ok(if result.queued > 0 {
        format!("Downloaded {} update(s); they apply on the next start.{skipped}", result.queued)
    } else {
        format!("Updated {}.{skipped}", result.applied)
    })
}

async fn far_task(app: &AppHandle, instance: &ServerInstance, task: &tasks::Task) -> Result<String, String> {
    match task.kind.as_str() {
        "command" => {
            let line = launch::console_line(&task.command)?;
            if !instance.panel_host.is_empty() {
                let (api, target) = panel_of(app, instance)?;
                api.command(&target, &line).await?;
            } else {
                let session = session_of(&app.state::<AppState>(), &instance.id);
                if let Err(error) = remote::send_line(app, &instance.remote_host, &instance.id, &line, session).await {
                    if error.contains("isn't running") {
                        return Ok("Skipped because the server is off.".into());
                    }
                    return Err(error);
                }
            }
            Ok(format!("Command sent: {}", task.command))
        }
        "backup" => {
            if !instance.panel_host.is_empty() {
                let (api, target) = panel_of(app, instance)?;
                let name = format!("Monolab auto {}", unix_now());
                api.create_backup(&target, &name).await?;
                record_audit(app, &instance.id, "Server", "Automatic backup (panel)", &name);
                return Ok(format!("Asked the panel to back up: {name}"));
            }
            let name = remote_backup(app, instance, "auto", read_app_settings(app).backup_worlds_only, &[]).await?;
            record_audit(app, &instance.id, "Server", "Automatic backup (SSH)", &name);
            let keep = read_app_settings(app).keep_backup_days;
            let _ = remote::sweep_backups(app, &instance.remote_host, &instance.path, keep).await;
            Ok(format!("Backed up: {name}"))
        }
        _ => Err("Unknown task type.".into()),
    }
}

async fn remote_backup(
    app: &AppHandle,
    instance: &ServerInstance,
    label: &str,
    worlds_only: bool,
    picked: &[String],
) -> Result<String, String> {
    let mut only = picked.to_vec();
    if worlds_only {
        for (name, dir, _) in server_names(app, instance, "").await {
            if !dir || name.starts_with('.') {
                continue;
            }
            let world = name.ends_with("_nether")
                || name.ends_with("_the_end")
                || server_names(app, instance, &name).await.iter().any(|(one, dir, _)| !dir && one == "level.dat");
            if world {
                only.push(name);
            }
        }
        if only.is_empty() {
            return Err("No world folder to back up. Start the server once.".into());
        }
    }
    let label = if worlds_only {
        format!("{label}_worlds_only")
    } else if !picked.is_empty() {
        format!("{label}_picked")
    } else {
        label.to_string()
    };
    let session = session_of(&app.state::<AppState>(), &instance.id);
    let mut paused = false;
    if let Some(name) = &session {
        paused = remote::send_line(app, &instance.remote_host, &instance.id, "save-off", Some(name.clone())).await.is_ok();
        if paused {
            let _ = remote::send_line(app, &instance.remote_host, &instance.id, "save-all flush", Some(name.clone())).await;
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
    let made = remote::make_backup(app, &instance.remote_host, &instance.path, &label, &only).await;
    if paused {
        let _ = remote::send_line(app, &instance.remote_host, &instance.id, "save-on", session.clone()).await;
    }
    made
}

fn backup_root(app: &AppHandle) -> Option<PathBuf> {
    let set = read_app_settings(app).backup_root;
    let path = PathBuf::from(set.trim());
    (path.is_dir()).then_some(path)
}

pub(crate) fn note_run(app: &AppHandle, id: &str, kind: &str, detail: &str) {
    if let Ok(folder) = instance_data_dir(app, id) {
        runlog::record(&folder, kind, detail);
    }
}

fn ran_minutes(process: &ManagedProcess) -> u64 {
    process.started_at.elapsed().map(|one| one.as_secs() / 60).unwrap_or(0)
}

#[tauri::command(async)]
#[specta::specta]
fn list_runs(app: AppHandle, id: String) -> Result<Vec<runlog::RunEntry>, String> {
    Ok(runlog::list(&instance_data_dir(&app, &id)?))
}

/// The user's notes and todos for one server, as JSON text (empty when there are none yet).
#[tauri::command(async)]
#[specta::specta]
fn server_memo(app: AppHandle, id: String) -> Result<String, String> {
    instance_by_id(&app, &id)?;
    let file = instance_data_dir(&app, &id)?.join("memo.json");
    Ok(store::read::<serde_json::Value>(&file)?.map(|one| one.to_string()).unwrap_or_default())
}

#[tauri::command(async)]
#[specta::specta]
fn set_server_memo(app: AppHandle, id: String, memo: String, expect_hash: Option<String>) -> Result<String, String> {
    instance_by_id(&app, &id)?;
    if memo.len() > 2 * 1024 * 1024 || serde_json::from_str::<serde_json::Value>(&memo).map(|one| !one.is_object()).unwrap_or(true) {
        return Err("Could not save the memo. It is too large or damaged.".into());
    }
    let file = instance_data_dir(&app, &id)?.join("memo.json");
    // F3: refuse when someone changed the memo after this window read it; the window merges and saves again.
    if let Some(want) = expect_hash.as_deref().filter(|one| !one.is_empty()) {
        let now = store::read::<serde_json::Value>(&file)?.map(|one| one.to_string()).unwrap_or_default();
        if saved_since(Some(&now), Some(want)) {
            return Err("Someone else saved this memo after you opened it.".into());
        }
    }
    let before: serde_json::Value = store::read(&file)?.unwrap_or_default();
    store::write(&file, &memo).map_err(|error| format!("Could not save the memo: {error}"))?;
    memo_changed(&app, &id);
    if let Ok(after) = serde_json::from_str::<serde_json::Value>(&memo) {
        for (who, text) in team_notice::newly_assigned(&before, &after) {
            team_notice::send(&app, &id, team_notice::NoticeKind::Assigned, Some(&who), &text);
        }
    }
    // The hash of the memo as `server_memo` will give it back, for the next save.
    Ok(store::read::<serde_json::Value>(&file)?.map(|one| studio::hash(&one.to_string())).unwrap_or_default())
}

/// One scan or restore at a time, so the watcher and a click never write the same history together.
static REWIND_LOCK: Mutex<()> = Mutex::new(());

/// The Rewind history of a server on this PC, and its folder.
fn rewind_store(app: &AppHandle, id: &str) -> Result<(rewind::Store, PathBuf), String> {
    let instance = instance_by_id(app, id)?;
    if !instance.remote_host.is_empty() || !instance.panel_host.is_empty() {
        return Err("Rewind keeps history for servers on this PC only.".into());
    }
    let root = Path::new(&instance.path).canonicalize().map(plain_path).map_err(|_| "The server folder is missing.".to_string())?;
    Ok((rewind::Store::open(instance_data_dir(app, id)?.join("rewind")), root))
}

fn rewind_off_marker(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(instance_data_dir(app, id)?.join("rewind-off"))
}

/// Looks for changed files now and returns how many changes it found.
#[tauri::command(async)]
#[specta::specta]
fn rewind_scan(app: AppHandle, id: String) -> Result<u32, String> {
    let (store, root) = rewind_store(&app, &id)?;
    if rewind_off_marker(&app, &id)?.exists() {
        return Ok(0);
    }
    let _held = REWIND_LOCK.lock().expect("rewind lock poisoned");
    Ok(store.scan(&root, &|path, at| authorship::who_changed(&id, path, at))?.len() as u32)
}

/// Newest changes first; only one file's when `path` is given.
#[tauri::command(async)]
#[specta::specta]
fn rewind_list(app: AppHandle, id: String, path: Option<String>, limit: u32) -> Result<Vec<rewind::RewindEntry>, String> {
    let (store, _) = rewind_store(&app, &id)?;
    Ok(store.list(path.as_deref(), limit.clamp(1, rewind::KEEP as u32) as usize))
}

#[tauri::command(async)]
#[specta::specta]
fn rewind_versions(app: AppHandle, id: String, entry: String) -> Result<rewind::RewindVersions, String> {
    let (store, root) = rewind_store(&app, &id)?;
    store.versions(&root, &entry)
}

/// Puts a file back as it was before or after a change. `side` is "before" or "after".
#[tauri::command(async)]
#[specta::specta]
fn rewind_restore(app: AppHandle, id: String, entry: String, side: String) -> Result<rewind::RewindEntry, String> {
    let (store, root) = rewind_store(&app, &id)?;
    let _held = REWIND_LOCK.lock().expect("rewind lock poisoned");
    // Catch up first, so edits made since the last look are in the history before they are overwritten.
    store.scan(&root, &|path, at| authorship::who_changed(&id, path, at))?;
    let done = store.restore(&root, &entry, &side)?;
    record_audit(&app, &id, "Files", "Rewind", &format!("Restored {} ({side} a change).", done.path));
    Ok(done)
}

#[tauri::command(async)]
#[specta::specta]
fn rewind_enabled(app: AppHandle, id: String) -> Result<bool, String> {
    instance_by_id(&app, &id)?;
    Ok(!rewind_off_marker(&app, &id)?.exists())
}

#[tauri::command(async)]
#[specta::specta]
fn set_rewind_enabled(app: AppHandle, id: String, on: bool) -> Result<(), String> {
    instance_by_id(&app, &id)?;
    let marker = rewind_off_marker(&app, &id)?;
    if on {
        let _ = fs::remove_file(&marker);
        Ok(())
    } else {
        fs::write(&marker, b"").map_err(|error| error.to_string())
    }
}

/// Looks at every local server's text files every few seconds, whether or not the server runs.
fn start_rewind_watcher(app: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(6));
        let Ok(instances) = load_instances(&app) else { continue };
        for instance in instances.iter().filter(|one| one.remote_host.is_empty() && one.panel_host.is_empty()) {
            if rewind_off_marker(&app, &instance.id).map(|one| one.exists()).unwrap_or(true) {
                continue;
            }
            let Ok((store, root)) = rewind_store(&app, &instance.id) else { continue };
            let found = {
                let _held = REWIND_LOCK.lock().expect("rewind lock poisoned");
                store.scan(&root, &|path, at| authorship::who_changed(&instance.id, path, at))
            };
            if let Ok(found) = found {
                if !found.is_empty() {
                    let _ = app.emit("rewind-changed", serde_json::json!({ "serverId": instance.id, "count": found.len() }));
                }
            }
        }
    });
}

/// The Memo tab as plain text for an agent, with the paths of attached pictures so it can open them.
pub(crate) fn memo_for_agent(app: &AppHandle, id: &str) -> Result<String, String> {
    let dir = instance_data_dir(app, id)?;
    let doc: serde_json::Value = store::read(&dir.join("memo.json"))?.unwrap_or_default();
    Ok(memo::as_text(&doc, &dir.join("memo-images")))
}

/// Adds a to-do or marks one done/open for an agent; the open Memo tab reloads on `memo-changed`.
pub(crate) fn update_todo(app: &AppHandle, id: &str, change: &memo::TodoChange) -> Result<String, String> {
    let file = instance_data_dir(app, id)?.join("memo.json");
    let mut doc: serde_json::Value = store::read(&file)?.unwrap_or_default();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_millis()).unwrap_or(0);
    let said = memo::apply_todo(&mut doc, change, stamp)?;
    store::write(&file, &doc.to_string()).map_err(|error| format!("Could not save the memo: {error}"))?;
    memo_changed(app, id);
    if change.action == "assign" {
        if let Some(who) = change.assignee.filter(|one| !one.is_empty()) {
            let text = change
                .todo
                .and_then(|wanted| {
                    doc["todos"].as_array()?.iter().find(|one| one["id"].as_str() == Some(wanted))?["text"].as_str().map(str::to_string)
                })
                .unwrap_or_default();
            team_notice::send(app, id, team_notice::NoticeKind::Assigned, Some(who), &text);
        }
    }
    Ok(said)
}

/// The Memo changed: the host's window reloads it, and so do teammates' windows on that server (F3).
pub(crate) fn memo_changed(app: &AppHandle, id: &str) {
    let _ = app.emit("memo-changed", id.to_string());
    remember_for_web(app, "memo-changed", &serde_json::json!({ "serverId": id }));
}

// ── F6: team Storage (host side) ─────────────────────────────────────────

fn team_storage_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("team-storage"))
}

pub(crate) fn team_storage_of(app: &AppHandle, team: &str) -> Result<team_storage::Storage, String> {
    team_storage::Storage::open(&team_storage_root(app)?, team)
}

/// Puts a Storage file into a server folder (`folder` inside it). Gives the path it went to.
pub(crate) async fn storage_into_server(
    app: &AppHandle,
    team: &str,
    item: &str,
    version: u32,
    server: &str,
    folder: &str,
) -> Result<String, String> {
    let storage = team_storage_of(app, team)?;
    let name =
        storage.index().items.into_iter().find(|one| one.id == item).map(|one| one.name).ok_or("There is no such file in Storage.")?;
    let source = storage.file(item, version)?;
    let target = team_storage::target_in_server(folder, &name)?;
    let instance = instance_by_id(app, server)?;
    if is_far(&instance) {
        let size = fs::metadata(&source).map(|one| one.len()).unwrap_or(0);
        if size > 256 * 1024 * 1024 {
            return Err("Files over 256 MB go into SSH and panel servers from the Files tab.".into());
        }
        let bytes = fs::read(&source).map_err(|error| format!("Could not read the file: {error}"))?;
        server_write_bytes(app, &instance, &target, &bytes).await?;
    } else {
        let root = Path::new(&instance.path);
        if let Some(parent) = Path::new(&target).parent().filter(|one| !one.as_os_str().is_empty()) {
            fs::create_dir_all(root.join(parent)).map_err(|error| format!("Could not make the folder: {error}"))?;
        }
        let path = paths::resolve_new_in_server(root, &target).map_err(|_| "Pick a folder inside the server.".to_string())?;
        fs::copy(&source, path).map_err(|error| format!("Could not copy the file: {error}"))?;
    }
    monolink_team::report(app, server, "storage_used", serde_json::json!({ "name": name }));
    Ok(target)
}

/// Keeps a file of a server in Storage (a new item, or a new version of the same name).
pub(crate) async fn storage_from_server(
    app: &AppHandle,
    team: &str,
    server: &str,
    path: &str,
    by: &str,
) -> Result<team_storage::StorageItem, String> {
    let instance = instance_by_id(app, server)?;
    let name = path.replace('\\', "/").rsplit('/').next().unwrap_or_default().to_string();
    let storage = team_storage_of(app, team)?;
    let item = if is_far(&instance) {
        let bytes = server_bytes(app, &instance, path, 256 * 1024 * 1024).await?;
        let temp = std::env::temp_dir().join(format!("monolab-storage-{}", unix_now()));
        fs::write(&temp, bytes).map_err(|error| error.to_string())?;
        let done = storage.put(&temp, &name, "", &[], by);
        let _ = fs::remove_file(&temp);
        done?
    } else {
        let source =
            paths::resolve_in_server(Path::new(&instance.path), path).map_err(|_| "There is no such file in the server.".to_string())?;
        storage.put(&source, &name, "", &[], by)?
    };
    monolink_team::report(app, server, "storage_added", serde_json::json!({ "name": item.name }));
    Ok(item)
}

#[tauri::command(async)]
#[specta::specta]
fn storage_view(app: AppHandle, team: String) -> Result<team_storage::StorageView, String> {
    Ok(team_storage_of(&app, &team)?.view())
}

/// Adds a file on this PC to the team's Storage (the host's own window).
#[tauri::command(async)]
#[specta::specta]
fn storage_put_file(
    app: AppHandle,
    team: String,
    path: String,
    name: String,
    note: String,
    tags: Vec<String>,
) -> Result<team_storage::StorageItem, String> {
    team_storage_of(&app, &team)?.put(Path::new(&path), &name, &note, &tags, "host")
}

#[tauri::command(async)]
#[specta::specta]
fn storage_trash(app: AppHandle, team: String, item: String, trashed: bool) -> Result<(), String> {
    team_storage_of(&app, &team)?.set_trashed(&item, trashed)
}

#[tauri::command(async)]
#[specta::specta]
fn storage_empty_trash(app: AppHandle, team: String) -> Result<u32, String> {
    team_storage_of(&app, &team)?.empty_trash()
}

#[tauri::command(async)]
#[specta::specta]
fn storage_set_limit(app: AppHandle, team: String, gb: u32) -> Result<(), String> {
    team_storage_of(&app, &team)?.set_limit(u64::from(gb))
}

#[tauri::command(async)]
#[specta::specta]
async fn storage_to_server(app: AppHandle, team: String, item: String, version: u32, id: String, folder: String) -> Result<String, String> {
    storage_into_server(&app, &team, &item, version, &id, &folder).await
}

#[tauri::command(async)]
#[specta::specta]
async fn storage_put_from_server(app: AppHandle, team: String, id: String, path: String) -> Result<team_storage::StorageItem, String> {
    storage_from_server(&app, &team, &id, &path, "host").await
}

/// Saves a copy of a Storage file somewhere on this PC (the host's own window).
#[tauri::command(async)]
#[specta::specta]
fn storage_save_copy(app: AppHandle, team: String, item: String, version: u32, target: String) -> Result<(), String> {
    let source = team_storage_of(&app, &team)?.file(&item, version)?;
    fs::copy(source, target).map(|_| ()).map_err(|error| format!("Could not save the copy: {error}"))
}

// ── F8: change proposals ─────────────────────────────────────────────────

fn proposals_store(app: &AppHandle) -> Result<proposals::Proposals, String> {
    proposals::Proposals::open(&app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("proposals"))
}

pub(crate) fn proposals_of(app: &AppHandle, server: &str) -> Result<Vec<proposals::Proposal>, String> {
    Ok(proposals_store(app)?.list(server))
}

/// SHA-256 of a server file's bytes now, None when there is none.
async fn file_hash_now(app: &AppHandle, instance: &ServerInstance, path: &str) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = server_bytes(app, instance, path, 64 * 1024 * 1024).await.ok()?;
    Some(Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect())
}

async fn hashes_now(app: &AppHandle, instance: &ServerInstance, command: &str, body: &serde_json::Value) -> Vec<proposals::FileBefore> {
    let root = studio_root(instance);
    let mut out = Vec::new();
    for path in authorship::paths_of(command, body) {
        let path = authorship::with_studio_root(&path, root);
        out.push(proposals::FileBefore { hash: file_hash_now(app, instance, &path).await, path });
    }
    out
}

/// Holds a teammate's change for review (F8) and tells the server's people.
pub(crate) async fn propose(
    app: &AppHandle,
    server: &str,
    by: &str,
    command: &str,
    body: &serde_json::Value,
) -> Result<proposals::Proposal, String> {
    let instance = instance_by_id(app, server)?;
    let before = hashes_now(app, &instance, command, body).await;
    let upload = if command == "studio_asset_add_upload" {
        Some(PathBuf::from(remote_upload_path(app, body.get("upload").and_then(serde_json::Value::as_str).unwrap_or_default())?))
    } else {
        None
    };
    let mut kept = body.clone();
    if let Some(map) = kept.as_object_mut() {
        map.remove("expectHash");
    }
    let proposal = proposals::Proposal {
        id: format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|time| time.as_millis()).unwrap_or(0)),
        server: server.to_string(),
        by: by.to_string(),
        at: unix_now(),
        command: command.to_string(),
        body: kept,
        before,
        ..Default::default()
    };
    let done = proposals_store(app)?.add(proposal, upload.as_deref())?;
    let paths: Vec<String> = done.before.iter().map(|one| one.path.clone()).collect();
    team_notice::send(app, server, team_notice::NoticeKind::ProposalNew, None, &paths.join(", "));
    Ok(done)
}

/// Runs a proposal as the host's own change, unless a file it touches changed since (F8).
pub(crate) async fn merge_proposal(app: &AppHandle, server: &str, id: &str, by: &str) -> Result<(), String> {
    use base64::Engine;
    let store = proposals_store(app)?;
    let proposal = store.get(server, id)?;
    if proposal.state != "open" {
        return Err("This proposal was already answered.".into());
    }
    let instance = instance_by_id(app, server)?;
    let mut now = Vec::new();
    for one in &proposal.before {
        now.push((one.path.clone(), file_hash_now(app, &instance, &one.path).await));
    }
    if proposals::moved_on(&proposal.before, &|path| now.iter().find(|(one, _)| one == path).and_then(|(_, hash)| hash.clone())) {
        return Err("Conflicts with a newer change. Reject it, or ask for it again.".into());
    }
    let (command, mut body) = (proposal.command.clone(), proposal.body.clone());
    let command = if command == "studio_asset_add_upload" {
        let file = proposal.file.clone().ok_or("The proposal's file is gone.")?;
        let bytes = fs::read(&file).map_err(|error| format!("Could not read the proposal's file: {error}"))?;
        if let Some(map) = body.as_object_mut() {
            map.remove("upload");
            map.insert("data".into(), serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(bytes)));
        }
        "studio_asset_add".to_string()
    } else {
        command
    };
    let run = {
        let app = app.clone();
        let command = command.clone();
        let body = body.clone();
        tauri::async_runtime::spawn_blocking(move || web_invoke(&app, &command, &body)).await.map_err(|error| error.to_string())?
    };
    run?;
    for one in &proposal.before {
        authorship::note(server, &one.path, &proposal.by);
    }
    store.close(server, id, "merged", "")?;
    team_notice::send(
        app,
        server,
        team_notice::NoticeKind::ProposalDone,
        Some(proposal.by.trim_end_matches("'s AI")),
        &format!("{by} merged your change to {}.", proposal.before.iter().map(|one| one.path.as_str()).collect::<Vec<_>>().join(", ")),
    );
    Ok(())
}

pub(crate) fn reject_proposal(app: &AppHandle, server: &str, id: &str, reason: &str) -> Result<(), String> {
    let done = proposals_store(app)?.close(server, id, "rejected", reason)?;
    let paths = done.before.iter().map(|one| one.path.as_str()).collect::<Vec<_>>().join(", ");
    let said = if reason.trim().is_empty() {
        format!("Your change to {paths} was not taken.")
    } else {
        format!("Your change to {paths} was not taken: {}", reason.trim())
    };
    team_notice::send(app, server, team_notice::NoticeKind::ProposalDone, Some(done.by.trim_end_matches("'s AI")), &said);
    Ok(())
}

/// What a text proposal changes: the file now and as proposed (for the compare view).
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProposalTexts {
    pub path: String,
    pub now: Option<String>,
    pub proposed: Option<String>,
}

pub(crate) async fn proposal_texts_of(app: &AppHandle, server: &str, id: &str) -> Result<ProposalTexts, String> {
    let proposal = proposals_store(app)?.get(server, id)?;
    let instance = instance_by_id(app, server)?;
    let path = proposal.before.first().map(|one| one.path.clone()).unwrap_or_default();
    let now = if path.is_empty() { None } else { server_text(app, &instance, &path).await.ok().flatten() };
    let text = |key: &str| proposal.body.get(key).and_then(serde_json::Value::as_str).map(str::to_string);
    // (Not a `match` on command names: the web tests read "<command>" => arms in this file as web_invoke's.)
    let command = proposal.command.as_str();
    let proposed = if command == "save_editable_file" {
        text("content")
    } else if command == "studio_save" {
        text("text")
    } else if command == "create_server_file" {
        Some(String::new())
    } else {
        None
    };
    Ok(ProposalTexts { path, now, proposed })
}

#[tauri::command(async)]
#[specta::specta]
fn proposals_list(app: AppHandle, id: String) -> Result<Vec<proposals::Proposal>, String> {
    proposals_of(&app, &id)
}

#[tauri::command(async)]
#[specta::specta]
async fn proposal_merge(app: AppHandle, id: String, proposal: String) -> Result<(), String> {
    merge_proposal(&app, &id, &proposal, "The host").await
}

#[tauri::command(async)]
#[specta::specta]
fn proposal_reject(app: AppHandle, id: String, proposal: String, reason: String) -> Result<(), String> {
    reject_proposal(&app, &id, &proposal, &reason)
}

#[tauri::command(async)]
#[specta::specta]
async fn proposal_texts(app: AppHandle, id: String, proposal: String) -> Result<ProposalTexts, String> {
    proposal_texts_of(&app, &id, &proposal).await
}

/// F3: the teammates a to-do on this server can be given to (`@handle`), empty when the server isn't shared.
#[tauri::command(async)]
#[specta::specta]
async fn memo_people(app: AppHandle, id: String) -> Vec<String> {
    monolink_team::team_people(&app, &id).await
}

/// The Memo as an agent reads it, for a teammate's agent on a shared server (F3; this PC's agent reads it directly).
#[tauri::command(async)]
#[specta::specta]
fn agent_read_memo(app: AppHandle, id: String) -> Result<String, String> {
    instance_by_id(&app, &id)?;
    memo_for_agent(&app, &id)
}

/// `update_todo` for a teammate's agent on a shared server.
#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
fn agent_update_todo(
    app: AppHandle,
    id: String,
    action: String,
    todo: Option<String>,
    text: Option<String>,
    status: Option<String>,
    assignee: Option<String>,
) -> Result<String, String> {
    instance_by_id(&app, &id)?;
    update_todo(
        &app,
        &id,
        &memo::TodoChange {
            action: &action,
            todo: todo.as_deref(),
            words: text.as_deref(),
            status: status.as_deref(),
            assignee: assignee.as_deref(),
            by: "AI",
        },
    )
}

/// A picture kept with a server's memo: a file name the app made, so it never points outside the memo folder.
fn memo_image_path(app: &AppHandle, id: &str, name: &str) -> Result<PathBuf, String> {
    instance_by_id(app, id)?;
    let fine = !name.is_empty()
        && name.len() < 80
        && !name.starts_with('.')
        && name.chars().all(|one| one.is_ascii_alphanumeric() || one == '-' || one == '.');
    if !fine || !matches!(image_kind(name), Some("image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/bmp")) {
        return Err("This isn't a memo picture.".into());
    }
    Ok(instance_data_dir(app, id)?.join("memo-images").join(name))
}

/// Saves a picture for the memo and returns its new file name.
#[tauri::command(async)]
#[specta::specta]
fn add_memo_image(app: AppHandle, id: String, extension: String, data: String) -> Result<String, String> {
    use base64::Engine;
    let extension = extension.trim_start_matches('.').to_ascii_lowercase();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_millis()).unwrap_or(0);
    let mut name = format!("{stamp}.{extension}");
    let mut path = memo_image_path(&app, &id, &name)?;
    let mut n = 2;
    while path.exists() {
        name = format!("{stamp}-{n}.{extension}");
        path = memo_image_path(&app, &id, &name)?;
        n += 1;
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|_| "Could not read the picture.".to_string())?;
    if bytes.len() > 20 * 1024 * 1024 {
        return Err("This picture is too large (20 MB max).".into());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("Could not save the picture: {error}"))?;
    }
    fs::write(&path, bytes).map_err(|error| format!("Could not save the picture: {error}"))?;
    Ok(name)
}

#[tauri::command(async)]
#[specta::specta]
fn memo_image(app: AppHandle, id: String, name: String) -> Result<String, String> {
    use base64::Engine;
    let path = memo_image_path(&app, &id, &name)?;
    let bytes = fs::read(&path).map_err(|error| format!("Could not read the picture: {error}"))?;
    Ok(format!("data:{};base64,{}", image_kind(&name).unwrap_or("image/png"), base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[tauri::command(async)]
#[specta::specta]
fn delete_memo_image(app: AppHandle, id: String, name: String) -> Result<(), String> {
    let path = memo_image_path(&app, &id, &name)?;
    match fs::remove_file(&path) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(format!("Could not delete the picture: {error}")),
        _ => Ok(()),
    }
}

fn instance_data_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let safe: String =
        id.chars().map(|letter| if letter.is_ascii_alphanumeric() || letter == '-' || letter == '_' { letter } else { '_' }).collect();
    if safe.is_empty() {
        return Err("The server number is malformed.".into());
    }
    let path = app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("server-data").join(safe);
    fs::create_dir_all(&path).map_err(|error| format!("Could not create the server storage folder: {error}"))?;
    Ok(path)
}

fn backup_folder(app: &AppHandle, instance: &ServerInstance) -> Result<PathBuf, String> {
    if let Some(root) = backup_root(app) {
        let name =
            Path::new(&instance.path).file_name().map(|one| one.to_string_lossy().to_string()).unwrap_or_else(|| instance.id.clone());
        return Ok(root.join(name));
    }
    Ok(instance_data_dir(app, &instance.id)?.join("backups"))
}

fn pending_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(instance_data_dir(app, id)?.join("pending"))
}

fn upgrade_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let path = instance_data_dir(app, id)?.join("upgrade");
    fs::create_dir_all(&path).map_err(|error| format!("Could not create the upgrade storage folder: {error}"))?;
    Ok(path)
}

fn addon_trash_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let path = instance_data_dir(app, id)?.join("trash");
    fs::create_dir_all(&path).map_err(|error| format!("Could not create the trash folder: {error}"))?;
    Ok(path)
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::copy(from, to)?;
    fs::remove_file(from)
}

fn move_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if !to.exists() && fs::rename(from, to).is_ok() {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            move_tree(&entry.path(), &target)?;
        } else if target.exists() {
            let _ = fs::remove_file(entry.path());
        } else {
            move_file(&entry.path(), &target)?;
        }
    }
    fs::remove_dir_all(from)
}

#[tauri::command(async)]
#[specta::specta]
async fn create_backup(
    app: AppHandle,
    id: String,
    note: String,
    worlds_only: bool,
    paths: Vec<String>,
) -> Result<backups::BackupFile, String> {
    let instance = instance_by_id(&app, &id)?;
    let picks = if paths.is_empty() { Vec::new() } else { backups::clean_picks(&paths)? };
    if !instance.panel_host.is_empty() {
        if worlds_only || !picks.is_empty() {
            return Err("Panel backups always include the whole server.".into());
        }
        let (api, target) = panel_of(&app, &instance)?;
        let name = if note.trim().is_empty() { format!("Monolab {}", unix_now()) } else { note.trim().to_string() };
        api.create_backup(&target, &name).await?;
        record_audit(&app, &id, "Backup", "Create backup (panel)", &name);
        return Ok(backups::BackupFile {
            file_name: String::new(),
            size_mib: 0.0,
            created_at: unix_now(),
            note: format!("{name} · in progress"),
            worlds_only: false,
        });
    }
    if !instance.remote_host.is_empty() {
        tell_job(&app, "backup", &instance.name, "taring", 0, 0);
        let made = remote_backup(&app, &instance, "manual", worlds_only, &picks).await;
        tell_job(&app, "backup", &instance.name, "done", 0, 0);
        let name = made?;
        record_audit(&app, &id, "Backup", "Create backup (SSH)", &name);
        let made = remote::backups(&app, &instance.remote_host, &instance.path).await.into_iter().find(|one| one.name == name);
        return Ok(backups::BackupFile {
            size_mib: made.as_ref().map(|one| one.size_bytes as f64 / 1_048_576.0).unwrap_or(0.0),
            created_at: made.as_ref().map(|one| one.modified_at).unwrap_or(0),
            file_name: name,
            note: if note.trim().is_empty() { "Manual".into() } else { note.trim().into() },
            worlds_only,
        });
    }
    let note = if note.trim().is_empty() { "Manual".to_string() } else { note.trim().to_string() };
    let vault = backup_folder(&app, &instance)?;
    let task = progress::begin(&app, "createBackup", &id);
    let made = if picks.is_empty() {
        off_runtime(|| backups::create_followed(Path::new(&instance.path), &note, &vault, worlds_only, &task))?
    } else {
        off_runtime(|| backups::create_picked(Path::new(&instance.path), &note, &vault, &picks, &task))?
    };
    record_audit(&app, &id, "Server", "Backup", &made.file_name);
    cloud::copy_after_backup(&app, vault.join(&made.file_name), &instance.name);
    let set = read_app_settings(&app);
    let (keep, count) = (set.keep_backup_days, set.keep_auto_backups);
    off_runtime(move || backups::sweep(&vault, keep, count));
    Ok(made)
}

#[tauri::command(async)]
#[specta::specta]
async fn list_backups(app: AppHandle, id: String) -> Result<Vec<backups::BackupFile>, String> {
    {
        let instance = instance_by_id(&app, &id)?;
        if !instance.panel_host.is_empty() {
            let (api, target) = panel_of(&app, &instance)?;
            let mut rows = api.backups(&target).await?;
            rows.sort_by_key(|one| std::cmp::Reverse(one.created_at));
            return Ok(rows
                .into_iter()
                .map(|one| backups::BackupFile {
                    size_mib: one.bytes / 1_048_576.0,
                    created_at: one.created_at,
                    note: if !one.completed {
                        format!("{} · in progress", one.name)
                    } else if !one.successful {
                        format!("{} · failed", one.name)
                    } else if one.locked {
                        format!("{} · locked", one.name)
                    } else {
                        one.name
                    },
                    file_name: one.uuid,
                    worlds_only: false,
                })
                .collect());
        }
        if !instance.remote_host.is_empty() {
            return Ok(remote::backups(&app, &instance.remote_host, &instance.path)
                .await
                .into_iter()
                .map(|one| backups::BackupFile {
                    size_mib: one.size_bytes as f64 / 1_048_576.0,
                    created_at: one.modified_at,
                    file_name: one.name,
                    note: "SSH".into(),
                    worlds_only: false,
                })
                .collect());
        }
    }
    let instance = instance_by_id(&app, &id)?;
    let vault = backup_folder(&app, &instance)?;
    Ok(off_runtime(|| backups::list(&vault)))
}

#[tauri::command(async)]
#[specta::specta]
async fn restore_backup(app: AppHandle, state: State<'_, AppState>, id: String, file_name: String) -> Result<backups::BackupFile, String> {
    {
        let instance = instance_by_id(&app, &id)?;
        if !instance.panel_host.is_empty() {
            ensure_panel_stopped(&app, &instance).await?;
            let (api, target) = panel_of(&app, &instance)?;
            let safety = format!("Before restore {}", unix_now());
            tell_job(&app, "backup", &instance.name, "taring", 0, 0);
            let made = async {
                api.create_backup(&target, &safety).await?;
                let began = std::time::Instant::now();
                loop {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    let rows = api.backups(&target).await?;
                    if let Some(one) = rows.iter().find(|one| one.name == safety && one.completed) {
                        if !one.successful {
                            return Err("Stopped: the pre-restore backup failed.".to_string());
                        }
                        return Ok(one.clone());
                    }
                    if began.elapsed() > Duration::from_secs(600) {
                        return Err("Stopped: the pre-restore backup didn't finish within 10 minutes.".to_string());
                    }
                }
            }
            .await;
            tell_job(&app, "backup", &instance.name, "done", 0, 0);
            let made = made?;
            api.restore_backup(&target, &file_name).await?;
            record_audit(&app, &id, "Backup", "Restore backup (panel)", &file_name);
            return Ok(backups::BackupFile {
                size_mib: made.bytes / 1_048_576.0,
                created_at: made.created_at,
                file_name: made.uuid,
                note: made.name,
                worlds_only: false,
            });
        }
        if !instance.remote_host.is_empty() {
            ensure_remote_stopped(&app, &state, &instance).await?;
            let safety = remote::restore_backup(&app, &instance.remote_host, &instance.path, &file_name).await?;
            record_audit(&app, &id, "Backup", "Restore backup (SSH)", &file_name);
            let made = remote::backups(&app, &instance.remote_host, &instance.path).await.into_iter().find(|one| one.name == safety);
            return Ok(backups::BackupFile {
                size_mib: made.as_ref().map(|one| one.size_bytes as f64 / 1_048_576.0).unwrap_or(0.0),
                created_at: made.as_ref().map(|one| one.modified_at).unwrap_or(0),
                file_name: safety,
                note: "Before rollback".into(),
                worlds_only: false,
            });
        }
    }
    ensure_stopped(&state, &id)?;
    let instance = instance_by_id(&app, &id)?;
    let vault = backup_folder(&app, &instance)?;
    let made = off_runtime(|| backups::restore(Path::new(&instance.path), &file_name, &vault))?;
    record_audit(&app, &id, "Server", "Backup restore", &file_name);
    Ok(made)
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ProxyBackend {
    id: String,
    name: String,
    core: String,
    port: u16,
    /// What velocity.toml points at: 127.0.0.1 on the same machine, the SSH host's address otherwise.
    address: String,
    /// Listed in velocity.toml.
    linked: bool,
    /// `Ready`, `Setup` (can be set up), `NeedsMod` (FabricProxy-Lite missing), `StartFirst` or `Unsupported`.
    state: String,
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ProxyNetwork {
    modern: bool,
    first: String,
    proxy_port: u16,
    backends: Vec<ProxyBackend>,
}

/// A text file in the server folder, or None when it isn't there.
async fn server_text(app: &AppHandle, instance: &ServerInstance, relative: &str) -> Result<Option<String>, String> {
    if is_far(instance) {
        return match far_read(app, instance, relative).await {
            Ok(text) => Ok(Some(text)),
            Err(error) if remote::is_missing(&error) => Ok(None),
            Err(error) => Err(error),
        };
    }
    match paths::resolve_in_server(Path::new(&instance.path), relative) {
        Ok(path) => fs::read_to_string(path).map(Some).map_err(|error| format!("Could not read {relative}: {error}")),
        Err(paths::PathError::Missing) => Ok(None),
        Err(_) => Err(format!("Could not read {relative}.")),
    }
}

async fn server_write_text(app: &AppHandle, instance: &ServerInstance, relative: &str, text: &str) -> Result<(), String> {
    if is_far(instance) {
        return far_write(app, instance, relative, text).await;
    }
    let root = Path::new(&instance.path);
    let checked = paths::check_relative(relative).map_err(|_| format!("Could not write {relative}."))?;
    if let Some(parent) = checked.parent().filter(|one| !one.as_os_str().is_empty()) {
        fs::create_dir_all(root.join(parent)).map_err(|error| format!("Could not write {relative}: {error}"))?;
    }
    let path = paths::resolve_new_in_server(root, relative).map_err(|_| format!("Could not write {relative}."))?;
    fs::write(path, text).map_err(|error| format!("Could not write {relative}: {error}"))
}

async fn has_fabric_proxy(app: &AppHandle, instance: &ServerInstance) -> bool {
    let names: Vec<String> = if is_far(instance) {
        far_list(app, instance, "mods").await.map(|rows| rows.into_iter().map(|row| row.name).collect()).unwrap_or_default()
    } else {
        fs::read_dir(Path::new(&instance.path).join("mods"))
            .map(|entries| entries.flatten().map(|one| one.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    };
    names.iter().any(|name| name.to_lowercase().contains("fabricproxy-lite") && name.ends_with(".jar"))
}

/// Servers a proxy can reach at 127.0.0.1: on this computer for a local proxy, on the same SSH host for an SSH one.
fn proxy_neighbors(app: &AppHandle, proxy: &ServerInstance) -> Result<Vec<ServerInstance>, String> {
    if !proxy.panel_host.is_empty() {
        return Err("Panel proxies can't be linked from the app yet.".into());
    }
    Ok(load_instances(app)?
        .into_iter()
        .filter(|one| {
            one.id != proxy.id
                && one.panel_host.is_empty()
                && (one.remote_host == proxy.remote_host || (proxy.remote_host.is_empty() && !one.remote_host.is_empty()))
        })
        .filter(|one| kinds::minecraft(&one.core).is_some() && !kinds::is_proxy(&one.core) && !pumpkin::is_pumpkin(&one.core))
        .collect())
}

fn backend_port(instance: &ServerInstance) -> u16 {
    if is_far(instance) {
        instance.port
    } else {
        port_from_properties(instance)
    }
}

async fn proxy_secret(app: &AppHandle, proxy: &ServerInstance, file: &str, make: bool) -> Result<Option<String>, String> {
    if let Some(found) = server_text(app, proxy, file).await?.map(|text| text.trim().to_string()).filter(|text| !text.is_empty()) {
        return Ok(Some(found));
    }
    if !make {
        return Ok(None);
    }
    let fresh = proxy::new_secret()?;
    server_write_text(app, proxy, file, &fresh).await?;
    Ok(Some(fresh))
}

async fn backend_state(app: &AppHandle, backend: &ServerInstance, secret: Option<&str>) -> Result<String, String> {
    if kinds::addon_word(&backend.core) == "plugin" {
        let Some(yaml) = server_text(app, backend, "config/paper-global.yml").await? else {
            let started = server_text(app, backend, "server.properties").await?.is_some();
            return Ok(if started { "Unsupported" } else { "StartFirst" }.into());
        };
        let offline =
            server_text(app, backend, "server.properties").await?.and_then(|text| proxy::property(&text, "online-mode")).as_deref()
                == Some("false");
        let ready = secret.is_some_and(|secret| proxy::paper_velocity_on(&yaml, secret)) && offline;
        return Ok(if ready { "Ready" } else { "Setup" }.into());
    }
    if backend.core == "Fabric" {
        if !has_fabric_proxy(app, backend).await {
            return Ok("NeedsMod".into());
        }
        let config = server_text(app, backend, "config/FabricProxy-Lite.toml").await?.unwrap_or_default();
        let offline =
            server_text(app, backend, "server.properties").await?.and_then(|text| proxy::property(&text, "online-mode")).as_deref()
                == Some("false");
        let ready = secret.is_some_and(|secret| proxy::fabric_proxy_on(&config, secret)) && offline;
        return Ok(if ready { "Ready" } else { "Setup" }.into());
    }
    Ok("Unsupported".into())
}

fn is_local_address(address: &str, port: u16) -> bool {
    let Some((host, at)) = address.rsplit_once(':') else { return false };
    at.parse::<u16>().ok() == Some(port) && matches!(host, "127.0.0.1" | "localhost" | "0.0.0.0")
}

/// Where a proxy reaches a backend: loopback on the same machine, the SSH host's address for a local proxy.
fn backend_address(app: &AppHandle, proxy: &ServerInstance, backend: &ServerInstance) -> Result<String, String> {
    let port = backend_port(backend);
    if backend.remote_host == proxy.remote_host {
        return Ok(format!("127.0.0.1:{port}"));
    }
    let host = remote::host(app, &backend.remote_host)?.address.trim().to_string();
    Ok(if host.contains(':') && !host.starts_with('[') { format!("[{host}]:{port}") } else { format!("{host}:{port}") })
}

fn same_backend(listed: &str, address: &str, port: u16) -> bool {
    if address.starts_with("127.0.0.1:") {
        is_local_address(listed, port)
    } else {
        listed.eq_ignore_ascii_case(address)
    }
}

async fn answers(address: &str) -> bool {
    let address = address.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        use std::net::ToSocketAddrs;
        address
            .to_socket_addrs()
            .ok()
            .and_then(|mut all| all.next())
            .is_some_and(|spot| std::net::TcpStream::connect_timeout(&spot, Duration::from_secs(4)).is_ok())
    })
    .await
    .unwrap_or(false)
}

#[tauri::command(async)]
#[specta::specta]
async fn proxy_network(app: AppHandle, id: String) -> Result<ProxyNetwork, String> {
    let instance = instance_by_id(&app, &id)?;
    if instance.core != "Velocity" {
        return Err("Only Velocity proxies can be linked from the app.".into());
    }
    let neighbors = proxy_neighbors(&app, &instance)?;
    let Some(text) = server_text(&app, &instance, "velocity.toml").await? else {
        return Err("Start the proxy once so it writes velocity.toml.".into());
    };
    let config = proxy::read_velocity(&text)?;
    let secret = proxy_secret(&app, &instance, &config.secret_file, false).await?;
    let mut backends = Vec::new();
    for one in neighbors {
        let port = backend_port(&one);
        let Ok(address) = backend_address(&app, &instance, &one) else { continue };
        let linked = config.servers.iter().any(|(_, listed)| same_backend(listed, &address, port));
        let state = backend_state(&app, &one, secret.as_deref()).await?;
        backends.push(ProxyBackend { id: one.id, name: one.name, core: one.core, port, address, linked, state });
    }
    let first = config
        .first
        .first()
        .and_then(|name| config.servers.iter().find(|(key, _)| key == name))
        .and_then(|(_, listed)| backends.iter().find(|one| same_backend(listed, &one.address, one.port)))
        .map(|one| one.id.clone())
        .unwrap_or_default();
    Ok(ProxyNetwork { modern: config.modern, first, proxy_port: backend_port(&instance), backends })
}

/// Lists the chosen servers in velocity.toml with modern forwarding, sets each of them up to accept it,
/// and turns forwarding back off on servers taken out of the list. Returns what still needs doing.
#[tauri::command(async)]
#[specta::specta]
async fn link_proxy(app: AppHandle, id: String, servers: Vec<String>, first: String) -> Result<Vec<String>, String> {
    let instance = instance_by_id(&app, &id)?;
    if instance.core != "Velocity" {
        return Err("Only Velocity proxies can be linked from the app.".into());
    }
    let neighbors = proxy_neighbors(&app, &instance)?;
    let Some(text) = server_text(&app, &instance, "velocity.toml").await? else {
        return Err("Start the proxy once so it writes velocity.toml.".into());
    };
    let config = proxy::read_velocity(&text)?;
    let secret = proxy_secret(&app, &instance, &config.secret_file, true).await?.unwrap_or_default();
    let proxy_port = backend_port(&instance);
    let mut notes = Vec::new();
    let mut entries: Vec<(String, String)> = Vec::new();
    let mut first_key = String::new();
    // Ports already taken on each machine (the proxy's and the linked servers'), to hand out free ones on a clash.
    let everyone = load_instances(&app)?;
    let mut taken_ports: Vec<(String, u16)> = vec![(instance.remote_host.clone(), proxy_port)];
    for one in neighbors.iter().filter(|one| servers.contains(&one.id)) {
        let mut port = backend_port(one);
        if taken_ports.iter().any(|(host, used)| *host == one.remote_host && *used == port) {
            if is_far(one) {
                return Err(format!(
                    "{} uses port {port}, which the proxy or another linked server already has. Change one of the ports first.",
                    one.name
                ));
            }
            // A server on this PC moves to the next free port by itself (applies when it restarts).
            let mut next = next_free_port(&everyone, port.max(25565));
            while taken_ports.iter().any(|(host, used)| *host == one.remote_host && *used == next) || !port_free(next) {
                next = next_free_port(&everyone, next);
            }
            let props = server_text(&app, one, "server.properties").await?.unwrap_or_default();
            server_write_text(&app, one, "server.properties", &proxy::set_property(&props, "server-port", &next.to_string())).await?;
            set_instance_port(&app, &one.id, next)?;
            notes.push(format!("{} used port {port}, which was taken, so it moved to {next}.", one.name));
            port = next;
        }
        taken_ports.push((one.remote_host.clone(), port));
        match backend_state(&app, one, Some(&secret)).await?.as_str() {
            "StartFirst" => {
                notes.push(format!("Start {} once, then link it again.", one.name));
                continue;
            }
            "Unsupported" => {
                notes.push(format!(
                    "{} can't take players from Velocity: it needs Paper or a fork of it, or Fabric with FabricProxy-Lite.",
                    one.name
                ));
                continue;
            }
            "NeedsMod" => {
                notes.push(format!("Add the FabricProxy-Lite mod to {}, then link it again.", one.name));
                continue;
            }
            _ => {}
        }
        if kinds::addon_word(&one.core) == "plugin" {
            let yaml = server_text(&app, one, "config/paper-global.yml").await?.unwrap_or_default();
            server_write_text(&app, one, "config/paper-global.yml", &proxy::paper_velocity(&yaml, true, &secret)).await?;
        } else {
            let toml = server_text(&app, one, "config/FabricProxy-Lite.toml").await?.unwrap_or_default();
            server_write_text(&app, one, "config/FabricProxy-Lite.toml", &proxy::fabric_proxy(&toml, &secret)?).await?;
        }
        let props = server_text(&app, one, "server.properties").await?.unwrap_or_default();
        server_write_text(&app, one, "server.properties", &proxy::set_property(&props, "online-mode", "false")).await?;
        let taken: Vec<String> = entries.iter().map(|(key, _)| key.clone()).collect();
        let key = proxy::server_key(&one.name, &taken);
        if one.id == first || first_key.is_empty() {
            first_key = key.clone();
        }
        let address = backend_address(&app, &instance, one)?;
        if !address.starts_with("127.0.0.1:") && !answers(&address).await {
            notes.push(format!(
                "{address} ({}) didn't answer from this PC. Open that port on the SSH server's firewall and start the server.",
                one.name
            ));
        }
        entries.push((key, address));
    }
    if entries.is_empty() && !servers.is_empty() {
        return Err(notes.first().cloned().unwrap_or_else(|| "Choose at least one server to link.".into()));
    }
    for one in neighbors.iter().filter(|one| !servers.contains(&one.id)) {
        let port = backend_port(one);
        let Ok(address) = backend_address(&app, &instance, one) else { continue };
        if !config.servers.iter().any(|(_, listed)| same_backend(listed, &address, port)) {
            continue;
        }
        if kinds::addon_word(&one.core) == "plugin" {
            if let Some(yaml) = server_text(&app, one, "config/paper-global.yml").await? {
                server_write_text(&app, one, "config/paper-global.yml", &proxy::paper_velocity(&yaml, false, "")).await?;
            }
        }
        if let Some(props) = server_text(&app, one, "server.properties").await? {
            server_write_text(&app, one, "server.properties", &proxy::set_property(&props, "online-mode", "true")).await?;
        }
    }
    server_write_text(&app, &instance, "velocity.toml", &proxy::write_velocity(&text, &entries, &first_key)?).await?;
    record_audit(
        &app,
        &id,
        "Server",
        "Link servers to the proxy",
        &entries.iter().map(|(key, address)| format!("{key}={address}")).collect::<Vec<_>>().join(", "),
    );
    notes.push("Restart the proxy and the linked servers to apply.".into());
    Ok(notes)
}

const PANEL_WHOLE: &str = "Panel backups can only be restored whole.";

#[tauri::command(async)]
#[specta::specta]
async fn backup_contents(app: AppHandle, id: String, file_name: String) -> Result<backups::BackupContents, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        return Err(PANEL_WHOLE.into());
    }
    if !instance.remote_host.is_empty() {
        let (then, now) = remote::backup_listing(&app, &instance.remote_host, &instance.path, &file_name).await?;
        let entries = backups::compare(then, now, backups::is_partial(&file_name), |_, _| true);
        return Ok(backups::BackupContents { entries, exact: false });
    }
    let vault = backup_folder(&app, &instance)?;
    let task = progress::begin(&app, "backupContents", &id);
    off_runtime(|| backups::contents(Path::new(&instance.path), &vault, &file_name, &task))
}

/// Stops a long command the app shows a job for. `kind` is the command's name in the app.
#[tauri::command]
#[specta::specta]
fn cancel_task(kind: String, server_id: String) -> bool {
    progress::cancel(&kind, &server_id)
}

#[tauri::command(async)]
#[specta::specta]
async fn backup_entry_text(app: AppHandle, id: String, file_name: String, path: String) -> Result<Option<String>, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        return Err(PANEL_WHOLE.into());
    }
    let inside = backups::clean_picks(std::slice::from_ref(&path))?.remove(0);
    if !instance.remote_host.is_empty() {
        return remote::backup_entry_text(&app, &instance.remote_host, &instance.path, &file_name, &inside).await;
    }
    let vault = backup_folder(&app, &instance)?;
    off_runtime(|| backups::entry_text(&vault, &file_name, &inside))
}

#[tauri::command(async)]
#[specta::specta]
async fn restore_backup_paths(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    file_name: String,
    paths: Vec<String>,
) -> Result<backups::BackupFile, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        return Err(PANEL_WHOLE.into());
    }
    let picks = backups::clean_picks(&paths)?;
    let detail = format!("{file_name}: {}", picks.join(", "));
    if !instance.remote_host.is_empty() {
        ensure_remote_stopped(&app, &state, &instance).await?;
        let safety = remote::restore_backup_paths(&app, &instance.remote_host, &instance.path, &file_name, &picks).await?;
        record_audit(&app, &id, "Backup", "Restore files from backup (SSH)", &detail);
        return Ok(backups::BackupFile {
            file_name: safety,
            size_mib: 0.0,
            created_at: unix_now(),
            note: "Before partial rollback".into(),
            worlds_only: false,
        });
    }
    ensure_stopped(&state, &id)?;
    let vault = backup_folder(&app, &instance)?;
    let made = off_runtime(|| backups::restore_paths(Path::new(&instance.path), &file_name, &vault, &picks))?;
    record_audit(&app, &id, "Server", "Restore files from backup", &detail);
    Ok(made)
}

#[tauri::command(async)]
#[specta::specta]
async fn delete_backup(app: AppHandle, id: String, file_name: String) -> Result<(), String> {
    {
        let instance = instance_by_id(&app, &id)?;
        if !instance.panel_host.is_empty() {
            let (api, target) = panel_of(&app, &instance)?;
            api.delete_backup(&target, &file_name).await?;
            record_audit(&app, &id, "Backup", "Delete backup (panel)", &file_name);
            return Ok(());
        }
        if !instance.remote_host.is_empty() {
            let name = plain_name(&file_name)?;
            let vault = remote::backup_vault(&instance.path);
            let conn = remote::connect(&app, &instance.remote_host).await?;
            conn.run(&format!("rm -f {}", remote::quote(&format!("{vault}/{name}")))).await?.ok()?;
            record_audit(&app, &id, "Backup", "Delete backup (SSH)", name);
            return Ok(());
        }
    }
    let instance = instance_by_id(&app, &id)?;
    let vault = backup_folder(&app, &instance)?;
    off_runtime(|| backups::remove(&vault, &file_name))?;
    record_audit(&app, &id, "Server", "Backup deleted", &file_name);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
async fn list_remote_javas(app: AppHandle, id: String) -> Result<Vec<java::JavaRuntime>, String> {
    let instance = remote_instance(&app, &id)?.ok_or("This isn't an SSH server.")?;
    remote::javas(&app, &instance.remote_host).await
}

#[tauri::command(async)]
#[specta::specta]
async fn precheck_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<Vec<doctor::Finding>, String> {
    if panel_instance(&app, &id)?.is_some() {
        return Ok(Vec::new());
    }
    if let Some(instance) = remote_instance(&app, &id)? {
        if pumpkin::is_pumpkin(&instance.core) {
            return Ok(Vec::new());
        }
        let jar = if instance.custom_command.trim().is_empty() && instance.start_script.trim().is_empty() {
            remote_jar(&app, &instance).await?.jar
        } else {
            String::new()
        };
        let needs = java::required_java_major_for(&instance.core, &instance.version);
        return remote::precheck(&app, &instance.remote_host, &instance.path, &jar, &instance.java_path, needs).await;
    }
    let instance = instance_by_id(&app, &id)?;
    let root = Path::new(&instance.path);
    let native = pumpkin::is_pumpkin(&instance.core);
    let jar =
        instance.server_jar.as_ref().map(|name| root.join(name)).filter(|_| {
            !native && instance.launch_mode != "script" && instance.start_script.trim().is_empty() && !needs_args_file(&instance)
        });
    let port = port_from_properties(&instance);
    let needs = java::required_java_major_for(&instance.core, &instance.version);
    let java_major = if native {
        None
    } else {
        java::resolve_java_runtime(&app, &instance.core, &instance.version, &instance.java_path)
            .ok()
            .and_then(|path| java::ask_java_major(&path))
    };
    let machine = {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        system.total_memory() / 1024 / 1024
    };
    let _ = &state;
    let max_memory = if native { 0 } else { instance.max_memory_mib };
    let exe_missing =
        native && !instance.server_jar.as_ref().map(|name| root.join(name)).unwrap_or_else(|| root.join(cores::PUMPKIN_EXE)).is_file();
    Ok(off_runtime(move || {
        let mut findings = Vec::new();
        if exe_missing {
            findings.push(doctor::Finding::stop(
                "The Pumpkin executable is missing",
                "There is no pumpkin.exe in the server folder. Create the server again, or download the release again from the Upgrade tab.",
            ));
        }
        findings.extend(doctor::precheck(root, jar.as_deref(), port, max_memory, machine, java_major, needs));
        findings
    }))
}

#[tauri::command(async)]
#[specta::specta]
async fn server_commands(app: AppHandle, id: String) -> Result<events::CommandBook, String> {
    let instance = instance_by_id(&app, &id)?;
    if is_far(&instance) {
        let path = format!("{}/commands.json", remote_addon_data_dir(&instance));
        return Ok(far_read(&app, &instance, &path).await.map(|text| events::command_book_from_text(&text)).unwrap_or_default());
    }
    Ok(events::read_command_book(Path::new(&instance.path)))
}

#[tauri::command(async)]
#[specta::specta]
fn required_java(app: AppHandle, id: String) -> Result<u32, String> {
    let instance = instance_by_id(&app, &id)?;
    Ok(java::required_java_major_for(&instance.core, &instance.version))
}

#[tauri::command(async)]
#[specta::specta]
fn download_java(app: AppHandle, major: u32) -> Result<java::JavaRuntime, String> {
    java::fetch_runtime(&app, major)
}

#[tauri::command(async)]
#[specta::specta]
fn item_icons(app: AppHandle, version: String, ids: Vec<String>) -> Result<HashMap<String, String>, String> {
    off_runtime(|| textures::icons(&app, &version, &ids))
}

/// Every item id the Minecraft client has (`diamond`, `oak_planks`...), sorted, for the studio's item picker.
#[tauri::command(async)]
#[specta::specta]
fn item_ids(app: AppHandle, version: String) -> Result<Vec<String>, String> {
    off_runtime(|| textures::item_ids(&app, &version))
}

/// The game's pictures for the studio previews: the client version they come from, and data URLs by texture path.
#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct GameFiles {
    version: String,
    files: HashMap<String, String>,
}

/// Texture files from the Minecraft client (`gui/container/generic_54.png`, `font/ascii.png`...) for the studio
/// previews. `version` is the one `studio_assets` named; it must already be downloaded (this never downloads).
#[tauri::command(async)]
#[specta::specta]
fn game_files(app: AppHandle, version: String, paths: Vec<String>) -> Result<GameFiles, String> {
    off_runtime(|| {
        let version = version.trim().to_string();
        if !textures::has_client(&app, &version) {
            return Err("Download the game's pictures for this version first.".into());
        }
        let files = textures::files(&app, &version, &paths)?;
        Ok(GameFiles { version, files })
    })
}

/// The Minecraft pictures a server's studio needs: the client version and whether it is on this PC yet.
#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct StudioAssets {
    version: String,
    ready: bool,
}

/// Which client version this server's studio needs (its own Minecraft version, or the newest release for Pumpkin
/// and the like) and whether it is downloaded. The studio can't be used until it is.
#[tauri::command(async)]
#[specta::specta]
fn studio_assets(app: AppHandle, id: String) -> Result<StudioAssets, String> {
    let instance = instance_by_id(&app, &id)?;
    off_runtime(|| {
        let version = textures::art_version(&app, &instance.version)?;
        let ready = textures::has_client(&app, &version);
        Ok(StudioAssets { version, ready })
    })
}

/// Downloads the Minecraft client file this server's studio needs from Mojang (checked against Mojang's sha1).
#[tauri::command(async)]
#[specta::specta]
fn download_studio_assets(app: AppHandle, id: String) -> Result<StudioAssets, String> {
    let instance = instance_by_id(&app, &id)?;
    off_runtime(|| {
        let version = textures::art_version(&app, &instance.version)?;
        textures::client_jar(&app, &version)?;
        Ok(StudioAssets { ready: textures::has_client(&app, &version), version })
    })
}

/// The studio only works with the pictures for its version on this PC.
fn studio_assets_ready(app: &AppHandle, instance: &ServerInstance) -> Result<(), String> {
    let version = textures::art_version(app, &instance.version)?;
    if textures::has_client(app, &version) {
        Ok(())
    } else {
        Err(format!("Download the Minecraft {version} pictures in the studio first."))
    }
}

#[tauri::command(async)]
#[specta::specta]
fn remove_java_runtime(app: AppHandle, path: String) -> Result<String, String> {
    let major = java::remove_runtime(&app, path.trim())?;
    {
        let _guard = lock_instance_store();
        let mut instances = load_instances(&app)?;
        let mut touched = false;
        for instance in &mut instances {
            if instance.java_path.eq_ignore_ascii_case(path.trim()) {
                instance.java_path = "java".into();
                touched = true;
            }
        }
        if touched {
            save_instances(&app, &instances)?;
        }
    }
    Ok(if major > 0 { format!("Removed Java {major}.") } else { "Java removed.".into() })
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct CoreBuildNews {
    current: String,
    newest: String,
    channel: String,
    behind: bool,
    unknown: bool,
}

#[tauri::command(async)]
#[specta::specta]
fn check_core_build(app: AppHandle, id: String) -> Result<CoreBuildNews, String> {
    let instance = instance_by_id(&app, &id)?;
    let core = instance.core.clone();
    let version = instance.version.clone();
    let builds = off_runtime(move || cores::list_core_builds(&core, &version))?;
    let Some(newest) = builds.iter().find(|one| one.stable).or_else(|| builds.first()) else {
        return Ok(CoreBuildNews {
            current: instance.core_build.clone(),
            newest: String::new(),
            channel: String::new(),
            behind: false,
            unknown: false,
        });
    };
    let current = instance.core_build.clone();
    Ok(CoreBuildNews {
        behind: !current.is_empty() && current != newest.id,
        unknown: current.is_empty(),
        newest: newest.id.clone(),
        channel: newest.channel.clone(),
        current,
    })
}

#[tauri::command(async)]
#[specta::specta]
fn list_core_versions(core: String, snapshots: Option<bool>) -> Result<Vec<String>, String> {
    off_runtime(|| cores::list_core_versions(&core, snapshots.unwrap_or(false)))
}

pub(crate) type WebEvents = Arc<Mutex<Vec<(u64, String, serde_json::Value)>>>;

/// Keeps an event for web panel and Monolink guests, who catch up with `__events`. Kept even while
/// the web panel is off, since Monolink guests read the same list (2000 newest).
fn remember_for_web<T: serde::Serialize>(app: &AppHandle, event: &str, payload: &T) {
    let Ok(value) = serde_json::to_value(payload) else {
        return;
    };
    let held = app.state::<WebEvents>();
    let mut queue = held.lock().expect("web events poisoned");
    let at = queue.last().map(|(at, _, _)| *at).unwrap_or(0) + 1;
    queue.push((at, event.to_string(), value));
    if queue.len() > 2000 {
        let cut = queue.len() - 2000;
        queue.drain(..cut);
    }
}

pub(crate) fn web_invoke(app: &AppHandle, command: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    fn arg<T: serde::de::DeserializeOwned>(body: &serde_json::Value, name: &str) -> Result<T, String> {
        serde_json::from_value(body.get(name).cloned().unwrap_or(serde_json::Value::Null))
            .map_err(|error| format!("{name} is invalid: {error}"))
    }
    fn ok<T: serde::Serialize>(value: T) -> Result<serde_json::Value, String> {
        serde_json::to_value(value).map_err(|error| error.to_string())
    }
    match command {
        "list_instances" => ok(list_instances(app.clone(), app.state())?),
        "get_app_settings" => ok(get_app_settings(app.clone())?),
        "app_version" => ok(app_version(app.clone())),
        "get_servers_root" => ok(get_servers_root(app.clone())?),
        "list_trash_instances" => ok(list_trash_instances(app.clone())?),
        "read_live" => ok(tauri::async_runtime::block_on(read_live(app.clone(), arg(body, "id")?))?),
        "list_runs" => ok(list_runs(app.clone(), arg(body, "id")?)?),
        "server_memo" => ok(server_memo(app.clone(), arg(body, "id")?)?),
        "set_server_memo" => ok(set_server_memo(app.clone(), arg(body, "id")?, arg(body, "memo")?, arg(body, "expectHash")?)?),
        "agent_read_memo" => ok(agent_read_memo(app.clone(), arg(body, "id")?)?),
        "agent_update_todo" => ok(agent_update_todo(
            app.clone(),
            arg(body, "id")?,
            arg(body, "action")?,
            arg(body, "todo")?,
            arg(body, "text")?,
            arg(body, "status")?,
            arg(body, "assignee")?,
        )?),
        "rewind_scan" => ok(rewind_scan(app.clone(), arg(body, "id")?)?),
        "rewind_list" => ok(rewind_list(app.clone(), arg(body, "id")?, arg(body, "path")?, arg(body, "limit")?)?),
        "rewind_versions" => ok(rewind_versions(app.clone(), arg(body, "id")?, arg(body, "entry")?)?),
        "rewind_restore" => ok(rewind_restore(app.clone(), arg(body, "id")?, arg(body, "entry")?, arg(body, "side")?)?),
        "rewind_enabled" => ok(rewind_enabled(app.clone(), arg(body, "id")?)?),
        // Live settings (MSMP): the agent's tools, the web panel and teammates.
        "msmp_state" => ok(tauri::async_runtime::block_on(msmp_state(app.clone(), app.state(), arg(body, "id")?))?),
        "msmp_setup" => ok(tauri::async_runtime::block_on(msmp_setup(app.clone(), arg(body, "id")?))?),
        "msmp_settings" => ok(tauri::async_runtime::block_on(msmp_settings(app.clone(), arg(body, "id")?))?),
        "msmp_set_setting" => {
            ok(tauri::async_runtime::block_on(msmp_set_setting(app.clone(), arg(body, "id")?, arg(body, "key")?, arg(body, "value")?))?)
        }
        "msmp_rules" => ok(tauri::async_runtime::block_on(msmp_rules(app.clone(), arg(body, "id")?))?),
        "msmp_set_rule" => {
            ok(tauri::async_runtime::block_on(msmp_set_rule(app.clone(), arg(body, "id")?, arg(body, "key")?, arg(body, "value")?))?)
        }
        "set_rewind_enabled" => ok(set_rewind_enabled(app.clone(), arg(body, "id")?, arg(body, "on")?)?),
        "create_web_invite" => ok(create_web_invite(app.clone(), app.state(), arg(body, "id")?, arg(body, "role")?)?),
        "list_web_invites" => ok(list_web_invites(app.clone(), arg(body, "id")?)?),
        "revoke_web_invite" => ok(revoke_web_invite(app.clone(), arg(body, "invite")?)?),
        "server_people" => ok(server_people(app.clone(), app.state(), arg(body, "id")?)?),
        "remove_server_access" => ok(remove_server_access(app.clone(), app.state(), arg(body, "id")?, arg(body, "name")?)?),
        "web_online" => ok(web_online(app.state())),
        "add_memo_image" => ok(add_memo_image(app.clone(), arg(body, "id")?, arg(body, "extension")?, arg(body, "data")?)?),
        "memo_image" => ok(memo_image(app.clone(), arg(body, "id")?, arg(body, "name")?)?),
        "delete_memo_image" => ok(delete_memo_image(app.clone(), arg(body, "id")?, arg(body, "name")?)?),
        "map_surface_height" => {
            ok(map_surface_height(app.state(), arg(body, "id")?, arg(body, "realm")?, arg(body, "x")?, arg(body, "z")?))
        }
        "disk_usage" => ok(tauri::async_runtime::block_on(disk_usage(app.clone(), arg(body, "id")?))?),
        "chunk_stats" => ok(tauri::async_runtime::block_on(chunk_stats(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "realm")?,
            arg(body, "metric")?,
            arg(body, "rx")?,
            arg(body, "rz")?,
        ))?),
        "chunky_status" => ok(tauri::async_runtime::block_on(chunky_status(app.clone(), app.state(), arg(body, "id")?))?),
        "chunky_run" => ok(tauri::async_runtime::block_on(chunky_run(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "action")?,
            arg(body, "world")?,
            arg(body, "x")?,
            arg(body, "z")?,
            arg(body, "radius")?,
            arg(body, "radiusZ")?,
            arg(body, "shape")?,
        ))?),
        "clean_disk" => ok(tauri::async_runtime::block_on(clean_disk(app.clone(), arg(body, "id")?, arg(body, "kind")?))?),
        "read_loaded_chunks" => ok(tauri::async_runtime::block_on(read_loaded_chunks(app.clone(), arg(body, "id")?))?),
        "diagnose_lag" => ok(tauri::async_runtime::block_on(diagnose_lag(app.clone(), app.state(), arg(body, "id")?))?),
        "addon_status" => ok(tauri::async_runtime::block_on(addon_status(app.clone(), app.state(), arg(body, "id")?))?),
        "read_server_properties" => ok(tauri::async_runtime::block_on(read_server_properties(app.clone(), arg(body, "id")?))?),
        "list_worlds" => ok(tauri::async_runtime::block_on(list_worlds(app.clone(), arg(body, "id")?))?),
        "browse_server_files" => ok(tauri::async_runtime::block_on(browse_server_files(
            app.clone(),
            arg(body, "id")?,
            arg(body, "path")?,
            arg(body, "offset")?,
            arg(body, "limit")?,
        ))?),
        "list_clients" => ok(clients::list_clients(app.clone())?),
        "create_client" => ok(clients::create_client(app.clone(), arg(body, "name")?, arg(body, "version")?, arg(body, "loader")?)?),
        "rename_client" => ok(clients::rename_client(app.clone(), arg(body, "id")?, arg(body, "name")?)?),
        "delete_client" => ok(clients::delete_client(app.clone(), arg(body, "id")?)?),
        "client_content" => ok(client_content::client_content(app.clone(), arg(body, "id")?)?),
        "client_content_cards" => ok(client_content::client_content_cards(app.clone(), arg(body, "id")?)?),
        "set_client_content_enabled" => ok(client_content::set_client_content_enabled(
            app.clone(),
            arg(body, "id")?,
            arg(body, "kind")?,
            arg(body, "fileName")?,
            arg(body, "enabled")?,
        )?),
        "remove_client_content" => {
            ok(client_content::remove_client_content(app.clone(), arg(body, "id")?, arg(body, "kind")?, arg(body, "fileNames")?)?)
        }
        "update_client_content" => {
            ok(client_content::update_client_content(app.clone(), arg(body, "id")?, arg(body, "kind")?, arg(body, "fileNames")?)?)
        }
        "get_client_account" => ok(client_launch::get_client_account(app.clone())?),
        "set_offline_name" => ok(client_launch::set_offline_name(app.clone(), arg(body, "name")?)?),
        "launch_client" => ok(client_launch::launch_client(app.clone(), arg(body, "id")?)?),
        "client_runs" => ok(client_launch::client_runs()),
        "bundled_forks" => ok(forks::bundled_forks()),
        "stop_client" => ok(client_launch::stop_client(arg(body, "id")?)?),
        "dismiss_client_run" => {
            client_launch::dismiss_client_run(arg(body, "id")?);
            ok(())
        }
        "nbt_open" => ok(nbt_edit::nbt_open(app.clone(), arg(body, "id")?, arg(body, "path")?)?),
        "nbt_open_chunk" => ok(nbt_edit::nbt_open_chunk(arg(body, "session")?, arg(body, "index")?)?),
        "nbt_act" => ok(nbt_edit::nbt_act(arg(body, "session")?, arg(body, "action")?, arg(body, "search")?)?),
        "nbt_copy" => ok(nbt_edit::nbt_copy(arg(body, "session")?, arg(body, "path")?, arg(body, "pretty")?)?),
        "nbt_save" => ok(nbt_edit::nbt_save(app.clone(), arg(body, "id")?, arg(body, "session")?, arg(body, "force")?)?),
        "nbt_close" => {
            nbt_edit::nbt_close(arg(body, "session")?);
            ok(())
        }
        "studio_overview" => ok(tauri::async_runtime::block_on(studio_overview(app.clone(), arg(body, "id")?))?),
        "studio_read" => {
            ok(tauri::async_runtime::block_on(studio_read(app.clone(), arg(body, "id")?, arg(body, "kind")?, arg(body, "name")?))?)
        }
        "studio_save" => ok(tauri::async_runtime::block_on(studio_save(
            app.clone(),
            arg(body, "id")?,
            arg(body, "kind")?,
            arg(body, "name")?,
            arg(body, "text")?,
            arg(body, "hash")?,
        ))?),
        "studio_template" => ok(studio_template(arg(body, "kind")?, arg(body, "name")?)?),
        "studio_worldgen_preview" => ok(studio_worldgen_preview(
            arg(body, "generator")?,
            arg(body, "seed")?,
            arg(body, "world")?,
            arg(body, "x")?,
            arg(body, "z")?,
            arg(body, "size")?,
            arg(body, "step")?,
        )?),
        "studio_watch" => ok(tauri::async_runtime::block_on(studio_watch_for(app.clone(), arg(body, "id")?, true))?),
        // A guest leaving doesn't close a line the host or other guests may use (the id is still read, so the
        // per-server check runs).
        "studio_unwatch" => {
            let _server: String = arg(body, "id")?;
            ok(())
        }
        "read_server_agents_md" => ok(team_agent::read_server_agents_md(app.clone(), arg(body, "id")?)?),
        "save_server_agents_md" => ok(team_agent::save_server_agents_md(app.clone(), arg(body, "id")?, arg(body, "content")?)?),
        "read_editable_file" => ok(tauri::async_runtime::block_on(read_editable_file(app.clone(), arg(body, "id")?, arg(body, "path")?))?),
        "read_server_image" => ok(tauri::async_runtime::block_on(read_server_image(app.clone(), arg(body, "id")?, arg(body, "path")?))?),
        "view_server_file" => ok(tauri::async_runtime::block_on(view_server_file(app.clone(), arg(body, "id")?, arg(body, "path")?))?),
        "read_texture_animation" => {
            ok(tauri::async_runtime::block_on(read_texture_animation(app.clone(), arg(body, "id")?, arg(body, "path")?))?)
        }
        "read_server_file_download" => {
            ok(tauri::async_runtime::block_on(read_server_file_download(app.clone(), arg(body, "id")?, arg(body, "path")?))?)
        }
        "list_backups" => ok(tauri::async_runtime::block_on(list_backups(app.clone(), arg(body, "id")?))?),
        "list_log_files" => ok(tauri::async_runtime::block_on(list_log_files(app.clone(), arg(body, "id")?))?),
        "list_crash_reports" => ok(tauri::async_runtime::block_on(list_crash_reports(app.clone(), arg(body, "id")?))?),
        "inspect_addons" => ok(tauri::async_runtime::block_on(inspect_addons(app.clone(), arg(body, "id")?))?),
        "list_event_days" => ok(list_event_days(app.clone(), arg(body, "id")?)?),
        "perf_history" => ok(perf_history(app.clone(), arg(body, "id")?, arg(body, "day")?)?),
        "server_commands" => ok(tauri::async_runtime::block_on(server_commands(app.clone(), arg(body, "id")?))?),
        "list_java_runtimes" => ok(list_java_runtimes(app.clone())?),
        "required_java" => ok(required_java(app.clone(), arg(body, "id")?)?),
        "list_tasks" => ok(list_tasks(app.clone())?),
        "list_audit_entries" => ok(list_audit_entries(app.clone(), arg(body, "id")?)?),
        "get_console_logs" => ok(get_console_logs(app.state(), arg(body, "id")?)),
        "panel_usage" => ok(panel_usage(app.state(), arg(body, "id")?)),
        "get_resource_snapshot" => ok(tauri::async_runtime::block_on(get_resource_snapshot(app.clone(), app.state(), arg(body, "id")?))?),
        "system_memory_mib" => ok(system_memory_mib(app.state())),
        "get_server_icon" => ok(tauri::async_runtime::block_on(get_server_icon(app.clone(), arg(body, "id")?))?),
        "list_players" => ok(tauri::async_runtime::block_on(list_players(app.clone(), arg(body, "id")?))?),
        "read_world_settings" => {
            ok(tauri::async_runtime::block_on(read_world_settings(app.clone(), arg(body, "id")?, arg(body, "world")?))?)
        }
        "list_bookmarks" => ok(list_bookmarks(app.clone(), arg(body, "id")?)?),
        "summarize_logs" => ok(tauri::async_runtime::block_on(summarize_logs(app.clone(), arg(body, "id")?))?),
        "read_log_timeline" => ok(tauri::async_runtime::block_on(read_log_timeline(
            app.clone(),
            arg(body, "id")?,
            arg(body, "fileName")?,
            arg(body, "kind")?,
        ))?),
        "list_log_entries" => ok(tauri::async_runtime::block_on(list_log_entries(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "query")?,
            arg(body, "level")?,
            arg(body, "fileName")?,
        ))?),
        "list_player_activity" => ok(tauri::async_runtime::block_on(list_player_activity(app.clone(), arg(body, "id")?))?),
        "list_core_versions" => ok(list_core_versions(arg(body, "core")?, arg(body, "snapshots")?)?),
        "precheck_instance" => ok(tauri::async_runtime::block_on(precheck_instance(app.clone(), app.state(), arg(body, "id")?))?),
        "list_remote_javas" => ok(tauri::async_runtime::block_on(list_remote_javas(app.clone(), arg(body, "id")?))?),
        "start_instance" => {
            ok(tauri::async_runtime::block_on(start_instance_as(app.clone(), app.state::<AppState>().inner(), arg(body, "id")?, false))?)
        }
        "stop_instance" => ok(tauri::async_runtime::block_on(stop_instance(app.clone(), app.state(), arg(body, "id")?))?),
        "restart_instance" => {
            ok(tauri::async_runtime::block_on(restart_instance_as(app.clone(), app.state::<AppState>().inner(), arg(body, "id")?, false))?)
        }
        "send_console_command" => {
            ok(tauri::async_runtime::block_on(send_console_command(app.clone(), app.state(), arg(body, "id")?, arg(body, "command")?))?)
        }
        "plugin_integrations" => ok(tauri::async_runtime::block_on(plugin_integrations(app.clone(), arg(body, "id")?))?),
        "essentials_data" => ok(tauri::async_runtime::block_on(essentials_data(app.clone(), arg(body, "id")?))?),
        "essentials_set_setting" => ok(tauri::async_runtime::block_on(essentials_set_setting(
            app.clone(),
            arg(body, "id")?,
            arg(body, "key")?,
            arg(body, "value")?,
        ))?),
        "luckperms_data" => ok(tauri::async_runtime::block_on(luckperms_data(app.clone(), app.state(), arg(body, "id")?))?),
        "addon_config" => ok(tauri::async_runtime::block_on(addon_config(app.clone(), arg(body, "id")?))?),
        "set_addon_config" => {
            ok(tauri::async_runtime::block_on(set_addon_config(app.clone(), arg(body, "id")?, arg(body, "key")?, arg(body, "on")?))?)
        }
        "extension_data" => ok(tauri::async_runtime::block_on(extension_data(app.clone(), arg(body, "id")?))?),
        "run_extension_action" => ok(tauri::async_runtime::block_on(run_extension_action(
            app.clone(),
            arg(body, "id")?,
            arg(body, "folder")?,
            arg(body, "label")?,
            arg(body, "argsJson")?,
        ))?),
        "extension_set" => ok(tauri::async_runtime::block_on(extension_set(
            app.clone(),
            arg(body, "id")?,
            arg(body, "folder")?,
            arg(body, "index")?,
            arg(body, "value")?,
        ))?),
        "save_server_properties" => ok(tauri::async_runtime::block_on(save_server_properties(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "properties")?,
        ))?),
        "save_editable_file" => ok(tauri::async_runtime::block_on(save_editable_file(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "path")?,
            arg(body, "content")?,
            arg(body, "expectHash")?,
        ))?),
        "rename_server_path" => ok(tauri::async_runtime::block_on(rename_server_path(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "path")?,
            arg(body, "newName")?,
        ))?),
        "trash_server_path" => {
            ok(tauri::async_runtime::block_on(trash_server_path(app.clone(), app.state(), arg(body, "id")?, arg(body, "path")?))?)
        }
        "list_server_trash" => ok(tauri::async_runtime::block_on(list_server_trash(app.clone(), arg(body, "id")?))?),
        "restore_server_trash" => {
            ok(tauri::async_runtime::block_on(restore_server_trash(app.clone(), app.state(), arg(body, "id")?, arg(body, "stored")?))?)
        }
        "delete_server_trash" => {
            ok(tauri::async_runtime::block_on(delete_server_trash(app.clone(), arg(body, "id")?, arg(body, "stored")?))?)
        }
        "empty_server_trash" => ok(tauri::async_runtime::block_on(empty_server_trash(app.clone(), arg(body, "id")?))?),
        "create_server_folder" => ok(tauri::async_runtime::block_on(create_server_folder(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "path")?,
            arg(body, "name")?,
        ))?),
        "create_server_file" => ok(tauri::async_runtime::block_on(create_server_file(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "path")?,
            arg(body, "name")?,
        ))?),
        "add_server_files" => ok(tauri::async_runtime::block_on(add_server_files(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "path")?,
            arg(body, "sources")?,
            arg::<Option<bool>>(body, "merge")?.unwrap_or(false),
        ))?),
        "add_server_uploads" => ok(tauri::async_runtime::block_on(add_server_uploads(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "path")?,
            arg(body, "uploads")?,
        ))?),
        "zip_server_paths" => {
            ok(tauri::async_runtime::block_on(zip_server_paths(app.clone(), arg(body, "id")?, arg(body, "paths")?, arg(body, "name")?))?)
        }
        "unzip_server_path" => {
            ok(tauri::async_runtime::block_on(unzip_server_path(app.clone(), app.state(), arg(body, "id")?, arg(body, "path")?))?)
        }
        "create_backup" => ok(tauri::async_runtime::block_on(create_backup(
            app.clone(),
            arg(body, "id")?,
            arg(body, "note")?,
            arg(body, "worldsOnly").unwrap_or(false),
            arg(body, "paths").unwrap_or_default(),
        ))?),
        "update_launch_settings" => {
            let input: LaunchSettingsInput = arg(body, "input")?;
            remote_java_allowed(app, &input)?;
            ok(update_launch_settings(app.clone(), app.state(), input)?)
        }
        "toggle_addon" => ok(tauri::async_runtime::block_on(toggle_addon(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "fileName")?,
            arg(body, "kind")?,
        ))?),
        "save_task" => ok(save_task(app.clone(), arg(body, "task")?)?),
        "delete_task" => ok(delete_task(app.clone(), arg(body, "id")?)?),
        "run_task_now" => ok(run_task_now(app.clone(), app.state(), arg(body, "id")?)?),
        "save_bookmark" => ok(save_bookmark(app.clone(), arg(body, "input")?)?),
        "save_world_settings" => ok(tauri::async_runtime::block_on(save_world_settings(app.clone(), app.state(), arg(body, "input")?))?),
        "toggle_player_access" => ok(tauri::async_runtime::block_on(toggle_player_access(app.clone(), app.state(), arg(body, "input")?))?),
        "scan_addons" => ok(tauri::async_runtime::block_on(scan_addons(app.clone(), arg(body, "id")?))?),
        "list_addon_rollbacks" => ok(list_addon_rollbacks(app.clone(), arg(body, "id")?)?),
        "list_automation_history" => ok(list_automation_history(app.clone(), arg(body, "id")?)?),
        "list_upgrade_history" => ok(list_upgrade_history(app.clone(), arg(body, "id")?)),
        "get_resource_history" => ok(get_resource_history(app.state(), arg(body, "id")?)),
        "get_tps_snapshot" => ok(get_tps_snapshot(app.clone(), app.state(), arg(body, "id")?)?),
        "check_core_build" => ok(check_core_build(app.clone(), arg(body, "id")?)?),
        "list_core_builds" => ok(cores::list_core_builds(&arg::<String>(body, "core")?, &arg::<String>(body, "version")?)?),
        "suggest_server_name" => ok(suggest_server_name(app.clone())?),
        "create_instance" => ok(create_instance(app.clone(), app.state(), arg(body, "input")?)?),
        "create_from_modpack" => ok(create_from_modpack(app.clone(), app.state(), arg(body, "input")?)?),
        "set_server_icon_upload" => ok(tauri::async_runtime::block_on(set_server_icon_upload(
            app.clone(),
            arg(body, "id")?,
            arg(body, "name")?,
            arg(body, "data")?,
        ))?),
        "add_addon_upload" => {
            ok(tauri::async_runtime::block_on(add_addon_upload(app.clone(), app.state(), arg(body, "id")?, arg(body, "files")?))?)
        }
        "stash_upload" => ok(stash_upload(app.clone(), arg(body, "name")?, arg(body, "data")?)?),
        "preview_mrpack" => ok(preview_mrpack(remote_upload_path(app, &arg::<String>(body, "path")?)?)?),
        "search_modpacks" => ok(search_modpacks(arg(body, "query")?, arg(body, "page")?)?),
        "download_modpack" => ok(download_modpack(app.clone(), arg(body, "projectId")?)?),
        "datapack_folder" => ok(tauri::async_runtime::block_on(datapack_folder(app.clone(), arg(body, "id")?))?),
        "search_datapacks" => ok(search_datapacks(app.clone(), arg(body, "id")?, arg(body, "query")?, arg(body, "page")?)?),
        "install_datapack" => ok(tauri::async_runtime::block_on(install_datapack(app.clone(), arg(body, "id")?, arg(body, "projectId")?))?),
        "import_mrpack" => {
            let mut input: MrpackImportInput = arg(body, "input")?;
            input.path = remote_upload_path(app, &input.path)?;
            ok(tauri::async_runtime::block_on(import_mrpack(app.clone(), app.state(), input))?)
        }
        "preview_cfpack" => ok(preview_cfpack(remote_upload_path(app, &arg::<String>(body, "path")?)?)?),
        "import_cfpack" => ok(tauri::async_runtime::block_on(import_cfpack(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            remote_upload_path(app, &arg::<String>(body, "path")?)?,
        ))?),
        "list_java_processes" => ok(list_java_processes(app.clone(), app.state())?),
        "list_pending_addons" => ok(list_pending_addons(app.clone(), arg(body, "id")?)?),
        "item_icons" => ok(item_icons(app.clone(), arg(body, "version")?, arg(body, "ids")?)?),
        "item_ids" => ok(item_ids(app.clone(), arg(body, "version")?)?),
        "game_files" => ok(game_files(app.clone(), arg(body, "version")?, arg(body, "paths")?)?),
        "studio_assets" => ok(studio_assets(app.clone(), arg(body, "id")?)?),
        // F1: the studio's assets and pack for teammates and the web panel (uploads only, never a path on the host).
        "studio_asset_list" => ok(tauri::async_runtime::block_on(studio_asset_list(app.clone(), arg(body, "id")?))?),
        "studio_asset_image" => ok(tauri::async_runtime::block_on(studio_asset_image(app.clone(), arg(body, "id")?, arg(body, "file")?))?),
        "studio_expansions" => ok(tauri::async_runtime::block_on(studio_expansions::studio_expansions(app.clone(), arg(body, "id")?))?),
        "studio_asset_add_upload" => ok(tauri::async_runtime::block_on(studio_asset_add_upload(
            app.clone(),
            arg(body, "id")?,
            arg(body, "folder")?,
            arg(body, "file")?,
            arg(body, "upload")?,
        ))?),
        // This PC's own agent passes the file's bytes (base64); never reachable from the web (not in the lists).
        "studio_asset_add" => ok(tauri::async_runtime::block_on(studio_asset_add(
            app.clone(),
            arg(body, "id")?,
            arg(body, "folder")?,
            arg(body, "file")?,
            arg(body, "data")?,
        ))?),
        "studio_asset_remove" => ok(tauri::async_runtime::block_on(studio_asset_remove(
            app.clone(),
            arg(body, "id")?,
            arg(body, "folder")?,
            arg(body, "file")?,
        ))?),
        "studio_pack_build" => ok(tauri::async_runtime::block_on(studio_pack_build(app.clone(), arg(body, "id")?))?),
        "studio_bundle_peek_upload" => ok(tauri::async_runtime::block_on(studio_bundle::studio_bundle_peek_upload(
            app.clone(),
            arg(body, "id")?,
            arg(body, "upload")?,
        ))?),
        "studio_bundle_import_upload" => ok(tauri::async_runtime::block_on(studio_bundle::studio_bundle_import_upload(
            app.clone(),
            arg(body, "id")?,
            arg(body, "upload")?,
            arg(body, "replace")?,
        ))?),
        "download_studio_assets" => ok(download_studio_assets(app.clone(), arg(body, "id")?)?),
        "item_names" => ok(lang::item_names(app.clone(), arg(body, "version")?, arg(body, "ids")?)?),
        "enchantment_names" => ok(lang::enchantment_names(app.clone(), arg(body, "version")?, arg(body, "ids")?)?),
        "cancel_pending_addon" => ok(cancel_pending_addon(app.clone(), arg(body, "id")?, arg(body, "entryId")?)?),
        "kill_java_process" => ok(kill_java_process(app.clone(), arg(body, "pid")?)?),
        "export_mrpack_download" => ok(tauri::async_runtime::block_on(export_mrpack_download(app.clone(), arg(body, "id")?))?),
        "add_addon_files" => {
            // From the web panel or Monolink: only files uploaded to this PC first, never any other path here.
            let paths: Vec<String> =
                arg::<Vec<String>>(body, "paths")?.iter().map(|one| remote_upload_path(app, one)).collect::<Result<_, _>>()?;
            let done = tauri::async_runtime::block_on(add_addon_files(app.clone(), app.state(), arg(body, "id")?, paths.clone()));
            for one in &paths {
                unstash(Path::new(one));
            }
            ok(done?)
        }
        "get_rcon_status" => ok(get_rcon_status(app.clone(), arg(body, "id")?)?),
        "get_server_ping" => ok(get_server_ping(app.clone(), app.state(), arg(body, "id")?)?),
        "list_addon_versions" => {
            ok(list_addon_versions(app.clone(), arg(body, "id")?, arg(body, "projectId")?, arg(body, "projectType")?)?)
        }
        "modrinth_project_detail" => ok(modrinth_project_detail(arg(body, "projectId")?)?),
        "list_player_spots" => ok(tauri::async_runtime::block_on(list_player_spots(app.clone(), arg(body, "id")?))?),
        "lookup_mojang_profile" => ok(lookup_mojang_profile(app.clone(), arg(body, "name")?)?),
        "plan_addon_install" => ok(plan_addon_install(app.clone(), arg(body, "id")?, arg(body, "projectId")?, arg(body, "installed")?)?),
        "read_player_nbt" => ok(tauri::async_runtime::block_on(read_player_nbt(app.clone(), arg(body, "id")?, arg(body, "uuid")?))?),
        "edit_player_item" => ok(tauri::async_runtime::block_on(edit_player_item(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "uuid")?,
            arg(body, "ender")?,
            arg(body, "slot")?,
            arg(body, "count")?,
        ))?),
        "render_map_tile" => ok(tauri::async_runtime::block_on(render_map_tile(
            app.clone(),
            arg(body, "id")?,
            arg(body, "realm")?,
            arg(body, "z")?,
            arg(body, "tx")?,
            arg(body, "ty")?,
            arg(body, "night")?,
        ))?),
        "world_map_bounds" => {
            ok(tauri::async_runtime::block_on(world_map_bounds(app.clone(), app.state(), arg(body, "id")?, arg(body, "realm")?))?)
        }
        "map_changes" => ok(tauri::async_runtime::block_on(map_changes(app.clone(), app.state(), arg(body, "id")?, arg(body, "realm")?))?),
        "search_addon_catalog" => ok(search_addon_catalog(
            app.clone(),
            arg(body, "id")?,
            arg(body, "query")?,
            arg(body, "page")?,
            arg(body, "categories")?,
            arg(body, "environment")?,
            arg(body, "versions")?,
            arg(body, "loader")?,
        )?),
        "check_addon_updates" => ok(tauri::async_runtime::block_on(check_addon_updates(app.clone(), arg(body, "id")?))?),
        "enrich_addons" => ok(tauri::async_runtime::block_on(enrich_addons(app.clone(), arg(body, "id")?))?),
        "apply_addon_updates" => {
            ok(tauri::async_runtime::block_on(apply_addon_updates(app.clone(), app.state(), arg(body, "id")?, arg(body, "fileNames")?))?)
        }
        "delete_addon" => ok(tauri::async_runtime::block_on(delete_addon(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "fileName")?,
            arg(body, "kind")?,
        ))?),
        "delete_backup" => ok(tauri::async_runtime::block_on(delete_backup(app.clone(), arg(body, "id")?, arg(body, "fileName")?))?),
        "delete_bookmark" => ok(delete_bookmark(app.clone(), arg(body, "id")?)?),
        "force_stop_instance" => ok(tauri::async_runtime::block_on(force_stop_instance(app.clone(), app.state(), arg(body, "id")?))?),
        "install_addon" => ok(tauri::async_runtime::block_on(install_addon(app.clone(), app.state(), arg(body, "id")?))?),
        "pumpkin_addon_allowed" => ok(tauri::async_runtime::block_on(pumpkin_addon_allowed(app.clone(), arg(body, "id")?))?),
        "set_pumpkin_addon_allowed" => {
            ok(tauri::async_runtime::block_on(set_pumpkin_addon_allowed(app.clone(), arg(body, "id")?, arg(body, "allow")?))?)
        }
        "pumpkin_refused_permissions" => {
            ok(tauri::async_runtime::block_on(pumpkin_refused_permissions(app.clone(), arg(body, "id")?, arg(body, "plugin")?))?)
        }
        "allow_pumpkin_plugin" => ok(tauri::async_runtime::block_on(allow_pumpkin_plugin(
            app.clone(),
            arg(body, "id")?,
            arg(body, "plugin")?,
            arg(body, "permissions")?,
        ))?),
        "install_addon_with_dependencies" => ok(install_addon_with_dependencies(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "projectId")?,
            arg(body, "dependencies")?,
            arg(body, "installed")?,
        )?),
        "install_modrinth" => ok(install_modrinth(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "projectId")?,
            arg(body, "projectType")?,
            arg(body, "versionId")?,
        )?),
        "remove_addon" => ok(tauri::async_runtime::block_on(remove_addon(app.clone(), app.state(), arg(body, "id")?))?),
        "rollback_addon_update" => ok(rollback_addon_update(app.clone(), app.state(), arg(body, "id")?, arg(body, "storedName")?)?),
        "send_rcon_command" => ok(send_rcon_command(app.clone(), arg(body, "id")?, arg(body, "command")?)?),
        "clear_server_icon" => ok(tauri::async_runtime::block_on(clear_server_icon(app.clone(), arg(body, "id")?))?),
        "share_log" => ok(tauri::async_runtime::block_on(share_log(app.clone(), arg(body, "id")?, arg(body, "fileName")?))?),
        "preview_share_log" => {
            ok(tauri::async_runtime::block_on(preview_share_log(app.clone(), arg(body, "id")?, arg(body, "fileName")?))?)
        }
        "set_start_with_app" => ok(set_start_with_app(app.clone(), arg(body, "id")?, arg(body, "on")?)?),
        "set_crash_restart" => ok(set_crash_restart(app.clone(), arg(body, "id")?, arg(body, "on")?)?),
        "rename_instance" => ok(rename_instance(app.clone(), app.state(), arg(body, "input")?)?),
        "set_server_group" => ok(set_server_group(app.clone(), app.state(), arg(body, "id")?, arg(body, "group")?)?),
        "rename_server_group" => ok(rename_server_group(app.clone(), arg(body, "from")?, arg(body, "to")?)?),
        "duplicate_instance" => ok(tauri::async_runtime::block_on(duplicate_instance(app.clone(), app.state(), arg(body, "id")?))?),
        "trash_instance" => ok(tauri::async_runtime::block_on(trash_instance(app.clone(), app.state(), arg(body, "id")?))?),
        "restore_trash_instance" => ok(restore_trash_instance(app.clone(), app.state(), arg(body, "id")?)?),
        "purge_trash_instance" => ok(purge_trash_instance(app.clone(), arg(body, "id")?)?),
        "empty_trash" => ok(empty_trash(app.clone())?),
        "reorder_instances" => ok(reorder_instances(app.clone(), arg(body, "ids")?)?),
        "upgrade_instance" => ok(tauri::async_runtime::block_on(upgrade_instance(app.clone(), app.state(), arg(body, "input")?))?),
        "rollback_last_upgrade" => ok(tauri::async_runtime::block_on(rollback_last_upgrade(app.clone(), app.state(), arg(body, "id")?))?),
        "restore_backup" => {
            ok(tauri::async_runtime::block_on(restore_backup(app.clone(), app.state(), arg(body, "id")?, arg(body, "fileName")?))?)
        }
        "proxy_network" => ok(tauri::async_runtime::block_on(proxy_network(app.clone(), arg(body, "id")?))?),
        "link_proxy" => {
            ok(tauri::async_runtime::block_on(link_proxy(app.clone(), arg(body, "id")?, arg(body, "servers")?, arg(body, "first")?))?)
        }
        "backup_contents" => ok(tauri::async_runtime::block_on(backup_contents(app.clone(), arg(body, "id")?, arg(body, "fileName")?))?),
        "backup_entry_text" => ok(tauri::async_runtime::block_on(backup_entry_text(
            app.clone(),
            arg(body, "id")?,
            arg(body, "fileName")?,
            arg(body, "path")?,
        ))?),
        "restore_backup_paths" => ok(tauri::async_runtime::block_on(restore_backup_paths(
            app.clone(),
            app.state(),
            arg(body, "id")?,
            arg(body, "fileName")?,
            arg(body, "paths")?,
        ))?),
        "set_app_settings" => ok(set_app_settings(app.clone(), arg(body, "settings")?)?),
        "playit_state" => ok(playit_state(app.clone())),
        "lan_ip" => ok(lan_ip()),
        "playit_expose" => ok(playit_expose(app.clone(), arg(body, "id")?)?),
        "playit_unexpose" => ok(playit_unexpose(app.clone(), arg(body, "id")?)?),
        "list_web_accounts" => ok(list_web_accounts(app.clone())),
        "save_web_account" => ok(save_web_account(
            app.clone(),
            app.state(),
            arg(body, "name")?,
            arg(body, "password")?,
            arg(body, "role")?,
            arg(body, "servers")?,
        )?),
        "delete_web_account" => ok(delete_web_account(app.clone(), app.state(), arg(body, "name")?)?),
        "role_catalog" => ok(role_catalog(app.clone())),
        "save_custom_role" => ok(save_custom_role(app.clone(), arg(body, "id")?, arg(body, "name")?, arg(body, "perms")?)?),
        "delete_custom_role" => ok(delete_custom_role(app.clone(), arg(body, "id")?, arg(body, "moveTo")?)?),
        "download_java" => ok(download_java(app.clone(), arg(body, "major")?)?),
        "remove_java_runtime" => ok(remove_java_runtime(app.clone(), arg(body, "path")?)?),
        "cancel_create_download" => ok(cancel_create_download(app.state(), arg(body, "operationId")?)?),
        _ => Err("Not available from the web.".into()),
    }
}

fn stash(app: &AppHandle, name: &str, data: &str) -> Result<PathBuf, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|_| "Could not read the uploaded file.".to_string())?;
    let path = stash_slot(app, name)?;
    fs::write(&path, bytes).map_err(|error| format!("Could not store the uploaded file: {error}"))?;
    Ok(path)
}

/// What an uploaded file is called afterwards (`<folder>/<name>` under the uploads folder), as the
/// web panel and Monolink hand it back.
pub(crate) fn upload_token(slot: &Path) -> String {
    slot.strip_prefix(slot.parent().and_then(|one| one.parent()).unwrap_or(slot))
        .map(|one| one.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

pub(crate) fn stash_slot(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    let dir = uploads_dir(app)?;
    let safe: String = Path::new(name)
        .file_name()
        .map(|one| one.to_string_lossy().to_string())
        .unwrap_or_else(|| "upload".into())
        .chars()
        .filter(|letter| !matches!(letter, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    sweep_uploads(&dir)?;
    // A random folder name: the token that names an upload can't be guessed by someone else.
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).map_err(|error| error.to_string())?;
    let folder = dir.join(random.iter().map(|byte| format!("{byte:02x}")).collect::<String>());
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    Ok(folder.join(safe))
}

/// Uploads (and downloaded modpacks) are only on their way somewhere: anything left for a day is
/// removed, and new ones are refused once the folder holds this much.
const UPLOADS_KEEP: Duration = Duration::from_secs(24 * 60 * 60);
const UPLOADS_MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;

fn sweep_uploads(dir: &Path) -> Result<(), String> {
    let mut total = 0u64;
    for entry in fs::read_dir(dir).map_err(|error| error.to_string())?.flatten() {
        let path = entry.path();
        let old =
            entry.metadata().and_then(|one| one.modified()).ok().and_then(|at| at.elapsed().ok()).is_some_and(|age| age > UPLOADS_KEEP);
        if old {
            let _ = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
            continue;
        }
        total += if path.is_dir() { folder_size(&path) } else { entry.metadata().map(|one| one.len()).unwrap_or(0) };
    }
    if total > UPLOADS_MAX_BYTES {
        return Err("The host has too many unfinished uploads (16 GB). Try again later, or ask the host to restart the app.".into());
    }
    Ok(())
}

fn folder_size(path: &Path) -> u64 {
    fs::read_dir(path)
        .map(|list| {
            list.flatten()
                .map(|one| if one.path().is_dir() { folder_size(&one.path()) } else { one.metadata().map(|meta| meta.len()).unwrap_or(0) })
                .sum()
        })
        .unwrap_or(0)
}

fn uploads_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("uploads");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

fn uploaded_path(app: &AppHandle, token: &str) -> Result<PathBuf, String> {
    let relative = paths::check_relative(token).map_err(|_| "The uploaded file list is malformed.".to_string())?;
    if relative.components().count() != 2 {
        return Err("The uploaded file list is malformed.".into());
    }
    let path = uploads_dir(app)?.join(relative);
    if !path.is_file() {
        return Err("The uploaded file could not be found. Please upload it again.".into());
    }
    Ok(path)
}

/// A file named from the web panel or Monolink: only something uploaded, or a modpack this app
/// downloaded (both in the uploads folder) — never any other path on this PC.
fn remote_upload_path(app: &AppHandle, text: &str) -> Result<String, String> {
    let text = text.trim();
    let outside = || "Upload the file first: only uploaded files can be used from here.".to_string();
    let path = if Path::new(text).is_absolute() { PathBuf::from(text) } else { uploaded_path(app, text).map_err(|_| outside())? };
    let real = fs::canonicalize(&path).map_err(|_| outside())?;
    let base = fs::canonicalize(uploads_dir(app)?).map_err(|error| error.to_string())?;
    if !real.starts_with(&base) || !real.is_file() {
        return Err(outside());
    }
    Ok(path.to_string_lossy().to_string())
}

fn unstash(path: &Path) {
    let _ = fs::remove_file(path);
    if let Some(folder) = path.parent() {
        let _ = fs::remove_dir(folder);
    }
}

#[tauri::command(async)]
#[specta::specta]
async fn set_server_icon_upload(app: AppHandle, id: String, name: String, data: String) -> Result<String, String> {
    let path = stash(&app, &name, &data)?;
    let done = set_server_icon(app.clone(), id, path.to_string_lossy().to_string()).await;
    unstash(&path);
    done
}

#[tauri::command(async)]
#[specta::specta]
async fn add_addon_upload(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    files: Vec<UploadFile>,
) -> Result<AddonAddResult, String> {
    let mut paths = Vec::new();
    for one in &files {
        paths.push(stash(&app, &one.name, &one.data)?);
    }
    let done = add_addon_files(app.clone(), state, id, paths.iter().map(|one| one.to_string_lossy().to_string()).collect()).await;
    for one in &paths {
        unstash(one);
    }
    done
}

#[tauri::command(async)]
#[specta::specta]
fn stash_upload(app: AppHandle, name: String, data: String) -> Result<String, String> {
    Ok(stash(&app, &name, &data)?.to_string_lossy().to_string())
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct UploadFile {
    name: String,
    data: String,
}

pub(crate) fn web_accounts(app: &AppHandle) -> Vec<web::auth::Account> {
    read_app_settings(app).web_accounts
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct WebPanelView {
    on: bool,
    port: u16,
    key: String,
    address: String,
    accounts: usize,
    viewers: usize,
    direct: bool,
    https: bool,
    /// The saved bind setting ("" = every network).
    bind: String,
    /// SHA-256 of the HTTPS certificate (AB:CD:…), empty before HTTPS was ever turned on.
    fingerprint: String,
}

/// This PC's address on the home network, for the Network tab.
#[tauri::command(async)]
#[specta::specta]
fn lan_ip() -> String {
    lan_address()
}

pub(crate) fn lan_address() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            socket.local_addr()
        })
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

#[tauri::command(async)]
#[specta::specta]
fn web_panel_state(app: AppHandle, running: State<'_, web::Running>) -> WebPanelView {
    let settings = read_app_settings(&app);
    let fingerprint = web::tls::fingerprint(&app);
    let held = running.lock().expect("web panel poisoned");
    let Some(panel) = held.as_ref() else {
        return WebPanelView {
            on: false,
            port: settings.web_panel_port,
            key: String::new(),
            address: String::new(),
            accounts: web_accounts(&app).len(),
            viewers: 0,
            direct: settings.web_direct,
            https: settings.web_https,
            bind: settings.web_bind,
            fingerprint,
        };
    };
    let host = if panel.bound().is_unspecified() { lan_address() } else { panel.bound().to_string() };
    let scheme = if panel.https() { "https" } else { "http" };
    WebPanelView {
        on: true,
        port: panel.port(),
        key: String::new(),
        address: format!("{scheme}://{host}:{}/", panel.port()),
        accounts: web_accounts(&app).len(),
        viewers: panel.viewers(),
        direct: settings.web_direct,
        https: panel.https(),
        bind: settings.web_bind,
        fingerprint,
    }
}

/// This PC's network cards, for "where the web panel listens".
#[tauri::command(async)]
#[specta::specta]
fn web_bind_choices() -> Vec<web::BindChoice> {
    web::bind_choices()
}

/// Starts the panel again with the saved settings when it is on (HTTPS or the network changed).
fn restart_web_panel(app: &AppHandle) -> Result<(), String> {
    let running = app.state::<web::Running>().inner().clone();
    let port = match running.lock().expect("web panel poisoned").as_ref() {
        Some(panel) => panel.port(),
        None => return Ok(()),
    };
    web::stop(app);
    web::start(app, port).map(|_| ())
}

/// HTTPS for the web panel. Turning it on makes the certificate (if there is none or it is old).
#[tauri::command(async)]
#[specta::specta]
fn set_web_https(app: AppHandle, running: State<'_, web::Running>, on: bool) -> Result<WebPanelView, String> {
    if on {
        web::tls::ensure(&app)?;
    }
    let mut settings = read_app_settings(&app);
    settings.web_https = on;
    write_app_settings(&app, &settings)?;
    restart_web_panel(&app)?;
    Ok(web_panel_state(app.clone(), running))
}

/// Where the web panel listens: "" every network, "127.0.0.1" this PC only, or one network card's IPv4.
#[tauri::command(async)]
#[specta::specta]
fn set_web_bind(app: AppHandle, running: State<'_, web::Running>, bind: String) -> Result<WebPanelView, String> {
    let bind = bind.trim().to_string();
    web::bind_address(&bind)?;
    let mut settings = read_app_settings(&app);
    settings.web_bind = bind;
    write_app_settings(&app, &settings)?;
    restart_web_panel(&app)?;
    Ok(web_panel_state(app.clone(), running))
}

#[tauri::command(async)]
#[specta::specta]
fn set_web_direct(app: AppHandle, running: State<'_, web::Running>, on: bool) -> Result<WebPanelView, String> {
    let mut settings = read_app_settings(&app);
    settings.web_direct = on;
    store::write(&app_settings_path(&app)?, &serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?)
        .map_err(|error| format!("Could not save settings: {error}"))?;
    Ok(web_panel_state(app.clone(), running))
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct WebAccountView {
    name: String,
    role: String,
    servers: Vec<String>,
}

#[tauri::command(async)]
#[specta::specta]
fn list_web_accounts(app: AppHandle) -> Vec<WebAccountView> {
    web_accounts(&app).into_iter().map(|one| WebAccountView { name: one.name, role: one.role, servers: one.servers }).collect()
}

#[tauri::command(async)]
#[specta::specta]
fn save_web_account(
    app: AppHandle,
    running: State<'_, web::Running>,
    name: String,
    password: String,
    role: String,
    servers: Vec<String>,
) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Enter an account name.".into());
    }
    let password = password.trim();
    let mut settings = read_app_settings(&app);
    if !web::auth::is_role(&role) && !web::auth::is_server_role(&role, &settings.custom_roles) {
        return Err("Pick Admin, Manager, Viewer or one of your own roles.".into());
    }
    let found = settings.web_accounts.iter().position(|one| one.name == name);
    if !password.is_empty() && password.chars().count() < 8 {
        return Err("Passwords must be at least 8 characters.".into());
    }
    if password.is_empty() && found.is_none() {
        return Err("A new account needs a password.".into());
    }
    let hash = if password.is_empty() {
        settings.web_accounts[found.expect("Would have been rejected above if missing")].hash.clone()
    } else {
        web::auth::Guard::hash(password)?
    };
    let servers = if role == web::auth::ADMIN { Vec::new() } else { servers };
    let account = web::auth::Account { name, hash, role, servers };
    match found {
        Some(at) => settings.web_accounts[at] = account,
        None => settings.web_accounts.push(account),
    }
    save_web_accounts(&app, &running, settings)
}

#[tauri::command(async)]
#[specta::specta]
fn delete_web_account(app: AppHandle, running: State<'_, web::Running>, name: String) -> Result<(), String> {
    let mut settings = read_app_settings(&app);
    settings.web_accounts.retain(|one| one.name != name);
    save_web_accounts(&app, &running, settings)
}

/// A role as the screens show it: built-in ones first, then the host's own, with how many people have it.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct RoleView {
    id: String,
    name: String,
    builtin: bool,
    perms: Vec<String>,
    /// Web accounts and teammates (on this PC) with this role.
    people: u32,
}

/// Everyone on this PC who has `role`: web accounts and teammates given it here.
fn role_users(settings: &AppSettings, role: &str) -> u32 {
    let web = settings.web_accounts.iter().filter(|one| one.role == role).count();
    let team = settings.monolink_teams.iter().flat_map(|one| &one.people).filter(|one| one.role == role).count();
    (web + team) as u32
}

/// Gives everyone (and every open invite) with role `from` the role `to` instead.
fn move_role(settings: &mut AppSettings, from: &str, to: &str) {
    settings.web_accounts.iter_mut().filter(|one| one.role == from).for_each(|one| one.role = to.to_string());
    settings.web_invites.iter_mut().filter(|one| one.role == from).for_each(|one| one.role = to.to_string());
    settings
        .monolink_teams
        .iter_mut()
        .flat_map(|one| one.people.iter_mut())
        .filter(|one| one.role == from)
        .for_each(|one| one.role = to.to_string());
}

fn role_views(settings: &AppSettings) -> Vec<RoleView> {
    let builtin = [web::auth::ADMIN, web::auth::MANAGER, web::auth::VIEWER].into_iter().map(|id| RoleView {
        id: id.into(),
        name: id.into(),
        builtin: true,
        perms: web::auth::perms_of(id, &[]),
        people: role_users(settings, id),
    });
    let custom = settings.custom_roles.iter().map(|one| RoleView {
        id: one.id.clone(),
        name: one.name.clone(),
        builtin: false,
        perms: web::auth::perms_of(&one.id, &settings.custom_roles),
        people: role_users(settings, &one.id),
    });
    builtin.chain(custom).collect()
}

/// The roles people can be given: Admin, Manager, Viewer and the host's own.
#[tauri::command(async)]
#[specta::specta]
fn role_catalog(app: AppHandle) -> Vec<RoleView> {
    role_views(&read_app_settings(&app))
}

/// Makes a role (empty `id`) or renames one and changes its permissions. Takes effect on the next request.
#[tauri::command(async)]
#[specta::specta]
fn save_custom_role(app: AppHandle, id: String, name: String, perms: Vec<String>) -> Result<Vec<RoleView>, String> {
    let mut settings = read_app_settings(&app);
    let id = if id.is_empty() {
        if settings.custom_roles.len() >= 50 {
            return Err("You already have 50 roles. Delete some first.".into());
        }
        let mut random = [0u8; 6];
        getrandom::fill(&mut random).map_err(|error| format!("Could not make a role: {error}"))?;
        format!("{}{}", web::auth::CUSTOM_PREFIX, relay_proto::hex(&random))
    } else if settings.custom_roles.iter().any(|one| one.id == id) {
        id
    } else {
        return Err("That role no longer exists.".into());
    };
    let role = web::auth::clean_role(&id, &name, &perms, &settings.custom_roles)?;
    match settings.custom_roles.iter_mut().find(|one| one.id == id) {
        Some(found) => *found = role,
        None => settings.custom_roles.push(role),
    }
    write_app_settings(&app, &settings)?;
    Ok(role_views(&settings))
}

/// Deletes one of the host's roles. While people have it, `move_to` (Manager, Viewer or another
/// custom role) says what they get instead; without it nothing is deleted.
#[tauri::command(async)]
#[specta::specta]
fn delete_custom_role(app: AppHandle, id: String, move_to: Option<String>) -> Result<Vec<RoleView>, String> {
    let mut settings = read_app_settings(&app);
    if !settings.custom_roles.iter().any(|one| one.id == id) {
        return Err("That role no longer exists.".into());
    }
    let open_invites = settings.web_invites.iter().any(|one| one.role == id);
    if role_users(&settings, &id) > 0 || open_invites {
        let Some(to) = move_to.filter(|to| *to != id && web::auth::is_server_role(to, &settings.custom_roles)) else {
            return Err("People still have this role. Pick a role to move them to first.".into());
        };
        move_role(&mut settings, &id, &to);
    }
    settings.custom_roles.retain(|one| one.id != id);
    write_app_settings(&app, &settings)?;
    Ok(role_views(&settings))
}

pub(crate) fn write_app_settings(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    store::write(&app_settings_path(app)?, &serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?)
        .map_err(|error| format!("Could not save settings: {error}"))
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct InviteLink {
    id: String,
    /// The whole link to send. It works once and is shown only now.
    link: String,
    role: String,
    expires: u64,
}

/// Where invited people reach the web panel: this PC on the home network.
fn web_panel_base(running: &State<'_, web::Running>) -> Result<String, String> {
    let port = running
        .lock()
        .expect("web panel poisoned")
        .as_ref()
        .map(|one| one.port())
        .ok_or("Turn on the web panel in Settings first. Invites open it.")?;
    Ok(format!("http://{}:{port}/", lan_address()))
}

/// A one-time link that lets someone make their own account for this server, as Manager or Viewer.
#[tauri::command(async)]
#[specta::specta]
fn create_web_invite(app: AppHandle, running: State<'_, web::Running>, id: String, role: String) -> Result<InviteLink, String> {
    let instance = instance_by_id(&app, &id)?;
    let base = web_panel_base(&running)?;
    let now = unix_now();
    let mut settings = read_app_settings(&app);
    let (token, invite) = web::auth::new_invite(&id, &role, now, &settings.custom_roles)?;
    let shown = role_name(&role, &settings.custom_roles);
    settings.web_invites.retain(|one| one.expires > now);
    if settings.web_invites.iter().filter(|one| one.server == id).count() >= 20 {
        return Err("This server already has 20 open invites. Remove some first.".into());
    }
    settings.web_invites.push(invite.clone());
    write_app_settings(&app, &settings)?;
    record_audit(&app, &id, "People", "Invite created", &format!("{shown} invite for {}", instance.name));
    Ok(InviteLink { id: invite.id, link: format!("{base}#invite={token}"), role, expires: invite.expires })
}

/// Open invites for a server (without their tokens).
#[tauri::command(async)]
#[specta::specta]
fn list_web_invites(app: AppHandle, id: String) -> Result<Vec<web::auth::Invite>, String> {
    instance_by_id(&app, &id)?;
    let now = unix_now();
    Ok(read_app_settings(&app)
        .web_invites
        .into_iter()
        .filter(|one| one.server == id && one.expires > now)
        .map(|mut one| {
            one.digest.clear();
            one
        })
        .collect())
}

#[tauri::command(async)]
#[specta::specta]
fn revoke_web_invite(app: AppHandle, invite: String) -> Result<(), String> {
    let mut settings = read_app_settings(&app);
    settings.web_invites.retain(|one| one.id != invite);
    write_app_settings(&app, &settings)
}

/// Someone who can open a server from the web panel.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ServerPerson {
    name: String,
    role: String,
    online: bool,
    /// Online and looking at this server right now.
    here: bool,
}

#[tauri::command(async)]
#[specta::specta]
fn server_people(app: AppHandle, running: State<'_, web::Running>, id: String) -> Result<Vec<ServerPerson>, String> {
    instance_by_id(&app, &id)?;
    let online = running.lock().expect("web panel poisoned").as_ref().map(|one| one.online()).unwrap_or_default();
    let mut people: Vec<ServerPerson> = web_accounts(&app)
        .into_iter()
        .filter(|one| one.may(&id))
        .map(|one| {
            let now = online.iter().find(|row| row.name == one.name);
            ServerPerson { online: now.is_some(), here: now.is_some_and(|row| row.server == id), name: one.name, role: one.role }
        })
        .collect();
    people.sort_by(|a, b| b.here.cmp(&a.here).then(b.online.cmp(&a.online)).then(a.name.cmp(&b.name)));
    Ok(people)
}

/// Takes this server away from an account. An account left with no servers is deleted. Admins can't be removed here.
#[tauri::command(async)]
#[specta::specta]
fn remove_server_access(app: AppHandle, running: State<'_, web::Running>, id: String, name: String) -> Result<(), String> {
    let mut settings = read_app_settings(&app);
    let Some(at) = settings.web_accounts.iter().position(|one| one.name == name) else {
        return Err("That account no longer exists.".into());
    };
    if settings.web_accounts[at].is_admin() {
        return Err("Admins can open every server. Change or delete the account in Settings.".into());
    }
    settings.web_accounts[at].servers.retain(|one| one != &id);
    if settings.web_accounts[at].servers.is_empty() {
        settings.web_accounts.remove(at);
    }
    record_audit(&app, &id, "People", "Access removed", &name);
    save_web_accounts(&app, &running, settings)
}

/// Everyone using the web panel right now.
#[tauri::command(async)]
#[specta::specta]
fn web_online(running: State<'_, web::Running>) -> Vec<web::auth::Online> {
    running.lock().expect("web panel poisoned").as_ref().map(|one| one.online()).unwrap_or_default()
}

/// Uses an invite from the web panel's sign-in page. Returns the account name to sign in as.
pub(crate) fn accept_web_invite(app: &AppHandle, token: &str, name: &str, password: &str) -> Result<String, String> {
    static ACCEPTING: Mutex<()> = Mutex::new(());
    let _held = ACCEPTING.lock().expect("invite lock poisoned");
    let now = unix_now();
    let mut settings = read_app_settings(app);
    let invite = web::auth::find_invite(&settings.web_invites, token, now)
        .cloned()
        .ok_or("This invite link has expired or was already used. Ask for a new one.")?;
    if instance_by_id(app, &invite.server).is_err() {
        return Err("The server this invite was for is gone.".into());
    }
    web::auth::accept_invite(&mut settings.web_accounts, &invite, name, password, &settings.custom_roles)?;
    settings.web_invites.retain(|one| one.id != invite.id && one.expires > now);
    // Only rights were added, so nobody else has to sign in again.
    write_app_settings(app, &settings)?;
    record_audit(
        app,
        &invite.server,
        "People",
        "Joined",
        &format!("{} joined as {}", name.trim(), role_name(&invite.role, &settings.custom_roles)),
    );
    Ok(name.trim().to_string())
}

/// What an invite link is for, so the sign-in page can say so before anyone types a password.
pub(crate) fn web_invite_info(app: &AppHandle, token: &str) -> Option<serde_json::Value> {
    let settings = read_app_settings(app);
    let invite = web::auth::find_invite(&settings.web_invites, token, unix_now())?;
    let server = instance_by_id(app, &invite.server).ok()?;
    Some(serde_json::json!({ "server": server.name, "role": invite.role, "roleName": role_name(&invite.role, &settings.custom_roles) }))
}

/// The servers a web request is about: from `id`, `input`, `task`, or a task or bookmark it names.
pub(crate) fn web_servers_of(app: &AppHandle, command: &str, body: &serde_json::Value) -> Vec<String> {
    let text = |value: &serde_json::Value| value.as_str().filter(|one| !one.is_empty()).map(str::to_string);
    let id = body.get("id").and_then(text);
    match command {
        "delete_task" | "run_task_now" => {
            let task = read_tasks(app).into_iter().find(|one| Some(&one.id) == id.as_ref());
            return task.map(|one| vec![one.server_id]).unwrap_or_else(|| vec![String::new()]);
        }
        "delete_bookmark" => {
            let mark = load_bookmarks(app).into_iter().find(|one| Some(&one.id) == id.as_ref());
            return mark.map(|one| vec![one.server_id]).unwrap_or_else(|| vec![String::new()]);
        }
        "save_task" => {
            let task = body.get("task");
            let mut out: Vec<String> = task.and_then(|one| one.get("serverId")).and_then(text).into_iter().collect();
            let existing = task.and_then(|one| one.get("id")).and_then(text);
            if let Some(found) = existing.and_then(|wanted| read_tasks(app).into_iter().find(|one| one.id == wanted)) {
                out.push(found.server_id);
            }
            if out.is_empty() {
                out.push(String::new());
            }
            return out;
        }
        _ => {}
    }
    if let Some(input) = body.get("input") {
        if let Some(server) = input.get("serverId").and_then(text).or_else(|| input.get("id").and_then(text)) {
            return vec![server];
        }
    }
    id.into_iter().collect()
}

/// Each server's id and port, for matching playit tunnels to servers.
pub(crate) fn web_server_ports(app: &AppHandle) -> Vec<(String, u16)> {
    load_instances(app).unwrap_or_default().into_iter().map(|one| (one.id, one.port)).collect()
}

/// Audit-log line for a change made from the web panel, so everyone sees who did what.
pub(crate) fn web_audit(app: &AppHandle, server: &str, who: &str, command: &str) {
    if matches!(command, "stash_upload" | "set_server_memo" | "search_modpacks" | "preview_mrpack" | "preview_cfpack") {
        return;
    }
    record_audit(app, server, "Web", &command.replace('_', " "), &format!("by {who}"));
}

fn save_web_accounts(app: &AppHandle, running: &State<'_, web::Running>, settings: AppSettings) -> Result<(), String> {
    store::write(&app_settings_path(app)?, &serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?)
        .map_err(|error| format!("Could not save settings: {error}"))?;
    if let Some(panel) = running.lock().expect("web panel poisoned").as_ref() {
        panel.forget_everyone();
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct PanelProbe {
    servers: Vec<panel::PanelServer>,
    kind: panel::PanelKind,
    daemon: String,
    permissions: Vec<String>,
    username: String,
    admin: bool,
    account_key: bool,
}

#[tauri::command(async)]
#[specta::specta]
fn list_panel_hosts(app: AppHandle) -> Result<Vec<panel::PanelHostView>, String> {
    panel::list_hosts(&app)
}

#[tauri::command(async)]
#[specta::specta]
async fn save_panel_host(
    app: AppHandle,
    mut host: panel::PanelHost,
    key: Option<String>,
    app_key: Option<String>,
    detect: bool,
) -> Result<(), String> {
    if detect {
        let (saved_key, saved_app) = panel::saved_keys(&app, &host.id);
        let filled = |given: &Option<String>, saved: Option<String>| given.clone().filter(|one| !one.trim().is_empty()).or(saved);
        if let Ok(api) = panel::Api::new(&host.url, filled(&key, saved_key).as_deref(), filled(&app_key, saved_app).as_deref(), host.role) {
            let servers = if api.has_account_key() { api.servers().await.unwrap_or_default() } else { Vec::new() };
            host.kind = api.detect_kind(&servers).await;
        }
    }
    panel::save_host(&app, host, key, app_key)
}

#[tauri::command(async)]
#[specta::specta]
fn delete_panel_host(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<u32, String> {
    panel::delete_host(&app, &id)?;
    let gone = drop_servers_of(&app, |one| one.panel_host == id)?;
    let mut live = state.panel_live.lock().expect("panel state poisoned");
    live.retain(|key, _| !gone.contains(key));
    Ok(gone.len() as u32)
}

fn drop_servers_of(app: &AppHandle, matches: impl Fn(&ServerInstance) -> bool) -> Result<Vec<String>, String> {
    let _store = lock_instance_store();
    let mut instances = load_instances(app)?;
    let gone: Vec<String> = instances.iter().filter(|one| matches(one)).map(|one| one.id.clone()).collect();
    if gone.is_empty() {
        return Ok(gone);
    }
    instances.retain(|one| !matches(one));
    save_instances(app, &instances)?;
    for id in &gone {
        record_audit(
            app,
            id,
            "Server",
            "Remove from list (connection deleted)",
            "Removed from the list because the connection was deleted. Files are untouched.",
        );
    }
    Ok(gone)
}

#[tauri::command(async)]
#[specta::specta]
async fn test_panel_host(
    app: AppHandle,
    url: String,
    key: Option<String>,
    app_key: Option<String>,
    role: panel::PanelRole,
    host_id: Option<String>,
) -> Result<PanelProbe, String> {
    let (saved_key, saved_app) = host_id.map(|id| panel::saved_keys(&app, &id)).unwrap_or_default();
    let filled = |given: Option<String>, saved: Option<String>| given.filter(|one| !one.trim().is_empty()).or(saved);
    let (key, app_key) = (filled(key, saved_key), filled(app_key, saved_app));
    let api = panel::Api::new(&url, key.as_deref(), app_key.as_deref(), role)?;
    if role == panel::PanelRole::Admin {
        api.check_application().await?;
    }
    let account = if api.has_account_key() { Some(api.account().await?) } else { None };
    if let Some(account) = &account {
        if role == panel::PanelRole::Admin && !account.admin {
            return Err(format!(
                "The account key's owner ({}) isn't a panel admin. Use an account key made by an admin account, or leave it empty.",
                if account.username.is_empty() { "?" } else { &account.username }
            ));
        }
    }
    let servers = api.servers().await?;
    let kind = api.detect_kind(&servers).await;
    let api = api.with_kind(kind);
    let (daemon, permissions) = match servers.first().filter(|_| api.has_account_key()) {
        Some(one) => (api.daemon(&one.identifier).await.unwrap_or_default(), api.permissions(&one.identifier).await.unwrap_or_default()),
        None => (String::new(), Vec::new()),
    };
    Ok(PanelProbe {
        servers,
        kind,
        daemon,
        permissions,
        account_key: account.is_some(),
        username: account.as_ref().map(|one| one.username.clone()).unwrap_or_default(),
        admin: account.is_some_and(|one| one.admin),
    })
}

#[tauri::command(async)]
#[specta::specta]
async fn panel_create_options(app: AppHandle, host_id: String) -> Result<panel::PanelCreateOptions, String> {
    panel::api(&app, &host_id)?.create_options().await
}

#[tauri::command(async)]
#[specta::specta]
async fn panel_free_allocations(app: AppHandle, host_id: String, node: u32) -> Result<Vec<panel::PanelAllocation>, String> {
    panel::api(&app, &host_id)?.free_allocations(node).await
}

#[tauri::command(async)]
#[specta::specta]
async fn panel_create_server(app: AppHandle, host_id: String, server: panel::PanelNewServer) -> Result<String, String> {
    panel::api(&app, &host_id)?.create_server(&server).await
}

#[tauri::command(async)]
#[specta::specta]
async fn panel_startup(app: AppHandle, id: String) -> Result<panel::PanelStartup, String> {
    let (_, api, target) = panel_instance(&app, &id)?.ok_or("Not a panel server.")?;
    api.startup(&target).await
}

#[tauri::command(async)]
#[specta::specta]
async fn set_panel_variable(app: AppHandle, id: String, env: String, value: String) -> Result<(), String> {
    let (_, api, target) = panel_instance(&app, &id)?.ok_or("Not a panel server.")?;
    api.set_variable(&target, &env, &value).await?;
    record_audit(&app, &id, "Settings", "Change startup variable (panel)", &format!("{env}={value}"));
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
fn list_ssh_hosts(app: AppHandle) -> Result<Vec<remote::SshHostView>, String> {
    remote::list_hosts(&app)
}

#[tauri::command(async)]
#[specta::specta]
async fn probe_ssh_host(app: AppHandle, address: String, port: u16) -> Result<remote::HostKey, String> {
    remote::probe(&app, address.trim(), port).await
}

#[tauri::command(async)]
#[specta::specta]
fn save_ssh_host(app: AppHandle, host: remote::SshHost, secret: Option<String>) -> Result<(), String> {
    remote::save_host(&app, host, secret)
}

#[tauri::command(async)]
#[specta::specta]
async fn ensure_ssh_root(app: AppHandle, host_id: String) -> Result<String, String> {
    remote::server_root(&app, &host_id).await
}

#[tauri::command(async)]
#[specta::specta]
fn delete_ssh_host(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<u32, String> {
    remote::delete_host(&app, &id)?;
    let gone = drop_servers_of(&app, |one| one.remote_host == id)?;
    state.remote_running.lock().expect("remote state poisoned").retain(|key, _| !gone.contains(key));
    state.remote_starting.lock().expect("remote state poisoned").retain(|key, _| !gone.contains(key));
    Ok(gone.len() as u32)
}

#[tauri::command(async)]
#[specta::specta]
async fn scan_remote_servers(app: AppHandle, host_id: String) -> Result<Vec<remote::FoundFolder>, String> {
    remote::scan(&app, &host_id).await
}

#[tauri::command(async)]
#[specta::specta]
async fn browse_remote_dir(app: AppHandle, host_id: String, path: String) -> Result<Vec<remote::RemoteEntry>, String> {
    remote::browse(&app, &host_id, &path).await
}

#[tauri::command(async)]
#[specta::specta]
async fn import_remote_instance(
    app: AppHandle,
    state: State<'_, AppState>,
    host_id: String,
    folder: String,
) -> Result<InstanceView, String> {
    let host = remote::host(&app, &host_id)?;
    let folder = folder.trim().trim_end_matches('/').to_string();
    if folder.is_empty() {
        return Err("Please choose a server folder.".into());
    }
    let found = remote::detect(&app, &host_id, &folder).await?;

    let instance = {
        let _store = lock_instance_store();
        let mut instances = load_instances(&app)?;
        if instances.iter().any(|one| one.remote_host == host_id && one.path == folder) {
            return Err("This folder is already registered.".into());
        }
        let name = folder.rsplit('/').find(|bit| !bit.is_empty()).unwrap_or("Unnamed server").to_string();
        let instance = ServerInstance {
            id: instance_id(),
            name,
            path: folder,
            core: found.core,
            version: if found.version.is_empty() { "Needs review".into() } else { found.version },
            launch_mode: default_launch_mode(),
            server_jar: (!found.jar.is_empty()).then(|| found.jar.clone()),
            java_path: "java".into(),
            max_memory_mib: 4096,
            min_memory_mib: 4096,
            jvm_args: String::new(),
            port: found.port,
            sort_order: 0,
            group: String::new(),
            start_with_app: false,
            crash_restart: false,
            core_build: String::new(),
            created_at: unix_now(),
            last_run_at: 0,
            custom_command: String::new(),
            start_script: String::new(),
            start_script_hash: String::new(),
            remote_host: host.id,
            linked: true,
            panel_host: String::new(),
            panel_server: String::new(),
            panel_daemon: String::new(),
            named: false,
        };
        instances.push(instance.clone());
        save_instances(&app, &instances)?;
        instance
    };
    adopt_remote(&app, &state, &instance).await;
    Ok(instance_view(instance, &state))
}

fn game_version_of(text: &str) -> String {
    text.trim().split(['-', '@', '+', ' ']).next().unwrap_or_default().to_string()
}

fn is_game_version(text: &str) -> bool {
    let text = text.trim();
    text.contains('.') && text.starts_with(|c: char| c.is_ascii_digit()) && text.chars().all(|c| c.is_ascii_digit() || c == '.')
}

async fn panel_new_instance(api: &panel::Api, host_id: &str, row: &panel::PanelServer) -> ServerInstance {
    let ident = row.identifier.clone();
    let daemon = api.daemon(&ident).await.unwrap_or_else(|_| api.kind().fallback_daemon().into());
    let target = panel::Target { ident: ident.clone(), daemon: daemon.clone() };
    let (core, version, jar) = panel_detect(api, &target).await;
    ServerInstance {
        id: instance_id(),
        name: row.name.clone(),
        path: format!("panel:{ident}"),
        core,
        version,
        launch_mode: default_launch_mode(),
        server_jar: (!jar.is_empty()).then_some(jar),
        java_path: "java".into(),
        max_memory_mib: row.memory_limit_mib,
        min_memory_mib: row.memory_limit_mib,
        jvm_args: String::new(),
        port: row.port,
        sort_order: 0,
        group: String::new(),
        start_with_app: false,
        crash_restart: false,
        core_build: String::new(),
        created_at: unix_now(),
        last_run_at: 0,
        custom_command: String::new(),
        start_script: String::new(),
        start_script_hash: String::new(),
        remote_host: String::new(),
        linked: true,
        panel_host: host_id.to_string(),
        panel_server: ident,
        panel_daemon: daemon,
        named: false,
    }
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PanelSync {
    pub added: u32,
    pub removed: u32,
    pub failed: Vec<String>,
}

async fn sync_panel_all(app: &AppHandle, state: &AppState) -> PanelSync {
    let mut out = PanelSync::default();
    let Ok(hosts) = panel::load_hosts(app) else { return out };
    for host in hosts {
        let Ok(api) = panel::api(app, &host.id) else { continue };
        if !api.has_account_key() {
            continue;
        }
        let listed = match api.my_servers().await {
            Ok(list) => list,
            Err(error) => {
                out.failed.push(format!("{}: {error}", host.name));
                continue;
            }
        };
        let wanted: Vec<&panel::PanelServer> = listed.iter().filter(|one| host.role != panel::PanelRole::Admin || one.owner).collect();
        let have: Vec<ServerInstance> =
            load_instances(app).unwrap_or_default().into_iter().filter(|one| one.panel_host == host.id).collect();
        let mut made = Vec::new();
        for row in &wanted {
            if have.iter().any(|one| one.panel_server == row.identifier) || host.hidden.contains(&row.identifier) {
                continue;
            }
            made.push(panel_new_instance(&api, &host.id, row).await);
        }
        let mut learned: Vec<(String, String, String, String)> = Vec::new();
        for one in have.iter().filter(|one| {
            one.core == sniff::UNKNOWN
                || if pumpkin::is_pumpkin(&one.core) { one.version == "Needs review" } else { !is_game_version(&one.version) }
        }) {
            let (core, version, jar) = panel_detect(&api, &panel_live::target_of(one)).await;
            if core != one.core || version != one.version {
                learned.push((one.id.clone(), core, version, jar));
            }
        }
        let gone: Vec<String> =
            have.iter().filter(|one| !wanted.iter().any(|row| row.identifier == one.panel_server)).map(|one| one.id.clone()).collect();
        if made.is_empty()
            && gone.is_empty()
            && learned.is_empty()
            && !have.iter().any(|one| wanted.iter().any(|row| row.identifier == one.panel_server && row.name != one.name))
        {
            continue;
        }
        let _store = lock_instance_store();
        let Ok(mut instances) = load_instances(app) else { continue };
        instances.retain(|one| !gone.contains(&one.id));
        for one in instances.iter_mut().filter(|one| one.panel_host == host.id) {
            if let Some(row) = wanted.iter().find(|row| row.identifier == one.panel_server) {
                if !row.name.is_empty() {
                    one.name = row.name.clone();
                }
            }
            if let Some((_, core, version, jar)) = learned.iter().find(|each| each.0 == one.id) {
                if core != sniff::UNKNOWN {
                    one.core = core.clone();
                }
                if is_game_version(version) || (pumpkin::is_pumpkin(core) && version != "Needs review") {
                    one.version = version.clone();
                }
                if !jar.is_empty() {
                    one.server_jar = Some(jar.clone());
                }
            }
        }
        out.added += made.len() as u32;
        out.removed += gone.len() as u32;
        instances.extend(made);
        if save_instances(app, &instances).is_ok() {
            for id in &gone {
                record_audit(app, id, "Server", "Removed from list (gone from panel)", &host.name);
            }
        }
    }
    if out.added > 0 {
        state.panel_kick.notify_one();
    }
    out
}

#[tauri::command(async)]
#[specta::specta]
async fn sync_panel_servers(app: AppHandle, state: State<'_, AppState>) -> Result<PanelSync, String> {
    Ok(sync_panel_all(&app, &state).await)
}

fn drop_orphan_servers(app: &AppHandle) {
    let (Ok(ssh), Ok(panels)) = (remote::load_hosts(app), panel::load_hosts(app)) else { return };
    let ssh: Vec<String> = ssh.into_iter().map(|one| one.id).collect();
    let panels: Vec<String> = panels.into_iter().map(|one| one.id).collect();
    let orphan = |one: &ServerInstance| {
        (!one.remote_host.is_empty() && !ssh.contains(&one.remote_host))
            || (!one.panel_host.is_empty() && !panels.contains(&one.panel_host))
            || (one.panel_host.is_empty() && one.remote_host.is_empty() && one.path.starts_with("panel:"))
    };
    let _ = drop_servers_of(app, orphan);
}

fn start_panel_sync(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            drop_orphan_servers(&app);
            let state = app.state::<AppState>();
            sync_panel_all(&app, &state).await;
            tokio::time::sleep(Duration::from_secs(5 * 60)).await;
        }
    });
}

#[tauri::command(async)]
#[specta::specta]
async fn panel_unhide(app: AppHandle, state: State<'_, AppState>, host_id: String, identifier: String) -> Result<PanelSync, String> {
    panel::unhide_server(&app, &host_id, &identifier)?;
    Ok(sync_panel_all(&app, &state).await)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_info(app: AppHandle, server: String) -> Result<claude_agent::ClaudeInfo, String> {
    claude_agent::info(&app, &server)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_open_folder(app: AppHandle, server: String) -> Result<(), String> {
    claude_agent::open_folder(&app, &server)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_accounts(app: AppHandle, server: String) -> claude_accounts::ClaudeAccounts {
    claude_accounts::list(&app, &server)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_account_add(app: AppHandle, name: String) -> Result<String, String> {
    claude_accounts::add(&app, &name)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_account_rename(app: AppHandle, id: String, name: String) -> Result<(), String> {
    claude_accounts::rename(&app, &id, &name)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_account_remove(app: AppHandle, id: String) -> Result<(), String> {
    claude_accounts::remove(&app, &id)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_account_logout(app: AppHandle, id: String) -> Result<(), String> {
    claude_accounts::logout(&app, &id)?;
    claude_chat::close_all();
    Ok(())
}

#[tauri::command]
#[specta::specta]
async fn claude_account_login(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || claude_accounts::login(&app, &id)).await.map_err(|error| error.to_string())?
}

#[tauri::command(async)]
#[specta::specta]
fn claude_account_login_code(code: String) -> Result<(), String> {
    claude_accounts::login_code(&code)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_account_login_cancel() {
    claude_accounts::login_cancel()
}

#[tauri::command(async)]
#[specta::specta]
fn claude_account_use(app: AppHandle, server: String, id: String) -> Result<(), String> {
    claude_accounts::use_for(&app, &server, &id)?;
    claude_chat::close_server(&server);
    acp::close_server(&server);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
fn codex_models(app: AppHandle) -> Vec<codex_chat::CodexModel> {
    codex_chat::models(&app)
}

#[tauri::command]
#[specta::specta]
async fn codex_login(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || codex_chat::login(&app)).await.map_err(|error| error.to_string())?
}

#[tauri::command(async)]
#[specta::specta]
fn claude_chat_state(server: String, engine: String, session: String) -> claude_chat::ChatState {
    if engine == "codex" {
        codex_chat::state(&server)
    } else {
        claude_chat::state(&server, &session)
    }
}

#[tauri::command(async)]
#[specta::specta]
fn agent_skills(app: AppHandle, server: String) -> Result<Vec<skills::AgentSkill>, String> {
    skills::list(&app, &server)
}

#[tauri::command(async)]
#[specta::specta]
fn save_agent_skill(app: AppHandle, server: String, old: String, name: String, description: String, body: String) -> Result<(), String> {
    skills::save(&app, &server, &old, &name, &description, &body)
}

#[tauri::command(async)]
#[specta::specta]
fn set_agent_skill_enabled(app: AppHandle, server: String, name: String, enabled: bool) -> Result<(), String> {
    skills::set_enabled(&app, &server, &name, enabled)
}

#[tauri::command(async)]
#[specta::specta]
fn delete_agent_skill(app: AppHandle, server: String, name: String) -> Result<(), String> {
    skills::delete(&app, &server, &name)
}

#[tauri::command(async)]
#[specta::specta]
fn copy_agent_skill(app: AppHandle, server: String, name: String, to: String) -> Result<(), String> {
    skills::copy(&app, &server, &name, &to)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_chat_sessions(app: AppHandle, server: String) -> Result<Vec<claude_chat::ChatSession>, String> {
    claude_chat::sessions(&app, &server)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_chat_rename(app: AppHandle, session: String, title: String) -> Result<(), String> {
    claude_chat::rename(&app, &session, &title)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_chat_delete(app: AppHandle, server: String, session: String) -> Result<(), String> {
    claude_chat::delete(&app, &server, &session)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_chat_open(
    app: AppHandle,
    server: String,
    session: String,
    mode: String,
    model: String,
    engine: String,
) -> Result<String, String> {
    if engine == "codex" {
        codex_chat::open(&app, &server, session.is_empty(), &mode, &model)
    } else {
        codex_chat::close_server(&server);
        claude_chat::open(&app, &server, &session, &mode, &model)
    }
}

#[tauri::command(async)]
#[specta::specta]
fn claude_chat_model(id: String, model: String) -> Result<(), String> {
    if codex_chat::is_codex(&id) {
        codex_chat::set_model(&id, &model)
    } else {
        claude_chat::set_model(&id, &model)
    }
}

#[tauri::command(async)]
#[specta::specta]
fn claude_chat_mode(id: String, mode: String) -> Result<(), String> {
    if codex_chat::is_codex(&id) {
        codex_chat::set_mode(&id, &mode)
    } else {
        claude_chat::set_mode(&id, &mode)
    }
}

#[tauri::command(async)]
#[specta::specta]
fn claude_attach_paths(app: AppHandle, server: String, paths: Vec<String>) -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    for path in paths {
        let path = PathBuf::from(path);
        if path.is_dir() {
            return Err(format!("Folders can't be attached: {}", path.display()));
        }
        let name = path.file_name().map(|one| one.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
        let bytes = fs::read(&path).map_err(|error| format!("Could not read {name}: {error}"))?;
        done.push(claude_agent::attach(&app, &server, &name, &bytes)?);
    }
    Ok(done)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_attach_bytes(app: AppHandle, server: String, name: String, data: String) -> Result<String, String> {
    use base64::Engine;
    let bytes =
        base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).map_err(|_| "Could not read the attached file.".to_string())?;
    claude_agent::attach(&app, &server, &name, &bytes)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_chat_send(app: AppHandle, id: String, content: String) -> Result<(), String> {
    if codex_chat::is_codex(&id) {
        codex_chat::send(&app, &id, &content)
    } else {
        claude_chat::send(&app, &id, &content)
    }
}

#[tauri::command(async)]
#[specta::specta]
fn claude_chat_answer(
    id: String,
    request_id: String,
    allow: bool,
    input: Option<String>,
    permissions: Option<String>,
    message: Option<String>,
) -> Result<(), String> {
    claude_chat::answer(&id, &request_id, allow, input, permissions, message)
}

#[tauri::command(async)]
#[specta::specta]
fn claude_chat_interrupt(app: AppHandle, id: String) -> Result<(), String> {
    if codex_chat::is_codex(&id) {
        codex_chat::interrupt(&app, &id)
    } else {
        claude_chat::interrupt(&id)
    }
}

#[tauri::command(async)]
#[specta::specta]
fn claude_chat_history(app: AppHandle, server: String, engine: String, session: String) -> Result<Vec<String>, String> {
    if engine == "codex" {
        codex_chat::history(&app, &server)
    } else {
        claude_chat::history(&app, &server, &session)
    }
}

#[tauri::command(async)]
#[specta::specta]
fn claude_chat_close(id: String) {
    if codex_chat::is_codex(&id) {
        codex_chat::close(&id)
    } else {
        claude_chat::close(&id)
    }
}

#[tauri::command]
#[specta::specta]
async fn acp_agents(app: AppHandle, refresh: bool) -> Result<Vec<acp::AcpAgent>, String> {
    tauri::async_runtime::spawn_blocking(move || acp::agents(&app, refresh)).await.map_err(|error| error.to_string())?
}

/// Downloads an agent the registry ships as a program for this computer. Progress comes as `acp-install` events.
#[tauri::command]
#[specta::specta]
async fn acp_install(app: AppHandle, agent: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || acp::install(&app, &agent)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
async fn acp_uninstall(app: AppHandle, agent: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || acp_binary::remove(&app, &agent)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
async fn acp_open(app: AppHandle, server: String, agent: String, session: String) -> Result<acp::AcpOpened, String> {
    tauri::async_runtime::spawn_blocking(move || acp::open(&app, &server, &agent, &session)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
async fn acp_authenticate(id: String, method: String) -> Result<acp::AcpOpened, String> {
    tauri::async_runtime::spawn_blocking(move || acp::authenticate(&id, &method)).await.map_err(|error| error.to_string())?
}

#[tauri::command(async)]
#[specta::specta]
fn acp_prompt(id: String, content: String) -> Result<(), String> {
    acp::prompt(&id, &content)
}

#[tauri::command(async)]
#[specta::specta]
fn acp_cancel(id: String) -> Result<(), String> {
    acp::cancel(&id)
}

#[tauri::command(async)]
#[specta::specta]
fn acp_answer(id: String, request_id: String, option: Option<String>) -> Result<(), String> {
    acp::answer(&id, &request_id, option)
}

#[tauri::command]
#[specta::specta]
async fn acp_set_mode(id: String, mode: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || acp::set_mode(&id, &mode)).await.map_err(|error| error.to_string())?
}

#[tauri::command]
#[specta::specta]
async fn acp_set_config(id: String, config: String, value: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || acp::set_config(&id, &config, &value)).await.map_err(|error| error.to_string())?
}

#[tauri::command(async)]
#[specta::specta]
fn acp_state(server: String) -> Vec<acp::AcpLive> {
    acp::state(&server)
}

#[tauri::command(async)]
#[specta::specta]
fn acp_close(id: String) {
    acp::close(&id)
}

fn acp_chat_root(app: &AppHandle, server: &str) -> Result<PathBuf, String> {
    // A server shared through a team keeps its chats here too, under its reference.
    if team_agent::parse(server).is_none() {
        instance_by_id(app, server)?;
    }
    app.path().monolab_data_dir().map_err(|error| error.to_string())
}

/// Saved conversations with ACP agents on this server, newest first.
#[tauri::command(async)]
#[specta::specta]
fn acp_chats(app: AppHandle, server: String) -> Result<Vec<acp_chats::AcpChatMeta>, String> {
    Ok(acp_chats::list(&acp_chat_root(&app, &server)?, &server))
}

#[tauri::command(async)]
#[specta::specta]
fn acp_chat_read(app: AppHandle, server: String, id: String) -> Result<acp_chats::AcpChat, String> {
    acp_chats::read(&acp_chat_root(&app, &server)?, &server, &id)
}

#[tauri::command(async)]
#[specta::specta]
fn acp_chat_save(app: AppHandle, server: String, chat: acp_chats::AcpChat) -> Result<(), String> {
    acp_chats::save(&acp_chat_root(&app, &server)?, &server, &chat)
}

#[tauri::command(async)]
#[specta::specta]
fn acp_chat_delete(app: AppHandle, server: String, id: String) -> Result<(), String> {
    acp_chats::delete(&acp_chat_root(&app, &server)?, &server, &id)
}

#[tauri::command(async)]
#[specta::specta]
async fn sftp_list(app: AppHandle, side: sftp::Side, path: String) -> Result<sftp::Listing, String> {
    sftp::list(&app, &side, &path).await
}

#[tauri::command(async)]
#[specta::specta]
async fn sftp_make_dir(app: AppHandle, side: sftp::Side, dir: String, name: String) -> Result<(), String> {
    sftp::make_dir(&app, &side, &dir, &name).await
}

#[tauri::command(async)]
#[specta::specta]
async fn sftp_rename(app: AppHandle, side: sftp::Side, dir: String, from: String, to: String) -> Result<(), String> {
    sftp::rename(&app, &side, &dir, &from, &to).await
}

#[tauri::command(async)]
#[specta::specta]
async fn sftp_read_text(app: AppHandle, side: sftp::Side, dir: String, name: String) -> Result<String, String> {
    sftp::read_text(&app, &side, &dir, &name).await
}

#[tauri::command(async)]
#[specta::specta]
async fn sftp_write_text(app: AppHandle, side: sftp::Side, dir: String, name: String, text: String) -> Result<(), String> {
    sftp::write_text(&app, &side, &dir, &name, &text).await
}

#[tauri::command(async)]
#[specta::specta]
async fn sftp_delete(app: AppHandle, side: sftp::Side, dir: String, names: Vec<String>) -> Result<(), String> {
    sftp::delete(&app, &side, &dir, &names).await
}

#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
async fn sftp_transfer(
    app: AppHandle,
    job: String,
    from: sftp::Side,
    from_dir: String,
    names: Vec<String>,
    to: sftp::Side,
    to_dir: String,
) -> Result<sftp::TransferResult, String> {
    sftp::transfer(&app, &job, &from, &from_dir, &names, &to, &to_dir).await
}

#[tauri::command(async)]
#[specta::specta]
fn sftp_cancel(job: String) {
    sftp::cancel(&job);
}

/// F2: the host's window has `path` open (every 30 seconds) or closed it. Gives everyone with a file of that
/// server open, so the window can show "@amy is editing" right away.
#[tauri::command(async)]
#[specta::specta]
fn file_editing(app: AppHandle, id: String, path: String, closed: bool) -> Vec<editing::OpenFile> {
    if !path.is_empty() {
        editing::mark(&app, &id, &path, "host", closed);
    }
    editing::open_files(&id)
}

/// Opens the Shell tab for a server: this PC's shell in its folder, or a login shell on its SSH host. App only.
#[tauri::command(async)]
#[specta::specta]
async fn shell_open(app: AppHandle, id: String, cols: u16, rows: u16) -> Result<String, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        return Err("Panel servers have no shell here. Use the panel's own console.".into());
    }
    shell::open(app, instance.remote_host.clone(), instance.path.clone(), cols, rows).await
}

#[tauri::command(async)]
#[specta::specta]
fn shell_input(session: String, data: String) -> Result<(), String> {
    shell::input(&session, &data)
}

#[tauri::command(async)]
#[specta::specta]
fn shell_resize(session: String, cols: u16, rows: u16) -> Result<(), String> {
    shell::resize(&session, cols, rows)
}

#[tauri::command(async)]
#[specta::specta]
fn shell_close(session: String) {
    shell::close(&session);
}

#[tauri::command(async)]
#[specta::specta]
async fn panel_call(app: AppHandle, host_id: String, method: String, path: String, body: Option<String>) -> Result<String, String> {
    let body = match body {
        Some(text) if !text.trim().is_empty() => {
            Some(serde_json::from_str(&text).map_err(|_| "The value to send is not JSON.".to_string())?)
        }
        _ => None,
    };
    let got = panel::api(&app, &host_id)?.call(&method, &path, body).await?;
    Ok(got.to_string())
}

async fn panel_detect(api: &panel::Api, target: &panel::Target) -> (String, String, String) {
    let mut core = String::new();
    let mut version = String::new();
    let mut jar = String::new();
    if let Ok(startup) = api.startup(target).await {
        for one in &startup.variables {
            let env = one.env.to_ascii_uppercase();
            let value = one.value.trim();
            if env.contains("JAR") && value.ends_with(".jar") {
                jar = value.to_string();
            }
            if (env == "MINECRAFT_VERSION" || env == "MC_VERSION" || env == "VERSION")
                && !value.is_empty()
                && !value.eq_ignore_ascii_case("latest")
            {
                version = value.to_string();
            }
        }
    }
    if let Ok(rows) = api.list(target, "").await {
        let jars: Vec<&str> = rows.iter().filter(|one| !one.directory && one.name.ends_with(".jar")).map(|one| one.name.as_str()).collect();
        if jar.is_empty() {
            jar = jars.iter().find(|one| **one == "server.jar").or(jars.first()).map(|one| one.to_string()).unwrap_or_default();
        }
        core = jars.iter().chain(std::iter::once(&jar.as_str())).find_map(|one| sniff::core_in_name(one)).unwrap_or_default();
    }
    if core.is_empty() {
        if let Some(egg) = api.egg_name(&target.ident).await {
            core = sniff::core_in_name(&egg)
                .or_else(|| egg.to_lowercase().contains("vanilla").then(|| "Vanilla".to_string()))
                .unwrap_or_default();
        }
    }
    if core.is_empty() {
        if let Ok(rows) = api.list(target, "").await {
            if rows.iter().any(|one| one.directory && one.name == "plugins") {
                core = "Paper".into();
            }
        }
    }
    if version.is_empty() {
        if let Ok(text) = api.read_text(target, "version_history.json").await {
            if let Some(at) = text.find("(MC: ") {
                version = text[at + 5..].chars().take_while(|one| *one != ')').collect();
            }
        }
    }
    if version.is_empty() {
        if let Ok(rows) = api.list(target, "versions").await {
            if let Some(found) = rows
                .iter()
                .filter(|one| one.directory && is_game_version(&one.name))
                .map(|one| one.name.clone())
                .max_by_key(|one| one.split('.').map(|part| part.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>())
            {
                version = found;
            }
        }
    }
    if version.is_empty() {
        if let Ok(bytes) = api.read_head(target, "logs/latest.log", 128 * 1024).await {
            let text = String::from_utf8_lossy(&bytes);
            if let Some(found) = text.lines().find_map(|line| {
                let lower = line.to_lowercase();
                if !(line.contains("(MC: ")
                    || lower.contains("running")
                    || lower.contains("server version")
                    || lower.contains("starting pumpkin"))
                {
                    return None;
                }
                let one = remote::version_in_log(line)?;
                let one = if pumpkin::is_pumpkin(&core) { one } else { game_version_of(&one) };
                (pumpkin::is_pumpkin(&core) || is_game_version(&one)).then_some(one)
            }) {
                version = found;
            }
        }
    }
    (if core.is_empty() { sniff::UNKNOWN.to_string() } else { core }, if version.is_empty() { "Needs review".into() } else { version }, jar)
}

async fn adopt_remote(app: &AppHandle, state: &AppState, instance: &ServerInstance) {
    let live = remote::live_state(app, &instance.remote_host, &[(instance.id.clone(), instance.path.clone())]).await;
    if !live.is_empty() {
        state.remote_running.lock().expect("remote state poisoned").extend(live);
    }
    state.remote_kick.notify_one();
}

#[tauri::command(async)]
#[specta::specta]
async fn import_remote_archive(
    app: AppHandle,
    state: State<'_, AppState>,
    host_id: String,
    archive_path: String,
    name: Option<String>,
) -> Result<InstanceView, String> {
    let archive = PathBuf::from(archive_path.trim());
    if !archive.is_file() {
        return Err("The chosen archive was not found.".into());
    }
    let wanted = name
        .as_deref()
        .map(str::trim)
        .filter(|one| !one.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| archive.file_stem().map(|one| one.to_string_lossy().to_string()).unwrap_or_else(|| "server".into()));
    tell_upload(&app, &wanted, "reading", 0, 0);
    let bytes = fs::read(&archive).map_err(|error| format!("Could not read the archive: {error}"));
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            tell_upload(&app, &wanted, "done", 0, 0);
            return Err(error);
        }
    };
    let made = send_zip_to_host(&app, &state, &host_id, &wanted, &bytes).await;
    tell_upload(&app, &wanted, "done", 0, 0);
    made
}

fn tell_job(app: &AppHandle, kind: &str, name: &str, stage: &str, sent: u64, total: u64) {
    let _ = app.emit("remote-upload", serde_json::json!({ "kind": kind, "name": name, "stage": stage, "sent": sent, "total": total }));
}

fn tell_upload(app: &AppHandle, name: &str, stage: &str, sent: u64, total: u64) {
    tell_job(app, "upload", name, stage, sent, total);
}

async fn watched_upload(app: &AppHandle, host: &str, folder: &str, relative: &str, bytes: &[u8], shown: &str) -> Result<(), String> {
    let step = std::sync::atomic::AtomicU64::new(u64::MAX);
    let done = remote::upload_watched(app, host, folder, relative, bytes, &|sent, total| {
        let at = (sent * 100).checked_div(total).unwrap_or(0);
        if step.swap(at, std::sync::atomic::Ordering::Relaxed) != at {
            tell_upload(app, shown, "uploading", sent, total);
        }
    })
    .await;
    tell_upload(app, shown, "done", 0, 0);
    done
}

async fn send_zip_to_host(app: &AppHandle, state: &AppState, host_id: &str, name: &str, bytes: &[u8]) -> Result<InstanceView, String> {
    let root = remote::server_root(app, host_id).await?;
    let folder = ascii_folder_name(name);
    let last = std::sync::atomic::AtomicU64::new(u64::MAX);
    let tick = |sent: u64, total: u64| {
        let step = (sent * 100).checked_div(total).unwrap_or(0);
        if last.swap(step, std::sync::atomic::Ordering::Relaxed) != step {
            tell_upload(app, name, "uploading", sent, total);
            if sent >= total {
                tell_upload(app, name, "unpacking", sent, total);
            }
        }
    };
    let dest = remote::unpack_zip(app, host_id, &root, &folder, bytes, &tick).await?;
    register_remote_folder(app, state, host_id, &dest, name).await
}

#[tauri::command(async)]
#[specta::specta]
async fn upload_instance_to_host(app: AppHandle, state: State<'_, AppState>, id: String, host_id: String) -> Result<InstanceView, String> {
    ensure_stopped(&state, &id)?;
    let instance = instance_by_id(&app, &id)?;
    if !instance.remote_host.is_empty() {
        return Err("This server is already on the SSH server.".into());
    }
    let server = PathBuf::from(&instance.path);
    if !server.is_dir() {
        return Err("Server folder not found.".into());
    }
    let staging = upgrade_dir(&app, &instance.id)?.join("upload");
    tell_upload(&app, &instance.name, "packing", 0, 0);
    let bytes = backups::create(&server, "Upload", &staging, false).and_then(|packed| {
        let zip = staging.join(&packed.file_name);
        let bytes = fs::read(&zip).map_err(|error| format!("Could not read the packed file: {error}"));
        let _ = fs::remove_file(&zip);
        bytes
    });
    let made = match bytes {
        Ok(bytes) => send_zip_to_host(&app, &state, &host_id, &instance.name, &bytes).await,
        Err(error) => Err(error),
    };
    tell_upload(&app, &instance.name, "done", 0, 0);
    made
}

/// Creating on an SSH machine: the server is built here first, then moved over and the local copy removed.
#[tauri::command]
#[specta::specta]
async fn move_instance_to_host(app: AppHandle, state: State<'_, AppState>, id: String, host_id: String) -> Result<InstanceView, String> {
    let instance = instance_by_id(&app, &id)?;
    if pumpkin::is_pumpkin(&instance.core) {
        let conn = remote::connect(&app, &host_id).await?;
        let arch = conn.run("uname -m").await?.stdout;
        let arch = arch.trim().to_string();
        if !matches!(arch.as_str(), "x86_64" | "amd64" | "aarch64" | "arm64") {
            return Err(format!("Pumpkin has no Linux build for this SSH server's CPU ({arch})."));
        }
        let arm = matches!(arch.as_str(), "aarch64" | "arm64");
        let folder = PathBuf::from(&instance.path);
        let version = instance.version.clone();
        let target = folder.join("pumpkin");
        tauri::async_runtime::spawn_blocking(move || off_runtime(move || cores::pumpkin_linux_download(&version, arm, &target)))
            .await
            .map_err(|error| error.to_string())??;
        let _ = fs::remove_file(folder.join(cores::PUMPKIN_EXE));
        {
            let _store = lock_instance_store();
            let mut instances = load_instances(&app)?;
            if let Some(saved) = instances.iter_mut().find(|saved| saved.id == id) {
                saved.server_jar = Some("pumpkin".into());
            }
            save_instances(&app, &instances)?;
        }
    }
    let moved = upload_instance_to_host(app.clone(), state, id.clone(), host_id).await?;
    let local = PathBuf::from(&instance.path);
    {
        let _store = lock_instance_store();
        let mut instances = load_instances(&app)?;
        instances.retain(|saved| saved.id != id);
        save_instances(&app, &instances)?;
    }
    let root = managed_servers_root(&app)?;
    if local.starts_with(&root) && local != root {
        let _ = fs::remove_dir_all(&local);
    }
    Ok(moved)
}

async fn register_remote_folder(
    app: &AppHandle,
    state: &AppState,
    host_id: &str,
    folder: &str,
    name: &str,
) -> Result<InstanceView, String> {
    let found = remote::detect(app, host_id, folder).await?;
    let instance = {
        let _store = lock_instance_store();
        let mut instances = load_instances(app)?;
        if instances.iter().any(|one| one.remote_host == host_id && one.path == folder) {
            return Err("This folder is already registered.".into());
        }
        let instance = ServerInstance {
            id: instance_id(),
            name: name.to_string(),
            path: folder.to_string(),
            core: found.core,
            version: if found.version.is_empty() { "Needs review".into() } else { found.version },
            launch_mode: default_launch_mode(),
            server_jar: (!found.jar.is_empty()).then(|| found.jar.clone()),
            java_path: "java".into(),
            max_memory_mib: 4096,
            min_memory_mib: 4096,
            jvm_args: String::new(),
            port: found.port,
            sort_order: instances.len() as u32,
            group: String::new(),
            start_with_app: false,
            crash_restart: false,
            core_build: String::new(),
            created_at: unix_now(),
            last_run_at: 0,
            custom_command: String::new(),
            start_script: String::new(),
            start_script_hash: String::new(),
            remote_host: host_id.to_string(),
            linked: true,
            panel_host: String::new(),
            panel_server: String::new(),
            panel_daemon: String::new(),
            named: false,
        };
        instances.push(instance.clone());
        save_instances(app, &instances)?;
        instance
    };
    adopt_remote(app, state, &instance).await;
    Ok(instance_view(instance, state))
}

#[tauri::command(async)]
#[specta::specta]
async fn test_ssh_host(app: AppHandle, id: String) -> Result<remote::BoxInfo, String> {
    remote::survey(&app, &id).await
}

#[tauri::command(async)]
#[specta::specta]
fn playit_state(app: AppHandle) -> playit::Status {
    playit::status(&app)
}

#[tauri::command(async)]
#[specta::specta]
fn playit_claim(app: AppHandle) -> Result<String, String> {
    playit::begin_claim(&app)
}

#[tauri::command(async)]
#[specta::specta]
fn playit_expose(app: AppHandle, id: String) -> Result<playit::Status, String> {
    playit::expose(&app, &id)
}

#[tauri::command(async)]
#[specta::specta]
fn playit_unexpose(app: AppHandle, id: String) -> Result<playit::Status, String> {
    playit::unexpose(&app, &id)
}

#[tauri::command(async)]
#[specta::specta]
fn playit_forget(app: AppHandle) -> Result<playit::Status, String> {
    playit::forget(&app)
}

#[tauri::command(async)]
#[specta::specta]
fn vps_tunnel_state(app: AppHandle, id: String) -> vps_tunnel::VpsTunnel {
    vps_tunnel::state(&app, &id)
}

/// Opens the server to players through a port on one of your saved SSH servers (`ssh -R`). This PC's servers only.
#[tauri::command(async)]
#[specta::specta]
fn vps_tunnel_start(app: AppHandle, id: String, host: String, port: u16) -> Result<vps_tunnel::VpsTunnel, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.remote_host.is_empty() || !instance.panel_host.is_empty() {
        return Err("A VPS tunnel is for servers on this PC. An SSH or panel server already has its own address.".into());
    }
    vps_tunnel::start(&app, &id, &host, port, instance.port)
}

#[tauri::command(async)]
#[specta::specta]
fn vps_tunnel_stop(app: AppHandle, id: String, forget: bool) -> Result<vps_tunnel::VpsTunnel, String> {
    if forget {
        vps_tunnel::forget(&app, &id)?;
    } else {
        vps_tunnel::stop(&id);
    }
    Ok(vps_tunnel::state(&app, &id))
}

#[tauri::command(async)]
#[specta::specta]
fn set_web_panel(app: AppHandle, running: State<'_, web::Running>, on: bool, port: u16) -> Result<WebPanelView, String> {
    let mut settings = read_app_settings(&app);
    if on && port < 1024 {
        return Err("Choose a port above 1024.".into());
    }
    web::stop(&app);
    if on {
        web::start(&app, port)?;
    }
    settings.web_panel = on;
    settings.web_panel_port = port;
    store::write(&app_settings_path(&app)?, &serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?)
        .map_err(|error| format!("Could not save settings: {error}"))?;
    Ok(web_panel_state(app.clone(), running))
}

#[tauri::command(async)]
#[specta::specta]
fn list_core_builds(core: String, version: String) -> Result<Vec<cores::CoreBuild>, String> {
    off_runtime(|| cores::list_core_builds(&core, &version))
}

fn picked_build(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// A sharper copy of the server icon for the app's own lists; server-icon.png stays 64×64 for Minecraft.
fn large_icon_path(app: &AppHandle, id: &str) -> Option<PathBuf> {
    instance_data_dir(app, id).ok().map(|dir| dir.join("server-icon-large.png"))
}

fn save_large_icon(app: &AppHandle, id: &str, picture: &image::DynamicImage) {
    let Some(path) = large_icon_path(app, id) else { return };
    let side = picture.width().min(picture.height()).clamp(64, 256);
    let large = picture.resize_to_fill(side, side, image::imageops::FilterType::Lanczos3);
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = large.save_with_format(&path, image::ImageFormat::Png);
}

/// The sharper copy as a data URL; for a local server only while it is newer than server-icon.png.
fn large_icon_url(app: &AppHandle, id: &str, local_icon: Option<&Path>) -> Option<String> {
    let path = large_icon_path(app, id)?;
    if let Some(icon) = local_icon {
        let icon_time = fs::metadata(icon).and_then(|meta| meta.modified()).ok()?;
        let large_time = fs::metadata(&path).and_then(|meta| meta.modified()).ok()?;
        if large_time < icon_time {
            return None;
        }
    }
    let bytes = fs::read(&path).ok()?;
    use base64::Engine;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[tauri::command(async)]
#[specta::specta]
async fn get_server_icon(app: AppHandle, id: String) -> Result<Option<String>, String> {
    let instance = instance_by_id(&app, &id)?;
    let bytes = if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        match api.exists(&target, "server-icon.png").await {
            Ok(Some(false)) => match api.read_bytes(&target, "server-icon.png", 2 * 1024 * 1024).await {
                Ok(bytes) => bytes,
                Err(_) => return Ok(None),
            },
            _ => return Ok(None),
        }
    } else if instance.remote_host.is_empty() {
        let path = Path::new(&instance.path).join("server-icon.png");
        if !path.is_file() {
            return Ok(None);
        }
        if let Some(large) = large_icon_url(&app, &id, Some(&path)) {
            return Ok(Some(large));
        }
        fs::read(&path).map_err(|error| error.to_string())?
    } else {
        match remote::read_bytes(&app, &instance.remote_host, &instance.path, "server-icon.png", 2 * 1024 * 1024).await {
            Ok(bytes) => bytes,
            Err(_) => return Ok(None),
        }
    };
    if bytes.len() > 2 * 1024 * 1024 {
        return Ok(None);
    }
    if let Some(large) = large_icon_url(&app, &id, None) {
        return Ok(Some(large));
    }
    use base64::Engine;
    Ok(Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
}

#[tauri::command(async)]
#[specta::specta]
async fn set_server_icon(app: AppHandle, id: String, source: String) -> Result<String, String> {
    let instance = instance_by_id(&app, &id)?;
    let source = PathBuf::from(source.trim());
    if !source.is_file() {
        return Err("The image file was not found.".into());
    }
    let bytes = fs::read(&source).map_err(|error| format!("Could not read the image: {error}"))?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err("Images larger than 10 MB cannot be used.".into());
    }
    let picture = image::load_from_memory(&bytes).map_err(|_| "Not an image, or an unsupported format.".to_string())?;
    let square = picture.resize_to_fill(64, 64, image::imageops::FilterType::Lanczos3);
    save_large_icon(&app, &id, &picture);
    if !instance.panel_host.is_empty() {
        let mut png = std::io::Cursor::new(Vec::new());
        square.write_to(&mut png, image::ImageFormat::Png).map_err(|error| format!("Could not create the icon: {error}"))?;
        let png = png.into_inner();
        let (api, target) = panel_of(&app, &instance)?;
        api.upload(&target, "", "server-icon.png", png.clone()).await?;
        record_audit(&app, &id, "Server", "Change icon (panel)", "Saved server-icon.png at 64×64.");
        if let Some(large) = large_icon_url(&app, &id, None) {
            return Ok(large);
        }
        use base64::Engine;
        return Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)));
    }
    if !instance.remote_host.is_empty() {
        let mut png = std::io::Cursor::new(Vec::new());
        square.write_to(&mut png, image::ImageFormat::Png).map_err(|error| format!("Could not create the icon: {error}"))?;
        let png = png.into_inner();
        // Don't use `block_on` here: this runs on a tokio worker and would deadlock.
        remote::upload(&app, &instance.remote_host, &instance.path, "server-icon.png", &png).await?;
        record_audit(&app, &id, "Server", "Change icon (SSH)", "Saved server-icon.png at 64×64.");
        if let Some(large) = large_icon_url(&app, &id, None) {
            return Ok(large);
        }
        use base64::Engine;
        return Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)));
    }
    let target = Path::new(&instance.path).join("server-icon.png");
    square.save_with_format(&target, image::ImageFormat::Png).map_err(|error| format!("Could not save the icon: {error}"))?;
    if pumpkin::is_pumpkin(&instance.core) {
        pumpkin::set_favicon(Path::new(&instance.path), Some("server-icon.png"))?;
    }
    record_audit(&app, &id, "Server", "Icon changed", "Saved server-icon.png at 64×64.");
    // Saved after server-icon.png, so it counts as fresh.
    save_large_icon(&app, &id, &picture);
    if let Some(large) = large_icon_url(&app, &id, Some(&target)) {
        return Ok(large);
    }
    let saved = fs::read(&target).map_err(|error| error.to_string())?;
    use base64::Engine;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(saved)))
}

#[tauri::command(async)]
#[specta::specta]
async fn clear_server_icon(app: AppHandle, id: String) -> Result<(), String> {
    let instance = instance_by_id(&app, &id)?;
    if let Some(large) = large_icon_path(&app, &id) {
        let _ = fs::remove_file(large);
    }
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        if api.exists(&target, "server-icon.png").await?.is_some() {
            pfs::trash_path(&api, &target, "server-icon.png").await?;
        }
        return Ok(());
    }
    if !instance.remote_host.is_empty() {
        remote::drop_path(&app, &instance.remote_host, &instance.path, "server-icon.png").await;
        return Ok(());
    }
    let path = Path::new(&instance.path).join("server-icon.png");
    if path.is_file() {
        fs::remove_file(&path).map_err(|error| format!("Could not delete the icon: {error}"))?;
    }
    if pumpkin::is_pumpkin(&instance.core) {
        pumpkin::set_favicon(Path::new(&instance.path), None)?;
    }
    Ok(())
}

fn guess_java(command: &str) -> (String, Option<String>) {
    let flat = command.replace('\\', "/");
    let low = flat.to_lowercase();

    if low.contains("net.minecraft.client.main.main") || low.contains("--assetsdir") || low.contains("--gamedir") {
        return ("Minecraft game".into(), Some("Not a server".into()));
    }

    if low.contains("pumpkin.exe") {
        let folder = flat
            .split('"')
            .find(|piece| piece.to_lowercase().ends_with("pumpkin.exe"))
            .and_then(|piece| piece.rsplit_once('/').map(|(dir, _)| dir.to_string()))
            .and_then(|dir| dir.rsplit('/').next().map(str::to_owned));
        return ("Pumpkin server".into(), folder);
    }

    let jar_path = flat
        .split(|letter: char| letter.is_whitespace() || letter == '"')
        .rfind(|piece| piece.to_lowercase().ends_with(".jar"))
        .map(|one| one.to_string());
    let jar = jar_path.as_deref().map(|path| path.rsplit('/').next().unwrap_or(path).to_string());

    let name = jar.clone().unwrap_or_default().to_lowercase();
    let core = [
        ("folia", "Folia"),
        ("purpur", "Purpur"),
        ("leaf", "Leaf"),
        ("pufferfish", "Pufferfish"),
        ("paper", "Paper"),
        ("spigot", "Spigot"),
        ("craftbukkit", "CraftBukkit"),
        ("neoforge", "NeoForge"),
        ("fabric", "Fabric"),
        ("quilt", "Quilt"),
        ("forge", "Forge"),
        ("velocity", "Velocity"),
        ("waterfall", "Waterfall"),
        ("bungeecord", "BungeeCord"),
        ("minecraft_server", "Vanilla"),
    ]
    .into_iter()
    .find(|(key, _)| name.contains(key))
    .map(|(_, shown)| shown);

    let version = name
        .split(|letter: char| !letter.is_ascii_digit() && letter != '.')
        .map(|piece| piece.trim_matches('.'))
        .find(|piece| piece.contains('.') && piece.starts_with(|one: char| one.is_ascii_digit()))
        .map(|piece| piece.to_string());

    let folder = jar_path.as_deref().filter(|path| path.contains('/')).and_then(|path| path.rsplit('/').nth(1)).map(|one| one.to_string());

    let why: Vec<String> = folder.into_iter().chain(jar.clone()).collect();
    let why = (!why.is_empty()).then(|| why.join(" · "));

    if let Some(core) = core {
        return match version {
            Some(version) => (format!("{core} {version} server"), why),
            None => (format!("{core} server"), why),
        };
    }
    if low.contains("nogui") || name == "server.jar" {
        return ("Minecraft server".into(), why);
    }
    ("Other Java program".into(), why)
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct JavaProcess {
    pid: u32,
    label: String,
    detail: Option<String>,
    server_id: Option<String>,
    orphan: bool,
    memory_mib: u64,
}

#[tauri::command(async)]
#[specta::specta]
fn list_java_processes(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<JavaProcess>, String> {
    let instances = load_instances(&app).unwrap_or_default();
    let managed: HashMap<u32, String> = {
        let processes = state.processes.lock().expect("process state poisoned");
        processes.iter().map(|(id, process)| (process.pid, id.clone())).collect()
    };
    let output = off_runtime(|| {
        hidden_command("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "@(Get-CimInstance Win32_Process | Where-Object { $_.Name -eq 'java.exe' -or $_.Name -eq 'javaw.exe' -or $_.Name -eq 'pumpkin.exe' } | Select-Object ProcessId,WorkingSetSize,CommandLine) | ConvertTo-Json -Compress -Depth 2",
            ])
            .output()
            .ok()
    })
    .ok_or("Could not check running Java processes.")?;
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    let mut found = Vec::new();
    if !text.is_empty() {
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap_or(serde_json::Value::Null);
        let rows: Vec<&serde_json::Value> = match &parsed {
            serde_json::Value::Array(items) => items.iter().collect(),
            serde_json::Value::Null => Vec::new(),
            single => vec![single],
        };
        for row in rows {
            let Some(pid) = row.get("ProcessId").and_then(|value| value.as_u64()) else {
                continue;
            };
            let pid = pid as u32;
            let raw = row.get("CommandLine").and_then(|value| value.as_str()).unwrap_or_default().to_string();
            let command = raw.replace('\\', "/").to_lowercase();
            let memory = row.get("WorkingSetSize").and_then(|value| value.as_u64()).unwrap_or(0);
            let owner = instances.iter().find(|instance| {
                let path = instance.path.replace('\\', "/").to_lowercase();
                !path.is_empty() && command.contains(&path)
            });
            let server_id = managed.get(&pid).cloned();
            let (label, detail) = match owner {
                Some(instance) => (instance.name.clone(), Some(format!("{} {}", instance.core, instance.version))),
                None => guess_java(&raw),
            };
            found.push(JavaProcess {
                pid,
                label,
                detail,
                orphan: owner.is_some() && server_id.is_none(),
                server_id,
                memory_mib: memory / (1024 * 1024),
            });
        }
    }
    found.sort_by(|a, b| b.orphan.cmp(&a.orphan).then(b.memory_mib.cmp(&a.memory_mib)));
    Ok(found)
}

#[tauri::command(async)]
#[specta::specta]
fn kill_java_process(app: AppHandle, pid: u32) -> Result<(), String> {
    kill_process_tree(pid);
    record_audit(&app, "", "Server", "Leftover process terminated", &format!("Terminated process {pid}."));
    Ok(())
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AddonCard {
    file_name: String,
    project_id: String,
    title: String,
    icon_url: Option<String>,
    compatibility: String,
    warn: bool,
}

fn loader_label(loader: &str) -> String {
    match loader {
        "paper" => "Paper".into(),
        "purpur" => "Purpur".into(),
        "leaf" => "Leaf".into(),
        "folia" => "Folia".into(),
        "fabric" => "Fabric".into(),
        "neoforge" => "NeoForge".into(),
        "forge" => "Forge".into(),
        "velocity" => "Velocity".into(),
        "bungeecord" => "BungeeCord".into(),
        "spigot" => "Spigot".into(),
        "bukkit" => "Bukkit".into(),
        "quilt" => "Quilt".into(),
        other => other.to_string(),
    }
}

fn version_span(versions: &[String]) -> String {
    let shown: Vec<&String> = versions.iter().take(3).collect();
    let head = match shown.as_slice() {
        [] => String::new(),
        [one] => (*one).clone(),
        [first, .., last] => format!("{first} ~ {last}"),
    };
    let more = versions.len().saturating_sub(3);
    if more > 0 {
        format!("{head} and {more} more")
    } else {
        head
    }
}

fn addon_fit(addon: &addons::InstalledAddon, card: &addons::ProjectCard, instance: &ServerInstance) -> (String, bool) {
    if card.server_side == "unsupported" {
        return ("Client only".into(), true);
    }
    let wanted = modrinth_loader(instance, "mod")
        .or_else(|_| modrinth_loader(instance, "plugin"))
        .map(|(_, loader)| loader.to_string())
        .unwrap_or_default();
    if !wanted.is_empty() && !addon.loaders.is_empty() && !addon.loaders.iter().any(|one| one == &wanted) {
        return (
            format!(
                "{} only · {} not supported",
                addon.loaders.iter().map(|one| loader_label(one)).collect::<Vec<_>>().join("·"),
                loader_label(&wanted)
            ),
            true,
        );
    }
    if !addon.game_versions.is_empty() && !addon.game_versions.iter().any(|one| one == &instance.version) {
        return (format!("Supports {} · {} not supported", version_span(&addon.game_versions), instance.version), true);
    }
    (String::new(), false)
}

#[tauri::command(async)]
#[specta::specta]
async fn enrich_addons(app: AppHandle, id: String) -> Result<Vec<AddonCard>, String> {
    let remote_found = {
        let instance = instance_by_id(&app, &id)?;
        if !is_far(&instance) {
            None
        } else {
            Some(remote_addon_list(&app, &instance).await)
        }
    };
    off_runtime(move || {
        let instance = instance_by_id(&app, &id)?;
        let mut found = match remote_found {
            Some(list) => list,
            None => addons::scan(Path::new(&instance.path)),
        };
        if found.is_empty() {
            return Ok(Vec::new());
        }
        let client = addons::client()?;
        if addons::identify(&client, &mut found).is_err() {
            return Ok(Vec::new());
        }
        let ids: std::collections::HashSet<&str> = found.iter().filter_map(|addon| addon.project_id.as_deref()).collect();
        let cards = addons::fetch_project_cards(&client, &ids).unwrap_or_default();
        Ok(found
            .iter()
            .filter_map(|addon| {
                let project = addon.project_id.as_deref()?;
                let card = cards.get(project)?;
                let (compatibility, warn) = addon_fit(addon, card, &instance);
                Some(AddonCard {
                    file_name: addon.file_name.clone(),
                    project_id: project.to_string(),
                    title: card.title.clone(),
                    icon_url: card.icon.clone(),
                    compatibility,
                    warn,
                })
            })
            .collect())
    })
}

#[tauri::command(async)]
#[specta::specta]
fn list_java_runtimes(app: AppHandle) -> Result<Vec<java::JavaRuntime>, String> {
    Ok(off_runtime(|| java::scan_installed_javas(&app)))
}

#[tauri::command(async)]
#[specta::specta]
fn get_servers_root(app: AppHandle) -> Result<String, String> {
    Ok(managed_servers_root(&app)?.to_string_lossy().to_string())
}

#[tauri::command(async)]
#[specta::specta]
fn set_servers_root(app: AppHandle, folder: String) -> Result<String, String> {
    let path = PathBuf::from(folder.trim());
    if !path.is_dir() {
        return Err("That folder was not found.".into());
    }
    let path = path.canonicalize().map(plain_path).map_err(|error| format!("Could not verify the folder: {error}"))?;
    let probe = path.join(".monolab-write-test");
    fs::write(&probe, b"ok").map_err(|error| format!("Cannot write to that folder: {error}"))?;
    let _ = fs::remove_file(&probe);
    fs::write(servers_root_setting_path(&app)?, path.to_string_lossy().as_bytes())
        .map_err(|error| format!("Could not remember the storage location: {error}"))?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command(async)]
#[specta::specta]
fn reset_servers_root(app: AppHandle) -> Result<String, String> {
    let path = servers_root_setting_path(&app)?;
    if path.is_file() {
        fs::remove_file(&path).map_err(|error| format!("Could not reset the storage location: {error}"))?;
    }
    Ok(managed_servers_root(&app)?.to_string_lossy().to_string())
}

fn ascii_folder_name(name: &str) -> String {
    let mut out = String::new();
    for character in name.trim().chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let out = out.trim_matches('_').to_string();
    if out.is_empty() {
        "server".into()
    } else if !out.chars().any(|character| character.is_ascii_alphabetic()) {
        format!("server_{out}")
    } else {
        out
    }
}

fn free_folder(root: &Path, name: &str) -> PathBuf {
    let base = ascii_folder_name(name);
    let mut folder = root.join(&base);
    let mut suffix = 2;
    while folder.exists() {
        folder = root.join(format!("{base}_{suffix}"));
        suffix += 1;
    }
    folder
}

fn find_jar(folder: &Path) -> Option<PathBuf> {
    let standard = folder.join("server.jar");
    if standard.is_file() {
        return Some(standard);
    }
    fs::read_dir(folder).ok()?.flatten().map(|entry| entry.path()).find(|path| {
        path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("jar"))
            && !path.file_name().is_some_and(|name| name.to_string_lossy().contains("installer"))
    })
}

pub(crate) fn download_file(client: &reqwest::blocking::Client, url: &str, destination: &Path) -> Result<(), String> {
    download_file_watched(client, url, destination, &mut |_, _| {})
}

pub(crate) fn download_file_watched(
    client: &reqwest::blocking::Client,
    url: &str,
    destination: &Path,
    tick: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(), String> {
    // Must be HTTPS from a known distributor (net.rs).
    net::check_download_url(url)?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|error| format!("The download request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("The download server did not provide the file: {error}"))?;
    let total = response.content_length();
    let temporary = destination.with_extension("part");
    let mut file = fs::File::create(&temporary).map_err(|error| format!("Could not create the download file: {error}"))?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut received = 0_u64;
    loop {
        let read = response.read(&mut buffer).map_err(|error| format!("Problem while downloading: {error}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|error| format!("Could not save the file: {error}"))?;
        received += read as u64;
        tick(received, total);
    }
    file.flush().map_err(|error| error.to_string())?;
    drop(file);
    fs::rename(&temporary, destination).map_err(|error| format!("Could not finish the download file: {error}"))
}

fn tell_addon_download(app: &AppHandle, server_id: &str, project_id: &str, file: &str, received: u64, total: Option<u64>) {
    let payload = serde_json::json!({ "serverId": server_id, "projectId": project_id, "file": file, "received": received, "total": total });
    let _ = app.emit("addon-download", &payload);
    remember_for_web(app, "addon-download", &payload);
}

fn download_cancelled(app: &AppHandle, operation_id: &str) -> bool {
    !operation_id.is_empty() && app.state::<AppState>().cancelled_downloads.lock().expect("download state poisoned").contains(operation_id)
}

const CONSOLE_FLAGS: [&str; 3] = ["-Dterminal.ansi=true", "-Duser.language=en", "-Duser.country=US"];

fn strip_ansi(line: &str) -> String {
    if !line.contains('\u{1b}') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(one) = chars.next() {
        if one != '\u{1b}' {
            out.push(one);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for inside in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&inside) {
                        break;
                    }
                }
            }
            Some(other) => {
                out.push('\u{1b}');
                out.push(other);
            }
            None => out.push('\u{1b}'),
        }
    }
    out
}

pub(crate) fn download_server_file(
    app: &AppHandle,
    client: &reqwest::blocking::Client,
    source: &DownloadSource,
    destination: &Path,
    core: &str,
    version: &str,
    operation_id: &str,
) -> Result<(), String> {
    net::check_download_url(&source.url)?;
    let temporary = destination.with_extension("part");
    let mut file = fs::File::create(&temporary).map_err(|error| format!("Could not create the download file: {error}"))?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut received = 0_u64;
    let mut total: Option<u64> = None;
    // Some download servers (new Purpur builds before their CDN caches them) stall part way;
    // a stalled read is retried from the byte it stopped at instead of starting over.
    let mut attempt = 0;
    loop {
        attempt += 1;
        let mut request = client.get(&source.url).timeout(Duration::from_secs(20));
        if received > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={received}-"));
        }
        let response = request.send().and_then(|one| one.error_for_status());
        let mut response = match response {
            Ok(one) => one,
            Err(_) if attempt < 8 && received > 0 => {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            }
            Err(error) => return Err(format!("The download server did not provide the file: {error}")),
        };
        if received > 0 && response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            // The server ignored the range, so the whole file comes again.
            file = fs::File::create(&temporary).map_err(|error| format!("Could not create the download file: {error}"))?;
            received = 0;
        }
        if total.is_none() {
            total = response.content_length().map(|left| left + received);
        }
        let stalled = loop {
            if download_cancelled(app, operation_id) {
                drop(file);
                let _ = fs::remove_file(&temporary);
                let _ = app.emit("server-download", serde_json::json!({ "stage": "cancelled", "core": core, "version": version }));
                return Err("Download cancelled.".into());
            }
            let read = match response.read(&mut buffer) {
                Ok(read) => read,
                Err(error) => break Some(error),
            };
            if read == 0 {
                break None;
            }
            file.write_all(&buffer[..read]).map_err(|error| format!("Could not save the file: {error}"))?;
            received += read as u64;
            let _ = app.emit(
                "server-download",
                serde_json::json!({ "stage": "downloading", "core": core, "version": version, "received": received, "total": total }),
            );
        };
        let short = total.is_some_and(|whole| received < whole);
        match stalled {
            None if !short => break,
            Some(error) if attempt >= 8 => return Err(format!("Problem while downloading: {error}")),
            None if attempt >= 8 => return Err("The download ended before the whole file arrived.".into()),
            _ => std::thread::sleep(Duration::from_secs(1)),
        }
    }
    file.flush().map_err(|error| error.to_string())?;
    drop(file);
    if let Some(expected) = &source.sha256 {
        if let Err(error) = verify_sha256(&temporary, expected) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    }
    if let Some(expected) = &source.sha1 {
        if let Err(error) = verify_sha1(&temporary, expected) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    }
    if let Some(expected) = &source.md5 {
        if let Err(error) = verify_md5(&temporary, expected) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    }
    if source.sha256.is_none() && source.sha1.is_none() && source.md5.is_none() {
        let check = if core == "Pumpkin" { hashes::verify_windows_exe(&temporary) } else { verify_java_archive(&temporary) };
        if let Err(error) = check {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    }
    fs::rename(&temporary, destination).map_err(|error| format!("Could not finish the download file: {error}"))
}

fn installed_args_file(folder: &Path) -> Option<String> {
    let mine = if cfg!(windows) { "win_args.txt" } else { "unix_args.txt" };
    let exists = |relative: &str| folder.join(relative).is_file();
    for runner in ["run.bat", "run.sh"] {
        let Ok(text) = fs::read_to_string(folder.join(runner)) else {
            continue;
        };
        for token in text.split_whitespace().filter_map(|token| token.strip_prefix('@')) {
            let token = token.trim_matches(|c| c == '"' || c == '\'');
            for suffix in ["win_args.txt", "unix_args.txt"] {
                if let Some(stem) = token.strip_suffix(suffix) {
                    let wanted = format!("{stem}{mine}");
                    if exists(&wanted) {
                        return Some(wanted);
                    }
                }
            }
        }
    }
    let mut found: Vec<(Vec<u64>, String)> = Vec::new();
    for base in ["libraries/net/neoforged/neoforge", "libraries/net/neoforged/forge", "libraries/net/minecraftforge/forge"] {
        let Ok(entries) = fs::read_dir(folder.join(base)) else {
            continue;
        };
        for entry in entries.flatten() {
            let version = entry.file_name().to_string_lossy().to_string();
            let relative = format!("{base}/{version}/{mine}");
            if exists(&relative) {
                found.push((version_key(&version), relative));
            }
        }
    }
    found.sort();
    found.pop().map(|(_, path)| path)
}

fn version_key(version: &str) -> Vec<u64> {
    version.split(|c: char| !c.is_ascii_digit()).filter(|part| !part.is_empty()).map(|part| part.parse().unwrap_or(0)).collect()
}

fn script_args_file(folder: &Path) -> Result<String, String> {
    installed_args_file(folder).ok_or_else(|| {
        "The args file (libraries/…/win_args.txt) for this installed server could not be found. The server folder may have moved or installation did not finish.".into()
    })
}

fn update_script_memory(path: &Path, memory_mib: u32, min_memory_mib: u32, jvm_args: &str, previous_jvm_args: &str) -> Result<(), String> {
    let previous_arguments = previous_jvm_args.split_whitespace().collect::<HashSet<_>>();
    let current = fs::read_to_string(path).unwrap_or_default();
    let mut lines = current
        .lines()
        .filter(|line| {
            let value = line.trim();
            !value.starts_with("-Xms") && !value.starts_with("-Xmx") && !previous_arguments.contains(value)
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    lines.push(format!("-Xms{}M", sane_min_memory(min_memory_mib, memory_mib)));
    lines.push(format!("-Xmx{memory_mib}M"));
    for argument in launch::jvm_arguments(jvm_args)? {
        if !lines.contains(&argument) {
            lines.push(argument);
        }
    }
    fs::write(path, format!("{}\n", lines.join("\n"))).map_err(|error| format!("Could not save the memory settings: {error}"))
}

fn ensure_port_available(port: u16) -> Result<(), String> {
    TcpListener::bind(("0.0.0.0", port)).map(drop).map_err(|_| format!("Port {port} is already in use by another program or server. Change Connection port under Server settings → General, or stop that server first."))
}

fn level_name_of(instance: &ServerInstance) -> String {
    if pumpkin::is_pumpkin(&instance.core) {
        return pumpkin::level_name(Path::new(&instance.path));
    }
    fs::read_to_string(Path::new(&instance.path).join("server.properties"))
        .ok()
        .and_then(|text| text.lines().find_map(|line| line.trim().strip_prefix("level-name=")).map(|value| value.trim().to_string()))
        .filter(|value| !value.is_empty() && !value.contains(['/', '\\', ':']))
        .unwrap_or_else(|| "world".into())
}

fn port_from_properties(instance: &ServerInstance) -> u16 {
    if pumpkin::is_pumpkin(&instance.core) {
        return pumpkin::port(Path::new(&instance.path)).unwrap_or(instance.port);
    }
    if instance.core == "Velocity" {
        let config = Path::new(&instance.path).join("velocity.toml");
        if let Some(port) = fs::read_to_string(config).ok().and_then(|text| {
            text.lines()
                .find_map(|line| line.trim().strip_prefix("bind ="))
                .and_then(|value| value.trim().trim_matches('"').rsplit_once(':').and_then(|(_, port)| port.parse::<u16>().ok()))
        }) {
            return port;
        }
    }
    if instance.core == "BungeeCord" {
        let config = Path::new(&instance.path).join("config.yml");
        if let Some(port) = fs::read_to_string(config).ok().and_then(|text| {
            text.lines()
                .find_map(|line| line.trim().strip_prefix("host:"))
                .and_then(|value| value.trim().trim_matches('"').rsplit_once(':').and_then(|(_, port)| port.parse::<u16>().ok()))
        }) {
            return port;
        }
    }
    let properties = Path::new(&instance.path).join("server.properties");
    fs::read_to_string(properties)
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let (key, value) = line.trim().split_once('=')?;
                (key.trim() == "server-port").then(|| value.trim().parse::<u16>().ok()).flatten()
            })
        })
        .unwrap_or(instance.port)
}

fn write_instance_port(folder: &Path, core: &str, port: u16) -> Result<(), String> {
    if pumpkin::is_pumpkin(core) {
        return pumpkin::write_port(folder, port);
    }
    if core == "Velocity" {
        let config = folder.join("velocity.toml");
        if let Ok(text) = fs::read_to_string(&config) {
            let updated = replace_config_line(&text, "bind", &format!("bind = \"0.0.0.0:{port}\""));
            return fs::write(config, updated).map_err(|error| format!("Could not write the port to velocity.toml: {error}"));
        }
        return Ok(());
    }
    if core == "BungeeCord" {
        let config = folder.join("config.yml");
        if let Ok(text) = fs::read_to_string(&config) {
            let updated = replace_config_line(&text, "host", &format!("  host: 0.0.0.0:{port}"));
            return fs::write(config, updated).map_err(|error| format!("Could not write the port to config.yml: {error}"));
        }
        return Ok(());
    }
    let properties = folder.join("server.properties");
    let current = fs::read_to_string(&properties).unwrap_or_default();
    fs::write(properties, with_port(&current, port)).map_err(|error| format!("Could not write the port to server.properties: {error}"))
}

fn with_port(current: &str, port: u16) -> String {
    let mut lines = current
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("server-port=") {
                format!("server-port={port}")
            } else if line.trim_start().starts_with("query.port=") {
                format!("query.port={port}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>();
    if !lines.iter().any(|line| line.trim_start().starts_with("server-port=")) {
        lines.push(format!("server-port={port}"));
    }
    format!("{}\n", lines.join("\n"))
}

fn replace_config_line(text: &str, key: &str, replacement: &str) -> String {
    let mut replaced = false;
    let mut lines = text
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if !replaced && (trimmed.starts_with(&format!("{key} =")) || trimmed.starts_with(&format!("{key}:"))) {
                replaced = true;
                replacement.to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>();
    if !replaced {
        lines.push(replacement.to_string());
    }
    format!("{}\n", lines.join("\n"))
}

fn needs_args_file(instance: &ServerInstance) -> bool {
    let root = Path::new(&instance.path);
    let has_jar = instance.server_jar.as_ref().map(|jar| root.join(jar).is_file()).unwrap_or(false) || find_jar(root).is_some();
    (!has_jar || matches!(instance.core.as_str(), "Forge" | "NeoForge")) && installed_args_file(root).is_some()
}

fn detect_launch_mode(_core: &str, folder: &Path) -> String {
    if installed_args_file(folder).is_some() {
        "script".into()
    } else {
        default_launch_mode()
    }
}

fn remote_still_starting(state: &AppState, id: &str) -> bool {
    state.remote_starting.lock().expect("remote state poisoned").get(id).is_some_and(|since| since.elapsed() < Duration::from_secs(600))
}

fn instance_view(instance: ServerInstance, state: &AppState) -> InstanceView {
    if !instance.panel_host.is_empty() {
        let live = state.panel_live.lock().expect("panel state poisoned").get(&instance.id).cloned();
        let checking = live.is_none() && !state.panel_checked.lock().expect("panel state poisoned").contains(&instance.id);
        let unreachable = live.is_none() && state.panel_unreachable.lock().expect("panel state poisoned").contains(&instance.id);
        let now = live.as_ref().map(|one| one.stats.state.as_str()).unwrap_or("offline");
        let status = match now {
            "running" | "starting" => "running",
            "stopping" => "stopping",
            _ => "stopped",
        };
        return InstanceView {
            uptime_seconds: live.as_ref().filter(|one| one.stats.uptime_ms > 0.0).map(|one| (one.stats.uptime_ms / 1000.0) as u64),
            crash_reason: None,
            ready: now == "running",
            checking,
            unreachable,
            missing: false,
            status: status.into(),
            instance,
        };
    }
    if !instance.remote_host.is_empty() {
        let live = state.remote_running.lock().expect("remote state poisoned").get(&instance.id).cloned();
        let starting = remote_still_starting(state, &instance.id);
        let unreachable = live.is_none() && state.remote_unreachable.lock().expect("remote state poisoned").contains(&instance.remote_host);
        let checking = live.is_none() && !state.remote_checked.lock().expect("remote state poisoned").contains(&instance.remote_host);
        return InstanceView {
            instance,
            status: if live.is_some() { "running".into() } else { "stopped".into() },
            uptime_seconds: None,
            crash_reason: live.as_ref().and_then(|one| {
                if one.ours {
                    None
                } else if one.session.is_empty() {
                    Some(format!("Running outside the app (pid {}) · console can't attach", one.pid))
                } else {
                    Some(format!("Running outside the app · took over session {}", one.session))
                }
            }),
            ready: live.is_some() && !starting,
            checking,
            unreachable,
            missing: false,
        };
    }
    let process_map = state.processes.lock().expect("process state poisoned");
    if let Some(process) = process_map.get(&instance.id) {
        let uptime = process.started_at.elapsed().ok().map(|duration| duration.as_secs());
        let status = if process.stop_requested_at.is_some() { "stopping" } else { "running" };
        return InstanceView {
            instance,
            status: status.into(),
            uptime_seconds: uptime,
            crash_reason: None,
            ready: process.ready,
            checking: false,
            unreachable: false,
            missing: false,
        };
    }
    drop(process_map);
    let missing = !Path::new(&instance.path).is_dir();
    let crashed = state.crashed.lock().expect("crash state poisoned").get(&instance.id).cloned();
    if let Some(reason) = crashed {
        return InstanceView {
            instance,
            status: "crashed".into(),
            uptime_seconds: None,
            crash_reason: Some(reason),
            ready: false,
            checking: false,
            unreachable: false,
            missing,
        };
    }
    InstanceView {
        instance,
        status: "stopped".into(),
        uptime_seconds: None,
        crash_reason: None,
        ready: false,
        checking: false,
        unreachable: false,
        missing,
    }
}

#[tauri::command(async)]
#[specta::specta]
fn list_instances(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<InstanceView>, String> {
    let mut instances = if RESNIFFED.swap(true, Ordering::SeqCst) {
        load_instances(&app)?
    } else {
        let _store = lock_instance_store();
        let mut instances = load_instances(&app)?;
        if resniff_unknown_cores(&mut instances) {
            save_instances(&app, &instances)?;
        }
        instances
    };
    instances.sort_by(|left, right| left.sort_order.cmp(&right.sort_order).then_with(|| left.created_at.cmp(&right.created_at)));
    Ok(instances.into_iter().map(|instance| instance_view(instance, &state)).collect())
}

static RESNIFFED: AtomicBool = AtomicBool::new(false);

fn resniff_unknown_cores(instances: &mut [ServerInstance]) -> bool {
    let mut changed = false;
    for instance in instances
        .iter_mut()
        .filter(|instance| instance.core == sniff::UNKNOWN && instance.remote_host.is_empty() && instance.panel_host.is_empty())
    {
        let folder = Path::new(&instance.path);
        let pumpkin = folder.join(cores::PUMPKIN_EXE);
        let jar =
            instance.server_jar.clone().map(PathBuf::from).or_else(|| if pumpkin.is_file() { Some(pumpkin) } else { find_jar(folder) });
        let found = sniff::sniff(folder, jar.as_deref());
        if found.core == sniff::UNKNOWN {
            continue;
        }
        instance.core = found.core;
        if instance.version == "Needs review" {
            if let Some(version) = found.version {
                instance.version = version;
            }
        }
        changed = true;
    }
    changed
}

#[tauri::command(async)]
#[specta::specta]
fn reorder_instances(app: AppHandle, ids: Vec<String>) -> Result<(), String> {
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    if ids.len() != instances.len() || ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err("The server order is malformed.".into());
    }
    for (order, id) in ids.iter().enumerate() {
        let instance = instances.iter_mut().find(|instance| instance.id == *id).ok_or("The server order contains an unknown server.")?;
        instance.sort_order = order as u32;
    }
    save_instances(&app, &instances)
}

#[tauri::command(async)]
#[specta::specta]
fn import_instance(app: AppHandle, state: State<'_, AppState>, folder: String, copy: Option<bool>) -> Result<InstanceView, String> {
    let folder = normalized_existing_path(&folder)?;
    let folder = if copy.unwrap_or(false) { copy_into_managed_root(&app, &folder)? } else { folder };
    let pumpkin = folder.join(cores::PUMPKIN_EXE);
    let jar = if pumpkin.is_file() { Some(pumpkin) } else { find_jar(&folder) };
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    if instances.iter().any(|instance| Path::new(&instance.path) == folder) {
        return Err("This folder is already registered.".into());
    }
    let name = folder.file_name().and_then(|name| name.to_str()).unwrap_or("Unnamed server").to_string();
    let sniff::Sniffed { core, version } = sniff::sniff(&folder, jar.as_deref());
    let version = version.unwrap_or_else(|| "Needs review".into());
    let probe = ServerInstance {
        id: String::new(),
        name: String::new(),
        path: folder.to_string_lossy().to_string(),
        core: core.clone(),
        version: version.clone(),
        launch_mode: default_launch_mode(),
        server_jar: None,
        java_path: "java".into(),
        max_memory_mib: 4096,
        min_memory_mib: 4096,
        jvm_args: String::new(),
        port: if kinds::is_proxy(&core) { 25577 } else { 25565 },
        sort_order: 0,
        group: String::new(),
        start_with_app: false,
        crash_restart: false,
        core_build: String::new(),
        created_at: 0,
        last_run_at: 0,
        custom_command: String::new(),
        start_script: String::new(),
        start_script_hash: String::new(),
        remote_host: String::new(),
        linked: false,
        panel_host: String::new(),
        panel_server: String::new(),
        panel_daemon: String::new(),
        named: false,
    };
    let instance = ServerInstance {
        id: instance_id(),
        name,
        path: folder.to_string_lossy().to_string(),
        core: core.clone(),
        version,
        launch_mode: detect_launch_mode(&core, &folder),
        server_jar: jar.map(|path| path.to_string_lossy().to_string()),
        java_path: "java".into(),
        max_memory_mib: 4096,
        min_memory_mib: 4096,
        jvm_args: String::new(),
        port: port_from_properties(&probe),
        sort_order: 0,
        group: String::new(),
        start_with_app: false,
        crash_restart: false,
        core_build: String::new(),
        created_at: unix_now(),
        last_run_at: 0,
        custom_command: String::new(),
        start_script: String::new(),
        start_script_hash: String::new(),
        remote_host: String::new(),
        linked: false,
        panel_host: String::new(),
        panel_server: String::new(),
        panel_daemon: String::new(),
        named: false,
    };
    instances.push(instance.clone());
    save_instances(&app, &instances)?;
    Ok(instance_view(instance, &state))
}

fn copy_into_managed_root(app: &AppHandle, source: &Path) -> Result<PathBuf, String> {
    let root = managed_servers_root(app)?;
    let root_real = root.canonicalize().map(plain_path).unwrap_or(root.clone());
    if source.starts_with(&root_real) || source.starts_with(&root) {
        return Ok(source.to_path_buf());
    }
    if root_real.starts_with(source) || root.starts_with(source) || source.parent().is_none() {
        return Err("This folder holds the app's own server folder (or is a whole drive), so it can't be copied into it. Choose the server's own folder.".into());
    }
    let name = source.file_name().and_then(|name| name.to_str()).unwrap_or("Imported server");
    let destination = free_folder(&root, name);
    let total = count_files(source);
    let mut done = 0usize;
    let _ = app.emit("archive-import", serde_json::json!({ "stage": "copying", "done": 0, "total": total }));
    let copied = copy_instance_folder(source, &destination, &mut || {
        done += 1;
        if done.is_multiple_of(50) || done == total {
            let _ = app.emit("archive-import", serde_json::json!({ "stage": "copying", "done": done, "total": total }));
        }
    });
    if let Err(error) = copied {
        let _ = fs::remove_dir_all(&destination);
        return Err(error);
    }
    let _ = app.emit("archive-import", serde_json::json!({ "stage": "complete" }));
    destination.canonicalize().map(plain_path).map_err(|error| format!("Could not verify the server folder: {error}"))
}

fn archive_root_prefix(names: &[String]) -> Option<String> {
    let markers = ["server.properties", "server.jar", "eula.txt", "run.bat", "velocity.toml", "bungeecord.jar"];
    let is_marker = |name: &str| markers.iter().any(|marker| name.eq_ignore_ascii_case(marker));
    if names.iter().any(|name| is_marker(name)) {
        return None;
    }
    names.iter().find_map(|name| {
        let (prefix, rest) = name.split_once('/')?;
        is_marker(rest).then(|| format!("{prefix}/"))
    })
}

fn chosen_root(into: Option<&str>) -> Result<Option<PathBuf>, String> {
    let Some(picked) = into.map(str::trim).filter(|one| !one.is_empty()) else {
        return Ok(None);
    };
    let path = PathBuf::from(picked);
    if !path.is_dir() {
        return Err("The folder to extract into was not found.".into());
    }
    path.canonicalize().map(plain_path).map(Some).map_err(|error| format!("Could not verify the folder to extract into: {error}"))
}

#[tauri::command(async)]
#[specta::specta]
fn import_instance_archive(
    app: AppHandle,
    state: State<'_, AppState>,
    archive_path: String,
    into: Option<String>,
) -> Result<InstanceView, String> {
    let archive_path = PathBuf::from(archive_path.trim());
    if !archive_path.is_file() {
        return Err("The chosen archive was not found.".into());
    }
    let file = fs::File::open(&archive_path).map_err(|error| format!("Could not open the archive: {error}"))?;
    let mut archive = ZipArchive::new(file).map_err(|_| "Only zip archives can be imported.".to_string())?;
    let names = archive.file_names().filter_map(|name| name.ok().map(|name| name.into_owned())).collect::<Vec<String>>();
    if names.is_empty() {
        return Err("The archive is empty.".into());
    }
    let prefix = archive_root_prefix(&names);
    let suggested = archive_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| "Imported server".into());
    let root = match chosen_root(into.as_deref())? {
        Some(picked) => picked,
        None => managed_servers_root(&app)?,
    };
    let folder = free_folder(&root, &suggested);
    fs::create_dir_all(&folder).map_err(|error| format!("Could not create the server folder: {error}"))?;
    let folder = folder.canonicalize().map(plain_path).map_err(|error| format!("Could not verify the server folder: {error}"))?;
    let extract = (|| -> Result<(), String> {
        let total = archive.len();
        for index in 0..total {
            let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
            let Some(name) = entry.enclosed_name() else {
                continue;
            };
            let relative = match &prefix {
                Some(prefix) => match name.strip_prefix(prefix.trim_end_matches('/')) {
                    Ok(rest) => rest.to_path_buf(),
                    Err(_) => continue,
                },
                None => name,
            };
            if relative.as_os_str().is_empty() {
                continue;
            }
            let output = folder.join(&relative);
            if !output.starts_with(&folder) {
                return Err("The archive has a path pointing outside the server folder.".into());
            }
            if entry.is_dir() {
                fs::create_dir_all(&output).map_err(|error| error.to_string())?;
                continue;
            }
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut target = fs::File::create(&output).map_err(|error| error.to_string())?;
            std::io::copy(&mut entry, &mut target).map_err(|error| error.to_string())?;
            let _ = app.emit("archive-import", serde_json::json!({ "stage": "extracting", "done": index + 1, "total": total }));
        }
        Ok(())
    })();
    if let Err(error) = extract {
        let _ = fs::remove_dir_all(&folder);
        return Err(error);
    }
    let _ = app.emit("archive-import", serde_json::json!({ "stage": "complete" }));
    import_instance(app, state, folder.to_string_lossy().to_string(), None)
}

fn count_files(folder: &Path) -> usize {
    fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| {
                    let path = entry.path();
                    if path.is_dir() {
                        if path.file_name().is_some_and(|name| name == ".monolab-backups") {
                            0
                        } else {
                            count_files(&path)
                        }
                    } else {
                        1
                    }
                })
                .sum()
        })
        .unwrap_or(0)
}

fn copy_instance_folder(source: &Path, destination: &Path, tick: &mut dyn FnMut()) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())?.flatten() {
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == ".monolab-backups") {
                continue;
            }
            copy_instance_folder(&path, &target, tick)?;
        } else if path.is_file() {
            fs::copy(&path, &target).map_err(|error| format!("Could not duplicate the server files: {error}"))?;
            tick();
        }
    }
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
fn rename_instance(app: AppHandle, state: State<'_, AppState>, input: RenameInstanceInput) -> Result<InstanceView, String> {
    let name = input.name.trim();
    if name.is_empty() || name.len() > 80 || name.contains(['\\', '/', ':']) {
        return Err("Server names are 1–80 characters and cannot contain \\ / :".into());
    }
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    let instance = instances.iter_mut().find(|instance| instance.id == input.id).ok_or("Server not found.")?;
    instance.name = name.into();
    instance.named = true;
    let saved = instance.clone();
    save_instances(&app, &instances)?;
    Ok(instance_view(saved, &state))
}

fn clean_group(group: &str) -> Result<String, String> {
    let group = group.trim();
    if group.chars().count() > 40 || group.contains(['\r', '\n']) {
        return Err("Group names are up to 40 characters on one line.".into());
    }
    Ok(group.to_string())
}

/// Puts a server in a home screen group; an empty name takes it out.
#[tauri::command]
#[specta::specta]
fn set_server_group(app: AppHandle, state: State<'_, AppState>, id: String, group: String) -> Result<InstanceView, String> {
    let group = clean_group(&group)?;
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    let instance = instances.iter_mut().find(|instance| instance.id == id).ok_or("Server not found.")?;
    instance.group = group;
    let saved = instance.clone();
    save_instances(&app, &instances)?;
    Ok(instance_view(saved, &state))
}

/// Renames a group for every server in it; an empty new name ungroups them.
#[tauri::command]
#[specta::specta]
fn rename_server_group(app: AppHandle, from: String, to: String) -> Result<u32, String> {
    let to = clean_group(&to)?;
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    let mut moved = 0;
    for instance in instances.iter_mut().filter(|instance| instance.group == from) {
        instance.group = to.clone();
        moved += 1;
    }
    save_instances(&app, &instances)?;
    Ok(moved)
}

#[tauri::command]
#[specta::specta]
async fn duplicate_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<InstanceView, String> {
    let source = instance_by_id(&app, &id)?;
    if !source.remote_host.is_empty() {
        return duplicate_remote(&app, &state, source).await;
    }
    only_local(&app, &id, "Duplicate server")?;
    ensure_stopped(&state, &id)?;
    let worker = app.clone();
    let made =
        tauri::async_runtime::spawn_blocking(move || duplicate_local(&worker, source)).await.map_err(|error| error.to_string())??;
    Ok(instance_view(made, &state))
}

/// A new local server's port: 25565 unless another local server or program has it, then one up at a time.
fn new_server_port(instances: &[ServerInstance], from: u16) -> u16 {
    let mut port = from;
    while port < u16::MAX
        && (instances.iter().any(|instance| !is_far(instance) && instance.port == port) || TcpListener::bind(("0.0.0.0", port)).is_err())
    {
        port += 1;
    }
    port
}

/// Writes a picked port into a new server folder before its first start (only when it is not the default).
fn claim_new_server_port(app: &AppHandle, folder: &Path, core: &str, default_port: u16) -> u16 {
    if kinds::is_proxy(core) || pumpkin::is_pumpkin(core) {
        return default_port;
    }
    let port = new_server_port(&load_instances(app).unwrap_or_default(), default_port);
    if port != default_port {
        let properties = folder.join("server.properties");
        let current = fs::read_to_string(&properties).unwrap_or_default();
        if fs::write(&properties, with_port(&current, port)).is_err() {
            return default_port;
        }
    }
    port
}

/// Nothing on this PC listens on the port yet.
fn port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
}

/// Saves a server's port in the app's list (after server.properties changed).
fn set_instance_port(app: &AppHandle, id: &str, port: u16) -> Result<(), String> {
    let mut all = load_instances(app)?;
    if let Some(one) = all.iter_mut().find(|one| one.id == id) {
        one.port = port;
        save_instances(app, &all)?;
    }
    Ok(())
}

fn next_free_port(instances: &[ServerInstance], from: u16) -> u16 {
    let mut port = from.saturating_add(1).max(1024);
    while instances.iter().any(|instance| instance.port == port) {
        port = port.saturating_add(1);
    }
    port
}

async fn duplicate_remote(app: &AppHandle, state: &AppState, source: ServerInstance) -> Result<InstanceView, String> {
    ensure_remote_stopped(app, state, &source).await?;
    let made = remote::copy_folder(app, &source.remote_host, &source.path).await?;
    let port = next_free_port(&load_instances(app)?, source.port);
    let mut duplicate = source.clone();
    duplicate.id = instance_id();
    duplicate.name = format!("{} copy", source.name);
    duplicate.path = made.clone();
    duplicate.created_at = unix_now();
    duplicate.port = port;
    if !pumpkin::is_pumpkin(&source.core) && !kinds::is_proxy(&source.core) {
        let current = remote::read_text(app, &source.remote_host, &made, "server.properties").await.unwrap_or_default();
        remote::write_text(app, &source.remote_host, &made, "server.properties", &with_port(&current, port)).await?;
    }
    let _store = lock_instance_store();
    let mut instances = load_instances(app)?;
    duplicate.sort_order = instances.iter().map(|instance| instance.sort_order).max().unwrap_or(0).saturating_add(1);
    instances.push(duplicate.clone());
    save_instances(app, &instances)?;
    record_audit(app, &duplicate.id, "Server", "Duplicated (SSH)", &made);
    Ok(instance_view(duplicate, state))
}

fn duplicate_local(app: &AppHandle, source: ServerInstance) -> Result<ServerInstance, String> {
    let source_path = PathBuf::from(&source.path);
    let parent = source_path.parent().ok_or("Could not find the parent of the server folder.")?;
    let stem = source_path.file_name().and_then(|name| name.to_str()).ok_or("Could not read the server folder name.")?;
    let mut destination = parent.join(format!("{stem} - copy"));
    for number in 2..1000 {
        if !destination.exists() {
            break;
        }
        destination = parent.join(format!("{stem} - copy {number}"));
    }
    if destination.exists() {
        return Err("Could not create a folder name for the copy.".into());
    }
    copy_instance_folder(&source_path, &destination, &mut || {})?;
    let _store = lock_instance_store();
    let mut instances = load_instances(app)?;
    let mut duplicate = source.clone();
    duplicate.id = instance_id();
    duplicate.name = format!("{} copy", source.name);
    duplicate.path = destination.to_string_lossy().to_string();
    duplicate.created_at = unix_now();
    duplicate.sort_order = instances.iter().map(|instance| instance.sort_order).max().unwrap_or(0).saturating_add(1);
    if let Some(jar) = &source.server_jar {
        if let Ok(relative) = Path::new(jar).strip_prefix(&source_path) {
            duplicate.server_jar = Some(destination.join(relative).to_string_lossy().to_string());
        }
    }
    let port = next_free_port(&instances, source.port);
    duplicate.port = port;
    if let Err(error) = write_instance_port(&destination, &duplicate.core, port) {
        let _ = fs::remove_dir_all(&destination);
        return Err(error);
    }
    instances.push(duplicate.clone());
    save_instances(app, &instances)?;
    Ok(duplicate)
}

#[tauri::command(async)]
#[specta::specta]
async fn trash_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    ensure_stopped(&state, &id)?;
    let instance = instance_by_id(&app, &id)?;
    if !instance.remote_host.is_empty() {
        if state.remote_running.lock().expect("remote state poisoned").contains_key(&id) {
            return Err("Stop the server first.".into());
        }
        let _store = lock_instance_store();
        let mut instances = load_instances(&app)?;
        instances.retain(|saved| saved.id != id);
        save_instances(&app, &instances)?;
        record_audit(&app, &id, "Server", "Remove from list (SSH)", &instance.path);
        return Ok(());
    }
    if !instance.panel_host.is_empty() {
        panel::hide_server(&app, &instance.panel_host, &instance.panel_server)?;
        let _store = lock_instance_store();
        let mut instances = load_instances(&app)?;
        instances.retain(|saved| saved.id != id);
        save_instances(&app, &instances)?;
        record_audit(&app, &id, "Server", "Removed from list (panel)", "The server on the panel is untouched.");
        return Ok(());
    }
    if instance.linked {
        let _store = lock_instance_store();
        let mut instances = load_instances(&app)?;
        instances.retain(|saved| saved.id != id);
        save_instances(&app, &instances)?;
        record_audit(&app, &id, "Server", "Remove from list (folder kept)", &instance.path);
        return Ok(());
    }
    let source = PathBuf::from(&instance.path);
    if !source.exists() {
        let _store = lock_instance_store();
        let mut instances = load_instances(&app)?;
        instances.retain(|saved| saved.id != id);
        save_instances(&app, &instances)?;
        record_audit(&app, &id, "Server", "Remove from list (folder was already gone)", &instance.path);
        return Ok(());
    }
    if !source.is_dir() {
        return Err("Server folder not found.".into());
    }
    let parent = source.parent().ok_or("Could not find the parent of the server folder.")?;
    let inside = app
        .path()
        .monolab_data_dir()
        .map(|data| data.join("trash"))
        .ok()
        .filter(|home| fs::create_dir_all(home).is_ok())
        .map(|home| home.join(format!("{}-{}", instance.id, unix_now())))
        .filter(|target| fs::rename(&source, target).is_ok());
    let target = match inside {
        Some(target) => target,
        None => {
            let trash = parent.join(".monolab-trash");
            fs::create_dir_all(&trash).map_err(|error| error.to_string())?;
            let beside = trash.join(format!("{}-{}", instance.id, unix_now()));
            fs::rename(&source, &beside).map_err(|error| format!("Could not move the server folder to the trash: {error}"))?;
            beside
        }
    };
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    instances.retain(|saved| saved.id != id);
    save_instances(&app, &instances)?;
    let mut entries = load_trash_entries(&app)?;
    entries.push(TrashEntry {
        id: instance.id.clone(),
        name: instance.name.clone(),
        original_path: source.to_string_lossy().to_string(),
        trash_path: target.to_string_lossy().to_string(),
        deleted_at: unix_now(),
        instance: Some(instance),
    });
    save_trash_entries(&app, &entries)
}

#[tauri::command(async)]
#[specta::specta]
fn list_trash_instances(app: AppHandle) -> Result<Vec<TrashEntry>, String> {
    let mut entries = load_trash_entries(&app)?;
    entries.sort_by_key(|left| std::cmp::Reverse(left.deleted_at));
    Ok(entries)
}

#[tauri::command(async)]
#[specta::specta]
fn purge_trash_instance(app: AppHandle, id: String) -> Result<Vec<TrashEntry>, String> {
    let mut entries = load_trash_entries(&app)?;
    let Some(at) = entries.iter().position(|entry| entry.id == id) else {
        return Ok(entries);
    };
    let entry = entries.remove(at);
    let path = PathBuf::from(&entry.trash_path);
    if path.is_dir() {
        fs::remove_dir_all(&path).map_err(|error| format!("Could not delete {}: {error}", entry.name))?;
    }
    save_trash_entries(&app, &entries)?;
    record_audit(&app, &entry.id, "Server", "Deleted from trash", &format!("Permanently deleted the files of {}.", entry.name));
    Ok(entries)
}

#[tauri::command(async)]
#[specta::specta]
fn empty_trash(app: AppHandle) -> Result<usize, String> {
    let entries = load_trash_entries(&app)?;
    let mut failed = Vec::new();
    for entry in &entries {
        let path = PathBuf::from(&entry.trash_path);
        if path.is_dir() {
            if let Err(error) = fs::remove_dir_all(&path) {
                failed.push(format!("{}({error})", entry.name));
            }
        }
    }
    if !failed.is_empty() {
        let left: Vec<TrashEntry> = entries.into_iter().filter(|entry| PathBuf::from(&entry.trash_path).is_dir()).collect();
        save_trash_entries(&app, &left)?;
        return Err(format!("Some items could not be deleted: {}", failed.join(", ")));
    }
    let count = entries.len();
    save_trash_entries(&app, &[])?;
    record_audit(&app, "", "Server", "Trash emptied", &format!("Permanently deleted {count} server files."));
    Ok(count)
}

#[tauri::command(async)]
#[specta::specta]
fn restore_trash_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<InstanceView, String> {
    let mut entries = load_trash_entries(&app)?;
    let index = entries.iter().position(|entry| entry.id == id).ok_or("Trashed server not found.")?;
    let entry = entries[index].clone();
    let source = PathBuf::from(&entry.trash_path);
    let target = PathBuf::from(&entry.original_path);
    if !source.is_dir() {
        return Err("The trashed server folder was not found.".into());
    }
    if target.exists() {
        return Err("A folder with the same name already exists at the original location.".into());
    }
    fs::rename(&source, &target).map_err(|error| format!("Could not restore the server folder: {error}"))?;
    let result = match entry.instance.clone() {
        Some(saved) => restore_saved_instance(&app, saved, &target),
        None => import_instance(app.clone(), state, target.to_string_lossy().to_string(), None).map(|view| view.instance),
    };
    let restored = match result {
        Ok(instance) => instance,
        Err(error) => {
            let _ = fs::rename(&target, &source);
            return Err(error);
        }
    };
    entries.remove(index);
    save_trash_entries(&app, &entries)?;
    let state = app.state::<AppState>();
    Ok(instance_view(restored, &state))
}

fn restore_saved_instance(app: &AppHandle, mut saved: ServerInstance, folder: &Path) -> Result<ServerInstance, String> {
    saved.path = folder.to_string_lossy().to_string();
    let _store = lock_instance_store();
    let mut instances = load_instances(app)?;
    if instances.iter().any(|instance| instance.id == saved.id) {
        return Err("This server is already in the list.".into());
    }
    if instances.iter().any(|instance| Path::new(&instance.path) == folder) {
        return Err("This folder is already registered.".into());
    }
    instances.push(saved.clone());
    save_instances(app, &instances)?;
    Ok(saved)
}

#[tauri::command(async)]
#[specta::specta]
fn create_instance(app: AppHandle, state: State<'_, AppState>, input: CreateInstanceInput) -> Result<InstanceView, String> {
    let name = if input.name.trim().is_empty() { peek_next_server_name(&app) } else { input.name.trim().to_string() };
    if !input.eula_accepted {
        return Err("Creating a server requires accepting the Minecraft EULA.".into());
    }
    let folder = free_folder(&managed_servers_root(&app)?, &name);
    fs::create_dir_all(&folder).map_err(|error| format!("Could not create the server folder: {error}"))?;
    let folder = folder.canonicalize().map(plain_path).map_err(|error| format!("Could not verify the server folder: {error}"))?;
    if load_instances(&app)?.iter().any(|instance| Path::new(&instance.path) == folder) {
        return Err("This folder is already registered.".into());
    }
    fs::write(folder.join("eula.txt"), "eula=true\n").map_err(|error| format!("Could not create the EULA file: {error}"))?;
    if !input.operation_id.is_empty() {
        state.cancelled_downloads.lock().expect("download state poisoned").remove(&input.operation_id);
    }
    let jar = match download_server_jar(&app, &input.core, &input.version, picked_build(&input.core_build), &folder, &input.operation_id) {
        Ok(path) => path,
        Err(error) => {
            let _ = fs::remove_dir_all(&folder);
            return Err(error);
        }
    };
    fs::write(folder.join("eula.txt"), "eula=true\n").map_err(|error| format!("Could not create the EULA file: {error}"))?;
    let default_port = if kinds::is_proxy(&input.core) { 25577 } else { 25565 };
    let default_port = claim_new_server_port(&app, &folder, &input.core, default_port);
    let instance = ServerInstance {
        id: instance_id(),
        name,
        path: folder.to_string_lossy().to_string(),
        core: input.core,
        version: input.version,
        launch_mode: if jar.is_some() { "jar".into() } else { "script".into() },
        server_jar: jar.map(|path| path.to_string_lossy().to_string()),
        java_path: "java".into(),
        max_memory_mib: input.max_memory_mib.clamp(512, 65536),
        min_memory_mib: sane_min_memory(input.min_memory_mib, input.max_memory_mib.clamp(512, 65536)),
        jvm_args: String::new(),
        port: default_port,
        sort_order: 0,
        group: String::new(),
        start_with_app: false,
        crash_restart: false,
        core_build: input.core_build.trim().to_string(),
        created_at: unix_now(),
        last_run_at: 0,
        custom_command: String::new(),
        start_script: String::new(),
        start_script_hash: String::new(),
        remote_host: String::new(),
        linked: false,
        panel_host: String::new(),
        panel_server: String::new(),
        panel_daemon: String::new(),
        named: false,
    };
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    instances.push(instance.clone());
    save_instances(&app, &instances)?;
    drop(_store);
    Ok(instance_view(instance, &state))
}

#[tauri::command(async)]
#[specta::specta]
fn servers_root_free_bytes(app: AppHandle) -> Result<u64, String> {
    Ok(local_free_bytes(&managed_servers_root(&app)?))
}

#[tauri::command(async)]
#[specta::specta]
fn create_instance_from_jar(app: AppHandle, state: State<'_, AppState>, jar_path: String) -> Result<InstanceView, String> {
    let source = PathBuf::from(jar_path.trim());
    if !source.is_file() || source.extension().is_none_or(|extension| !extension.eq_ignore_ascii_case("jar")) {
        return Err("Choose a jar file.".into());
    }
    verify_java_archive(&source)?;

    let name = peek_next_server_name(&app);
    let folder = free_folder(&managed_servers_root(&app)?, &name);
    fs::create_dir_all(&folder).map_err(|error| format!("Could not create the server folder: {error}"))?;
    let folder = folder.canonicalize().map(plain_path).map_err(|error| format!("Could not verify the server folder: {error}"))?;

    let destination = folder.join("server.jar");
    if let Err(error) = fs::copy(&source, &destination) {
        let _ = fs::remove_dir_all(&folder);
        return Err(format!("Could not copy the core file: {error}"));
    }
    fs::write(
        folder.join("eula.txt"),
        "eula=true
",
    )
    .map_err(|error| format!("Could not create the EULA file: {error}"))?;

    let sniff::Sniffed { core, version } = sniff::sniff(&folder, Some(&destination));
    let version = version.unwrap_or_else(|| "Needs review".into());
    let instance = ServerInstance {
        id: instance_id(),
        name,
        path: folder.to_string_lossy().to_string(),
        core: core.clone(),
        version,
        launch_mode: default_launch_mode(),
        server_jar: Some(destination.to_string_lossy().to_string()),
        java_path: "java".into(),
        max_memory_mib: default_memory_mib(),
        min_memory_mib: default_memory_mib(),
        jvm_args: String::new(),
        port: claim_new_server_port(&app, &folder, &core, if core == "Velocity" { 25577 } else { 25565 }),
        sort_order: 0,
        group: String::new(),
        start_with_app: false,
        crash_restart: false,
        core_build: String::new(),
        created_at: unix_now(),
        last_run_at: 0,
        custom_command: String::new(),
        start_script: String::new(),
        start_script_hash: String::new(),
        remote_host: String::new(),
        linked: false,
        panel_host: String::new(),
        panel_server: String::new(),
        panel_daemon: String::new(),
        named: false,
    };
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    instances.push(instance.clone());
    save_instances(&app, &instances)?;
    drop(_store);
    record_audit(&app, &instance.id, "Server", "Created from a core file", &source.to_string_lossy());
    Ok(instance_view(instance, &state))
}

#[tauri::command(async)]
#[specta::specta]
fn cancel_create_download(state: State<'_, AppState>, operation_id: String) -> Result<(), String> {
    if operation_id.trim().is_empty() {
        return Err("No download to cancel.".into());
    }
    state.cancelled_downloads.lock().expect("download state poisoned").insert(operation_id);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
fn open_server_folder(app: AppHandle, id: String, inside: Option<String>) -> Result<(), String> {
    only_local(&app, &id, "Open folder in Explorer")?;
    let instance = instance_by_id(&app, &id)?;
    let root = Path::new(&instance.path);
    let path = match inside.as_deref().map(str::trim).filter(|one| !one.is_empty()) {
        Some(relative) => paths::resolve_in_server(root, relative).map_err(|_| "That folder was not found.".to_string())?,
        None => root.to_path_buf(),
    };
    if !path.is_dir() {
        return Err("Server folder not found.".into());
    }
    #[cfg(windows)]
    {
        hidden_command("explorer").arg(&path).spawn().map_err(|error| format!("Could not open Explorer: {error}"))?;
    }
    #[cfg(not(windows))]
    {
        hidden_command("xdg-open").arg(&path).spawn().map_err(|error| format!("Could not open the folder: {error}"))?;
    }
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command(async)]
#[specta::specta]
fn open_notices(app: AppHandle) -> Result<(), String> {
    let path = app
        .path()
        .resolve("resources/THIRD-PARTY-NOTICES.txt", BaseDirectory::Resource)
        .map_err(|error| format!("The notices file could not be found: {error}"))?;
    if !path.is_file() {
        return Err("The notices file bundled with the app is missing. Try reinstalling the app.".into());
    }
    #[cfg(windows)]
    {
        hidden_command("explorer").arg(&path).spawn().map_err(|error| format!("Could not open the file: {error}"))?;
    }
    #[cfg(not(windows))]
    {
        hidden_command("xdg-open").arg(&path).spawn().map_err(|error| format!("Could not open the file: {error}"))?;
    }
    Ok(())
}

fn check_script_name(name: &str, remote: bool) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Pick a start file.".into());
    }
    if name.contains(['/', '\\', ':']) || name.starts_with('.') {
        return Err("The start file must be a single file name at the top of the server folder.".into());
    }
    if name.chars().any(|letter| "&|<>^%\"'$`;\n\r\0*?".contains(letter)) {
        return Err("The start file name has characters that can't be used.".into());
    }
    let lower = name.to_ascii_lowercase();
    let fits = if remote { lower.ends_with(".sh") } else { lower.ends_with(".bat") || lower.ends_with(".cmd") };
    if !fits {
        return Err(if remote {
            "SSH servers can start from a .sh file.".into()
        } else {
            "Local servers can start from a .bat or .cmd file.".into()
        });
    }
    Ok(name.to_string())
}

async fn script_digest(app: &AppHandle, instance: &ServerInstance, name: &str) -> Result<String, String> {
    if instance.remote_host.is_empty() {
        let bytes = std::fs::read(Path::new(&instance.path).join(name))
            .map_err(|_| format!("Start file {name} was not found in the server folder."))?;
        return Ok(format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(&bytes)));
    }
    let conn = remote::connect(app, &instance.remote_host).await?;
    let path = format!("{}/{}", instance.path.trim_end_matches('/'), name);
    let out = conn.run(&format!("sha256sum -- {}", remote::quote(&path))).await?;
    let digest = out.stdout.split_whitespace().next().unwrap_or("").to_string();
    if digest.len() != 64 {
        return Err(format!("Start file {name} was not found in the server folder on the SSH server."));
    }
    Ok(digest)
}

async fn approve_script(app: &AppHandle, instance: &ServerInstance, trusted: bool) -> Result<String, String> {
    let name = check_script_name(&instance.start_script, !instance.remote_host.is_empty())?;
    let digest = script_digest(app, instance, &name).await?;
    if digest == instance.start_script_hash {
        return Ok(name);
    }
    if !trusted {
        return Err(format!(
            "Start file {name} changed since it was last started in the app. For safety, start it once from the app on this computer."
        ));
    }
    let _store = lock_instance_store();
    let mut instances = load_instances(app)?;
    if let Some(found) = instances.iter_mut().find(|one| one.id == instance.id) {
        found.start_script_hash = digest;
        save_instances(app, &instances)?;
    }
    record_audit(app, &instance.id, "Server", "Check start file", &name);
    Ok(name)
}

#[tauri::command(async)]
#[specta::specta]
async fn list_start_scripts(app: AppHandle, id: String) -> Result<Vec<String>, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        return Ok(Vec::new());
    }
    let remote = !instance.remote_host.is_empty();
    let names: Vec<String> = if remote {
        remote::list_dir(&app, &instance.remote_host, &instance.path, "")
            .await?
            .into_iter()
            .filter(|row| !row.directory)
            .map(|row| row.name)
            .collect()
    } else {
        std::fs::read_dir(&instance.path)
            .map_err(|error| format!("Could not open the server folder: {error}"))?
            .flatten()
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()
    };
    let mut found: Vec<String> = names.into_iter().filter(|name| check_script_name(name, remote).is_ok()).collect();
    found.sort_by_key(|name| name.to_lowercase());
    Ok(found)
}

#[tauri::command(async)]
#[specta::specta]
async fn set_start_script(app: AppHandle, id: String, script: String) -> Result<(), String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        return Err("Panel servers start the way the panel decides.".into());
    }
    let (name, digest) = if script.trim().is_empty() {
        (String::new(), String::new())
    } else {
        let name = check_script_name(&script, !instance.remote_host.is_empty())?;
        let digest = script_digest(&app, &instance, &name).await?;
        (name, digest)
    };
    {
        let _store = lock_instance_store();
        let mut instances = load_instances(&app)?;
        let found = instances.iter_mut().find(|one| one.id == id).ok_or("Server not found.")?;
        found.start_script = name.clone();
        found.start_script_hash = digest;
        if !name.is_empty() {
            found.custom_command = String::new();
        }
        save_instances(&app, &instances)?;
    }
    record_audit(&app, &id, "Server", if name.is_empty() { "Not using a start file" } else { "Start with start file" }, &name);
    Ok(())
}

fn watch_script_shell(pid: u32) {
    thread::spawn(move || {
        let root = Pid::from_u32(pid);
        let mut system = System::new();
        let mut seen_java = false;
        let mut gone_for = 0;
        loop {
            thread::sleep(Duration::from_secs(1));
            system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
            if system.process(root).is_none() {
                return;
            }
            let has_java = system.processes().values().any(|process| {
                let name = process.name().to_string_lossy().to_ascii_lowercase();
                if !name.starts_with("java") {
                    return false;
                }
                let mut current = process.parent();
                for _ in 0..16 {
                    match current {
                        Some(parent) if parent == root => return true,
                        Some(parent) => current = system.process(parent).and_then(|one| one.parent()),
                        None => return false,
                    }
                }
                false
            });
            if has_java {
                seen_java = true;
                gone_for = 0;
            } else if seen_java {
                gone_for += 1;
                if gone_for >= 2 {
                    kill_process_tree(pid);
                    return;
                }
            }
        }
    });
}

#[tauri::command(async)]
#[specta::specta]
fn set_custom_command(app: AppHandle, id: String, command: String) -> Result<(), String> {
    let text = command.trim();
    if !text.is_empty() {
        let (program, _) = launch::custom_command(text)?;
        if program != "java" {
            launch::verify_java(Path::new(&program), java::ask_java_major)?;
        }
    }
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    let found = instances.iter_mut().find(|instance| instance.id == id).ok_or("Server not found.")?;
    found.custom_command = text.to_string();
    if !text.is_empty() {
        found.start_script = String::new();
        found.start_script_hash = String::new();
    }
    save_instances(&app, &instances)?;
    record_audit(&app, &id, "Server", if text.is_empty() { "Start command set to automatic" } else { "Custom start command" }, text);
    Ok(())
}

/// From the web panel or Monolink, Java can only be one this PC found itself (or stay as it is):
/// checking a path runs it, so a path to a file someone uploaded would run their program here.
fn remote_java_allowed(app: &AppHandle, input: &LaunchSettingsInput) -> Result<(), String> {
    let java = input.java_path.trim();
    if java.is_empty() || java == "java" {
        return Ok(());
    }
    let instance = instance_by_id(app, &input.id)?;
    if instance.java_path.trim() == java {
        return Ok(());
    }
    if !instance.remote_host.is_empty() {
        return Err("The Java path of an SSH server can only be changed on the host PC.".into());
    }
    let same = |one: &str| plain_path(PathBuf::from(one)) == plain_path(PathBuf::from(java));
    if java::scan_installed_javas(app).iter().any(|one| same(&one.path)) {
        return Ok(());
    }
    Err("Pick one of the Java versions found on the host PC. Other paths can only be set on the host PC.".into())
}

#[tauri::command(async)]
#[specta::specta]
fn update_launch_settings(app: AppHandle, state: State<'_, AppState>, input: LaunchSettingsInput) -> Result<InstanceView, String> {
    if instance_by_id(&app, &input.id).is_ok_and(|one| !one.panel_host.is_empty()) {
        return Err("Memory and Java for panel servers are set by the panel. Change them in Overview's startup variables.".into());
    }
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    let instance = instances.iter_mut().find(|instance| instance.id == input.id).ok_or("Server not found.")?;
    let previous_jvm_args = instance.jvm_args.clone();
    instance.max_memory_mib = input.max_memory_mib.clamp(512, 65536);
    instance.min_memory_mib = sane_min_memory(input.min_memory_mib, instance.max_memory_mib);
    launch::jvm_arguments(&input.jvm_args)?;
    instance.jvm_args = input.jvm_args.trim().into();
    let java = input.java_path.trim();
    if !java.is_empty() && java != "java" {
        if instance.remote_host.is_empty() {
            launch::verify_java(Path::new(java), java::ask_java_major)?;
        } else {
            remote::check_java_path(java)?;
        }
    }
    instance.java_path = if java.is_empty() { "java".into() } else { java.into() };
    if instance.launch_mode == "script" {
        update_script_memory(
            &Path::new(&instance.path).join("user_jvm_args.txt"),
            instance.max_memory_mib,
            instance.min_memory_mib,
            &instance.jvm_args,
            &previous_jvm_args,
        )?;
    }
    let view = instance.clone();
    save_instances(&app, &instances)?;
    record_audit(
        &app,
        &view.id,
        "Settings",
        "Launch settings saved",
        &format!("Memory {}–{} MB, {} JVM flags", view.min_memory_mib, view.max_memory_mib, view.jvm_args.split_whitespace().count()),
    );
    Ok(instance_view(view, &state))
}

fn watch_process(
    app: AppHandle,
    state: Arc<Mutex<HashMap<String, ManagedProcess>>>,
    crashed: Arc<Mutex<HashMap<String, String>>>,
    server_id: String,
) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(500));
        let mut processes = state.lock().expect("process state poisoned");
        let Some(process) = processes.get_mut(&server_id) else {
            return;
        };
        match process.child.try_wait() {
            Ok(Some(exit)) => {
                let stop_requested = process.stop_requested_at.is_some();
                let crashed_now = !stop_requested || (process.stop_from_log && process.crash_seen);
                let ran = ran_minutes(process);
                processes.remove(&server_id);
                if crashed_now {
                    crashed.lock().expect("crash state poisoned").insert(server_id.clone(), format!("exit code {:?}", exit.code()));
                    let _ = announce(&app, &server_id, "Crash", "The server process crashed.");
                    note_run(
                        &app,
                        &server_id,
                        "crash",
                        &format!("exit code {} · ran {} min", exit.code().map_or("?".into(), |code| code.to_string()), ran),
                    );
                } else {
                    note_run(&app, &server_id, "stop", &format!("Ran {} min", ran));
                    crashed.lock().expect("crash state poisoned").remove(&server_id);
                }
                drop(processes);
                let done = serde_json::json!({ "serverId": server_id, "status": if crashed_now { "crashed" } else { "stopped" } });
                monolink_team::report(
                    &app,
                    &server_id,
                    if crashed_now { "server_crashed" } else { "server_stopped" },
                    serde_json::json!({}),
                );
                remember_for_web(&app, "server-status", &done);
                let _ = app.emit("server-status", done);
                if crashed_now {
                    restart_after_crash(&app, &server_id);
                }
                return;
            }
            Err(error) => {
                processes.remove(&server_id);
                crashed.lock().expect("crash state poisoned").insert(server_id.clone(), error.to_string());
                drop(processes);
                let _ = announce(&app, &server_id, "Crash", "Could not check the server process.");
                note_run(&app, &server_id, "crash", &error.to_string());
                let _ = app.emit("server-status", serde_json::json!({ "serverId": server_id, "status": "crashed" }));
                restart_after_crash(&app, &server_id);
                return;
            }
            Ok(None) => {}
        }
    });
}

fn publish_log(app: &AppHandle, logs: &Arc<Mutex<HashMap<String, Vec<ServerLogEvent>>>>, mut event: ServerLogEvent) {
    event.noise = doctor::harmless(&event.line.to_ascii_lowercase());
    if is_ready_line(&event.line) {
        mark_ready(app, &event.server_id);
    }
    if is_stop_line(&event.line) {
        mark_stop_intended(app, &event.server_id);
    }
    if is_crash_line(&event.line) {
        mark_crash_seen(app, &event.server_id);
    }
    // Pumpkin refused a plugin's permissions (nobody can answer its console question when Monolab starts it).
    pumpkin_permission_watch(app, &event);
    // Joins and leaves for the "players join and leave" notification (the screen decides whether to show one).
    if event.stream != "input" {
        if let Some(seen) = logs::classify(&event.line).filter(|one| one.kind == "Join" || one.kind == "Leave") {
            let _ =
                app.emit("player-presence", serde_json::json!({ "serverId": event.server_id, "kind": seen.kind, "player": seen.player }));
            let name = instance_by_id(app, &event.server_id).map(|one| one.name).unwrap_or_default();
            let (what, said) = if seen.kind == "Join" { ("join", "joined the server.") } else { ("leave", "left the server.") };
            webhook::send(app, &event.server_id, &name, what, &format!("{} {said}", seen.player));
        }
    }
    if let Some(progress) = chunky::read_line(&event.line, unix_now()) {
        app.state::<AppState>().chunky.lock().expect("chunky state poisoned").insert(event.server_id.clone(), progress);
    }
    if let Some(url) = spark::report_url(&event.line) {
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_millis() as u64).unwrap_or(0);
        app.state::<AppState>()
            .spark_reports
            .lock()
            .expect("spark state poisoned")
            .insert(event.server_id.clone(), spark::SparkReport { url, at });
    }
    if let Some(snapshot) = parse_tps_line(&event.line) {
        let state = app.state::<AppState>();
        let mut snapshots = state.tps.lock().expect("tps state poisoned");
        let current = snapshots.get(&event.server_id).cloned();
        snapshots.insert(
            event.server_id.clone(),
            TpsSnapshot {
                tps: snapshot.tps.or(current.as_ref().and_then(|value| value.tps)),
                mspt: snapshot.mspt.or(current.as_ref().and_then(|value| value.mspt)),
                updated_at: snapshot.updated_at,
                heap_mib: current.and_then(|value| value.heap_mib),
            },
        );
    }
    {
        let mut stored = logs.lock().expect("log state poisoned");
        let entries = stored.entry(event.server_id.clone()).or_default();
        entries.push(event.clone());
        let limit = read_app_settings(app).console_line_limit as usize;
        if entries.len() > limit {
            entries.drain(..entries.len() - limit);
        }
    }
    app.state::<logbatch::Batcher<ServerLogEvent>>().push(event);
}

fn log_message_body(line: &str) -> &str {
    line.rsplit_once("]: ").map(|(_, rest)| rest).unwrap_or(line).trim()
}

fn strip_color_codes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character == '§' {
            characters.next();
        } else {
            result.push(character);
        }
    }
    result
}

fn first_number(text: &str) -> Option<f32> {
    text.split(|value: char| !value.is_ascii_digit() && value != '.')
        .filter(|part| !part.is_empty())
        .find_map(|part| part.trim_matches('.').parse::<f32>().ok())
}

fn is_ready_line(line: &str) -> bool {
    let low = line.to_ascii_lowercase();
    (low.contains("done (") && low.contains("s)!")) || low.contains("started server;")
}

fn is_stop_command(line: &str) -> bool {
    let word = line.trim().trim_start_matches('/').split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    matches!(word.as_str(), "stop" | "end" | "shutdown")
}

fn is_stop_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("stopping the server") || lower.contains("issued server command: /stop") || lower.contains("shutting down the proxy")
}

fn mark_stop_intended(app: &AppHandle, server_id: &str) {
    let state = app.state::<AppState>();
    let mut processes = state.processes.lock().expect("process state poisoned");
    match processes.get_mut(server_id) {
        Some(process) => {
            if process.stop_requested_at.is_none() {
                process.stop_requested_at = Some(SystemTime::now());
                process.stop_from_log = true;
            }
        }
        None => runlog::heard_stop(server_id),
    }
}

fn is_crash_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "---- minecraft crash report ----",
        "crash report has been saved to",
        "crash report saved to",
        "encountered an unexpected exception",
        "exception in server tick loop",
        "the server has stopped responding",
        "considering it to be crashed",
        "java.lang.outofmemoryerror",
    ]
    .iter()
    .any(|mark| lower.contains(mark))
}

fn mark_crash_seen(app: &AppHandle, server_id: &str) {
    let state = app.state::<AppState>();
    let mut processes = state.processes.lock().expect("process state poisoned");
    match processes.get_mut(server_id) {
        Some(process) => process.crash_seen = true,
        None => runlog::heard_crash(server_id),
    }
}

fn mark_ready(app: &AppHandle, server_id: &str) {
    let state = app.state::<AppState>();
    let mut processes = state.processes.lock().expect("process state poisoned");
    let Some(process) = processes.get_mut(server_id) else {
        runlog::started(server_id);
        return;
    };
    if process.ready {
        return;
    }
    process.ready = true;
    if !process.stop_when_ready {
        return;
    }
    process.stop_when_ready = false;
    let _ = process.send_stop();
}

#[cfg(test)]
mod crash_restart_tests {
    use super::*;

    #[test]
    fn tells_a_stop_from_a_crash() {
        assert!(is_stop_line("[12:00:01] [Server thread/INFO]: Stopping the server"));
        assert!(is_stop_line("[12:00:01 INFO]: [Steve: Stopping the server...]"));
        assert!(is_stop_line("[12:00:01 INFO]: Steve issued server command: /stop"));
        assert!(is_stop_line("[12:00:01 INFO]: Shutting down the proxy..."));
        assert!(!is_stop_line("[12:00:01] [Server thread/INFO]: Stopping server"));
        assert!(!is_stop_line("[12:00:01] [Server Watchdog/ERROR]: The server has stopped responding!"));
        assert!(is_stop_command("stop"));
        assert!(is_stop_command(" /stop "));
        assert!(is_stop_command("end"));
        assert!(!is_stop_command("stopsound @a"));
        assert!(!is_stop_command("say stop it"));
    }

    #[test]
    fn sees_a_crash_even_when_a_stop_follows() {
        assert!(is_crash_line("[12:00:01] [Server thread/ERROR]: Encountered an unexpected exception"));
        assert!(is_crash_line(
            "[12:00:01] [Server thread/ERROR]: This crash report has been saved to: C:\\server\\crash-reports\\crash.txt"
        ));
        assert!(is_crash_line("[12:00:01] [Server Watchdog/FATAL]: Considering it to be crashed, server will forcibly shutdown."));
        assert!(is_crash_line("java.lang.OutOfMemoryError: Java heap space"));
        assert!(!is_crash_line("[12:00:01] [Server thread/INFO]: Stopping the server"));
        assert!(!is_crash_line("[12:00:01] [Server thread/WARN]: Can't keep up! Is the server overloaded?"));
    }
}

fn parse_tps_line(line: &str) -> Option<TpsSnapshot> {
    let body = strip_color_codes(log_message_body(line));
    let body = body.trim();
    let normalized = body.to_ascii_lowercase();
    if normalized.starts_with("tps from last") {
        let values = body.rsplit_once(':')?.1;
        return Some(TpsSnapshot {
            tps: first_number(values).map(|value| value.min(20.0)),
            mspt: None,
            updated_at: unix_now(),
            heap_mib: None,
        });
    }
    if body.contains('/') && body.chars().all(|character| character.is_ascii_digit() || " ./,*-◴".contains(character)) {
        return Some(TpsSnapshot { tps: None, mspt: first_number(body), updated_at: unix_now(), heap_mib: None });
    }
    None
}

fn restart_after_crash(app: &AppHandle, server_id: &str) {
    static RECENT: Mutex<Option<HashMap<String, Vec<SystemTime>>>> = Mutex::new(None);
    const WINDOW: Duration = Duration::from_secs(600);
    const MOST: usize = 3;
    fn wanted(app: &AppHandle, id: &str) -> Option<ServerInstance> {
        load_instances(app)
            .ok()
            .and_then(|all| all.into_iter().find(|one| one.id == id))
            .filter(|one| one.crash_restart && one.remote_host.is_empty() && one.panel_host.is_empty())
    }
    if wanted(app, server_id).is_none() {
        return;
    }
    let tries = {
        let mut guard = RECENT.lock().expect("restart state poisoned");
        let map = guard.get_or_insert_with(HashMap::new);
        let list = map.entry(server_id.to_string()).or_default();
        let now = SystemTime::now();
        list.retain(|at| now.duration_since(*at).map(|gap| gap < WINDOW).unwrap_or(false));
        if list.len() >= MOST {
            None
        } else {
            list.push(now);
            Some(list.len())
        }
    };
    let Some(tries) = tries else {
        let _ = announce(app, server_id, "Crash", "Auto-restart stopped after three crashes within 10 minutes. Please check the logs.");
        record_audit(app, server_id, "Server", "Auto-restart stopped", "Crashed three times within 10 minutes.");
        return;
    };
    let app = app.clone();
    let id = server_id.to_string();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(5));
        let Some(instance) = wanted(&app, &id) else {
            return;
        };
        let state = app.state::<AppState>();
        let approved = if instance.start_script.trim().is_empty() {
            Ok(String::new())
        } else {
            tauri::async_runtime::block_on(approve_script(&app, &instance, false))
        };
        match approved.and_then(|_| start_instance_inner(app.clone(), &state, id.clone())) {
            Ok(()) => {
                let note = format!("Restarted after a crash ({tries}/{MOST}).");
                let _ = announce(&app, &id, "Crash", &note);
                record_audit(&app, &id, "Server", "Auto-restart after crash", &note);
                note_run(&app, &id, "restart", &note);
            }
            Err(reason) if reason.contains("already running") => {}
            Err(reason) => {
                let _ = announce(&app, &id, "Crash", &format!("Auto-restart failed: {reason}"));
                record_audit(&app, &id, "Server", "Auto-restart failed", &reason);
            }
        }
    });
}

#[tauri::command(async)]
#[specta::specta]
fn set_crash_restart(app: AppHandle, id: String, on: bool) -> Result<(), String> {
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    let found = instances.iter_mut().find(|instance| instance.id == id).ok_or("Server not found.")?;
    found.crash_restart = on;
    save_instances(&app, &instances)?;
    record_audit(
        &app,
        &id,
        "Server",
        if on { "Auto-restart on crash on" } else { "Auto-restart on crash off" },
        "Changed whether to restart after a crash.",
    );
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
fn set_start_with_app(app: AppHandle, id: String, on: bool) -> Result<(), String> {
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    let found = instances.iter_mut().find(|instance| instance.id == id).ok_or("Server not found.")?;
    found.start_with_app = on;
    save_instances(&app, &instances)?;
    record_audit(
        &app,
        &id,
        "Server",
        if on { "Start with app: on" } else { "Start with app: off" },
        "Changed whether this server starts with the app.",
    );
    Ok(())
}

fn start_marked_servers(app: &AppHandle) {
    let Ok(instances) = load_instances(app) else {
        return;
    };
    let marked: Vec<ServerInstance> = instances
        .into_iter()
        .filter(|instance| instance.start_with_app && instance.remote_host.is_empty() && instance.panel_host.is_empty())
        .collect();
    if marked.is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    for instance in marked {
        let approved = if instance.start_script.trim().is_empty() {
            Ok(String::new())
        } else {
            tauri::async_runtime::block_on(approve_script(app, &instance, false))
        };
        if let Err(reason) = approved.and_then(|_| start_instance_inner(app.clone(), &state, instance.id.clone())) {
            record_audit(app, &instance.id, "Server", "Start with app failed", &reason);
        }
        thread::sleep(Duration::from_secs(3));
    }
}

/// Says plainly that the folder is gone, rather than the OS error a launch in a missing folder gives ("The directory name is invalid").
fn ensure_folder_present(instance: &ServerInstance) -> Result<(), String> {
    if Path::new(&instance.path).is_dir() {
        return Ok(());
    }
    let path = &instance.path;
    Err(format!(
        "The server folder {path} is missing. It may have been moved, renamed or deleted, or its drive is unplugged. Put it back, or remove the server from the list."
    ))
}

fn start_instance_inner(app: AppHandle, state: &AppState, id: String) -> Result<(), String> {
    if state.processes.lock().expect("process state poisoned").contains_key(&id) {
        return Err("This server is already running.".into());
    }
    state.crashed.lock().expect("crash state poisoned").remove(&id);
    state.logs.lock().expect("log state poisoned").remove(&id);
    let instance = load_instances(&app)?.into_iter().find(|instance| instance.id == id).ok_or("Server not found.")?;
    ensure_folder_present(&instance)?;
    ensure_port_available(port_from_properties(&instance))?;
    apply_pending_addons(&app, &instance);
    if pumpkin::is_pumpkin(&instance.core) && instance.version == forks::PUMPKIN_FORK {
        updates::before_pumpkin_start(&app, Path::new(&instance.path), instance.server_jar.as_deref());
    }
    if pumpkin::is_pumpkin(&instance.core) && events::installed_addon(Path::new(&instance.path), events::AddonKind::Pumpkin).is_some() {
        if let Err(error) = events::write_timezone(Path::new(&instance.path)) {
            eprintln!("[pumpkin] timezone.json: {error}");
        }
    }
    let custom = instance.custom_command.trim();
    let script = instance.start_script.trim();
    let mut command = if !script.is_empty() {
        let name = check_script_name(script, false)?;
        if !Path::new(&instance.path).join(&name).is_file() {
            return Err(format!("Start file {name} was not found in the server folder."));
        }
        let mut command = hidden_command("cmd");
        command.current_dir(&instance.path).arg("/c").arg(format!(".\\{name}"));
        command
    } else if !custom.is_empty() && instance.core != "Pumpkin" {
        let (program, rest) = launch::custom_command(custom)?;
        let java = if program == "java" { PathBuf::from("java") } else { PathBuf::from(&program) };
        if program != "java" {
            launch::verify_java(&java, java::ask_java_major)?;
        }
        let mut command = hidden_command(&java);
        command.current_dir(&instance.path);
        if !rest.iter().any(|token| token.starts_with("-Dstdout.encoding")) {
            command.arg("-Dstdout.encoding=UTF-8").arg("-Dstderr.encoding=UTF-8");
        }
        command.args(CONSOLE_FLAGS);
        command.args(rest);
        command
    } else if instance.core == "Pumpkin" {
        let exe = instance
            .server_jar
            .clone()
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .or_else(|| {
                let guess = Path::new(&instance.path).join(cores::PUMPKIN_EXE);
                guess.is_file().then_some(guess)
            })
            .ok_or("The Pumpkin executable is missing. Recreate the server or put the file in the server folder.")?;
        let mut command = hidden_command(exe);
        command.current_dir(&instance.path);
        command
    } else if instance.launch_mode == "script" || needs_args_file(&instance) {
        update_script_memory(
            &Path::new(&instance.path).join("user_jvm_args.txt"),
            instance.max_memory_mib,
            instance.min_memory_mib,
            &instance.jvm_args,
            "",
        )?;
        let args_file = script_args_file(Path::new(&instance.path))?;
        let java = resolve_java_runtime(&app, &instance.core, &instance.version, &instance.java_path)?;
        let mut command = hidden_command(java);
        command
            .current_dir(&instance.path)
            .arg("-Dstdout.encoding=UTF-8")
            .arg("-Dstderr.encoding=UTF-8")
            .args(CONSOLE_FLAGS)
            .arg("@user_jvm_args.txt")
            .arg(format!("@{args_file}"))
            .arg("nogui");
        command
    } else {
        let jar = instance
            .server_jar
            .clone()
            .map(PathBuf::from)
            .or_else(|| find_jar(Path::new(&instance.path)))
            .ok_or("No .jar to run. Put a jar in the server folder and try again.")?;
        if !jar.is_file() {
            return Err("The registered server jar was not found.".into());
        }
        let java = resolve_java_runtime(&app, &instance.core, &instance.version, &instance.java_path)?;
        let mut command = hidden_command(java);
        command
            .current_dir(&instance.path)
            .arg(format!("-Xms{}M", sane_min_memory(instance.min_memory_mib, instance.max_memory_mib)))
            .arg(format!("-Xmx{}M", instance.max_memory_mib))
            .arg("-Dstdout.encoding=UTF-8")
            .arg("-Dstderr.encoding=UTF-8")
            .args(CONSOLE_FLAGS);
        if instance.jvm_args.trim().is_empty() {
            command.arg("-XX:+UseG1GC");
        }
        for argument in launch::jvm_arguments(&instance.jvm_args)? {
            command.arg(argument);
        }
        command.arg("-jar").arg(&jar);
        if !kinds::is_proxy(&instance.core) {
            command.arg("nogui");
        }
        command
    };
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.stdin(Stdio::piped()).spawn().map_err(|error| format!("Could not start the server: {error}"))?;
    child_job::bind(&child);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let server_id = instance.id.clone();
    if let Some(output) = stdout {
        let handle = app.clone();
        let event_id = server_id.clone();
        let logs = state.logs.clone();
        thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                publish_log(
                    &handle,
                    &logs,
                    ServerLogEvent {
                        server_id: event_id.clone(),
                        line: strip_ansi(&line),
                        stream: "stdout".into(),
                        noise: false,
                        ansi: if line.contains('\u{1b}') { line.clone() } else { String::new() },
                        by: String::new(),
                    },
                );
            }
        });
    }
    if let Some(output) = stderr {
        let handle = app.clone();
        let event_id = server_id.clone();
        let logs = state.logs.clone();
        thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                publish_log(
                    &handle,
                    &logs,
                    ServerLogEvent {
                        server_id: event_id.clone(),
                        line: strip_ansi(&line),
                        stream: "stderr".into(),
                        noise: false,
                        ansi: if line.contains('\u{1b}') { line.clone() } else { String::new() },
                        by: String::new(),
                    },
                );
            }
        });
    }
    let pid = child.id();
    if !instance.start_script.trim().is_empty() {
        watch_script_shell(pid);
    }
    state.processes.lock().expect("process state poisoned").insert(
        instance.id.clone(),
        ManagedProcess {
            child,
            pid,
            started_at: SystemTime::now(),
            stop_requested_at: None,
            ready: false,
            stop_when_ready: false,
            stop_from_log: false,
            crash_seen: false,
        },
    );
    let _ = app.emit("server-status", serde_json::json!({ "serverId": instance.id, "status": "running" }));
    remember_for_web(&app, "server-status", &serde_json::json!({ "serverId": instance.id, "status": "running" }));
    monolink_team::report(&app, &instance.id, "server_started", serde_json::json!({}));
    record_audit(&app, &instance.id, "Server", "Server start", "Server process started.");
    mark_last_run(&app, &instance.id);
    note_run(&app, &server_id, "start", "");
    watch_process(app, state.processes.clone(), state.crashed.clone(), server_id);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
async fn restart_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    restart_instance_as(app, state.inner(), id, true).await
}

async fn restart_instance_as(app: AppHandle, state: &AppState, id: String, trusted: bool) -> Result<(), String> {
    if panel_power(&app, state, &id, "restart").await? {
        return Ok(());
    }
    if let Some(instance) = remote_instance(&app, &id)? {
        return restart_remote(&app, state, &instance, trusted).await;
    }
    let instance = instance_by_id(&app, &id)?;
    if !instance.start_script.trim().is_empty() {
        approve_script(&app, &instance, trusted).await?;
    }
    request_stop(&app, state, &id)?;
    let waited = std::time::Instant::now();
    loop {
        {
            let processes = state.processes.lock().expect("process state poisoned");
            if !processes.contains_key(&id) {
                break;
            }
        }
        if waited.elapsed() > Duration::from_secs(180) {
            return Err("Timed out waiting for the stop. Try End task.".into());
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    record_audit(&app, &id, "Server", "Restart", "Stops, then starts again.");
    start_instance_inner(app, state, id)?;
    Ok(())
}

async fn restart_remote(app: &AppHandle, state: &AppState, instance: &ServerInstance, trusted: bool) -> Result<(), String> {
    if !instance.start_script.trim().is_empty() {
        approve_script(app, instance, trusted).await?;
    }
    let id = instance.id.clone();
    let host = instance.remote_host.clone();
    let live = state.remote_running.lock().expect("remote state poisoned").get(&id).cloned();
    if let Some(live) = live {
        if live.session.is_empty() {
            return Err(format!(
                "This server wasn't started by the app (pid {}), so the console can't attach. Stop it on Ubuntu, then press Start.",
                live.pid
            ));
        }
        remote::send_line(app, &host, &id, "stop", Some(live.session.clone())).await?;
        runlog::asked_to_stop(&id);
        record_audit(app, &id, "Server", "Restart via SSH", "Stops, then starts again.");
        let _ = app.emit("server-status", serde_json::json!({ "serverId": id, "status": "stopping" }));
        let waited = std::time::Instant::now();
        let who = [(id.clone(), instance.path.clone())];
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if let Ok(seen) = remote::live_state_checked(app, &host, &who).await {
                if !seen.contains_key(&id) {
                    break;
                }
            }
            if waited.elapsed() > Duration::from_secs(180) {
                return Err("Timed out waiting for the stop. Try End task.".into());
            }
        }
        // Must be removed from the table too — otherwise below it backs off with "already running".
        state.remote_running.lock().expect("remote state poisoned").remove(&id);
        state.remote_starting.lock().expect("remote state poisoned").remove(&id);
    }
    start_remote(app, state, instance, trusted).await
}

#[tauri::command(async)]
#[specta::specta]
async fn start_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    start_instance_as(app, state.inner(), id, true).await
}

async fn start_instance_as(app: AppHandle, state: &AppState, id: String, trusted: bool) -> Result<(), String> {
    if panel_power(&app, state, &id, "start").await? {
        return Ok(());
    }
    if let Some(instance) = remote_instance(&app, &id)? {
        return start_remote(&app, state, &instance, trusted).await;
    }
    let instance = instance_by_id(&app, &id)?;
    if !instance.start_script.trim().is_empty() {
        approve_script(&app, &instance, trusted).await?;
    }
    start_instance_inner(app, state, id)
}

async fn start_remote(app: &AppHandle, state: &AppState, instance: &ServerInstance, trusted: bool) -> Result<(), String> {
    let id = instance.id.clone();
    let script_line = if instance.start_script.trim().is_empty() {
        String::new()
    } else {
        format!("bash ./{}", approve_script(app, instance, trusted).await?)
    };
    let native = pumpkin::is_pumpkin(&instance.core) && instance.custom_command.trim().is_empty() && script_line.is_empty();
    let launch = if instance.custom_command.trim().is_empty() && script_line.is_empty() && !native {
        remote_jar(app, instance).await?
    } else {
        remote::Launch::default()
    };
    let line = if native {
        "chmod +x ./pumpkin 2>/dev/null; ./pumpkin".to_string()
    } else {
        remote::launch_line(
            if script_line.is_empty() { &instance.custom_command } else { &script_line },
            &instance.java_path,
            &launch,
            instance.min_memory_mib.max(1),
            instance.max_memory_mib.max(1),
            &instance.jvm_args,
        )?
    };
    // Don't start if it's already running outside.
    let already = |live: &remote::Live| {
        if live.session.is_empty() {
            format!(
                "This server is already running (pid {}). It wasn't started by the app, so the console can't attach — stop it on that machine and try again.",
                live.pid
            )
        } else {
            format!("This server is already running (session {}).", live.session)
        }
    };
    let seen = state.remote_running.lock().expect("remote state poisoned").get(&id).cloned();
    if let Some(live) = seen {
        return Err(already(&live));
    }
    if let Ok(found) = remote::live_state_checked(app, &instance.remote_host, &[(id.clone(), instance.path.clone())]).await {
        if let Some(live) = found.get(&id) {
            return Err(already(live));
        }
    }
    apply_pending_remote(app, instance).await;
    remote::start(app, &instance.remote_host, &instance.path, &id, &line, instance.crash_restart).await?;
    state.remote_starting.lock().expect("remote state poisoned").insert(id.clone(), std::time::Instant::now());
    state
        .remote_running
        .lock()
        .expect("remote state poisoned")
        .insert(id.clone(), remote::Live { pid: 0, session: remote::Holder::session(&id), ours: true });
    state.remote_kick.notify_one();
    if let Err(error) = remote::confirm_started(app, &instance.remote_host, &id).await {
        state.remote_running.lock().expect("remote state poisoned").remove(&id);
        state.remote_starting.lock().expect("remote state poisoned").remove(&id);
        return Err(error);
    }
    record_audit(app, &id, "Server", "Start via SSH", &instance.path);
    Ok(())
}

fn session_of(state: &AppState, id: &str) -> Option<String> {
    state.remote_running.lock().expect("remote state poisoned").get(id).map(|live| live.session.clone()).filter(|name| !name.is_empty())
}

async fn remote_jar(app: &AppHandle, instance: &ServerInstance) -> Result<remote::Launch, String> {
    let wanted = instance.server_jar.clone().unwrap_or_default();
    let launch = remote::resolve_launch(app, &instance.remote_host, &instance.path, &wanted, &instance.core).await?;
    let jar = launch.jar.clone();
    if launch.args.is_empty() && jar != wanted {
        let _store = lock_instance_store();
        let mut list = load_instances(app)?;
        if let Some(mine) = list.iter_mut().find(|item| item.id == instance.id) {
            mine.server_jar = (!jar.is_empty()).then(|| jar.clone());
            save_instances(app, &list)?;
        }
    }
    Ok(launch)
}

fn remote_instance(app: &AppHandle, id: &str) -> Result<Option<ServerInstance>, String> {
    Ok(load_instances(app)?.into_iter().find(|one| one.id == id && !one.remote_host.is_empty()))
}

fn panel_instance(app: &AppHandle, id: &str) -> Result<Option<(ServerInstance, panel::Api, panel::Target)>, String> {
    let Some(instance) = load_instances(app)?.into_iter().find(|one| one.id == id && !one.panel_host.is_empty()) else {
        return Ok(None);
    };
    let api = panel::api(app, &instance.panel_host)?;
    let target = panel_live::target_of(&instance);
    Ok(Some((instance, api, target)))
}

fn only_local(app: &AppHandle, id: &str, what: &str) -> Result<(), String> {
    if panel_instance(app, id)?.is_some() {
        return Err(format!("{what} isn't available on panel servers yet."));
    }
    if remote_instance(app, id)?.is_none() {
        return Ok(());
    }
    Err(format!("{what} isn't available on SSH servers yet."))
}

fn panel_of(app: &AppHandle, instance: &ServerInstance) -> Result<(panel::Api, panel::Target), String> {
    Ok((panel::api(app, &instance.panel_host)?, panel_live::target_of(instance)))
}

fn is_far(instance: &ServerInstance) -> bool {
    !instance.remote_host.is_empty() || !instance.panel_host.is_empty()
}

async fn far_read(app: &AppHandle, instance: &ServerInstance, relative: &str) -> Result<String, String> {
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(app, instance)?;
        return api.read_text(&target, relative).await;
    }
    remote::read_text(app, &instance.remote_host, &instance.path, relative).await
}

async fn far_read_bytes(app: &AppHandle, instance: &ServerInstance, relative: &str, limit: u64) -> Result<Vec<u8>, String> {
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(app, instance)?;
        return api.read_bytes(&target, relative, limit).await;
    }
    remote::read_bytes(app, &instance.remote_host, &instance.path, relative, limit).await
}

async fn far_write(app: &AppHandle, instance: &ServerInstance, relative: &str, text: &str) -> Result<(), String> {
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(app, instance)?;
        return api.write_text(&target, relative, text).await;
    }
    remote::write_text(app, &instance.remote_host, &instance.path, relative, text).await
}

async fn far_list(app: &AppHandle, instance: &ServerInstance, relative: &str) -> Result<Vec<remote::FileRow>, String> {
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(app, instance)?;
        return api.list(&target, relative).await;
    }
    remote::list_dir(app, &instance.remote_host, &instance.path, relative).await
}

async fn far_upload(app: &AppHandle, instance: &ServerInstance, relative: &str, bytes: &[u8], shown: &str) -> Result<(), String> {
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(app, instance)?;
        let (dir, name) = panel::split_path(relative)?;
        tell_upload(app, shown, "uploading", 0, bytes.len() as u64);
        let sent = api.upload(&target, &dir, &name, bytes.to_vec()).await;
        tell_upload(app, shown, "done", 0, 0);
        return sent;
    }
    watched_upload(app, &instance.remote_host, &instance.path, relative, bytes, shown).await
}

async fn far_log_files(app: &AppHandle, instance: &ServerInstance) -> Vec<remote::FileRow> {
    if !instance.panel_host.is_empty() {
        return match panel_of(app, instance) {
            Ok((api, target)) => pfs::log_files(&api, &target).await,
            Err(_) => Vec::new(),
        };
    }
    remote::log_files(app, &instance.remote_host, &instance.path).await
}

async fn far_read_log(app: &AppHandle, instance: &ServerInstance, name: &str, lines: u32) -> Result<String, String> {
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(app, instance)?;
        return pfs::read_log(&api, &target, name, lines).await;
    }
    remote::read_log(app, &instance.remote_host, &instance.path, name, lines).await
}

async fn ensure_panel_stopped(app: &AppHandle, instance: &ServerInstance) -> Result<(), String> {
    let now = panel_live::state_now(app, instance).await.map_err(|error| format!("Could not confirm the server is stopped: {error}"))?;
    if now != "offline" {
        return Err("The server is running. Please stop it first.".into());
    }
    Ok(())
}

async fn panel_power(app: &AppHandle, state: &AppState, id: &str, signal: &str) -> Result<bool, String> {
    let Some((instance, api, target)) = panel_instance(app, id)? else { return Ok(false) };
    if signal == "start" {
        if let Ok(now) = panel_live::state_now(app, &instance).await {
            if now != "offline" {
                return Err(format!("This server is already {}.", if now == "stopping" { "Stopping" } else { "Running" }));
            }
        }
    }
    api.power(&target, signal).await?;
    if signal == "stop" || signal == "kill" {
        runlog::asked_to_stop(id);
    }
    let what = match signal {
        "start" => "Start via panel",
        "stop" => "Stop via panel",
        "restart" => "Restart via panel",
        _ => "End task via panel",
    };
    record_audit(app, id, "Server", what, &instance.panel_server);
    panel_live::ensure_console(app, &instance);
    state.panel_kick.notify_one();
    Ok(true)
}

#[tauri::command(async)]
#[specta::specta]
async fn stop_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    stop_instance_as(app, state.inner(), id).await
}

async fn stop_instance_as(app: AppHandle, state: &AppState, id: String) -> Result<(), String> {
    if panel_power(&app, state, &id, "stop").await? {
        return Ok(());
    }
    if let Some(instance) = remote_instance(&app, &id)? {
        remote::send_line(&app, &instance.remote_host, &id, "stop", session_of(state, &id)).await?;
        runlog::asked_to_stop(&id);
        record_audit(&app, &id, "Server", "Stop via SSH", "Sent stop to the console.");
        return Ok(());
    }
    request_stop(&app, state, &id)?;
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
async fn force_stop_instance(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    if panel_power(&app, &state, &id, "kill").await? {
        return Ok(());
    }
    if let Some(instance) = remote_instance(&app, &id)? {
        let live = state.remote_running.lock().expect("remote state poisoned").get(&id).cloned();
        let session = live.as_ref().map(|one| one.session.clone()).filter(|name| !name.is_empty());
        let pid = live.as_ref().map(|one| one.pid).unwrap_or(0);
        remote::force_stop(&app, &instance.remote_host, &id, session, pid).await?;
        runlog::asked_to_stop(&id);
        state.remote_running.lock().expect("remote state poisoned").remove(&id);
        state.remote_starting.lock().expect("remote state poisoned").remove(&id);
        record_audit(&app, &id, "Server", "End task (SSH)", "Closed the session and killed the process without a stop command.");
        return Ok(());
    }
    {
        let mut processes = state.processes.lock().expect("process state poisoned");
        let process = processes.get_mut(&id).ok_or("The server is not running.")?;
        kill_process_tree(process.pid);
        let _ = process.child.kill();
        process.stop_requested_at = Some(SystemTime::now());
    }
    record_audit(&app, &id, "Server", "End task", "Killed the process without a stop command.");
    Ok(())
}

fn request_stop(app: &AppHandle, state: &AppState, id: &str) -> Result<bool, String> {
    let mut processes = state.processes.lock().expect("process state poisoned");
    let process = processes.get_mut(id).ok_or("The server is not running.")?;
    let deferred = !process.ready;
    if deferred {
        process.stop_when_ready = true;
    } else {
        process.send_stop()?;
    }
    process.stop_requested_at = Some(SystemTime::now());
    let _ = app.emit("server-status", serde_json::json!({ "serverId": id, "status": "stopping" }));
    record_audit(
        app,
        id,
        "Server",
        "Stop requested",
        if deferred { "Stop is queued until startup finishes." } else { "Sent stop to the console." },
    );
    Ok(deferred)
}

fn write_console(state: &AppState, id: &str, line: &str) -> Result<(), String> {
    let mut processes = state.processes.lock().expect("process state poisoned");
    let process = processes.get_mut(id).ok_or("The server is not running.")?;
    if is_stop_command(line) && process.stop_requested_at.is_none() {
        process.stop_requested_at = Some(SystemTime::now());
    }
    let line = launch::console_line(line)?;
    let stdin = process.child.stdin.as_mut().ok_or("Could not connect to the server console.")?;
    stdin.write_all(format!("{line}\n").as_bytes()).map_err(|error| format!("Could not send the command: {error}"))?;
    stdin.flush().map_err(|error| error.to_string())
}

#[tauri::command(async)]
#[specta::specta]
async fn send_console_command(app: AppHandle, state: State<'_, AppState>, id: String, command: String) -> Result<(), String> {
    send_console_command_inner(&app, &state, &id, &command).await?;
    remember_sent_command(&app, &state, &id, &command);
    Ok(())
}

fn remember_sent_command(app: &AppHandle, state: &AppState, id: &str, command: &str) {
    let event = ServerLogEvent {
        server_id: id.to_string(),
        line: command.trim().to_string(),
        stream: "input".to_string(),
        noise: false,
        ansi: String::new(),
        by: monolink_team::console_by(),
    };
    {
        let mut stored = state.logs.lock().expect("log state poisoned");
        let entries = stored.entry(id.to_string()).or_default();
        entries.push(event.clone());
        let limit = read_app_settings(app).console_line_limit as usize;
        if entries.len() > limit {
            entries.drain(..entries.len() - limit);
        }
    }
    app.state::<logbatch::Batcher<ServerLogEvent>>().push(event);
}

async fn send_console_command_inner(app: &AppHandle, state: &State<'_, AppState>, id: &str, command: &str) -> Result<(), String> {
    let (app, state, id, command) = (app.clone(), state, id.to_string(), command.to_string());
    if let Some((_, api, target)) = panel_instance(&app, &id)? {
        api.command(&target, &launch::console_line(&command)?).await?;
        record_audit(&app, &id, "Console", "Command via panel", &command);
        return Ok(());
    }
    if let Some(instance) = remote_instance(&app, &id)? {
        remote::send_line(&app, &instance.remote_host, &id, &launch::console_line(&command)?, session_of(state, &id)).await?;
        record_audit(&app, &id, "Console", "Send console command (SSH)", "Command contents are not stored in the audit log for security.");
        return Ok(());
    }
    write_console(state, &id, &command)?;
    record_audit(&app, &id, "Console", "Console command", "Command contents are not stored in the audit log for security.");
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
fn get_console_logs(state: State<'_, AppState>, id: String) -> Vec<ServerLogEvent> {
    state.logs.lock().expect("log state poisoned").get(&id).cloned().unwrap_or_default()
}

fn parse_log_entry(line: String, source: &str) -> ParsedLogEntry {
    let upper = line.to_ascii_uppercase();
    let level = if upper.contains("ERROR") || upper.contains("EXCEPTION") || upper.contains("FATAL") {
        "Error"
    } else if upper.contains("WARN") {
        "Warning"
    } else if upper.contains("DEBUG") {
        "Debug"
    } else {
        "Info"
    };
    let lower = line.to_ascii_lowercase();
    let event = if lower.contains(" joined the game") || lower.contains(" joined") {
        "Join"
    } else if lower.contains(" left the game") || lower.contains(" lost connection") {
        "Leave"
    } else if lower.contains(" was slain") || lower.contains(" fell from") || lower.contains(" died") {
        "Death"
    } else if lower.contains(" issued server command") || lower.contains(" ran command") {
        "Command"
    } else if level == "Error" {
        "Error"
    } else {
        "Log"
    };
    ParsedLogEntry { line, level: level.into(), event: event.into(), source: source.into() }
}

#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
async fn list_log_entries(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    query: String,
    level: String,
    file_name: String,
) -> Result<Vec<ParsedLogEntry>, String> {
    let instance = instance_by_id(&app, &id)?;
    let wanted = if file_name.trim().is_empty() { "latest.log".to_string() } else { file_name.trim().to_string() };
    let mut lines = VecDeque::new();
    if !is_far(&instance) {
        if let Ok(path) = logs::safe_log_path(Path::new(&instance.path), &wanted) {
            for line in logs::read_lines(&path, 1_500).unwrap_or_default() {
                lines.push_back((line, "File".to_string()));
            }
        }
    } else {
        for line in far_read_log(&app, &instance, &wanted, 1_500).await.unwrap_or_default().lines() {
            lines.push_back((line.to_string(), "File".to_string()));
        }
    }
    if wanted == "latest.log" {
        for event in state.logs.lock().expect("log state poisoned").get(&id).cloned().unwrap_or_default() {
            lines.push_back((event.line, "Live".to_string()));
            if lines.len() > 2_000 {
                lines.pop_front();
            }
        }
    }
    let needle = query.trim().to_lowercase();
    let selected_level = level.trim();
    Ok(lines
        .into_iter()
        .map(|(line, source)| parse_log_entry(line, &source))
        .filter(|entry| {
            (selected_level.is_empty() || selected_level == "All" || entry.level == selected_level)
                && (needle.is_empty() || entry.line.to_lowercase().contains(&needle) || entry.event.contains(&needle))
        })
        .collect())
}

#[tauri::command(async)]
#[specta::specta]
fn list_audit_entries(app: AppHandle, id: String) -> Result<Vec<AuditEntry>, String> {
    instance_by_id(&app, &id)?;
    let path = audit_log_path(&app)?;
    let mut entries = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<AuditEntry>>(&text).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.server_id == id)
        .collect::<Vec<_>>();
    entries.sort_by_key(|left| std::cmp::Reverse(left.created_at));
    entries.truncate(100);
    Ok(entries)
}

fn instance_by_id(app: &AppHandle, id: &str) -> Result<ServerInstance, String> {
    load_instances(app)?.into_iter().find(|instance| instance.id == id).ok_or("Server not found.".into())
}

fn ensure_stopped(state: &AppState, id: &str) -> Result<(), String> {
    if state.processes.lock().expect("process state poisoned").contains_key(id) {
        Err("Stop the server completely before changing data.".into())
    } else {
        Ok(())
    }
}

fn restore_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    Ok(instance_data_dir(app, id)?.join("restore"))
}

fn directory_size(path: &Path) -> u64 {
    fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                directory_size(&path)
            } else {
                entry.metadata().map(|metadata| metadata.len()).unwrap_or(0)
            }
        })
        .sum()
}

fn world_entry(path: &Path, name: String, world: String, realm: &str, has_level_data: bool, inside_parent: bool) -> Option<WorldEntry> {
    let modified_at = fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(WorldEntry {
        name,
        size_mib: directory_size(path) as f64 / 1024.0 / 1024.0,
        modified_at,
        has_level_data,
        realm: realm.to_string(),
        world,
        inside_parent,
    })
}

#[tauri::command(async)]
#[specta::specta]
async fn list_worlds(app: AppHandle, id: String) -> Result<Vec<WorldEntry>, String> {
    {
        let instance = instance_by_id(&app, &id)?;
        if !instance.panel_host.is_empty() {
            let (api, target) = panel_of(&app, &instance)?;
            return Ok(pfs::worlds(&api, &target)
                .await
                .into_iter()
                .map(|(name, at)| {
                    let realm = if name.ends_with("_nether") {
                        "nether"
                    } else if name.ends_with("_the_end") {
                        "end"
                    } else {
                        "overworld"
                    };
                    WorldEntry {
                        size_mib: f64::NAN,
                        modified_at: at,
                        has_level_data: realm == "overworld",
                        realm: realm.into(),
                        world: name.clone(),
                        inside_parent: false,
                        name,
                    }
                })
                .collect());
        }
        if !instance.remote_host.is_empty() {
            return Ok(remote::worlds(&app, &instance.remote_host, &instance.path)
                .await
                .into_iter()
                .map(|one| WorldEntry {
                    size_mib: one.kib as f64 / 1024.0,
                    modified_at: one.at,
                    has_level_data: one.level,
                    realm: one.realm,
                    world: one.world,
                    inside_parent: one.inside,
                    name: one.name,
                })
                .collect());
        }
    }
    let instance = instance_by_id(&app, &id)?;
    let root = Path::new(&instance.path);
    let mut worlds = Vec::new();
    for entry in fs::read_dir(root).map_err(|error| error.to_string())?.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !path.is_dir() {
            continue;
        }
        let has_level_data = path.join("level.dat").is_file();
        let sibling = matches!(name.as_str(), "world_nether" | "world_the_end");
        if !has_level_data && !sibling {
            continue;
        }
        let realm = if name.ends_with("_nether") {
            "nether"
        } else if name.ends_with("_the_end") {
            "end"
        } else {
            "overworld"
        };
        let owner = if sibling { name.trim_end_matches("_nether").trim_end_matches("_the_end").to_string() } else { name.clone() };
        if let Some(one) = world_entry(&path, name.clone(), owner, realm, has_level_data, false) {
            worlds.push(one);
        }
        if !has_level_data {
            continue;
        }
        for (realm, kind, label) in [(worldmap::Realm::Nether, "nether", "Nether"), (worldmap::Realm::End, "end", "End")] {
            let Some(region) = worldmap::region_dir(root, &name, realm) else {
                continue;
            };
            // Don't list twice what was already listed as a sibling folder.
            if !region.starts_with(&path) {
                continue;
            }
            let Some(dim) = region.parent() else { continue };
            if let Some(one) = world_entry(dim, format!("{name} ({label})"), name.clone(), kind, false, true) {
                worlds.push(one);
            }
        }
        for (key, dim) in worldmap::modded_dims(&path) {
            if let Some(one) = world_entry(&dim, format!("{name} ({key})"), name.clone(), &key, false, true) {
                worlds.push(one);
            }
        }
    }
    worlds.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(worlds)
}

fn editable_file(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|one| one.to_str()) else {
        return true;
    };
    !matches!(
        extension.to_ascii_lowercase().as_str(),
        "jar"
            | "zip"
            | "gz"
            | "7z"
            | "rar"
            | "exe"
            | "dll"
            | "mca"
            | "dat"
            | "dat_old"
            | "nbt"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "ico"
            | "mp3"
            | "ogg"
            | "wav"
            | "ttf"
            | "otf"
            | "woff"
            | "woff2"
            | "pdf"
            | "db"
            | "mv"
            | "lock"
            | "class"
            | "so"
            | "dylib"
            | "bin"
            | "hprof"
            | "jfr"
            | "pyc"
            | "tar"
            | "xz"
            | "bz2"
            | "zst"
            | "mp4"
            | "webm"
            | "pump"
            | "mcr"
    )
}

/// Bytes that aren't text (NUL early on, or mostly undecodable) — opening them in the editor and saving would wreck them.
pub(crate) fn looks_binary(text: &str) -> bool {
    let head: Vec<char> = text.chars().take(8192).collect();
    let broken = head.iter().filter(|one| **one == '\u{FFFD}').count();
    head.contains(&'\0') || broken * 20 > head.len().max(1)
}

fn safe_editable_path(instance: &ServerInstance, relative: &str) -> Result<PathBuf, String> {
    let path = paths::resolve_in_server(Path::new(&instance.path), relative).map_err(|error| match error {
        paths::PathError::Missing => "File not found.".to_string(),
        other => other.to_string(),
    })?;
    if !path.is_file() || !editable_file(&path) {
        return Err("Only text files can be opened.".into());
    }
    if fs::metadata(&path).map_err(|error| error.to_string())?.len() > 8 * 1024 * 1024 {
        return Err("Files larger than 8 MB cannot be opened in the editor.".into());
    }
    Ok(path)
}

fn send_process_command(state: &AppState, id: &str, command: &str) -> Result<bool, String> {
    let mut processes = state.processes.lock().expect("process state poisoned");
    let Some(process) = processes.get_mut(id) else {
        return Ok(false);
    };
    let stdin = process.child.stdin.as_mut().ok_or("Could not connect to the running server's console.")?;
    let command = launch::console_line(command)?;
    stdin.write_all(format!("{command}\n").as_bytes()).map_err(|error| format!("Could not send the server command: {error}"))?;
    stdin.flush().map_err(|error| error.to_string())?;
    Ok(true)
}

fn start_backup_scheduler(app: AppHandle) {
    {
        let handle = app.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(30));
            run_due_tasks(&handle);
        });
    }
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AddonStatus {
    supported: bool,
    reason: String,
    kind: String,
    installed: bool,
    installed_version: Option<u32>,
    latest_version: u32,
    needs_update: bool,
    running: bool,
    folder: String,
}

async fn install_addon_panel(app: &AppHandle, instance: &ServerInstance) -> Result<AddonStatus, String> {
    let Some(kind) = events::addon_kind(&instance.core) else {
        return Err(format!("Cannot install on {}. Only Paper, Purpur, Folia, NeoForge, Fabric and Pumpkin are supported.", instance.core));
    };
    if kind == events::AddonKind::Fabric {
        return Err("Fabric servers also need fabric-api, so this can't be installed through the panel yet. Install fabric-api from the panel's marketplace, then upload the file in Files.".into());
    }
    let addons = updates::current_addon(app);
    let Some((version, jar)) = addons.pick(kind, &instance.version) else {
        return Err(format!(
            "Not available yet for Minecraft {}. If the version says Needs check, let the list detect it first and try again.",
            instance.version
        ));
    };
    ensure_panel_stopped(app, instance).await?;
    let source = addons.resolve(&jar).ok_or("Add‑on file not found. Try reinstalling the app.")?;
    let bytes = fs::read(&source).map_err(|error| format!("Could not read the add-on file bundled with the app: {error}"))?;
    let folder = match kind {
        events::AddonKind::Paper | events::AddonKind::Pumpkin => "plugins",
        events::AddonKind::NeoForge | events::AddonKind::Fabric => "mods",
    };
    let (api, target) = panel_of(app, instance)?;
    let old: Vec<String> = api
        .list(&target, folder)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|one| events::is_our_addon(&one.name))
        .map(|one| format!("{folder}/{}", one.name))
        .collect();
    if !old.is_empty() {
        api.delete(&target, &old).await?;
    }
    api.upload(&target, folder, &jar, bytes).await?;
    record_audit(app, &instance.id, "Add-on", "Install add-on (panel)", &jar);
    Ok(remote_addon_status(instance, Some((jar, version)), false))
}

async fn install_addon_remote(app: &AppHandle, state: &AppState, instance: &ServerInstance) -> Result<AddonStatus, String> {
    let Some(kind) = events::addon_kind(&instance.core) else {
        return Err(format!("Cannot install on {}. Only Paper, Purpur, Folia, NeoForge, Fabric and Pumpkin are supported.", instance.core));
    };
    if kind == events::AddonKind::Fabric {
        return Err(
            "Fabric servers also need fabric-api, so this can't be installed over SSH yet. Install it on that machine directly.".into()
        );
    }
    let addons = updates::current_addon(app);
    let Some((version, jar)) = addons.pick(kind, &instance.version) else {
        return Err(format!("Not available yet for Minecraft {}.", instance.version));
    };
    if state.remote_running.lock().expect("remote state poisoned").contains_key(&instance.id) {
        return Err("Stop the server first. Nothing can be installed while it is running.".into());
    }
    let source = addons.resolve(&jar).ok_or("Add‑on file not found. Try reinstalling the app.")?;
    let bytes = fs::read(&source).map_err(|error| format!("Could not read the add-on file bundled with the app: {error}"))?;
    let folder = match kind {
        events::AddonKind::Paper | events::AddonKind::Pumpkin => "plugins",
        events::AddonKind::NeoForge | events::AddonKind::Fabric => "mods",
    };
    let conn = remote::connect(app, &instance.remote_host).await?;
    let at = format!("{}/{folder}", instance.path.trim_end_matches('/'));
    conn.run(&format!("mkdir -p {0} && rm -f {0}/Monolab-*.jar {0}/Monolab-*.wasm", remote::quote(&at))).await?.ok()?;
    remote::upload(app, &instance.remote_host, &instance.path, &format!("{folder}/{jar}"), &bytes).await?;
    record_audit(app, &instance.id, "Add-on", "Install add-on (SSH)", &jar);
    Ok(remote_addon_status(instance, Some((jar, version)), false))
}

fn remote_addon_status(instance: &ServerInstance, installed: Option<(String, u32)>, running: bool) -> AddonStatus {
    let kind = events::addon_kind(&instance.core);
    let picked = kind.and_then(|kind| updates::current().pick(kind, &instance.version));
    let latest = picked.as_ref().map(|(version, _)| *version).unwrap_or(events::ADDON_VERSION);
    let wanted = picked.map(|(_, name)| name);
    AddonStatus {
        supported: wanted.is_some(),
        reason: if wanted.is_some() { String::new() } else { format!("Not available yet for {} {}.", instance.core, instance.version) },
        kind: match kind {
            Some(events::AddonKind::NeoForge) | Some(events::AddonKind::Fabric) => "Mod".into(),
            _ => "Plugin".into(),
        },
        installed: installed.is_some(),
        installed_version: installed.as_ref().map(|(_, version)| *version),
        latest_version: latest,
        needs_update: match (&installed, &wanted) {
            (Some((name, version)), Some(jar)) => name != jar || *version < latest,
            _ => false,
        },
        running,
        folder: match kind {
            Some(events::AddonKind::NeoForge) | Some(events::AddonKind::Fabric) => "mods".into(),
            _ => "plugins".into(),
        },
    }
}

fn addon_status_of(state: &AppState, instance: &ServerInstance) -> AddonStatus {
    let server = Path::new(&instance.path);
    let kind = events::addon_kind(&instance.core);
    let picked = kind.and_then(|kind| updates::current().pick(kind, &instance.version));
    let latest = picked.as_ref().map(|(version, _)| *version).unwrap_or(events::ADDON_VERSION);
    let wanted = picked.map(|(_, name)| name);
    let installed = kind.and_then(|kind| events::installed_addon(server, kind));
    AddonStatus {
        supported: wanted.is_some(),
        reason: match (kind, &wanted) {
            (_, Some(_)) => String::new(),
            (None, _) => format!(
                "Nothing is available for {} yet. Only Paper, Purpur, Folia, NeoForge, Fabric and Pumpkin are supported.",
                instance.core
            ),
            (Some(events::AddonKind::Pumpkin), None) => format!(
                "No plugin is available for Pumpkin {} yet. Right now it works on {}.",
                instance.version,
                events::supported_games(events::AddonKind::Pumpkin).join(", ")
            ),
            (Some(kind), None) => format!(
                "No mod is available for Minecraft {} yet. Currently available for {}.",
                instance.version,
                events::supported_games(kind).join(", ")
            ),
        },
        kind: match kind {
            Some(events::AddonKind::NeoForge) | Some(events::AddonKind::Fabric) => "Mod".into(),
            _ => "Plugin".into(),
        },
        installed: installed.is_some(),
        installed_version: installed.as_ref().map(|(_, version)| *version),
        latest_version: latest,
        needs_update: match (&installed, &wanted) {
            (Some((name, version)), Some(jar)) => name != jar || *version < latest,
            _ => false,
        },
        running: state.processes.lock().expect("process state poisoned").contains_key(&instance.id),
        folder: events::event_folder(server).to_string_lossy().to_string(),
    }
}

#[tauri::command(async)]
#[specta::specta]
async fn addon_status(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<AddonStatus, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        let folder = match events::addon_kind(&instance.core) {
            Some(events::AddonKind::NeoForge) | Some(events::AddonKind::Fabric) => "mods",
            _ => "plugins",
        };
        let (api, target) = panel_of(&app, &instance)?;
        let found =
            api.list(&target, folder).await.unwrap_or_default().into_iter().find(|one| events::is_our_addon(&one.name)).and_then(|one| {
                events::addon_kind(&instance.core)
                    .and_then(|kind| events::addon_version_in_name(kind, &one.name))
                    .map(|version| (one.name, version))
            });
        let running = state.panel_live.lock().expect("panel state poisoned").get(&id).is_some_and(|one| one.stats.state != "offline");
        return Ok(remote_addon_status(&instance, found, running));
    }
    if !instance.remote_host.is_empty() {
        let folder = match events::addon_kind(&instance.core) {
            Some(events::AddonKind::NeoForge) | Some(events::AddonKind::Fabric) => "mods",
            _ => "plugins",
        };
        let found = far_list(&app, &instance, folder)
            .await
            .unwrap_or_default()
            .into_iter()
            .find(|one| !one.directory && events::is_our_addon(&one.name))
            .and_then(|one| {
                events::addon_kind(&instance.core)
                    .and_then(|kind| events::addon_version_in_name(kind, &one.name))
                    .map(|version| (one.name, version))
            });
        let running = state.remote_running.lock().expect("remote state poisoned").contains_key(&id);
        return Ok(remote_addon_status(&instance, found, running));
    }
    Ok(addon_status_of(&state, &instance))
}

#[tauri::command(async)]
#[specta::specta]
async fn add_addon_files(app: AppHandle, state: State<'_, AppState>, id: String, paths: Vec<String>) -> Result<AddonAddResult, String> {
    let instance = instance_by_id(&app, &id)?;
    let running = is_running(&state, &id);
    let native = pumpkin::is_pumpkin(&instance.core);
    if is_far(&instance) {
        if running {
            return Err("Can't add files over SSH while the server is running. Please stop it first.".into());
        }
        let sub = match events::addon_kind(&instance.core) {
            Some(events::AddonKind::NeoForge) | Some(events::AddonKind::Fabric) => "mods",
            Some(_) => "plugins",
            None => {
                if matches!(instance.core.as_str(), "Fabric" | "Forge") {
                    "mods"
                } else {
                    "plugins"
                }
            }
        };
        let mut result = AddonAddResult::default();
        for one in &paths {
            let source = Path::new(one);
            let name = source.file_name().and_then(|name| name.to_str()).ok_or("Could not read the file name.")?.to_string();
            if native {
                if pumpkin::plugin_state(&name) != Some(true) {
                    return Err(format!("{name} is not a Pumpkin plugin (.wasm) file."));
                }
            } else if !name.to_ascii_lowercase().ends_with(".jar") {
                return Err(format!("{name} is not a .jar file."));
            }
            let bytes = fs::read(source).map_err(|error| format!("Could not read {name}: {error}"))?;
            far_upload(&app, &instance, &format!("{sub}/{name}"), &bytes, &name).await?;
            result.added.push(name);
        }
        if result.added.is_empty() {
            return Err("No .jar files to add.".into());
        }
        record_audit(&app, &id, "Mods & plugins", "Add manually (SSH)", &format!("Added {}.", result.added.join(", ")));
        return Ok(result);
    }
    let folder = match events::addon_kind(&instance.core) {
        Some(kind) => events::addon_folder(Path::new(&instance.path), kind),
        None => Path::new(&instance.path).join(if matches!(instance.core.as_str(), "Fabric" | "Forge") { "mods" } else { "plugins" }),
    };
    fs::create_dir_all(&folder).map_err(|error| format!("Could not create the target folder: {error}"))?;

    let installed = addons::scan(Path::new(&instance.path));
    let keep = addon_restore_dir(&app, &id)?;
    let resting = pending_dir(&app, &id)?;

    let mut result = AddonAddResult::default();
    for one in paths {
        let source = Path::new(&one);
        let name = source.file_name().and_then(|name| name.to_str()).ok_or("Could not read the file name.")?.to_string();
        if native {
            if pumpkin::plugin_state(&name) != Some(true) {
                return Err(format!("{name} is not a Pumpkin plugin (.wasm) file."));
            }
        } else if !name.to_ascii_lowercase().ends_with(".jar") {
            return Err(format!("{name} is not a .jar file."));
        }
        if !source.is_file() {
            return Err(format!("{name} was not found."));
        }

        let (mod_id, display_name, version) = addons::jar_identity(source);
        let older = installed.iter().find(|one| !mod_id.is_empty() && one.mod_id == mod_id && one.file_name != name);

        if running && (older.is_some() || folder.join(&name).exists()) {
            pending::queue(
                &resting,
                pending::Waiting {
                    id: format!("{}-{}", unix_now(), result.queued.len()),
                    replaces: older.map(|one| one.file_name.clone()).unwrap_or_default(),
                    file_name: name.clone(),
                    display_name: if display_name.is_empty() { name.clone() } else { display_name },
                    from_version: older.map(|one| one.version.clone()).unwrap_or_default(),
                    to_version: version,
                    staged: String::new(),
                    queued_at: unix_now(),
                    source: "Added manually".into(),
                },
                // A copy: queueing moves the file it is given, and this one is the user's own (Downloads, a build).
                &{
                    let copy = stash_slot(&app, &name)?;
                    fs::copy(source, &copy).map_err(|error| format!("Could not queue {name}: {error}"))?;
                    copy
                },
            )?;
            result.queued.push(name);
            continue;
        }

        if let Some(older) = older {
            let old = folder.join(&older.file_name);
            if old.is_file() {
                move_file(&old, &keep.join(format!("{}-{}", unix_now(), older.file_name)))
                    .map_err(|error| format!("Could not keep the previous file: {error}"))?;
            }
            result.replaced.push(older.file_name.clone());
        }
        let folder_name = folder.file_name().and_then(|one| one.to_str()).unwrap_or("plugins");
        let destination = paths::resolve_new_in_server(Path::new(&instance.path), &format!("{folder_name}/{name}"))?;
        fs::copy(source, destination).map_err(|error| format!("Could not install {name}: {error}"))?;
        result.added.push(name);
    }
    if result.added.is_empty() && result.queued.is_empty() {
        return Err("No .jar files to add.".into());
    }
    record_audit(
        &app,
        &id,
        "Mods & plugins",
        if result.queued.is_empty() { "Added manually" } else { "Added manually (queued for restart)" },
        &format!("Added {}.", result.added.iter().chain(result.queued.iter()).cloned().collect::<Vec<_>>().join(", ")),
    );
    Ok(result)
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AddonAddResult {
    added: Vec<String>,
    replaced: Vec<String>,
    queued: Vec<String>,
}

/// Whether a running Pumpkin server was started from `exe` itself (not its own copy).
pub(crate) fn pumpkin_exe_in_use(app: &AppHandle, exe: &Path) -> bool {
    let state = app.state::<AppState>();
    let running: HashSet<String> = state.processes.lock().expect("process state poisoned").keys().cloned().collect();
    if running.is_empty() {
        return false;
    }
    let wanted = fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    load_instances(app).unwrap_or_default().into_iter().filter(|one| running.contains(&one.id) && pumpkin::is_pumpkin(&one.core)).any(
        |one| {
            let root = Path::new(&one.path);
            let path = one.server_jar.as_ref().map(|jar| root.join(jar)).unwrap_or_else(|| root.join(cores::PUMPKIN_EXE));
            fs::canonicalize(&path).unwrap_or(path) == wanted
        },
    )
}

fn is_running(state: &AppState, id: &str) -> bool {
    state.processes.lock().expect("process state poisoned").contains_key(id)
}

#[tauri::command(async)]
#[specta::specta]
async fn install_addon(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<AddonStatus, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        return install_addon_panel(&app, &instance).await;
    }
    if !instance.remote_host.is_empty() {
        return install_addon_remote(&app, &state, &instance).await;
    }
    let Some(kind) = events::addon_kind(&instance.core) else {
        return Err(format!("Cannot install on {}. Only Paper, Purpur, Folia, NeoForge, Fabric and Pumpkin are supported.", instance.core));
    };
    let addons = updates::current_addon(&app);
    let Some((version, jar)) = addons.pick(kind, &instance.version) else {
        return Err(if kind == events::AddonKind::Pumpkin {
            format!("No plugin is available for Pumpkin {} yet.", instance.version)
        } else {
            format!("No mod is available for Minecraft {} yet.", instance.version)
        });
    };
    if state.processes.lock().expect("process state poisoned").contains_key(&id) {
        return Err("Stop the server first. Nothing can be installed while it is running.".into());
    }
    let Some(source) = addons.resolve(&jar) else {
        return Err("The bundled add‑on file is missing. Try reinstalling the app.".into());
    };
    let server = Path::new(&instance.path);
    let folder = events::addon_folder(server, kind);
    let folder_name = folder.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
    fs::create_dir_all(&folder).map_err(|error| format!("Could not create the folder {folder_name}: {error}"))?;
    let brought = if kind == events::AddonKind::Fabric { ensure_fabric_api(&instance, &folder)? } else { None };
    remove_old_addon_jars(&folder);
    fs::copy(&source, folder.join(&jar)).map_err(|error| format!("Could not install the add‑on: {error}"))?;
    let spark_note = match kind {
        events::AddonKind::NeoForge | events::AddonKind::Fabric => match ensure_spark(&instance, &folder) {
            Ok(Some(name)) => format!(" The spark profiler ({name}) was added too."),
            Ok(None) => String::new(),
            Err(error) => format!(" spark could not be added: {error}"),
        },
        _ => String::new(),
    };
    // Whether Pumpkin may let it use its data folder is asked separately (`set_pumpkin_addon_allowed`).
    if kind == events::AddonKind::Pumpkin {
        events::write_timezone(server)?;
    }
    record_audit(
        &app,
        &id,
        "Events",
        "Add‑on install",
        &match &brought {
            Some(name) => {
                format!("Installed version {} into the {folder_name} folder. The required {name} was downloaded too.{spark_note}", version)
            }
            None => format!("Installed version {} into the {folder_name} folder.{spark_note}", version),
        },
    );
    Ok(addon_status_of(&state, &instance))
}

fn ensure_spark(instance: &ServerInstance, folder: &Path) -> Result<Option<String>, String> {
    if spark::info(&instance.core, &instance.version, Path::new(&instance.path)).available {
        return Ok(None);
    }
    let (_, loader) = instance_loader(instance)?;
    let client = addons::client()?;
    let plan = addons::plan_install(&client, spark::MODRINTH_ID, loader, &instance.version, &Default::default())
        .map_err(|error| format!("No spark build for Minecraft {} was found ({error})", instance.version))?;
    let staging = folder.join(format!(".{}.download", plan.file_name));
    if let Err(error) = download_file(&client, &plan.download_url, &staging).and_then(|()| verify_sha512(&staging, &plan.sha512)) {
        let _ = fs::remove_file(&staging);
        return Err(format!("Download failed: {error}"));
    }
    fs::rename(&staging, folder.join(&plan.file_name)).map_err(|error| error.to_string())?;
    Ok(Some(plan.file_name))
}

#[tauri::command(async)]
#[specta::specta]
async fn remove_addon(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<AddonStatus, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        ensure_panel_stopped(&app, &instance).await?;
        let folder = match events::addon_kind(&instance.core) {
            Some(events::AddonKind::NeoForge) | Some(events::AddonKind::Fabric) => "mods",
            _ => "plugins",
        };
        let (api, target) = panel_of(&app, &instance)?;
        let old: Vec<String> = api
            .list(&target, folder)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|one| events::is_our_addon(&one.name))
            .map(|one| format!("{folder}/{}", one.name))
            .collect();
        if !old.is_empty() {
            api.delete(&target, &old).await?;
        }
        record_audit(&app, &id, "Add-on", "Remove add-on (panel)", folder);
        return Ok(remote_addon_status(&instance, None, false));
    }
    if !instance.remote_host.is_empty() {
        if state.remote_running.lock().expect("remote state poisoned").contains_key(&id) {
            return Err("Stop the server first.".into());
        }
        let folder = match events::addon_kind(&instance.core) {
            Some(events::AddonKind::NeoForge) | Some(events::AddonKind::Fabric) => "mods",
            _ => "plugins",
        };
        let conn = remote::connect(&app, &instance.remote_host).await?;
        let at = format!("{}/{folder}", instance.path.trim_end_matches('/'));
        conn.run(&format!("rm -f {0}/Monolab-*.jar {0}/Monolab-*.wasm", remote::quote(&at))).await?.ok()?;
        record_audit(&app, &id, "Add-on", "Remove add-on (SSH)", folder);
        return Ok(remote_addon_status(&instance, None, false));
    }
    if state.processes.lock().expect("process state poisoned").contains_key(&id) {
        return Err("Stop the server first.".into());
    }
    let server = Path::new(&instance.path);
    remove_old_addon_jars(&events::addon_folder(server, events::AddonKind::Paper));
    remove_old_addon_jars(&events::addon_folder(server, events::AddonKind::NeoForge));
    record_audit(&app, &id, "Events", "Add‑on removal", "Files recorded so far are kept.");
    Ok(addon_status_of(&state, &instance))
}

const FABRIC_API_ID: &str = "fabric-api";

fn ensure_fabric_api(instance: &ServerInstance, folder: &Path) -> Result<Option<String>, String> {
    let installed = addons::scan(Path::new(&instance.path));
    if installed.iter().any(|one| one.mod_id.eq_ignore_ascii_case(FABRIC_API_ID) && one.enabled) {
        return Ok(None);
    }
    let client = addons::client()?;
    let plan = addons::plan_install(&client, FABRIC_API_ID, "fabric", &instance.version, &Default::default()).map_err(|error| {
        format!(
            "The required Fabric API was not found, so nothing was installed ({error}). Fabric API for Minecraft {} may not exist yet.",
            instance.version
        )
    })?;
    let staging = folder.join(format!(".{}.download", plan.file_name));
    if let Err(error) = download_file(&client, &plan.download_url, &staging).and_then(|()| verify_sha512(&staging, &plan.sha512)) {
        let _ = fs::remove_file(&staging);
        return Err(format!("Could not download the required Fabric API: {error}"));
    }
    fs::rename(&staging, folder.join(&plan.file_name)).map_err(|error| format!("Could not install Fabric API: {error}"))?;
    Ok(Some(plan.file_name))
}

fn remove_old_addon_jars(folder: &Path) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let ours = events::is_our_addon(&name)
            && (name.ends_with(".jar") || name.ends_with(".wasm") || name.ends_with(&format!(".wasm{}", pumpkin::DISABLED)));
        if ours {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[tauri::command(async)]
#[specta::specta]
fn list_event_days(app: AppHandle, id: String) -> Result<Vec<String>, String> {
    let instance = instance_by_id(&app, &id)?;
    if is_far(&instance) {
        return Ok(Vec::new());
    }
    Ok(events::event_days(Path::new(&instance.path)))
}

#[tauri::command(async)]
#[specta::specta]
fn perf_history(app: AppHandle, id: String, day: String) -> Result<Vec<events::GameEvent>, String> {
    let instance = instance_by_id(&app, &id)?;
    if is_far(&instance) {
        return Ok(Vec::new());
    }
    let server = instance.path;
    let server = Path::new(&server);
    let day = if day.trim().is_empty() {
        match events::event_days(server).into_iter().next() {
            Some(newest) => newest,
            None => return Ok(Vec::new()),
        }
    } else {
        day
    };
    let mut found = events::read_day(server, &day, &events::Filter { kinds: vec!["perf".into()], player: String::new(), limit: 2000 });
    found.reverse();
    Ok(found)
}

const TILE: u32 = 128;

#[tauri::command(async)]
#[specta::specta]
async fn render_map_tile(
    app: AppHandle,
    id: String,
    realm: Option<String>,
    z: i32,
    tx: i32,
    ty: i32,
    night: Option<bool>,
) -> Result<String, String> {
    let (png, partial) = map_tile_png(app, id, realm, z, tx, ty, night.unwrap_or(false)).await?;
    use base64::Engine;
    let url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png.as_slice()));
    Ok(if partial { format!("partial:{url}") } else { url })
}

/// The desktop map loads tiles as plain images from `http://maptile.localhost/<id>/<layer>/<z>/<tx>/<ty>/<d|n>`, skipping
/// base64 and JSON on the way. `?probe` draws the tile and answers without the image, telling whether it is partial.
fn map_tile_protocol(app: AppHandle, request: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    tauri::async_runtime::spawn(async move {
        let uri = request.uri();
        let probe = uri.query().is_some_and(|query| query.split('&').any(|one| one == "probe"));
        let parts: Vec<String> = uri.path().trim_start_matches('/').split('/').map(web::unescape).collect();
        let answer = match parts.as_slice() {
            [id, layer, z, tx, ty, mode] => match (z.parse(), tx.parse(), ty.parse()) {
                (Ok(z), Ok(tx), Ok(ty)) => map_tile_png(app, id.clone(), Some(layer.clone()), z, tx, ty, mode == "n").await,
                _ => Err("Bad map tile address.".to_string()),
            },
            _ => Err("Bad map tile address.".to_string()),
        };
        let builder = tauri::http::Response::builder()
            .header("Access-Control-Allow-Origin", "*")
            .header("Access-Control-Expose-Headers", "X-Partial");
        let response = match answer {
            Ok((png, partial)) => builder
                .status(if probe { 204 } else { 200 })
                .header("Content-Type", "image/png")
                .header("X-Partial", if partial { "1" } else { "0" })
                .body(if probe { Vec::new() } else { png.to_vec() }),
            Err(error) => builder.status(500).header("Content-Type", "text/plain; charset=utf-8").body(error.into_bytes()),
        };
        responder.respond(response.unwrap_or_else(|_| tauri::http::Response::new(Vec::new())));
    });
}

/// One map tile as PNG bytes, from memory, the saved copy or a fresh drawing; `true` when part of it couldn't be read yet.
async fn map_tile_png(
    app: AppHandle,
    id: String,
    realm: Option<String>,
    z: i32,
    tx: i32,
    ty: i32,
    night: bool,
) -> Result<(Arc<Vec<u8>>, bool), String> {
    let z = z.clamp(-4, 5);
    let realm_name = worldmap::realm_key(realm.as_deref().unwrap_or("overworld"));
    let key = (id.clone(), if night { format!("{realm_name}#night") } else { realm_name.clone() }, z, tx, ty);
    let (cells, coarse, tiles, palettes, index) = {
        let state = app.state::<AppState>();
        if let Some(found) = state.map_tiles.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&key) {
            return Ok((found.clone(), false));
        }
        let index = state.map_index.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&(id.clone(), realm_name.clone())).cloned();
        (state.map_cells.clone(), state.map_coarse.clone(), state.map_tiles.clone(), state.map_palettes.clone(), index)
    };
    let instance = instance_by_id(&app, &id)?;

    tauri::async_runtime::spawn_blocking(move || {
        off_runtime(move || {
            let bpp = 2f32.powi(z);
            let span = (TILE as f32 * bpp) as i32;
            let saved = index.as_ref().and_then(|known| {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                ("tile-v2", night, z).hash(&mut hasher);
                for rz in (ty * span).div_euclid(512) - 1..=((ty + 1) * span - 1).div_euclid(512) + 1 {
                    for rx in (tx * span).div_euclid(512) - 1..=((tx + 1) * span - 1).div_euclid(512) + 1 {
                        if let Some(stamp) = known.get(&(rx, rz)) {
                            (rx, rz, format!("{stamp:?}")).hash(&mut hasher);
                        }
                    }
                }
                let dir = instance_data_dir(&app, &id).ok()?.join("map-tiles").join(&realm_name);
                let name = format!("{}{z}_{tx}_{ty}", if night { "n" } else { "d" });
                Some((dir.join(format!("{name}.png")), dir.join(format!("{name}.sig")), format!("{:016x}", hasher.finish())))
            });
            if let Some((png, sig, hash)) = &saved {
                if fs::read_to_string(sig).ok().as_deref() == Some(hash.as_str()) {
                    if let Ok(bytes) = fs::read(png) {
                        let bytes = Arc::new(bytes);
                        tiles.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(key, bytes.clone());
                        return Ok((bytes, false));
                    }
                }
            }
            static PALETTE_LOAD: Mutex<()> = Mutex::new(());
            let _loading = PALETTE_LOAD.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let held = palettes.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&id).cloned();
            let mut palette_failed = false;
            let palette = match held {
                Some(found) => found,
                None => {
                    let mut failed = false;
                    let mut read = |name: &str| {
                        map_file_bytes(&app, &instance, name).unwrap_or_else(|_| {
                            failed = true;
                            None
                        })
                    };
                    let grass = read("grass.png");
                    let foliage = read("foliage.png");
                    let palette_json = read("palette.json").unwrap_or_default();
                    const DRAFT: &[u8] = b"\"vanilla\":false";
                    if palette_json.windows(DRAFT.len()).any(|one| one == DRAFT) {
                        supply_client_jar(&app, &instance);
                    }
                    let one = Arc::new(worldmap::Palette::parse(&palette_json, grass.as_deref(), foliage.as_deref()));
                    if !failed {
                        palettes.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(id.clone(), one.clone());
                    }
                    palette_failed = failed;
                    one
                }
            };
            drop(_loading);
            let missed = std::cell::Cell::new(false);
            let scale = worldmap::shrink_for(z);
            let full = |rx: i32, rz: i32| -> Arc<worldmap::Tile> {
                let key = (id.clone(), realm_name.clone(), rx, rz);
                if let Some(found) = cells.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&key) {
                    return found.clone();
                }
                let stamp = match index.as_ref() {
                    Some(known) => match known.get(&(rx, rz)) {
                        Some(stamp) => Some(*stamp),
                        None => return worldmap::Tile::blank(),
                    },
                    None => None,
                };
                let read = map_region_bytes(&app, &instance, &realm_name, rx, rz, stamp);
                let Ok(found) = read else {
                    missed.set(true);
                    return worldmap::Tile::blank();
                };
                let one = found.and_then(|bytes| worldmap::parse_tile(&bytes)).map(Arc::new).unwrap_or_else(worldmap::Tile::blank);
                // Far-out views keep only the shrunken copy, leaving room for the regions close-up views need.
                if scale == 1 && one.present {
                    cells.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(key, one.clone());
                }
                one
            };
            let mut fetch = |rx: i32, rz: i32| -> Arc<worldmap::Tile> {
                if scale == 1 {
                    return full(rx, rz);
                }
                let stamp = index.as_ref().and_then(|known| known.get(&(rx, rz)).copied());
                let key = (id.clone(), realm_name.clone(), rx, rz, scale, stamp);
                if stamp.is_some() {
                    if let Some(found) = coarse.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&key) {
                        return found.clone();
                    }
                }
                let whole = full(rx, rz);
                if !whole.present {
                    return whole;
                }
                let small = Arc::new(whole.shrink(scale));
                if stamp.is_some() {
                    coarse.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(key, small.clone());
                }
                small
            };
            let map = Arc::new(worldmap::render_view(
                &mut fetch,
                &palette,
                tx * span,
                ty * span,
                TILE,
                TILE,
                bpp,
                night,
                worldmap::void_color(&realm_name),
            )?);
            if missed.get() || palette_failed {
                return Ok((map, true));
            }
            tiles.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(key, map.clone());
            if let Some((png, sig, hash)) = saved {
                if let Some(dir) = png.parent() {
                    let _ = fs::create_dir_all(dir);
                }
                if fs::write(&png, map.as_slice()).is_ok() {
                    let _ = fs::write(&sig, hash);
                }
            }
            Ok((map, false))
        })
    })
    .await
    .map_err(|error| format!("Could not draw the map tile: {error}"))?
}

fn map_tile_folder(instance: &ServerInstance, realm: &str) -> String {
    let base = if is_far(instance) {
        format!("{}/map", remote_addon_data_dir(instance))
    } else {
        events::event_folder(Path::new(&instance.path)).join("map").to_string_lossy().into_owned()
    };
    if realm.is_empty() {
        base
    } else {
        format!("{base}/{realm}")
    }
}

fn supply_client_jar(app: &AppHandle, instance: &ServerInstance) {
    static TRIED: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    let version =
        if pumpkin::is_pumpkin(&instance.core) { pumpkin_game_version(&instance.version) } else { instance.version.trim().to_string() };
    if !looks_like_version(&version) {
        return;
    }
    let tried = TRIED.get_or_init(Default::default);
    if !tried.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(format!("{}|{version}", instance.id)) {
        return;
    }
    let (app, instance) = (app.clone(), instance.clone());
    std::thread::spawn(move || {
        let done = (|| -> Result<(), String> {
            let jar = textures::client_jar(&app, &version)?;
            if pumpkin::is_pumpkin(&instance.core) {
                return supply_map_palette(&app, &instance, &jar);
            }
            let name = format!("client-{version}.jar");
            let folder = map_tile_folder(&instance, "");
            if is_far(&instance) {
                let bytes = fs::read(&jar).map_err(|error| error.to_string())?;
                tauri::async_runtime::block_on(far_upload(&app, &instance, &format!("{folder}/{name}"), &bytes, &name))
            } else {
                let target = Path::new(&folder).join(&name);
                fs::copy(&jar, &target).map(|_| ()).map_err(|error| error.to_string())
            }
        })();
        if let Err(error) = done {
            eprintln!("[map] Could not supply the client jar: {error}");
        }
    });
}

/// `0.2.1+26.3-26.51` → `26.3`.
fn pumpkin_game_version(version: &str) -> String {
    // The bundled fork's own version is the one the add-on was built against (`0.2.2+26.3-26.63`).
    let version = match events::pumpkin_target(version) {
        Some(target) if version == forks::PUMPKIN_FORK => target.games.first().map(String::as_str).unwrap_or(version),
        _ => version,
    };
    version.split('+').nth(1).and_then(|rest| rest.split('-').next()).unwrap_or_default().trim().to_string()
}

fn supply_map_palette(app: &AppHandle, instance: &ServerInstance, jar: &Path) -> Result<(), String> {
    let palette = textures::map_palette(jar)?;
    let folder = map_tile_folder(instance, "");
    let mut files = vec![("palette.json", palette.json.into_bytes())];
    files.extend(palette.grass.map(|bytes| ("grass.png", bytes)));
    files.extend(palette.foliage.map(|bytes| ("foliage.png", bytes)));
    for (name, bytes) in files.iter().rev() {
        if is_far(instance) {
            tauri::async_runtime::block_on(far_upload(app, instance, &format!("{folder}/{name}"), bytes, name))?;
        } else {
            fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
            fs::write(Path::new(&folder).join(name), bytes).map_err(|error| error.to_string())?;
        }
    }
    let state = app.state::<AppState>();
    state.map_palettes.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&instance.id);
    state.map_tiles.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).retain(|key, _| key.0 != instance.id);
    // An open map only redraws regions it is told changed, so report every region once.
    for (key, stamps) in state.map_index.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter_mut() {
        if key.0 == instance.id {
            *stamps = Arc::new(RegionStamps::default());
        }
    }
    if let Ok(dir) = instance_data_dir(app, &instance.id) {
        let _ = fs::remove_dir_all(dir.join("map-tiles"));
    }
    Ok(())
}

fn map_file_bytes(app: &AppHandle, instance: &ServerInstance, name: &str) -> Result<Option<Vec<u8>>, String> {
    let folder = map_tile_folder(instance, "");
    if !is_far(instance) {
        return match fs::read(Path::new(&folder).join(name)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.to_string()),
        };
    }
    const LIMIT: u64 = 16 * 1024 * 1024;
    let relative = format!("{folder}/{name}");
    let read = tauri::async_runtime::block_on(async {
        if !instance.panel_host.is_empty() {
            let (api, target) = panel_of(app, instance)?;
            api.read_bytes(&target, &relative, LIMIT).await
        } else {
            remote::read_bytes(app, &instance.remote_host, &instance.path, &relative, LIMIT).await
        }
    });
    match read {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if remote::is_missing(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn map_region_bytes(
    app: &AppHandle,
    instance: &ServerInstance,
    realm: &str,
    rx: i32,
    rz: i32,
    stamp: Option<(u64, u64)>,
) -> Result<Option<Vec<u8>>, String> {
    let name = format!("{realm}/{}", worldmap::tile_name(rx, rz));
    let (Some((at, size)), true) = (stamp, is_far(instance)) else {
        return map_file_bytes(app, instance, &name);
    };
    let folder = instance_data_dir(app, &instance.id)?.join("map-cache").join(realm);
    let prefix = format!("r.{rx}.{rz}.");
    let kept = folder.join(format!("{prefix}{at}-{size}.mnm"));
    if let Ok(bytes) = fs::read(&kept) {
        return Ok(Some(bytes));
    }
    let found = map_file_bytes(app, instance, &name)?;
    if let Some(bytes) = &found {
        let _ = fs::create_dir_all(&folder);
        if let Ok(entries) = fs::read_dir(&folder) {
            for one in entries.flatten() {
                if one.file_name().to_string_lossy().starts_with(&prefix) {
                    let _ = fs::remove_file(one.path());
                }
            }
        }
        let _ = fs::write(&kept, bytes);
    }
    Ok(found)
}

async fn map_stamps(app: &AppHandle, instance: &ServerInstance, realm: &str) -> Option<RegionStamps> {
    let folder = map_tile_folder(instance, realm);
    let rows: Option<Vec<(String, u64, u64)>> = if is_far(instance) {
        far_list(app, instance, &folder)
            .await
            .ok()
            .map(|rows| rows.into_iter().map(|row| (row.name, row.modified_at, row.size_bytes)).collect())
    } else {
        fs::read_dir(&folder).ok().map(|entries| {
            entries
                .flatten()
                .map(|one| {
                    let meta = one.metadata().ok();
                    let at = meta
                        .as_ref()
                        .and_then(|meta| meta.modified().ok())
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|gap| gap.as_secs())
                        .unwrap_or(0);
                    (one.file_name().to_string_lossy().into_owned(), at, meta.map(|meta| meta.len()).unwrap_or(0))
                })
                .collect()
        })
    };
    rows.map(|rows| rows.iter().filter_map(|(name, at, size)| worldmap::tile_coords(name).map(|spot| (spot, (*at, *size)))).collect())
}

/// Regions whose tile was added, removed or redrawn between two listings.
fn changed_regions(old: &RegionStamps, fresh: &RegionStamps) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = fresh
        .iter()
        .filter(|(spot, stamp)| old.get(spot) != Some(stamp))
        .map(|(spot, _)| *spot)
        .chain(old.keys().filter(|spot| !fresh.contains_key(spot)).copied())
        .collect();
    out.sort_unstable();
    out
}

/// Whether a rendered tile could show anything from this region, counting the one-block edge its shading reads.
fn tile_touches_region(z: i32, tx: i32, ty: i32, rx: i32, rz: i32) -> bool {
    let span = (TILE as f64 * 2f64.powi(z)) as i64;
    let (left, top) = (tx as i64 * span, ty as i64 * span);
    let (rleft, rtop) = (rx as i64 * 512 - 1, rz as i64 * 512 - 1);
    left < rleft + 514 && rleft < left + span && top < rtop + 514 && rtop < top + span
}

/// What changed in the map since the last look: drops just those regions from the caches, so an open map can
/// refetch the tiles that show them while the add-on keeps drawing.
#[tauri::command(async)]
#[specta::specta]
async fn map_changes(app: AppHandle, state: State<'_, AppState>, id: String, realm: Option<String>) -> Result<Vec<(i32, i32)>, String> {
    let instance = instance_by_id(&app, &id)?;
    let realm = worldmap::realm_key(realm.as_deref().unwrap_or("overworld"));
    let Some(fresh) = map_stamps(&app, &instance, &realm).await else {
        return Ok(Vec::new());
    };
    let index_key = (id.clone(), realm.clone());
    let old = state.map_index.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&index_key).cloned();
    let old = match old {
        Some(old) => old,
        None => {
            state.map_index.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(index_key, Arc::new(fresh));
            return Ok(Vec::new());
        }
    };
    let changed = changed_regions(&old, &fresh);
    if changed.is_empty() {
        return Ok(changed);
    }
    let night = format!("{realm}#night");
    state
        .map_cells
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .retain(|key, _| key.0 != id || key.1 != realm || !changed.contains(&(key.2, key.3)));
    state.map_tiles.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).retain(|key, _| {
        key.0 != id
            || (key.1 != realm && key.1 != night)
            || !changed.iter().any(|(rx, rz)| tile_touches_region(key.2, key.3, key.4, *rx, *rz))
    });
    state.map_index.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(index_key, Arc::new(fresh));
    Ok(changed)
}

#[tauri::command(async)]
#[specta::specta]
async fn world_map_bounds(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    realm: Option<String>,
) -> Result<(i32, i32, i32, i32), String> {
    let instance = instance_by_id(&app, &id)?;
    let realm = worldmap::realm_key(realm.as_deref().unwrap_or("overworld"));
    let realm = realm.as_str();
    forget_chunk_stats(state.inner(), &id);
    let fresh = map_stamps(&app, &instance, realm).await;
    let index_key = (id.clone(), realm.to_string());
    let old = state.map_index.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&index_key).cloned();
    {
        let mut cells = state.map_cells.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        cells.retain(|key, _| {
            if key.0 != id || key.1 != realm {
                return true;
            }
            match (&old, &fresh) {
                (Some(old), Some(fresh)) => old.get(&(key.2, key.3)) == fresh.get(&(key.2, key.3)),
                _ => false,
            }
        });
    }
    // Drawn tiles stay unless a region under them changed (opening the map again used to redraw everything).
    let changed = match (&old, &fresh) {
        (Some(old), Some(fresh)) => Some(changed_regions(old, fresh)),
        _ => None,
    };
    let night = format!("{realm}#night");
    state.map_tiles.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).retain(|key, _| {
        if key.0 != id || (key.1 != realm && key.1 != night) {
            return true;
        }
        changed.as_ref().is_some_and(|changed| !changed.iter().any(|(rx, rz)| tile_touches_region(key.2, key.3, key.4, *rx, *rz)))
    });
    state.map_palettes.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&id);
    {
        let mut index = state.map_index.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match &fresh {
            Some(fresh) => {
                index.insert(index_key, Arc::new(fresh.clone()));
            }
            None => {
                index.remove(&index_key);
            }
        }
    }
    let found: Vec<(i32, i32)> = fresh.map(|fresh| fresh.into_keys().collect()).unwrap_or_default();
    if found.is_empty() {
        return Err(
            "No map drawn yet. Keep the server running with the Monolab mod/plugin installed and it starts drawing within 1–2 minutes."
                .into(),
        );
    }
    let min_x = found.iter().map(|(x, _)| *x).min().unwrap_or(0) * 512;
    let max_x = (found.iter().map(|(x, _)| *x).max().unwrap_or(0) + 1) * 512;
    let min_z = found.iter().map(|(_, z)| *z).min().unwrap_or(0) * 512;
    let max_z = (found.iter().map(|(_, z)| *z).max().unwrap_or(0) + 1) * 512;
    Ok((min_x, min_z, max_x, max_z))
}

static NO_ADDON: Mutex<Option<HashSet<String>>> = Mutex::new(None);
fn no_addon(id: &str) -> bool {
    NO_ADDON.lock().expect("addon state poisoned").as_ref().is_some_and(|set| set.contains(id))
}

#[tauri::command(async)]
#[specta::specta]
async fn read_live(app: AppHandle, id: String) -> Result<Option<events::LiveState>, String> {
    let live = read_live_inner(&app, &id).await?;
    {
        let mut guard = NO_ADDON.lock().expect("addon state poisoned");
        let set = guard.get_or_insert_with(HashSet::new);
        if live.is_some() {
            set.remove(&id);
        } else {
            set.insert(id.clone());
        }
    }
    if let Some(now) = live.as_ref().filter(|one| one.tps.is_some()) {
        app.state::<AppState>().tps.lock().expect("tps state poisoned").insert(
            id.clone(),
            TpsSnapshot {
                tps: now.tps.map(|v| v as f32),
                mspt: now.mspt.map(|v| v as f32),
                updated_at: unix_now(),
                heap_mib: now.memory_mib,
            },
        );
    }
    Ok(live)
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct LoadedWorld {
    world: String,
    chunks: Vec<i32>,
    forced: Vec<i32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct LoadedChunks {
    at: f64,
    worlds: Vec<LoadedWorld>,
}

fn loaded_from_text(text: &str) -> Option<LoadedChunks> {
    let found: LoadedChunks = serde_json::from_str(text).ok()?;
    let age = unix_now() as f64 * 1000.0 - found.at;
    (age < 30_000.0).then_some(found)
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ChunkyStatus {
    installed: bool,
    tool: String,
    folder: String,
    worlds: Vec<String>,
    spawn: Option<[i32; 2]>,
    progress: Option<chunky::ChunkyProgress>,
}

async fn chunky_folder_and_jar(app: &AppHandle, instance: &ServerInstance) -> (String, String) {
    for folder in ["plugins", "mods"] {
        let names: Vec<String> = if is_far(instance) {
            far_list(app, instance, folder).await.unwrap_or_default().into_iter().filter(|one| !one.directory).map(|one| one.name).collect()
        } else {
            fs::read_dir(Path::new(&instance.path).join(folder))
                .into_iter()
                .flatten()
                .flatten()
                .map(|one| one.file_name().to_string_lossy().to_string())
                .collect()
        };
        if names.iter().any(|name| chunky::is_chunksmith_jar(name)) {
            return (folder.to_string(), "chunksmith".into());
        }
        if names.iter().any(|name| chunky::is_chunky_jar(name)) {
            return (folder.to_string(), "chunky".into());
        }
    }
    let folder = if matches!(instance.core.as_str(), "Paper" | "Purpur" | "Folia" | "Leaf") { "plugins" } else { "mods" };
    (folder.to_string(), String::new())
}

#[tauri::command(async)]
#[specta::specta]
async fn chunky_status(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<ChunkyStatus, String> {
    let instance = instance_by_id(&app, &id)?;
    let (folder, tool) = chunky_folder_and_jar(&app, &instance).await;
    let installed = !tool.is_empty();
    let level = if is_far(&instance) {
        far_read(&app, &instance, "server.properties")
            .await
            .ok()
            .and_then(|text| text.lines().find_map(|line| line.trim().strip_prefix("level-name=")).map(|value| value.trim().to_string()))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "world".into())
    } else {
        level_name_of(&instance)
    };
    let worlds = ["overworld", "nether", "end"].iter().map(|realm| chunky::world_name(&instance.core, &level, realm)).collect();
    let spawn = if is_far(&instance) {
        None
    } else {
        nbt::read_level_data(&Path::new(&instance.path).join(&level).join("level.dat")).ok().map(|data| [data.spawn[0], data.spawn[2]])
    };
    let progress = state.chunky.lock().expect("chunky state poisoned").get(&id).cloned();
    Ok(ChunkyStatus { installed, tool, folder, worlds, spawn, progress })
}

#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
async fn chunky_run(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    action: String,
    world: String,
    x: i32,
    z: i32,
    radius: u32,
    radius_z: u32,
    shape: String,
) -> Result<(), String> {
    let instance = instance_by_id(&app, &id)?;
    if !server_on(&app, state.inner(), &id)? {
        return Err("Start the server first. The pre-generator runs inside the server.".into());
    }
    let (_, tool) = chunky_folder_and_jar(&app, &instance).await;
    let root = chunky::command_root(&tool);
    let world = world.trim().to_string();
    if world.is_empty() || world.chars().any(|one| one.is_whitespace() || one.is_control()) {
        return Err("The world name is malformed.".into());
    }
    let lines: Vec<String> = match action.as_str() {
        "start" => {
            let radius_z = if radius_z == 0 { radius } else { radius_z };
            if radius == 0 || radius > 100_000 || radius_z > 100_000 {
                return Err("Choose a radius between 1 and 100,000 blocks.".into());
            }
            let shape = match shape.as_str() {
                "circle" => "circle",
                "rectangle" if radius_z != radius => "rectangle",
                _ => "square",
            };
            let size = if shape == "rectangle" { format!("{radius} {radius_z}") } else { radius.to_string() };
            vec![
                format!("{root} world {world}"),
                format!("{root} center {x} {z}"),
                format!("{root} radius {size}"),
                format!("{root} shape {shape}"),
                format!("{root} start"),
            ]
        }
        "pause" => vec![format!("{root} pause")],
        "continue" => vec![format!("{root} continue")],
        "cancel" => vec![format!("{root} cancel"), format!("{root} confirm")],
        _ => return Err("Unknown pre-generation action.".into()),
    };
    for line in &lines {
        say_to(&app, state.inner(), &instance.id, line);
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if action == "start" {
        state.chunky.lock().expect("chunky state poisoned").insert(
            id.clone(),
            chunky::ChunkyProgress { world: world.clone(), state: "running".into(), at: unix_now(), ..Default::default() },
        );
    }
    record_audit(&app, &id, "Server", "Chunk pre-generation", &format!("{action} · {world}"));
    Ok(())
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct MapWebView {
    supported: bool,
    reason: String,
    addon_ready: bool,
    enabled: bool,
    port: u16,
    private: bool,
    players: bool,
    running: bool,
    error: String,
    path: String,
    lan_url: String,
}

async fn map_web_read(app: &AppHandle, instance: &ServerInstance, name: &str) -> Option<String> {
    if is_far(instance) {
        return far_read(app, instance, &format!("{}/{name}", remote_addon_data_dir(instance))).await.ok();
    }
    fs::read_to_string(events::event_folder(Path::new(&instance.path)).join(name)).ok()
}

async fn map_web_write(app: &AppHandle, instance: &ServerInstance, name: &str, text: &str) -> Result<(), String> {
    if is_far(instance) {
        return far_write(app, instance, &format!("{}/{name}", remote_addon_data_dir(instance)), text).await;
    }
    let target = events::event_folder(Path::new(&instance.path)).join(name);
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    fs::write(target, text).map_err(|error| error.to_string())
}

async fn map_web_view(app: &AppHandle, state: &AppState, instance: &ServerInstance) -> MapWebView {
    let kind = events::addon_kind(&instance.core);
    let supported = matches!(kind, Some(events::AddonKind::Paper | events::AddonKind::NeoForge | events::AddonKind::Fabric));
    let settings = map_web_read(app, instance, "web.json")
        .await
        .map(|text| mapshare::read_settings(&text))
        .unwrap_or_else(|| mapshare::read_settings(""));
    let status = map_web_read(app, instance, "web-status.json").await.and_then(|text| mapshare::read_status(&text)).unwrap_or_default();
    let (addon_ready, reason) = if !supported {
        (
            false,
            "The web map runs inside the Monolab add-on, which is available for Paper, Purpur, Folia, NeoForge and Fabric servers."
                .to_string(),
        )
    } else if is_far(instance) {
        (true, String::new())
    } else {
        let addon = addon_status_of(state, instance);
        let ready = addon.installed && addon.installed_version.unwrap_or(0) >= mapshare::FIRST_ADDON;
        (ready, if ready { String::new() } else { "Install or update the Monolab add-on first. The web map comes with it.".to_string() })
    };
    let alive = is_far(instance) || state.processes.lock().expect("process state poisoned").contains_key(&instance.id);
    MapWebView {
        supported,
        reason,
        addon_ready,
        enabled: settings.enabled,
        port: settings.port,
        private: !settings.token.is_empty(),
        players: settings.players,
        running: settings.enabled && status.running && alive,
        error: if settings.enabled { status.error.unwrap_or_default() } else { String::new() },
        path: mapshare::path_of(&settings),
        lan_url: if is_far(instance) {
            String::new()
        } else {
            format!("http://{}:{}{}", lan_address(), settings.port, mapshare::path_of(&settings))
        },
    }
}

#[tauri::command(async)]
#[specta::specta]
async fn map_web_state(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<MapWebView, String> {
    let instance = instance_by_id(&app, &id)?;
    Ok(map_web_view(&app, &state, &instance).await)
}

#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
async fn map_web_set(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
    port: u16,
    secret: bool,
    players: bool,
    renew: bool,
    labels: HashMap<String, String>,
) -> Result<MapWebView, String> {
    let instance = instance_by_id(&app, &id)?;
    if !mapshare::port_ok(port) {
        return Err("Use a port between 1024 and 65535.".into());
    }
    let old = map_web_read(&app, &instance, "web.json")
        .await
        .map(|text| mapshare::read_settings(&text))
        .unwrap_or_else(|| mapshare::read_settings(""));
    let token = match (secret, old.token.is_empty() || renew) {
        (false, _) => String::new(),
        (true, true) => mapshare::make_token(),
        (true, false) => old.token.clone(),
    };
    let labels = if labels.is_empty() {
        old.labels
    } else {
        labels.into_iter().filter(|(key, value)| key.len() <= 64 && value.len() <= 200).take(64).collect()
    };
    let settings = mapshare::Settings { enabled, port, token, players, name: instance.name.clone(), labels };
    let text = serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?;
    map_web_write(&app, &instance, "web.json", &text).await?;
    record_audit(&app, &id, "Server", if enabled { "Web map on" } else { "Web map off" }, &port.to_string());
    Ok(map_web_view(&app, &state, &instance).await)
}

/// The address friends outside the house reach this computer by. Asked only when the user presses the button.
#[tauri::command(async)]
#[specta::specta]
async fn public_address() -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent(net::USER_AGENT)
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|error| error.to_string())?;
    let text = client
        .get("https://api.ipify.org")
        .send()
        .await
        .map_err(|_| "Could not look up the public address. Check the internet connection.".to_string())?
        .text()
        .await
        .map_err(|_| "Could not look up the public address. Check the internet connection.".to_string())?;
    let text = text.trim();
    text.parse::<std::net::IpAddr>().map(|ip| ip.to_string()).map_err(|_| "The lookup did not return an address.".to_string())
}

/// Whether something answers on this address and port from here. A router that can't loop back to itself can say no
/// even when friends get through, so the dialog words a "no" carefully.
#[tauri::command(async)]
#[specta::specta]
async fn map_web_probe(host: String, port: u16) -> Result<bool, String> {
    let ip: std::net::IpAddr = host.trim().parse().map_err(|_| "That is not an address.".to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        std::net::TcpStream::connect_timeout(&std::net::SocketAddr::new(ip, port), Duration::from_secs(4)).is_ok()
    })
    .await
    .map_err(|error| error.to_string())
}

fn forget_chunk_stats(state: &AppState, id: &str) {
    state.chunk_stats.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).retain(|key, _| key.0 != id);
    state.chunk_entities.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).retain(|key, _| key.0 != id);
    state.chunk_folders.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).retain(|key, _| key.0 != id);
}

async fn level_name_any(app: &AppHandle, instance: &ServerInstance) -> String {
    if !is_far(instance) {
        return level_name_of(instance);
    }
    far_read(app, instance, "server.properties")
        .await
        .ok()
        .and_then(|text| text.lines().find_map(|line| line.trim().strip_prefix("level-name=")).map(|value| value.trim().to_string()))
        .filter(|value| !value.is_empty() && !value.contains(['/', '\\', ':']))
        .unwrap_or_else(|| "world".into())
}

async fn chunk_folder(app: &AppHandle, state: &AppState, instance: &ServerInstance, realm: &str) -> ChunkFolder {
    let key = (instance.id.clone(), realm.to_string());
    if let Some(found) = state.chunk_folders.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&key) {
        return found.clone();
    }
    let level = level_name_any(app, instance).await;
    let mut candidates = trim_candidates(&level, realm);
    if let Some((space, path)) = realm.split_once('.') {
        candidates = vec![format!("{level}/dimensions/{space}/{}", path.replace('.', "/"))];
    }
    let mut found = ChunkFolder::default();
    for candidate in candidates {
        let regions: HashMap<String, (u64, u64)> = stamp_list(app, instance, &format!("{candidate}/region"))
            .await
            .into_iter()
            .filter(|(name, _)| trim::region_coords(name).is_some())
            .collect();
        if !regions.is_empty() {
            let entities = stamp_list(app, instance, &format!("{candidate}/entities")).await.into_iter().collect();
            found = ChunkFolder { folder: candidate, regions, entities };
            break;
        }
    }
    state.chunk_folders.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(key, found.clone());
    found
}

#[tauri::command(async)]
#[specta::specta]
async fn chunk_stats(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    realm: String,
    metric: String,
    rx: i32,
    rz: i32,
) -> Result<Option<Vec<Option<f64>>>, String> {
    if !chunkstats::METRICS.contains(&metric.as_str()) {
        return Err("Unknown map view.".into());
    }
    let instance = instance_by_id(&app, &id)?;
    let realm = worldmap::realm_key(&realm);
    let key = (id.clone(), realm.clone(), rx, rz);
    let cached_grid = |state: &AppState| -> Option<Vec<Option<f64>>> {
        if metric == "entities" {
            state.chunk_entities.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&key).map(|grid| grid.as_ref().clone())
        } else {
            state.chunk_stats.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&key).and_then(|one| one.get(&metric).cloned())
        }
    };
    if let Some(grid) = cached_grid(state.inner()) {
        return Ok(Some(grid));
    }
    let tried = key.clone();
    {
        let file = format!("{realm}/r.{rx}.{rz}.mst");
        let app_for = app.clone();
        let instance_for = instance.clone();
        let read = tauri::async_runtime::spawn_blocking(move || off_runtime(move || map_file_bytes(&app_for, &instance_for, &file)))
            .await
            .map_err(|error| error.to_string())?;
        if let Ok(Some(bytes)) = read {
            if let Some((stats, entities)) = chunkstats::read_mst(&bytes) {
                state.chunk_stats.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(tried.clone(), Arc::new(stats));
                state.chunk_entities.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(tried, Arc::new(entities));
                return Ok(cached_grid(state.inner()));
            }
        }
    }
    let found = chunk_folder(&app, state.inner(), &instance, &realm).await;
    let name = format!("r.{rx}.{rz}.mca");
    if metric == "entities" {
        if let Some(grid) = state.chunk_entities.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&key) {
            return Ok(Some(grid.as_ref().clone()));
        }
        let Some(stamp) = found.entities.get(&name).copied() else {
            return Ok(None);
        };
        let cache = stats_cache_file(&app, &id, &realm, &format!("{name}.entities"));
        let grid = match read_cached::<chunkstats::Grid>(&cache, stamp) {
            Some(grid) => Arc::new(grid),
            None => {
                let Some(bytes) = trim_read(&app, &instance, &format!("{}/entities/{name}", found.folder)).await else { return Ok(None) };
                let grid = tauri::async_runtime::spawn_blocking(move || chunkstats::read_entities(&bytes))
                    .await
                    .map_err(|error| error.to_string())?;
                write_cached(&cache, stamp, &grid);
                Arc::new(grid)
            }
        };
        state.chunk_entities.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(key, grid.clone());
        return Ok(Some(grid.as_ref().clone()));
    }
    let held = state.chunk_stats.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&key).cloned();
    let stats = match held {
        Some(one) => one,
        None => {
            let Some(stamp) = found.regions.get(&name).copied() else {
                return Ok(None);
            };
            let cache = stats_cache_file(&app, &id, &realm, &name);
            let one = match read_cached::<chunkstats::RegionStats>(&cache, stamp) {
                Some(stats) => Arc::new(stats),
                None => {
                    let Some(bytes) = trim_read(&app, &instance, &format!("{}/region/{name}", found.folder)).await else { return Ok(None) };
                    let stats = tauri::async_runtime::spawn_blocking(move || chunkstats::read_region(&bytes))
                        .await
                        .map_err(|error| error.to_string())?;
                    write_cached(&cache, stamp, &stats);
                    Arc::new(stats)
                }
            };
            state.chunk_stats.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(key, one.clone());
            one
        }
    };
    Ok(stats.get(&metric).cloned())
}

fn local_free_bytes(folder: &Path) -> u64 {
    let Ok(folder) = folder.canonicalize().map(plain_path) else { return 0 };
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|disk| folder.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map_or(0, |disk| disk.available_space())
}

#[tauri::command(async)]
#[specta::specta]
async fn disk_usage(app: AppHandle, id: String) -> Result<disk::DiskReport, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        return Err("Disk usage isn't available for panel servers yet.".into());
    }
    if !instance.remote_host.is_empty() {
        let conn = remote::connect(&app, &instance.remote_host).await?;
        let script = format!("cd {} || exit 9\n{}", remote::quote(&instance.path), disk::remote_script());
        let out = conn.run_long(&script).await?;
        if out.code == 9 {
            return Err("The server folder was not found on the SSH server.".into());
        }
        return Ok(disk::parse_remote(&out.stdout));
    }
    let root = PathBuf::from(&instance.path);
    let vault = backup_folder(&app, &instance)?;
    let trash = instance_data_dir(&app, &id)?.join("trash");
    tauri::async_runtime::spawn_blocking(move || {
        let entries = disk::scan_local(&root);
        disk::DiskReport {
            total_bytes: disk::top_total(&entries),
            entries,
            free_bytes: local_free_bytes(&root),
            backups_bytes: disk::tree_size(&vault).0,
            trash_bytes: disk::tree_size(&trash).0,
            cleanable: disk::local_cleanable(&root),
        }
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command(async)]
#[specta::specta]
async fn clean_disk(app: AppHandle, id: String, kind: String) -> Result<u32, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        return Err("Disk usage isn't available for panel servers yet.".into());
    }
    if !disk::KINDS.contains(&kind.as_str()) {
        return Err("Unknown cleanup type.".into());
    }
    let count = if !instance.remote_host.is_empty() {
        let command = disk::remote_clean_command(&kind).ok_or("Unknown cleanup type.")?;
        let conn = remote::connect(&app, &instance.remote_host).await?;
        conn.run_long(&format!("cd {} && {command}", remote::quote(&instance.path))).await?.ok()?;
        0
    } else {
        let mut count = 0;
        for file in disk::cleanable_files(Path::new(&instance.path), &kind) {
            if fs::remove_file(&file).is_ok() {
                count += 1;
            }
        }
        count
    };
    record_audit(&app, &id, "File", "Disk cleanup", &kind);
    Ok(count)
}

#[tauri::command(async)]
#[specta::specta]
fn map_surface_height(state: State<'_, AppState>, id: String, realm: Option<String>, x: i32, z: i32) -> Option<i32> {
    let realm = worldmap::realm_key(realm.as_deref().unwrap_or("overworld"));
    let tile = state
        .map_cells
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&(id, realm, x.div_euclid(512), z.div_euclid(512)))
        .cloned()?;
    let at = z.rem_euclid(512) as usize * worldmap::SPAN + x.rem_euclid(512) as usize;
    let block = *tile.block.get(at)?;
    if !tile.present || block == u16::MAX {
        return None;
    }
    Some(i32::from(*tile.height.get(at)?) + 1)
}

#[tauri::command(async)]
#[specta::specta]
async fn read_loaded_chunks(app: AppHandle, id: String) -> Result<Option<LoadedChunks>, String> {
    let instance = instance_by_id(&app, &id)?;
    if is_far(&instance) {
        let at = format!("{}/loaded.json", remote_addon_data_dir(&instance));
        return Ok(far_read(&app, &instance, &at).await.ok().and_then(|text| loaded_from_text(&text)));
    }
    let file = events::event_folder(Path::new(&instance.path)).join("loaded.json");
    Ok(fs::read_to_string(file).ok().and_then(|text| loaded_from_text(&text)))
}

async fn read_live_inner(app: &AppHandle, id: &str) -> Result<Option<events::LiveState>, String> {
    let app = app.clone();
    let instance = instance_by_id(&app, id)?;
    if is_far(&instance) {
        let at = format!("{}/live.json", remote_addon_data_dir(&instance));
        return Ok(match far_read(&app, &instance, &at).await {
            Ok(text) => events::live_from_text(&text),
            Err(_) => None,
        });
    }
    Ok(events::read_live(Path::new(&instance.path)))
}

fn remote_addon_data_dir(instance: &ServerInstance) -> &'static str {
    if crate::pumpkin::is_pumpkin(&instance.core) {
        "plugins/data/monolab"
    } else {
        match events::addon_kind(&instance.core) {
            Some(events::AddonKind::NeoForge) | Some(events::AddonKind::Fabric) => "config/monolab",
            _ => "plugins/Monolab",
        }
    }
}

#[tauri::command(async)]
#[specta::specta]
async fn diagnose_lag(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<lag::LagDiagnosis, String> {
    lag_report(&app, &state, &id).await
}

async fn lag_report(app: &AppHandle, state: &AppState, id: &str) -> Result<lag::LagDiagnosis, String> {
    let app = app.clone();
    let id = id.to_string();
    let instance = instance_by_id(&app, &id)?;
    let server = Path::new(&instance.path);
    let far = is_far(&instance);
    let live = if far { read_live(app.clone(), id.clone()).await.ok().flatten() } else { events::read_live(server) };
    let (machine_memory_mib, cpu_percent) = if far {
        (0, None)
    } else {
        let pid = state.processes.lock().expect("process state poisoned").get(&id).map(|process| process.pid);
        let mut system = state.system.lock().expect("system state poisoned");
        system.refresh_memory();
        let machine = system.total_memory() / 1024 / 1024;
        let cpu = pid.map(|pid| {
            system.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::All,
                true,
                sysinfo::ProcessRefreshKind::nothing().with_cpu().with_memory(),
            );
            process_tree_usage(&system, Pid::from_u32(pid)).0
        });
        (machine, cpu)
    };
    let properties = if pumpkin::is_pumpkin(&instance.core) {
        String::new()
    } else if far {
        far_read(&app, &instance, "server.properties").await.unwrap_or_default()
    } else {
        fs::read_to_string(server.join("server.properties")).unwrap_or_default()
    };
    let property = |key: &str| -> Option<u32> {
        properties.lines().find_map(|line| {
            let (name, value) = line.trim().split_once('=')?;
            (name.trim() == key).then(|| value.trim().parse::<u32>().ok())?
        })
    };
    let perf = perf_history(app.clone(), id.clone(), String::new()).unwrap_or_default();
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_millis() as u64).unwrap_or(0);
    let mut found = lag::diagnose(&lag::Input {
        live: live.as_ref(),
        max_memory_mib: u64::from(instance.max_memory_mib),
        machine_memory_mib,
        cpu_percent,
        view_distance: property("view-distance"),
        simulation_distance: property("simulation-distance"),
        jvm_args: &instance.jvm_args,
        perf: &perf,
        now_ms,
    });
    let mut info = if far {
        let mut has_jar = false;
        for folder in ["mods", "plugins"] {
            if let Ok(rows) = far_list(&app, &instance, folder).await {
                has_jar |= rows.iter().any(|row| !row.directory && spark::jar_name_is_spark(&row.name));
            }
        }
        spark::info_with(&instance.core, &instance.version, has_jar)
    } else {
        spark::info(&instance.core, &instance.version, server)
    };
    info.report = state.spark_reports.lock().expect("spark state poisoned").get(&id).cloned();
    found.spark = Some(info);
    Ok(found)
}

const LAG_ALERT_TPS: f64 = 15.0;
/// Must last this long to notify (seconds).
const LAG_ALERT_HOLD_SECS: u64 = 60;
const LAG_ALERT_COOLDOWN_SECS: u64 = 600;

const RELOOK_EVERY: u64 = 60;

async fn relook_remote(app: &AppHandle, instances: &[ServerInstance], looked: &mut HashMap<String, u64>) {
    let now = unix_now();
    for one in instances.iter().filter(|one| !one.remote_host.is_empty()) {
        if ssh_paused(app, &one.remote_host) {
            continue;
        }
        if one.core != sniff::UNKNOWN && looks_like_version(&one.version) {
            continue;
        }
        let last = looked.entry(one.id.clone()).or_insert(0);
        if now.saturating_sub(*last) < RELOOK_EVERY {
            continue;
        }
        *last = now;
        let found = match remote::detect(app, &one.remote_host, &one.path).await {
            Ok(found) => found,
            Err(error) => {
                eprintln!("[remote] relook {}: {error}", one.name);
                continue;
            }
        };
        let fresh_core = found.core != sniff::UNKNOWN && one.core == sniff::UNKNOWN;
        let fresh_version = looks_like_version(&found.version) && !looks_like_version(&one.version);
        if !fresh_core && !fresh_version {
            continue;
        }
        let _store = lock_instance_store();
        let Ok(mut list) = load_instances(app) else {
            continue;
        };
        let Some(mine) = list.iter_mut().find(|item| item.id == one.id) else {
            continue;
        };
        if fresh_core {
            mine.core = found.core.clone();
        }
        if fresh_version {
            mine.version = found.version.clone();
        }
        let _ = save_instances(app, &list);
    }
}

const SSH_GIVE_UP_AFTER: u32 = 3;

fn ssh_paused(app: &AppHandle, host: &str) -> bool {
    app.state::<AppState>()
        .remote_failures
        .lock()
        .expect("remote state poisoned")
        .get(host)
        .is_some_and(|failed| *failed >= SSH_GIVE_UP_AFTER)
}

/// Lets the watcher dial an SSH host again after it gave up.
#[tauri::command(async)]
#[specta::specta]
fn retry_ssh_host(app: AppHandle, id: String) {
    let state = app.state::<AppState>();
    state.remote_failures.lock().expect("remote state poisoned").remove(&id);
    state.remote_kick.notify_one();
}

fn start_remote_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut looked: HashMap<String, u64> = HashMap::new();
        let mut answered: HashMap<String, std::time::Instant> = HashMap::new();
        let kick = app.state::<AppState>().remote_kick.clone();
        let mut first = true;
        let mut rounds: u64 = 0;
        loop {
            if !first {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(3)) => {}
                    _ = kick.notified() => {}
                }
            }
            first = false;
            let instances = load_instances(&app).unwrap_or_default();
            let mut alive: HashMap<String, remote::Live> = HashMap::new();
            let mut blind: Vec<String> = Vec::new();
            let mut by_host: HashMap<String, Vec<(String, String)>> = HashMap::new();
            for one in instances.iter().filter(|one| !one.remote_host.is_empty()) {
                by_host.entry(one.remote_host.clone()).or_default().push((one.id.clone(), one.path.clone()));
            }
            for (host, servers) in by_host {
                const KEEP_BLIND: Duration = Duration::from_secs(30);
                if ssh_paused(&app, &host) {
                    continue;
                }
                match remote::live_state_checked(&app, &host, &servers).await {
                    Ok(found) => {
                        app.state::<AppState>().remote_failures.lock().expect("remote state poisoned").remove(&host);
                        alive.extend(found);
                        answered.insert(host.clone(), std::time::Instant::now());
                        app.state::<AppState>().remote_unreachable.lock().expect("remote state poisoned").remove(&host);
                    }
                    Err(_) => {
                        *app.state::<AppState>().remote_failures.lock().expect("remote state poisoned").entry(host.clone()).or_insert(0) +=
                            1;
                        app.state::<AppState>().remote_unreachable.lock().expect("remote state poisoned").insert(host.clone());
                        let fresh = answered.get(&host).is_some_and(|when| when.elapsed() < KEEP_BLIND);
                        if fresh {
                            blind.push(host.clone());
                        }
                    }
                }
                app.state::<AppState>().remote_checked.lock().expect("remote state poisoned").insert(host);
            }
            relook_remote(&app, &instances, &mut looked).await;
            let state = app.state::<AppState>();
            {
                let mut have = state.remote_running.lock().expect("remote state poisoned");
                let mut starting = state.remote_starting.lock().expect("remote state poisoned");
                for one in instances.iter().filter(|one| blind.contains(&one.remote_host)) {
                    if let Some(kept) = have.get(&one.id).cloned() {
                        alive.entry(one.id.clone()).or_insert(kept);
                    }
                }
                for (id, since) in starting.iter() {
                    if since.elapsed() < Duration::from_secs(10) && !alive.contains_key(id) {
                        if let Some(one) = have.get(id).cloned() {
                            alive.insert(id.clone(), one);
                        }
                    }
                }
                starting.retain(|id, _| alive.contains_key(id));
                if rounds > 0 {
                    for one in instances.iter().filter(|one| !one.remote_host.is_empty() && !blind.contains(&one.remote_host)) {
                        let was = have.contains_key(&one.id);
                        let now = alive.contains_key(&one.id);
                        let reachable = !state.remote_unreachable.lock().expect("remote state poisoned").contains(&one.remote_host);
                        if was && !now && reachable {
                            if runlog::take_asked(&one.id) {
                                note_run(&app, &one.id, "stop", "");
                            } else {
                                note_run(&app, &one.id, "crash", "Stopped without a stop request (crash or stopped outside the app)");
                            }
                        } else if !was && now {
                            runlog::started(&one.id);
                            note_run(&app, &one.id, "start", "");
                        }
                    }
                }
                rounds += 1;
                *have = alive.clone();
            }
            for one in instances.iter().filter(|one| alive.contains_key(&one.id)) {
                if !app.state::<AppState>().remote_tailing.lock().expect("remote state poisoned").insert(one.id.clone()) {
                    continue;
                }
                let app = app.clone();
                let id = one.id.clone();
                let host = one.remote_host.clone();
                let folder = one.path.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let logs = state.logs.clone();
                    let running = state.remote_running.clone();
                    let mine = id.clone();
                    let app_for_lines = app.clone();
                    let starting = state.remote_starting.clone();
                    let tail_began = std::time::Instant::now();
                    let backlog_is_fresh = starting
                        .lock()
                        .ok()
                        .and_then(|have| have.get(&id).map(|since| since.elapsed() > Duration::from_millis(2500)))
                        .unwrap_or(false);
                    let result = remote::tail_log(
                        &app,
                        &host,
                        &folder,
                        |line| {
                            if is_ready_line(&line) && (backlog_is_fresh || tail_began.elapsed() > Duration::from_millis(1500)) {
                                if let Ok(mut have) = starting.lock() {
                                    have.remove(&mine);
                                }
                            }
                            publish_log(
                                &app_for_lines,
                                &logs,
                                ServerLogEvent {
                                    server_id: mine.clone(),
                                    line,
                                    stream: "stdout".into(),
                                    noise: false,
                                    ansi: String::new(),
                                    by: String::new(),
                                },
                            );
                        },
                        || running.lock().map(|have| have.contains_key(&id)).unwrap_or(false),
                    )
                    .await;
                    if let Err(error) = result {
                        eprintln!("[remote] tail: {error}");
                    }
                    app.state::<AppState>().remote_tailing.lock().expect("remote state poisoned").remove(&id);
                });
            }
        }
    });
}

fn start_lag_watcher(app: AppHandle) {
    struct Watch {
        low_since: Option<u64>,
        last_notified: u64,
        recovered: bool,
    }
    thread::spawn(move || {
        let mut watches: HashMap<String, Watch> = HashMap::new();
        loop {
            thread::sleep(Duration::from_secs(10));
            let state = app.state::<AppState>();
            let running: Vec<String> = state.processes.lock().expect("process state poisoned").keys().cloned().collect();
            watches.retain(|id, _| running.contains(id));
            let now = unix_now();
            for id in running {
                let Ok(instance) = instance_by_id(&app, &id) else { continue };
                let Some(tps) = events::read_live(Path::new(&instance.path)).and_then(|live| live.tps) else { continue };
                let watch = watches.entry(id.clone()).or_insert(Watch { low_since: None, last_notified: 0, recovered: true });
                if tps >= lag::LAGGY_TPS {
                    watch.recovered = true;
                }
                if tps >= LAG_ALERT_TPS {
                    watch.low_since = None;
                    continue;
                }
                let since = *watch.low_since.get_or_insert(now);
                if now.saturating_sub(since) < LAG_ALERT_HOLD_SECS
                    || !watch.recovered
                    || now.saturating_sub(watch.last_notified) < LAG_ALERT_COOLDOWN_SECS
                {
                    continue;
                }
                watch.last_notified = now;
                watch.recovered = false;
                let findings = tauri::async_runtime::block_on(lag_report(&app, &state, &id))
                    .map(|report| report.findings.into_iter().take(3).collect::<Vec<_>>())
                    .unwrap_or_default();
                let payload = serde_json::json!({
                    "serverId": id,
                    "name": instance.name,
                    "tps": tps,
                    "findings": findings,
                });
                remember_for_web(&app, "server-lag", &payload);
                let _ = app.emit("server-lag", payload);
                webhook::send(&app, &id, &instance.name, "lag", &format!("Lagging: {tps:.1} TPS for a minute."));
            }
        }
    });
}

#[tauri::command(async)]
#[specta::specta]
async fn read_server_properties(app: AppHandle, id: String) -> Result<Vec<properties::PropertyInfo>, String> {
    let instance = instance_by_id(&app, &id)?;
    if is_far(&instance) {
        let text = far_read(&app, &instance, "server.properties").await.unwrap_or_default();
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        return Ok(properties::parse(&text).iter().map(|(key, value)| properties::describe(key, value)).collect());
    }
    if pumpkin::is_pumpkin(&instance.core) {
        return pumpkin::describe(Path::new(&instance.path));
    }
    let path = Path::new(&instance.path).join("server.properties");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    Ok(properties::parse(&text).iter().map(|(key, value)| properties::describe(key, value)).collect())
}

#[tauri::command(async)]
#[specta::specta]
async fn save_server_properties(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    properties: Vec<PropertyEntry>,
) -> Result<(), String> {
    let _ = &state;
    let instance = instance_by_id(&app, &id)?;
    if pumpkin::is_pumpkin(&instance.core) {
        let entries: Vec<(String, String)> = properties.into_iter().map(|entry| (entry.key, entry.value)).collect();
        pumpkin::save(Path::new(&instance.path), &entries)?;
        let _store = lock_instance_store();
        let mut instances = load_instances(&app)?;
        if let Some(saved) = instances.iter_mut().find(|saved| saved.id == id) {
            saved.port = port_from_properties(saved);
        }
        save_instances(&app, &instances)?;
        record_audit(&app, &id, "Settings", "Saved pumpkin.toml", "Settings saved!");
        return Ok(());
    }
    if is_far(&instance) {
        let before = properties::parse(&far_read(&app, &instance, "server.properties").await.unwrap_or_default());
        let mut lines = Vec::new();
        for entry in &properties {
            let (key, value) = (entry.key.trim(), entry.value.trim());
            if key.is_empty() {
                continue;
            }
            if properties::needs_check(&before, key, value) {
                properties::validate(key, value)?;
            }
            lines.push(format!("{key}={}", properties::escape(value)));
        }
        lines.sort();
        far_write(&app, &instance, "server.properties", &format!("{}\n", lines.join("\n"))).await?;
        let _store = lock_instance_store();
        let mut instances = load_instances(&app)?;
        if let Some(saved) = instances.iter_mut().find(|saved| saved.id == id) {
            if let Some(port) = properties.iter().find(|one| one.key == "server-port").and_then(|one| one.value.trim().parse().ok()) {
                saved.port = port;
            }
        }
        save_instances(&app, &instances)?;
        record_audit(&app, &id, "Settings", "Save server.properties (SSH)", "Settings saved!");
        return Ok(());
    }
    let _target = Path::new(&instance.path).join("server.properties");
    let before = properties::parse(&fs::read_to_string(Path::new(&instance.path).join("server.properties")).unwrap_or_default());
    for entry in &properties {
        let (key, value) = (entry.key.trim(), entry.value.trim());
        if key.is_empty() || !properties::needs_check(&before, key, value) {
            continue;
        }
        properties::validate(key, value)?;
    }
    let text = properties
        .into_iter()
        .filter(|entry| !entry.key.trim().is_empty())
        .map(|entry| format!("{}={}", entry.key.trim(), properties::escape(entry.value.trim())))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(Path::new(&instance.path).join("server.properties"), text)
        .map_err(|error| format!("Could not save server.properties: {error}"))?;
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    if let Some(saved) = instances.iter_mut().find(|saved| saved.id == id) {
        saved.port = port_from_properties(saved);
    }
    save_instances(&app, &instances)?;
    record_audit(&app, &id, "Settings", "server.properties save", "Settings saved!");
    Ok(())
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct BrowseEntry {
    name: String,
    path: String,
    is_dir: bool,
    size_bytes: u64,
    editable: bool,
    modified_at: u64,
    item_count: u64,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct BrowsePage {
    entries: Vec<BrowseEntry>,
    total: u64,
    offset: u64,
}

const BROWSE_PAGE: usize = 500;

#[tauri::command(async)]
#[specta::specta]
async fn browse_server_files(
    app: AppHandle,
    id: String,
    path: String,
    offset: Option<u32>,
    limit: Option<u32>,
) -> Result<BrowsePage, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        let here = path.trim().trim_matches(['/', '\\']).to_string();
        let rows = pfs::list_dir(&api, &target, &path).await?;
        let total = rows.len() as u64;
        return Ok(BrowsePage {
            entries: rows
                .into_iter()
                .filter(|one| one.name != ".monolab-trash")
                .map(|one| BrowseEntry {
                    path: if here.is_empty() { one.name.clone() } else { format!("{here}/{}", one.name) },
                    editable: !one.directory && editable_file(Path::new(&one.name)),
                    is_dir: one.directory,
                    size_bytes: one.size_bytes,
                    modified_at: one.modified_at,
                    item_count: u64::from(u32::MAX),
                    name: one.name,
                })
                .collect(),
            total,
            offset: 0,
        });
    }
    if !instance.remote_host.is_empty() {
        let here = path.trim().trim_matches(['/', '\\']).to_string();
        let rows = far_list(&app, &instance, &path).await?;
        let total = rows.len() as u64;
        return Ok(BrowsePage {
            entries: rows
                .into_iter()
                .map(|one| BrowseEntry {
                    path: if here.is_empty() { one.name.clone() } else { format!("{here}/{}", one.name) },
                    editable: !one.directory && editable_file(Path::new(&one.name)),
                    is_dir: one.directory,
                    size_bytes: one.size_bytes,
                    modified_at: one.modified_at,
                    item_count: u64::from(u32::MAX),
                    name: one.name,
                })
                .collect(),
            total,
            offset: 0,
        });
    }
    let root = Path::new(&instance.path).canonicalize().map(plain_path).map_err(|error| format!("Server folder not found: {error}"))?;
    let requested = path.trim().trim_matches(['/', '\\']);
    let target = if requested.is_empty() {
        root.clone()
    } else {
        paths::resolve_in_server(&root, requested).map_err(|error| match error {
            paths::PathError::Escape => "Cannot browse outside the server folder.".to_string(),
            other => other.to_string(),
        })?
    };
    browse_dir(&root, &target, offset, limit)
}

fn browse_dir(root: &Path, target: &Path, offset: Option<u32>, limit: Option<u32>) -> Result<BrowsePage, String> {
    if !target.is_dir() {
        return Err("Not a folder.".into());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(target).map_err(|error| error.to_string())? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if name.starts_with(".monolab-") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        entries.push(BrowseEntry {
            name: name.to_string(),
            path: relative,
            is_dir: meta.is_dir(),
            size_bytes: if meta.is_dir() { 0 } else { meta.len() },
            editable: !meta.is_dir() && editable_file(&path),
            modified_at: meta.modified().ok().and_then(|when| when.duration_since(UNIX_EPOCH).ok()).map(|gap| gap.as_secs()).unwrap_or(0),
            item_count: 0,
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    let total = entries.len() as u64;
    let offset = (offset.unwrap_or(0) as usize).min(entries.len());
    let limit = (limit.unwrap_or(BROWSE_PAGE as u32) as usize).clamp(1, BROWSE_PAGE);
    let mut page: Vec<BrowseEntry> = entries.into_iter().skip(offset).take(limit).collect();
    for entry in page.iter_mut().filter(|entry| entry.is_dir) {
        entry.item_count = fs::read_dir(root.join(&entry.path)).map(|list| list.count() as u64).unwrap_or(0);
    }
    Ok(BrowsePage { entries: page, total, offset: offset as u64 })
}

#[cfg(test)]
mod browse_tests {
    #[test]
    fn pages_a_big_folder_and_counts_only_what_it_sends() {
        let dir = std::env::temp_dir().join(format!("se-browse-{}", super::unix_now()));
        std::fs::create_dir_all(dir.join("region")).unwrap();
        std::fs::write(dir.join("region/r.0.0.mca"), b"x").unwrap();
        std::fs::write(dir.join("region/r.0.1.mca"), b"x").unwrap();
        std::fs::create_dir_all(dir.join(".monolab-pending")).unwrap();
        for n in 0..1203 {
            std::fs::write(dir.join(format!("f{n:04}.txt")), b"x").unwrap();
        }
        let first = super::browse_dir(&dir, &dir, None, None).unwrap();
        assert_eq!(first.total, 1204, "one folder + 1203 files, without the hidden folder");
        assert_eq!(first.offset, 0);
        assert_eq!(first.entries.len(), super::BROWSE_PAGE);
        assert!(first.entries[0].is_dir && first.entries[0].name == "region");
        assert_eq!(first.entries[0].item_count, 2);
        assert_eq!(first.entries[1].name, "f0000.txt");
        let last = super::browse_dir(&dir, &dir, Some(1000), None).unwrap();
        assert_eq!(last.entries.len(), 204);
        assert_eq!(last.entries[0].name, "f0999.txt");
        let past = super::browse_dir(&dir, &dir, Some(9999), Some(10)).unwrap();
        assert!(past.entries.is_empty());
        assert_eq!(past.total, 1204);
        assert!(super::browse_dir(&dir, &dir.join("f0000.txt"), None, None).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[tauri::command(async)]
#[specta::specta]
async fn read_editable_file(app: AppHandle, id: String, path: String) -> Result<String, String> {
    let instance = instance_by_id(&app, &id)?;
    let text = if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        api.read_text(&target, &path).await?
    } else if !instance.remote_host.is_empty() {
        far_read(&app, &instance, &path).await?
    } else {
        let path = safe_editable_path(&instance, &path)?;
        let bytes = fs::read(path).map_err(|error| format!("Could not read the text file: {error}"))?;
        String::from_utf8(bytes).map_err(|_| "This file isn't text, so it can't be opened in the editor.".to_string())?
    };
    if looks_binary(&text) {
        return Err("This file isn't text, so it can't be opened in the editor.".into());
    }
    Ok(text)
}

pub(crate) fn image_kind(name: &str) -> Option<&'static str> {
    let extension = name.rsplit('.').next()?.to_ascii_lowercase();
    match extension.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

#[tauri::command(async)]
#[specta::specta]
async fn read_server_image(app: AppHandle, id: String, path: String) -> Result<String, String> {
    use base64::Engine;
    const LIMIT: u64 = 4 * 1024 * 1024;
    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
    let Some(mime) = image_kind(&name) else {
        return Err("This isn't an image file.".into());
    };
    let instance = instance_by_id(&app, &id)?;
    let bytes = if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        api.read_bytes(&target, &path, LIMIT).await?
    } else if instance.remote_host.is_empty() {
        let at = paths::resolve_in_server(Path::new(&instance.path), &path).map_err(|error| match error {
            paths::PathError::Missing => "File not found.".to_string(),
            other => other.to_string(),
        })?;
        if fs::metadata(&at).map_err(|error| error.to_string())?.len() > LIMIT {
            return Err("This image is too large to open in the app (over 4 MB).".into());
        }
        fs::read(&at).map_err(|error| format!("Could not read the image: {error}"))?
    } else {
        remote::read_bytes(&app, &instance.remote_host, &instance.path, &path, LIMIT).await?
    };
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes)))
}

/// Names in a server folder (name, is folder, modified), for local, SSH and panel servers alike.
async fn server_names(app: &AppHandle, instance: &ServerInstance, relative: &str) -> Vec<(String, bool, u64)> {
    if is_far(instance) {
        return far_list(app, instance, relative)
            .await
            .map(|rows| rows.into_iter().map(|row| (row.name, row.directory, row.modified_at)).collect())
            .unwrap_or_default();
    }
    let Ok(dir) = paths::resolve_in_server(Path::new(&instance.path), relative) else {
        return Vec::new();
    };
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|one| {
                    let meta = one.metadata().ok();
                    let modified = meta
                        .as_ref()
                        .and_then(|meta| meta.modified().ok())
                        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                        .map(|time| time.as_secs())
                        .unwrap_or(0);
                    (one.file_name().to_string_lossy().into_owned(), meta.is_some_and(|meta| meta.is_dir()), modified)
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn server_has_dir(app: &AppHandle, instance: &ServerInstance, relative: &str) -> bool {
    let (parent, name) = relative.rsplit_once('/').unwrap_or(("", relative));
    server_names(app, instance, parent).await.iter().any(|(one, dir, _)| *dir && one == name)
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AddonSwitch {
    key: String,
    on: bool,
    /// Takes effect only after the server restarts.
    restart: bool,
}

const ADDON_SWITCHES: &[(&str, bool)] = &[
    ("map", true),
    ("live-map", false),
    ("web-map", true),
    ("inventories", false),
    ("loaded-chunks", false),
    ("commands", false),
    ("performance-log", false),
    ("jvm-stats", false),
    ("studio", false),
];
const PUMPKIN_SWITCHES: &[&str] = &["map", "performance-log", "studio"];

fn addon_config_path(instance: &ServerInstance) -> Option<&'static str> {
    if pumpkin::is_pumpkin(&instance.core) {
        Some("plugins/data/monolab/config.yml")
    } else {
        match kinds::addon_word(&instance.core) {
            "plugin" => Some("plugins/Monolab/config.yml"),
            "mod" => Some("config/monolab/config.yml"),
            _ => None,
        }
    }
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct MsmpState {
    /// This server can have live settings: a Java server on 1.21.9 or newer, on this computer.
    supported: bool,
    /// Why not, when `supported` is false.
    reason: String,
    /// server.properties is set up so Monolab can connect (on, local, fixed port, secret, no TLS).
    ready: bool,
    running: bool,
    /// Connected just now; empty `error` means it worked.
    connected: bool,
    api_version: String,
    error: String,
}

/// Live settings reach this server: on this computer, or an SSH server (through a tunnel). Not panel servers.
fn msmp_reachable(instance: &ServerInstance) -> bool {
    instance.panel_host.is_empty()
}

/// Where the management server is (this computer or through SSH) and its secret, when Monolab can use it.
async fn msmp_target(app: &AppHandle, id: &str) -> Result<(msmp::Link, String), String> {
    let instance = instance_by_id(app, id)?;
    if !msmp_reachable(&instance) {
        return Err("Live settings don't work on panel servers.".into());
    }
    let text = server_text(app, &instance, "server.properties").await?.unwrap_or_default();
    let config = msmp::read_config(&text);
    if !config.ready() {
        return Err("Live settings aren't set up for this server yet.".into());
    }
    let link = if instance.remote_host.is_empty() {
        msmp::Link::Local(config.port)
    } else {
        msmp::Link::Ssh(remote::connect(app, &instance.remote_host).await?, config.port)
    };
    Ok((link, config.secret))
}

#[tauri::command(async)]
#[specta::specta]
async fn msmp_state(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<MsmpState, String> {
    let instance = instance_by_id(&app, &id)?;
    let running = is_running(&state, &id);
    let mut out = MsmpState {
        supported: false,
        reason: String::new(),
        ready: false,
        running,
        connected: false,
        api_version: String::new(),
        error: String::new(),
    };
    if kinds::is_proxy(&instance.core) || pumpkin::is_pumpkin(&instance.core) {
        out.reason = "This server type has no live settings.".into();
        return Ok(out);
    }
    if !msmp::version_has_msmp(&instance.version) {
        out.reason = "Live settings need Minecraft 1.21.9 or newer.".into();
        return Ok(out);
    }
    if !msmp_reachable(&instance) {
        out.reason = "Live settings don't work on panel servers.".into();
        return Ok(out);
    }
    out.supported = true;
    let text = server_text(&app, &instance, "server.properties").await?.unwrap_or_default();
    let config = msmp::read_config(&text);
    out.ready = config.ready();
    if out.ready && running {
        let reached = match msmp_target(&app, &id).await {
            Ok((link, secret)) => msmp::api_version(&link, &secret).await,
            Err(error) => Err(error),
        };
        match reached {
            Ok(version) => {
                out.connected = true;
                out.api_version = version;
            }
            Err(error) => out.error = error,
        }
    }
    Ok(out)
}

/// Turns the management server on in server.properties (local only, no TLS). Applies at the next start.
#[tauri::command(async)]
#[specta::specta]
async fn msmp_setup(app: AppHandle, id: String) -> Result<(), String> {
    let instance = instance_by_id(&app, &id)?;
    if !msmp_reachable(&instance) || !msmp::version_has_msmp(&instance.version) {
        return Err("Live settings can't be set up for this server.".into());
    }
    let text = server_text(&app, &instance, "server.properties").await?.ok_or("Start the server once so it makes server.properties.")?;
    let port = if instance.remote_host.is_empty() {
        std::net::TcpListener::bind(("127.0.0.1", 0))
            .and_then(|listener| listener.local_addr())
            .map(|address| address.port())
            .map_err(|error| format!("Could not find a free port: {error}"))?
    } else {
        // On the SSH machine: a high port picked at random (it only listens on that machine's localhost).
        let mut random = [0u8; 2];
        getrandom::fill(&mut random).map_err(|error| error.to_string())?;
        40_000 + u16::from_le_bytes(random) % 20_000
    };
    let changed = msmp::setup_properties(&text, port, &msmp::new_secret());
    if changed != text {
        server_write_text(&app, &instance, "server.properties", &changed).await?;
    }
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
async fn msmp_settings(app: AppHandle, id: String) -> Result<Vec<msmp::MsmpSetting>, String> {
    let (link, secret) = msmp_target(&app, &id).await?;
    msmp::settings(&link, &secret).await
}

/// `value` is JSON text: true/false, a number, or a quoted word. Returns what the server now has.
#[tauri::command(async)]
#[specta::specta]
async fn msmp_set_setting(app: AppHandle, id: String, key: String, value: String) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(&value).map_err(|_| "That value can't be read.".to_string())?;
    let (link, secret) = msmp_target(&app, &id).await?;
    msmp::set_setting(&link, &secret, &key, value).await.map(|now| now.to_string())
}

#[tauri::command(async)]
#[specta::specta]
async fn msmp_rules(app: AppHandle, id: String) -> Result<Vec<msmp::MsmpRule>, String> {
    let (link, secret) = msmp_target(&app, &id).await?;
    msmp::rules(&link, &secret).await
}

#[tauri::command(async)]
#[specta::specta]
async fn msmp_set_rule(app: AppHandle, id: String, key: String, value: String) -> Result<(), String> {
    let value: serde_json::Value = serde_json::from_str(&value).map_err(|_| "That value can't be read.".to_string())?;
    let (link, secret) = msmp_target(&app, &id).await?;
    let api = msmp::api_version(&link, &secret).await?;
    msmp::set_rule(&link, &secret, &api, &key, value).await
}

/// The studio folder for this server's add-on, or `None` when its add-on has no studio.
fn studio_root(instance: &ServerInstance) -> Option<&'static str> {
    if pumpkin::is_pumpkin(&instance.core) {
        Some(studio::PUMPKIN_ROOT)
    } else if events::addon_kind(&instance.core) == Some(events::AddonKind::Paper) {
        Some(studio::PAPER_ROOT)
    } else {
        None
    }
}

fn studio_supported(instance: &ServerInstance) -> bool {
    studio_root(instance).is_some()
}

/// The kinds this server's add-on runs.
fn studio_kinds(instance: &ServerInstance) -> &'static [&'static str] {
    if pumpkin::is_pumpkin(&instance.core) {
        &studio::PUMPKIN_KINDS
    } else {
        &studio::KINDS
    }
}

/// Where one studio file lives on this server; refuses kinds its add-on doesn't run.
fn studio_path(instance: &ServerInstance, kind: &str, name: &str) -> Result<String, String> {
    let root = studio_root(instance).ok_or("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.")?;
    if studio::valid_kind(kind) && !studio_kinds(instance).contains(&kind) {
        return Err("This server's add-on doesn't run this kind.".into());
    }
    studio::path_of(root, kind, name)
}

async fn server_bytes(app: &AppHandle, instance: &ServerInstance, relative: &str, limit: u64) -> Result<Vec<u8>, String> {
    if is_far(instance) {
        return far_read_bytes(app, instance, relative, limit).await;
    }
    let path = paths::resolve_in_server(Path::new(&instance.path), relative).map_err(|_| format!("Could not read {relative}."))?;
    if fs::metadata(&path).map(|one| one.len()).unwrap_or(0) > limit {
        return Err(format!("{relative} is too big."));
    }
    fs::read(path).map_err(|error| format!("Could not read {relative}: {error}"))
}

async fn server_write_bytes(app: &AppHandle, instance: &ServerInstance, relative: &str, bytes: &[u8]) -> Result<(), String> {
    if is_far(instance) {
        let shown = relative.rsplit('/').next().unwrap_or(relative).to_string();
        return far_upload(app, instance, relative, bytes, &shown).await;
    }
    let root = Path::new(&instance.path);
    let checked = paths::check_relative(relative).map_err(|_| format!("Could not write {relative}."))?;
    if let Some(parent) = checked.parent().filter(|one| !one.as_os_str().is_empty()) {
        fs::create_dir_all(root.join(parent)).map_err(|error| format!("Could not write {relative}: {error}"))?;
    }
    let path = paths::resolve_new_in_server(root, relative).map_err(|_| format!("Could not write {relative}."))?;
    let staging = path.with_extension("monolab-part");
    fs::write(&staging, bytes).map_err(|error| format!("Could not write {relative}: {error}"))?;
    fs::rename(&staging, &path).map_err(|error| format!("Could not write {relative}: {error}"))
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct StudioAsset {
    folder: String,
    file: String,
    name: String,
    modified: u64,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct StudioAssetList {
    assets: Vec<StudioAsset>,
    pack: Option<studio_pack::PackInfo>,
}

async fn studio_asset_rows(app: &AppHandle, instance: &ServerInstance, root: &str) -> Vec<StudioAsset> {
    let mut out = Vec::new();
    for folder in studio_pack::FOLDERS {
        for (file, dir, modified) in server_names(app, instance, &format!("{root}/assets/{folder}")).await {
            if let Some(name) = studio_pack::asset_name(folder, &file).filter(|_| !dir) {
                out.push(StudioAsset { folder: folder.into(), name: name.into(), file, modified });
            }
        }
    }
    out.sort_by(|a, b| (&a.folder, &a.file).cmp(&(&b.folder, &b.file)));
    out
}

/// Every good file of one studio kind, for the pack (items' looks, fonts' glyphs).
async fn studio_kind_files(
    app: &AppHandle,
    instance: &ServerInstance,
    root: &str,
    kind: &str,
) -> Vec<(String, serde_json::Map<String, serde_json::Value>)> {
    let mut out = Vec::new();
    for (file, dir, _) in server_names(app, instance, &format!("{root}/{kind}")).await {
        let Some(id) = file.strip_suffix(".json").filter(|id| !dir && studio::valid_id(id)) else { continue };
        if let Some(text) = server_text(app, instance, &format!("{root}/{kind}/{file}")).await.ok().flatten() {
            if let Ok(map) = studio_format::check(id, &text) {
                out.push((id.to_string(), map));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

async fn studio_pack_info(app: &AppHandle, instance: &ServerInstance, root: &str) -> Option<studio_pack::PackInfo> {
    let text = server_text(app, instance, &format!("{root}/pack/pack.json")).await.ok().flatten()?;
    serde_json::from_str(&text).ok()
}

/// Builds `studio/pack/monolab.zip` from the assets and writes `pack.json` (keeping how it is sent). The add-on
/// notices the new SHA-1 and hands the pack to players again.
async fn studio_pack_write(app: &AppHandle, instance: &ServerInstance, root: &str) -> Result<studio_pack::PackInfo, String> {
    let mut sources = Vec::new();
    for asset in studio_asset_rows(app, instance, root).await {
        let bytes =
            server_bytes(app, instance, &format!("{root}/assets/{}/{}", asset.folder, asset.file), studio_pack::MOST_SOUND as u64).await?;
        sources.push((asset.folder, asset.file, bytes));
    }
    let items = studio_kind_files(app, instance, root, "item").await;
    let fonts = studio_kind_files(app, instance, root, "font").await;
    let models = studio_kind_files(app, instance, root, "model").await;
    let blocks = studio_kind_files(app, instance, root, "block").await;
    let furniture = studio_kind_files(app, instance, root, "furniture").await;
    let paintings = studio_kind_files(app, instance, root, "painting").await;
    let mut ids: studio_pack::Ids = server_text(app, instance, &format!("{root}/pack/ids.json"))
        .await
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    ids.schema = 1;
    let ids_before = serde_json::to_string(&ids).unwrap_or_default();
    // A Pumpkin server's version is Pumpkin's own (0.2.1+26.3-26.51); the pack needs the game's. Pumpkin follows
    // the newest Minecraft, so a build without one counts as the newest.
    let version = if pumpkin::is_pumpkin(&instance.core) {
        Some(pumpkin_game_version(&instance.version)).filter(|one| looks_like_version(one)).unwrap_or_else(|| "26.1".to_string())
    } else {
        instance.version.clone()
    };
    let (zip, mut info, ids, glyphs, rigs) = tauri::async_runtime::spawn_blocking(move || {
        // M1: item models (and armor pictures copied where the game looks for them); M2: glyph fonts.
        let mut extra = studio_pack_items::item_files(&version, &items, &mut ids);
        for (texture, path) in studio_pack_items::item_copies(&version, &items) {
            if let Some((_, _, bytes)) = sources.iter().find(|(folder, file, _)| folder == "textures" && *file == format!("{texture}.png"))
            {
                extra.push((path, bytes.clone()));
            }
        }
        // Paintings: their picture where the game's painting atlas looks.
        for (id, data) in paintings.iter().filter(|(_, data)| studio_format::enabled(data)) {
            let texture = studio_format::painting::texture(data);
            if let Some((_, _, bytes)) = sources.iter().find(|(folder, file, _)| folder == "textures" && *file == format!("{texture}.png"))
            {
                extra.push((studio_format::painting::pack_path(id), bytes.clone()));
            }
        }
        // M3: borrowed block states and furniture looks (old-version overrides join M1's on the same base item).
        let more = studio_pack_blocks::block_files(&version, &blocks, &furniture, &mut ids);
        studio_pack_blocks::merge(&mut extra, more);
        let (table, fonts): (Vec<_>, Vec<_>) = studio_pack_fonts::font_files(&fonts, &sources, &mut ids)
            .into_iter()
            .partition(|(path, _)| path == studio_pack_fonts::TABLE_FILE);
        extra.extend(fonts);
        // M4: one item model per bone, and a rig file per model that the add-ons step through.
        let modeled = studio_pack_models::model_files(&version, &models, &sources);
        extra.extend(modeled.files);
        let (zip, info) = studio_pack::build(&version, &sources, &extra)?;
        Ok::<_, String>((zip, info, ids, table.into_iter().next().map(|(_, bytes)| bytes), modeled.rigs))
    })
    .await
    .map_err(|error| error.to_string())??;
    let ids_text = serde_json::to_string_pretty(&ids).map_err(|error| error.to_string())?;
    if serde_json::to_string(&ids).unwrap_or_default() != ids_before {
        server_write_text(app, instance, &format!("{root}/pack/ids.json"), &ids_text).await?;
    }
    if let Some(glyphs) = glyphs {
        server_write_bytes(app, instance, &format!("{root}/pack/glyphs.json"), &glyphs).await?;
    }
    let rig_folder = format!("{root}/{}", studio_pack_models::RIG_FOLDER);
    for (id, bytes) in &rigs {
        server_write_bytes(app, instance, &format!("{rig_folder}/{id}.json"), bytes).await?;
    }
    for (file, dir, _) in server_names(app, instance, &rig_folder).await {
        if !dir && file.ends_with(".json") && !rigs.iter().any(|(id, _)| format!("{id}.json") == file) {
            let _ = server_remove_file(app, instance, &format!("{rig_folder}/{file}")).await;
        }
    }
    let before = studio_pack_info(app, instance, root).await;
    info.sending = before.as_ref().map(|one| one.sending.clone()).unwrap_or_default();
    info.built =
        if before.as_ref().is_some_and(|one| one.sha1 == info.sha1) { before.map(|one| one.built).unwrap_or_default() } else { unix_now() };
    server_write_bytes(app, instance, &format!("{root}/pack/monolab.zip"), &zip).await?;
    let text = serde_json::to_string_pretty(&info).map_err(|error| error.to_string())?;
    server_write_text(app, instance, &format!("{root}/pack/pack.json"), &text).await?;
    Ok(info)
}

/// The studio's assets and its resource pack.
#[tauri::command(async)]
#[specta::specta]
async fn studio_asset_list(app: AppHandle, id: String) -> Result<StudioAssetList, String> {
    let instance = instance_by_id(&app, &id)?;
    let root = studio_root(&instance).ok_or("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.")?;
    Ok(StudioAssetList { assets: studio_asset_rows(&app, &instance, root).await, pack: studio_pack_info(&app, &instance, root).await })
}

/// Adds (or replaces) an asset, base64, then rebuilds the pack.
#[tauri::command(async)]
#[specta::specta]
async fn studio_asset_add(app: AppHandle, id: String, folder: String, file: String, data: String) -> Result<studio_pack::PackInfo, String> {
    note_writes(&app, &id, "studio_asset_add_upload", &serde_json::json!({ "folder": folder, "file": file }), "host");
    use base64::Engine;
    let instance = instance_by_id(&app, &id)?;
    let root = studio_root(&instance).ok_or("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|_| "The file didn't arrive whole. Try again.".to_string())?;
    studio_pack::check_upload(&folder, &file, &bytes)?;
    server_write_bytes(&app, &instance, &format!("{root}/assets/{folder}/{file}"), &bytes).await?;
    studio_pack_write(&app, &instance, root).await
}

/// `studio_asset_add` for the web panel and teammates: the file came in pieces (`__upload_*`) and is named by its upload.
#[tauri::command(async)]
#[specta::specta]
async fn studio_asset_add_upload(
    app: AppHandle,
    id: String,
    folder: String,
    file: String,
    upload: String,
) -> Result<studio_pack::PackInfo, String> {
    note_writes(&app, &id, "studio_asset_add_upload", &serde_json::json!({ "folder": folder, "file": file }), "host");
    let path = remote_upload_path(&app, &upload)?;
    let bytes = fs::read(&path).map_err(|error| format!("Could not read the upload: {error}"));
    unstash(Path::new(&path));
    let bytes = bytes?;
    let instance = instance_by_id(&app, &id)?;
    let root = studio_root(&instance).ok_or("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.")?;
    studio_pack::check_upload(&folder, &file, &bytes)?;
    server_write_bytes(&app, &instance, &format!("{root}/assets/{folder}/{file}"), &bytes).await?;
    let info = studio_pack_write(&app, &instance, root).await?;
    monolink_team::studio_asset_added(&app, &id, &file);
    team_notice::send(&app, &id, team_notice::NoticeKind::AssetAdded, None, &file);
    Ok(info)
}

#[tauri::command(async)]
#[specta::specta]
async fn studio_asset_remove(app: AppHandle, id: String, folder: String, file: String) -> Result<studio_pack::PackInfo, String> {
    note_writes(&app, &id, "studio_asset_remove", &serde_json::json!({ "folder": folder, "file": file }), "host");
    let instance = instance_by_id(&app, &id)?;
    let root = studio_root(&instance).ok_or("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.")?;
    if studio_pack::asset_name(&folder, &file).is_none() {
        return Err("There is no such asset.".into());
    }
    server_remove_file(&app, &instance, &format!("{root}/assets/{folder}/{file}")).await?;
    studio_pack_write(&app, &instance, root).await
}

/// A picture asset as a data URL, for the library's thumbnails.
#[tauri::command(async)]
#[specta::specta]
async fn studio_asset_image(app: AppHandle, id: String, file: String) -> Result<String, String> {
    use base64::Engine;
    let instance = instance_by_id(&app, &id)?;
    let root = studio_root(&instance).ok_or("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.")?;
    if studio_pack::asset_name("textures", &file).is_none() || !file.ends_with(".png") {
        return Err("There is no such picture.".into());
    }
    let bytes = server_bytes(&app, &instance, &format!("{root}/assets/textures/{file}"), studio_pack::MOST_TEXTURE as u64).await?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[tauri::command(async)]
#[specta::specta]
async fn studio_pack_build(app: AppHandle, id: String) -> Result<studio_pack::PackInfo, String> {
    let instance = instance_by_id(&app, &id)?;
    let root = studio_root(&instance).ok_or("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.")?;
    studio_pack_write(&app, &instance, root).await
}

/// How the add-on sends the pack (`pack.json`'s `sending`); builds the pack first if there is none.
#[tauri::command(async)]
#[specta::specta]
async fn studio_pack_sending(app: AppHandle, id: String, sending: studio_pack::Sending) -> Result<studio_pack::PackInfo, String> {
    if !matches!(sending.mode.as_str(), "self" | "url" | "off") {
        return Err("Send the pack with the add-on, from an address, or not at all.".into());
    }
    if sending.mode == "url" && !(sending.address.starts_with("https://") || sending.address.starts_with("http://")) {
        return Err("The pack address must start with https:// or http://.".into());
    }
    if sending.mode == "self" && sending.port < 1024 {
        return Err("Pick a port from 1024 up.".into());
    }
    let instance = instance_by_id(&app, &id)?;
    let root = studio_root(&instance).ok_or("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.")?;
    let mut info = match studio_pack_info(&app, &instance, root).await {
        Some(info) => info,
        None => studio_pack_write(&app, &instance, root).await?,
    };
    info.sending = sending;
    let text = serde_json::to_string_pretty(&info).map_err(|error| error.to_string())?;
    server_write_text(&app, &instance, &format!("{root}/pack/pack.json"), &text).await?;
    Ok(info)
}

/// Keeps `studio/AGENTS.md` (how to write the files, for AI agents) current. Best effort: a failure changes nothing.
async fn studio_guide(app: &AppHandle, instance: &ServerInstance, root: &str) {
    let path = format!("{root}/AGENTS.md");
    let next = studio::guide(root, studio_kinds(instance));
    let Ok(now) = server_text(app, instance, &path).await else { return };
    if studio::guide_update(now.as_deref(), &next) {
        let _ = server_write_text(app, instance, &path, &next).await;
    }
}

/// The studio files on this server and what the add-on said about them.
#[tauri::command(async)]
#[specta::specta]
async fn studio_overview(app: AppHandle, id: String) -> Result<studio::StudioOverview, String> {
    let instance = instance_by_id(&app, &id)?;
    let Some(root) = studio_root(&instance) else { return Ok(studio::overview(None, &[], None, Vec::new())) };
    let status = server_text(&app, &instance, &format!("{root}/status.json")).await.ok().flatten();
    let mut files = Vec::new();
    for kind in studio::KINDS {
        for (name, dir, modified) in server_names(&app, &instance, &format!("{root}/{kind}")).await {
            match name.strip_suffix(".json") {
                Some(stem) if !dir && studio::valid_id(stem) => files.push((kind.to_string(), stem.to_string(), modified)),
                _ => {}
            }
        }
    }
    if status.is_some() || !files.is_empty() {
        studio_guide(&app, &instance, root).await;
    }
    let mut overview = studio::overview(Some(root), studio_kinds(&instance), status.as_deref(), files);
    if let Some(path) = addon_config_path(&instance) {
        let config = integrations::parse(&server_text(&app, &instance, path).await.ok().flatten().unwrap_or_default());
        overview.off = studio::KINDS
            .iter()
            .filter(|kind| config.text(&format!("studio-{kind}")).as_deref() == Some("false"))
            .map(|kind| kind.to_string())
            .collect();
    }
    let names: Vec<String> =
        server_names(&app, &instance, "plugins").await.into_iter().filter(|(_, dir, _)| !dir).map(|(name, _, _)| name).collect();
    // On this PC, each jar's plugin.yml name too (a jar renamed on download is still found); over SSH only file names.
    let jars: Vec<(String, Option<String>)> = if is_far(&instance) {
        names.into_iter().map(|name| (name, None)).collect()
    } else {
        let folder = Path::new(&instance.path).join("plugins");
        tauri::async_runtime::spawn_blocking(move || {
            names
                .into_iter()
                .map(|name| {
                    let declared = name.to_ascii_lowercase().ends_with(".jar").then(|| addons::jar_identity(&folder.join(&name)).1);
                    (name, declared)
                })
                .collect()
        })
        .await
        .unwrap_or_default()
    };
    overview.overlaps = studio::overlaps(&jars);
    // The Pumpkin add-on registers advancements itself now: an older app's data pack for them is taken out.
    if pumpkin::is_pumpkin(&instance.core) {
        let _ = pumpkin_old_advancement_pack(&app, &instance).await;
    }
    Ok(overview)
}

/// Where the studio advancement data pack goes on a Pumpkin server.
fn pumpkin_pack_dir(instance: &ServerInstance) -> String {
    let level = if instance.remote_host.is_empty() && instance.panel_host.is_empty() {
        pumpkin::level_name(Path::new(&instance.path))
    } else {
        "world".to_string()
    };
    format!("{level}/datapacks/monolab-studio")
}

/// Takes out the data pack older apps wrote studio advancements into on Pumpkin (`manifest.txt` marks ours); the
/// add-on now registers them with the server (`register-advancement`) and turns that pack off.
async fn pumpkin_old_advancement_pack(app: &AppHandle, instance: &ServerInstance) -> Result<(), String> {
    let pack = pumpkin_pack_dir(instance);
    if server_text(app, instance, &format!("{pack}/manifest.txt")).await.ok().flatten().is_none() {
        return Ok(());
    }
    let folder = format!("{pack}/data/{}/advancement", events::PUMPKIN_PLUGIN_NAME);
    for (name, dir, _) in server_names(app, instance, &folder).await {
        if !dir && name.ends_with(".json") {
            server_remove_file(app, instance, &format!("{folder}/{name}")).await?;
        }
    }
    server_remove_file(app, instance, &format!("{pack}/pack.mcmeta")).await?;
    server_remove_file(app, instance, &format!("{pack}/manifest.txt")).await
}

/// Removes one file the app made on the server (local: deleted; SSH and panel: moved to their trash).
async fn server_remove_file(app: &AppHandle, instance: &ServerInstance, relative: &str) -> Result<(), String> {
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(app, instance)?;
        return pfs::trash_path(&api, &target, relative).await;
    }
    if !instance.remote_host.is_empty() {
        return remote::trash_path(app, &instance.remote_host, &instance.path, relative).await;
    }
    let path = Path::new(&instance.path).join(relative);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not remove {relative}: {error}")),
    }
}

#[tauri::command(async)]
#[specta::specta]
async fn studio_read(app: AppHandle, id: String, kind: String, name: String) -> Result<studio::StudioFile, String> {
    let instance = instance_by_id(&app, &id)?;
    let root = studio_root(&instance).ok_or("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.")?;
    // Reading works for every kind, so a file made on Paper still opens after moving the server.
    let path = studio::path_of(root, &kind, &name)?;
    let text = server_text(&app, &instance, &path).await?.ok_or("This file is gone. It may have been deleted elsewhere.")?;
    Ok(studio::StudioFile { hash: studio::hash(&text), text })
}

/// Saves one studio file. `hash` is what it looked like when opened (empty for a new file), so changes made
/// elsewhere in the meantime aren't overwritten. Returns the new hash.
#[tauri::command(async)]
#[specta::specta]
async fn studio_save(
    app: AppHandle,
    id: String,
    kind: String,
    name: String,
    text: String,
    hash: String,
) -> Result<studio::StudioSaved, String> {
    note_writes(&app, &id, "studio_save", &serde_json::json!({ "kind": kind, "name": name }), "host");
    let instance = instance_by_id(&app, &id)?;
    {
        let (app, instance) = (app.clone(), instance.clone());
        tauri::async_runtime::spawn_blocking(move || studio_assets_ready(&app, &instance)).await.map_err(|error| error.to_string())??;
    }
    let path = studio_path(&instance, &kind, &name)?;
    if let Some(problem) = studio::check(&name, &text) {
        return Err(problem);
    }
    if let Some(root) = studio_root(&instance) {
        studio_guide(&app, &instance, root).await;
    }
    match server_text(&app, &instance, &path).await? {
        Some(_) if hash.is_empty() => return Err("A file with this name already exists.".into()),
        Some(now) if studio::hash(&now) != hash => {
            return Err("This file changed elsewhere since you opened it. Reload it, then save again.".into())
        }
        None if !hash.is_empty() => return Err("This file is gone. It may have been deleted elsewhere.".into()),
        _ => {}
    }
    if pumpkin::is_pumpkin(&instance.core) {
        server_write_text(&app, &instance, &path, &text).await?;
        if kind == "world" {
            studio_main_world(&app, &instance).await?;
        }
        let applied = pumpkin_reload(&app, &instance).await;
        return Ok(studio::StudioSaved { hash: studio::hash(&text), applied });
    }
    server_write_text(&app, &instance, &path, &text).await?;
    if kind == "world" {
        studio_main_world(&app, &instance).await?;
    }
    // Items' looks and fonts live in the resource pack: build it again so the add-on sends the new one.
    if matches!(kind.as_str(), "item" | "font" | "model" | "block" | "furniture") {
        if let Some(root) = studio_root(&instance) {
            studio_pack_write(&app, &instance, root).await?;
        }
    }
    // A running server on this PC or over SSH rereads at once over the live line; any trouble there just leaves it
    // to the file watch.
    let apply = async {
        let (line, token, _conn) = studio_line(&app, &instance).await?;
        studio::reload_on(line, &token).await
    };
    let applied = matches!(tokio::time::timeout(std::time::Duration::from_secs(6), apply).await, Ok(Ok(_)));
    Ok(studio::StudioSaved { hash: studio::hash(&text), applied })
}

/// Points the server's main world at the custom world file marked "main" (or takes Monolab's generator back out):
/// `bukkit.yml` `worlds.<level-name>.generator` on Paper, `pumpkin.toml` `[world.generators]` on Pumpkin. The
/// server reads it when it starts; only chunks it hasn't made yet use the generator.
async fn studio_main_world(app: &AppHandle, instance: &ServerInstance) -> Result<(), String> {
    let Some(root) = studio_root(instance) else { return Ok(()) };
    let files = studio_kind_files(app, instance, root, "world").await;
    let main = studio::main_world(&files).map(str::to_string);
    let (file, before, after) = if pumpkin::is_pumpkin(&instance.core) {
        let Some(before) = server_text(app, instance, pumpkin::CONFIG).await? else {
            if main.is_some() {
                return Err(
                    "The file was saved, but pumpkin.toml is missing. Start the server once, then save again to use it for the main world."
                        .into(),
                );
            }
            return Ok(());
        };
        let after = pumpkin::set_world_generator(&before, main.as_deref())?;
        (pumpkin::CONFIG, before, after)
    } else {
        let level = server_text(app, instance, "server.properties")
            .await?
            .and_then(|text| text.lines().find_map(|line| line.trim().strip_prefix("level-name=")).map(|value| value.trim().to_string()))
            .filter(|one| !one.is_empty())
            .unwrap_or_else(|| "world".into());
        let before = server_text(app, instance, "bukkit.yml").await?.unwrap_or_default();
        let after = studio::bukkit_generator(&before, &level, main.as_deref());
        ("bukkit.yml", before, after)
    };
    if after != before {
        server_write_text(app, instance, file, &after).await?;
        let what = match &main {
            Some(id) => format!("The main world uses the studio world {id} from the next start"),
            None => "The main world uses the server's own generator from the next start".into(),
        };
        record_audit(app, &instance.id, "Studio", "Main world generator", &what);
    }
    Ok(())
}

/// Pumpkin's add-on can't open a socket, so the app leaves `studio/live/reload` and waits (up to 2.5 seconds)
/// for status.json to be rewritten. True when the running server took it.
async fn pumpkin_reload(app: &AppHandle, instance: &ServerInstance) -> bool {
    let root = studio::PUMPKIN_ROOT;
    let at = |text: Option<String>| {
        text.and_then(|one| serde_json::from_str::<serde_json::Value>(&one).ok())
            .and_then(|one| one.get("at").and_then(serde_json::Value::as_f64))
            .unwrap_or(0.0)
    };
    let before = at(server_text(app, instance, &format!("{root}/status.json")).await.ok().flatten());
    if server_write_text(app, instance, &format!("{root}/live/reload"), "").await.is_err() {
        return false;
    }
    for _ in 0..10 {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        if at(server_text(app, instance, &format!("{root}/status.json")).await.ok().flatten()) > before {
            return true;
        }
    }
    false
}

/// The studio live line of a server, with its token. On this PC it's a plain socket; on an SSH server it's a
/// tunnel to that machine's 127.0.0.1, so the add-on never opens a port to the outside. Panel servers have none.
/// The SSH connection comes along so the tunnel outlives a reconnect elsewhere.
async fn studio_line(
    app: &AppHandle,
    instance: &ServerInstance,
) -> Result<(Box<dyn studio::Line>, String, Option<Arc<remote::Conn>>), String> {
    if !instance.panel_host.is_empty() {
        return Err("Panel servers have no live line.".into());
    }
    let root = studio_root(instance).ok_or("This server has no studio.")?;
    let live = server_text(app, instance, &format!("{root}/live.json")).await?.ok_or("The add-on isn't listening.")?;
    let live = studio::read_live(&live)?;
    if instance.remote_host.is_empty() {
        return Ok((Box::new(studio::local_line(live.port).await?), live.token, None));
    }
    let conn = remote::connect(app, &instance.remote_host).await?;
    let tunnel = tokio::time::timeout(std::time::Duration::from_secs(4), conn.forward("127.0.0.1", live.port))
        .await
        .map_err(|_| "The SSH server didn't open a tunnel.".to_string())??;
    Ok((Box::new(tunnel), live.token, Some(conn)))
}

/// One open live line per server, shared by everyone watching it: the app's own studio screens (counted)
/// and web panel or Monolink guests (who only keep it open; it ends when the line closes).
struct StudioWatch {
    stop: Arc<std::sync::atomic::AtomicBool>,
    here: usize,
    guests: bool,
}

fn studio_watchers() -> &'static std::sync::Mutex<HashMap<String, StudioWatch>> {
    static ALL: std::sync::OnceLock<std::sync::Mutex<HashMap<String, StudioWatch>>> = std::sync::OnceLock::new();
    ALL.get_or_init(Default::default)
}

#[derive(Serialize, Clone, specta::Type)]
#[serde(rename_all = "camelCase")]
struct StudioLiveEvent {
    /// `serverId` so web panel and Monolink guests get only their own servers' lines.
    server_id: String,
    /// One message from the add-on as JSON text: `player` (join/leave) or `trace` (a rule ran).
    data: String,
}

/// Listens on a running server's studio live line (this PC or SSH) and passes joins and rule traces on as
/// `studio-live` events, until `studio_unwatch` or the line closes (then `studio-live` comes with `data` empty).
#[tauri::command]
#[specta::specta]
async fn studio_watch(app: AppHandle, id: String) -> Result<bool, String> {
    studio_watch_for(app, id, false).await
}

/// `guest`: asked from the web panel or Monolink. A guest can't close a line others use, so theirs just stays
/// open until the server's line closes.
async fn studio_watch_for(app: AppHandle, id: String, guest: bool) -> Result<bool, String> {
    let instance = instance_by_id(&app, &id)?;
    if !studio_supported(&instance) || !instance.panel_host.is_empty() {
        return Ok(false);
    }
    let join = |all: &mut HashMap<String, StudioWatch>| match all.get_mut(&id) {
        Some(open) => {
            if guest {
                open.guests = true;
            } else {
                open.here += 1;
            }
            true
        }
        None => false,
    };
    if join(&mut studio_watchers().lock().unwrap_or_else(|poison| poison.into_inner())) {
        return Ok(true);
    }
    if pumpkin::is_pumpkin(&instance.core) {
        return Ok(studio_watch_pumpkin(app, instance, id, guest));
    }
    let Ok((line, token, conn)) = studio_line(&app, &instance).await else { return Ok(false) };
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut all = studio_watchers().lock().unwrap_or_else(|poison| poison.into_inner());
        // Someone else opened it meanwhile: use theirs.
        if join(&mut all) {
            return Ok(true);
        }
        all.insert(id.clone(), StudioWatch { stop: stop.clone(), here: usize::from(!guest), guests: guest });
    }
    tauri::async_runtime::spawn(async move {
        let tell = |data: String| {
            let event = StudioLiveEvent { server_id: id.clone(), data };
            remember_for_web(&app, "studio-live", &event);
            let _ = app.emit("studio-live", event);
        };
        let _ = studio::watch_on(line, &token, &stop, |message| tell(message.to_string())).await;
        drop(conn);
        let mut all = studio_watchers().lock().unwrap_or_else(|poison| poison.into_inner());
        if all.get(&id).is_some_and(|now| Arc::ptr_eq(&now.stop, &stop)) {
            all.remove(&id);
            drop(all);
            tell(String::new());
        }
    });
    Ok(true)
}

/// Pumpkin's "live line": the add-on appends rule traces to `studio/live/out.jsonl`, and this reads the new ones
/// every second and passes them on like the socket line does.
fn studio_watch_pumpkin(app: AppHandle, instance: ServerInstance, id: String, guest: bool) -> bool {
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut all = studio_watchers().lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(open) = all.get_mut(&id) {
            if guest {
                open.guests = true;
            } else {
                open.here += 1;
            }
            return true;
        }
        all.insert(id.clone(), StudioWatch { stop: stop.clone(), here: usize::from(!guest), guests: guest });
    }
    tauri::async_runtime::spawn(async move {
        let path = format!("{}/live/out.jsonl", studio::PUMPKIN_ROOT);
        let stamp = |line: &str| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|one| one.get("at").and_then(serde_json::Value::as_f64))
                .unwrap_or(0.0)
        };
        // Only what happens from now on.
        let mut seen = server_text(&app, &instance, &path).await.ok().flatten().unwrap_or_default().lines().map(stamp).fold(0.0, f64::max);
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let Ok(Some(text)) = server_text(&app, &instance, &path).await else { continue };
            for line in text.lines() {
                let at = stamp(line);
                if at > seen {
                    seen = at;
                    let event = StudioLiveEvent { server_id: id.clone(), data: line.to_string() };
                    remember_for_web(&app, "studio-live", &event);
                    let _ = app.emit("studio-live", event);
                }
            }
        }
    });
    true
}

#[tauri::command(async)]
#[specta::specta]
fn studio_unwatch(id: String) {
    let mut all = studio_watchers().lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(open) = all.get_mut(&id) {
        open.here = open.here.saturating_sub(1);
        if open.here == 0 && !open.guests {
            open.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            all.remove(&id);
        }
    }
}

/// Brings a `.schem` or `.nbt` file in as studio structure `name`: its block list goes to `structures/<name>.json`
/// and the `structure/<name>.json` file is made (or its size updated). Returns its size and how many blocks it has.
#[tauri::command(async)]
#[specta::specta]
async fn studio_structure_import(
    app: AppHandle,
    id: String,
    name: String,
    file: String,
    data: String,
) -> Result<StudioStructureInfo, String> {
    use base64::Engine;
    note_writes(&app, &id, "studio_structure_import", &serde_json::json!({ "name": name, "file": file }), "host");
    let instance = instance_by_id(&app, &id)?;
    let root = studio_root(&instance).ok_or("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.")?;
    if !studio::valid_id(&name) {
        return Err("Names may only use a-z, 0-9, - and _ (up to 48).".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|_| "The file didn't arrive whole. Try again.".to_string())?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("The file is over 64 MB.".into());
    }
    let structure = tauri::async_runtime::spawn_blocking(move || studio_structure::read(bytes, &file).map(|one| (one, file)))
        .await
        .map_err(|error| error.to_string())??;
    let (structure, file) = structure;
    server_write_text(&app, &instance, &format!("{root}/{}", studio_format::structure::data_path(&name)), &structure.to_json()).await?;
    let path = studio::path_of(root, "structure", &name)?;
    let mut map: serde_json::Map<String, serde_json::Value> = match server_text(&app, &instance, &path).await? {
        Some(text) => serde_json::from_str(&text).unwrap_or_default(),
        None => serde_json::from_str(&studio::template("structure", &name)).unwrap_or_default(),
    };
    map.insert("source".into(), serde_json::json!(file));
    map.insert("size".into(), serde_json::json!([structure.size.0, structure.size.1, structure.size.2]));
    map.insert("blocks".into(), serde_json::json!(structure.blocks.len()));
    let text = format!("{}\n", serde_json::to_string_pretty(&map).map_err(|error| error.to_string())?);
    server_write_text(&app, &instance, &path, &text).await?;
    if pumpkin::is_pumpkin(&instance.core) {
        pumpkin_reload(&app, &instance).await;
    } else {
        let apply = async {
            let (line, token, _conn) = studio_line(&app, &instance).await?;
            studio::reload_on(line, &token).await
        };
        let _ = tokio::time::timeout(std::time::Duration::from_secs(6), apply).await;
    }
    Ok(StudioStructureInfo { blocks: structure.blocks.len() as u32, size: [structure.size.0, structure.size.1, structure.size.2] })
}

#[derive(Serialize, specta::Type)]
struct StudioStructureInfo {
    blocks: u32,
    size: [i32; 3],
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct StudioSyncTarget {
    id: String,
    name: String,
    core: String,
    /// Behind the same Velocity proxy as this server (listed in its velocity.toml).
    network: bool,
}

/// The servers this one's studio can be copied to: every other server with a studio, the ones behind the same
/// Velocity proxy first.
#[tauri::command(async)]
#[specta::specta]
async fn studio_sync_targets(app: AppHandle, id: String) -> Result<Vec<StudioSyncTarget>, String> {
    let instance = instance_by_id(&app, &id)?;
    let everyone = load_instances(&app)?;
    // Servers listed by each Velocity proxy that lists this one.
    let mut network: Vec<String> = Vec::new();
    for proxy in everyone.iter().filter(|one| one.core == "Velocity" && one.panel_host.is_empty()) {
        let Some(text) = server_text(&app, proxy, "velocity.toml").await.ok().flatten() else { continue };
        let Ok(config) = proxy::read_velocity(&text) else { continue };
        let listed = |one: &ServerInstance| {
            let port = backend_port(one);
            backend_address(&app, proxy, one).is_ok_and(|address| config.servers.iter().any(|(_, at)| same_backend(at, &address, port)))
        };
        if listed(&instance) {
            network.extend(everyone.iter().filter(|one| one.id != instance.id && listed(one)).map(|one| one.id.clone()));
        }
    }
    let mut out: Vec<StudioSyncTarget> = everyone
        .into_iter()
        .filter(|one| one.id != instance.id && studio_supported(one))
        .map(|one| StudioSyncTarget { network: network.contains(&one.id), id: one.id, name: one.name, core: one.core })
        .collect();
    out.sort_by(|a, b| b.network.cmp(&a.network).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

/// Copies this server's whole studio (every file, the assets and the pack numbers in `pack/ids.json`) to other
/// servers, such as the others behind the same Velocity proxy, so players find the same items, blocks and pack on
/// each. `mirror` also takes out studio files and assets the others have that this one doesn't. Runtime data
/// (`data/`, `live/`, `status.json`) stays on each server. Returns one line per server.
#[tauri::command(async)]
#[specta::specta]
async fn studio_sync(app: AppHandle, id: String, targets: Vec<String>, mirror: bool) -> Result<Vec<String>, String> {
    let instance = instance_by_id(&app, &id)?;
    let root = studio_root(&instance).ok_or("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.")?;
    let mut lines = Vec::new();
    for target in targets.iter().filter(|one| **one != id) {
        let Ok(to) = instance_by_id(&app, target) else { continue };
        note_writes(&app, &to.id, "studio_sync", &serde_json::json!({ "from": instance.id, "mirror": mirror }), "host");
        lines.push(match studio_copy_to(&app, &instance, root, &to, mirror).await {
            Ok(line) => line,
            Err(error) => format!("{}: {error}", to.name),
        });
    }
    Ok(lines)
}

async fn studio_copy_to(app: &AppHandle, from: &ServerInstance, root: &str, to: &ServerInstance, mirror: bool) -> Result<String, String> {
    let to_root = studio_root(to).ok_or("It has no studio (Paper, Purpur, Folia, Leaf or Pumpkin only).")?;
    let (mut copied, mut removed) = (0usize, 0usize);
    for kind in studio::KINDS {
        let names: Vec<String> = server_names(app, from, &format!("{root}/{kind}"))
            .await
            .into_iter()
            .filter(|(name, dir, _)| !dir && name.strip_suffix(".json").is_some_and(studio::valid_id))
            .map(|(name, _, _)| name)
            .collect();
        for name in &names {
            if let Some(text) = server_text(app, from, &format!("{root}/{kind}/{name}")).await? {
                server_write_text(app, to, &format!("{to_root}/{kind}/{name}"), &text).await?;
                copied += 1;
            }
        }
        if mirror {
            for (name, dir, _) in server_names(app, to, &format!("{to_root}/{kind}")).await {
                if !dir && name.strip_suffix(".json").is_some_and(studio::valid_id) && !names.contains(&name) {
                    server_remove_file(app, to, &format!("{to_root}/{kind}/{name}")).await?;
                    removed += 1;
                }
            }
        }
    }
    let assets = studio_asset_rows(app, from, root).await;
    for asset in &assets {
        let bytes =
            server_bytes(app, from, &format!("{root}/assets/{}/{}", asset.folder, asset.file), studio_pack::MOST_SOUND as u64).await?;
        server_write_bytes(app, to, &format!("{to_root}/assets/{}/{}", asset.folder, asset.file), &bytes).await?;
        copied += 1;
    }
    if mirror {
        for asset in studio_asset_rows(app, to, to_root).await {
            if !assets.iter().any(|one| one.folder == asset.folder && one.file == asset.file) {
                server_remove_file(app, to, &format!("{to_root}/assets/{}/{}", asset.folder, asset.file)).await?;
                removed += 1;
            }
        }
    }
    // The same numbers, so the same item looks and borrowed block states on every server.
    if let Some(ids) = server_text(app, from, &format!("{root}/pack/ids.json")).await? {
        server_write_text(app, to, &format!("{to_root}/pack/ids.json"), &ids).await?;
    }
    studio_main_world(app, to).await?;
    studio_pack_write(app, to, to_root).await?;
    let applied = if pumpkin::is_pumpkin(&to.core) {
        pumpkin_reload(app, to).await
    } else {
        let apply = async {
            let (line, token, _conn) = studio_line(app, to).await?;
            studio::reload_on(line, &token).await
        };
        matches!(tokio::time::timeout(std::time::Duration::from_secs(6), apply).await, Ok(Ok(_)))
    };
    let mut line = format!("{}: copied {copied} files", to.name);
    if removed > 0 {
        line.push_str(&format!(", removed {removed}"));
    }
    line.push_str(if applied { ", running with them now." } else { ". It picks them up when it next runs." });
    Ok(line)
}

/// Asks the server's add-on for `player`'s main-hand item and returns it as pretty item JSON (all components).
#[tauri::command(async)]
#[specta::specta]
async fn studio_held_item(app: AppHandle, id: String, player: String) -> Result<String, String> {
    let instance = instance_by_id(&app, &id)?;
    studio_held_ask(&app, &instance, studio_held::ASK_FILE, |nonce| Ok(serde_json::json!({ "player": player, "nonce": nonce }))).await
}

/// Puts `item` (item JSON) in `player`'s main hand and returns the item the game made of it.
#[tauri::command(async)]
#[specta::specta]
async fn studio_set_held_item(app: AppHandle, id: String, player: String, item: String) -> Result<String, String> {
    let instance = instance_by_id(&app, &id)?;
    let pumpkin = pumpkin::is_pumpkin(&instance.core);
    studio_held_ask(&app, &instance, studio_held::SET_FILE, |nonce| studio_held::set_request(&player, nonce, &item, pumpkin)).await
}

async fn studio_held_ask(
    app: &AppHandle,
    instance: &ServerInstance,
    file: &str,
    request: impl FnOnce(u64) -> Result<serde_json::Value, String>,
) -> Result<String, String> {
    let value = studio_live_ask(app, instance, file, studio_held::ANSWER_FILE, request).await?;
    studio_held::read_answer(&value)
}

/// Leaves a request in the studio's `live/` folder and waits (up to 6 seconds) for the add-on's answer with the same nonce.
async fn studio_live_ask(
    app: &AppHandle,
    instance: &ServerInstance,
    file: &str,
    answer_file: &str,
    request: impl FnOnce(u64) -> Result<serde_json::Value, String>,
) -> Result<serde_json::Value, String> {
    let root = studio_root(instance).ok_or("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.")?;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_millis() as u64).unwrap_or(1);
    let request = request(nonce)?;
    server_write_text(app, instance, &format!("{root}/{file}"), &request.to_string()).await?;
    let answer = format!("{root}/{answer_file}");
    for _ in 0..24 {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let Some(text) = server_text(app, instance, &answer).await.ok().flatten() else { continue };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        if value.get("nonce").and_then(serde_json::Value::as_u64) == Some(nonce) {
            return Ok(value);
        }
    }
    Err("The add-on didn't answer. Is the server running with the Monolab add-on, and the player online?".into())
}

/// The in-game test tool: makes the rule event `event` happen to `player` (empty for timers, schedules and plugin
/// events) with `vars` as its placeholders, and returns what each rule for it did. `rule` narrows it to one rule.
#[tauri::command(async)]
#[specta::specta]
async fn studio_test_event(
    app: AppHandle,
    id: String,
    event: String,
    rule: String,
    player: String,
    vars: std::collections::BTreeMap<String, String>,
) -> Result<Vec<studio_held::TestRun>, String> {
    let instance = instance_by_id(&app, &id)?;
    let value = studio_live_ask(&app, &instance, studio_held::FIRE_FILE, studio_held::FIRED_FILE, |nonce| {
        Ok(serde_json::json!({ "nonce": nonce, "event": event, "rule": rule, "player": player, "vars": vars }))
    })
    .await?;
    studio_held::read_runs(&value)
}

#[tauri::command(async)]
#[specta::specta]
fn studio_template(kind: String, name: String) -> Result<String, String> {
    studio::path_of(studio::PAPER_ROOT, &kind, &name)?;
    Ok(studio::template(&kind, &name))
}

/// The editor's top-down look at a custom world's generator (no server needed).
#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
fn studio_worldgen_preview(
    generator: String,
    seed: String,
    world: String,
    x: i32,
    z: i32,
    size: u32,
    step: u32,
) -> Result<studio::WorldgenPreview, String> {
    studio::worldgen_preview(&generator, &seed, &world, x, z, size, step)
}

/// The one permission the Pumpkin add-on asks for: its own data folder (map, records, studio).
const PUMPKIN_ADDON_PERMISSION: &str = "fs.write.data";

/// What each Pumpkin server's console last listed per plugin: (server, plugin) → permissions it asked for,
/// and the plugin whose list is being read right now per server.
#[derive(Default)]
struct PumpkinAsked {
    asked: HashMap<(String, String), Vec<String>>,
    reading: HashMap<String, String>,
}

fn pumpkin_asked() -> &'static Mutex<PumpkinAsked> {
    static ASKED: std::sync::OnceLock<Mutex<PumpkinAsked>> = std::sync::OnceLock::new();
    ASKED.get_or_init(Default::default)
}

fn pumpkin_permission_watch(app: &AppHandle, event: &ServerLogEvent) {
    if event.stream == "input" {
        return;
    }
    let Some(seen) = pumpkin::permission_line(&event.line) else {
        // Anything else ends a permission list.
        if !event.line.trim().is_empty() {
            pumpkin_asked().lock().unwrap_or_else(|poison| poison.into_inner()).reading.remove(&event.server_id);
        }
        return;
    };
    let mut held = pumpkin_asked().lock().unwrap_or_else(|poison| poison.into_inner());
    match seen {
        pumpkin::PermissionLine::Asks(plugin) => {
            held.asked.insert((event.server_id.clone(), plugin.clone()), Vec::new());
            held.reading.insert(event.server_id.clone(), plugin);
        }
        pumpkin::PermissionLine::Wants(permission) => {
            if let Some(plugin) = held.reading.get(&event.server_id).cloned() {
                let list = held.asked.entry((event.server_id.clone(), plugin)).or_default();
                if !list.contains(&permission) && list.len() < 32 {
                    list.push(permission);
                }
            }
        }
        pumpkin::PermissionLine::Denied(plugin) => {
            held.reading.remove(&event.server_id);
            drop(held);
            let payload = serde_json::json!({ "serverId": event.server_id, "plugin": plugin });
            remember_for_web(app, "pumpkin-permission-denied", &payload);
            let _ = app.emit("pumpkin-permission-denied", payload);
        }
    }
}

/// The permissions a plugin Pumpkin refused asked for: as its console listed them this session, else every
/// refused permission in Pumpkin's cache (that file doesn't name plugins).
#[tauri::command(async)]
#[specta::specta]
async fn pumpkin_refused_permissions(app: AppHandle, id: String, plugin: String) -> Result<Vec<String>, String> {
    let instance = instance_by_id(&app, &id)?;
    if !pumpkin::is_pumpkin(&instance.core) {
        return Err("This is only for Pumpkin servers.".into());
    }
    let listed = pumpkin_asked()
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .asked
        .get(&(id.clone(), plugin.clone()))
        .cloned()
        .unwrap_or_default();
    if !listed.is_empty() {
        return Ok(listed);
    }
    let cache = server_text(&app, &instance, "plugins/permission_cache.json").await?.unwrap_or_default();
    Ok(pumpkin::refused_permissions(&cache))
}

/// Lets a plugin Pumpkin refused have the permissions it asked for (in `pumpkin.toml`, next start). Only
/// permissions [`pumpkin_refused_permissions`] names can be given.
#[tauri::command(async)]
#[specta::specta]
async fn allow_pumpkin_plugin(app: AppHandle, id: String, plugin: String, permissions: Vec<String>) -> Result<(), String> {
    let plugin = plugin.trim().to_string();
    if plugin.is_empty() || plugin.len() > 64 {
        return Err("Unknown plugin.".into());
    }
    let asked = pumpkin_refused_permissions(app.clone(), id.clone(), plugin.clone()).await?;
    if permissions.is_empty() || permissions.iter().any(|one| !asked.contains(one)) {
        return Err("Only the permissions this plugin asked for can be allowed.".into());
    }
    let instance = instance_by_id(&app, &id)?;
    let mut text = server_text(&app, &instance, pumpkin::CONFIG)
        .await?
        .ok_or("pumpkin.toml is missing. Start the server once so Pumpkin makes it.")?;
    let before = text.clone();
    for one in &permissions {
        text = pumpkin::set_plugin_permission(&text, &plugin, one, true)?;
    }
    if text != before {
        server_write_text(&app, &instance, pumpkin::CONFIG, &text).await?;
        record_audit(&app, &id, "Events", "Plugin permission", &format!("Allowed {plugin}: {}", permissions.join(", ")));
    }
    Ok(())
}

/// Whether `pumpkin.toml` lets the Monolab plugin use its data folder. `None` for servers that aren't Pumpkin.
#[tauri::command(async)]
#[specta::specta]
async fn pumpkin_addon_allowed(app: AppHandle, id: String) -> Result<Option<bool>, String> {
    let instance = instance_by_id(&app, &id)?;
    if !pumpkin::is_pumpkin(&instance.core) {
        return Ok(None);
    }
    let text = server_text(&app, &instance, pumpkin::CONFIG).await?.unwrap_or_default();
    Ok(Some(pumpkin::plugin_permission_allowed(&text, events::PUMPKIN_PLUGIN_NAME, PUMPKIN_ADDON_PERMISSION)))
}

/// The user's answer to "may the Monolab plugin use its data folder?". Pumpkin asks at its console, where nobody
/// is when Monolab starts the server, so it would refuse; this writes the answer into `pumpkin.toml` instead.
/// Applies on the next start.
#[tauri::command(async)]
#[specta::specta]
async fn set_pumpkin_addon_allowed(app: AppHandle, id: String, allow: bool) -> Result<(), String> {
    let instance = instance_by_id(&app, &id)?;
    if !pumpkin::is_pumpkin(&instance.core) {
        return Err("This is only for Pumpkin servers.".into());
    }
    let text = server_text(&app, &instance, pumpkin::CONFIG)
        .await?
        .ok_or("pumpkin.toml is missing. Start the server once so Pumpkin makes it.")?;
    let changed = pumpkin::set_plugin_permission(&text, events::PUMPKIN_PLUGIN_NAME, PUMPKIN_ADDON_PERMISSION, allow)?;
    if changed != text {
        server_write_text(&app, &instance, pumpkin::CONFIG, &changed).await?;
        let what = if allow {
            "Allowed the Monolab plugin to use its data folder"
        } else {
            "Took back the Monolab plugin's data folder permission"
        };
        record_audit(&app, &id, "Events", "Add‑on permission", what);
    }
    Ok(())
}

/// The add-on's feature switches (config.yml); missing file or key = on.
#[tauri::command(async)]
#[specta::specta]
async fn addon_config(app: AppHandle, id: String) -> Result<Vec<AddonSwitch>, String> {
    let instance = instance_by_id(&app, &id)?;
    let Some(path) = addon_config_path(&instance) else { return Ok(Vec::new()) };
    let text = server_text(&app, &instance, path).await?.unwrap_or_default();
    let read = integrations::parse(&text);
    let pumpkin = pumpkin::is_pumpkin(&instance.core);
    Ok(ADDON_SWITCHES
        .iter()
        .filter(|(key, _)| !pumpkin || PUMPKIN_SWITCHES.contains(key))
        .filter(|(key, _)| *key != "studio" || studio_supported(&instance))
        .map(|(key, restart)| AddonSwitch { key: key.to_string(), on: read.text(key).as_deref() != Some("false"), restart: *restart })
        .collect())
}

#[tauri::command(async)]
#[specta::specta]
async fn set_addon_config(app: AppHandle, id: String, key: String, on: bool) -> Result<(), String> {
    // `studio-<kind>`: one studio kind on or off (missing = on), e.g. because another plugin does that job.
    let studio_kind = key.strip_prefix("studio-").is_some_and(studio::valid_kind);
    if !studio_kind && !ADDON_SWITCHES.iter().any(|(one, _)| *one == key) {
        return Err("This setting can't be changed here.".into());
    }
    let instance = instance_by_id(&app, &id)?;
    let path = addon_config_path(&instance).ok_or("This server has no Monolab add-on.")?;
    let text = server_text(&app, &instance, path).await?.unwrap_or_default();
    let value = if on { "true" } else { "false" };
    let changed = match integrations::set_top_level(&text, &key, value) {
        Some(changed) => changed,
        None => {
            let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
            let gap = if text.is_empty() || text.ends_with(newline) { "" } else { newline };
            format!("{text}{gap}{key}: {value}{newline}")
        }
    };
    server_write_text(&app, &instance, path, &changed).await
}

/// Which plugin integrations this server has (EssentialsX, LuckPerms).
#[tauri::command(async)]
#[specta::specta]
async fn plugin_integrations(app: AppHandle, id: String) -> Result<integrations::Integrations, String> {
    let instance = instance_by_id(&app, &id)?;
    let mut found =
        integrations::Integrations { essentials: server_has_dir(&app, &instance, "plugins/Essentials").await, ..Default::default() };
    for folder in ["plugins/LuckPerms", "plugins/luckperms", "config/luckperms"] {
        if server_has_dir(&app, &instance, folder).await {
            found.luckperms = folder.to_string();
            break;
        }
    }
    found.extensions.extend(api_extensions(&app, &instance).await.into_iter().map(|one| one.folder));
    for base in ["plugins", "config"] {
        let folders: Vec<String> = server_names(&app, &instance, base)
            .await
            .into_iter()
            .filter(|(name, dir, _)| *dir && !name.eq_ignore_ascii_case("monolab") && !name.starts_with('.'))
            .map(|(name, _, _)| name)
            .take(80)
            .collect();
        for name in folders {
            let folder = format!("{base}/{name}");
            if server_names(&app, &instance, &folder).await.iter().any(|(one, dir, _)| !dir && one == "monolab.json") {
                found.extensions.push(folder);
            }
        }
    }
    Ok(found)
}

/// Tabs other plugins publish through MonolabApi (the add-on writes them to integrations.json).
async fn api_extensions(app: &AppHandle, instance: &ServerInstance) -> Vec<extensions::Extension> {
    let Some(config) = addon_config_path(instance).filter(|_| !pumpkin::is_pumpkin(&instance.core)) else { return Vec::new() };
    let path = config.replace("config.yml", "integrations.json");
    let Ok(Some(text)) = server_text(app, instance, &path).await else { return Vec::new() };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|time| time.as_millis() as u64).unwrap_or(0);
    extensions::from_api(&text, now)
}

async fn read_extension(app: &AppHandle, instance: &ServerInstance, folder: &str) -> Result<extensions::Extension, String> {
    let json = server_text(app, instance, &format!("{folder}/monolab.json")).await?.unwrap_or_default();
    let mut texts = HashMap::new();
    for file in extensions::declared_files(&json) {
        texts.insert(file.clone(), server_text(app, instance, &format!("{folder}/{file}")).await.ok().flatten());
    }
    Ok(extensions::build(folder, &json, |file| texts.get(file).cloned().flatten()))
}

/// Presses a MonolabApi button: leaves a request for the add-on, which calls the plugin's handler on the server
/// thread, then waits for its answer. Nothing goes through the console.
#[tauri::command(async)]
#[specta::specta]
async fn run_extension_action(app: AppHandle, id: String, folder: String, label: String, args_json: String) -> Result<String, String> {
    let owner = folder.strip_prefix("api:").ok_or("This button isn't from a plugin.")?.to_string();
    let args: HashMap<String, String> = serde_json::from_str(&args_json).map_err(|_| "The button values are malformed.".to_string())?;
    let instance = instance_by_id(&app, &id)?;
    let config =
        addon_config_path(&instance).filter(|_| !pumpkin::is_pumpkin(&instance.core)).ok_or("This server has no Monolab add-on.")?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|time| time.as_millis()).unwrap_or(0);
    let request = config.replace("config.yml", &format!("actions/req-{stamp}.json"));
    let result = config.replace("config.yml", &format!("actions/req-{stamp}.result.json"));
    let body = serde_json::json!({ "owner": owner, "label": label, "args": args }).to_string();
    server_write_text(&app, &instance, &request, &body).await?;
    record_audit(&app, &id, "Server", "Plugin button", &format!("{owner} · {label}"));
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let Ok(Some(text)) = server_text(&app, &instance, &result).await else { continue };
        let answer: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        drop_server_file(&app, &instance, &result).await;
        let message = answer.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
        return if answer.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) { Ok(message) } else { Err(message) };
    }
    drop_server_file(&app, &instance, &request).await;
    Err("The plugin didn't answer in 15 seconds. Check that the server is running with add-on 37 or later.".into())
}

/// Removes one of the add-on's own exchange files; a leftover is harmless, so failures are ignored.
async fn drop_server_file(app: &AppHandle, instance: &ServerInstance, relative: &str) {
    if !instance.panel_host.is_empty() {
        if let Ok((api, target)) = panel_of(app, instance) {
            let _ = api.delete(&target, &[relative.to_string()]).await;
        }
    } else if !instance.remote_host.is_empty() {
        remote::drop_path(app, &instance.remote_host, &instance.path, relative).await;
    } else if let Ok(path) = paths::resolve_in_server(Path::new(&instance.path), relative) {
        let _ = fs::remove_file(path);
    }
}

/// Controls declared in monolab.json files of this server's plugins and mods.
#[tauri::command(async)]
#[specta::specta]
async fn extension_data(app: AppHandle, id: String) -> Result<Vec<extensions::Extension>, String> {
    let instance = instance_by_id(&app, &id)?;
    let found = plugin_integrations(app.clone(), id).await?;
    let mut all = api_extensions(&app, &instance).await;
    for folder in found.extensions.iter().filter(|folder| !folder.starts_with("api:")) {
        all.push(read_extension(&app, &instance, folder).await?);
    }
    Ok(all)
}

#[tauri::command(async)]
#[specta::specta]
async fn extension_set(app: AppHandle, id: String, folder: String, index: u32, value: String) -> Result<(), String> {
    if !extensions::usable_folder(&folder) {
        return Err("This folder can't hold app controls.".into());
    }
    let instance = instance_by_id(&app, &id)?;
    let built = read_extension(&app, &instance, &folder).await?;
    let setting = built.settings.get(index as usize).ok_or("This setting is no longer in monolab.json.")?;
    let value = extensions::check(setting, &value)?;
    let path = format!("{folder}/{}", setting.file);
    let text = server_text(&app, &instance, &path).await?.ok_or_else(|| format!("{} was not found.", setting.file))?;
    let changed = extensions::set_at(&setting.file, &text, &setting.key, &setting.kind, &value)?;
    server_write_text(&app, &instance, &path, &changed).await
}

#[tauri::command(async)]
#[specta::specta]
async fn essentials_data(app: AppHandle, id: String) -> Result<integrations::EssentialsData, String> {
    let instance = instance_by_id(&app, &id)?;
    let config = server_text(&app, &instance, "plugins/Essentials/config.yml").await?.unwrap_or_default();
    let kits = match server_text(&app, &instance, "plugins/Essentials/kits.yml").await? {
        Some(text) => integrations::kits(&text),
        None => integrations::kits(&config),
    };
    let mut warps = Vec::new();
    for (name, dir, _) in server_names(&app, &instance, "plugins/Essentials/warps").await {
        let Some(stem) = name.strip_suffix(".yml").filter(|_| !dir) else { continue };
        if let Some(text) = server_text(&app, &instance, &format!("plugins/Essentials/warps/{name}")).await? {
            warps.extend(integrations::warp(stem, &text));
        }
    }
    warps.sort_by_key(|one| one.name.to_lowercase());
    let files: Vec<(String, u64)> = server_names(&app, &instance, "plugins/Essentials/userdata")
        .await
        .into_iter()
        .filter(|(name, dir, _)| !dir && name.ends_with(".yml"))
        .map(|(name, _, modified)| (name, modified))
        .collect();
    let users_total = files.len() as u32;
    let limit = if is_far(&instance) { 80 } else { 500 };
    let mut users = Vec::new();
    for name in integrations::newest_first(files).into_iter().take(limit) {
        if let Some(text) = server_text(&app, &instance, &format!("plugins/Essentials/userdata/{name}")).await? {
            users.push(integrations::user(name.trim_end_matches(".yml"), &text));
        }
    }
    Ok(integrations::EssentialsData { warps, kits, users, users_total, settings: integrations::settings(&config) })
}

#[tauri::command(async)]
#[specta::specta]
async fn essentials_set_setting(app: AppHandle, id: String, key: String, value: String) -> Result<(), String> {
    if !integrations::ESS_SETTINGS.contains(&key.as_str()) {
        return Err("This setting can't be changed here.".into());
    }
    let value = value.trim();
    if value.is_empty() || value.len() > 200 || value.contains(['\r', '\n']) {
        return Err("Enter a value on one line.".into());
    }
    let instance = instance_by_id(&app, &id)?;
    let config = server_text(&app, &instance, "plugins/Essentials/config.yml").await?.ok_or("EssentialsX config.yml was not found.")?;
    let changed = integrations::set_top_level(&config, &key, value).ok_or("EssentialsX config.yml was not found.")?;
    server_write_text(&app, &instance, "plugins/Essentials/config.yml", &changed).await
}

/// Runs `lp export` on the running server, reads the file it writes, then removes it.
#[tauri::command(async)]
#[specta::specta]
async fn luckperms_data(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<integrations::LuckPermsData, String> {
    let instance = instance_by_id(&app, &id)?;
    let found = plugin_integrations(app.clone(), id.clone()).await?;
    if found.luckperms.is_empty() {
        return Err("LuckPerms isn't installed on this server.".into());
    }
    let command = if kinds::is_proxy(&instance.core) { "lpv" } else { "lp" };
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|time| time.as_millis()).unwrap_or(0);
    let name = format!("monolab-export-{stamp}");
    send_console_command_inner(&app, &state, &id, &format!("{command} export {name}")).await?;
    let file = format!("{}/{name}.json.gz", found.luckperms);
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let bytes = if is_far(&instance) {
            far_read_bytes(&app, &instance, &file, 64 * 1024 * 1024).await.ok()
        } else {
            paths::resolve_in_server(Path::new(&instance.path), &file).ok().and_then(|path| fs::read(path).ok())
        };
        let Some(bytes) = bytes else { continue };
        let mut json = String::new();
        if flate2::read::GzDecoder::new(&bytes[..]).read_to_string(&mut json).is_err() {
            continue;
        }
        let Ok(data) = integrations::luckperms(&json, command) else { continue };
        if !instance.panel_host.is_empty() {
            if let Ok((api, target)) = panel_of(&app, &instance) {
                let _ = api.delete(&target, std::slice::from_ref(&file)).await;
            }
        } else if !instance.remote_host.is_empty() {
            remote::drop_path(&app, &instance.remote_host, &instance.path, &file).await;
        } else if let Ok(path) = paths::resolve_in_server(Path::new(&instance.path), &file) {
            let _ = fs::remove_file(path);
        }
        return Ok(data);
    }
    Err("LuckPerms didn't finish exporting in 20 seconds. Check the console.".into())
}

/// Read-only view of files the editor can't open: class files, NBT, archives (and files inside them), gzip.
#[tauri::command(async)]
#[specta::specta]
async fn view_server_file(app: AppHandle, id: String, path: String) -> Result<viewer::FileView, String> {
    let (outer, inner) = match path.split_once(viewer::SPLIT) {
        Some((outer, inner)) => (outer.to_string(), Some(inner.to_string())),
        None => (path.clone(), None),
    };
    let instance = instance_by_id(&app, &id)?;
    let bytes = if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        api.read_bytes(&target, &outer, viewer::FILE_LIMIT).await?
    } else if instance.remote_host.is_empty() {
        let at = paths::resolve_in_server(Path::new(&instance.path), &outer).map_err(|error| match error {
            paths::PathError::Missing => "File not found.".to_string(),
            other => other.to_string(),
        })?;
        if fs::metadata(&at).map_err(|error| error.to_string())?.len() > viewer::FILE_LIMIT {
            return Err("This file is too large to open in the app.".into());
        }
        fs::read(&at).map_err(|error| format!("Could not read the file: {error}"))?
    } else {
        remote::read_bytes(&app, &instance.remote_host, &instance.path, &outer, viewer::FILE_LIMIT).await?
    };
    let name = outer.rsplit(['/', '\\']).next().unwrap_or(&outer).to_string();
    tauri::async_runtime::spawn_blocking(move || off_runtime(|| viewer::view_bytes(&app, &name, bytes, inner.as_deref())))
        .await
        .map_err(|error| error.to_string())?
}

/// The folder of a trashed server, for looking only; nothing reached through it writes there.
fn trash_root(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let entry = load_trash_entries(app)?.into_iter().find(|entry| entry.id == id).ok_or("Trashed server not found.")?;
    Path::new(&entry.trash_path).canonicalize().map(plain_path).map_err(|_| "The trashed server folder was not found.".to_string())
}

fn in_trash(root: &Path, relative: &str) -> Result<PathBuf, String> {
    paths::resolve_in_server(root, relative).map_err(|error| match error {
        paths::PathError::Escape => "Cannot browse outside the server folder.".to_string(),
        paths::PathError::Missing => "File not found.".to_string(),
        other => other.to_string(),
    })
}

#[tauri::command(async)]
#[specta::specta]
fn browse_trash_files(app: AppHandle, id: String, path: String, offset: Option<u32>) -> Result<BrowsePage, String> {
    let root = trash_root(&app, &id)?;
    let requested = path.trim().trim_matches(['/', '\\']);
    let target = if requested.is_empty() { root.clone() } else { in_trash(&root, requested)? };
    browse_dir(&root, &target, offset, None)
}

#[tauri::command(async)]
#[specta::specta]
async fn view_trash_file(app: AppHandle, id: String, path: String) -> Result<viewer::FileView, String> {
    let (outer, inner) = match path.split_once(viewer::SPLIT) {
        Some((outer, inner)) => (outer.to_string(), Some(inner.to_string())),
        None => (path.clone(), None),
    };
    let at = in_trash(&trash_root(&app, &id)?, &outer)?;
    if fs::metadata(&at).map_err(|error| error.to_string())?.len() > viewer::FILE_LIMIT {
        return Err("This file is too large to open in the app.".into());
    }
    let bytes = fs::read(&at).map_err(|error| format!("Could not read the file: {error}"))?;
    let name = outer.rsplit(['/', '\\']).next().unwrap_or(&outer).to_string();
    tauri::async_runtime::spawn_blocking(move || off_runtime(|| viewer::view_bytes(&app, &name, bytes, inner.as_deref())))
        .await
        .map_err(|error| error.to_string())?
}

/// Server ids a database record can still belong to: the server list and the server trash.
fn known_server_ids(app: &AppHandle) -> Result<Vec<String>, String> {
    let mut known: Vec<String> = load_instances(app)?.into_iter().map(|one| one.id).collect();
    known.extend(load_trash_entries(app)?.into_iter().map(|one| one.id));
    Ok(known)
}

#[tauri::command(async)]
#[specta::specta]
fn server_databases(app: AppHandle, id: String) -> Result<Vec<databases::ServerDb>, String> {
    instance_by_id(&app, &id)?;
    databases::of_server(&app, &id)
}

#[tauri::command(async)]
#[specta::specta]
fn db_hosts(app: AppHandle) -> Result<Vec<databases::DbHostView>, String> {
    databases::list_hosts(&app)
}

#[tauri::command(async)]
#[specta::specta]
async fn save_db_host(app: AppHandle, input: databases::DbHostInput) -> Result<String, String> {
    databases::save_host(&app, input).await
}

#[tauri::command(async)]
#[specta::specta]
async fn check_db_host(app: AppHandle, id: String) -> Result<String, String> {
    databases::check_host(&app, &id).await
}

#[tauri::command(async)]
#[specta::specta]
fn delete_db_host(app: AppHandle, id: String) -> Result<(), String> {
    databases::delete_host(&app, &id)
}

#[tauri::command(async)]
#[specta::specta]
async fn create_server_database(app: AppHandle, id: String, host: String, name: String) -> Result<databases::ServerDb, String> {
    instance_by_id(&app, &id)?;
    let made = databases::create(&app, &id, &host, &name).await?;
    record_audit(&app, &id, "Databases", "Create database", &made.database);
    Ok(made)
}

#[tauri::command(async)]
#[specta::specta]
async fn link_server_database(app: AppHandle, id: String, input: databases::DbLinkInput) -> Result<databases::ServerDb, String> {
    instance_by_id(&app, &id)?;
    let made = databases::link_existing(&app, &id, input).await?;
    record_audit(&app, &id, "Databases", "Add existing database", &made.database);
    Ok(made)
}

/// App only: kept out of the web panel's allow list and Monolink on purpose.
#[tauri::command(async)]
#[specta::specta]
fn server_database_password(app: AppHandle, id: String, db: String) -> Result<String, String> {
    databases::password_of(&app, &id, &db)
}

#[tauri::command(async)]
#[specta::specta]
async fn rotate_server_database_password(app: AppHandle, id: String, db: String) -> Result<(), String> {
    databases::rotate_password(&app, &id, &db).await?;
    record_audit(&app, &id, "Databases", "Change database password", "");
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
async fn delete_server_database(app: AppHandle, id: String, db: String) -> Result<(), String> {
    let name = databases::record(&app, &id, &db)?.database;
    databases::delete(&app, &id, &db).await?;
    record_audit(&app, &id, "Databases", "Delete database", &name);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
fn orphan_databases(app: AppHandle) -> Result<Vec<databases::ServerDb>, String> {
    databases::orphans(&app, &known_server_ids(&app)?)
}

#[tauri::command(async)]
#[specta::specta]
async fn delete_orphan_database(app: AppHandle, db: String) -> Result<(), String> {
    let known = known_server_ids(&app)?;
    databases::delete_orphan(&app, &known, &db).await
}

/// The settings file of each plugin the app knows how to fill, when the server has it.
async fn db_plugin_files(app: &AppHandle, instance: &ServerInstance) -> Vec<(&'static str, &'static str)> {
    let mut found: Vec<(&'static str, &'static str)> = Vec::new();
    for (plugin, file) in databases::PLUGIN_FILES {
        if found.iter().any(|(seen, _)| *seen == plugin) {
            continue;
        }
        let there = if is_far(instance) {
            far_read(app, instance, file).await.is_ok()
        } else {
            paths::resolve_in_server(Path::new(&instance.path), file).is_ok_and(|path| path.is_file())
        };
        if there {
            found.push((plugin, file));
        }
    }
    found
}

#[tauri::command(async)]
#[specta::specta]
async fn db_plugin_targets(app: AppHandle, id: String) -> Result<Vec<String>, String> {
    let instance = instance_by_id(&app, &id)?;
    Ok(db_plugin_files(&app, &instance).await.into_iter().map(|(plugin, _)| plugin.to_string()).collect())
}

#[tauri::command(async)]
#[specta::specta]
async fn fill_plugin_database(app: AppHandle, id: String, db: String, plugin: String) -> Result<(), String> {
    let instance = instance_by_id(&app, &id)?;
    let one = databases::record(&app, &id, &db)?;
    let password = databases::password_of(&app, &id, &db)?;
    let (_, file) = db_plugin_files(&app, &instance)
        .await
        .into_iter()
        .find(|(name, _)| *name == plugin)
        .ok_or(format!("Could not find the settings file of {plugin}."))?;
    if is_far(&instance) {
        let text = far_read(&app, &instance, file).await?;
        far_write(&app, &instance, file, &databases::fill_plugin(&plugin, file, &text, &one, &password)?).await?;
    } else {
        let path = paths::resolve_in_server(Path::new(&instance.path), file).map_err(|error| error.to_string())?;
        let text = fs::read_to_string(&path).map_err(|error| format!("Could not read {file}: {error}"))?;
        fs::write(&path, databases::fill_plugin(&plugin, file, &text, &one, &password)?)
            .map_err(|error| format!("Could not save {file}: {error}"))?;
    }
    record_audit(&app, &id, "Databases", "Fill plugin settings", &format!("{plugin} · {}", one.database));
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
async fn database_tables(app: AppHandle, id: String, db: String) -> Result<Vec<String>, String> {
    let reach = databases::reach_of(&app, &id, &db)?;
    databases::tables(&app, &reach).await
}

#[tauri::command(async)]
#[specta::specta]
async fn database_rows(app: AppHandle, id: String, db: String, table: String, offset: u32) -> Result<databases::TablePage, String> {
    let reach = databases::reach_of(&app, &id, &db)?;
    databases::rows(&app, &reach, &table, offset).await
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct SqliteFile {
    path: String,
    size_bytes: u64,
    modified_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct SqliteScan {
    files: Vec<SqliteFile>,
    incomplete: bool,
}

/// Where plugins and mods keep their SQLite files.
const SQLITE_DIRS: [&str; 3] = ["plugins", "mods", "config"];
const SQLITE_DEPTH: usize = 4;
const SQLITE_VISITS: usize = 4000;
/// Far servers' SQLite files are copied to this computer before they're opened.
const SQLITE_COPY_LIMIT: u64 = 256 * 1024 * 1024;

fn scan_sqlite_local(root: &Path) -> SqliteScan {
    let mut scan = SqliteScan::default();
    let mut stack: Vec<(PathBuf, usize)> = SQLITE_DIRS.iter().map(|dir| (root.join(dir), 0)).filter(|(dir, _)| dir.is_dir()).collect();
    let mut visits = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(list) = fs::read_dir(&dir) else {
            scan.incomplete = true;
            continue;
        };
        for entry in list.flatten() {
            visits += 1;
            if visits > SQLITE_VISITS {
                scan.incomplete = true;
                break;
            }
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                if depth < SQLITE_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !databases::is_sqlite_name(&name) || !databases::has_sqlite_header(&path) {
                continue;
            }
            scan.files.push(SqliteFile {
                path: path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/"),
                size_bytes: meta.len(),
                modified_at: meta.modified().ok().and_then(|when| when.duration_since(UNIX_EPOCH).ok()).map_or(0, |gap| gap.as_secs()),
            });
        }
    }
    scan.files.sort_by(|a, b| a.path.cmp(&b.path));
    scan
}

async fn scan_sqlite_far(app: &AppHandle, instance: &ServerInstance) -> SqliteScan {
    let mut scan = SqliteScan::default();
    let Ok(top) = far_list(app, instance, "").await else {
        return SqliteScan { files: Vec::new(), incomplete: true };
    };
    let mut stack: Vec<(String, usize)> =
        top.into_iter().filter(|one| one.directory && SQLITE_DIRS.contains(&one.name.as_str())).map(|one| (one.name, 0)).collect();
    let mut visits = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rows) = far_list(app, instance, &dir).await else {
            scan.incomplete = true;
            continue;
        };
        for one in rows {
            visits += 1;
            if visits > SQLITE_VISITS {
                scan.incomplete = true;
                break;
            }
            let path = format!("{dir}/{}", one.name);
            if one.directory {
                if depth < SQLITE_DEPTH {
                    stack.push((path, depth + 1));
                }
            } else if databases::is_sqlite_name(&one.name) {
                scan.files.push(SqliteFile { path, size_bytes: one.size_bytes, modified_at: one.modified_at });
            }
        }
    }
    scan.files.sort_by(|a, b| a.path.cmp(&b.path));
    scan
}

#[tauri::command(async)]
#[specta::specta]
async fn server_sqlite_files(app: AppHandle, id: String) -> Result<SqliteScan, String> {
    let instance = instance_by_id(&app, &id)?;
    if is_far(&instance) {
        return Ok(scan_sqlite_far(&app, &instance).await);
    }
    let root = Path::new(&instance.path).canonicalize().map(plain_path).map_err(|error| format!("Server folder not found: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || scan_sqlite_local(&root)).await.map_err(|error| error.to_string())
}

/// A SQLite file to open read-only: the file itself, or for a far server a copy on this computer.
async fn sqlite_at(app: &AppHandle, id: &str, path: &str, fresh: bool) -> Result<PathBuf, String> {
    let instance = instance_by_id(app, id)?;
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if !databases::is_sqlite_name(name) {
        return Err("This file isn't a SQLite database.".into());
    }
    if !is_far(&instance) {
        return paths::resolve_in_server(Path::new(&instance.path), path).map_err(|error| match error {
            paths::PathError::Missing => "File not found.".to_string(),
            other => other.to_string(),
        });
    }
    let dir = app.path().app_cache_dir().map_err(|error| error.to_string())?.join("sqlite");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let key: String = <sha2::Sha256 as sha2::Digest>::digest(format!("{id}\n{path}").as_bytes())
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let copy = dir.join(format!("{key}.db"));
    if fresh || !copy.is_file() {
        let bytes = far_read_bytes(app, &instance, path, SQLITE_COPY_LIMIT).await?;
        fs::write(&copy, bytes).map_err(|error| format!("Could not keep a copy of the file: {error}"))?;
    }
    Ok(copy)
}

#[tauri::command(async)]
#[specta::specta]
async fn sqlite_tables(app: AppHandle, id: String, path: String) -> Result<Vec<String>, String> {
    let at = sqlite_at(&app, &id, &path, true).await?;
    tauri::async_runtime::spawn_blocking(move || databases::sqlite_tables(&at)).await.map_err(|error| error.to_string())?
}

#[tauri::command(async)]
#[specta::specta]
async fn sqlite_rows(app: AppHandle, id: String, path: String, table: String, offset: u32) -> Result<databases::TablePage, String> {
    let at = sqlite_at(&app, &id, &path, false).await?;
    tauri::async_runtime::spawn_blocking(move || databases::sqlite_rows(&at, &table, offset)).await.map_err(|error| error.to_string())?
}

fn market_record_path(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("market");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir.join(format!("{id}.json")))
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct MarketInstall {
    file_name: String,
    project_id: String,
    version: String,
}

/// Plugins installed from Pumpkin Market on this server, by file name.
#[tauri::command(async)]
#[specta::specta]
fn market_installed(app: AppHandle, id: String) -> Result<Vec<MarketInstall>, String> {
    let mut all: Vec<MarketInstall> = market::read_installed(&market_record_path(&app, &id)?)
        .into_iter()
        .map(|(file_name, one)| MarketInstall { file_name, project_id: market::project_id(one.plugin), version: one.version })
        .collect();
    all.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    Ok(all)
}

/// Downloads a free Pumpkin Market plugin into plugins/. Returns the file name.
#[tauri::command(async)]
#[specta::specta]
async fn install_market_plugin(app: AppHandle, state: State<'_, AppState>, id: String, project: String) -> Result<String, String> {
    let instance = instance_by_id(&app, &id)?;
    if !pumpkin::is_pumpkin(&instance.core) {
        return Err("Pumpkin Market plugins only run on Pumpkin servers.".into());
    }
    let plugin = market::plugin_of(&project).ok_or("This isn't a Pumpkin Market plugin.")?;
    if is_far(&instance) && is_running(&state, &id) {
        return Err("Can't add files over SSH while the server is running. Please stop it first.".into());
    }
    let (about, version, bytes) = tauri::async_runtime::spawn_blocking(move || {
        let client = crate::net::builder().timeout(Duration::from_secs(60)).build().map_err(|error| error.to_string())?;
        let about = market::detail(&client, plugin)?;
        if !market::is_free(&about) {
            return Err("This plugin isn't free, so it can only be bought on its page.".to_string());
        }
        let version = market::latest_version(&client, plugin)?;
        let bytes = client
            .get(market::download_url(plugin))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::bytes)
            .map_err(|error| format!("Could not download the plugin: {error}"))?;
        Ok((about, version, bytes.to_vec()))
    })
    .await
    .map_err(|error| error.to_string())??;
    let title = market::text(&about, "name");
    let name = market::file_name(&title);
    let staged = app.path().app_cache_dir().map_err(|error| error.to_string())?.join(format!("market-{plugin}.wasm"));
    if let Some(dir) = staged.parent() {
        fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    fs::write(&staged, &bytes).map_err(|error| format!("Could not save the download: {error}"))?;
    let wasm = market::is_wasm(&staged);
    let _ = fs::remove_file(&staged);
    if !wasm {
        return Err("The download isn't a Pumpkin plugin (.wasm) file.".into());
    }
    if is_far(&instance) {
        far_upload(&app, &instance, &format!("plugins/{name}"), &bytes, &name).await?;
    } else {
        let dir = Path::new(&instance.path).join("plugins");
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        fs::write(dir.join(&name), &bytes).map_err(|error| format!("Could not save {name}: {error}"))?;
    }
    let record = market_record_path(&app, &id)?;
    let mut all = market::read_installed(&record);
    all.retain(|_, one| one.plugin != plugin);
    all.insert(name.clone(), market::Installed { plugin, version: version.clone(), title: title.clone(), icon: market::icon_of(&about) });
    market::write_installed(&record, &all)?;
    record_audit(&app, &id, "Mods & plugins", "Install from Pumpkin Market", &format!("{title} {version}"));
    Ok(name)
}

/// A Pumpkin Market plugin's page, shaped like a Modrinth one so the same screen draws it.
#[tauri::command(async)]
#[specta::specta]
async fn market_project_detail(project: String) -> Result<ProjectDetail, String> {
    let plugin = market::plugin_of(&project).ok_or("This isn't a Pumpkin Market plugin.")?;
    let about = tauri::async_runtime::spawn_blocking(move || {
        let client = crate::net::builder().timeout(Duration::from_secs(30)).build().map_err(|error| error.to_string())?;
        market::detail(&client, plugin)
    })
    .await
    .map_err(|error| error.to_string())??;
    let gallery = about
        .get("screenshots")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|one| one.as_str().or_else(|| ["url", "path"].iter().find_map(|key| one.get(*key).and_then(serde_json::Value::as_str))))
        .filter(|url| url.starts_with("https://"))
        .map(|url| GalleryItem { url: url.to_string(), title: None, description: None, featured: false })
        .collect();
    Ok(ProjectDetail {
        project_id: project,
        title: market::title_of(&about),
        description: market::short_of(&about),
        body_html: markdown::render(&market::body_of(&about)),
        icon_url: market::icon_of(&about),
        gallery,
        categories: Some(market::text(&about, "category")).filter(|one| !one.is_empty()).into_iter().collect(),
        loaders: vec!["pumpkin".into()],
        game_versions: Vec::new(),
        downloads: about.get("downloads").and_then(serde_json::Value::as_u64).unwrap_or(0),
        followers: 0,
        published: market::text(&about, "created_at"),
        updated: market::text(&about, "updated_at"),
        license: None,
        source_url: market::source_of(&about),
        issues_url: None,
        wiki_url: None,
        discord_url: None,
        page_url: market::page_url(plugin),
        team: Some(market::text(&about, "dev_name")).filter(|one| !one.is_empty()).into_iter().collect(),
        client_side: String::new(),
        server_side: String::new(),
    })
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct MarketUpdate {
    file_name: String,
    project_id: String,
    title: String,
    version: String,
    latest: String,
}

/// Pumpkin Market plugins on this server with a newer version on the market. Plugins the market
/// can't be asked about right now are left out.
#[tauri::command(async)]
#[specta::specta]
async fn market_updates(app: AppHandle, id: String) -> Result<Vec<MarketUpdate>, String> {
    let installed = market::read_installed(&market_record_path(&app, &id)?);
    if installed.is_empty() {
        return Ok(Vec::new());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let client = crate::net::builder().timeout(Duration::from_secs(20)).build().map_err(|error| error.to_string())?;
        let mut found: Vec<MarketUpdate> = installed
            .into_iter()
            .filter_map(|(file_name, one)| {
                let latest = market::latest_version(&client, one.plugin).ok()?;
                market::needs_update(&one.version, &latest).then(|| MarketUpdate {
                    file_name,
                    project_id: market::project_id(one.plugin),
                    title: one.title,
                    version: one.version,
                    latest,
                })
            })
            .collect();
        found.sort_by_key(|one| one.title.to_lowercase());
        Ok(found)
    })
    .await
    .map_err(|error| error.to_string())?
}
#[derive(serde::Serialize, specta::Type, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
struct TextureAnimation {
    frametime: u32,
    interpolate: bool,
    width: u32,
    height: u32,
    frames: Vec<TextureFrame>,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct TextureFrame {
    index: u32,
    time: u32,
}

fn read_animation(text: &str) -> Option<TextureAnimation> {
    let whole: serde_json::Value = serde_json::from_str(text).ok()?;
    let one = whole.get("animation")?;
    let number = |name: &str| one.get(name).and_then(|value| value.as_u64()).unwrap_or(0).min(u32::MAX as u64) as u32;
    let mut frames = Vec::new();
    if let Some(list) = one.get("frames").and_then(|value| value.as_array()) {
        for item in list {
            if let Some(index) = item.as_u64() {
                frames.push(TextureFrame { index: index as u32, time: 0 });
            } else if let Some(index) = item.get("index").and_then(|value| value.as_u64()) {
                let time = item.get("time").and_then(|value| value.as_u64()).unwrap_or(0) as u32;
                frames.push(TextureFrame { index: index as u32, time });
            }
        }
    }
    Some(TextureAnimation {
        frametime: number("frametime").max(1),
        interpolate: one.get("interpolate").and_then(|value| value.as_bool()).unwrap_or(false),
        width: number("width"),
        height: number("height"),
        frames,
    })
}

#[tauri::command(async)]
#[specta::specta]
async fn read_texture_animation(app: AppHandle, id: String, path: String) -> Result<Option<TextureAnimation>, String> {
    let beside = format!("{path}.mcmeta");
    let instance = instance_by_id(&app, &id)?;
    let text = if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        match api.read_text(&target, &beside).await {
            Ok(text) => text,
            Err(_) => return Ok(None),
        }
    } else if instance.remote_host.is_empty() {
        let at = match paths::resolve_in_server(Path::new(&instance.path), &beside) {
            Ok(at) => at,
            Err(_) => return Ok(None),
        };
        match fs::read_to_string(&at) {
            Ok(text) => text,
            Err(_) => return Ok(None),
        }
    } else {
        match far_read(&app, &instance, &beside).await {
            Ok(text) => text,
            Err(_) => return Ok(None),
        }
    };
    Ok(read_animation(&text))
}

#[tauri::command(async)]
#[specta::specta]
async fn save_editable_file(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    path: String,
    content: String,
    expect_hash: Option<String>,
) -> Result<(), String> {
    // F2: refuse to overwrite a file someone saved after this editor opened it (no hash = the old way).
    if let Some(want) = expect_hash.as_deref().filter(|one| !one.is_empty()) {
        let now = read_editable_file(app.clone(), id.clone(), path.clone()).await.ok();
        if saved_since(now.as_deref(), Some(want)) {
            let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|time| time.as_secs()).unwrap_or(0);
            let who = authorship::who_changed(&id, &path, at)
                .map(|one| if one == "host" { "The host".to_string() } else { one })
                .unwrap_or_else(|| "Someone".into());
            return Err(format!("{who} saved this file after you opened it."));
        }
    }
    note_writes(&app, &id, "save_editable_file", &serde_json::json!({ "path": path }), "host");
    save_editable_file_inner(app.clone(), state, id.clone(), path.clone(), content).await?;
    // F4: whoever else has this file open hears it changed (the saver's window ignores its own).
    if editing::people(&id).iter().any(|(open, people)| *open == path.replace('\\', "/").trim_start_matches('/') && !people.is_empty()) {
        team_notice::send(&app, &id, team_notice::NoticeKind::SavedOver, None, &path);
    }
    // The team sees which file changed (not what changed in it).
    monolink_team::file_changed(&app, &id, &path);
    Ok(())
}

async fn save_editable_file_inner(
    app: AppHandle,
    _state: State<'_, AppState>,
    id: String,
    path: String,
    content: String,
) -> Result<(), String> {
    if content.len() > 8 * 1024 * 1024 {
        return Err("Content larger than 8 MB cannot be saved.".into());
    }
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        api.write_text(&target, &path, &content).await?;
        record_audit(&app, &id, "File", "Save file (panel)", &path);
        return Ok(());
    }
    if !instance.remote_host.is_empty() {
        far_write(&app, &instance, &path, &content).await?;
        record_audit(&app, &id, "File", "Save file (SSH)", &path);
        return Ok(());
    }
    let path = safe_editable_path(&instance, &path)?;
    fs::write(path, content).map_err(|error| format!("Could not save the text file: {error}"))?;
    record_audit(&app, &id, "File", "Text file saved", "File saved!");
    Ok(())
}

fn file_in_server(app: &AppHandle, state: &AppState, id: &str, relative: &str) -> Result<(PathBuf, PathBuf), String> {
    let _ = state;
    let instance = instance_by_id(app, id)?;
    let root = PathBuf::from(&instance.path);
    let path = paths::resolve_in_server(&root, relative).map_err(|error| match error {
        paths::PathError::Missing => "File not found.".to_string(),
        paths::PathError::Escape => "Nothing outside the server folder can be touched.".to_string(),
        other => other.to_string(),
    })?;
    Ok((root, path))
}

fn parent_of(relative: &str) -> String {
    let normal = relative.replace('\\', "/");
    let trimmed = normal.trim().trim_matches('/');
    match trimmed.rsplit_once('/') {
        Some((head, _)) => head.to_string(),
        None => String::new(),
    }
}

fn plain_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Please enter a name.".into());
    }
    if name.contains(['/', '\\']) {
        return Err("Names cannot contain slashes.".into());
    }
    Ok(name)
}

#[tauri::command(async)]
#[specta::specta]
async fn rename_server_path(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    path: String,
    new_name: String,
) -> Result<String, String> {
    {
        let instance = instance_by_id(&app, &id)?;
        if !instance.panel_host.is_empty() {
            let name = plain_name(&new_name)?;
            let parent = parent_of(&path);
            let to = if parent.is_empty() { name.to_string() } else { format!("{parent}/{name}") };
            let (api, target) = panel_of(&app, &instance)?;
            pfs::move_path(&api, &target, &path, &to).await?;
            record_audit(&app, &id, "File", "Rename (panel)", &to);
            return Ok(to);
        }
        if !instance.remote_host.is_empty() {
            let name = plain_name(&new_name)?;
            let parent = parent_of(&path);
            let to = if parent.is_empty() { name.to_string() } else { format!("{parent}/{name}") };
            remote::move_path(&app, &instance.remote_host, &instance.path, &path, &to).await?;
            record_audit(&app, &id, "File", "Rename (SSH)", &to);
            return Ok(to);
        }
    }
    let (root, from) = file_in_server(&app, &state, &id, &path)?;
    let name = plain_name(&new_name)?;
    let parent = parent_of(&path);
    let relative = if parent.is_empty() { name.to_string() } else { format!("{parent}/{name}") };
    let to = paths::resolve_new_in_server(&root, &relative).map_err(|error| error.to_string())?;
    if to.exists() {
        return Err(format!("{name} already exists."));
    }
    fs::rename(&from, &to).map_err(|error| format!("Could not rename: {error}"))?;
    record_audit(&app, &id, "File", "Rename", &format!("{path} → {name}"));
    Ok(relative)
}

#[tauri::command(async)]
#[specta::specta]
async fn trash_server_path(app: AppHandle, state: State<'_, AppState>, id: String, path: String) -> Result<(), String> {
    note_writes(&app, &id, "trash_server_path", &serde_json::json!({ "path": path }), "host");
    {
        let instance = instance_by_id(&app, &id)?;
        if !instance.panel_host.is_empty() {
            let (api, target) = panel_of(&app, &instance)?;
            pfs::trash_path(&api, &target, &path).await?;
            record_audit(&app, &id, "File", "Move to trash (panel)", &path);
            return Ok(());
        }
        if !instance.remote_host.is_empty() {
            remote::trash_path(&app, &instance.remote_host, &instance.path, &path).await?;
            record_audit(&app, &id, "File", "Move to trash (SSH)", &path);
            return Ok(());
        }
    }
    let (_, from) = file_in_server(&app, &state, &id, &path)?;
    let name = from.file_name().map(|one| one.to_string_lossy().to_string()).ok_or("The name could not be read.")?;
    let stored = format!("{}-{name}", unix_now());
    let bin = addon_trash_dir(&app, &id)?;
    let to = bin.join(&stored);
    let moved = if from.is_dir() { move_tree(&from, &to) } else { move_file(&from, &to) };
    moved.map_err(|error| format!("Could not move to trash: {error}"))?;
    let mut note = trash_notes(&bin);
    note.insert(stored, path.trim_matches('/').replace('\\', "/"));
    write_trash_notes(&bin, &note);
    record_audit(&app, &id, "File", "Move to trash", &path);
    Ok(())
}

fn trash_notes(bin: &Path) -> HashMap<String, String> {
    fs::read_to_string(bin.join(remote::TRASH_NOTE)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn write_trash_notes(bin: &Path, note: &HashMap<String, String>) {
    if let Ok(text) = serde_json::to_string_pretty(note) {
        let _ = fs::write(bin.join(remote::TRASH_NOTE), text);
    }
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct TrashFile {
    stored: String,
    name: String,
    came_from: String,
    size_bytes: u64,
    at: u64,
    directory: bool,
}

#[tauri::command(async)]
#[specta::specta]
async fn list_server_trash(app: AppHandle, id: String) -> Result<Vec<TrashFile>, String> {
    let instance = instance_by_id(&app, &id)?;
    let mut out = Vec::new();
    let rows = if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        Some(pfs::trash_list(&api, &target).await?)
    } else if !instance.remote_host.is_empty() {
        Some(remote::trash_list(&app, &instance.remote_host, &instance.path).await?)
    } else {
        None
    };
    if let Some(rows) = rows {
        for (row, came) in rows {
            out.push(TrashFile {
                at: trash_stamp(&row.name).unwrap_or(row.modified_at),
                name: remote::plain_trash_name(&row.name),
                came_from: came,
                size_bytes: row.size_bytes,
                directory: row.directory,
                stored: row.name,
            });
        }
    } else {
        let bin = addon_trash_dir(&app, &id)?;
        let note = trash_notes(&bin);
        for entry in fs::read_dir(&bin).map_err(|error| error.to_string())?.flatten() {
            let stored = entry.file_name().to_string_lossy().to_string();
            if stored == remote::TRASH_NOTE {
                continue;
            }
            let meta = entry.metadata().map_err(|error| error.to_string())?;
            let name = remote::plain_trash_name(&stored);
            out.push(TrashFile {
                at: trash_stamp(&stored).unwrap_or(0),
                came_from: note.get(&stored).cloned().unwrap_or_else(|| name.clone()),
                name,
                size_bytes: if meta.is_dir() { 0 } else { meta.len() },
                directory: meta.is_dir(),
                stored,
            });
        }
    }
    out.sort_by_key(|one| std::cmp::Reverse(one.at));
    Ok(out)
}

fn trash_stamp(stored: &str) -> Option<u64> {
    stored.split_once('-').and_then(|(head, _)| head.parse().ok())
}

#[tauri::command(async)]
#[specta::specta]
async fn restore_server_trash(app: AppHandle, state: State<'_, AppState>, id: String, stored: String) -> Result<String, String> {
    let instance = instance_by_id(&app, &id)?;
    let back = if !instance.panel_host.is_empty() {
        ensure_panel_stopped(&app, &instance).await?;
        let (api, target) = panel_of(&app, &instance)?;
        pfs::trash_restore(&api, &target, &stored).await?
    } else if !instance.remote_host.is_empty() {
        ensure_remote_stopped(&app, &state, &instance).await?;
        remote::trash_restore(&app, &instance.remote_host, &instance.path, &stored).await?
    } else {
        ensure_stopped(&state, &id)?;
        paths::check_relative(&stored)?;
        let bin = addon_trash_dir(&app, &id)?;
        let note = trash_notes(&bin);
        let back = note.get(&stored).cloned().unwrap_or_else(|| remote::plain_trash_name(&stored));
        let to = paths::resolve_new_in_server(Path::new(&instance.path), &back)?;
        if to.exists() {
            return Err(format!("Something is already at {back}. Move it out of the way first."));
        }
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let from = bin.join(&stored);
        let moved = if from.is_dir() { move_tree(&from, &to) } else { move_file(&from, &to) };
        moved.map_err(|error| format!("Could not restore: {error}"))?;
        let mut note = note;
        note.remove(&stored);
        write_trash_notes(&bin, &note);
        back
    };
    record_audit(&app, &id, "File", "Restore from trash", &back);
    Ok(back)
}

#[tauri::command(async)]
#[specta::specta]
async fn delete_server_trash(app: AppHandle, id: String, stored: String) -> Result<(), String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        pfs::trash_delete(&api, &target, &stored).await?;
    } else if !instance.remote_host.is_empty() {
        remote::trash_delete(&app, &instance.remote_host, &instance.path, &stored).await?;
    } else {
        paths::check_relative(&stored)?;
        let bin = addon_trash_dir(&app, &id)?;
        let at = bin.join(&stored);
        let gone = if at.is_dir() { fs::remove_dir_all(&at) } else { fs::remove_file(&at) };
        gone.map_err(|error| format!("Could not delete: {error}"))?;
        let mut note = trash_notes(&bin);
        note.remove(&stored);
        write_trash_notes(&bin, &note);
    }
    record_audit(&app, &id, "File", "Delete from trash", &remote::plain_trash_name(&stored));
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
async fn empty_server_trash(app: AppHandle, id: String) -> Result<u32, String> {
    let instance = instance_by_id(&app, &id)?;
    let count = if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        pfs::trash_empty(&api, &target).await?
    } else if !instance.remote_host.is_empty() {
        remote::trash_empty(&app, &instance.remote_host, &instance.path).await?
    } else {
        let bin = addon_trash_dir(&app, &id)?;
        let mut count = 0_u32;
        for entry in fs::read_dir(&bin).map_err(|error| error.to_string())?.flatten() {
            let at = entry.path();
            let gone = if at.is_dir() { fs::remove_dir_all(&at) } else { fs::remove_file(&at) };
            if gone.is_ok() && entry.file_name().to_string_lossy() != remote::TRASH_NOTE {
                count += 1;
            }
        }
        count
    };
    record_audit(&app, &id, "File", "Trash emptied", &format!("{count}"));
    Ok(count)
}

#[tauri::command(async)]
#[specta::specta]
async fn add_server_folders(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    path: String,
    sources: Vec<String>,
    merge: bool,
) -> Result<u32, String> {
    let instance = instance_by_id(&app, &id)?;
    for one in &sources {
        if !PathBuf::from(one.trim()).is_dir() {
            return Err(format!("{one} is not a folder."));
        }
    }
    let count = put_folders(&app, &state, &instance, &path, &sources, merge).await?;
    record_audit(
        &app,
        &id,
        "File",
        if instance.remote_host.is_empty() { "Add folder" } else { "Add folder (SSH)" },
        &format!("{} · {count} files{}", sources.len(), if merge { " · merged into the existing folder" } else { "" }),
    );
    Ok(count)
}

#[tauri::command(async)]
#[specta::specta]
async fn folders_in_the_way(app: AppHandle, id: String, path: String, sources: Vec<String>) -> Result<Vec<String>, String> {
    let instance = instance_by_id(&app, &id)?;
    let here = path.replace('\\', "/").trim().trim_matches('/').to_string();
    let mut out = Vec::new();
    for one in &sources {
        let from = PathBuf::from(one.trim());
        if !from.is_dir() {
            continue;
        }
        let Some(name) = from.file_name().map(|one| one.to_string_lossy().to_string()) else {
            continue;
        };
        let at = if here.is_empty() { name.clone() } else { format!("{here}/{name}") };
        let there = if !instance.panel_host.is_empty() {
            let (api, target) = panel_of(&app, &instance)?;
            api.exists(&target, &at).await? == Some(true)
        } else if instance.remote_host.is_empty() {
            paths::resolve_new_in_server(Path::new(&instance.path), &at).map(|one| one.exists()).unwrap_or(false)
        } else {
            remote::is_dir_checked(&app, &instance.remote_host, &instance.path, &at).await?
        };
        if there {
            out.push(name);
        }
    }
    Ok(out)
}

async fn put_folders(
    app: &AppHandle,
    state: &AppState,
    instance: &ServerInstance,
    path: &str,
    sources: &[String],
    merge: bool,
) -> Result<u32, String> {
    let id = instance.id.clone();
    let here = path.replace('\\', "/").trim().trim_matches('/').to_string();
    let mut jobs = Vec::new();
    for one in sources {
        let from = PathBuf::from(one.trim());
        if !from.is_dir() {
            continue;
        }
        let name = from.file_name().map(|one| one.to_string_lossy().to_string()).ok_or("Could not read the folder name.")?;
        let at = if here.is_empty() { name.clone() } else { format!("{here}/{name}") };
        jobs.push((at, name, from));
    }
    let _ = (state, &id);
    let mut count = 0_u32;
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(app, instance)?;
        if !merge {
            for (at, name, _) in &jobs {
                if api.exists(&target, at).await?.is_some() {
                    return Err(format!("The folder {name} already exists. To merge, drop it again and choose Merge."));
                }
            }
        }
        for (_, name, from) in &jobs {
            tell_job(app, "upload", name, "packing", 0, 0);
            let packed = std::env::temp_dir().join(format!("monolab-up-{}-{}.zip", unix_now(), count));
            let files = (|| -> Result<u32, String> {
                let file = fs::File::create(&packed).map_err(|error| error.to_string())?;
                let mut writer = ZipWriter::new(file);
                let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
                zip_into(&mut writer, from.parent().unwrap_or(from), from, &packed, options)?;
                let total = count_files(from) as u32;
                writer.finish().map_err(|error| error.to_string())?;
                Ok(total)
            })();
            let sent = async {
                let files = files?;
                let bytes = fs::read(&packed).map_err(|error| error.to_string())?;
                let zip_name = format!(".monolab-up-{}.zip", unix_now());
                tell_upload(app, name, "uploading", 0, bytes.len() as u64);
                api.upload(&target, &here, &zip_name, bytes).await?;
                tell_upload(app, name, "unpacking", 0, 0);
                let at = if here.is_empty() { zip_name.clone() } else { format!("{here}/{zip_name}") };
                let opened = api.decompress(&target, &at).await;
                let _ = api.delete(&target, &[at]).await;
                opened.map(|_| files)
            }
            .await;
            let _ = fs::remove_file(&packed);
            tell_upload(app, name, "done", 0, 0);
            count += sent?;
        }
        return Ok(count);
    }
    if !instance.remote_host.is_empty() {
        if !merge {
            for (at, name, _) in &jobs {
                if remote::is_dir_checked(app, &instance.remote_host, &instance.path, at).await? {
                    return Err(format!("The folder {name} already exists. To merge, drop it again and choose Merge."));
                }
            }
        }
        for (at, name, from) in &jobs {
            let sent = remote::upload_folder(app, &instance.remote_host, &instance.path, at, from, &|done, whole, _| {
                tell_job(app, "upload", name, "files", done as u64, whole as u64);
            })
            .await;
            tell_upload(app, name, "done", 0, 0);
            count += sent? as u32;
        }
    } else {
        if !merge {
            for (at, name, _) in &jobs {
                if paths::resolve_new_in_server(Path::new(&instance.path), at)?.exists() {
                    return Err(format!("The folder {name} already exists. To merge, drop it again and choose Merge."));
                }
            }
        }
        for (at, name, from) in &jobs {
            let to = paths::resolve_new_in_server(Path::new(&instance.path), at)?;
            let whole = count_files(from) as u64;
            let done = std::sync::atomic::AtomicU64::new(0);
            let sent = copy_tree_counting(from, &to, &|| {
                let at = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                tell_job(app, "upload", name, "files", at, whole);
            });
            tell_upload(app, name, "done", 0, 0);
            count += sent?;
        }
    }
    Ok(count)
}

pub(crate) fn copy_tree_counting(from: &Path, to: &Path, tick: &dyn Fn()) -> Result<u32, String> {
    fs::create_dir_all(to).map_err(|error| error.to_string())?;
    let mut count = 0_u32;
    for entry in fs::read_dir(from).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        let next = to.join(entry.file_name());
        if kind.is_dir() {
            count += copy_tree_counting(&entry.path(), &next, tick)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &next).map_err(|error| error.to_string())?;
            count += 1;
            tick();
        }
    }
    Ok(count)
}

#[tauri::command(async)]
#[specta::specta]
async fn create_server_file(app: AppHandle, _state: State<'_, AppState>, id: String, path: String, name: String) -> Result<String, String> {
    note_writes(&app, &id, "create_server_file", &serde_json::json!({ "path": path, "name": name }), "host");
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        let name = plain_name(&name)?;
        if !editable_file(Path::new(name)) {
            return Err("This file type can't be created in the app. Only text files can.".into());
        }
        let here = path.replace('\\', "/").trim().trim_matches('/').to_string();
        let at = if here.is_empty() { name.to_string() } else { format!("{here}/{name}") };
        let (api, target) = panel_of(&app, &instance)?;
        if api.exists(&target, &at).await?.is_some() {
            return Err(format!("{name} already exists."));
        }
        api.write_text(&target, &at, "").await?;
        record_audit(&app, &id, "File", "Create file (panel)", &at);
        return Ok(at);
    }
    if !instance.remote_host.is_empty() {
        let name = plain_name(&name)?;
        if !editable_file(Path::new(name)) {
            return Err("This file type can't be created in the app. Only text files can.".into());
        }
        let here = path.replace('\\', "/").trim().trim_matches('/').to_string();
        let at = if here.is_empty() { name.to_string() } else { format!("{here}/{name}") };
        far_write(&app, &instance, &at, "").await?;
        return Ok(at);
    }
    let root = PathBuf::from(&instance.path);
    let name = plain_name(&name)?;
    if !editable_file(Path::new(name)) {
        return Err(format!("{name} is not a text file, so it cannot be created here. Use a name like .yml, .json, .txt or .properties."));
    }
    let here = path.replace('\\', "/").trim().trim_matches('/').to_string();
    let relative = if here.is_empty() { name.to_string() } else { format!("{here}/{name}") };
    let target = paths::resolve_new_in_server(&root, &relative).map_err(|error| error.to_string())?;
    if target.exists() {
        return Err(format!("{name} already exists."));
    }
    fs::write(&target, "").map_err(|error| format!("Could not create the file: {error}"))?;
    record_audit(&app, &id, "File", "Create file", &relative);
    Ok(relative)
}

#[tauri::command(async)]
#[specta::specta]
async fn create_server_folder(
    app: AppHandle,
    _state: State<'_, AppState>,
    id: String,
    path: String,
    name: String,
) -> Result<String, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        let name = plain_name(&name)?;
        let here = path.replace('\\', "/").trim().trim_matches('/').to_string();
        let at = if here.is_empty() { name.to_string() } else { format!("{here}/{name}") };
        let (api, target) = panel_of(&app, &instance)?;
        pfs::make_dir(&api, &target, &at).await?;
        return Ok(at);
    }
    if !instance.remote_host.is_empty() {
        let name = plain_name(&name)?;
        let here = path.replace('\\', "/").trim().trim_matches('/').to_string();
        let at = if here.is_empty() { name.to_string() } else { format!("{here}/{name}") };
        remote::make_dir(&app, &instance.remote_host, &instance.path, &at).await?;
        return Ok(at);
    }
    let root = PathBuf::from(&instance.path);
    let name = plain_name(&name)?;
    let here = path.replace('\\', "/").trim().trim_matches('/').to_string();
    let relative = if here.is_empty() { name.to_string() } else { format!("{here}/{name}") };
    let target = paths::resolve_new_in_server(&root, &relative).map_err(|error| error.to_string())?;
    if target.exists() {
        return Err(format!("{name} already exists."));
    }
    fs::create_dir(&target).map_err(|error| format!("Could not create the folder: {error}"))?;
    record_audit(&app, &id, "File", "Create folder", &relative);
    Ok(relative)
}

#[tauri::command(async)]
#[specta::specta]
async fn add_server_files(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    path: String,
    sources: Vec<String>,
    merge: bool,
) -> Result<u32, String> {
    let instance = instance_by_id(&app, &id)?;
    let folders: Vec<String> = sources.iter().filter(|one| PathBuf::from(one.trim()).is_dir()).cloned().collect();
    let mut count = 0_u32;
    if !folders.is_empty() {
        count += put_folders(&app, &state, &instance, &path, &folders, merge).await?;
        record_audit(
            &app,
            &id,
            "File",
            if instance.remote_host.is_empty() { "Add folder" } else { "Add folder (SSH)" },
            &format!("{} · {count} files", folders.len()),
        );
    }
    let sources: Vec<String> = sources.into_iter().filter(|one| !PathBuf::from(one.trim()).is_dir()).collect();
    if sources.is_empty() {
        return Ok(count);
    }
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        let here = path.replace('\\', "/").trim().trim_matches('/').to_string();
        let have = pfs::list_dir(&api, &target, &here).await?;
        let mut jobs = Vec::new();
        for one in &sources {
            let source = PathBuf::from(one.trim());
            if !source.is_file() {
                continue;
            }
            let name = source.file_name().map(|one| one.to_string_lossy().to_string()).ok_or("Could not read the file name.")?;
            if have.iter().any(|one| one.name == name) {
                return Err(format!("{name} already exists. Rename or delete it first."));
            }
            jobs.push((source, name));
        }
        let mut added = 0_u32;
        for (source, name) in jobs {
            let bytes = fs::read(&source).map_err(|error| format!("Could not read {name}: {error}"))?;
            tell_upload(&app, &name, "uploading", 0, bytes.len() as u64);
            let sent = api.upload(&target, &here, &name, bytes).await;
            tell_upload(&app, &name, "done", 0, 0);
            sent?;
            added += 1;
        }
        record_audit(&app, &id, "File", "Add files (panel)", &format!("{added}"));
        return Ok(count + added);
    }
    if !instance.remote_host.is_empty() {
        let here = path.replace('\\', "/").trim().trim_matches('/').to_string();
        let mut added = 0_u32;
        for one in &sources {
            let source = PathBuf::from(one.trim());
            if !source.is_file() {
                continue;
            }
            let name = source.file_name().map(|one| one.to_string_lossy().to_string()).ok_or("Could not read the file name.")?;
            let at = if here.is_empty() { name.clone() } else { format!("{here}/{name}") };
            let bytes = fs::read(&source).map_err(|error| format!("Could not read {name}: {error}"))?;
            watched_upload(&app, &instance.remote_host, &instance.path, &at, &bytes, &name).await?;
            added += 1;
        }
        record_audit(&app, &id, "File", "Add files (SSH)", &format!("{added}"));
        return Ok(count + added);
    }
    let root = PathBuf::from(&instance.path);
    let here = path.replace('\\', "/").trim().trim_matches('/').to_string();

    let mut jobs = Vec::new();
    for one in &sources {
        let source = PathBuf::from(one.trim());
        if !source.is_file() {
            return Err(format!("{one} could not be found."));
        }
        let name = source.file_name().map(|value| value.to_string_lossy().to_string()).ok_or("Could not read the file name.")?;
        let relative = if here.is_empty() { name.clone() } else { format!("{here}/{name}") };
        let target = paths::resolve_new_in_server(&root, &relative).map_err(|error| error.to_string())?;
        if target.exists() {
            return Err(format!("{name} already exists. Rename or delete it first."));
        }
        jobs.push((source, target, name));
    }

    let mut done = 0;
    for (source, target, name) in jobs {
        fs::copy(&source, &target).map_err(|error| format!("Could not add {name}: {error}"))?;
        done += 1;
    }
    record_audit(&app, &id, "File", "Add files", &format!("{done}"));
    Ok(count + done)
}

#[tauri::command(async)]
#[specta::specta]
async fn add_server_uploads(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    path: String,
    uploads: Vec<String>,
) -> Result<u32, String> {
    let mut sources = Vec::new();
    for token in &uploads {
        sources.push(uploaded_path(&app, token)?);
    }
    let done =
        add_server_files(app.clone(), state, id, path, sources.iter().map(|one| one.to_string_lossy().to_string()).collect(), false).await;
    for one in &sources {
        unstash(one);
    }
    done
}

#[tauri::command(async)]
#[specta::specta]
async fn zip_server_paths(app: AppHandle, id: String, paths: Vec<String>, name: String) -> Result<String, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        if paths.is_empty() {
            return Err("Select something to zip.".into());
        }
        let (api, target) = panel_of(&app, &instance)?;
        let mut dir = None;
        let mut names = Vec::new();
        for one in &paths {
            let (here, name) = panel::split_path(one)?;
            if dir.get_or_insert_with(|| here.clone()) != &here {
                return Err("On the panel, only items in the same folder can be compressed together.".into());
            }
            names.push(name);
        }
        let dir = dir.unwrap_or_else(|| "/".into());
        let made = api.compress(&target, &dir, &names).await?;
        record_audit(&app, &id, "File", "Compress (panel)", &made);
        return Ok(made);
    }
    if !instance.remote_host.is_empty() {
        let name = plain_name(&name)?;
        let name = if name.to_ascii_lowercase().ends_with(".zip") { name.to_string() } else { format!("{name}.zip") };
        remote::zip_paths(&app, &instance.remote_host, &instance.path, &paths, &name).await?;
        record_audit(&app, &id, "File", "Compress (SSH)", &name);
        return Ok(name);
    }
    let root = PathBuf::from(&instance.path);
    let name = plain_name(&name)?;
    let name = if name.to_ascii_lowercase().ends_with(".zip") { name.to_string() } else { format!("{name}.zip") };
    if paths.is_empty() {
        return Err("Select something to zip.".into());
    }
    let mut picked = Vec::new();
    for one in &paths {
        picked.push(paths::resolve_in_server(&root, one).map_err(|error| error.to_string())?);
    }
    let parent = picked[0].parent().unwrap_or(&root).to_path_buf();
    let mut target = parent.join(&name);
    let stem = name.trim_end_matches(".zip").to_string();
    let mut counter = 2;
    while target.exists() {
        target = parent.join(format!("{stem}-{counter}.zip"));
        counter += 1;
    }
    let task = progress::begin(&app, "zipServerPaths", &id);
    let total: u64 = picked.iter().map(|one| tree_size(one)).sum();
    let mut done = 0;
    let file = fs::File::create(&target).map_err(|error| format!("Could not create the zip: {error}"))?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let outcome = (|| -> Result<(), String> {
        for one in &picked {
            zip_into_counted(&mut writer, &parent, one, &target, options, &task, &mut done, total)?;
        }
        writer.finish().map_err(|error| error.to_string())?;
        Ok(())
    })();
    if let Err(error) = outcome {
        let _ = fs::remove_file(&target);
        return Err(error);
    }
    let relative = target.strip_prefix(&root).unwrap_or(&target).to_string_lossy().replace('\\', "/");
    record_audit(&app, &id, "File", "Zip", &relative);
    Ok(relative)
}

fn zip_into<W: std::io::Write + std::io::Seek>(
    writer: &mut ZipWriter<W>,
    base: &Path,
    path: &Path,
    skip: &Path,
    options: SimpleFileOptions,
) -> Result<(), String> {
    zip_into_counted(writer, base, path, skip, options, &progress::Task::quiet(), &mut 0, 0)
}

/// Bytes of every file under a path (or the file itself).
fn tree_size(path: &Path) -> u64 {
    if path.is_dir() {
        fs::read_dir(path).map(|entries| entries.flatten().map(|entry| tree_size(&entry.path())).sum()).unwrap_or(0)
    } else {
        fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
    }
}

#[allow(clippy::too_many_arguments)]
fn zip_into_counted<W: std::io::Write + std::io::Seek>(
    writer: &mut ZipWriter<W>,
    base: &Path,
    path: &Path,
    skip: &Path,
    options: SimpleFileOptions,
    task: &progress::Task,
    done: &mut u64,
    total: u64,
) -> Result<(), String> {
    task.check()?;
    if path == skip {
        return Ok(());
    }
    let inside = path.strip_prefix(base).unwrap_or(path).to_string_lossy().replace('\\', "/");
    if path.is_dir() {
        writer.add_directory(format!("{inside}/"), options).map_err(|error| error.to_string())?;
        for entry in fs::read_dir(path).map_err(|error| error.to_string())?.flatten() {
            zip_into_counted(writer, base, &entry.path(), skip, options, task, done, total)?;
        }
        return Ok(());
    }
    let Ok(mut file) = fs::File::open(path) else {
        return Ok(());
    };
    writer.start_file(inside.as_str(), options).map_err(|error| error.to_string())?;
    task.copy(&mut file, writer, done, total, &inside).map_err(|error| {
        if error == progress::CANCELLED {
            error
        } else {
            format!("Could not include {}: {error}", path.display())
        }
    })?;
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
async fn unzip_server_path(app: AppHandle, state: State<'_, AppState>, id: String, path: String) -> Result<String, String> {
    {
        let instance = instance_by_id(&app, &id)?;
        if !instance.panel_host.is_empty() {
            let (api, target) = panel_of(&app, &instance)?;
            api.decompress(&target, &path).await?;
            record_audit(&app, &id, "File", "Extract (panel)", &path);
            return Ok(parent_of(&path).to_string());
        }
    }
    let (root, archive_path) = file_in_server(&app, &state, &id, &path)?;
    if !archive_path.is_file() || !archive_path.to_string_lossy().to_ascii_lowercase().ends_with(".zip") {
        return Err("Not a zip file.".into());
    }
    let stem = archive_path.file_stem().map(|one| one.to_string_lossy().to_string()).unwrap_or_else(|| "unzipped".into());
    let parent = archive_path.parent().unwrap_or(&root).to_path_buf();
    let mut into = parent.join(&stem);
    let mut counter = 2;
    while into.exists() {
        into = parent.join(format!("{stem}-{counter}"));
        counter += 1;
    }
    let file = fs::File::open(&archive_path).map_err(|error| format!("Could not open the zip: {error}"))?;
    let mut archive = ZipArchive::new(file).map_err(|error| format!("The zip is corrupt: {error}"))?;
    zips::check_budget(&mut archive, zips::MAX_UNPACKED, zips::MAX_ENTRIES)?;
    let task = progress::begin(&app, "unzipServerPath", &id);
    let total: u64 = (0..archive.len()).filter_map(|at| archive.by_index_raw(at).ok().map(|item| item.size())).sum();
    fs::create_dir_all(&into).map_err(|error| error.to_string())?;
    let mut done = 0;
    let outcome = (|| -> Result<(), String> {
        for at in 0..archive.len() {
            let mut item = archive.by_index(at).map_err(|error| error.to_string())?;
            let Some(inside) = item.enclosed_name() else { continue };
            let target = into.join(&inside);
            if item.is_dir() {
                let _ = fs::create_dir_all(&target);
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut out = fs::File::create(&target).map_err(|error| error.to_string())?;
            task.copy(&mut item, &mut out, &mut done, total, &inside.to_string_lossy())?;
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        // The folder is new, made just now for this zip.
        let _ = fs::remove_dir_all(&into);
        return Err(error);
    }
    let relative = into.strip_prefix(&root).unwrap_or(&into).to_string_lossy().replace('\\', "/");
    record_audit(&app, &id, "File", "Unzip", &relative);
    Ok(relative)
}

const HANDOUT_LIMIT: u64 = 64 * 1024 * 1024;

#[tauri::command(async)]
#[specta::specta]
async fn read_server_file_download(app: AppHandle, id: String, path: String) -> Result<Handed, String> {
    use base64::Engine;
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        let folder = api.exists(&target, &path).await? == Some(true);
        let wanted = if folder {
            let (dir, name) = panel::split_path(&path)?;
            let made = api.compress(&target, &dir, &[name]).await?;
            if dir == "/" {
                made
            } else {
                format!("{}/{made}", dir.trim_start_matches('/'))
            }
        } else {
            path.clone()
        };
        let bytes = api.read_bytes(&target, &wanted, HANDOUT_LIMIT).await;
        if folder {
            let _ = api.delete(&target, std::slice::from_ref(&wanted)).await;
        }
        let bytes = bytes?;
        return Ok(Handed {
            name: wanted.rsplit('/').next().unwrap_or("File").to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(&bytes),
        });
    }
    if !instance.remote_host.is_empty() {
        let (wanted, packed) = match remote::is_dir_checked(&app, &instance.remote_host, &instance.path, &path).await? {
            true => (remote::pack_for_download(&app, &instance.remote_host, &instance.path, &path).await?, true),
            false => (path.clone(), false),
        };
        let bytes = remote::read_bytes(&app, &instance.remote_host, &instance.path, &wanted, HANDOUT_LIMIT).await;
        if packed {
            remote::drop_path(&app, &instance.remote_host, &instance.path, &wanted).await;
        }
        let bytes = bytes?;
        return Ok(Handed {
            name: wanted.rsplit('/').next().unwrap_or("File").to_string(),
            data: base64::engine::general_purpose::STANDARD.encode(&bytes),
        });
    }
    let target = paths::resolve_in_server(Path::new(&instance.path), &path).map_err(|error| error.to_string())?;
    if !target.is_file() {
        return Err("Not a file.".into());
    }
    let size = target.metadata().map_err(|error| error.to_string())?.len();
    if size > HANDOUT_LIMIT {
        return Err("Files larger than 64 MB cannot be downloaded through the browser.".into());
    }
    let bytes = fs::read(&target).map_err(|error| format!("Could not read the file: {error}"))?;
    Ok(Handed {
        name: target.file_name().map(|one| one.to_string_lossy().to_string()).unwrap_or_else(|| "file".into()),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

#[tauri::command(async)]
#[specta::specta]
async fn copy_server_file_out(app: AppHandle, id: String, path: String, to: String) -> Result<(), String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        let shown = path.rsplit('/').next().unwrap_or(&path).to_string();
        let folder = api.exists(&target, &path).await? == Some(true);
        let wanted = if folder {
            tell_job(&app, "download", &shown, "taring", 0, 0);
            let (dir, name) = panel::split_path(&path)?;
            let made = api.compress(&target, &dir, &[name]).await?;
            if dir == "/" {
                made
            } else {
                format!("{}/{made}", dir.trim_start_matches('/'))
            }
        } else {
            path.clone()
        };
        let last = std::sync::atomic::AtomicU64::new(u64::MAX);
        let got = api
            .download_to(&target, &wanted, Path::new(to.trim()), |got, whole| {
                let step = (got * 100).checked_div(whole).unwrap_or(0);
                if last.swap(step, std::sync::atomic::Ordering::Relaxed) != step {
                    tell_job(&app, "download", &shown, "downloading", got, whole);
                }
            })
            .await;
        if folder {
            let _ = api.delete(&target, std::slice::from_ref(&wanted)).await;
        }
        tell_job(&app, "download", &shown, "done", 0, 0);
        got?;
        record_audit(&app, &id, "File", "Download file (panel)", &path);
        return Ok(());
    }
    if !instance.remote_host.is_empty() {
        let shown = path.rsplit('/').next().unwrap_or(&path).to_string();
        let folder = remote::is_dir_checked(&app, &instance.remote_host, &instance.path, &path).await?;
        let wanted = if folder {
            tell_job(&app, "download", &shown, "taring", 0, 0);
            remote::pack_for_download(&app, &instance.remote_host, &instance.path, &path).await?
        } else {
            path.clone()
        };
        let last = std::sync::atomic::AtomicU64::new(u64::MAX);
        let got = remote::download_to(&app, &instance.remote_host, &instance.path, &wanted, Path::new(to.trim()), |got, whole| {
            let step = (got * 100).checked_div(whole).unwrap_or(0);
            if last.swap(step, std::sync::atomic::Ordering::Relaxed) != step {
                tell_job(&app, "download", &shown, "downloading", got, whole);
            }
        })
        .await;
        if folder {
            remote::drop_path(&app, &instance.remote_host, &instance.path, &wanted).await;
        }
        tell_job(&app, "download", &shown, "done", 0, 0);
        got?;
        record_audit(&app, &id, "File", "Download file (SSH)", &path);
        return Ok(());
    }
    let target = paths::resolve_in_server(Path::new(&instance.path), &path).map_err(|error| error.to_string())?;
    if !target.is_file() {
        return Err("Not a file.".into());
    }
    fs::copy(&target, PathBuf::from(to.trim())).map_err(|error| format!("Could not save the file: {error}"))?;
    Ok(())
}

fn process_tree(system: &System, root: Pid) -> HashSet<Pid> {
    let mut targets: HashSet<Pid> = HashSet::new();
    targets.insert(root);
    for (pid, process) in system.processes() {
        let mut current = process.parent();
        let mut depth = 0;
        while let Some(parent) = current {
            if depth > 16 {
                break;
            }
            if parent == root {
                targets.insert(*pid);
                break;
            }
            current = system.process(parent).and_then(|parent| parent.parent());
            depth += 1;
        }
    }
    targets
}

fn process_tree_usage(system: &System, root: Pid) -> (f32, u64) {
    let targets = process_tree(system, root);
    let mut cpu = 0.0_f32;
    let mut memory = 0_u64;
    for pid in &targets {
        if let Some(process) = system.process(*pid) {
            cpu += process.cpu_usage();
            memory = memory.saturating_add(process.memory());
        }
    }
    let cores = thread::available_parallelism().map(|value| value.get()).unwrap_or(1).max(1) as f32;
    ((cpu / cores).clamp(0.0, 100.0), memory)
}

#[tauri::command(async)]
#[specta::specta]
fn system_memory_mib(state: State<'_, AppState>) -> u64 {
    let mut system = state.system.lock().expect("system state poisoned");
    system.refresh_memory();
    system.total_memory() / 1024 / 1024
}

#[tauri::command(async)]
#[specta::specta]
fn panel_usage(state: State<'_, AppState>, id: String) -> Option<panel::PanelStats> {
    state.panel_live.lock().expect("panel state poisoned").get(&id).map(|one| one.stats.clone())
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ProcessDetails {
    cores: Vec<f32>,
    disk_read_kib: f64,
    disk_write_kib: f64,
    processes: u32,
    run_seconds: u64,
    system_memory_used_mib: u64,
    system_memory_total_mib: u64,
    swap_used_mib: u64,
    swap_total_mib: u64,
}

static DISK_TOTALS: Mutex<Option<(String, Instant, u64, u64)>> = Mutex::new(None);

/// Machine-side detail for the advanced monitor (local servers only; far servers return None).
#[tauri::command]
#[specta::specta]
fn process_details(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<Option<ProcessDetails>, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() || !instance.remote_host.is_empty() {
        return Ok(None);
    }
    let Some(pid) = state.processes.lock().expect("process state poisoned").get(&id).map(|process| process.pid) else {
        return Ok(None);
    };
    let mut system = state.system.lock().expect("system state poisoned");
    system.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, sysinfo::ProcessRefreshKind::nothing().with_disk_usage());
    system.refresh_cpu_usage();
    system.refresh_memory();
    let root = Pid::from_u32(pid);
    let tree = process_tree(&system, root);
    let (mut read, mut written) = (0_u64, 0_u64);
    for one in &tree {
        if let Some(process) = system.process(*one) {
            let usage = process.disk_usage();
            read = read.saturating_add(usage.total_read_bytes);
            written = written.saturating_add(usage.total_written_bytes);
        }
    }
    let now = Instant::now();
    let (disk_read_kib, disk_write_kib) = {
        let mut kept = DISK_TOTALS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let rate = match kept.as_ref() {
            Some((was, at, before_read, before_written)) if *was == id => {
                let seconds = now.duration_since(*at).as_secs_f64().max(0.5);
                (
                    read.saturating_sub(*before_read) as f64 / 1024.0 / seconds,
                    written.saturating_sub(*before_written) as f64 / 1024.0 / seconds,
                )
            }
            _ => (0.0, 0.0),
        };
        *kept = Some((id.clone(), now, read, written));
        rate
    };
    Ok(Some(ProcessDetails {
        cores: system.cpus().iter().map(|cpu| cpu.cpu_usage()).collect(),
        disk_read_kib,
        disk_write_kib,
        processes: tree.len() as u32,
        run_seconds: system.process(root).map(|process| process.run_time()).unwrap_or(0),
        system_memory_used_mib: system.used_memory() / 1_048_576,
        system_memory_total_mib: system.total_memory() / 1_048_576,
        swap_used_mib: system.used_swap() / 1_048_576,
        swap_total_mib: system.total_swap() / 1_048_576,
    }))
}

#[tauri::command(async)]
#[specta::specta]
async fn get_resource_snapshot(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<ResourceSnapshot, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        let live = state.panel_live.lock().expect("panel state poisoned").get(&id).cloned();
        let stats = live.map(|one| one.stats).unwrap_or_default();
        let on = stats.state != "offline" && !stats.state.is_empty();
        if on {
            record_far_sample(&state, &id, stats.cpu as f32, (stats.memory_bytes / 1_048_576.0) as u64);
        }
        return Ok(ResourceSnapshot {
            cpu_percent: if on { stats.cpu as f32 } else { 0.0 },
            memory_mib: if on { (stats.memory_bytes / 1_048_576.0) as u64 } else { 0 },
            disk_free_mib: 0,
            disk_total_mib: (stats.disk_bytes / 1_048_576.0) as u64,
        });
    }
    if !instance.remote_host.is_empty() {
        let pid = state.remote_running.lock().expect("remote state poisoned").get(&id).map(|live| live.pid).unwrap_or(0);
        let (cpu, memory) = if pid == 0 { (0.0, 0) } else { remote::resource(&app, &instance.remote_host, pid).await.unwrap_or((0.0, 0)) };
        if pid != 0 {
            record_far_sample(&state, &id, cpu as f32, memory);
        }
        let (disk_free_mib, disk_total_mib) = remote::disk_free_mib(&app, &instance.remote_host, &instance.path).await;
        return Ok(ResourceSnapshot { cpu_percent: cpu as f32, memory_mib: memory, disk_free_mib, disk_total_mib });
    }
    let pid = state.processes.lock().expect("process state poisoned").get(&id).map(|process| process.pid);
    let (cpu_percent, memory_mib) = {
        let mut system = state.system.lock().expect("system state poisoned");
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        pid.map(|pid| process_tree_usage(&system, Pid::from_u32(pid))).map(|(cpu, memory)| (cpu, memory / 1024 / 1024)).unwrap_or((0.0, 0))
    };
    let instance_path = Path::new(&instance.path).canonicalize().map(plain_path).map_err(|error| error.to_string())?;
    let disks = Disks::new_with_refreshed_list();
    let disk = disks
        .list()
        .iter()
        .filter(|disk| instance_path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len());
    let (disk_free_mib, disk_total_mib) =
        disk.map(|disk| (disk.available_space() / 1024 / 1024, disk.total_space() / 1024 / 1024)).unwrap_or((0, 0));
    if pid.is_some() {
        let tps = state.tps.lock().expect("tps state poisoned").get(&id).and_then(|snapshot| snapshot.tps);
        record_resource_sample(&state, &id, cpu_percent, memory_mib, tps);
    }
    Ok(ResourceSnapshot { cpu_percent, memory_mib, disk_free_mib, disk_total_mib })
}

#[tauri::command(async)]
#[specta::specta]
fn get_tps_snapshot(_app: AppHandle, state: State<'_, AppState>, id: String) -> Result<Option<TpsSnapshot>, String> {
    const FRESH_SECS: u64 = 60;
    Ok(state
        .tps
        .lock()
        .expect("tps state poisoned")
        .get(&id)
        .filter(|snapshot| unix_now().saturating_sub(snapshot.updated_at) <= FRESH_SECS)
        .cloned())
}

#[tauri::command(async)]
#[specta::specta]
fn get_server_ping(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<PingSnapshot, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.remote_host.is_empty() {
        let running = state.remote_running.lock().expect("remote state poisoned").contains_key(&id);
        if !running {
            return Err("Player counts are only available for running servers.".into());
        }
        let host = remote::load_hosts(&app)?.into_iter().find(|one| one.id == instance.remote_host).ok_or("SSH connection not found.")?;
        return ping::query_at(&host.address, port_from_properties(&instance));
    }
    if !state.processes.lock().expect("process state poisoned").contains_key(&id) {
        return Err("Player counts are only available for servers started by the app.".into());
    }
    query_server_ping(port_from_properties(&instance))
}

fn read_player_file(path: PathBuf, source: &str) -> Vec<PlayerEntry> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    players_from_json(&text, source)
}

fn players_from_json(text: &str, source: &str) -> Vec<PlayerEntry> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let object = item.as_object()?;
            let name = object.get("name").or_else(|| object.get("Name"))?.as_str()?.to_string();
            let uuid = object.get("uuid").or_else(|| object.get("UUID")).and_then(|value| value.as_str()).unwrap_or("—").to_string();
            Some(PlayerEntry {
                name,
                uuid,
                whitelisted: source == "whitelist",
                operator: source == "ops",
                banned: source == "banned",
                source: source.into(),
            })
        })
        .collect()
}

#[tauri::command(async)]
#[specta::specta]
fn player_lists_dir(instance: &ServerInstance) -> PathBuf {
    let folder = Path::new(&instance.path);
    if pumpkin::is_pumpkin(&instance.core) {
        pumpkin::player_lists_dir(folder)
    } else {
        folder.to_path_buf()
    }
}

#[tauri::command(async)]
#[specta::specta]
async fn list_players(app: AppHandle, id: String) -> Result<Vec<PlayerEntry>, String> {
    {
        let instance = instance_by_id(&app, &id)?;
        if is_far(&instance) {
            let base = if pumpkin::is_pumpkin(&instance.core) { "data/" } else { "" };
            let mut by_uuid: HashMap<String, PlayerEntry> = HashMap::new();
            for (file, source) in
                [("usercache.json", "known"), ("whitelist.json", "whitelist"), ("ops.json", "ops"), ("banned-players.json", "banned")]
            {
                let text = far_read(&app, &instance, &format!("{base}{file}")).await.unwrap_or_default();
                for player in players_from_json(&text, source) {
                    let entry = by_uuid.entry(player.uuid.clone()).or_insert(player.clone());
                    entry.whitelisted |= player.whitelisted;
                    entry.operator |= player.operator;
                    entry.banned |= player.banned;
                    if entry.name.is_empty() {
                        entry.name = player.name;
                    }
                }
            }
            let mut players: Vec<_> = by_uuid.into_values().collect();
            players.sort_by_key(|one| one.name.to_lowercase());
            return Ok(players);
        }
    }
    let instance = instance_by_id(&app, &id)?;
    let folder = player_lists_dir(&instance);
    let folder = folder.as_path();
    let mut by_uuid: HashMap<String, PlayerEntry> = HashMap::new();
    for (file, source) in
        [("usercache.json", "known"), ("whitelist.json", "whitelist"), ("ops.json", "ops"), ("banned-players.json", "banned")]
    {
        for player in read_player_file(folder.join(file), source) {
            let entry = by_uuid.entry(player.uuid.clone()).or_insert(player.clone());
            entry.whitelisted |= player.whitelisted;
            entry.operator |= player.operator;
            entry.banned |= player.banned;
            if entry.name.is_empty() {
                entry.name = player.name;
            }
        }
    }
    let mut players: Vec<_> = by_uuid.into_values().collect();
    players.sort_by_key(|a| a.name.to_lowercase());
    Ok(players)
}

#[tauri::command(async)]
#[specta::specta]
async fn list_player_activity(app: AppHandle, id: String) -> Result<Vec<PlayerActivityEntry>, String> {
    let instance = instance_by_id(&app, &id)?;
    let (text, last_seen_at) = if is_far(&instance) {
        let seen =
            far_log_files(&app, &instance).await.into_iter().find(|one| one.name == "latest.log").map(|one| one.modified_at).unwrap_or(0);
        (far_read_log(&app, &instance, "latest.log", 20_000).await.unwrap_or_default(), seen)
    } else {
        let log = Path::new(&instance.path).join("logs").join("latest.log");
        let seen = fs::metadata(&log)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|time| time.as_secs())
            .unwrap_or(0);
        (fs::read_to_string(log).unwrap_or_default(), seen)
    };
    let mut sessions: HashMap<String, u32> = HashMap::new();
    {
        for line in text.lines() {
            let Some(prefix) = line.split(" joined the game").next() else {
                continue;
            };
            if !line.contains(" joined the game") {
                continue;
            }
            let name = prefix.rsplit("]: ").next().unwrap_or_default().trim();
            if !name.is_empty() && !name.contains(' ') {
                *sessions.entry(name.into()).or_default() += 1;
            }
        }
    }
    let mut activity =
        sessions.into_iter().map(|(name, sessions)| PlayerActivityEntry { name, sessions, last_seen_at }).collect::<Vec<_>>();
    activity.sort_by(|left, right| right.sessions.cmp(&left.sessions).then_with(|| left.name.cmp(&right.name)));
    Ok(activity)
}

fn update_player_list(folder: &Path, file_name: &str, name: &str, uuid: &str, action: &str) -> Result<bool, String> {
    if uuid == "—" || uuid.trim().is_empty() {
        return Err("This player's UUID was not found, so offline permissions cannot be changed.".into());
    }
    let path = folder.join(file_name);
    let before = fs::read_to_string(&path).unwrap_or_default();
    let (text, added) = player_list_edit(&before, name, uuid, action)?;
    fs::write(path, text).map_err(|error| error.to_string())?;
    Ok(added)
}

async fn toggle_player_access_remote(
    app: &AppHandle,
    state: &AppState,
    instance: &ServerInstance,
    input: &PlayerAccessInput,
) -> Result<PlayerAccessResult, String> {
    let name = input.name.trim();
    let running = if instance.panel_host.is_empty() {
        state.remote_running.lock().expect("remote state poisoned").contains_key(&input.id)
    } else {
        state.panel_live.lock().expect("panel state poisoned").get(&input.id).is_some_and(|one| one.stats.state == "running")
    };
    let send = async |line: String, what: &str| -> Result<(), String> {
        let line = launch::console_line(&line)?;
        if !instance.panel_host.is_empty() {
            let (api, target) = panel_of(app, instance)?;
            api.command(&target, &line).await?;
        } else {
            remote::send_line(app, &instance.remote_host, &input.id, &line, session_of(state, &input.id)).await?;
        }
        record_audit(app, &input.id, "Player", what, &line);
        Ok(())
    };
    if input.action == "kick" {
        if name.is_empty() {
            return Err("A player name is required to kick.".into());
        }
        if !running {
            return Err("Players can only be kicked while the server is running.".into());
        }
        send(format!("kick {name}"), "Kick (SSH)").await?;
        return Ok(PlayerAccessResult { via_console: true, added: false });
    }
    let (file_name, add_command, remove_command) = match input.action.as_str() {
        "whitelist" => ("whitelist.json", "whitelist add", "whitelist remove"),
        "operator" => ("ops.json", "op", "deop"),
        "ban" => ("banned-players.json", "ban", "pardon"),
        _ => return Err("Unsupported player permission action.".into()),
    };
    let relative = if pumpkin::is_pumpkin(&instance.core) { format!("data/{file_name}") } else { file_name.to_string() };
    let text = far_read(app, instance, &relative).await.unwrap_or_default();
    let entries = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|value| value.as_array().cloned()).unwrap_or_default();
    let present = entries.iter().any(|entry| {
        let uuid = entry.get("uuid").or_else(|| entry.get("UUID")).and_then(|value| value.as_str());
        (!input.uuid.trim().is_empty() && uuid == Some(input.uuid.as_str()))
            || (input.uuid.trim().is_empty()
                && !name.is_empty()
                && entry.get("name").and_then(|value| value.as_str()).is_some_and(|found| found.eq_ignore_ascii_case(name)))
    });
    if running {
        if name.is_empty() {
            return Err("A player name is required while the server is running.".into());
        }
        let verb = if present { remove_command } else { add_command };
        send(format!("{verb} {name}"), "Change permissions (SSH)").await?;
        return Ok(PlayerAccessResult { via_console: true, added: !present });
    }
    let mut uuid = input.uuid.clone();
    if uuid.trim().is_empty() || uuid == "—" {
        if name.is_empty() {
            return Err("A player name is required.".into());
        }
        let cache = mojang_cache_dir(app)?;
        let looked = off_runtime(|| mojang::lookup_by_name(&cache, name))
            .map_err(|error| format!("No UUID found for {name}. Make sure it is a real account name. ({error})"))?;
        uuid = looked.uuid;
    }
    let (next, added) = player_list_edit(&text, name, &uuid, &input.action)?;
    far_write(app, instance, &relative, &next).await?;
    record_audit(app, &input.id, "Player", "Change permissions (SSH)", &format!("{file_name} — {name}"));
    Ok(PlayerAccessResult { via_console: false, added })
}

fn player_list_edit(before: &str, name: &str, uuid: &str, action: &str) -> Result<(String, bool), String> {
    let mut entries =
        serde_json::from_str::<serde_json::Value>(before).ok().and_then(|value| value.as_array().cloned()).unwrap_or_default();
    let position =
        entries.iter().position(|entry| entry.get("uuid").or_else(|| entry.get("UUID")).and_then(|value| value.as_str()) == Some(uuid));
    if let Some(index) = position {
        entries.remove(index);
    } else {
        let entry = match action {
            "operator" => {
                serde_json::json!({ "uuid": uuid, "name": name, "level": 4, "bypassesPlayerLimit": false })
            }
            "ban" => {
                serde_json::json!({ "uuid": uuid, "name": name, "created": "Monolab", "source": "Monolab", "expires": "forever", "reason": "Banned by Monolab" })
            }
            _ => serde_json::json!({ "uuid": uuid, "name": name }),
        };
        entries.push(entry);
    }
    let text = serde_json::to_string_pretty(&entries).map_err(|error| error.to_string())?;
    Ok((format!("{text}\n"), position.is_none()))
}

fn player_named_in_list(folder: &Path, file_name: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    fs::read_to_string(folder.join(file_name))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .any(|entry| entry.get("name").and_then(|value| value.as_str()).is_some_and(|found| found.eq_ignore_ascii_case(name)))
}

fn player_in_list(folder: &Path, file_name: &str, uuid: &str) -> bool {
    fs::read_to_string(folder.join(file_name))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .any(|entry| entry.get("uuid").or_else(|| entry.get("UUID")).and_then(|value| value.as_str()) == Some(uuid))
}

#[tauri::command(async)]
#[specta::specta]
async fn toggle_player_access(app: AppHandle, state: State<'_, AppState>, input: PlayerAccessInput) -> Result<PlayerAccessResult, String> {
    let instance = instance_by_id(&app, &input.id)?;
    if is_far(&instance) {
        return toggle_player_access_remote(&app, &state, &instance, &input).await;
    }
    let folder = player_lists_dir(&instance);
    let folder = folder.as_path();
    let name = input.name.trim();

    if input.action == "kick" {
        if name.is_empty() {
            return Err("A player name is required to kick.".into());
        }
        if !send_process_command(&state, &input.id, &format!("kick {name}"))? {
            return Err("Players can only be kicked while the server is running.".into());
        }
        record_audit(&app, &input.id, "Player", "Export", name);
        return Ok(PlayerAccessResult { via_console: true, added: false });
    }

    let (file_name, add_command, remove_command) = match input.action.as_str() {
        "whitelist" => ("whitelist.json", "whitelist add", "whitelist remove"),
        "operator" => ("ops.json", "op", "deop"),
        "ban" => ("banned-players.json", "ban", "pardon"),
        _ => return Err("Unsupported player permission action.".into()),
    };
    let present =
        player_in_list(folder, file_name, &input.uuid) || (input.uuid.trim().is_empty() && player_named_in_list(folder, file_name, name));
    let running = state.processes.lock().expect("process state poisoned").contains_key(&input.id);

    if running {
        if name.is_empty() {
            return Err("A player name is required while the server is running.".into());
        }
        let verb = if present { remove_command } else { add_command };
        if !send_process_command(&state, &input.id, &format!("{verb} {name}"))? {
            return Err("Could not send the command to the server console.".into());
        }
        record_audit(&app, &input.id, "Player", "Role change", &format!("{verb} {name}"));
        return Ok(PlayerAccessResult { via_console: true, added: !present });
    }

    let mut uuid = input.uuid.clone();
    if uuid.trim().is_empty() || uuid == "—" {
        if name.is_empty() {
            return Err("A player name is required.".into());
        }
        let cache = mojang_cache_dir(&app)?;
        let looked = off_runtime(|| mojang::lookup_by_name(&cache, name))
            .map_err(|error| format!("No UUID found for {name}. Make sure it is a real account name. ({error})"))?;
        uuid = looked.uuid;
    }
    let added = update_player_list(folder, file_name, name, &uuid, &input.action)?;
    record_audit(&app, &input.id, "Player", "Role change", &format!("{file_name} · {}", if added { "Add" } else { "Remove" }));
    Ok(PlayerAccessResult { via_console: false, added })
}

fn addon_compatibility(file_name: &str, kind: &str, client_only: bool) -> String {
    if kind == "plugin" {
        return String::new();
    }
    let lowered = file_name.to_lowercase();
    let known_client_only = ["optifine", "sodium", "iris", "xaero", "journeymap", "minimap", "litematica", "replaymod", "entityculling"];
    if known_client_only.iter().any(|needle| lowered.contains(needle)) {
        return "May be client only".into();
    }
    if client_only {
        return "Client only".into();
    }
    String::new()
}

fn modrinth_loader(instance: &ServerInstance, _project_type: &str) -> Result<(&'static str, &'static str), String> {
    match instance.core.as_str() {
        "Quilt" => Ok(("mods", "quilt")),
        _ => instance_loader(instance),
    }
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AddonVersion {
    id: String,
    name: String,
    channel: String,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    fits: bool,
    changelog: Option<String>,
    changelog_html: Option<String>,
    published: String,
    downloads: u64,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ProjectDetail {
    project_id: String,
    title: String,
    description: String,
    body_html: String,
    icon_url: Option<String>,
    gallery: Vec<GalleryItem>,
    categories: Vec<String>,
    loaders: Vec<String>,
    game_versions: Vec<String>,
    downloads: u64,
    followers: u64,
    published: String,
    updated: String,
    license: Option<String>,
    source_url: Option<String>,
    issues_url: Option<String>,
    wiki_url: Option<String>,
    discord_url: Option<String>,
    page_url: String,
    team: Vec<String>,
    client_side: String,
    server_side: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct GalleryItem {
    url: String,
    title: Option<String>,
    description: Option<String>,
    featured: bool,
}

#[tauri::command(async)]
#[specta::specta]
fn modrinth_project_detail(project_id: String) -> Result<ProjectDetail, String> {
    off_runtime(move || {
        let client = crate::net::builder()
            .user_agent("Monolab/0.1.0 (local Windows desktop app)")
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| error.to_string())?;
        let project: serde_json::Value = client
            .get(format!("https://api.modrinth.com/v2/project/{project_id}"))
            .send()
            .map_err(|error| format!("Could not fetch the project details: {error}"))?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .map_err(|error| error.to_string())?;
        let text = |key: &str| project.get(key).and_then(|value| value.as_str()).map(str::to_owned);
        let list = |key: &str| -> Vec<String> {
            project
                .get(key)
                .and_then(|value| value.as_array())
                .into_iter()
                .flatten()
                .filter_map(|one| one.as_str().map(str::to_owned))
                .collect()
        };
        let count = |key: &str| project.get(key).and_then(serde_json::Value::as_u64).unwrap_or(0);
        let slug = text("slug").unwrap_or_else(|| project_id.clone());
        let kind = text("project_type").unwrap_or_else(|| "mod".into());
        let team = client
            .get(format!("https://api.modrinth.com/v2/project/{project_id}/members"))
            .send()
            .ok()
            .and_then(|response| response.error_for_status().ok())
            .and_then(|response| response.json::<serde_json::Value>().ok())
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|member| member.pointer("/user/username").and_then(|value| value.as_str()).map(str::to_owned))
            .collect();
        Ok(ProjectDetail {
            title: text("title").unwrap_or_else(|| "Unnamed".into()),
            description: text("description").unwrap_or_default(),
            body_html: markdown::render(&text("body").unwrap_or_default()),
            icon_url: text("icon_url").filter(|value| !value.is_empty()),
            gallery: project
                .get("gallery")
                .and_then(|value| value.as_array())
                .into_iter()
                .flatten()
                .filter_map(|item| {
                    Some(GalleryItem {
                        url: item.get("url")?.as_str()?.to_string(),
                        title: item.get("title").and_then(|value| value.as_str()).map(str::to_owned),
                        description: item.get("description").and_then(|value| value.as_str()).map(str::to_owned),
                        featured: item.get("featured").and_then(|value| value.as_bool()).unwrap_or(false),
                    })
                })
                .collect(),
            categories: list("categories"),
            loaders: list("loaders"),
            game_versions: list("game_versions"),
            downloads: count("downloads"),
            followers: count("followers"),
            published: text("published").unwrap_or_default(),
            updated: text("updated").unwrap_or_default(),
            license: project.pointer("/license/id").and_then(|value| value.as_str()).map(str::to_owned),
            source_url: text("source_url"),
            issues_url: text("issues_url"),
            client_side: text("client_side").unwrap_or_default(),
            server_side: text("server_side").unwrap_or_default(),
            wiki_url: text("wiki_url"),
            discord_url: text("discord_url"),
            page_url: format!("https://modrinth.com/{kind}/{slug}"),
            team,
            project_id,
        })
    })
}

#[tauri::command(async)]
#[specta::specta]
fn list_addon_versions(app: AppHandle, id: String, project_id: String, project_type: String) -> Result<Vec<AddonVersion>, String> {
    off_runtime(move || {
        let instance = instance_by_id(&app, &id)?;
        let (_, loader) = modrinth_loader(&instance, &project_type)?;
        let client = crate::net::builder()
            .user_agent("Monolab/0.1.0 (local Windows desktop app)")
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| error.to_string())?;
        let releases: serde_json::Value = client
            .get(format!("https://api.modrinth.com/v2/project/{project_id}/version"))
            .send()
            .map_err(|error| format!("Could not fetch the version list: {error}"))?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .map_err(|error| error.to_string())?;
        let list = |item: &serde_json::Value, key: &str| -> Vec<String> {
            item.get(key)
                .and_then(|value| value.as_array())
                .map(|items| items.iter().filter_map(|one| one.as_str()).map(str::to_owned).collect())
                .unwrap_or_default()
        };
        Ok(releases
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let game_versions = list(item, "game_versions");
                let loaders = list(item, "loaders");
                Some(AddonVersion {
                    id: item.get("id")?.as_str()?.to_string(),
                    name: item.get("version_number").and_then(|value| value.as_str()).unwrap_or("Unnamed").to_string(),
                    channel: item.get("version_type").and_then(|value| value.as_str()).unwrap_or("release").to_string(),
                    fits: game_versions.iter().any(|one| one == &instance.version) && loaders.iter().any(|one| one == loader),
                    changelog: item
                        .get("changelog")
                        .and_then(|value| value.as_str())
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned),
                    changelog_html: item
                        .get("changelog")
                        .and_then(|value| value.as_str())
                        .map(markdown::render)
                        .filter(|value| !value.is_empty()),
                    published: item.get("date_published").and_then(|value| value.as_str()).unwrap_or_default().to_string(),
                    downloads: item.get("downloads").and_then(serde_json::Value::as_u64).unwrap_or(0),
                    game_versions,
                    loaders,
                })
            })
            .collect())
    })
}

/// The Discord webhook for alerts: whether one is set (never the address itself) and what it sends.
#[tauri::command]
#[specta::specta]
fn webhook_info(app: AppHandle) -> webhook::WebhookInfo {
    webhook::info(&app)
}

/// `url`: null keeps the saved address, "" forgets it. `events` from crash, lag, join, leave, task, server;
/// `servers` empty = every server.
#[tauri::command]
#[specta::specta]
fn set_webhook(app: AppHandle, url: Option<String>, events: Vec<String>, servers: Vec<String>) -> Result<webhook::WebhookInfo, String> {
    webhook::save(&app, url, events, servers)
}

#[tauri::command(async)]
#[specta::specta]
fn test_webhook(app: AppHandle) -> Result<(), String> {
    webhook::test(&app)
}

/// Backup copies to S3-compatible storage: the settings (never the secret key).
#[tauri::command]
#[specta::specta]
fn cloud_info(app: AppHandle) -> cloud::CloudInfo {
    cloud::info(&app)
}

/// `secret`: null keeps the saved one.
#[tauri::command]
#[specta::specta]
fn set_cloud(app: AppHandle, info: cloud::CloudInfo, secret: Option<String>) -> Result<cloud::CloudInfo, String> {
    cloud::save(&app, info, secret)
}

/// Writes a small test file to the bucket; returns where it went.
#[tauri::command(async)]
#[specta::specta]
fn test_cloud(app: AppHandle) -> Result<String, String> {
    cloud::test(&app)
}

/// Geyser and Floodgate in one go, so Bedrock players can join. Returns what to tell the user.
#[tauri::command(async)]
#[specta::specta]
fn install_geyser(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<String, String> {
    let instance = instance_by_id(&app, &id)?;
    let done = || {
        format!("Geyser and Floodgate are installed. Restart the server, then Bedrock players join on port {} (UDP).", geyser::BEDROCK_PORT)
    };
    if geyser::modded(&instance.core) {
        install_modrinth(app.clone(), state.clone(), id.clone(), geyser::GEYSER_MODRINTH.into(), "mod".into(), None)?;
        install_modrinth(app, state, id, geyser::FLOODGATE_MODRINTH.into(), "mod".into(), None)?;
        return Ok(done());
    }
    let Some((geyser_on, floodgate_on)) = geyser::platforms(&instance.core) else {
        return Err("Geyser runs on Paper, Purpur, Folia, Velocity, BungeeCord, Fabric and NeoForge servers.".into());
    };
    let proxy = geyser::proxy(&instance.core);
    if !proxy && geyser::older(&instance.version, geyser::OLDEST) {
        return Err(format!("Geyser needs Minecraft {} or newer. Upgrade the server first.", geyser::OLDEST));
    }
    let latest = if proxy { None } else { off_runtime(latest_release).ok() };
    let needs_via = latest.as_deref().is_some_and(|newest| geyser::older(&instance.version, newest));
    let installed = off_runtime(|| {
        let running = is_running(&state, &id);
        let client = crate::net::builder()
            .user_agent(net::USER_AGENT)
            .timeout(Duration::from_secs(90))
            .build()
            .map_err(|error| error.to_string())?;
        for (project, platform) in [("geyser", geyser_on), ("floodgate", floodgate_on)] {
            let build: serde_json::Value = client
                .get(format!("{}/{project}/versions/latest/builds/latest", geyser::API))
                .send()
                .and_then(|answer| answer.error_for_status())
                .and_then(|answer| answer.json())
                .map_err(|error| format!("Could not reach GeyserMC: {error}"))?;
            let (url, file_name, sha256) = geyser::pick(project, &build, platform)?;
            let staging = std::env::temp_dir().join(format!("monolab-{file_name}"));
            download_file_watched(&client, &url, &staging, &mut |received, total| {
                tell_addon_download(&app, &id, project, &file_name, received, total);
            })?;
            let checked = hashes::verify_sha256(&staging, &sha256).and_then(|()| hashes::verify_java_archive(&staging));
            let bytes = checked.and_then(|()| fs::read(&staging).map_err(|error| error.to_string()));
            let _ = fs::remove_file(&staging);
            put_plugin(&app, &instance, running, &file_name, bytes?)?;
            record_audit(&app, &id, "Mod", "Geyser install", &format!("Installed {file_name}."));
        }
        Ok::<String, String>(done())
    })?;
    if needs_via {
        install_modrinth(app, state, id, geyser::VIAVERSION_MODRINTH.into(), "plugin".into(), None)
            .map_err(|error| format!("{installed} ViaVersion could not be added ({error}); Geyser needs it on this older version."))?;
        return Ok(format!(
            "{installed} ViaVersion was added too: Geyser speaks Minecraft {}, and ViaVersion lets it into this {} server.",
            latest.unwrap_or_default(),
            instance.version
        ));
    }
    if proxy {
        return Ok(format!("{installed} Servers behind this proxy that run an older Minecraft version need ViaVersion."));
    }
    Ok(installed)
}

/// The newest Minecraft release, from Mojang's version list.
fn latest_release() -> Result<String, String> {
    let client =
        crate::net::builder().user_agent(net::USER_AGENT).timeout(Duration::from_secs(20)).build().map_err(|error| error.to_string())?;
    let manifest: serde_json::Value = client
        .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .send()
        .and_then(|answer| answer.error_for_status())
        .and_then(|answer| answer.json())
        .map_err(|error| format!("Could not fetch the Minecraft version list: {error}"))?;
    manifest
        .pointer("/latest/release")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "The Minecraft version list has no latest release.".into())
}

/// Puts a downloaded plugin jar into the server's plugins folder: a game panel, SSH or this PC.
fn put_plugin(app: &AppHandle, instance: &ServerInstance, running: bool, file_name: &str, bytes: Vec<u8>) -> Result<(), String> {
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(app, instance)?;
        tell_upload(app, file_name, "uploading", 0, bytes.len() as u64);
        let sent = tauri::async_runtime::block_on(api.upload(&target, "plugins", file_name, bytes));
        tell_upload(app, file_name, "done", 0, 0);
        return sent;
    }
    if !instance.remote_host.is_empty() {
        if running {
            return Err("Can't install over SSH while the server is running. Please stop it first.".into());
        }
        return tauri::async_runtime::block_on(watched_upload(
            app,
            &instance.remote_host,
            &instance.path,
            &format!("plugins/{file_name}"),
            &bytes,
            file_name,
        ));
    }
    let folder = Path::new(&instance.path).join("plugins");
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let destination = folder.join(file_name);
    if running && destination.exists() {
        return Err(format!("{file_name} already exists. A running server may be using it — stop the server before replacing it."));
    }
    let staging = folder.join(format!(".{file_name}.download"));
    fs::write(&staging, &bytes).map_err(|error| error.to_string())?;
    fs::rename(&staging, &destination).map_err(|error| format!("Could not install {file_name}: {error}"))
}

#[tauri::command(async)]
#[specta::specta]
fn install_modrinth(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    project_id: String,
    project_type: String,
    version_id: Option<String>,
) -> Result<(), String> {
    off_runtime(|| {
        let running = is_running(&state, &id);
        let instance = instance_by_id(&app, &id)?;
        let (folder_name, loader) = modrinth_loader(&instance, &project_type)?;
        let client = crate::net::builder()
            .user_agent("Monolab/0.1.0 (local Windows desktop app)")
            .timeout(Duration::from_secs(90))
            .build()
            .map_err(|error| error.to_string())?;
        let url = format!("https://api.modrinth.com/v2/project/{project_id}/version");
        let releases: serde_json::Value = client
            .get(url)
            .send()
            .map_err(|error| format!("Could not fetch Modrinth download info: {error}"))?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .map_err(|error| error.to_string())?;
        let picked = version_id.as_deref().unwrap_or_default();
        let release = releases
            .as_array()
            .and_then(|items| {
                if !picked.is_empty() {
                    return items.iter().find(|item| item.get("id").and_then(|v| v.as_str()) == Some(picked));
                }
                items.iter().find(|item| {
                    item.get("game_versions")
                        .and_then(|value| value.as_array())
                        .is_some_and(|versions| versions.iter().any(|version| version.as_str() == Some(&instance.version)))
                        && item
                            .get("loaders")
                            .and_then(|value| value.as_array())
                            .is_some_and(|loaders| loaders.iter().any(|item| item.as_str() == Some(loader)))
                })
            })
            .ok_or(if picked.is_empty() {
                "No Modrinth file matches this server's version and core."
            } else {
                "The chosen version was not found."
            })?;
        let file = release
            .get("files")
            .and_then(|value| value.as_array())
            .and_then(|files| {
                files.iter().find(|file| file.get("primary").and_then(|value| value.as_bool()).unwrap_or(false)).or_else(|| files.first())
            })
            .ok_or("No Modrinth release file was found.")?;
        let file_url = file.get("url").and_then(|value| value.as_str()).ok_or("The Modrinth file URL is malformed.")?;
        let file_name = file
            .get("filename")
            .and_then(|value| value.as_str())
            .and_then(|value| Path::new(value).file_name())
            .and_then(|value| value.to_str())
            .ok_or("The Modrinth file name is malformed.")?;
        let checksum = file.pointer("/hashes/sha512").and_then(|value| value.as_str()).ok_or("The Modrinth file has no SHA‑512.")?;
        if !instance.panel_host.is_empty() {
            let (api, target) = panel_of(&app, &instance)?;
            if tauri::async_runtime::block_on(api.exists(&target, &format!("{folder_name}/{file_name}")))?.is_some() {
                return Err(format!("{file_name} already exists. Delete it first or use Change version."));
            }
            let staging = std::env::temp_dir().join(format!("monolab-{file_name}"));
            download_file_watched(&client, file_url, &staging, &mut |received, total| {
                tell_addon_download(&app, &id, &project_id, file_name, received, total);
            })?;
            if let Err(error) = verify_sha512(&staging, checksum) {
                let _ = fs::remove_file(&staging);
                return Err(error);
            }
            let bytes = fs::read(&staging).map_err(|error| error.to_string())?;
            let _ = fs::remove_file(&staging);
            tell_upload(&app, file_name, "uploading", 0, bytes.len() as u64);
            let sent = tauri::async_runtime::block_on(api.upload(&target, folder_name, file_name, bytes));
            tell_upload(&app, file_name, "done", 0, 0);
            sent?;
            let title = client
                .get(format!("https://api.modrinth.com/v2/project/{project_id}"))
                .send()
                .ok()
                .and_then(|answer| answer.json::<serde_json::Value>().ok())
                .and_then(|one| one.get("title").and_then(|title| title.as_str()).map(str::to_string))
                .unwrap_or_else(|| file_name.trim_end_matches(".jar").to_string());
            let record = panel::MarketInstall {
                kind: if folder_name == "plugins" { "plugin".into() } else { "mod".into() },
                source: "modrinth".into(),
                project_id: project_id.clone(),
                project_title: Some(title),
                version_id: release.get("id").and_then(|one| one.as_str()).map(str::to_string),
                version_name: release.get("version_number").and_then(|one| one.as_str()).map(str::to_string),
                filename: Some(file_name.to_string()),
            };
            if let Err(error) = tauri::async_runtime::block_on(api.market_record(&target.ident, &record)) {
                eprintln!("[panel] Could not record the marketplace entry: {error}");
            }
            record_audit(&app, &id, "Mod", "Modrinth install (panel)", &format!("Installed {file_name}."));
            return Ok(());
        }
        if !instance.remote_host.is_empty() {
            if running {
                return Err("Can't install over SSH while the server is running. Please stop it first.".into());
            }
            let staging = std::env::temp_dir().join(format!("monolab-{file_name}"));
            download_file_watched(&client, file_url, &staging, &mut |received, total| {
                tell_addon_download(&app, &id, &project_id, file_name, received, total);
            })?;
            if let Err(error) = verify_sha512(&staging, checksum) {
                let _ = fs::remove_file(&staging);
                return Err(error);
            }
            let bytes = fs::read(&staging).map_err(|error| error.to_string())?;
            let _ = fs::remove_file(&staging);
            tauri::async_runtime::block_on(watched_upload(
                &app,
                &instance.remote_host,
                &instance.path,
                &format!("{folder_name}/{file_name}"),
                &bytes,
                file_name,
            ))?;
            record_audit(&app, &id, "Mod", "Modrinth install (SSH)", &format!("Installed {file_name}."));
            return Ok(());
        }
        let folder = Path::new(&instance.path).join(folder_name);
        fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
        let destination = folder.join(file_name);
        if running && destination.exists() {
            return Err(format!("{file_name} already exists. A running server may be using it — stop the server before replacing it."));
        }
        let staging = folder.join(format!(".{file_name}.download"));
        download_file_watched(&client, file_url, &staging, &mut |received, total| {
            tell_addon_download(&app, &id, &project_id, file_name, received, total);
        })?;
        if let Err(error) = verify_sha512(&staging, checksum) {
            let _ = fs::remove_file(&staging);
            return Err(error);
        }
        fs::rename(&staging, &destination).map_err(|error| format!("Could not install the Modrinth file: {error}"))?;
        record_audit(&app, &id, "Mod", "Modrinth install", &format!("Installed {file_name}."));
        Ok(())
    })
}

fn mrpack_index(path: &Path) -> Result<serde_json::Value, String> {
    let file = fs::File::open(path).map_err(|error| format!("Could not open the modpack file: {error}"))?;
    let mut archive = ZipArchive::new(file).map_err(|error| format!("The .mrpack is malformed: {error}"))?;
    let mut entry = archive.by_name("modrinth.index.json").map_err(|_| "modrinth.index.json was not found.".to_string())?;
    let mut text = String::new();
    entry.read_to_string(&mut text).map_err(|error| error.to_string())?;
    serde_json::from_str(&text).map_err(|error| format!("modrinth.index.json is malformed: {error}"))
}

fn mrpack_file_compatibility(file: &serde_json::Value) -> String {
    let Some(server) = file.pointer("/env/server").and_then(|value| value.as_str()) else {
        return "Needs review".into();
    };
    let client = file.pointer("/env/client").and_then(|value| value.as_str()).unwrap_or("required");
    if server == "unsupported" {
        "Client only".into()
    } else if client == "unsupported" {
        "Server only".into()
    } else {
        "Server".into()
    }
}

fn safe_pack_relative_path(relative: &str) -> Result<PathBuf, String> {
    paths::check_relative(relative).map_err(|error| match error {
        paths::PathError::Escape => {
            format!("The modpack has a path pointing outside the server folder: {relative}")
        }
        _ => format!("A modpack file path is malformed: {relative}"),
    })
}

fn extract_mrpack_overrides(pack: &Path, destination: &Path) -> Result<usize, String> {
    let file = fs::File::open(pack).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
    zips::check_budget(&mut archive, zips::MAX_UNPACKED, zips::MAX_ENTRIES)?;
    let mut written = 0_usize;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let relative = ["overrides", "server-overrides"].into_iter().find_map(|prefix| name.strip_prefix(prefix).ok());
        let Some(relative) = relative else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let output = destination.join(relative);
        if !output.starts_with(destination) {
            return Err("A modpack override path is malformed.".into());
        }
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut target = fs::File::create(&output).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut target).map_err(|error| error.to_string())?;
        written += 1;
    }
    Ok(written)
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct CfImportResult {
    overrides: usize,
    left: usize,
    list_path: String,
}

/// Whether a CurseForge API key is saved (the key itself never leaves the app).
#[tauri::command]
#[specta::specta]
fn curseforge_key_set(app: AppHandle) -> bool {
    curseforge::has_key(&app)
}

/// Saves a CurseForge API key; an empty one forgets it.
#[tauri::command(async)]
#[specta::specta]
fn set_curseforge_key(app: AppHandle, key: String) -> Result<bool, String> {
    curseforge::set_key(&app, &key)?;
    Ok(curseforge::has_key(&app))
}

/// FTB modpacks on CurseForge.
#[tauri::command(async)]
#[specta::specta]
fn search_ftb_packs(app: AppHandle, query: String) -> Result<Vec<catalog::CatalogHit>, String> {
    off_runtime(|| curseforge::search(&app, &query))
}

/// Downloads a CurseForge pack's newest file; the path goes to preview_cfpack and import_cfpack.
#[tauri::command(async)]
#[specta::specta]
fn download_curseforge_pack(app: AppHandle, project: String) -> Result<String, String> {
    let project: u64 = project.trim().parse().map_err(|_| "Unknown CurseForge project.".to_string())?;
    off_runtime(|| curseforge::download_pack(&app, project)).map(|path| path.to_string_lossy().to_string())
}

/// After import_cfpack: downloads the pack's mods CurseForge allows into the server's mods folder.
#[tauri::command(async)]
#[specta::specta]
fn fetch_curseforge_mods(app: AppHandle, state: State<'_, AppState>, id: String, path: String) -> Result<curseforge::FetchedMods, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.remote_host.is_empty() || !instance.panel_host.is_empty() {
        return Err("Mods are downloaded for servers on this PC only. Use the download list for this one.".into());
    }
    ensure_stopped(&state, &id)?;
    let pack = cfpack::read(Path::new(path.trim()))?;
    let files: Vec<u64> = pack.mods.iter().filter(|one| one.required).map(|one| one.file).collect();
    let done = off_runtime(|| curseforge::fetch_mods(&app, &files, Path::new(&instance.path)))?;
    record_audit(
        &app,
        &id,
        "Mod",
        "CurseForge mods",
        &format!("Downloaded {} mods · {} left to download yourself", done.fetched, done.left),
    );
    Ok(done)
}

#[tauri::command(async)]
#[specta::specta]
fn preview_cfpack(path: String) -> Result<cfpack::Pack, String> {
    let path = PathBuf::from(path.trim());
    if !path.is_file() {
        return Err("Modpack file not found.".into());
    }
    cfpack::read(&path)
}

#[tauri::command]
#[specta::specta]
async fn import_cfpack(app: AppHandle, state: State<'_, AppState>, id: String, path: String) -> Result<CfImportResult, String> {
    let Some(instance) = remote_instance(&app, &id)? else {
        only_local(&app, &id, "CurseForge modpack import")?;
        ensure_stopped(&state, &id)?;
        let worker = app.clone();
        return tauri::async_runtime::spawn_blocking(move || import_cfpack_local(worker, id, path))
            .await
            .map_err(|error| error.to_string())?;
    };
    ensure_remote_stopped(&app, &state, &instance).await?;
    let pack_path = PathBuf::from(path.trim()).canonicalize().map(plain_path).map_err(|_| "Modpack file not found.".to_string())?;
    let pack = cfpack::read(&pack_path)?;
    let data = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    let staging = data.join("cfpack-remote").join(format!("{}-{}", instance.id, unix_now()));
    fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let staged = (|| -> Result<(usize, PathBuf), String> {
        let overrides = cfpack::extract_overrides(&pack_path, &staging)?;
        let list = cfpack::write_list(&pack, &staging)?;
        let keep = data.join("cfpack-lists").join(&instance.id);
        fs::create_dir_all(&keep).map_err(|error| error.to_string())?;
        let kept = keep.join(list.file_name().unwrap_or_default());
        fs::copy(&list, &kept).map_err(|error| error.to_string())?;
        Ok((overrides, kept))
    })();
    let result = match staged {
        Ok(done) => remote::make_backup(&app, &instance.remote_host, &instance.path, "Before modpack import", &[])
            .await
            .map(|_| ())
            .or(Ok::<(), String>(()))
            .and(remote::upload_tree(&app, &instance.remote_host, &instance.path, &staging).await)
            .map(|_| done),
        Err(error) => Err(error),
    };
    let _ = fs::remove_dir_all(&staging);
    let (overrides, list) = result?;
    record_audit(
        &app,
        &id,
        "Mod",
        "Imported CurseForge modpack (SSH)",
        &format!("{} {} · {overrides} config files · {} mods to download yourself", pack.name, pack.version, pack.mods.len()),
    );
    Ok(CfImportResult { overrides, left: pack.mods.len(), list_path: list.to_string_lossy().to_string() })
}

fn import_cfpack_local(app: AppHandle, id: String, path: String) -> Result<CfImportResult, String> {
    let instance = instance_by_id(&app, &id)?;
    let pack_path = PathBuf::from(path.trim()).canonicalize().map(plain_path).map_err(|_| "Modpack file not found.".to_string())?;
    let pack = cfpack::read(&pack_path)?;
    let folder = PathBuf::from(&instance.path);

    if let Ok(vault) = backup_folder(&app, &instance) {
        let _ = backups::create(Path::new(&instance.path), "Before modpack import", &vault, false);
    }

    let overrides = cfpack::extract_overrides(&pack_path, &folder)?;
    let list = cfpack::write_list(&pack, &folder)?;
    record_audit(
        &app,
        &id,
        "Mod",
        "CurseForge modpack import",
        &format!("{} {} · {overrides} config files · {} mods to download yourself", pack.name, pack.version, pack.mods.len()),
    );
    Ok(CfImportResult { overrides, left: pack.mods.len(), list_path: list.to_string_lossy().to_string() })
}

#[tauri::command(async)]
#[specta::specta]
fn search_modpacks(query: String, page: Option<u32>) -> Result<CatalogPage, String> {
    off_runtime(|| {
        let page = page.unwrap_or(0);
        let per_page = 20_u32;
        let client = crate::net::builder()
            .user_agent("Monolab/0.1.0 (local Windows desktop app)")
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| error.to_string())?;
        let facets = serde_json::to_string(&vec![vec!["project_type:modpack"]]).map_err(|error| error.to_string())?;
        let offset = (page * per_page).to_string();
        let limit = per_page.to_string();
        let response: serde_json::Value = client
            .get("https://api.modrinth.com/v2/search")
            .query(&[
                ("query", query.trim()),
                ("limit", limit.as_str()),
                ("offset", offset.as_str()),
                ("facets", facets.as_str()),
                ("index", "downloads"),
            ])
            .send()
            .map_err(|error| format!("Modpack search failed: {error}"))?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .map_err(|error| error.to_string())?;
        let hits: Vec<catalog::CatalogHit> = response
            .get("hits")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|hit| {
                let project_id: String = hit.get("project_id")?.as_str()?.into();
                Some(catalog::CatalogHit {
                    page_url: format!("https://modrinth.com/modpack/{project_id}"),
                    source: "Modrinth".into(),
                    project_id,
                    title: hit.get("title")?.as_str()?.into(),
                    description: hit.get("description").and_then(|value| value.as_str()).unwrap_or_default().into(),
                    project_type: "modpack".into(),
                    installable: true,
                    icon_url: hit.get("icon_url").and_then(|value| value.as_str()).filter(|value| !value.is_empty()).map(str::to_string),
                    author: hit.get("author").and_then(|value| value.as_str()).map(str::to_owned),
                    downloads: hit.get("downloads").and_then(|value| value.as_u64()),
                    follows: hit.get("follows").and_then(|value| value.as_u64()),
                    updated_at: None,
                    loaders: Vec::new(),
                    versions: Vec::new(),
                })
            })
            .collect();
        Ok(CatalogPage { has_more: hits.len() as u32 >= per_page, hits, page })
    })
}

/// Where datapacks go for this server, relative to its folder.
#[tauri::command(async)]
#[specta::specta]
async fn datapack_folder(app: AppHandle, id: String) -> Result<String, String> {
    let instance = instance_by_id(&app, &id)?;
    if pumpkin::is_pumpkin(&instance.core) || matches!(instance.core.as_str(), "Velocity" | "BungeeCord") {
        return Err("This kind of server has no worlds, so it has no datapacks.".into());
    }
    Ok(format!("{}/datapacks", level_name_any(&app, &instance).await))
}

#[tauri::command(async)]
#[specta::specta]
fn search_datapacks(app: AppHandle, id: String, query: String, page: Option<u32>) -> Result<CatalogPage, String> {
    off_runtime(|| {
        let instance = instance_by_id(&app, &id)?;
        let page = page.unwrap_or(0);
        let per_page = 20_u32;
        let client = crate::net::builder()
            .user_agent("Monolab/0.1.0 (local Windows desktop app)")
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| error.to_string())?;
        let mut facets = vec![vec!["project_type:datapack".to_string()]];
        if looks_like_version(&instance.version) {
            facets.push(vec![format!("versions:{}", instance.version.trim())]);
        }
        let facets = serde_json::to_string(&facets).map_err(|error| error.to_string())?;
        let offset = (page * per_page).to_string();
        let limit = per_page.to_string();
        let response: serde_json::Value = client
            .get("https://api.modrinth.com/v2/search")
            .query(&[
                ("query", query.trim()),
                ("limit", limit.as_str()),
                ("offset", offset.as_str()),
                ("facets", facets.as_str()),
                ("index", "downloads"),
            ])
            .send()
            .map_err(|error| format!("Datapack search failed: {error}"))?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .map_err(|error| error.to_string())?;
        let hits: Vec<catalog::CatalogHit> = response
            .get("hits")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|hit| {
                let project_id: String = hit.get("project_id")?.as_str()?.into();
                Some(catalog::CatalogHit {
                    page_url: format!("https://modrinth.com/datapack/{project_id}"),
                    source: "Modrinth".into(),
                    project_id,
                    title: hit.get("title")?.as_str()?.into(),
                    description: hit.get("description").and_then(|value| value.as_str()).unwrap_or_default().into(),
                    project_type: "datapack".into(),
                    installable: true,
                    icon_url: hit.get("icon_url").and_then(|value| value.as_str()).filter(|value| !value.is_empty()).map(str::to_string),
                    author: hit.get("author").and_then(|value| value.as_str()).map(str::to_owned),
                    downloads: hit.get("downloads").and_then(|value| value.as_u64()),
                    follows: hit.get("follows").and_then(|value| value.as_u64()),
                    updated_at: None,
                    loaders: Vec::new(),
                    versions: Vec::new(),
                })
            })
            .collect();
        Ok(CatalogPage { has_more: hits.len() as u32 >= per_page, hits, page })
    })
}

/// Downloads the newest datapack file made for this server's Minecraft version into the world's datapacks folder.
#[tauri::command(async)]
#[specta::specta]
async fn install_datapack(app: AppHandle, id: String, project_id: String) -> Result<String, String> {
    let instance = instance_by_id(&app, &id)?;
    let folder = datapack_folder(app.clone(), id.clone()).await?;
    let version = instance.version.trim().to_string();
    let downloaded = {
        let (app, project_id) = (app.clone(), project_id.clone());
        tauri::async_runtime::spawn_blocking(move || {
            off_runtime(|| -> Result<(String, Vec<u8>), String> {
                let client = crate::net::builder()
                    .user_agent("Monolab/0.1.0 (local Windows desktop app)")
                    .timeout(Duration::from_secs(90))
                    .build()
                    .map_err(|error| error.to_string())?;
                let games = serde_json::to_string(&[version.as_str()]).map_err(|error| error.to_string())?;
                let releases: serde_json::Value = client
                    .get(format!("https://api.modrinth.com/v2/project/{project_id}/version"))
                    .query(&[("game_versions", games.as_str()), ("loaders", "[\"datapack\"]")])
                    .send()
                    .map_err(|error| format!("Could not fetch Modrinth download info: {error}"))?
                    .error_for_status()
                    .map_err(|error| error.to_string())?
                    .json()
                    .map_err(|error| error.to_string())?;
                let release = releases
                    .as_array()
                    .and_then(|items| items.first())
                    .ok_or_else(|| format!("No datapack file is made for Minecraft {version}."))?;
                let file = release
                    .get("files")
                    .and_then(|value| value.as_array())
                    .and_then(|files| {
                        files
                            .iter()
                            .find(|file| file.get("primary").and_then(|one| one.as_bool()).unwrap_or(false))
                            .or_else(|| files.first())
                    })
                    .ok_or("No Modrinth release file was found.")?;
                let url = file.get("url").and_then(|value| value.as_str()).ok_or("The Modrinth file URL is malformed.")?;
                let name = file
                    .get("filename")
                    .and_then(|value| value.as_str())
                    .and_then(|value| Path::new(value).file_name())
                    .and_then(|value| value.to_str())
                    .ok_or("The Modrinth file name is malformed.")?
                    .to_string();
                if !name.to_ascii_lowercase().ends_with(".zip") {
                    return Err(format!("{name} is not a datapack zip."));
                }
                let checksum =
                    file.pointer("/hashes/sha512").and_then(|value| value.as_str()).ok_or("The Modrinth file has no SHA‑512.")?;
                let staging = std::env::temp_dir().join(format!("monolab-datapack-{}", unix_now()));
                download_file_watched(&client, url, &staging, &mut |received, total| {
                    tell_addon_download(&app, "", &project_id, &name, received, total);
                })?;
                if let Err(error) = verify_sha512(&staging, checksum) {
                    let _ = fs::remove_file(&staging);
                    return Err(error);
                }
                let bytes = fs::read(&staging).map_err(|error| error.to_string());
                let _ = fs::remove_file(&staging);
                Ok((name, bytes?))
            })
        })
        .await
        .map_err(|error| error.to_string())??
    };
    let (name, bytes) = downloaded;
    let relative = format!("{folder}/{name}");
    if is_far(&instance) {
        far_upload(&app, &instance, &relative, &bytes, &name).await?;
    } else {
        let dir = Path::new(&instance.path).join(&folder);
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let target = dir.join(&name);
        if target.exists() {
            return Err(format!("{name} is already installed."));
        }
        fs::write(&target, &bytes).map_err(|error| format!("Could not save the datapack: {error}"))?;
    }
    record_audit(&app, &id, "Datapack", "Datapack install", &format!("Installed {name}."));
    Ok(name)
}

#[tauri::command(async)]
#[specta::specta]
fn download_modpack(app: AppHandle, project_id: String) -> Result<String, String> {
    off_runtime(move || {
        let client = crate::net::builder()
            .user_agent("Monolab/0.1.0 (local Windows desktop app)")
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|error| error.to_string())?;
        let versions: serde_json::Value = client
            .get(format!("https://api.modrinth.com/v2/project/{}/version", project_id.trim()))
            .send()
            .map_err(|error| format!("Could not fetch modpack versions: {error}"))?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .map_err(|error| error.to_string())?;
        let (url, name) = versions
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|version| {
                let files = version.get("files")?.as_array()?;
                let file = files.iter().find(|one| one.get("primary").and_then(|v| v.as_bool()) == Some(true)).or_else(|| files.first())?;
                Some((file.get("url")?.as_str()?.to_string(), file.get("filename")?.as_str()?.to_string()))
            })
            .ok_or("No downloadable file was found in this modpack.")?;
        let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("uploads");
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let safe: String = Path::new(&name)
            .file_name()
            .map(|one| one.to_string_lossy().to_string())
            .unwrap_or_else(|| "modpack.mrpack".into())
            .chars()
            .filter(|one| !matches!(one, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
            .collect();
        let path = dir.join(format!("{}-{}", unix_now(), safe));
        download_file(&client, &url, &path)?;
        Ok(path.to_string_lossy().to_string())
    })
}

#[tauri::command(async)]
#[specta::specta]
fn preview_mrpack(path: String) -> Result<Vec<MrpackFileEntry>, String> {
    let path = PathBuf::from(path.trim());
    if !path.is_file() {
        return Err("The .mrpack file could not be found.".into());
    }
    let index = mrpack_index(&path)?;
    Ok(index
        .get("files")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|file| {
            Some(MrpackFileEntry {
                path: file.get("path")?.as_str()?.into(),
                compatibility: mrpack_file_compatibility(file),
                download_count: file.get("downloads").and_then(|value| value.as_array()).map_or(0, Vec::len),
            })
        })
        .collect())
}

fn stage_mrpack(app: &AppHandle, instance: &ServerInstance, input: &MrpackImportInput, root: &Path) -> Result<(usize, usize), String> {
    let app = app.clone();
    let root = root.to_path_buf();
    off_runtime(|| {
        let pack = PathBuf::from(input.path.trim())
            .canonicalize()
            .map(plain_path)
            .map_err(|_| "The .mrpack file could not be found.".to_string())?;
        let index = mrpack_index(&pack)?;
        let files = index.get("files").and_then(|value| value.as_array()).ok_or("The modpack has no file list.")?;
        let client = crate::net::builder()
            .user_agent("Monolab/0.1.0 (local Windows desktop app)")
            .timeout(Duration::from_secs(90))
            .build()
            .map_err(|error| error.to_string())?;
        let mut downloads = Vec::new();
        let mut paths = HashSet::new();
        for file in files {
            let relative = file.get("path").and_then(|value| value.as_str()).ok_or("The modpack file path is malformed.")?;
            let path = safe_pack_relative_path(relative)?;
            if mrpack_file_compatibility(file) == "Client only" && !input.include_client_only {
                continue;
            }
            let url = file
                .get("downloads")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|value| value.as_str())
                .ok_or("The modpack file has no download URL.")?;
            let checksum = file.pointer("/hashes/sha512").and_then(|value| value.as_str()).ok_or("The modpack file has no SHA‑512.")?;
            let destination = root.join(&path);
            if !destination.starts_with(&root) {
                return Err("The modpack file path is malformed.".into());
            }
            if !paths.insert(path.clone()) {
                return Err("The modpack contains duplicate file paths.".into());
            }
            downloads.push((path, url.to_string(), checksum.to_string()));
        }
        let has_overrides = mrpack_has_overrides(&pack)?;
        if downloads.is_empty() && !has_overrides {
            return Err("No server‑side mods to install. Check whether client‑only mods are included.".into());
        }
        let staging_root = app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("mrpack-staging").join(format!(
            "{}-{}",
            instance.id,
            unix_now()
        ));
        fs::create_dir_all(&staging_root).map_err(|error| error.to_string())?;
        let stage_result = (|| -> Result<(), String> {
            for (done, (relative, url, checksum)) in downloads.iter().enumerate() {
                let _ = app.emit(
                    "server-download",
                    serde_json::json!({
                        "stage": "mods", "core": instance.core, "version": instance.version,
                        "received": done, "total": downloads.len(),
                    }),
                );
                let staging = staging_root.join(relative);
                if let Some(parent) = staging.parent() {
                    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                download_file(&client, url, &staging)?;
                verify_sha512(&staging, checksum)?;
            }
            Ok(())
        })();
        if let Err(error) = stage_result {
            let _ = fs::remove_dir_all(&staging_root);
            return Err(error);
        }
        for (relative, _, _) in &downloads {
            let source = staging_root.join(relative);
            let destination = root.join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::copy(&source, &destination).map_err(|error| format!("Could not install the modpack file: {error}"))?;
        }
        let _ = fs::remove_dir_all(&staging_root);
        let overrides = extract_mrpack_overrides(&pack, &root)?;
        Ok((downloads.len(), overrides))
    })
}

fn import_mrpack_local(app: AppHandle, state: State<'_, AppState>, input: MrpackImportInput) -> Result<usize, String> {
    ensure_stopped(&state, &input.id)?;
    let instance = instance_by_id(&app, &input.id)?;
    let root = Path::new(&instance.path).canonicalize().map(plain_path).map_err(|error| error.to_string())?;
    let (mods, overrides) = stage_mrpack(&app, &instance, &input, &root)?;
    record_audit(&app, &input.id, "Modpack", "Modpack import", &format!("Installed {mods} mods and {overrides} config files."));
    Ok(mods)
}

#[tauri::command(async)]
#[specta::specta]
async fn import_mrpack(app: AppHandle, state: State<'_, AppState>, input: MrpackImportInput) -> Result<usize, String> {
    let Some(instance) = remote_instance(&app, &input.id)? else {
        return import_mrpack_local(app, state, input);
    };
    ensure_remote_stopped(&app, &state, &instance).await?;
    let staging = app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("mrpack-remote").join(format!(
        "{}-{}",
        instance.id,
        unix_now()
    ));
    fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let staged = stage_mrpack(&app, &instance, &input, &staging);
    let result = match staged {
        Ok((mods, overrides)) => {
            remote::upload_tree(&app, &instance.remote_host, &instance.path, &staging).await.map(|_| (mods, overrides))
        }
        Err(error) => Err(error),
    };
    let _ = fs::remove_dir_all(&staging);
    let (mods, overrides) = result?;
    record_audit(&app, &input.id, "Modpack", "Import modpack (SSH)", &format!("Installed {mods} mods and {overrides} config files."));
    Ok(mods)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ModpackTarget {
    name: String,
    core: String,
    version: String,
    build: String,
}

fn modpack_target(index: &serde_json::Value) -> Result<ModpackTarget, String> {
    let deps = index
        .get("dependencies")
        .and_then(|value| value.as_object())
        .ok_or("The modpack does not say which Minecraft version or loader it needs.")?;
    let text =
        |key: &str| deps.get(key).and_then(|value| value.as_str()).map(str::trim).filter(|value| !value.is_empty()).map(str::to_string);
    let version = text("minecraft").ok_or("The modpack does not say which Minecraft version it needs.")?;
    let (core, build) = if let Some(loader) = text("fabric-loader") {
        ("Fabric", loader)
    } else if let Some(loader) = text("neoforge") {
        ("NeoForge", loader)
    } else if let Some(loader) = text("forge") {
        ("Forge", loader)
    } else if text("quilt-loader").is_some() {
        return Err("Quilt modpacks can't be created yet. Please pick a Fabric, NeoForge or Forge pack.".into());
    } else {
        ("Vanilla", String::new())
    };
    let name = index
        .get("name")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Modpack server")
        .to_string();
    Ok(ModpackTarget { name, core: core.into(), version, build })
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ModpackCreateInput {
    project_id: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    operation_id: String,
    #[serde(default = "default_memory_mib")]
    max_memory_mib: u32,
}

#[tauri::command(async)]
#[specta::specta]
fn create_from_modpack(app: AppHandle, state: State<'_, AppState>, input: ModpackCreateInput) -> Result<InstanceView, String> {
    let _ = app.emit("server-download", serde_json::json!({ "stage": "pack", "core": "", "version": "" }));
    let path = match input.path {
        Some(path) if !path.trim().is_empty() => path,
        _ => download_modpack(app.clone(), input.project_id.clone())?,
    };
    let index = mrpack_index(Path::new(&path))?;
    let target = modpack_target(&index)?;
    let make = |build: String| {
        create_instance(
            app.clone(),
            app.state(),
            CreateInstanceInput {
                name: target.name.clone(),
                core: target.core.clone(),
                version: target.version.clone(),
                core_build: build,
                max_memory_mib: input.max_memory_mib,
                min_memory_mib: input.max_memory_mib,
                eula_accepted: true,
                operation_id: input.operation_id.clone(),
            },
        )
    };
    let view = match make(target.build.clone()) {
        Ok(view) => view,
        Err(error) if !target.build.is_empty() && error.contains("not found") => make(String::new())?,
        Err(error) => return Err(error),
    };
    let put = import_mrpack_local(app.clone(), state, MrpackImportInput { id: view.instance.id.clone(), path, include_client_only: false });
    if let Err(error) = put {
        let _store = lock_instance_store();
        if let Ok(mut instances) = load_instances(&app) {
            instances.retain(|one| one.id != view.instance.id);
            let _ = save_instances(&app, &instances);
        }
        let _ = fs::remove_dir_all(&view.instance.path);
        return Err(format!("The mods could not be installed, so the server was not created: {error}"));
    }
    Ok(view)
}

fn mrpack_has_overrides(pack: &Path) -> Result<bool, String> {
    let file = fs::File::open(pack).map_err(|error| error.to_string())?;
    let archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
    let found =
        archive.file_names().any(|name| name.is_ok_and(|name| name.starts_with("overrides/") || name.starts_with("server-overrides/")));
    Ok(found)
}

#[tauri::command(async)]
#[specta::specta]
async fn scan_addons(app: AppHandle, id: String) -> Result<Vec<AddonEntry>, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        let (api, target) = panel_of(&app, &instance)?;
        let mut out = Vec::new();
        for sub in ["plugins", "mods"] {
            out.extend(pfs::addon_files(&api, &target, sub).await.into_iter().map(|one| AddonEntry {
                enabled: !one.name.ends_with(".disabled"),
                name: one.name.trim_end_matches(".disabled").trim_end_matches(".jar").to_string(),
                file_name: one.name,
                kind: sub.trim_end_matches('s').into(),
                metadata: String::new(),
                compatibility: String::new(),
                version: String::new(),
                icon: None,
            }));
        }
        return Ok(out);
    }
    if !instance.remote_host.is_empty() {
        let mut out = Vec::new();
        for sub in ["plugins", "mods"] {
            out.extend(remote::addon_files(&app, &instance.remote_host, &instance.path, sub).await.unwrap_or_default().into_iter().map(
                |one| {
                    let enabled = !one.name.ends_with(".disabled");
                    AddonEntry {
                        name: one.name.trim_end_matches(".disabled").trim_end_matches(".jar").to_string(),
                        file_name: one.name,
                        kind: sub.trim_end_matches('s').into(),
                        enabled,
                        metadata: String::new(),
                        compatibility: String::new(),
                        version: String::new(),
                        icon: None,
                    }
                },
            ));
        }
        return Ok(out);
    }
    let root = Path::new(&instance.path);
    let mut addons = Vec::new();
    if pumpkin::is_pumpkin(&instance.core) {
        if let Ok(entries) = fs::read_dir(root.join("plugins")) {
            for entry in entries.flatten() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                let Some(enabled) = pumpkin::plugin_state(&file_name) else {
                    continue;
                };
                addons.push(AddonEntry {
                    name: pumpkin::plugin_display_name(&file_name),
                    file_name,
                    kind: "plugin".into(),
                    enabled,
                    metadata: String::new(),
                    compatibility: String::new(),
                    version: String::new(),
                    icon: None,
                });
            }
        }
        addons.sort_by_key(|a| a.name.to_lowercase());
        return Ok(addons);
    }
    for (folder, kind) in [("plugins", "plugin"), ("mods", "mod")] {
        let directory = root.join(folder);
        if !directory.is_dir() {
            continue;
        }
        let mut jars: Vec<(PathBuf, String, bool)> = Vec::new();
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())?.flatten() {
            let path = entry.path();
            let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_string();
            let enabled = file_name.ends_with(".jar");
            if !enabled && !file_name.ends_with(".jar.disabled") {
                continue;
            }
            jars.push((path, file_name, enabled));
        }
        addons.extend(in_parallel(jars, |(path, file_name, enabled)| {
            let facts = addons::read_jar(&path, kind);
            AddonEntry {
                name: if facts.listed_name.trim().is_empty() { facts.display_name } else { facts.listed_name },
                compatibility: addon_compatibility(&file_name, kind, facts.client_only),
                file_name,
                kind: kind.into(),
                enabled,
                metadata: facts.metadata,
                version: facts.version,
                icon: facts.icon,
            }
        }));
    }
    addons.sort_by_key(|a| a.name.to_lowercase());
    Ok(addons)
}

fn addon_file_in(instance: &ServerInstance, folder: &str, file_name: &str) -> Result<(PathBuf, PathBuf), String> {
    if file_name.contains(['/', '\\']) {
        return Err(paths::PathError::Invalid.into());
    }
    let root = Path::new(&instance.path);
    let allowed = paths::resolve_in_server(root, folder).map_err(|_| "Add‑on folder not found.".to_string())?;
    let normalized = paths::resolve_in_server(root, &format!("{folder}/{file_name}")).map_err(|error| match error {
        paths::PathError::Missing => "Target not found.".to_string(),
        other => other.to_string(),
    })?;
    if normalized.parent() != Some(allowed.as_path()) {
        return Err("Files outside the allowed add‑on folder cannot be changed.".into());
    }
    Ok((normalized, allowed))
}

#[tauri::command(async)]
#[specta::specta]
async fn delete_addon(app: AppHandle, state: State<'_, AppState>, id: String, file_name: String, kind: String) -> Result<(), String> {
    {
        let instance = instance_by_id(&app, &id)?;
        if !instance.panel_host.is_empty() {
            let folder = if kind == "plugin" { "plugins" } else { "mods" };
            let name = plain_name(&file_name)?;
            let (api, target) = panel_of(&app, &instance)?;
            pfs::trash_path(&api, &target, &format!("{folder}/{name}")).await?;
            let plain = name.trim_end_matches(".disabled");
            if let Some(record) = api.market_installed(&target.ident).await.into_iter().find(|one| one.filename.as_deref() == Some(plain)) {
                let _ = api.market_forget(&target.ident, &record).await;
            }
            record_audit(&app, &id, "Mod", "Delete (panel)", name);
            return Ok(());
        }
        if !instance.remote_host.is_empty() {
            let folder = if kind == "plugin" { "plugins" } else { "mods" };
            let name = plain_name(&file_name)?;
            remote::trash_path(&app, &instance.remote_host, &instance.path, &format!("{folder}/{name}")).await?;
            record_audit(&app, &id, "Mod", "Delete (SSH)", name);
            return Ok(());
        }
    }
    ensure_stopped(&state, &id)?;
    let instance = instance_by_id(&app, &id)?;
    let folder = if kind == "plugin" { "plugins" } else { "mods" };
    let (normalized, _allowed) = addon_file_in(&instance, folder, &file_name)?;
    let bin = addon_trash_dir(&app, &id)?;
    move_file(&normalized, &bin.join(format!("{}-{file_name}", unix_now())))
        .map_err(|error| format!("Could not move the file: {error}"))?;
    record_audit(&app, &id, "Mod", "Add‑on deleted", &file_name);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
async fn toggle_addon(app: AppHandle, state: State<'_, AppState>, id: String, file_name: String, kind: String) -> Result<String, String> {
    {
        let instance = instance_by_id(&app, &id)?;
        if !instance.panel_host.is_empty() {
            ensure_panel_stopped(&app, &instance).await?;
            let folder = if kind == "plugin" { "plugins" } else { "mods" };
            let name = plain_name(&file_name)?;
            let to = if name.ends_with(".disabled") { name.trim_end_matches(".disabled").to_string() } else { format!("{name}.disabled") };
            let (api, target) = panel_of(&app, &instance)?;
            pfs::move_path(&api, &target, &format!("{folder}/{name}"), &format!("{folder}/{to}")).await?;
            record_audit(&app, &id, "Mod", "Enable/disable (panel)", &to);
            return Ok(to);
        }
        if !instance.remote_host.is_empty() {
            if state.remote_running.lock().expect("remote state poisoned").contains_key(&id) {
                return Err("Stop the server first.".into());
            }
            let folder = if kind == "plugin" { "plugins" } else { "mods" };
            let name = plain_name(&file_name)?;
            let to = if name.ends_with(".disabled") { name.trim_end_matches(".disabled").to_string() } else { format!("{name}.disabled") };
            remote::move_path(&app, &instance.remote_host, &instance.path, &format!("{folder}/{name}"), &format!("{folder}/{to}")).await?;
            record_audit(&app, &id, "Mod", "Enable/disable (SSH)", &to);
            return Ok(to);
        }
    }
    ensure_stopped(&state, &id)?;
    let instance = instance_by_id(&app, &id)?;
    let folder = if kind == "plugin" { "plugins" } else { "mods" };
    let (normalized, allowed) = addon_file_in(&instance, folder, &file_name)?;
    let new_name = if pumpkin::is_pumpkin(&instance.core) {
        pumpkin::toggled_name(&file_name)
    } else if file_name.ends_with(".jar.disabled") {
        file_name.trim_end_matches(".disabled").to_string()
    } else {
        format!("{file_name}.disabled")
    };
    fs::rename(normalized, allowed.join(&new_name)).map_err(|error| format!("Could not toggle the add‑on: {error}"))?;
    record_audit(&app, &id, "Mod", "Add‑on toggled", &format!("{file_name} -> {new_name}"));
    Ok(new_name)
}

#[tauri::command(async)]
#[specta::specta]
async fn upgrade_instance(app: AppHandle, state: State<'_, AppState>, input: UpgradeInput) -> Result<UpgradeHistoryEntry, String> {
    if let Some(instance) = remote_instance(&app, &input.id)? {
        ensure_remote_stopped(&app, &state, &instance).await?;
        return upgrade_remote(&app, instance, &input).await;
    }
    ensure_stopped(&state, &input.id)?;
    let target = input.target_version.trim();
    if target.is_empty() {
        return Err("Choose the Minecraft version to upgrade to.".into());
    }
    let instance = instance_by_id(&app, &input.id)?;
    if matches!(instance.core.as_str(), "Forge" | "NeoForge") {
        return Err("Forge and NeoForge use an installer layout that automatic upgrades don't support yet. Back up, create a new server and move your settings over.".into());
    }
    let core = core_after(&instance.core, input.target_core.as_deref())?;
    let root = Path::new(&instance.path).canonicalize().map(plain_path).map_err(|error| error.to_string())?;
    let current = instance
        .server_jar
        .as_ref()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| find_jar(&root))
        .ok_or("The server jar to replace was not found.")?
        .canonicalize()
        .map(plain_path)
        .map_err(|error| error.to_string())?;
    if !current.starts_with(&root) {
        return Err("A jar outside the server folder cannot be upgraded automatically.".into());
    }
    if !input.skip_backup {
        backups::create(&root, "Before upgrade", &backup_folder(&app, &instance)?, false)?;
    }
    let stamp = unix_now();
    let stash = upgrade_dir(&app, &instance.id)?;
    let staging = stash.join(format!("upgrade-{stamp}"));
    fs::create_dir_all(&staging).map_err(|error| format!("Could not create the upgrade staging folder: {error}"))?;
    let wanted = input.target_build.as_deref().map(str::trim).filter(|one| !one.is_empty());
    let downloaded = match download_server_jar(&app, &core, target, wanted, &staging, "") {
        Ok(Some(path)) => path,
        Ok(None) => {
            let _ = fs::remove_dir_all(&staging);
            return Err("Automatic upgrades are not supported for this core yet.".into());
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    let current_name = current.file_name().and_then(|name| name.to_str()).ok_or("The current jar file name is malformed.")?.to_string();
    let previous = stash.join(format!("previous-{stamp}-{current_name}"));
    move_file(&current, &previous).map_err(|error| format!("Could not keep the existing jar: {error}"))?;
    let destination = root.join(core_file_name(&core));
    if let Err(error) = move_file(&downloaded, &destination) {
        let _ = move_file(&previous, &current);
        let _ = fs::remove_dir_all(&staging);
        return Err(format!("Could not swap in the new jar, so the old one was restored: {error}"));
    }
    let _ = fs::remove_dir_all(&staging);
    let switched = core != instance.core;
    let entry = UpgradeHistoryEntry {
        server_id: instance.id.clone(),
        from_version: instance.version.clone(),
        to_version: target.into(),
        previous_jar: previous.to_string_lossy().to_string(),
        created_at: stamp,
        from_core: switched.then(|| instance.core.clone()),
        to_core: switched.then(|| core.clone()),
    };
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    let saved = instances.iter_mut().find(|saved| saved.id == instance.id).ok_or("Server not found.")?;
    saved.version = target.into();
    saved.core = core.clone();
    saved.core_build = wanted.unwrap_or("").to_string();
    saved.server_jar = Some(destination.to_string_lossy().to_string());
    save_instances(&app, &instances)?;
    let mut history = read_upgrade_history(&app);
    history.push(entry.clone());
    write_upgrade_history(&app, history);
    record_audit(&app, &entry.server_id, "Upgrade", "Server upgrade", &format!("{} → {}", entry.from_version, entry.to_version));
    Ok(entry)
}

async fn ensure_remote_stopped(app: &AppHandle, state: &AppState, instance: &ServerInstance) -> Result<(), String> {
    let listed = state.remote_running.lock().expect("remote state poisoned").contains_key(&instance.id);
    if listed {
        return Err("Stop the server first.".into());
    }
    let who = [(instance.id.clone(), instance.path.clone())];
    if let Ok(found) = remote::live_state_checked(app, &instance.remote_host, &who).await {
        if found.contains_key(&instance.id) {
            return Err("Stop the server first.".into());
        }
    }
    Ok(())
}

async fn upgrade_remote(app: &AppHandle, instance: ServerInstance, input: &UpgradeInput) -> Result<UpgradeHistoryEntry, String> {
    let target = input.target_version.trim();
    if target.is_empty() {
        return Err("Choose the Minecraft version to upgrade to.".into());
    }
    if matches!(instance.core.as_str(), "Forge" | "NeoForge") {
        return Err("Forge and NeoForge use an installer layout that automatic upgrades don't support yet. Back up, create a new server and move your settings over.".into());
    }
    if pumpkin::is_pumpkin(&instance.core) {
        return Err("Automatic upgrades aren't supported yet for Pumpkin on SSH servers.".into());
    }
    let core = core_after(&instance.core, input.target_core.as_deref())?;
    let jar = instance
        .server_jar
        .as_deref()
        .map(str::trim)
        .filter(|one| !one.is_empty())
        .ok_or("The server jar to replace was not found.")?
        .to_string();
    if !input.skip_backup {
        tell_job(app, "upgrade", &instance.name, "taring", 0, 0);
        remote::make_backup(app, &instance.remote_host, &instance.path, "before-upgrade", &[]).await?;
    }
    let stamp = unix_now();
    let stash = upgrade_dir(app, &instance.id)?;
    let staging = stash.join(format!("upgrade-{stamp}"));
    fs::create_dir_all(&staging).map_err(|error| format!("Could not create the upgrade staging folder: {error}"))?;
    let wanted = input.target_build.as_deref().map(str::trim).filter(|one| !one.is_empty());
    tell_job(app, "upgrade", &instance.name, "fetching", 0, 0);
    let downloaded = download_server_jar(app, &core, target, wanted, &staging, "");
    let bytes = match downloaded {
        Ok(Some(path)) => fs::read(&path).map_err(|error| error.to_string()),
        Ok(None) => Err("Automatic upgrades are not supported for this core yet.".into()),
        Err(error) => Err(error),
    };
    let _ = fs::remove_dir_all(&staging);
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(error) => {
            tell_job(app, "upgrade", &instance.name, "done", 0, 0);
            return Err(error);
        }
    };
    tell_job(app, "upgrade", &instance.name, "swapping", 0, 0);
    let previous = remote::swap_core(app, &instance.remote_host, &instance.path, &jar, &bytes, stamp).await;
    tell_job(app, "upgrade", &instance.name, "done", 0, 0);
    let previous = previous?;
    let switched = core != instance.core;
    let entry = UpgradeHistoryEntry {
        server_id: instance.id.clone(),
        from_version: instance.version.clone(),
        to_version: target.into(),
        previous_jar: previous,
        created_at: stamp,
        from_core: switched.then(|| instance.core.clone()),
        to_core: switched.then(|| core.clone()),
    };
    {
        let _store = lock_instance_store();
        let mut instances = load_instances(app)?;
        let saved = instances.iter_mut().find(|saved| saved.id == instance.id).ok_or("Server not found.")?;
        saved.version = target.into();
        saved.core = core.clone();
        saved.core_build = wanted.unwrap_or("").to_string();
        save_instances(app, &instances)?;
    }
    let mut history = read_upgrade_history(app);
    history.push(entry.clone());
    write_upgrade_history(app, history);
    record_audit(app, &entry.server_id, "Upgrade", "Upgrade server (SSH)", &format!("{} → {}", entry.from_version, entry.to_version));
    Ok(entry)
}

async fn rollback_remote(app: &AppHandle, instance: ServerInstance) -> Result<UpgradeHistoryEntry, String> {
    let entry = list_upgrade_history(app.clone(), instance.id.clone()).into_iter().next().ok_or("No upgrade to roll back.")?;
    let jar = instance.server_jar.as_deref().map(str::trim).filter(|one| !one.is_empty()).ok_or("Server jar file not found.")?.to_string();
    remote::restore_core(app, &instance.remote_host, &instance.path, &jar, &entry.previous_jar, unix_now()).await?;
    {
        let _store = lock_instance_store();
        let mut instances = load_instances(app)?;
        let saved = instances.iter_mut().find(|saved| saved.id == instance.id).ok_or("Server not found.")?;
        saved.version = entry.from_version.clone();
        if let Some(core) = &entry.from_core {
            saved.core = core.clone();
        }
        saved.core_build = String::new();
        save_instances(app, &instances)?;
    }
    record_audit(app, &instance.id, "Upgrade", "Revert upgrade (SSH)", &format!("Rolled back to version {}", entry.from_version));
    Ok(entry)
}

#[tauri::command(async)]
#[specta::specta]
fn list_upgrade_history(app: AppHandle, id: String) -> Vec<UpgradeHistoryEntry> {
    let mut history = read_upgrade_history(&app).into_iter().filter(|entry| entry.server_id == id).collect::<Vec<_>>();
    history.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    history
}

fn core_file_name(core: &str) -> &'static str {
    if pumpkin::is_pumpkin(core) {
        cores::PUMPKIN_EXE
    } else {
        "server.jar"
    }
}

#[tauri::command(async)]
#[specta::specta]
async fn rollback_last_upgrade(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<UpgradeHistoryEntry, String> {
    if let Some(instance) = remote_instance(&app, &id)? {
        ensure_remote_stopped(&app, &state, &instance).await?;
        return rollback_remote(&app, instance).await;
    }
    ensure_stopped(&state, &id)?;
    let entry = list_upgrade_history(app.clone(), id.clone()).into_iter().next().ok_or("No upgrade to roll back.")?;
    let instance = instance_by_id(&app, &id)?;
    let root = Path::new(&instance.path).canonicalize().map(plain_path).map_err(|error| error.to_string())?;
    let previous = PathBuf::from(&entry.previous_jar).canonicalize().map(plain_path).map_err(|_| "The previous jar was not found.")?;
    let stash = upgrade_dir(&app, &id)?;
    let stash_real = stash.canonicalize().map(plain_path).unwrap_or(stash);
    if !previous.starts_with(&root) && !previous.starts_with(&stash_real) {
        return Err("The previous jar path is malformed.".into());
    }
    let file_name = core_file_name(&instance.core);
    let current = root.join(file_name);
    let preserved = stash_real.join(format!("rollback-{}-{file_name}", unix_now()));
    if current.is_file() {
        move_file(&current, &preserved).map_err(|error| format!("Could not keep the current jar: {error}"))?;
    }
    if let Err(error) = move_file(&previous, &current) {
        if preserved.is_file() {
            let _ = move_file(&preserved, &current);
        }
        return Err(format!("Could not restore the previous jar: {error}"));
    }
    let _store = lock_instance_store();
    let mut instances = load_instances(&app)?;
    let saved = instances.iter_mut().find(|saved| saved.id == id).ok_or("Server not found.")?;
    saved.version = entry.from_version.clone();
    if let Some(core) = &entry.from_core {
        saved.core = core.clone();
    }
    saved.server_jar = Some(current.to_string_lossy().to_string());
    save_instances(&app, &instances)?;
    record_audit(&app, &id, "Upgrade", "Upgrade rolled back", &format!("Rolled back to version {}", entry.from_version));
    Ok(entry)
}

#[tauri::command(async)]
#[specta::specta]
fn list_automation_history(app: AppHandle, id: String) -> Result<Vec<AutomationHistoryEntry>, String> {
    let path = automation_history_path(&app)?;
    let mut entries = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<AutomationHistoryEntry>>(&text).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.server_id == id)
        .collect::<Vec<_>>();
    entries.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    Ok(entries)
}

fn record_far_sample(state: &AppState, id: &str, cpu: f32, memory: u64) {
    let tps = state.tps.lock().expect("tps state poisoned").get(id).and_then(|snapshot| snapshot.tps);
    record_resource_sample(state, id, cpu, memory, tps);
}

fn record_resource_sample(state: &AppState, id: &str, cpu: f32, memory: u64, tps: Option<f32>) {
    let heap = state.tps.lock().expect("tps state poisoned").get(id).and_then(|snapshot| snapshot.heap_mib);
    let memory = heap.or_else(|| no_addon(id).then_some(memory));
    let mut history = state.resource_history.lock().expect("resource history poisoned");
    let samples = history.entry(id.to_string()).or_default();
    samples.push_back(ResourceSample { at: unix_now(), cpu_percent: cpu, memory_mib: memory, tps });
    while samples.len() > 600 {
        samples.pop_front();
    }
}

#[tauri::command(async)]
#[specta::specta]
fn get_resource_history(state: State<'_, AppState>, id: String) -> Vec<ResourceSample> {
    state
        .resource_history
        .lock()
        .expect("resource history poisoned")
        .get(&id)
        .map(|samples| samples.iter().copied().collect())
        .unwrap_or_default()
}

fn instance_rcon(instance: &ServerInstance) -> Option<(u16, String)> {
    if pumpkin::is_pumpkin(&instance.core) {
        return pumpkin::rcon(Path::new(&instance.path));
    }
    let properties = fs::read_to_string(Path::new(&instance.path).join("server.properties")).ok()?;
    rcon::rcon_settings(&properties)
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct RconStatus {
    enabled: bool,
    port: u16,
    has_password: bool,
}

#[tauri::command(async)]
#[specta::specta]
fn get_rcon_status(app: AppHandle, id: String) -> Result<RconStatus, String> {
    let instance = instance_by_id(&app, &id)?;
    if is_far(&instance) {
        return Ok(RconStatus { enabled: false, port: 25575, has_password: false });
    }
    Ok(match instance_rcon(&instance) {
        Some((port, password)) => RconStatus { enabled: true, port, has_password: !password.is_empty() },
        None => RconStatus { enabled: false, port: 25575, has_password: false },
    })
}

#[tauri::command(async)]
#[specta::specta]
fn send_rcon_command(app: AppHandle, id: String, command: String) -> Result<String, String> {
    only_local(&app, &id, "RCON command")?;
    let command = command.trim();
    if command.is_empty() || command.len() > 500 || command.contains(['\r', '\n']) {
        return Err("Commands must be a single line of up to 500 characters.".into());
    }
    let instance = instance_by_id(&app, &id)?;
    let (port, password) =
        instance_rcon(&instance).ok_or("RCON is disabled on this server. Set enable-rcon=true in server.properties and restart.")?;
    let mut client = rcon::RconClient::connect("127.0.0.1", port, &password)?;
    let output = client.command(command)?;
    record_audit(&app, &id, "Console", "RCON command", "Command contents are not stored in the audit log for security.");
    Ok(output)
}

fn player_file(server: &Path, level: &str, kind: &str, name: &str) -> Option<PathBuf> {
    let world = server.join(level);
    let modern = world.join("players").join(kind).join(name);
    if modern.is_file() {
        return Some(modern);
    }
    let old_dir = if kind == "data" { "playerdata" } else { kind };
    let old = world.join(old_dir).join(name);
    old.is_file().then_some(old)
}

#[tauri::command(async)]
#[specta::specta]
async fn read_player_nbt(app: AppHandle, id: String, uuid: String) -> Result<nbt::PlayerSnapshot, String> {
    let instance = instance_by_id(&app, &id)?;
    let uuid = uuid.trim();
    if uuid.len() != 36 || !uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err("The UUID is malformed.".into());
    }
    let saved = read_saved_player(&app, &instance, uuid).await;
    let Some(live) = read_live_inventory(&app, &instance, uuid).await else {
        return saved;
    };
    let mut snapshot = saved.unwrap_or_else(|_| nbt::PlayerSnapshot::blank(uuid));
    snapshot.inventory = liveinv::merge(live.items, &snapshot.inventory);
    snapshot.ender_items = liveinv::merge(live.ender, &snapshot.ender_items);
    if let Ok(Some(state)) = read_live_inner(&app, &id).await {
        if let Some(person) = state.players.iter().find(|one| one.name == live.name) {
            snapshot.position = [person.x, person.y, person.z];
            snapshot.health = person.health as f32;
            snapshot.food = person.food as i32;
            if let Some(world) = &person.world {
                snapshot.dimension = world.clone();
            }
        }
    }
    snapshot.live = true;
    snapshot.modified_at = unix_now();
    Ok(snapshot)
}

/// Everything the info window shows about one player: statistics, sessions in the current log and the files
/// that hold their data (the world's and plugins').
#[tauri::command(async)]
#[specta::specta]
async fn player_info(app: AppHandle, id: String, uuid: String, name: String) -> Result<player_info::PlayerInfo, String> {
    let instance = instance_by_id(&app, &id)?;
    let uuid = uuid.trim().to_ascii_lowercase();
    if uuid.len() != 36 || !uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err("The UUID is malformed.".into());
    }
    let name = name.trim().to_string();
    let name = if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') { name } else { String::new() };
    let mut info = if is_far(&instance) {
        far_player_info(&app, &instance, &uuid).await
    } else {
        let root = PathBuf::from(&instance.path);
        let level = level_name_of(&instance);
        let (uuid, name) = (uuid.clone(), name.clone());
        off_runtime(move || local_player_info(&root, &level, &uuid, &name))
    };
    let log = if is_far(&instance) {
        far_read_log(&app, &instance, "latest.log", 20_000).await.unwrap_or_default()
    } else {
        fs::read_to_string(Path::new(&instance.path).join("logs").join("latest.log")).unwrap_or_default()
    };
    if !name.is_empty() {
        let (sessions, chats, commands) = player_info::read_log(&log, &name);
        info.sessions = sessions;
        info.chats = chats;
        info.commands = commands;
    }
    info.uuid = uuid;
    Ok(info)
}

fn local_player_info(root: &Path, level: &str, uuid: &str, name: &str) -> player_info::PlayerInfo {
    let mut info = player_info::PlayerInfo::default();
    let mut add = |path: &Path, kind: &str| {
        let Ok(meta) = fs::metadata(path) else { return };
        let modified_at = meta.modified().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map_or(0, |time| time.as_secs());
        let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        info.last_saved_at = info.last_saved_at.max(modified_at);
        info.files.push(player_info::PlayerDataFile { path: relative, kind: kind.into(), size: meta.len(), modified_at });
    };
    for file in [format!("{uuid}.dat"), format!("{uuid}.dat_old")] {
        if let Some(path) = player_file(root, level, "data", &file) {
            add(&path, "data");
        }
    }
    if let Some(path) = player_file(root, level, "stats", &format!("{uuid}.json")) {
        add(&path, "stats");
        let (play, stats) = player_info::read_stats(&fs::read_to_string(&path).unwrap_or_default());
        info.play_seconds = play;
        info.stats = stats;
    }
    if let Some(path) = player_file(root, level, "advancements", &format!("{uuid}.json")) {
        add(&path, "advancements");
        info.advancements = player_info::count_advancements(&fs::read_to_string(&path).unwrap_or_default());
    }
    // Plugins keep per-player files under their own folder; look a few levels down, not through map tiles.
    let mut seen = 0;
    let mut found = Vec::new();
    fn look(dir: &Path, depth: u32, uuid: &str, name: &str, seen: &mut u32, found: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            *seen += 1;
            if *seen > 20_000 || found.len() >= 50 {
                return;
            }
            let path = entry.path();
            if path.is_dir() {
                if depth < 3 {
                    look(&path, depth + 1, uuid, name, seen, found);
                }
            } else if player_info::names_player(&entry.file_name().to_string_lossy(), uuid, name) {
                found.push(path);
            }
        }
    }
    look(&root.join("plugins"), 0, uuid, name, &mut seen, &mut found);
    for path in found {
        let plugin = path
            .strip_prefix(root.join("plugins"))
            .ok()
            .and_then(|rest| rest.components().next())
            .map(|one| one.as_os_str().to_string_lossy().to_string())
            .unwrap_or_default();
        add(&path, &plugin);
    }
    info
}

async fn far_player_info(app: &AppHandle, instance: &ServerInstance, uuid: &str) -> player_info::PlayerInfo {
    let mut info = player_info::PlayerInfo::default();
    let level = far_read(app, instance, "server.properties")
        .await
        .ok()
        .and_then(|text| text.lines().find_map(|line| line.trim().strip_prefix("level-name=")).map(|value| value.trim().to_string()))
        .filter(|value| !value.is_empty() && !value.contains(['/', '\\', ':']))
        .unwrap_or_else(|| "world".into());
    for (modern, old, kind) in [
        ("players/data", "playerdata", "data"),
        ("players/stats", "stats", "stats"),
        ("players/advancements", "advancements", "advancements"),
    ] {
        let file = if kind == "data" { format!("{uuid}.dat") } else { format!("{uuid}.json") };
        for folder in [modern, old] {
            let path = format!("{level}/{folder}/{file}");
            let Ok(bytes) = far_read_bytes(app, instance, &path, 8 * 1024 * 1024).await else { continue };
            let text = String::from_utf8_lossy(&bytes);
            match kind {
                "stats" => {
                    let (play, stats) = player_info::read_stats(&text);
                    info.play_seconds = play;
                    info.stats = stats;
                }
                "advancements" => info.advancements = player_info::count_advancements(&text),
                _ => {}
            }
            info.files.push(player_info::PlayerDataFile { path, kind: kind.into(), size: bytes.len() as u64, modified_at: 0 });
            break;
        }
    }
    info
}

/// Change or remove one stack in a saved player file. The server has to be stopped (it would overwrite the file),
/// and the old file is kept under the app's restore folder first.
#[tauri::command(async)]
#[specta::specta]
async fn edit_player_item(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    uuid: String,
    ender: bool,
    slot: i32,
    count: i32,
) -> Result<(), String> {
    let instance = instance_by_id(&app, &id)?;
    let uuid = uuid.trim().to_string();
    if uuid.len() != 36 || !uuid.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err("The UUID is malformed.".into());
    }
    if !instance.panel_host.is_empty() {
        return Err("Editing items isn't available for panel servers yet.".into());
    }
    let keep = restore_dir(&app, &id)?.join("players");
    fs::create_dir_all(&keep).map_err(|error| error.to_string())?;
    let kept = keep.join(format!("{}-{uuid}.dat", unix_now()));
    if is_far(&instance) {
        ensure_remote_stopped(&app, &state, &instance).await?;
        let level = far_read(&app, &instance, "server.properties")
            .await
            .ok()
            .and_then(|text| text.lines().find_map(|line| line.trim().strip_prefix("level-name=")).map(|value| value.trim().to_string()))
            .filter(|value| !value.is_empty() && !value.contains(['/', '\\', ':']))
            .unwrap_or_else(|| "world".into());
        let mut found = None;
        for folder in ["players/data", "playerdata"] {
            let relative = format!("{level}/{folder}/{uuid}.dat");
            if let Ok(raw) = far_read_bytes(&app, &instance, &relative, 8 * 1024 * 1024).await {
                found = Some((relative, raw));
                break;
            }
        }
        let (relative, raw) = found.ok_or("No saved data. The server creates the file after a player joins and leaves once.")?;
        fs::write(&kept, &raw).map_err(|error| error.to_string())?;
        let changed = nbt::edit_player_item(raw, ender, slot as i64, count as i64)?;
        far_upload(&app, &instance, &relative, &changed, &format!("{uuid}.dat")).await?;
    } else {
        ensure_stopped(&state, &id)?;
        let path = player_file(Path::new(&instance.path), &level_name_of(&instance), "data", &format!("{uuid}.dat"))
            .ok_or("No saved data. The server creates the file after a player joins and leaves once.")?;
        let raw = fs::read(&path).map_err(|error| format!("Could not read the file: {error}"))?;
        fs::write(&kept, &raw).map_err(|error| error.to_string())?;
        let changed = nbt::edit_player_item(raw, ender, slot as i64, count as i64)?;
        let temp = path.with_extension("dat.monolab");
        fs::write(&temp, &changed).map_err(|error| format!("Could not save the file: {error}"))?;
        fs::rename(&temp, &path).map_err(|error| format!("Could not save the file: {error}"))?;
    }
    record_audit(&app, &id, "Player", "Edit saved item", &format!("{uuid} slot {slot} count {count}"));
    Ok(())
}

async fn read_live_inventory(app: &AppHandle, instance: &ServerInstance, uuid: &str) -> Option<liveinv::LiveInventory> {
    if instance_view(instance.clone(), &app.state::<AppState>()).status != "running" {
        return None;
    }
    let text = if is_far(instance) {
        far_read(app, instance, &format!("{}/inventories.json", remote_addon_data_dir(instance))).await.ok()?
    } else {
        fs::read_to_string(events::event_folder(Path::new(&instance.path)).join("inventories.json")).ok()?
    };
    liveinv::find(&text, uuid, unix_now() as f64 * 1000.0)
}

async fn read_saved_player(app: &AppHandle, instance: &ServerInstance, uuid: &str) -> Result<nbt::PlayerSnapshot, String> {
    let app = app.clone();
    let instance = instance.clone();
    if is_far(&instance) {
        let level = far_read(&app, &instance, "server.properties")
            .await
            .ok()
            .and_then(|text| text.lines().find_map(|line| line.trim().strip_prefix("level-name=")).map(|value| value.trim().to_string()))
            .filter(|value| !value.is_empty() && !value.contains(['/', '\\', ':']))
            .unwrap_or_else(|| "world".into());
        for folder in ["players/data", "playerdata"] {
            if let Ok(raw) = far_read_bytes(&app, &instance, &format!("{level}/{folder}/{uuid}.dat"), 8 * 1024 * 1024).await {
                return nbt::read_player_bytes(raw, uuid);
            }
        }
        return Err("No saved data. The server creates the file after a player joins and leaves once.".into());
    }
    let path = player_file(Path::new(&instance.path), &level_name_of(&instance), "data", &format!("{uuid}.dat"))
        .ok_or("No saved data. The server creates the file after a player joins and leaves once.")?;
    nbt::read_player_data(&path, uuid)
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct PlayerSpot {
    name: String,
    uuid: String,
    dimension: String,
    x: f64,
    z: f64,
    saved_at: u64,
}

#[tauri::command(async)]
#[specta::specta]
async fn list_player_spots(app: AppHandle, id: String) -> Result<Vec<PlayerSpot>, String> {
    let instance = instance_by_id(&app, &id)?;
    if is_far(&instance) {
        let people = list_players(app.clone(), id.clone()).await?;
        let mut found = Vec::new();
        for player in people.into_iter().filter(|one| one.uuid.len() == 36).take(60) {
            let Ok(snapshot) = read_saved_player(&app, &instance, &player.uuid).await else {
                continue;
            };
            found.push(PlayerSpot {
                name: player.name,
                uuid: player.uuid,
                dimension: snapshot.dimension,
                x: snapshot.position[0],
                z: snapshot.position[2],
                saved_at: snapshot.modified_at,
            });
        }
        return Ok(found);
    }
    let people = list_players(app.clone(), id.clone()).await?;
    off_runtime(move || {
        let server = Path::new(&instance.path);
        let level = level_name_of(&instance);
        let mut found = Vec::new();
        for player in people {
            let Some(path) = player_file(server, &level, "data", &format!("{}.dat", player.uuid)) else {
                continue;
            };
            let Ok(snapshot) = nbt::read_player_data(&path, &player.uuid) else {
                continue;
            };
            found.push(PlayerSpot {
                name: player.name,
                uuid: player.uuid,
                dimension: snapshot.dimension,
                x: snapshot.position[0],
                z: snapshot.position[2],
                saved_at: snapshot.modified_at,
            });
        }
        Ok(found)
    })
}

#[tauri::command(async)]
#[specta::specta]
async fn read_world_settings(app: AppHandle, id: String, world: String) -> Result<nbt::WorldSettings, String> {
    let instance = instance_by_id(&app, &id)?;
    if !instance.remote_host.is_empty() {
        let level = pull_remote_world(&app, &instance, &world).await?;
        return nbt::read_level_data(&level);
    }
    only_local(&app, &id, "World settings")?;
    let path = safe_world_level_path(&instance, &world)?;
    nbt::read_level_data(&path)
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct TrimReport {
    regions: u32,
    chunks: u32,
    removed: u32,
    bytes_before: f64,
    bytes_after: f64,
    applied: bool,
    backup: String,
}

fn trim_candidates(level: &str, realm: &str) -> Vec<String> {
    match realm {
        "nether" => vec![format!("{level}/dimensions/minecraft/the_nether"), format!("{level}/DIM-1"), format!("{level}_nether/DIM-1")],
        "end" => vec![format!("{level}/dimensions/minecraft/the_end"), format!("{level}/DIM1"), format!("{level}_the_end/DIM1")],
        _ => vec![format!("{level}/dimensions/minecraft/overworld"), level.to_string()],
    }
}

/// Files in a folder with (size, modified time), used to tell when a cached reading is stale.
async fn stamp_list(app: &AppHandle, instance: &ServerInstance, relative: &str) -> Vec<(String, (u64, u64))> {
    if is_far(instance) {
        return far_list(app, instance, relative)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|one| !one.directory)
            .map(|one| (one.name, (one.size_bytes, one.modified_at)))
            .collect();
    }
    fs::read_dir(Path::new(&instance.path).join(relative))
        .map(|entries| {
            entries
                .flatten()
                .filter(|one| one.file_type().is_ok_and(|kind| kind.is_file()))
                .map(|one| {
                    let meta = one.metadata().ok();
                    let size = meta.as_ref().map(|meta| meta.len()).unwrap_or(0);
                    let modified = meta
                        .and_then(|meta| meta.modified().ok())
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|time| time.as_secs())
                        .unwrap_or(0);
                    (one.file_name().to_string_lossy().to_string(), (size, modified))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn stats_cache_file(app: &AppHandle, id: &str, realm: &str, name: &str) -> Option<PathBuf> {
    Some(instance_data_dir(app, id).ok()?.join("chunk-stats").join(realm).join(format!("{name}.json")))
}

fn read_cached<T: serde::de::DeserializeOwned>(path: &Option<PathBuf>, stamp: (u64, u64)) -> Option<T> {
    let text = fs::read_to_string(path.as_ref()?).ok()?;
    let (saved, value): ((u64, u64), T) = serde_json::from_str(&text).ok()?;
    (saved == stamp).then_some(value)
}

fn write_cached<T: Serialize>(path: &Option<PathBuf>, stamp: (u64, u64), value: &T) {
    let Some(path) = path else { return };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(&(stamp, value)) {
        let _ = fs::write(path, text);
    }
}

async fn trim_list(app: &AppHandle, instance: &ServerInstance, relative: &str) -> Vec<(String, u64)> {
    if is_far(instance) {
        return far_list(app, instance, relative)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|one| !one.directory)
            .map(|one| (one.name, one.size_bytes))
            .collect();
    }
    fs::read_dir(Path::new(&instance.path).join(relative))
        .map(|entries| {
            entries
                .flatten()
                .filter(|one| one.file_type().is_ok_and(|kind| kind.is_file()))
                .map(|one| (one.file_name().to_string_lossy().to_string(), one.metadata().map(|meta| meta.len()).unwrap_or(0)))
                .collect()
        })
        .unwrap_or_default()
}

async fn trim_side_names(app: &AppHandle, instance: &ServerInstance, folder: &str, side: &str) -> Result<HashSet<String>, String> {
    if is_far(instance) {
        let parent = far_list(app, instance, folder).await.map_err(|error| format!("Could not list {}: {}", folder, error))?;
        if !parent.iter().any(|one| one.directory && one.name == side) {
            return Ok(HashSet::new());
        }
        let inside = format!("{folder}/{side}");
        let rows = far_list(app, instance, &inside).await.map_err(|error| format!("Could not list {}: {}", inside, error))?;
        return Ok(rows.into_iter().filter(|one| !one.directory).map(|one| one.name).collect());
    }
    match fs::read_dir(Path::new(&instance.path).join(folder).join(side)) {
        Ok(entries) => Ok(entries.flatten().map(|one| one.file_name().to_string_lossy().to_string()).collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(error) => Err(format!("Could not list {}/{}: {}", folder, side, error)),
    }
}

async fn trim_read(app: &AppHandle, instance: &ServerInstance, relative: &str) -> Option<Vec<u8>> {
    if is_far(instance) {
        return far_read_bytes(app, instance, relative, 256 * 1024 * 1024).await.ok();
    }
    fs::read(Path::new(&instance.path).join(relative)).ok()
}

async fn trim_write(app: &AppHandle, instance: &ServerInstance, relative: &str, bytes: Option<Vec<u8>>) -> Result<(), String> {
    if is_far(instance) {
        let body = bytes.unwrap_or_else(|| vec![0u8; 8192]);
        return remote::upload(app, &instance.remote_host, &instance.path, relative, &body).await;
    }
    let path = Path::new(&instance.path).join(relative);
    match bytes {
        Some(body) => {
            let temp = path.with_extension("mca.trimming");
            fs::write(&temp, body).map_err(|error| format!("Could not write {relative}: {error}"))?;
            fs::rename(&temp, &path).map_err(|error| format!("Could not replace {relative}: {error}"))
        }
        None => fs::remove_file(&path).map_err(|error| format!("Could not delete {relative}: {error}")),
    }
}

#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
async fn trim_chunks(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    realm: String,
    max_minutes: u32,
    keep_radius: u32,
    apply: bool,
    area: Option<Vec<i32>>,
) -> Result<TrimReport, String> {
    let area = match area.as_deref() {
        Some([x0, z0, x1, z1]) => {
            Some(((*x0.min(x1)).div_euclid(16), (*z0.min(z1)).div_euclid(16), (*x0.max(x1)).div_euclid(16), (*z0.max(z1)).div_euclid(16)))
        }
        Some(_) => return Err("The area is malformed.".into()),
        None => None,
    };
    let instance = instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() {
        return Err("Chunk cleanup isn't available for panel servers yet.".into());
    }
    if pumpkin::is_pumpkin(&instance.core) {
        let config = if is_far(&instance) {
            far_read(&app, &instance, pumpkin::CONFIG).await.ok()
        } else {
            std::fs::read_to_string(Path::new(&instance.path).join(pumpkin::CONFIG)).ok()
        };
        if let Some(kind) = config.as_deref().and_then(pumpkin::chunk_format).filter(|kind| kind != "anvil") {
            return Err(format!("Chunk cleanup only works with Anvil (.mca) worlds. This server stores chunks as {}.", kind));
        }
    }
    let level = if is_far(&instance) {
        far_read(&app, &instance, "server.properties")
            .await
            .ok()
            .and_then(|text| text.lines().find_map(|line| line.trim().strip_prefix("level-name=")).map(|value| value.trim().to_string()))
            .filter(|value| !value.is_empty() && !value.contains(['/', '\\', ':']))
            .unwrap_or_else(|| "world".into())
    } else {
        level_name_of(&instance)
    };
    let spawn = if is_far(&instance) {
        nbt::read_level_data(&pull_remote_world(&app, &instance, &level).await?)?.spawn
    } else {
        nbt::read_level_data(&Path::new(&instance.path).join(&level).join("level.dat"))?.spawn
    };
    let mut folder = None;
    for candidate in trim_candidates(&level, &realm) {
        let regions = trim_list(&app, &instance, &format!("{candidate}/region")).await;
        if regions.iter().any(|(name, _)| trim::region_coords(name).is_some()) {
            folder = Some((candidate, regions));
            break;
        }
    }
    let Some((folder, regions)) = folder else {
        return Err("This dimension has no region files yet.".into());
    };
    let regions: Vec<(String, u64)> = regions
        .into_iter()
        .filter(|(name, _)| match (trim::region_coords(name), area) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some((rx, rz)), Some((x0, z0, x1, z1))) => rx * 32 + 31 >= x0 && rx * 32 <= x1 && rz * 32 + 31 >= z0 && rz * 32 <= z1,
        })
        .collect();
    let rule = trim::Rule {
        max_minutes,
        spawn: (spawn[0].div_euclid(16), spawn[2].div_euclid(16)),
        keep_radius: keep_radius.min(512) as i32,
        area,
    };
    let mut backup = String::new();
    if apply {
        if is_far(&instance) {
            ensure_remote_stopped(&app, state.inner(), &instance).await?;
            backup = remote_backup(&app, &instance, "before-chunk-trim", false, &[]).await?;
        } else {
            ensure_stopped(state.inner(), &id)?;
            let vault = backup_folder(&app, &instance)?;
            let server = PathBuf::from(&instance.path);
            backup = tauri::async_runtime::spawn_blocking(move || backups::create(&server, "Before chunk trim", &vault, true))
                .await
                .map_err(|error| error.to_string())??
                .file_name;
        }
        record_audit(
            &app,
            &id,
            "Server",
            if area.is_some() { "Area chunks deleted" } else { "Chunk cleanup" },
            &format!("{folder} · {backup}"),
        );
    }
    let mut sides = Vec::new();
    if apply {
        for side in ["entities", "poi"] {
            sides.push((side, trim_side_names(&app, &instance, &folder, side).await?));
        }
    }
    let saved = backup.clone();
    let mut report =
        TrimReport { regions: regions.len() as u32, chunks: 0, removed: 0, bytes_before: 0.0, bytes_after: 0.0, applied: apply, backup };
    let total = regions.len();
    for (done, (name, _)) in regions.into_iter().enumerate() {
        let _ = app.emit("trim-progress", serde_json::json!({ "serverId": id, "done": done, "total": total }));
        let (rx, rz) = trim::region_coords(&name).unwrap_or((0, 0));
        let relative = format!("{folder}/region/{name}");
        let Some(bytes) = trim_read(&app, &instance, &relative).await else { continue };
        let before = bytes.len();
        let (plan, smaller) = tauri::async_runtime::spawn_blocking(move || {
            let plan = trim::plan(&bytes, rx, rz, &rule);
            let smaller = if plan.removed.is_empty() { Some(bytes) } else { trim::strip(&bytes, &plan.removed) };
            (plan, smaller)
        })
        .await
        .map_err(|error| error.to_string())?;
        report.chunks += plan.total as u32;
        report.removed += plan.removed.len() as u32;
        report.bytes_before += before as f64;
        report.bytes_after += smaller.as_ref().map_or(0, Vec::len) as f64;
        if !apply || plan.removed.is_empty() {
            continue;
        }
        let mut extras = Vec::new();
        for (side, names) in &sides {
            if !names.contains(&name) {
                continue;
            }
            let other = format!("{folder}/{side}/{name}");
            let Some(extra) = trim_read(&app, &instance, &other).await else {
                return Err(format!(
                    "Could not read {}, so cleanup stopped here. Earlier regions are already trimmed. Backup: {}",
                    other, saved
                ));
            };
            extras.push((other, extra));
        }
        let stopped = |error: String| format!("{} Cleanup stopped here. Backup: {}", error, saved);
        trim_write(&app, &instance, &relative, smaller).await.map_err(stopped)?;
        for (other, extra) in extras {
            let removed = plan.removed.clone();
            let cut =
                tauri::async_runtime::spawn_blocking(move || trim::strip(&extra, &removed)).await.map_err(|error| error.to_string())?;
            trim_write(&app, &instance, &other, cut).await.map_err(stopped)?;
        }
    }
    let _ = app.emit("trim-progress", serde_json::json!({ "serverId": id, "done": total, "total": total }));
    if apply {
        let cache = app.state::<AppState>();
        forget_chunk_stats(cache.inner(), &id);
        cache.map_cells.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
        cache.map_coarse.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
        cache.map_tiles.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
    }
    Ok(report)
}

const WORLD_SIDE_FILES: [&str; 2] = [
    "dimensions/minecraft/overworld/data/minecraft/game_rules.dat",
    "dimensions/minecraft/overworld/data/minecraft/world_gen_settings.dat",
];

async fn pull_remote_world(app: &AppHandle, instance: &ServerInstance, world: &str) -> Result<PathBuf, String> {
    let world = world.trim();
    if world.is_empty() || world.contains(['/', '\\', ':']) || world.starts_with('.') {
        return Err("The world name is malformed.".into());
    }
    let mirror = instance_data_dir(app, &instance.id)?.join("world-mirror").join(world);
    let _ = fs::remove_dir_all(&mirror);
    fs::create_dir_all(&mirror).map_err(|error| error.to_string())?;
    const LIMIT: u64 = 16 * 1024 * 1024;
    let level = remote::read_bytes(app, &instance.remote_host, &instance.path, &format!("{world}/level.dat"), LIMIT)
        .await
        .map_err(|error| format!("Could not download level.dat: {error}"))?;
    fs::write(mirror.join("level.dat"), level).map_err(|error| error.to_string())?;
    for side in WORLD_SIDE_FILES {
        if let Ok(bytes) = remote::read_bytes(app, &instance.remote_host, &instance.path, &format!("{world}/{side}"), LIMIT).await {
            let target = mirror.join(side);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(target, bytes).map_err(|error| error.to_string())?;
        }
    }
    Ok(mirror.join("level.dat"))
}

fn safe_world_level_path(instance: &ServerInstance, world: &str) -> Result<PathBuf, String> {
    let world = world.trim();
    if world.is_empty() || world.contains(['/', '\\', ':']) || world.starts_with('.') {
        return Err("The world name is malformed.".into());
    }
    paths::resolve_in_server(Path::new(&instance.path), &format!("{world}/level.dat")).map_err(|error| match error {
        paths::PathError::Missing => "level.dat was not found.".to_string(),
        paths::PathError::Escape => "Worlds outside the server folder cannot be opened.".to_string(),
        other => other.to_string(),
    })
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct WorldSettingsInput {
    id: String,
    world: String,
    difficulty: Option<u8>,
    hardcore: Option<bool>,
    spawn: Option<[i32; 3]>,
    #[serde(default)]
    game_rules: Vec<nbt::GameRule>,
}

#[tauri::command(async)]
#[specta::specta]
async fn save_world_settings(app: AppHandle, state: State<'_, AppState>, input: WorldSettingsInput) -> Result<(), String> {
    {
        let instance = instance_by_id(&app, &input.id)?;
        if !instance.remote_host.is_empty() {
            ensure_remote_stopped(&app, &state, &instance).await?;
            let level = pull_remote_world(&app, &instance, &input.world).await?;
            let original_hash = nbt::write_level_data(&level, input.difficulty, input.hardcore, input.spawn, &input.game_rules)?;
            let mirror = level.parent().map(Path::to_path_buf).unwrap_or_default();
            for side in ["level.dat", WORLD_SIDE_FILES[0]] {
                let local = mirror.join(side);
                if !local.is_file() {
                    continue;
                }
                let bytes = fs::read(&local).map_err(|error| error.to_string())?;
                remote::upload_watched(
                    &app,
                    &instance.remote_host,
                    &instance.path,
                    &format!("{}/{side}", input.world.trim()),
                    &bytes,
                    &|_, _| {},
                )
                .await?;
            }
            record_audit(
                &app,
                &input.id,
                "World",
                "Save level.dat (SSH)",
                &format!("{} · original SHA‑256 {}", input.world, &original_hash[..original_hash.len().min(16)]),
            );
            return Ok(());
        }
    }
    only_local(&app, &input.id, "Save world settings")?;
    ensure_stopped(&state, &input.id)?;
    let instance = instance_by_id(&app, &input.id)?;
    let path = safe_world_level_path(&instance, &input.world)?;
    let original_hash = nbt::write_level_data(&path, input.difficulty, input.hardcore, input.spawn, &input.game_rules)?;
    record_audit(
        &app,
        &input.id,
        "World",
        "level.dat save",
        &format!("{} · original SHA‑256 {}", input.world, &original_hash[..original_hash.len().min(16)]),
    );
    Ok(())
}

fn mojang_cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("mojang-cache");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

#[tauri::command(async)]
#[specta::specta]
fn lookup_mojang_profile(app: AppHandle, name: String) -> Result<mojang::MojangProfile, String> {
    off_runtime(|| mojang::lookup_by_name(&mojang_cache_dir(&app)?, &name))
}

#[tauri::command(async)]
#[specta::specta]
fn get_player_heads(app: AppHandle, uuids: Vec<String>) -> Result<HashMap<String, String>, String> {
    off_runtime(|| {
        let cache = mojang_cache_dir(&app)?;
        let mut heads = HashMap::new();
        for uuid in uuids.into_iter().take(60) {
            if let Some(profile) = mojang::head_for_uuid(&cache, &uuid) {
                if !profile.head.is_empty() {
                    heads.insert(uuid, profile.head);
                }
            }
        }
        Ok(heads)
    })
}

#[tauri::command(async)]
#[specta::specta]
fn get_heads_by_name(app: AppHandle, names: Vec<String>) -> Result<HashMap<String, String>, String> {
    off_runtime(|| {
        let cache = mojang_cache_dir(&app)?;
        let mut heads = HashMap::new();
        for name in names.into_iter().take(40) {
            if let Ok(profile) = mojang::lookup_by_name(&cache, &name) {
                if !profile.head.is_empty() {
                    heads.insert(name, profile.head);
                }
            }
        }
        Ok(heads)
    })
}

#[tauri::command(async)]
#[specta::specta]
async fn list_crash_reports(app: AppHandle, id: String) -> Result<Vec<crash::CrashReport>, String> {
    let instance = instance_by_id(&app, &id)?;
    if is_far(&instance) {
        let mut files: Vec<remote::FileRow> = far_list(&app, &instance, "crash-reports")
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|one| !one.directory && one.name.to_ascii_lowercase().ends_with(".txt"))
            .collect();
        files.sort_by_key(|one| std::cmp::Reverse(one.modified_at));
        files.truncate(10);
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let mut addons = Vec::new();
        for folder in ["mods", "plugins"] {
            for one in far_list(&app, &instance, folder).await.unwrap_or_default() {
                if one.name.to_ascii_lowercase().ends_with(".jar") {
                    addons.push(one.name);
                }
            }
        }
        let mut reports = Vec::new();
        for one in files {
            if let Ok(raw) = far_read_bytes(&app, &instance, &format!("crash-reports/{}", one.name), 4 * 1024 * 1024).await {
                let text = String::from_utf8_lossy(&raw);
                reports.push(crash::parse_text(&text, one.name, one.modified_at, &addons));
            }
        }
        return Ok(reports);
    }
    Ok(crash::list_reports(Path::new(&instance.path)))
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct CatalogPage {
    hits: Vec<catalog::CatalogHit>,
    has_more: bool,
    page: u32,
}

#[tauri::command(async)]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
fn search_addon_catalog(
    app: AppHandle,
    id: String,
    query: String,
    page: Option<u32>,
    categories: Option<Vec<String>>,
    environment: Option<String>,
    versions: Option<Vec<String>>,
    loader: Option<String>,
) -> Result<CatalogPage, String> {
    off_runtime(|| {
        let query = query.trim();
        let page = page.unwrap_or(0);
        let per_page = 20_u32;
        let categories = categories.unwrap_or_default();
        let environment = environment.unwrap_or_default();
        let instance = instance_by_id(&app, &id)?;
        // Pumpkin runs its own wasm plugins, which only Pumpkin Market lists.
        if pumpkin::is_pumpkin(&instance.core) {
            let client = crate::net::builder().timeout(Duration::from_secs(20)).build().map_err(|error| error.to_string())?;
            let hits = market::search(&client, query, page, categories.first().map_or("", String::as_str))?;
            let has_more = hits.len() as u32 >= market::PAGE;
            return Ok(CatalogPage { hits, has_more, page });
        }
        let versions = versions.unwrap_or_else(|| vec![instance.version.clone()]);
        let hits = search_modrinth_page(
            query,
            page,
            per_page,
            &categories,
            &environment,
            &instance,
            &versions,
            loader.as_deref().unwrap_or_default(),
        )?;
        let has_more = hits.len() as u32 >= per_page;
        Ok(CatalogPage { hits, has_more, page })
    })
}

fn is_hybrid(core: &str) -> bool {
    matches!(core, "Mohist" | "Arclight" | "Magma" | "Banner" | "Ketting" | "CatServer" | "Cardboard")
}

fn looks_like_version(text: &str) -> bool {
    let text = text.trim();
    !text.is_empty() && text.starts_with(|c: char| c.is_ascii_digit()) && text.chars().all(|c| c.is_ascii_graphic())
}

#[allow(clippy::too_many_arguments)]
fn search_modrinth_page(
    query: &str,
    page: u32,
    per_page: u32,
    categories: &[String],
    environment: &str,
    instance: &ServerInstance,
    versions: &[String],
    loader: &str,
) -> Result<Vec<catalog::CatalogHit>, String> {
    let client = crate::net::builder()
        .user_agent("Monolab/0.1.0 (local Windows desktop app)")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?;
    let mut facets: Vec<Vec<String>> = Vec::new();
    let kind = if matches!(instance.core.as_str(), "Paper" | "Purpur" | "Folia" | "Leaf" | "Spigot" | "Bukkit") { "plugin" } else { "mod" };
    if is_hybrid(&instance.core) {
        facets.push(vec!["project_type:mod".to_string(), "project_type:plugin".to_string()]);
    } else {
        facets.push(vec![format!("project_type:{kind}")]);
    }
    let picked = loader.trim();
    if picked == "all" {
        // No condition at all.
    } else if !picked.is_empty() {
        facets.push(vec![format!("categories:{picked}")]);
    } else if let Ok((_, found)) = modrinth_loader(instance, kind) {
        facets.push(vec![format!("categories:{found}")]);
    }
    let versions: Vec<&String> = versions.iter().filter(|one| looks_like_version(one)).collect();
    if !versions.is_empty() {
        facets.push(versions.iter().map(|one| format!("versions:{one}")).collect());
    }
    if !environment.is_empty() {
        facets.push(vec![format!("{environment}_side:required"), format!("{environment}_side:optional")]);
    }
    if !categories.is_empty() {
        facets.push(categories.iter().map(|item| format!("categories:{item}")).collect());
    }
    let facets = serde_json::to_string(&facets).map_err(|error| error.to_string())?;
    let offset = (page * per_page).to_string();
    let limit = per_page.to_string();
    let response: serde_json::Value = client
        .get("https://api.modrinth.com/v2/search")
        .query(&[
            ("query", query),
            ("limit", limit.as_str()),
            ("offset", offset.as_str()),
            ("facets", facets.as_str()),
            ("index", "downloads"),
        ])
        .send()
        .map_err(|error| format!("Modrinth search failed: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    Ok(response
        .get("hits")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|hit| {
            let project_id: String = hit.get("project_id")?.as_str()?.into();
            Some(catalog::CatalogHit {
                page_url: format!("https://modrinth.com/project/{project_id}"),
                source: "Modrinth".into(),
                project_id,
                title: hit.get("title")?.as_str()?.into(),
                description: hit.get("description").and_then(|value| value.as_str()).unwrap_or_default().into(),
                project_type: hit.get("project_type").and_then(|value| value.as_str()).unwrap_or("mod").into(),
                installable: true,
                icon_url: hit.get("icon_url").and_then(|value| value.as_str()).filter(|value| !value.is_empty()).map(str::to_owned),
                author: hit.get("author").and_then(|value| value.as_str()).map(str::to_owned),
                downloads: hit.get("downloads").and_then(|value| value.as_u64()),
                follows: hit.get("follows").and_then(|value| value.as_u64()),
                updated_at: hit
                    .get("date_modified")
                    .and_then(|value| value.as_str())
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .map(|value| value.timestamp()),
                loaders: hit
                    .get("categories")
                    .and_then(|value| value.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|value| value.as_str())
                    .filter(|name| catalog::is_loader(name))
                    .map(str::to_owned)
                    .collect(),
                versions: hit
                    .get("versions")
                    .and_then(|value| value.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect(),
            })
        })
        .collect())
}

fn instance_loader(instance: &ServerInstance) -> Result<(&'static str, &'static str), String> {
    match instance.core.as_str() {
        "Fabric" => Ok(("mods", "fabric")),
        "Forge" => Ok(("mods", "forge")),
        "NeoForge" => Ok(("mods", "neoforge")),
        "Paper" | "Purpur" | "Folia" | "Leaf" => Ok(("plugins", "paper")),
        "Velocity" => Ok(("plugins", "velocity")),
        "BungeeCord" => Ok(("plugins", "bungeecord")),
        _ => Err("This core does not support automatic mod/plugin management.".into()),
    }
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AddonReport {
    addons: Vec<addons::InstalledAddon>,
    conflicts: Vec<addons::AddonConflict>,
    unknown: usize,
    offline: bool,
}

#[tauri::command(async)]
#[specta::specta]
async fn inspect_addons(app: AppHandle, id: String) -> Result<AddonReport, String> {
    let remote_found = {
        let instance = instance_by_id(&app, &id)?;
        if !is_far(&instance) {
            None
        } else {
            Some(remote_addon_list(&app, &instance).await)
        }
    };
    off_runtime(move || {
        let instance = instance_by_id(&app, &id)?;
        let mut found = match remote_found {
            Some(list) => list,
            None => addons::scan(Path::new(&instance.path)),
        };
        let mut offline = false;
        if !found.is_empty() {
            match addons::client() {
                Ok(client) => {
                    if addons::identify(&client, &mut found).is_err() {
                        offline = true;
                    }
                }
                Err(_) => offline = true,
            }
        }
        let unknown = found.iter().filter(|addon| addon.project_id.is_none()).count();
        let conflicts = addons::find_conflicts(&found);
        Ok(AddonReport { addons: found, conflicts, unknown, offline })
    })
}

async fn remote_addon_list(app: &AppHandle, instance: &ServerInstance) -> Vec<addons::InstalledAddon> {
    if !instance.panel_host.is_empty() {
        let Ok((api, target)) = panel_of(app, instance) else { return Vec::new() };
        panel_backfill_market(&api, &target).await;
        let installs = api.market_installed(&target.ident).await;
        let mut list = Vec::new();
        for folder in ["mods", "plugins"] {
            for one in pfs::addon_files(&api, &target, folder).await {
                let plain = one.name.trim_end_matches(".disabled").to_string();
                let record = installs.iter().find(|each| each.filename.as_deref() == Some(plain.as_str()) && each.source == "modrinth");
                let facts = panel_jar_facts()
                    .lock()
                    .expect("jar facts poisoned")
                    .get(&format!("{}/{folder}/{plain}", target.ident))
                    .filter(|(size, modified, _)| *size == one.size_bytes && *modified == one.modified_at)
                    .map(|(_, _, facts)| facts.clone());
                list.push(addons::InstalledAddon {
                    display_name: record
                        .and_then(|each| each.project_title.clone())
                        .or_else(|| facts.as_ref().map(|(_, name, _)| name.clone()))
                        .unwrap_or_else(|| plain.trim_end_matches(".jar").to_string()),
                    mod_id: facts.as_ref().map(|(id, _, _)| id.clone()).unwrap_or_default(),
                    version: record
                        .and_then(|each| each.version_name.clone())
                        .or_else(|| facts.as_ref().map(|(_, _, version)| version.clone()))
                        .unwrap_or_default(),
                    enabled: one.name.ends_with(".jar"),
                    file_name: one.name,
                    sha512: String::new(),
                    project_id: record.map(|each| each.project_id.clone()),
                    project_title: record.and_then(|each| each.project_title.clone()),
                    loaders: Vec::new(),
                    game_versions: Vec::new(),
                });
            }
        }
        return list;
    }
    let mut list = Vec::new();
    for folder in ["mods", "plugins"] {
        for one in remote::addon_files(app, &instance.remote_host, &instance.path, folder).await.unwrap_or_default() {
            let enabled = one.name.ends_with(".jar");
            list.push(addons::InstalledAddon {
                display_name: one.name.trim_end_matches(".disabled").trim_end_matches(".jar").to_string(),
                mod_id: String::new(),
                version: String::new(),
                file_name: one.name,
                enabled,
                sha512: one.hash,
                project_id: None,
                project_title: None,
                loaders: Vec::new(),
                game_versions: Vec::new(),
            });
        }
    }
    list
}

type JarFactsCache = Mutex<HashMap<String, (u64, u64, (String, String, String))>>;

fn panel_jar_facts() -> &'static JarFactsCache {
    static FACTS: std::sync::OnceLock<JarFactsCache> = std::sync::OnceLock::new();
    FACTS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn panel_backfill_market(api: &panel::Api, target: &panel::Target) {
    use sha2::{Digest, Sha512};
    static UNKNOWN: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    let unknown = UNKNOWN.get_or_init(|| Mutex::new(HashSet::new()));
    let installs = api.market_installed(&target.ident).await;
    let mut pending: Vec<(String, String, String)> = Vec::new();
    for (folder, kind) in [("mods", "mod"), ("plugins", "plugin")] {
        for one in pfs::addon_files(api, target, folder).await {
            let plain = one.name.trim_end_matches(".disabled").to_string();
            if !plain.ends_with(".jar") || installs.iter().any(|each| each.filename.as_deref() == Some(plain.as_str())) {
                continue;
            }
            let key = format!("{}/{folder}/{}/{}/{}", target.ident, one.name, one.size_bytes, one.modified_at);
            if unknown.lock().expect("market poisoned").contains(&key) || one.size_bytes > 100 * 1024 * 1024 || pending.len() >= 30 {
                continue;
            }
            let Ok(bytes) = api.read_bytes(target, &format!("{folder}/{}", one.name), 100 * 1024 * 1024).await else { continue };
            let hash: String = Sha512::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
            unknown.lock().expect("market poisoned").insert(key);
            if let Some(facts) = addons::identity_of_bytes(&bytes, plain.trim_end_matches(".jar")) {
                panel_jar_facts()
                    .lock()
                    .expect("jar facts poisoned")
                    .insert(format!("{}/{folder}/{plain}", target.ident), (one.size_bytes, one.modified_at, facts));
            }
            pending.push((kind.to_string(), plain, hash));
        }
    }
    if pending.is_empty() {
        return;
    }
    let hashes: Vec<String> = pending.iter().map(|one| one.2.clone()).collect();
    let found = tauri::async_runtime::spawn_blocking(move || -> Option<(serde_json::Value, serde_json::Value)> {
        let client = addons::client().ok()?;
        let versions: serde_json::Value = client
            .post("https://api.modrinth.com/v2/version_files")
            .json(&serde_json::json!({ "hashes": hashes, "algorithm": "sha512" }))
            .send()
            .ok()?
            .json()
            .ok()?;
        let ids: Vec<String> =
            versions.as_object()?.values().filter_map(|one| one.get("project_id").and_then(|id| id.as_str()).map(str::to_string)).collect();
        let projects: serde_json::Value = client
            .get("https://api.modrinth.com/v2/projects")
            .query(&[("ids", serde_json::to_string(&ids).ok()?)])
            .send()
            .ok()?
            .json()
            .ok()?;
        Some((versions, projects))
    })
    .await
    .ok()
    .flatten();
    let Some((versions, projects)) = found else { return };
    let title_of = |project: &str| {
        projects.as_array().and_then(|list| {
            list.iter()
                .find(|one| one.get("id").and_then(|id| id.as_str()) == Some(project))
                .and_then(|one| one.get("title").and_then(|title| title.as_str()).map(str::to_string))
        })
    };
    for (kind, file, hash) in pending {
        let Some(version) = versions.get(&hash) else { continue };
        let Some(project) = version.get("project_id").and_then(|id| id.as_str()) else { continue };
        let record = panel::MarketInstall {
            kind,
            source: "modrinth".into(),
            project_id: project.to_string(),
            project_title: Some(title_of(project).unwrap_or_else(|| file.trim_end_matches(".jar").to_string())),
            version_id: version.get("id").and_then(|id| id.as_str()).map(str::to_string),
            version_name: version.get("version_number").and_then(|id| id.as_str()).map(str::to_string),
            filename: Some(file),
        };
        let _ = api.market_record(&target.ident, &record).await;
    }
}

#[tauri::command(async)]
#[specta::specta]
async fn check_addon_updates(app: AppHandle, id: String) -> Result<Vec<addons::AddonUpdate>, String> {
    let remote_found = {
        let instance = instance_by_id(&app, &id)?;
        if !is_far(&instance) {
            None
        } else {
            Some(remote_addon_list(&app, &instance).await)
        }
    };
    off_runtime(move || {
        let instance = instance_by_id(&app, &id)?;
        let (_, loader) = instance_loader(&instance)?;
        let mut found = match remote_found {
            Some(list) => list,
            None => addons::scan(Path::new(&instance.path)),
        };
        if found.is_empty() {
            return Ok(Vec::new());
        }
        let client = addons::client()?;
        addons::identify(&client, &mut found)?;
        addons::find_updates(&client, &found, loader, &instance.version)
    })
}

#[tauri::command(async)]
#[specta::specta]
async fn apply_addon_updates(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    file_names: Vec<String>,
) -> Result<AddonUpdateResult, String> {
    if let Some(instance) = remote_instance(&app, &id)? {
        return apply_addon_updates_remote(&app, &state, &instance, &file_names).await;
    }
    off_runtime(|| {
        let running = is_running(&state, &id);
        let instance = instance_by_id(&app, &id)?;
        let (folder_name, loader) = instance_loader(&instance)?;
        let folder = Path::new(&instance.path).join(folder_name);
        let keep = addon_restore_dir(&app, &id)?;
        let resting = pending_dir(&app, &id)?;
        let mut found = addons::scan(Path::new(&instance.path));
        let client = addons::client()?;
        addons::identify(&client, &mut found)?;
        let updates = addons::find_updates(&client, &found, loader, &instance.version)?;

        let mut result = AddonUpdateResult::default();
        for update in updates.into_iter().filter(|update| file_names.contains(&update.file_name)) {
            let current = folder.join(&update.file_name);
            if !current.is_file() {
                continue;
            }
            let staging = folder.join(format!(".{}.download", update.new_file_name));
            download_file(&client, &update.download_url, &staging)?;
            if let Err(error) = verify_sha512(&staging, &update.sha512) {
                let _ = fs::remove_file(&staging);
                return Err(error);
            }

            if running {
                pending::queue(
                    &resting,
                    pending::Waiting {
                        id: format!("{}-{}", unix_now(), result.queued),
                        replaces: update.file_name.clone(),
                        file_name: update.new_file_name.clone(),
                        display_name: update.display_name.clone(),
                        from_version: update.current_version.clone(),
                        to_version: update.new_version.clone(),
                        staged: String::new(),
                        queued_at: unix_now(),
                        source: "Repository".into(),
                    },
                    &staging,
                )?;
                record_audit(
                    &app,
                    &id,
                    "Mod",
                    "Update queued",
                    &format!("{} {} -> {} (applied on restart)", update.display_name, update.current_version, update.new_version),
                );
                result.queued += 1;
                continue;
            }

            let stamp = unix_now();
            move_file(&current, &keep.join(format!("{stamp}-{}", update.file_name)))
                .map_err(|error| format!("Could not keep the previous file: {error}"))?;
            fs::rename(&staging, folder.join(&update.new_file_name)).map_err(|error| format!("Could not install the new file: {error}"))?;
            record_audit(
                &app,
                &id,
                "Mod",
                "Update applied",
                &format!("{} {} -> {}", update.display_name, update.current_version, update.new_version),
            );
            result.applied += 1;
        }
        Ok(result)
    })
}

async fn apply_pending_remote(app: &AppHandle, instance: &ServerInstance) {
    let Ok(resting) = pending_dir(app, &instance.id) else { return };
    let Ok((folder_name, _)) = instance_loader(instance) else { return };
    let mut done = Vec::new();
    for one in pending::list(&resting) {
        let Ok(bytes) = fs::read(resting.join(&one.staged)) else { continue };
        if !one.replaces.is_empty() && one.replaces != one.file_name {
            remote::drop_path(app, &instance.remote_host, &instance.path, &format!("{folder_name}/{}", one.replaces)).await;
        }
        let put = remote::upload(app, &instance.remote_host, &instance.path, &format!("{folder_name}/{}", one.file_name), &bytes).await;
        if put.is_ok() {
            let _ = pending::cancel(&resting, &one.id);
            record_audit(
                app,
                &instance.id,
                "Mod",
                "Applied queued update (SSH)",
                &format!("{} {} -> {}", one.display_name, one.from_version, one.to_version),
            );
            done.push(one.display_name);
        }
    }
    if !done.is_empty() {
        let _ = announce(app, &instance.id, "Server", &format!("Applied {} pending update(s): {}", done.len(), done.join(", ")));
    }
}

async fn apply_addon_updates_remote(
    app: &AppHandle,
    state: &AppState,
    instance: &ServerInstance,
    file_names: &[String],
) -> Result<AddonUpdateResult, String> {
    let running = state.remote_running.lock().expect("remote state poisoned").contains_key(&instance.id);
    let (folder_name, loader) = instance_loader(instance)?;
    let mut found = remote_addon_list(app, instance).await;
    if found.is_empty() {
        return Ok(AddonUpdateResult::default());
    }
    let version = instance.version.clone();
    let updates = tokio::task::spawn_blocking(move || -> Result<Vec<addons::AddonUpdate>, String> {
        let client = addons::client()?;
        addons::identify(&client, &mut found)?;
        addons::find_updates(&client, &found, loader, &version)
    })
    .await
    .map_err(|error| error.to_string())??;
    let mut result = AddonUpdateResult::default();
    for update in updates.into_iter().filter(|update| file_names.contains(&update.file_name)) {
        let staging = std::env::temp_dir().join(format!("monolab-{}", update.new_file_name));
        let (url, sha512, place) = (update.download_url.clone(), update.sha512.clone(), staging.clone());
        let got = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
            let client = addons::client()?;
            download_file(&client, &url, &place)
                .and_then(|()| verify_sha512(&place, &sha512))
                .and_then(|()| fs::read(&place).map_err(|error| error.to_string()))
        })
        .await
        .map_err(|error| error.to_string())?;
        if running {
            let queued = got.and_then(|_| {
                pending::queue(
                    &pending_dir(app, &instance.id)?,
                    pending::Waiting {
                        id: format!("{}-{}", unix_now(), result.queued),
                        replaces: update.file_name.clone(),
                        file_name: update.new_file_name.clone(),
                        display_name: update.display_name.clone(),
                        from_version: update.current_version.clone(),
                        to_version: update.new_version.clone(),
                        staged: String::new(),
                        queued_at: unix_now(),
                        source: "Repository".into(),
                    },
                    &staging,
                )
            });
            let _ = fs::remove_file(&staging);
            queued?;
            result.queued += 1;
            continue;
        }
        let _ = fs::remove_file(&staging);
        let bytes = got?;
        if update.file_name != update.new_file_name {
            remote::drop_path(app, &instance.remote_host, &instance.path, &format!("{folder_name}/{}", update.file_name)).await;
        }
        watched_upload(
            app,
            &instance.remote_host,
            &instance.path,
            &format!("{folder_name}/{}", update.new_file_name),
            &bytes,
            &update.new_file_name,
        )
        .await?;
        record_audit(
            app,
            &instance.id,
            "Mod",
            "Apply updates (SSH)",
            &format!("{} {} -> {}", update.display_name, update.current_version, update.new_version),
        );
        result.applied += 1;
    }
    Ok(result)
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AddonUpdateResult {
    applied: usize,
    queued: usize,
}

#[tauri::command(async)]
#[specta::specta]
fn list_pending_addons(app: AppHandle, id: String) -> Result<Vec<pending::Waiting>, String> {
    let _ = instance_by_id(&app, &id)?;
    Ok(pending::list(&pending_dir(&app, &id)?))
}

#[tauri::command(async)]
#[specta::specta]
fn cancel_pending_addon(app: AppHandle, id: String, entry_id: String) -> Result<(), String> {
    let _ = instance_by_id(&app, &id)?;
    pending::cancel(&pending_dir(&app, &id)?, &entry_id)?;
    record_audit(&app, &id, "Mods & plugins", "Queue cancelled", &entry_id);
    Ok(())
}

fn apply_pending_addons(app: &AppHandle, instance: &ServerInstance) {
    let home = Path::new(&instance.path);
    let Ok(resting) = pending_dir(app, &instance.id) else {
        return;
    };
    if pending::list(&resting).is_empty() {
        return;
    }
    let Ok((folder_name, _)) = instance_loader(instance) else {
        return;
    };
    let Ok(keep) = addon_restore_dir(app, &instance.id) else {
        return;
    };
    let done = pending::apply(&resting, &home.join(folder_name), &keep, unix_now());
    for one in &done {
        record_audit(
            app,
            &instance.id,
            "Mod",
            "Queued updates applied",
            &format!("{} {} -> {}", one.display_name, one.from_version, one.to_version),
        );
    }
    if !done.is_empty() {
        let names: Vec<String> = done.iter().map(|one| one.display_name.clone()).collect();
        let _ = announce(app, &instance.id, "Server", &format!("Applied {} pending update(s): {}", done.len(), names.join(", ")));
    }
}

fn addon_restore_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let path = restore_dir(app, id)?.join("addons");
    fs::create_dir_all(&path).map_err(|error| format!("Could not create the folder for previous mods: {error}"))?;
    Ok(path)
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AddonRollbackEntry {
    file_name: String,
    stored_name: String,
    created_at: u64,
}

#[tauri::command(async)]
#[specta::specta]
fn list_addon_rollbacks(app: AppHandle, id: String) -> Result<Vec<AddonRollbackEntry>, String> {
    let _ = instance_by_id(&app, &id)?;
    let keep = addon_restore_dir(&app, &id)?;
    let mut entries = fs::read_dir(keep)
        .map_err(|error| error.to_string())?
        .flatten()
        .filter_map(|entry| {
            let stored_name = entry.file_name().to_string_lossy().to_string();
            let (stamp, original) = stored_name.split_once('-')?;
            Some(AddonRollbackEntry {
                file_name: original.to_string(),
                stored_name: stored_name.clone(),
                created_at: stamp.parse().unwrap_or(0),
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.created_at));
    entries.truncate(40);
    Ok(entries)
}

#[tauri::command(async)]
#[specta::specta]
fn rollback_addon_update(app: AppHandle, state: State<'_, AppState>, id: String, stored_name: String) -> Result<String, String> {
    only_local(&app, &id, "Revert to previous version")?;
    ensure_stopped(&state, &id)?;
    let instance = instance_by_id(&app, &id)?;
    let (folder_name, _) = instance_loader(&instance)?;
    let keep = addon_restore_dir(&app, &id)?;
    if stored_name.contains(['/', '\\']) {
        return Err("The kept file name is malformed.".into());
    }
    let source = keep.join(&stored_name);
    if !source.is_file() {
        return Err("The kept previous file was not found.".into());
    }
    let original = stored_name.split_once('-').map(|(_, rest)| rest.to_string()).unwrap_or_else(|| stored_name.clone());
    let folder = Path::new(&instance.path).join(folder_name);
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    move_file(&source, &folder.join(&original)).map_err(|error| format!("Could not restore the previous file: {error}"))?;
    record_audit(&app, &id, "Mod", "Update rolled back", &original);
    Ok(original)
}

#[tauri::command(async)]
#[specta::specta]
fn plan_addon_install(
    app: AppHandle,
    id: String,
    project_id: String,
    installed: Option<Vec<String>>,
) -> Result<addons::InstallPlan, String> {
    off_runtime(|| {
        let instance = instance_by_id(&app, &id)?;
        let (_, loader) = instance_loader(&instance)?;
        let client = addons::client()?;
        let installed = known_installed(&client, &instance, installed);
        addons::plan_install(&client, &project_id, loader, &instance.version, &installed)
    })
}

fn known_installed(
    client: &reqwest::blocking::Client,
    instance: &ServerInstance,
    given: Option<Vec<String>>,
) -> std::collections::HashSet<String> {
    if let Some(ids) = given {
        return ids.into_iter().collect();
    }
    if !instance.remote_host.is_empty() {
        return std::collections::HashSet::new();
    }
    let mut installed = addons::scan(Path::new(&instance.path));
    let _ = addons::identify(client, &mut installed);
    installed.into_iter().filter_map(|one| one.project_id).collect()
}

#[tauri::command(async)]
#[specta::specta]
fn install_addon_with_dependencies(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    project_id: String,
    dependencies: Vec<String>,
    installed: Option<Vec<String>>,
) -> Result<usize, String> {
    off_runtime(|| {
        let running = is_running(&state, &id);
        let instance = instance_by_id(&app, &id)?;
        let (folder_name, loader) = instance_loader(&instance)?;
        let client = addons::client()?;
        let installed = known_installed(&client, &instance, installed);
        let plan = addons::plan_install(&client, &project_id, loader, &instance.version, &installed)?;

        let mut queue = vec![(plan.file_name.clone(), plan.download_url.clone(), plan.sha512.clone())];
        for dependency in &plan.dependencies {
            if dependency.already_installed {
                continue;
            }
            if !dependencies.contains(&dependency.project_id) {
                continue;
            }
            queue.push((dependency.file_name.clone(), dependency.download_url.clone(), dependency.sha512.clone()));
        }

        if !instance.remote_host.is_empty() {
            if running {
                return Err("Can't install over SSH while the server is running. Please stop it first.".into());
            }
            let here = std::env::temp_dir();
            let mut bodies = Vec::new();
            for (name, url, hash) in &queue {
                let staging = here.join(format!("monolab-{name}"));
                let got = download_file_watched(&client, url, &staging, &mut |received, total| {
                    tell_addon_download(&app, &id, &project_id, name, received, total);
                })
                .and_then(|()| verify_sha512(&staging, hash))
                .and_then(|()| fs::read(&staging).map_err(|error| error.to_string()));
                let _ = fs::remove_file(&staging);
                match got {
                    Ok(bytes) => bodies.push((name.clone(), bytes)),
                    Err(error) => return Err(error),
                }
            }
            for (name, bytes) in &bodies {
                tauri::async_runtime::block_on(watched_upload(
                    &app,
                    &instance.remote_host,
                    &instance.path,
                    &format!("{folder_name}/{name}"),
                    bytes,
                    name,
                ))?;
            }
            record_audit(
                &app,
                &id,
                "Mod",
                "Install with dependencies (SSH)",
                &format!("{} and {} more", plan.title, queue.len().saturating_sub(1)),
            );
            return Ok(queue.len());
        }

        let folder = Path::new(&instance.path).join(folder_name);
        fs::create_dir_all(&folder).map_err(|error| error.to_string())?;

        if running {
            if let Some((name, _, _)) = queue.iter().find(|(name, _, _)| folder.join(name).exists()) {
                return Err(format!("{name} already exists. A running server may be using it — stop the server before replacing it."));
            }
        }
        let mut staged = Vec::new();
        for (name, url, hash) in &queue {
            let staging = folder.join(format!(".{name}.download"));
            if let Err(error) = download_file_watched(&client, url, &staging, &mut |received, total| {
                tell_addon_download(&app, &id, &project_id, name, received, total);
            })
            .and_then(|()| verify_sha512(&staging, hash))
            {
                for path in &staged {
                    let _ = fs::remove_file(path);
                }
                let _ = fs::remove_file(&staging);
                return Err(error);
            }
            staged.push(staging);
        }
        for ((name, _, _), staging) in queue.iter().zip(staged) {
            fs::rename(&staging, folder.join(name)).map_err(|error| format!("Could not install the file: {error}"))?;
        }
        record_audit(
            &app,
            &id,
            "Mod",
            "Installed with dependencies",
            &format!("{} and {} more", plan.title, queue.len().saturating_sub(1)),
        );
        Ok(queue.len())
    })
}

#[tauri::command(async)]
#[specta::specta]
async fn list_log_files(app: AppHandle, id: String) -> Result<Vec<logs::LogFile>, String> {
    {
        let instance = instance_by_id(&app, &id)?;
        if is_far(&instance) {
            return Ok(far_log_files(&app, &instance)
                .await
                .into_iter()
                .map(|one| logs::LogFile {
                    compressed: one.name.ends_with(".gz"),
                    file_name: one.name,
                    size_bytes: one.size_bytes,
                    modified_at: one.modified_at,
                })
                .collect());
        }
    }
    let instance = instance_by_id(&app, &id)?;
    Ok(logs::list_files(Path::new(&instance.path)))
}

async fn log_lines_of(app: &AppHandle, id: &str, file_name: &str, limit: u32) -> Result<Vec<String>, String> {
    let instance = instance_by_id(app, id)?;
    if is_far(&instance) {
        let text = far_read_log(app, &instance, file_name, limit).await?;
        return Ok(text.lines().map(str::to_string).collect());
    }
    let path = logs::safe_log_path(Path::new(&instance.path), file_name)?;
    logs::read_lines(&path, limit as usize)
}

#[tauri::command(async)]
#[specta::specta]
async fn read_log_timeline(app: AppHandle, id: String, file_name: String, kind: String) -> Result<Vec<logs::TimelineEvent>, String> {
    let lines = log_lines_of(&app, &id, &file_name, 20_000).await?;
    let wanted = kind.trim();
    Ok(logs::timeline(&lines)
        .into_iter()
        .filter(|event| wanted.is_empty() || wanted == "All" || event.kind == wanted)
        .rev()
        .take(800)
        .collect())
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*byte as char),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct SharePreview {
    lines: u32,
    masked: logs::Masked,
    sample: Vec<String>,
}

#[tauri::command(async)]
#[specta::specta]
async fn preview_share_log(app: AppHandle, id: String, file_name: String) -> Result<SharePreview, String> {
    let lines = log_lines_of(&app, &id, &file_name, 200_000).await?;
    let (text, masked) = logs::mask(&lines.join("\n"));
    let sample = text
        .lines()
        .filter(|line| line.contains("[IP]") || line.contains("[USER]") || line.contains("[REDACTED]"))
        .take(4)
        .map(|line| line.chars().take(160).collect())
        .collect();
    Ok(SharePreview { lines: lines.len() as u32, masked, sample })
}

#[tauri::command(async)]
#[specta::specta]
async fn share_log(app: AppHandle, id: String, file_name: String) -> Result<String, String> {
    let lines = log_lines_of(&app, &id, &file_name, 200_000).await?;
    let (mut text, _) = logs::mask(&lines.join("\n"));
    const LIMIT: usize = 9_000_000;
    if text.len() > LIMIT {
        let cut = text.len() - LIMIT;
        text = text.split_off(cut);
        text.insert_str(0, "(the beginning was too long and was cut)\n");
    }
    off_runtime(move || {
        let client = crate::net::builder()
            .user_agent("Monolab (local Windows desktop app)")
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|error| error.to_string())?;
        let body = format!("content={}", percent_encode(&text));
        let answer: serde_json::Value = client
            .post("https://api.mclo.gs/1/log")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .map_err(|error| format!("Upload to mclo.gs failed: {error}"))?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .map_err(|error| error.to_string())?;
        if answer.get("success").and_then(|one| one.as_bool()) != Some(true) {
            return Err(format!("mclo.gs rejected it: {}", answer.get("error").and_then(|one| one.as_str()).unwrap_or("Unknown reason")));
        }
        answer.get("url").and_then(|one| one.as_str()).map(str::to_owned).ok_or_else(|| "mclo.gs did not return a URL.".into())
    })
}

#[tauri::command(async)]
#[specta::specta]
async fn summarize_logs(app: AppHandle, id: String) -> Result<Vec<logs::DailySummary>, String> {
    let instance = instance_by_id(&app, &id)?;
    let mut summaries = Vec::new();
    if is_far(&instance) {
        for file in far_log_files(&app, &instance).await.into_iter().take(5) {
            let Ok(text) = far_read_log(&app, &instance, &file.name, 20_000).await else {
                continue;
            };
            let lines: Vec<String> = text.lines().map(str::to_string).collect();
            summaries.push(logs::summarize(&file.name, &lines));
        }
        return Ok(summaries);
    }
    let root = Path::new(&instance.path);
    for file in logs::list_files(root).into_iter().take(14) {
        let path = root.join("logs").join(&file.file_name);
        let Ok(lines) = logs::read_lines(&path, 20_000) else {
            continue;
        };
        summaries.push(logs::summarize(&file.file_name, &lines));
    }
    Ok(summaries)
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct Bookmark {
    id: String,
    server_id: String,
    name: String,
    dimension: String,
    x: i64,
    y: i64,
    z: i64,
    #[serde(default)]
    note: String,
    created_at: u64,
}

fn bookmarks_path(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&data_dir).map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(data_dir.join("bookmarks.json"))
}

fn load_bookmarks(app: &AppHandle) -> Vec<Bookmark> {
    bookmarks_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

#[tauri::command(async)]
#[specta::specta]
fn list_bookmarks(app: AppHandle, id: String) -> Result<Vec<Bookmark>, String> {
    let mut found = load_bookmarks(&app).into_iter().filter(|bookmark| bookmark.server_id == id).collect::<Vec<_>>();
    found.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(found)
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct BookmarkInput {
    id: Option<String>,
    server_id: String,
    name: String,
    dimension: String,
    x: i64,
    y: i64,
    z: i64,
    #[serde(default)]
    note: String,
}

#[tauri::command(async)]
#[specta::specta]
fn save_bookmark(app: AppHandle, input: BookmarkInput) -> Result<Bookmark, String> {
    let name = input.name.trim();
    if name.is_empty() || name.len() > 60 {
        return Err("Bookmark names are 1–60 characters.".into());
    }
    if !matches!(input.dimension.as_str(), "overworld" | "nether" | "end") {
        return Err("The dimension must be Overworld, Nether or End.".into());
    }
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let mut all = load_bookmarks(&app);
    let id = input.id.filter(|value| !value.trim().is_empty()).unwrap_or_else(|| format!("mark-{millis}"));
    let entry = Bookmark {
        created_at: all.iter().find(|mark| mark.id == id).map(|mark| mark.created_at).unwrap_or_else(unix_now),
        id: id.clone(),
        server_id: input.server_id,
        name: name.into(),
        dimension: input.dimension,
        x: input.x,
        y: input.y,
        z: input.z,
        note: input.note.trim().chars().take(300).collect(),
    };
    match all.iter().position(|mark| mark.id == id) {
        Some(index) => all[index] = entry.clone(),
        None => all.push(entry.clone()),
    }
    let text = serde_json::to_string_pretty(&all).map_err(|error| error.to_string())?;
    fs::write(bookmarks_path(&app)?, text).map_err(|error| format!("Could not save the bookmark: {error}"))?;
    Ok(entry)
}

#[tauri::command(async)]
#[specta::specta]
fn delete_bookmark(app: AppHandle, id: String) -> Result<(), String> {
    let mut all = load_bookmarks(&app);
    let before = all.len();
    all.retain(|mark| mark.id != id);
    if all.len() == before {
        return Err("Bookmark not found.".into());
    }
    let text = serde_json::to_string_pretty(&all).map_err(|error| error.to_string())?;
    fs::write(bookmarks_path(&app)?, text).map_err(|error| error.to_string())
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct Handed {
    name: String,
    data: String,
}

#[tauri::command(async)]
#[specta::specta]
async fn export_mrpack_download(app: AppHandle, id: String) -> Result<Handed, String> {
    use base64::Engine;
    let instance = instance_by_id(&app, &id)?;
    let name = format!("{}.mrpack", ascii_folder_name(&instance.name));
    let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("uploads");
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join(format!("{}-{name}", unix_now()));
    let made = export_mrpack(app.clone(), id, path.to_string_lossy().to_string()).await;
    let bytes = made.and_then(|_| fs::read(&path).map_err(|error| format!("Could not read the created modpack: {error}")));
    let _ = fs::remove_file(&path);
    Ok(Handed { name, data: base64::engine::general_purpose::STANDARD.encode(bytes?) })
}

struct PackFile {
    relative: String,
    sha512: String,
    size: u64,
    local: Option<PathBuf>,
}

#[tauri::command(async)]
#[specta::specta]
async fn export_mrpack(app: AppHandle, id: String, target: String) -> Result<usize, String> {
    let instance = instance_by_id(&app, &id)?;
    let destination = PathBuf::from(target.trim());
    if destination.extension().is_none_or(|ext| ext != "mrpack") {
        return Err("The export file name must end with .mrpack.".into());
    }
    if !instance.remote_host.is_empty() {
        return export_mrpack_remote(&app, &instance, &destination).await;
    }
    only_local(&app, &id, "Modpack export")?;
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        off_runtime(|| {
            let (folder_name, _) = instance_loader(&instance)?;
            let mut installed = addons::scan(Path::new(&instance.path));
            installed.retain(|addon| addon.enabled);
            if let Ok(client) = addons::client() {
                let _ = addons::identify(&client, &mut installed);
            }
            let source_folder = Path::new(&instance.path).join(folder_name);
            let files: Vec<PackFile> = installed
                .iter()
                .filter_map(|addon| {
                    let path = source_folder.join(&addon.file_name);
                    let size = fs::metadata(&path).ok()?.len();
                    Some(PackFile {
                        relative: format!("{folder_name}/{}", addon.file_name),
                        sha512: addon.sha512.clone(),
                        size,
                        local: Some(path),
                    })
                })
                .collect();
            write_mrpack(&worker, &instance, &destination, &files)
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

async fn export_mrpack_remote(app: &AppHandle, instance: &ServerInstance, destination: &Path) -> Result<usize, String> {
    let (folder_name, _) = instance_loader(instance)?;
    let listed = remote::addon_files(app, &instance.remote_host, &instance.path, folder_name).await?;
    let temp = app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("mrpack-export").join(format!(
        "{}-{}",
        instance.id,
        unix_now()
    ));
    fs::create_dir_all(&temp).map_err(|error| error.to_string())?;
    let mut files = Vec::new();
    let mut failure = None;
    for one in listed.into_iter().filter(|one| one.name.ends_with(".jar")) {
        let relative = format!("{folder_name}/{}", one.name);
        let local = if addons::download_url_for(&one.hash).is_some() {
            None
        } else {
            let to = temp.join(&one.name);
            match remote::download_to(app, &instance.remote_host, &instance.path, &relative, &to, |_, _| {}).await {
                Ok(_) => Some(to),
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        };
        files.push(PackFile { relative, sha512: one.hash, size: one.size, local });
    }
    let result = match failure {
        Some(error) => Err(error),
        None => {
            let (worker, instance, destination) = (app.clone(), instance.clone(), destination.to_path_buf());
            tauri::async_runtime::spawn_blocking(move || write_mrpack(&worker, &instance, &destination, &files))
                .await
                .map_err(|error| error.to_string())?
        }
    };
    let _ = fs::remove_dir_all(&temp);
    result
}

fn write_mrpack(app: &AppHandle, instance: &ServerInstance, destination: &Path, files: &[PackFile]) -> Result<usize, String> {
    let mut listed = Vec::new();
    let mut embedded = Vec::new();
    for one in files {
        match addons::download_url_for(&one.sha512) {
            Some(url) => listed.push(serde_json::json!({
                "path": one.relative,
                "hashes": { "sha512": one.sha512 },
                "env": { "client": "optional", "server": "required" },
                "downloads": [url],
                "fileSize": one.size,
            })),
            None => {
                if let Some(path) = &one.local {
                    embedded.push((one.relative.clone(), path.clone()));
                }
            }
        }
    }
    let index = serde_json::json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": instance.version,
        "name": instance.name,
        "summary": "Exported from Monolab.",
        "files": listed,
        "dependencies": { "minecraft": instance.version },
    });
    let file = fs::File::create(destination).map_err(|error| format!("Could not create the export file: {error}"))?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    writer.start_file("modrinth.index.json", options).map_err(|error| error.to_string())?;
    writer.write_all(serde_json::to_string_pretty(&index).unwrap_or_default().as_bytes()).map_err(|error| error.to_string())?;
    for (relative, path) in &embedded {
        writer.start_file(format!("overrides/{relative}"), options).map_err(|error| error.to_string())?;
        let mut source = fs::File::open(path).map_err(|error| error.to_string())?;
        std::io::copy(&mut source, &mut writer).map_err(|error| error.to_string())?;
    }
    writer.finish().map_err(|error| error.to_string())?;
    record_audit(app, &instance.id, "Mod", "Modpack export", &format!("{} items", files.len()));
    Ok(files.len())
}

#[tauri::command(async)]
#[specta::specta]
fn save_map_png(target: String, png: String) -> Result<(), String> {
    use base64::Engine;
    let destination = PathBuf::from(target.trim());
    if destination.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case("png")) {
        return Err("The file name must end with .png.".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(png.trim()).map_err(|_| "The picture data is damaged.".to_string())?;
    if !bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Err("The picture data is damaged.".into());
    }
    fs::write(&destination, bytes).map_err(|error| format!("Could not save the picture: {error}"))
}

/// App settings that travel in a settings file. Web panel accounts, invites, roles, Monolink teams and the
/// web panel's switches stay on this PC: they hold secrets or open the PC to others.
const PORTABLE_SETTINGS: &[&str] = &[
    "autoBackupOnSave",
    "openConsoleOnStart",
    "defaultMemoryMib",
    "keepBackupDays",
    "keepAutoBackups",
    "consoleLineLimit",
    "backupRoot",
    "backupWorldsOnly",
    "backupIncludeGit",
    "closeToTray",
    "discordPresence",
    "updateChannel",
];

fn portable_settings(settings: &AppSettings) -> serde_json::Map<String, serde_json::Value> {
    let all = serde_json::to_value(settings).unwrap_or_default();
    PORTABLE_SETTINGS.iter().filter_map(|key| all.get(*key).map(|value| (key.to_string(), value.clone()))).collect()
}

/// `current` with the portable settings of `incoming` laid over it; anything else in `incoming` is ignored.
fn merged_settings(current: &AppSettings, incoming: &serde_json::Value) -> Result<AppSettings, String> {
    let mut all = serde_json::to_value(current).map_err(|error| error.to_string())?;
    if let (Some(target), Some(source)) = (all.as_object_mut(), incoming.as_object()) {
        for key in PORTABLE_SETTINGS {
            if let Some(value) = source.get(*key) {
                target.insert(key.to_string(), value.clone());
            }
        }
    }
    serde_json::from_value(all).map_err(|error| format!("The app settings in the file are malformed: {error}"))
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct SettingChange {
    key: String,
    /// JSON text of the value now and in the file.
    from: String,
    to: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct SettingsImportPreview {
    changes: Vec<SettingChange>,
    /// Names of servers that would be added.
    new_servers: Vec<String>,
    /// Servers already here or without a folder on this PC.
    skipped_servers: usize,
    new_bookmarks: usize,
    /// The display preferences in the file (JSON text, "" when it has none).
    display: String,
}

fn read_settings_bundle(source: &str) -> Result<serde_json::Value, String> {
    let text = fs::read_to_string(source.trim()).map_err(|error| format!("Could not read the settings file: {error}"))?;
    let bundle: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("The settings file is malformed: {error}"))?;
    if !matches!(bundle.get("kind").and_then(|value| value.as_str()), Some("monolab-settings")) {
        return Err("Not a settings file exported from Monolab.".into());
    }
    Ok(bundle)
}

fn setting_changes(current: &AppSettings, incoming: &serde_json::Value) -> Result<Vec<SettingChange>, String> {
    let before = portable_settings(current);
    let after = portable_settings(&merged_settings(current, incoming)?);
    Ok(PORTABLE_SETTINGS
        .iter()
        .filter_map(|key| {
            let from = before.get(*key)?;
            let to = after.get(*key)?;
            (from != to).then(|| SettingChange { key: key.to_string(), from: from.to_string(), to: to.to_string() })
        })
        .collect())
}

/// What importing `source` would change, without changing anything.
#[tauri::command(async)]
#[specta::specta]
fn preview_settings_import(app: AppHandle, source: String) -> Result<SettingsImportPreview, String> {
    let bundle = read_settings_bundle(&source)?;
    let changes = match bundle.get("app") {
        Some(incoming) => setting_changes(&read_app_settings(&app), incoming)?,
        None => Vec::new(),
    };
    let incoming: Vec<ServerInstance> = serde_json::from_value(bundle.get("instances").cloned().unwrap_or_default())
        .map_err(|error| format!("Could not read the server list: {error}"))?;
    let current = load_instances(&app)?;
    let mut new_servers = Vec::new();
    let mut skipped_servers = 0;
    for instance in incoming {
        let known = current.iter().any(|saved| saved.id == instance.id || saved.path == instance.path);
        if known || !Path::new(&instance.path).is_dir() {
            skipped_servers += 1;
        } else {
            new_servers.push(instance.name);
        }
    }
    let marks = load_bookmarks(&app);
    let incoming_marks: Vec<Bookmark> = serde_json::from_value(bundle.get("bookmarks").cloned().unwrap_or_default()).unwrap_or_default();
    let new_bookmarks = incoming_marks.iter().filter(|mark| !marks.iter().any(|saved| saved.id == mark.id)).count();
    let display = bundle.get("display").filter(|value| value.is_object()).map(|value| value.to_string()).unwrap_or_default();
    Ok(SettingsImportPreview { changes, new_servers, skipped_servers, new_bookmarks, display })
}

/// `display` is JSON text of this window's own preferences (theme, compact lists), kept in the file as they are.
#[tauri::command(async)]
#[specta::specta]
fn export_settings(app: AppHandle, target: String, display: Option<String>) -> Result<usize, String> {
    let destination = PathBuf::from(target.trim());
    if destination.extension().is_none_or(|ext| ext != "json") {
        return Err("The export file name must end with .json.".into());
    }
    let instances = load_instances(&app)?;
    let display = display
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(|value| value.is_object())
        .unwrap_or_default();
    let bundle = serde_json::json!({
        "kind": "monolab-settings",
        "version": 2,
        "exportedAt": unix_now(),
        "instances": instances,
        "bookmarks": load_bookmarks(&app),
        // Hosts and records only; passwords never leave this computer.
        "databases": databases::export(&app).unwrap_or_default(),
        // No accounts, invites, teams or web panel switches: see PORTABLE_SETTINGS.
        "app": portable_settings(&read_app_settings(&app)),
        "display": display,
    });
    fs::write(&destination, serde_json::to_string_pretty(&bundle).map_err(|error| error.to_string())?)
        .map_err(|error| format!("Could not export settings: {error}"))?;
    Ok(instances.len())
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct ImportSummary {
    instances: usize,
    bookmarks: usize,
    skipped: usize,
}

#[tauri::command(async)]
#[specta::specta]
fn import_settings(app: AppHandle, source: String) -> Result<ImportSummary, String> {
    let bundle = read_settings_bundle(&source)?;
    if let Some(incoming) = bundle.get("app") {
        let merged = merged_settings(&read_app_settings(&app), incoming)?;
        set_app_settings(app.clone(), merged)?;
    }
    let incoming: Vec<ServerInstance> = serde_json::from_value(bundle.get("instances").cloned().unwrap_or_default())
        .map_err(|error| format!("Could not read the server list: {error}"))?;

    let _store = lock_instance_store();
    let mut current = load_instances(&app)?;
    let mut added = 0;
    let mut skipped = 0;
    for instance in incoming {
        if !Path::new(&instance.path).is_dir() {
            skipped += 1;
            continue;
        }
        if current.iter().any(|saved| saved.id == instance.id || saved.path == instance.path) {
            skipped += 1;
            continue;
        }
        current.push(instance);
        added += 1;
    }
    save_instances(&app, &current)?;
    drop(_store);

    if let Some(found) = bundle.get("databases").filter(|value| value.is_object()) {
        let _ = databases::import(&app, found.clone());
    }

    let mut marks = load_bookmarks(&app);
    let incoming_marks: Vec<Bookmark> = serde_json::from_value(bundle.get("bookmarks").cloned().unwrap_or_default()).unwrap_or_default();
    let mut mark_count = 0;
    for mark in incoming_marks {
        if !marks.iter().any(|saved| saved.id == mark.id) {
            marks.push(mark);
            mark_count += 1;
        }
    }
    if let Ok(path) = bookmarks_path(&app) {
        let _ = serde_json::to_string_pretty(&marks)
            .map_err(|error| error.to_string())
            .and_then(|text| fs::write(path, text).map_err(|error| error.to_string()));
    }

    Ok(ImportSummary { instances: added, bookmarks: mark_count, skipped })
}

#[tauri::command(async)]
#[specta::specta]
fn stop_servers_for_update(app: AppHandle, state: State<'_, AppState>) -> Result<usize, String> {
    // The installer ends the app without RunEvent::Exit, so the playit helper would outlive it.
    playit::stop();
    let running = {
        let processes = state.processes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        processes.len()
    };
    if running == 0 {
        return Ok(0);
    }
    let handle = app.clone();
    off_runtime(move || shutdown_managed_servers(&handle));
    record_audit(&app, "", "App", "Servers stopped before update", "All running servers were stopped before installing the update.");
    Ok(running)
}

#[derive(Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
struct AppRelease {
    version: String,
    published: String,
}

const APP_RELEASES: &str = "https://api.github.com/repos/chicodev11/Monolab/releases?per_page=60";

fn fetch_app_releases() -> Result<Vec<AppRelease>, String> {
    let client = net::builder().timeout(Duration::from_secs(20)).build().map_err(|error| error.to_string())?;
    let releases: serde_json::Value = client
        .get(APP_RELEASES)
        .send()
        .map_err(|error| format!("Could not fetch the version list: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Could not fetch the version list: {error}"))?
        .json()
        .map_err(|error| error.to_string())?;
    let list = releases
        .as_array()
        .ok_or("The version list is malformed.")?
        .iter()
        .filter(|one| {
            !one.get("draft").and_then(|v| v.as_bool()).unwrap_or(false)
                && !one.get("prerelease").and_then(|v| v.as_bool()).unwrap_or(false)
                && one
                    .get("assets")
                    .and_then(|v| v.as_array())
                    .is_some_and(|assets| assets.iter().any(|asset| asset.get("name").and_then(|v| v.as_str()) == Some("latest.json")))
        })
        .filter_map(|one| {
            let version = one.get("tag_name")?.as_str()?.strip_prefix('v')?.to_string();
            if !valid_app_version(&version) {
                return None;
            }
            let published =
                one.get("published_at").and_then(|v| v.as_str()).map(|date| date.chars().take(10).collect()).unwrap_or_default();
            Some(AppRelease { version, published })
        })
        .collect();
    Ok(list)
}

fn valid_app_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

#[tauri::command(async)]
#[specta::specta]
async fn app_releases() -> Result<Vec<AppRelease>, String> {
    tauri::async_runtime::spawn_blocking(|| off_runtime(fetch_app_releases)).await.map_err(|error| error.to_string())?
}

/// The stable channel's manifest (also in tauri.conf.json) and the beta one next to it: a release
/// tagged `beta` whose latest.json points at the newest pre-release.
const STABLE_UPDATES: &str = "https://github.com/chicodev11/Monolab/releases/latest/download/latest.json";
const BETA_UPDATES: &str = "https://github.com/chicodev11/Monolab/releases/download/beta/latest.json";

/// The newest update for the beta channel: the beta manifest, or the stable one when that is newer.
async fn beta_update(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let check = |url: &str| {
        let url = url.parse::<tauri::Url>().map_err(|error| error.to_string());
        let app = app.clone();
        async move {
            app.updater_builder()
                .endpoints(vec![url?])
                .map_err(|error| error.to_string())?
                .build()
                .map_err(|error| error.to_string())?
                .check()
                .await
                .map_err(|error| error.to_string())
        }
    };
    let beta = check(BETA_UPDATES).await;
    let stable = check(STABLE_UPDATES).await;
    Ok(match (beta, stable) {
        (Ok(Some(beta)), Ok(Some(stable))) => Some(if newer_version(&stable.version, &beta.version) { stable } else { beta }),
        (Ok(Some(one)), _) | (_, Ok(Some(one))) => Some(one),
        (Err(error), Err(_)) => return Err(format!("Could not check for updates: {error}")),
        _ => None,
    })
}

/// Whether `a` is a newer release than `b` ("2.0.0" beats "2.0.0-beta.3").
fn newer_version(a: &str, b: &str) -> bool {
    match (semver::Version::parse(a.trim_start_matches('v')), semver::Version::parse(b.trim_start_matches('v'))) {
        (Ok(a), Ok(b)) => a > b,
        _ => false,
    }
}

#[cfg(test)]
mod channel_tests {
    #[test]
    fn releases_beat_their_betas() {
        assert!(super::newer_version("2.0.0", "2.0.0-beta.3"));
        assert!(super::newer_version("1.10.0", "1.9.9"));
        assert!(!super::newer_version("2.0.0-beta.1", "2.0.0"));
        assert!(!super::newer_version("junk", "1.0.0"));
    }
}

/// The version the beta channel would install, or `None` when this is the newest.
#[tauri::command(async)]
#[specta::specta]
async fn check_beta_update(app: AppHandle) -> Result<Option<String>, String> {
    Ok(beta_update(&app).await?.map(|update| update.version))
}

/// Downloads and installs the beta channel's update, then restarts.
#[tauri::command(async)]
#[specta::specta]
async fn install_beta_update(app: AppHandle) -> Result<(), String> {
    let update = beta_update(&app).await?.ok_or("You're on the latest version.")?;
    let version = update.version.clone();
    install_update(app, update, &version).await
}

async fn install_update(app: AppHandle, update: tauri_plugin_updater::Update, version: &str) -> Result<(), String> {
    let mut received = 0u64;
    let progress = app.clone();
    let bytes = update
        .download(
            move |chunk, total| {
                received += chunk as u64;
                let _ = progress.emit("app-version-download", serde_json::json!({ "received": received, "total": total }));
            },
            || {},
        )
        .await
        .map_err(|error| format!("Could not download version {version}: {error}"))?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || stop_servers_for_update(handle.clone(), handle.state::<AppState>()))
        .await
        .map_err(|error| error.to_string())??;
    update.install(bytes).map_err(|error| format!("Could not install version {version}: {error}"))?;
    app.restart();
}

/// Installs a chosen release (older ones too) through the signed updater bundle of that release.
#[tauri::command(async)]
#[specta::specta]
async fn install_app_version(app: AppHandle, version: String) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    if !valid_app_version(&version) {
        return Err("Unknown version.".into());
    }
    let endpoint: tauri::Url = format!("https://github.com/chicodev11/Monolab/releases/download/v{version}/latest.json")
        .parse::<tauri::Url>()
        .map_err(|error| error.to_string())?;
    let wanted = version.clone();
    let update = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|error| error.to_string())?
        .version_comparator(move |_, release| release.version.to_string() == wanted)
        .build()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| format!("Could not fetch version {version}: {error}"))?
        .ok_or_else(|| format!("Version {version} is already installed."))?;
    install_update(app, update, &version).await
}

/// cloudflared and playitd started by an earlier run that ended without cleaning up (crash, force quit, update).
/// Only our own copies in the app data folder whose Monolab is gone; another running Monolab keeps its own.
fn sweep_stray_helpers(app: &AppHandle) {
    let Ok(dir) = app.path().monolab_data_dir() else { return };
    let dir = dir.to_string_lossy().to_lowercase();
    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::nothing().with_exe(sysinfo::UpdateKind::Always),
    );
    for process in system.processes().values() {
        let Some(exe) = process.exe() else { continue };
        let path = exe.to_string_lossy().to_lowercase();
        let name = exe.file_name().map(|one| one.to_string_lossy().to_lowercase()).unwrap_or_default();
        let owned = process
            .parent()
            .and_then(|pid| system.process(pid))
            .is_some_and(|parent| parent.name().to_string_lossy().to_lowercase().starts_with("monolab"));
        if !owned && path.starts_with(&dir) && (name == "cloudflared.exe" || name.starts_with("playitd")) {
            process.kill();
        }
    }
}

fn shutdown_managed_servers(app: &AppHandle) {
    let state = app.state::<AppState>();
    let pending = {
        let mut processes = state.processes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for process in processes.values_mut() {
            if let Some(stdin) = process.child.stdin.as_mut() {
                let _ = stdin.write_all(b"stop\n");
                let _ = stdin.flush();
            }
            if process.stop_requested_at.is_none() {
                process.stop_requested_at = Some(SystemTime::now());
            }
        }
        processes.len()
    };
    if pending == 0 {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(25);
    loop {
        {
            let mut processes = state.processes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            processes.retain(|_, process| !matches!(process.child.try_wait(), Ok(Some(_))));
            if processes.is_empty() {
                return;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    let mut processes = state.processes.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    for process in processes.values_mut() {
        kill_process_tree(process.pid);
        let _ = process.child.kill();
    }
    processes.clear();
}

fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .error_handling(tauri_specta::ErrorHandlingMode::Throw)
        .dangerously_cast_bigints_to_number()
        .commands(tauri_specta::collect_commands![
            web_bind_choices,
            set_web_https,
            set_web_bind,
            tray::quit_app,
            tray::autostart_enabled,
            tray::set_autostart,
            list_instances,
            reorder_instances,
            import_instance,
            import_instance_archive,
            suggest_server_name,
            rename_instance,
            duplicate_instance,
            trash_instance,
            list_trash_instances,
            get_servers_root,
            servers_root_free_bytes,
            clients::list_clients,
            clients::create_client,
            clients::rename_client,
            clients::delete_client,
            clients::open_client_folder,
            client_content::client_content,
            client_content::client_content_cards,
            client_content::set_client_content_enabled,
            client_content::remove_client_content,
            client_content::add_client_content,
            client_content::update_client_content,
            client_content::open_client_content_folder,
            client_launch::get_client_account,
            forks::bundled_forks,
            client_launch::set_offline_name,
            client_launch::launch_client,
            client_launch::client_runs,
            client_launch::stop_client,
            client_launch::dismiss_client_run,
            nbt_edit::nbt_open,
            nbt_edit::nbt_open_chunk,
            nbt_edit::nbt_act,
            nbt_edit::nbt_copy,
            nbt_edit::nbt_save,
            nbt_edit::nbt_close,
            list_java_runtimes,
            enrich_addons,
            list_java_processes,
            kill_java_process,
            get_server_icon,
            set_server_icon,
            clear_server_icon,
            list_core_versions,
            list_core_builds,
            check_core_build,
            web_panel_state,
            set_web_panel,
            set_web_direct,
            monolink_account::account_state,
            monolink_account::account_sign_in,
            monolink_account::account_cancel_sign_in,
            monolink_account::account_check_handle,
            monolink_account::account_finish,
            monolink_account::account_sign_out,
            monolink_account::account_api,
            monolink_account::take_pending_invite,
            role_catalog,
            save_custom_role,
            delete_custom_role,
            monolink_call,
            monolink_open,
            monolink_close,
            monolink_team::monolink_team_links,
            teamlog::team_log,
            teamlog::export_team_log,
            teamlog::clear_team_log,
            gitlink::git_state,
            gitlink::git_start,
            gitlink::git_commit,
            gitlink::git_log,
            gitlink::git_diff,
            gitlink::git_restore,
            gitlink::git_set_remote,
            gitlink::git_push,
            gitlink::git_pull,
            gitlink::dev_repos,
            gitlink::dev_repo_add,
            gitlink::dev_repo_set,
            gitlink::dev_repo_remove,
            gitlink::dev_repo_build,
            monolink_team::monolink_team_attach,
            monolink_team::monolink_team_detach,
            monolink_team::monolink_team_admin_only,
            monolink_team::monolink_team_person_role,
            monolink_team::team_presence,
            monolink_team::monolink_team_agents,
            team_agent::team_agent_asks,
            team_agent::team_agent_answer,
            team_agent::read_server_agents_md,
            team_agent::save_server_agents_md,
            monolink_team_route,
            list_web_accounts,
            save_web_account,
            delete_web_account,
            set_server_icon_upload,
            add_addon_upload,
            stash_upload,
            lang::item_names,
            lang::enchantment_names,
            server_commands,
            precheck_instance,
            list_remote_javas,
            create_backup,
            list_backups,
            restore_backup,
            backup_contents,
            cancel_task,
            player_info,
            proxy_network,
            link_proxy,
            backup_entry_text,
            restore_backup_paths,
            delete_backup,
            required_java,
            download_java,
            remove_java_runtime,
            item_icons,
            item_ids,
            game_files,
            studio_assets,
            download_studio_assets,
            list_tasks,
            save_task,
            delete_task,
            run_task_now,
            get_app_settings,
            check_beta_update,
            install_beta_update,
            pending_crash,
            answer_crash,
            save_ui_crash,
            curseforge_key_set,
            set_curseforge_key,
            search_ftb_packs,
            download_curseforge_pack,
            fetch_curseforge_mods,
            preview_settings_import,
            discord_presence_available,
            set_discord_presence,
            set_app_settings,
            set_servers_root,
            reset_servers_root,
            restore_trash_instance,
            purge_trash_instance,
            empty_trash,
            create_instance,
            create_instance_from_jar,
            cancel_create_download,
            update_launch_settings,
            set_custom_command,
            set_start_script,
            list_start_scripts,
            open_server_folder,
            app_version,
            open_notices,
            start_instance,
            restart_instance,
            stop_instance,
            force_stop_instance,
            stop_servers_for_update,
            app_releases,
            set_server_group,
            rename_server_group,
            plugin_integrations,
            essentials_data,
            essentials_set_setting,
            luckperms_data,
            addon_config,
            set_addon_config,
            extension_data,
            extension_set,
            run_extension_action,
            process_details,
            view_server_file,
            browse_trash_files,
            view_trash_file,
            server_databases,
            db_hosts,
            save_db_host,
            check_db_host,
            delete_db_host,
            create_server_database,
            link_server_database,
            server_database_password,
            rotate_server_database_password,
            delete_server_database,
            orphan_databases,
            delete_orphan_database,
            db_plugin_targets,
            fill_plugin_database,
            database_tables,
            database_rows,
            server_sqlite_files,
            sqlite_tables,
            sqlite_rows,
            market_installed,
            install_market_plugin,
            market_project_detail,
            market_updates,
            install_app_version,
            set_start_with_app,
            set_crash_restart,
            send_console_command,
            get_console_logs,
            list_log_entries,
            list_audit_entries,
            read_server_properties,
            save_server_properties,
            browse_server_files,
            rename_server_path,
            trash_server_path,
            list_server_trash,
            restore_server_trash,
            delete_server_trash,
            empty_server_trash,
            create_server_folder,
            create_server_file,
            add_server_files,
            add_server_folders,
            folders_in_the_way,
            add_server_uploads,
            zip_server_paths,
            unzip_server_path,
            read_server_file_download,
            copy_server_file_out,
            read_editable_file,
            read_server_image,
            read_texture_animation,
            save_editable_file,
            list_worlds,
            get_resource_snapshot,
            system_memory_mib,
            get_tps_snapshot,
            addon_status,
            install_addon,
            pumpkin_addon_allowed,
            set_pumpkin_addon_allowed,
            pumpkin_refused_permissions,
            allow_pumpkin_plugin,
            add_addon_files,
            remove_addon,
            render_map_tile,
            world_map_bounds,
            map_changes,
            list_event_days,
            perf_history,
            read_live,
            read_loaded_chunks,
            map_surface_height,
            disk_usage,
            clean_disk,
            chunky_status,
            chunky_run,
            map_web_state,
            map_web_set,
            public_address,
            map_web_probe,
            chunk_stats,
            list_runs,
            server_memo,
            set_server_memo,
            rewind_scan,
            rewind_list,
            rewind_versions,
            rewind_restore,
            rewind_enabled,
            set_rewind_enabled,
            create_web_invite,
            list_web_invites,
            revoke_web_invite,
            server_people,
            remove_server_access,
            web_online,
            add_memo_image,
            memo_image,
            delete_memo_image,
            trim_chunks,
            diagnose_lag,
            get_server_ping,
            list_players,
            list_player_activity,
            toggle_player_access,
            install_modrinth,
            install_geyser,
            webhook_info,
            set_webhook,
            test_webhook,
            studio_asset_list,
            studio_asset_add,
            studio_asset_remove,
            studio_asset_image,
            studio_pack_build,
            studio_pack_sending,
            studio_expansions::studio_expansions,
            studio_bundle::studio_bundle_export,
            studio_bundle::studio_bundle_peek,
            studio_bundle::studio_bundle_import,
            studio_bundle::studio_bundle_peek_upload,
            studio_bundle::studio_bundle_import_upload,
            studio_asset_add_upload,
            cloud_info,
            set_cloud,
            test_cloud,
            list_addon_versions,
            modrinth_project_detail,
            preview_mrpack,
            search_modpacks,
            download_modpack,
            datapack_folder,
            search_datapacks,
            install_datapack,
            preview_cfpack,
            import_cfpack,
            import_mrpack,
            scan_addons,
            toggle_addon,
            delete_addon,
            upgrade_instance,
            list_upgrade_history,
            rollback_last_upgrade,
            list_automation_history,
            get_resource_history,
            send_rcon_command,
            get_rcon_status,
            read_player_nbt,
            edit_player_item,
            list_player_spots,
            read_world_settings,
            save_world_settings,
            lookup_mojang_profile,
            get_player_heads,
            get_heads_by_name,
            list_crash_reports,
            search_addon_catalog,
            inspect_addons,
            check_addon_updates,
            apply_addon_updates,
            list_pending_addons,
            cancel_pending_addon,
            list_addon_rollbacks,
            rollback_addon_update,
            plan_addon_install,
            install_addon_with_dependencies,
            list_log_files,
            share_log,
            preview_share_log,
            read_log_timeline,
            summarize_logs,
            list_bookmarks,
            save_bookmark,
            delete_bookmark,
            export_mrpack,
            export_mrpack_download,
            export_settings,
            save_map_png,
            import_settings,
            list_panel_hosts,
            save_panel_host,
            delete_panel_host,
            test_panel_host,
            panel_create_options,
            sync_panel_servers,
            panel_unhide,
            panel_call,
            sftp_list,
            claude_info,
            claude_chat_state,
            claude_chat_open,
            claude_chat_send,
            claude_chat_answer,
            claude_chat_interrupt,
            claude_chat_mode,
            claude_chat_model,
            claude_chat_history,
            claude_chat_sessions,
            agent_skills,
            save_agent_skill,
            set_agent_skill_enabled,
            delete_agent_skill,
            copy_agent_skill,
            claude_chat_rename,
            claude_chat_delete,
            claude_chat_close,
            acp_agents,
            acp_install,
            acp_uninstall,
            acp_open,
            acp_authenticate,
            acp_prompt,
            acp_cancel,
            acp_answer,
            acp_set_mode,
            acp_set_config,
            acp_state,
            acp_close,
            acp_chats,
            acp_chat_read,
            acp_chat_save,
            acp_chat_delete,
            studio_overview,
            studio_read,
            studio_save,
            studio_template,
            studio_held_item,
            studio_sync_targets,
            studio_sync,
            studio_structure_import,
            studio_set_held_item,
            studio_test_event,
            studio_worldgen_preview,
            studio_watch,
            studio_unwatch,
            msmp_state,
            msmp_setup,
            msmp_settings,
            msmp_set_setting,
            msmp_rules,
            msmp_set_rule,
            claude_open_folder,
            claude_accounts,
            claude_attach_paths,
            claude_attach_bytes,
            claude_account_add,
            claude_account_rename,
            claude_account_remove,
            claude_account_logout,
            claude_account_use,
            claude_account_login,
            codex_login,
            codex_models,
            claude_account_login_code,
            claude_account_login_cancel,
            sftp_make_dir,
            sftp_rename,
            sftp_delete,
            sftp_read_text,
            sftp_write_text,
            sftp_transfer,
            sftp_cancel,
            shell_open,
            agent_read_memo,
            proposals_list,
            proposal_merge,
            proposal_reject,
            proposal_texts,
            monolink_team::monolink_team_proposals,
            storage_view,
            storage_put_file,
            storage_trash,
            storage_empty_trash,
            storage_set_limit,
            storage_to_server,
            storage_put_from_server,
            storage_save_copy,
            memo_people,
            agent_update_todo,
            file_editing,
            shell_input,
            shell_resize,
            shell_close,
            panel_startup,
            set_panel_variable,
            panel_free_allocations,
            panel_create_server,
            list_ssh_hosts,
            browse_remote_dir,
            scan_remote_servers,
            import_remote_instance,
            import_remote_archive,
            upload_instance_to_host,
            move_instance_to_host,
            panel_usage,
            probe_ssh_host,
            save_ssh_host,
            ensure_ssh_root,
            delete_ssh_host,
            test_ssh_host,
            retry_ssh_host,
            playit_state,
            lan_ip,
            playit_claim,
            playit_expose,
            playit_unexpose,
            playit_forget,
            vps_tunnel_state,
            vps_tunnel_start,
            vps_tunnel_stop,
            create_from_modpack
        ])
}

fn export_bindings(path: &std::path::Path) -> Result<(), String> {
    let path = path.to_path_buf();
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || export_bindings_here(&path))
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "Exporting bindings stopped.".to_string())?
}

fn export_bindings_here(path: &std::path::Path) -> Result<(), String> {
    specta_builder()
        .export(
            specta_typescript::Typescript::default().header("// Generated file — do not edit. `npm run bindings:check` keeps it in sync."),
            path,
        )
        .map_err(|error| error.to_string())?;
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let from = "import { invoke as __TAURI_INVOKE } from \"@tauri-apps/api/core\";";
    if !text.contains(from) {
        return Err("The bindings have no invoke import line — did tauri-specta change?".into());
    }
    let text = text.replace(from, "import { call as __TAURI_INVOKE } from \"./bridge\";");
    fs::write(path, text).map_err(|error| error.to_string())
}

/// A crash report from the last run that the user hasn't answered yet.
#[tauri::command]
#[specta::specta]
fn pending_crash() -> Option<app_crash::PendingCrash> {
    app_crash::pending()
}

/// The user's answer to the crash dialog: send the report to the account server, or forget it.
#[tauri::command]
#[specta::specta]
async fn answer_crash(send: bool) -> Result<(), String> {
    if !send {
        app_crash::forget();
        return Ok(());
    }
    app_crash::send(&monolink_account::server()).await
}

/// A screen that couldn't draw; kept like a panic and asked about on the next start.
#[tauri::command]
#[specta::specta]
fn save_ui_crash(message: String, detail: String) {
    app_crash::keep(&app_crash::make("ui", &message, &detail, unix_now()));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
fn record_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let where_at = info.location().map(|at| format!("{}:{}", at.file(), at.line())).unwrap_or_else(|| "?".into());
        let trace = std::backtrace::Backtrace::force_capture().to_string();
        let line = format!("[{}] {where_at} — {info}\n{trace}\n\n", unix_now());
        // Asked about on the next start; sent only if the user agrees.
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|text| text.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".into());
        app_crash::keep(&app_crash::make("panic", &message, &format!("{where_at}\n{trace}"), unix_now()));
        if let Some(dir) = data_root::current() {
            let path = dir.join("logs");
            let _ = fs::create_dir_all(&path);
            let path = path.join("crash-log.txt");
            if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
                use std::io::Write;
                let _ = file.write_all(line.as_bytes());
            }
        }
        eprintln!("{line}");
        previous(info);
    }));
}

pub fn run() {
    record_panics();
    let specta = specta_builder();
    let mut args = std::env::args().skip(1);
    let first = args.next();
    // An agent's stdio MCP server: runs headless and never opens a window.
    if first.as_deref() == Some(acp::MCP_STDIO_FLAG) {
        std::process::exit(acp::mcp_stdio_bridge(&args.next().unwrap_or_default()));
    }
    if first.as_deref() == Some("--export-bindings") {
        let path = args.next().expect("--export-bindings needs a path");
        match export_bindings(std::path::Path::new(&path)) {
            Ok(()) => std::process::exit(0),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    #[cfg(debug_assertions)]
    export_bindings(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../src/lib/bindings.ts")))
        .expect("Could not export the TypeScript bindings");
    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("maptile", |context, request, responder| {
            map_tile_protocol(context.app_handle().clone(), request, responder)
        })
        // A second launch (double click, shortcut, autostart) brings this window forward instead of opening another app.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // A Monolink invite link opened while the app runs arrives as an argument of the second launch.
            for arg in args.iter().filter(|one| one.starts_with("monolab://")) {
                monolink_account::open_link(app, arg);
            }
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![tray::HIDDEN_ARG])))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    tray::on_close(window);
                }
            }
        })
        .manage(AppState {
            processes: Arc::new(Mutex::new(HashMap::new())),
            crashed: Arc::new(Mutex::new(HashMap::new())),
            logs: Arc::new(Mutex::new(HashMap::new())),
            tps: Arc::new(Mutex::new(HashMap::new())),
            cancelled_downloads: Arc::new(Mutex::new(HashSet::new())),
            remote_running: Arc::new(Mutex::new(HashMap::new())),
            remote_checked: Arc::new(Mutex::new(HashSet::new())),
            remote_unreachable: Arc::new(Mutex::new(HashSet::new())),
            remote_failures: Arc::new(Mutex::new(HashMap::new())),
            remote_tailing: Arc::new(Mutex::new(HashSet::new())),
            remote_starting: Arc::new(Mutex::new(HashMap::new())),
            remote_kick: Arc::new(tokio::sync::Notify::new()),
            panel_live: Arc::new(Mutex::new(HashMap::new())),
            panel_checked: Arc::new(Mutex::new(HashSet::new())),
            panel_unreachable: Arc::new(Mutex::new(HashSet::new())),
            panel_sockets: Arc::new(Mutex::new(HashSet::new())),
            panel_backlog: Arc::new(Mutex::new(HashSet::new())),
            panel_kick: Arc::new(tokio::sync::Notify::new()),
            system: Arc::new(Mutex::new(System::new())),
            // A region is about 3 MB in memory and a tile about 30 KB.
            map_cells: Arc::new(Mutex::new(Lru::new(64))),
            map_coarse: Arc::new(Mutex::new(Lru::new(256))),
            map_tiles: Arc::new(Mutex::new(Lru::new(1200))),
            map_palettes: Arc::new(Mutex::new(HashMap::new())),
            map_index: Arc::new(Mutex::new(HashMap::new())),
            resource_history: Arc::new(Mutex::new(HashMap::new())),
            spark_reports: Arc::new(Mutex::new(HashMap::new())),
            chunky: Arc::new(Mutex::new(HashMap::new())),
            chunk_stats: Arc::new(Mutex::new(HashMap::new())),
            chunk_entities: Arc::new(Mutex::new(HashMap::new())),
            chunk_folders: Arc::new(Mutex::new(HashMap::new())),
        })
        .manage(web::Running::default())
        .manage(WebEvents::default())
        .setup(|app| {
            data_root::init(app.handle());
            tray::build(app.handle())?;
            // Started by "Start with Windows": stay in the tray until the user opens the window.
            fit_main_to_screen(app.handle());
            if !std::env::args().any(|one| one == tray::HIDDEN_ARG) {
                tray::show_main(app.handle());
            }
            forks::init(app.handle());
            updates::init(app.handle());
            if let Ok(root) = app.path().monolab_data_dir() {
                store::set_root(root.clone());
                let ids: Vec<String> = load_instances(app.handle()).unwrap_or_default().into_iter().map(|one| one.id).collect();
                tidy::tidy(&root, &ids);
            }
            let handle = app.handle().clone();
            app.manage(logbatch::Batcher::start(move |batch: Vec<ServerLogEvent>| {
                remember_for_web(&handle, logbatch::EVENT, &batch);
                let _ = handle.emit(logbatch::EVENT, batch);
            }));
            start_backup_scheduler(app.handle().clone());
            start_lag_watcher(app.handle().clone());
            start_rewind_watcher(app.handle().clone());
            start_remote_watcher(app.handle().clone());
            panel_live::start_watcher(app.handle().clone());
            start_panel_sync(app.handle().clone());
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let set = read_app_settings(&handle);
                for instance in load_instances(&handle).unwrap_or_default() {
                    if let Ok(vault) = backup_folder(&handle, &instance) {
                        backups::sweep(&vault, set.keep_backup_days, set.keep_auto_backups);
                    }
                }
            });
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                thread::sleep(Duration::from_secs(2));
                start_marked_servers(&handle);
            });
            sweep_stray_helpers(app.handle());
            // Earlier versions downloaded cloudflared here for the web panel; it's no longer used.
            if let Ok(dir) = app.path().monolab_data_dir() {
                let _ = std::fs::remove_dir_all(dir.join("tunnel"));
            }
            if playit::read_secret(app.handle()).is_some() {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    let _ = playit::start(&handle);
                });
            }
            let settings = read_app_settings(app.handle());
            if settings.web_panel {
                let _ = web::start(app.handle(), settings.web_panel_port);
            }
            // monolab:// links: the installer registers the scheme; a dev build registers itself here.
            #[cfg(debug_assertions)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let _ = app.deep_link().register_all();
            }
            for arg in std::env::args().filter(|one| one.starts_with("monolab://")) {
                monolink_account::open_link(app.handle(), &arg);
            }
            monolink_account::start_live(app.handle());
            Ok(())
        })
        .invoke_handler({
            let handler = specta.invoke_handler();
            move |invoke| {
                let label = invoke.message.webview_ref().label().to_string();
                let args = match invoke.message.payload() {
                    tauri::ipc::InvokeBody::Json(value) => Some(value),
                    _ => None,
                };
                if let Some(refusal) = monolink_guard(&label, invoke.message.command(), args) {
                    invoke.resolver.reject(refusal);
                    return true;
                }
                handler(invoke)
            }
        })
        .build(app_context())
        .expect("error while running Monolab")
        .run(|handle, event| {
            if matches!(event, RunEvent::Exit) {
                monolink_account::stop(handle);
                updates::stop();
                playit::stop();
                claude_chat::close_all();
                codex_chat::close_all();
                acp::close_all();
                shutdown_managed_servers(handle);
            }
        });
}

/// Development builds only: `MONOLAB_PROFILE=b` runs a second, separate Monolab next to the usual one.
/// It gets its own identifier, so its own data folder, device key, web storage and single-instance lock,
/// which is what trying Monolink between two "devices" on one PC needs.
fn dev_profile() -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let name = std::env::var("MONOLAB_PROFILE").ok()?.trim().to_ascii_lowercase();
    (!name.is_empty() && name.len() <= 16 && name.chars().all(|letter| letter.is_ascii_alphanumeric())).then_some(name)
}

fn app_context() -> tauri::Context<tauri::Wry> {
    let mut context = tauri::generate_context!();
    if let Some(profile) = dev_profile() {
        let config = context.config_mut();
        config.identifier = format!("{}.{profile}", config.identifier);
        for window in &mut config.app.windows {
            window.title = format!("{} ({profile})", window.title);
        }
    }
    context
}

#[cfg(test)]
mod tests {
    #[test]
    fn saves_without_a_hash_like_before() {
        assert!(!super::saved_since(Some("a: 1"), None));
        assert!(!super::saved_since(None, None));
    }

    #[test]
    fn refuses_when_the_file_changed() {
        let opened = crate::studio::hash("a: 1");
        assert!(!super::saved_since(Some("a: 1"), Some(&opened)));
        assert!(super::saved_since(Some("a: 2"), Some(&opened)));
        assert!(super::saved_since(None, Some(&opened)));
    }

    #[test]
    fn hash_is_of_the_text_the_editor_got() {
        // A CRLF file hashes the same as the text the editor was given (the read function's string), not as LF.
        let text = "a: 1\r\nb: 2\r\n";
        assert!(!super::saved_since(Some(text), Some(&crate::studio::hash(text))));
        assert!(super::saved_since(Some(text), Some(&crate::studio::hash("a: 1\nb: 2\n"))));
    }

    #[test]
    fn monolink_windows_only_reach_their_own_host() {
        let host = relay_proto::DeviceId([7; 16]);
        let other = relay_proto::DeviceId([9; 16]);
        let teammate = super::monolink_label(&host, "team-1");
        let call = |peer: &relay_proto::DeviceId, team: &str, command: &str| serde_json::json!({ "peer": peer.text(), "team": team, "command": command, "body": "{}" });
        let guard = |label: &str, command: &str, args: serde_json::Value| super::monolink_guard(label, command, Some(&args));
        // The main window: every app command, but only the team log from a host.
        assert!(guard("main", "set_custom_command", serde_json::json!({})).is_none());
        assert!(guard("main", "monolink_call", call(&host, "team-1", "__team_log")).is_none());
        assert!(guard("main", "monolink_call", call(&host, "team-1", "send_command")).is_some());
        // A Monolink window: its own host and team only, and no other app command.
        assert!(guard(&teammate, "monolink_call", call(&host, "team-1", "send_command")).is_none());
        assert!(guard(&teammate, "monolink_call", call(&other, "team-1", "list_instances")).is_some());
        assert!(guard(&teammate, "monolink_call", call(&host, "", "list_instances")).is_some());
        assert!(guard(&teammate, "monolink_call", call(&host, "team-1", "list_instances")).is_none());
        assert!(guard(&teammate, "monolink_call", call(&host, "team-2", "list_instances")).is_some());
        assert!(guard(&teammate, "monolink_team_route", serde_json::json!({ "team": "team-1", "host": host.text() })).is_none());
        assert!(guard(&teammate, "set_custom_command", serde_json::json!({})).is_some());
        assert!(guard(&teammate, "read_editable_file", serde_json::json!({})).is_some());
        assert!(super::monolink_guard(&teammate, "monolink_call", None).is_some());
        // A team view closes only itself.
        assert!(guard(&teammate, "monolink_close", call(&host, "team-1", "")).is_none());
        assert!(guard(&teammate, "monolink_close", call(&other, "team-1", "")).is_some());
    }

    #[test]
    fn switches_only_between_cores_that_keep_worlds_and_plugins() {
        assert_eq!(super::core_after("Paper", None).unwrap(), "Paper");
        assert_eq!(super::core_after("Paper", Some("Purpur")).unwrap(), "Purpur");
        assert_eq!(super::core_after("Vanilla", Some("Leaf")).unwrap(), "Leaf");
        assert!(super::core_after("Paper", Some("Vanilla")).is_err());
        assert!(super::core_after("Fabric", Some("Paper")).is_err());
        assert_eq!(super::core_after("Fabric", Some("Fabric")).unwrap(), "Fabric");
    }

    #[test]
    fn map_cache_drops_the_least_recently_used() {
        let mut cache = super::Lru::new(4);
        for at in 0..4 {
            cache.insert(at, at);
        }
        assert_eq!(cache.get(&0), Some(&0));
        cache.insert(4, 4);
        assert_eq!(cache.map.len(), 3);
        assert!(cache.get(&0).is_some() && cache.get(&4).is_some() && cache.get(&1).is_none());
    }

    #[test]
    fn zips_a_folder_and_a_file_without_itself() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("se-zip-{}", super::unix_now()));
        std::fs::create_dir_all(dir.join("plugins/sub")).unwrap();
        std::fs::write(dir.join("plugins/a.jar"), b"a").unwrap();
        std::fs::write(dir.join("plugins/sub/b.yml"), b"b").unwrap();
        std::fs::write(dir.join("server.properties"), b"p").unwrap();
        let target = dir.join("out.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&target).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        super::zip_into(&mut writer, &dir, &dir.join("plugins"), &target, options).unwrap();
        super::zip_into(&mut writer, &dir, &dir.join("server.properties"), &target, options).unwrap();
        super::zip_into(&mut writer, &dir, &target, &target, options).unwrap();
        writer.finish().unwrap().flush().unwrap();
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&target).unwrap()).unwrap();
        let names: Vec<String> = (0..archive.len()).map(|at| archive.by_index(at).unwrap().name().unwrap().to_string()).collect();
        assert!(names.contains(&"plugins/a.jar".to_string()), "{names:?}");
        assert!(names.contains(&"plugins/sub/b.yml".to_string()), "{names:?}");
        assert!(names.contains(&"server.properties".to_string()), "{names:?}");
        assert!(!names.iter().any(|one| one.contains("out.zip")), "it included itself: {names:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keeps_file_names_inside_their_folder() {
        assert_eq!(super::parent_of("plugins/Foo.jar"), "plugins");
        assert_eq!(super::parent_of(r"plugins\deep\Foo.jar"), "plugins/deep");
        assert_eq!(super::parent_of("/server.properties"), "");
        assert_eq!(super::parent_of("world"), "");
        assert_eq!(super::plain_name("  Foo.jar ").unwrap(), "Foo.jar");
        assert!(super::plain_name("").is_err());
        assert!(super::plain_name("../x").is_err());
        assert!(super::plain_name(r"a\b").is_err());
    }

    #[test]
    fn moves_an_old_folder_even_when_the_new_one_is_there() {
        let dir = std::env::temp_dir().join(format!("se-tidy-{}", super::unix_now()));
        let old = dir.join("server/.monolab-backups");
        let new = dir.join("data/backups");
        std::fs::create_dir_all(old.join("addons")).unwrap();
        std::fs::write(old.join("1-manual.zip"), b"new").unwrap();
        std::fs::write(old.join("addons/9-mine.jar"), b"jar").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("1-manual.zip"), b"first").unwrap();

        super::move_tree(&old, &new).unwrap();

        assert!(!old.exists(), "the old folder is still there");
        assert!(new.join("addons/9-mine.jar").is_file(), "the subfolder did not come along");
        assert_eq!(std::fs::read(new.join("1-manual.zip")).unwrap(), b"first", "overwrote something already moved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_a_server_folder_with_a_progress_count() {
        let dir = std::env::temp_dir().join(format!("se-copy-test-{}", super::unix_now()));
        let source = dir.join("src");
        std::fs::create_dir_all(source.join("world/region")).unwrap();
        std::fs::create_dir_all(source.join(".monolab-backups")).unwrap();
        std::fs::write(source.join("server.jar"), b"jar").unwrap();
        std::fs::write(source.join("world/level.dat"), b"lvl").unwrap();
        std::fs::write(source.join("world/region/r.0.0.mca"), b"mca").unwrap();
        std::fs::write(source.join(".monolab-backups/old.zip"), b"zip").unwrap();

        assert_eq!(super::count_files(&source), 3);
        let destination = super::free_folder(&dir, "My server");
        assert!(destination.file_name().unwrap().to_str().unwrap().is_ascii());
        let mut ticks = 0;
        super::copy_instance_folder(&source, &destination, &mut || ticks += 1).unwrap();
        assert_eq!(ticks, 3);
        assert!(destination.join("world/region/r.0.0.mca").is_file());
        assert!(!destination.join(".monolab-backups").exists());
        assert!(source.join("server.jar").is_file());
        std::fs::create_dir_all(&destination).unwrap();
        let again = super::free_folder(&dir, "My server");
        assert_ne!(again, destination);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pours_a_folder_into_one_that_is_already_there() {
        let dir = std::env::temp_dir().join(format!("se-merge-test-{}", super::unix_now()));
        let from = dir.join("new/pack");
        let to = dir.join("server/pack");
        std::fs::create_dir_all(from.join("assets")).unwrap();
        std::fs::create_dir_all(to.join("assets")).unwrap();
        std::fs::write(from.join("config.yml"), b"new").unwrap();
        std::fs::write(from.join("newfile.txt"), b"fresh").unwrap();
        std::fs::write(from.join("assets/image.txt"), b"art").unwrap();
        std::fs::write(to.join("config.yml"), b"old").unwrap();
        std::fs::write(to.join("existing.txt"), b"keep").unwrap();
        std::fs::write(to.join("assets/old-image.txt"), b"keep").unwrap();

        let ticks = std::cell::Cell::new(0_u32);
        let sent = super::copy_tree_counting(&from, &to, &|| ticks.set(ticks.get() + 1)).unwrap();
        assert_eq!(sent, 3);
        assert_eq!(ticks.get(), 3);
        assert_eq!(std::fs::read(to.join("config.yml")).unwrap(), b"new", "same name is replaced with the new one");
        assert!(to.join("newfile.txt").is_file(), "a new file was added");
        assert!(to.join("assets/image.txt").is_file(), "including inner folders");
        assert!(to.join("existing.txt").is_file(), "existing files stay");
        assert!(to.join("assets/old-image.txt").is_file(), "including what was inside");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_an_animated_texture_mcmeta() {
        let plain = super::read_animation(r#"{ "animation": { "frametime": 3, "interpolate": true, "width": 16, "height": 16 } }"#)
            .expect("animation");
        assert_eq!(plain.frametime, 3);
        assert!(plain.interpolate);
        assert_eq!((plain.width, plain.height), (16, 16));
        assert!(plain.frames.is_empty(), "frames play in order when none are given");

        let picked = super::read_animation(r#"{ "animation": { "frames": [0, 1, { "index": 2, "time": 10 }, 3] } }"#).expect("animation");
        assert_eq!(picked.frametime, 1, "one tick when not given");
        assert_eq!(picked.frames.iter().map(|one| (one.index, one.time)).collect::<Vec<_>>(), vec![(0, 0), (1, 0), (2, 10), (3, 0)]);

        assert!(super::read_animation(r#"{ "pack": { "pack_format": 34 } }"#).is_none());
        assert!(super::read_animation("{ this is not json }").is_none());
    }

    #[test]
    fn reads_which_server_a_modpack_wants() {
        let fabric = serde_json::json!({
            "name": "Fabulously Optimized",
            "dependencies": { "minecraft": "1.21.1", "fabric-loader": "0.16.5" }
        });
        let target = super::modpack_target(&fabric).unwrap();
        assert_eq!(target.core, "Fabric");
        assert_eq!(target.version, "1.21.1");
        assert_eq!(target.build, "0.16.5");
        assert_eq!(target.name, "Fabulously Optimized");
        let neo = serde_json::json!({
            "dependencies": { "minecraft": "1.21.1", "neoforge": "21.1.72" }
        });
        let target = super::modpack_target(&neo).unwrap();
        assert_eq!((target.core.as_str(), target.build.as_str()), ("NeoForge", "21.1.72"));
        assert_eq!(target.name, "Modpack server");
        let quilt = serde_json::json!({
            "dependencies": { "minecraft": "1.20.1", "quilt-loader": "0.26.0" }
        });
        assert!(super::modpack_target(&quilt).unwrap_err().contains("Quilt"));
        let plain = serde_json::json!({ "dependencies": { "minecraft": "1.21.4" } });
        assert_eq!(super::modpack_target(&plain).unwrap().core, "Vanilla");
    }

    #[test]
    fn guesses_what_a_java_process_is() {
        let cases: &[(&str, &str, Option<&str>)] = &[
            (
                r#""C:\Program Files\Java\jdk-21\bin\java.exe" -Xmx4G -jar "D:\smp\paper-1.21.4-232.jar" nogui"#,
                "Paper 1.21.4 server",
                Some("smp · paper-1.21.4-232.jar"),
            ),
            (
                r"java -Xms2G -jar C:/servers/hardcore/purpur-1.21.1-2333.jar nogui",
                "Purpur 1.21.1 server",
                Some("hardcore · purpur-1.21.1-2333.jar"),
            ),
            (r"java -jar C:/mc/folia-26.1.2-15.jar nogui", "Folia 26.1.2 server", Some("mc · folia-26.1.2-15.jar")),
            (
                r"java -jar fabric-server-mc.1.21.4-loader.0.16.9-launcher.1.0.1.jar nogui",
                "Fabric 1.21.4 server",
                Some("fabric-server-mc.1.21.4-loader.0.16.9-launcher.1.0.1.jar"),
            ),
            (r"java -jar D:/proxy/velocity-3.4.0.jar", "Velocity 3.4.0 server", Some("proxy · velocity-3.4.0.jar")),
            (r"java -Xmx2G -jar C:/vanilla/server.jar nogui", "Minecraft server", Some("vanilla · server.jar")),
            (
                r"javaw.exe -cp C:/Users/me/AppData/Roaming/.minecraft/libraries/... net.minecraft.client.main.Main --username me --gameDir C:/Users/me/AppData/Roaming/.minecraft",
                "Minecraft game",
                Some("Not a server"),
            ),
            (r"java -jar C:/tools/jetbrains/gradle-wrapper.jar --daemon", "Other Java program", Some("jetbrains · gradle-wrapper.jar")),
        ];
        for (command, expected, why) in cases {
            let (label, detail) = super::guess_java(command);
            assert_eq!(&label, expected, "command line: {command}");
            assert_eq!(detail.as_deref(), *why, "command line: {command}");
        }
    }
    #[test]
    fn knows_when_the_server_finished_starting() {
        assert!(super::is_ready_line("[13:54:20 INFO]: Done (10.725s)! For help, type \"help\""));
        assert!(super::is_ready_line("2026-08-30 15:36:37  INFO main pumpkin: Started server; took 57ms"));
        assert!(!super::is_ready_line("[13:54:10 INFO]: Preparing spawn area: 42%"));
        assert!(!super::is_ready_line("[13:54:11 INFO]: Done preparing level \"world\" (2.054s)"));
    }

    #[test]
    fn strips_the_colour_codes_pumpkin_prints() {
        let esc = '\u{1b}';
        let line = format!(
            "{esc}[2m2026-08-26 15:36:37{esc}[0m {esc}[32m INFO{esc}[0m main {esc}[2mpumpkin{esc}[0m{esc}[2m:{esc}[0m Started server; took {esc}[33m57ms{esc}[0m"
        );
        assert_eq!(super::strip_ansi(&line), "2026-08-26 15:36:37  INFO main pumpkin: Started server; took 57ms");
        let plain = "[15:36:37 INFO]: Done (1.234s)! For help, type \"help\"";
        assert_eq!(super::strip_ansi(plain), plain);
        assert_eq!(super::strip_ansi(&format!("{esc}[1;31;40mred{esc}[0m")), "red");
        assert_eq!(super::strip_ansi(&format!("{esc}[2Jend")), "end");
    }

    #[test]
    fn encodes_a_form_body() {
        assert_eq!(super::percent_encode("abcXYZ019-_.~"), "abcXYZ019-_.~");
        assert_eq!(super::percent_encode("a b"), "a%20b");
        assert_eq!(super::percent_encode("a+b&c=d"), "a%2Bb%26c%3Dd");
        assert_eq!(super::percent_encode("\n"), "%0A");
        assert_eq!(super::percent_encode("\u{AC00}"), "%EA%B0%80");
    }

    #[test]
    fn opens_anything_that_is_text() {
        use std::path::Path;
        let can = |name: &str| super::editable_file(Path::new(name));
        assert!(can("run.bat"));
        assert!(can("start.sh"));
        assert!(can("latest.log"));
        assert!(can("user_jvm_args.txt"));
        assert!(can("pumpkin.toml"));
        assert!(can("Dockerfile"));
        assert!(can("server.properties"));
        assert!(can("ops.json"));
        assert!(!can("server.jar"));
        assert!(!can("r.0.0.mca"));
        assert!(!can("level.dat"));
        assert!(!can("pack.png"));
        assert!(!can("world.zip"));
    }

    #[test]
    fn finds_player_files_in_both_layouts() {
        let root = std::env::temp_dir().join("monolab-player-file-test");
        std::fs::remove_dir_all(&root).ok();

        let modern = root.join("world/players/data");
        std::fs::create_dir_all(&modern).unwrap();
        std::fs::write(modern.join("a.dat"), b"x").unwrap();
        assert_eq!(super::player_file(&root, "world", "data", "a.dat"), Some(modern.join("a.dat")));

        let old = root.join("world/playerdata");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("b.dat"), b"x").unwrap();
        assert_eq!(super::player_file(&root, "world", "data", "b.dat"), Some(old.join("b.dat")));

        let stats = root.join("world/stats");
        std::fs::create_dir_all(&stats).unwrap();
        std::fs::write(stats.join("c.json"), b"{}").unwrap();
        assert_eq!(super::player_file(&root, "world", "stats", "c.json"), Some(stats.join("c.json")));

        assert_eq!(super::player_file(&root, "world", "data", "missing.dat"), None);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn finds_the_args_file_a_run_script_uses() {
        let mine = if cfg!(windows) { "win_args.txt" } else { "unix_args.txt" };
        let folder = std::env::temp_dir().join("monolab-args-test");
        std::fs::remove_dir_all(&folder).ok();
        let deep = folder.join("libraries/net/neoforged/neoforge/21.1.248");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join(mine), "--launchTarget forgeserver").unwrap();
        std::fs::write(
            folder.join("run.bat"),
            "@echo off\nREM note\njava @user_jvm_args.txt              @libraries/net/neoforged/neoforge/21.1.248/win_args.txt %*\npause\n",
        )
        .unwrap();
        let wanted = format!("libraries/net/neoforged/neoforge/21.1.248/{mine}");
        assert_eq!(super::script_args_file(&folder).unwrap(), wanted);

        std::fs::remove_file(folder.join("run.bat")).unwrap();
        let newer = folder.join("libraries/net/neoforged/neoforge/21.1.251");
        std::fs::create_dir_all(&newer).unwrap();
        std::fs::write(newer.join(mine), "x").unwrap();
        let older = folder.join("libraries/net/neoforged/neoforge/21.1.77");
        std::fs::create_dir_all(&older).unwrap();
        std::fs::write(older.join(mine), "x").unwrap();
        assert_eq!(super::script_args_file(&folder).unwrap(), format!("libraries/net/neoforged/neoforge/21.1.251/{mine}"));
        assert_eq!(super::detect_launch_mode("Custom", &folder), "script");

        std::fs::remove_dir_all(folder.join("libraries")).unwrap();
        let forge = folder.join("libraries/net/minecraftforge/forge/1.20.1-47.2.0");
        std::fs::create_dir_all(&forge).unwrap();
        std::fs::write(forge.join(mine), "x").unwrap();
        assert!(super::script_args_file(&folder).unwrap().contains("forge/1.20.1-47.2.0"));

        std::fs::remove_dir_all(folder.join("libraries")).unwrap();
        assert!(super::script_args_file(&folder).is_err());
        assert_ne!(super::detect_launch_mode("NeoForge", &folder), "script");
        std::fs::remove_dir_all(&folder).ok();
    }

    use super::{parse_tps_line, replace_config_line};

    #[test]
    fn unwraps_a_zipped_server_folder() {
        let names = |list: &[&str]| list.iter().map(|one| one.to_string()).collect::<Vec<_>>();
        assert_eq!(super::archive_root_prefix(&names(&["Pack/server.jar", "Pack/eula.txt"])), Some("Pack/".into()));
    }

    #[test]
    fn unpacks_where_you_point_it() {
        use super::chosen_root;
        let here = std::env::temp_dir().join("se-chosen-root");
        std::fs::create_dir_all(&here).unwrap();

        assert_eq!(chosen_root(None).unwrap(), None);
        assert_eq!(chosen_root(Some("   ")).unwrap(), None);

        let got = chosen_root(Some(here.to_str().unwrap())).unwrap().unwrap();
        assert_eq!(got, here.canonicalize().map(super::plain_path).unwrap());

        let missing = here.join("missing-folder");
        assert!(chosen_root(Some(missing.to_str().unwrap())).is_err());

        let _ = std::fs::remove_dir_all(&here);
    }

    #[test]
    fn relabels_servers_registered_before_the_sniffer() {
        use super::{resniff_unknown_cores, ServerInstance};
        let here = std::env::temp_dir().join("se-resniff");
        let _ = std::fs::remove_dir_all(&here);
        let paper = here.join("paper");
        std::fs::create_dir_all(paper.join("plugins")).unwrap();
        let bare = here.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        let make = |path: &std::path::Path, core: &str| ServerInstance {
            id: String::new(),
            name: String::new(),
            path: path.to_string_lossy().to_string(),
            core: core.into(),
            version: "Needs review".into(),
            launch_mode: super::default_launch_mode(),
            server_jar: None,
            java_path: "java".into(),
            max_memory_mib: 4096,
            min_memory_mib: 4096,
            jvm_args: String::new(),
            port: 25565,
            sort_order: 0,
            group: String::new(),
            start_with_app: false,
            crash_restart: false,
            core_build: String::new(),
            created_at: 0,
            last_run_at: 0,
            custom_command: String::new(),
            start_script: String::new(),
            start_script_hash: String::new(),
            remote_host: String::new(),
            linked: false,
            panel_host: String::new(),
            panel_server: String::new(),
            panel_daemon: String::new(),
            named: false,
        };
        let mut instances = vec![make(&paper, super::sniff::UNKNOWN), make(&bare, super::sniff::UNKNOWN), make(&paper, "Fabric")];
        assert!(resniff_unknown_cores(&mut instances));
        assert_eq!(instances[0].core, "Paper");
        assert_eq!(instances[1].core, super::sniff::UNKNOWN);
        assert_eq!(instances[2].core, "Fabric");
        assert!(!resniff_unknown_cores(&mut instances));
        let _ = std::fs::remove_dir_all(&here);
    }

    #[test]
    fn new_server_port_skips_ports_of_local_servers() {
        let make = |port: u16, remote: &str| super::ServerInstance {
            id: String::new(),
            name: String::new(),
            path: String::new(),
            core: "Paper".into(),
            version: String::new(),
            launch_mode: super::default_launch_mode(),
            server_jar: None,
            java_path: "java".into(),
            max_memory_mib: 4096,
            min_memory_mib: 4096,
            jvm_args: String::new(),
            port,
            sort_order: 0,
            group: String::new(),
            start_with_app: false,
            crash_restart: false,
            core_build: String::new(),
            created_at: 0,
            last_run_at: 0,
            custom_command: String::new(),
            start_script: String::new(),
            start_script_hash: String::new(),
            remote_host: remote.into(),
            linked: false,
            panel_host: String::new(),
            panel_server: String::new(),
            panel_daemon: String::new(),
            named: false,
        };
        let held = std::net::TcpListener::bind(("0.0.0.0", 47213)).unwrap();
        let instances = vec![make(47210, ""), make(47211, ""), make(47212, "host-a")];
        assert_eq!(super::new_server_port(&instances, 47210), 47212);
        assert_eq!(super::new_server_port(&instances, 47213), 47214);
        drop(held);
    }

    #[test]
    fn makes_english_folder_names() {
        use super::ascii_folder_name;
        assert_eq!(ascii_folder_name("Server 3"), "server_3");
        assert_eq!(ascii_folder_name("My Server"), "my_server");
        assert_eq!(ascii_folder_name("\u{C11C}\u{BC84}"), "server");
        assert_eq!(ascii_folder_name("  survival  world  "), "survival_world");
        assert_eq!(ascii_folder_name("Paper 1.21.4"), "paper_1_21_4");
    }

    #[test]
    fn reads_tps_from_the_value_list_not_the_timestamp() {
        let snapshot =
            parse_tps_line("[13:45:22 INFO]: TPS from last 1m, 5m, 15m: 19.8, 20.0, 20.0").expect("should recognise the TPS line");
        assert_eq!(snapshot.tps, Some(19.8));
        assert_eq!(snapshot.mspt, None);
    }

    #[test]
    fn reads_tps_with_color_codes() {
        let snapshot = parse_tps_line("[13:45:22 INFO]: TPS from last 1m, 5m, 15m: §a20.0, §a20.0, §a20.0")
            .expect("should recognise it even with color codes");
        assert_eq!(snapshot.tps, Some(20.0));
    }

    #[test]
    fn reads_average_tick_time_from_the_numeric_row() {
        let snapshot = parse_tps_line("[13:45:22 INFO]: ◴ 1.2/0.5/8.0, 1.3/0.4/9.1, 1.4/0.3/22.0").expect("should recognise the MSPT line");
        assert_eq!(snapshot.mspt, Some(1.2));
        assert_eq!(snapshot.tps, None);
    }

    #[test]
    fn ignores_ordinary_log_lines() {
        assert!(parse_tps_line("[13:45:22 INFO]: Steve joined the game").is_none());
        assert!(parse_tps_line("[13:45:22 INFO]: Done (12.345s)! For help, type \"help\"").is_none());
    }

    #[test]
    fn replaces_a_configuration_line_in_place() {
        let updated = replace_config_line("# comment\nbind = \"0.0.0.0:25577\"\nmotd = \"hi\"", "bind", "bind = \"0.0.0.0:25578\"");
        assert!(updated.contains("bind = \"0.0.0.0:25578\""));
        assert!(updated.contains("motd = \"hi\""));
        assert!(!updated.contains("25577"));
    }
}

#[cfg(test)]
mod panel_version_tests {
    #[test]
    fn keeps_only_the_game_version() {
        assert_eq!(super::game_version_of("26.3-42-main@08ca088"), "26.3");
        assert_eq!(super::game_version_of("1.21.8-40"), "1.21.8");
        assert!(super::is_game_version("26.3"));
        assert!(super::is_game_version("1.21.8"));
        assert!(!super::is_game_version("Needs review"));
        assert!(!super::is_game_version("26.3-42-main@08ca088"));
        assert!(!super::is_game_version("latest"));
    }
}

#[cfg(test)]
mod start_script_tests {
    use super::check_script_name;

    #[test]
    fn takes_only_a_plain_script_name() {
        assert_eq!(check_script_name("start.bat", false).unwrap(), "start.bat");
        assert_eq!(check_script_name(" Run Server.CMD ", false).unwrap(), "Run Server.CMD");
        assert_eq!(check_script_name("start.sh", true).unwrap(), "start.sh");
        assert!(check_script_name("start.sh", false).is_err());
        assert!(check_script_name("start.bat", true).is_err());
        for bad in ["", "../start.bat", "sub/start.bat", "a&b.bat", "x%y.bat", "C:start.bat", ".hidden.bat", "a;b.sh", "a$b.sh"] {
            assert!(check_script_name(bad, bad.ends_with(".sh")).is_err(), "{bad}");
        }
    }
}

#[cfg(test)]
mod map_change_tests {
    use super::{changed_regions, tile_touches_region, RegionStamps};

    #[test]
    fn finds_redrawn_added_and_removed_regions() {
        let old: RegionStamps = [((0, 0), (10, 5)), ((1, 0), (10, 5)), ((2, 0), (10, 5))].into();
        let fresh: RegionStamps = [((0, 0), (10, 5)), ((1, 0), (11, 6)), ((3, 0), (12, 7))].into();
        assert_eq!(changed_regions(&old, &fresh), vec![(1, 0), (2, 0), (3, 0)]);
        assert!(changed_regions(&fresh, &fresh).is_empty());
    }

    #[test]
    fn knows_which_tiles_show_a_region() {
        assert!(tile_touches_region(0, 0, 0, 0, 0));
        assert!(tile_touches_region(0, 3, 3, 0, 0));
        assert!(tile_touches_region(0, 4, 0, 0, 0), "the next tile shades against the region edge");
        assert!(!tile_touches_region(0, 5, 0, 0, 0));
        assert!(tile_touches_region(3, 0, 0, 1, 1));
        assert!(!tile_touches_region(0, -2, 0, 0, 0));
    }
}

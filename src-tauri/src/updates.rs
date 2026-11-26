//! Add-on and PumpkinMC fork updates without a new app, and the app's live connection to the
//! account server (`/v2/app`). See `docs/updates-and-stats.md` sections 1–3.
//!
//! Only a manifest signed with the updater key (`plugins.updater.pubkey` in tauri.conf.json) is
//! believed, and a downloaded file is used only when its sha256 and size match that manifest. The
//! live connection's `updates` message is just a hint to read the manifest again.

use crate::data_root::MonolabDirs;
use crate::events::{self, AddonKind};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio_tungstenite::tungstenite::Message;

pub const MANIFEST_URL: &str = "https://github.com/chicodev11/Monolab/releases/download/updates/updates.json";
/// Tells the screens a download finished: `{ "kind": "addon" | "fork", "version": n }`.
pub const READY: &str = "app-updates-ready";
const SCHEMA: u32 = 1;
const ADDONS: &str = "addons";
const FORKS: &str = "forks";
const KEEP_ADDONS: usize = 2;

// ── the manifest ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub schema: u32,
    #[serde(default)]
    pub addon: Option<AddonEntry>,
    #[serde(default)]
    pub fork: Option<ForkEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AddonEntry {
    pub version: u32,
    pub min_app: String,
    #[serde(default)]
    pub notes: String,
    pub base: String,
    pub files: BTreeMap<String, AddonFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AddonFile {
    pub sha256: String,
    pub size: u64,
    /// A Pumpkin wasm built against a newer fork: skipped while this PC's fork build is lower.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_fork: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ForkEntry {
    pub build: u32,
    pub min_app: String,
    #[serde(default)]
    pub notes: String,
    pub base: String,
    /// Keyed by platform, like `windows-x86_64`.
    pub files: BTreeMap<String, ForkFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ForkFile {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, String> {
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|error| format!("The update list is malformed: {error}"))?;
    if manifest.schema != SCHEMA {
        return Err(format!("The update list has schema {}, this app reads {SCHEMA}.", manifest.schema));
    }
    Ok(manifest)
}

/// Whether this app (`running`) is at least `min_app`. A version that doesn't parse never fits.
pub fn app_fits(min_app: &str, running: &str) -> bool {
    match (semver::Version::parse(min_app.trim()), semver::Version::parse(running.trim())) {
        (Ok(min), Ok(running)) => running >= min,
        _ => false,
    }
}

fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The platform key the fork files are listed under: `windows-x86_64`, `linux-aarch64`, `macos-aarch64`…
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

// ── the signature ──────────────────────────────────────────────────────────

/// The updater's public key: base64 of a minisign public key file.
fn updater_pubkey() -> Result<String, String> {
    let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).map_err(|error| error.to_string())?;
    config["plugins"]["updater"]["pubkey"].as_str().map(str::to_string).ok_or_else(|| "The app has no updater key.".to_string())
}

fn decode_text(text: &str) -> Result<String, String> {
    use base64::Engine;
    let trimmed = text.trim();
    if trimmed.starts_with("untrusted comment:") {
        return Ok(trimmed.to_string());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(trimmed.replace(['\r', '\n'], ""))
        .map_err(|error| format!("Not base64: {error}"))?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

/// Checks `bytes` against a minisign signature (the .sig file, plain or base64 like tauri's signer
/// writes it) made with the key `pubkey` (base64 of a minisign public key file).
pub fn verify_signature(bytes: &[u8], signature: &str, pubkey: &str) -> Result<(), String> {
    let key_text = decode_text(pubkey).map_err(|error| format!("The updater key is malformed: {error}"))?;
    let key = minisign_verify::PublicKey::decode(&key_text)
        .or_else(|_| minisign_verify::PublicKey::from_base64(key_text.lines().last().unwrap_or_default().trim()))
        .map_err(|error| format!("The updater key is malformed: {error}"))?;
    let signature_text = decode_text(signature).map_err(|error| format!("The update list's signature is malformed: {error}"))?;
    let signature = minisign_verify::Signature::decode(&signature_text)
        .map_err(|error| format!("The update list's signature is malformed: {error}"))?;
    key.verify(bytes, &signature, true).map_err(|error| format!("The update list's signature doesn't match: {error}"))
}

// ── files on disk ──────────────────────────────────────────────────────────

static RESOURCES: OnceLock<PathBuf> = OnceLock::new();

fn data_dir() -> Option<PathBuf> {
    crate::data_root::current()
}

fn addons_dir() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join(ADDONS))
}

fn forks_dir() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join(FORKS))
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Plain file names only: what the add-on release holds, nothing that could leave the folder.
fn safe_addon_name(name: &str) -> bool {
    name.starts_with(events::PREFIX)
        && (name.ends_with(".jar") || name.ends_with(".wasm"))
        && name.len() <= 120
        && name.chars().all(|letter| letter.is_ascii_alphanumeric() || matches!(letter, '.' | '-' | '_'))
}

fn safe_url_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 120 && name.chars().all(|letter| letter.is_ascii_alphanumeric() || matches!(letter, '.' | '-' | '_'))
}

/// The add-on version a file name carries, whatever kind of server it's for.
fn version_in_file(name: &str) -> Option<u32> {
    [AddonKind::Paper, AddonKind::NeoForge, AddonKind::Fabric, AddonKind::Pumpkin]
        .into_iter()
        .find_map(|kind| events::addon_version_in_name(kind, name))
}

fn record_name(version: u32) -> String {
    format!("addon-{version}.json")
}

/// Downloaded add-on versions whose files were all checked, newest first.
fn downloaded_addons(dir: &Path) -> Vec<AddonEntry> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<AddonEntry> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let version: u32 = name.strip_prefix("addon-")?.strip_suffix(".json")?.parse().ok()?;
            let entry: AddonEntry = serde_json::from_slice(&fs::read(entry.path()).ok()?).ok()?;
            (entry.version == version).then_some(entry)
        })
        .collect();
    found.sort_by_key(|entry| std::cmp::Reverse(entry.version));
    found
}

fn read_build(path: &Path) -> Option<u32> {
    let value: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    value["build"].as_u64().and_then(|build| u32::try_from(build).ok())
}

/// This PC's fork build: `<data>/forks/fork.json`, else the bundled `resources/forks/fork.json`, else 0.
pub fn local_fork_build() -> u32 {
    forks_dir()
        .and_then(|dir| read_build(&dir.join("fork.json")))
        .or_else(|| RESOURCES.get().and_then(|dir| read_build(&dir.join(FORKS).join("fork.json"))))
        .unwrap_or(0)
}

/// A downloaded fork build waiting to be swapped in.
fn pending_fork_build() -> Option<u32> {
    let dir = forks_dir()?;
    dir.join(format!("{}.new", exe("pumpkin"))).is_file().then(|| read_build(&dir.join("fork.json.new"))).flatten()
}

// ── which add-on to use ────────────────────────────────────────────────────

/// What a server gets when its add-on is installed or checked: the bundled version or a newer
/// downloaded one, chosen per server kind and game version.
pub struct Addons {
    downloaded: Vec<AddonEntry>,
    addons: Option<PathBuf>,
    resources: Option<PathBuf>,
    fork: u32,
}

/// The add-ons this app can install right now (`app` only settles the folders first).
pub fn current_addon(app: &AppHandle) -> Addons {
    let _ = app.path().monolab_data_dir();
    current()
}

pub fn current() -> Addons {
    let addons = addons_dir();
    Addons {
        downloaded: addons.as_deref().map(downloaded_addons).unwrap_or_default(),
        addons,
        resources: RESOURCES.get().cloned(),
        fork: local_fork_build(),
    }
}

impl Addons {
    /// The version and file name a server of `kind` on `game` gets; `None` when nothing is built for it.
    pub fn pick(&self, kind: AddonKind, game: &str) -> Option<(u32, String)> {
        let addons = self.addons.clone();
        choose(kind, game, events::ADDON_VERSION, &self.downloaded, self.fork, |name| {
            addons.as_ref().is_some_and(|dir| dir.join(name).is_file())
        })
    }

    /// Where the file `name` is: the downloaded folder first, then the app's resources.
    pub fn resolve(&self, name: &str) -> Option<PathBuf> {
        if !safe_addon_name(name) {
            return None;
        }
        [self.addons.as_ref(), self.resources.as_ref()].into_iter().flatten().map(|dir| dir.join(name)).find(|path| path.is_file())
    }
}

/// Per kind and game version: the highest downloaded version newer than the bundled one whose file
/// for this server is there (and, for Pumpkin, whose `minFork` this PC's fork meets); else the
/// bundled version's file.
fn choose(
    kind: AddonKind,
    game: &str,
    bundled: u32,
    downloaded: &[AddonEntry],
    fork: u32,
    present: impl Fn(&str) -> bool,
) -> Option<(u32, String)> {
    for entry in downloaded.iter().filter(|entry| entry.version > bundled) {
        let Some(name) = events::jar_name(kind, game, entry.version) else { continue };
        let Some(file) = entry.files.get(&name) else { continue };
        if kind == AddonKind::Pumpkin && file.min_fork.is_some_and(|needed| needed > fork) {
            continue;
        }
        if present(&name) {
            return Some((entry.version, name));
        }
    }
    events::jar_name(kind, game, bundled).map(|name| (bundled, name))
}

/// The newest add-on version this app holds, for the live hello.
fn have_addon() -> u32 {
    let downloaded = addons_dir().as_deref().map(downloaded_addons).unwrap_or_default();
    downloaded.first().map(|entry| entry.version).unwrap_or(0).max(events::ADDON_VERSION)
}

fn have_fork() -> u32 {
    local_fork_build().max(pending_fork_build().unwrap_or(0))
}

// ── downloads ──────────────────────────────────────────────────────────────

fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder().connect_timeout(Duration::from_secs(20)).user_agent("Monolab").build().expect("http client")
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Whether what arrived is what the manifest lists.
fn check_digest(got_size: u64, want_size: u64, got_sha256: &str, want_sha256: &str) -> Result<(), String> {
    if got_size != want_size {
        return Err(format!("Expected {want_size} bytes, got {got_size}."));
    }
    if !got_sha256.eq_ignore_ascii_case(want_sha256.trim()) {
        return Err("The file's sha256 doesn't match the update list.".into());
    }
    Ok(())
}

/// Whether a file already on disk is the one listed.
fn file_matches(path: &Path, sha256: &str, size: u64) -> bool {
    let Ok(meta) = fs::metadata(path) else { return false };
    if meta.len() != size {
        return false;
    }
    let Ok(bytes) = fs::read(path) else { return false };
    check_digest(bytes.len() as u64, size, &hex(&Sha256::digest(&bytes)), sha256).is_ok()
}

/// Downloads `url` into `dest` through `dest.part`, keeping it only when size and sha256 match.
async fn download(url: &str, dest: &Path, sha256: &str, size: u64) -> Result<(), String> {
    let part = dest.with_file_name(format!("{}.part", dest.file_name().and_then(|name| name.to_str()).unwrap_or("download")));
    let result = async {
        let mut response = http()
            .get(url)
            .timeout(Duration::from_secs(15 * 60))
            .send()
            .await
            .and_then(|reply| reply.error_for_status())
            .map_err(|error| format!("Could not download {url}: {error}"))?;
        let mut file = fs::File::create(&part).map_err(|error| error.to_string())?;
        let mut hasher = Sha256::new();
        let mut got = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(|error| format!("The download stopped: {error}"))? {
            got += chunk.len() as u64;
            if got > size {
                return Err(format!("{url} is bigger than the update list says."));
            }
            hasher.update(&chunk);
            file.write_all(&chunk).map_err(|error| error.to_string())?;
        }
        file.flush().map_err(|error| error.to_string())?;
        drop(file);
        check_digest(got, size, &hex(&hasher.finalize()), sha256)?;
        fs::rename(&part, dest).map_err(|error| error.to_string())
    }
    .await;
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

fn file_url(base: &str, name: &str) -> Result<String, String> {
    if !base.starts_with("https://") || !safe_url_name(name) {
        return Err(format!("The update list points at an unusable address for {name}."));
    }
    Ok(format!("{}/{name}", base.trim_end_matches('/')))
}

async fn fetch_manifest() -> Result<Manifest, String> {
    let get = |url: String| async move {
        http()
            .get(&url)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .and_then(|reply| reply.error_for_status())
            .map_err(|error| format!("Could not read {url}: {error}"))?
            .bytes()
            .await
            .map_err(|error| error.to_string())
    };
    let (body, signature) = tokio::join!(get(MANIFEST_URL.to_string()), get(format!("{MANIFEST_URL}.sig")));
    let (body, signature) = (body?, signature?);
    verify_signature(&body, &String::from_utf8_lossy(&signature), &updater_pubkey()?)?;
    parse_manifest(&body)
}

async fn download_addon(dir: &Path, entry: &AddonEntry) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    for (name, file) in &entry.files {
        if !safe_addon_name(name) || version_in_file(name) != Some(entry.version) {
            return Err(format!("The update list names an unexpected add-on file: {name}"));
        }
        let dest = dir.join(name);
        if file_matches(&dest, &file.sha256, file.size) {
            continue;
        }
        download(&file_url(&entry.base, name)?, &dest, &file.sha256, file.size).await?;
    }
    let record = serde_json::to_vec_pretty(entry).map_err(|error| error.to_string())?;
    let temp = dir.join(format!("{}.part", record_name(entry.version)));
    fs::write(&temp, record).map_err(|error| error.to_string())?;
    fs::rename(&temp, dir.join(record_name(entry.version))).map_err(|error| error.to_string())?;
    prune_addons(dir);
    Ok(())
}

/// Keeps the newest two downloaded versions; older files, records and leftovers go.
fn prune_addons(dir: &Path) {
    let all = downloaded_addons(dir);
    let kept: Vec<&AddonEntry> = all.iter().take(KEEP_ADDONS).collect();
    let keep = |name: &str| kept.iter().any(|entry| entry.files.contains_key(name));
    for old in all.iter().skip(KEEP_ADDONS) {
        for name in old.files.keys().filter(|name| safe_addon_name(name) && !keep(name)) {
            let _ = fs::remove_file(dir.join(name));
        }
        let _ = fs::remove_file(dir.join(record_name(old.version)));
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let listed = keep(&name) || kept.iter().any(|one| record_name(one.version) == name);
            if name.ends_with(".part") || (safe_addon_name(&name) && !listed) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

async fn download_fork(dir: &Path, entry: &ForkEntry, file: &ForkFile) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let program = dir.join(format!("{}.new", exe("pumpkin")));
    let record = dir.join("fork.json.new");
    let _ = fs::remove_file(&record);
    download(&file_url(&entry.base, &file.name)?, &program, &file.sha256, file.size).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&program, fs::Permissions::from_mode(0o755));
    }
    fs::write(&record, json!({ "build": entry.build }).to_string()).map_err(|error| error.to_string())
}

static SWAP: Mutex<()> = Mutex::new(());

/// Puts a downloaded fork in place when no running server uses `<data>/forks/pumpkin(.exe)`.
/// Servers run their own copy, so this almost always goes through. `true` when it swapped.
pub fn swap_fork(app: &AppHandle) -> bool {
    let _guard = SWAP.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(dir) = forks_dir() else { return false };
    let new = dir.join(format!("{}.new", exe("pumpkin")));
    let new_record = dir.join("fork.json.new");
    if !new.is_file() || read_build(&new_record).is_none() {
        return false;
    }
    let target = dir.join(exe("pumpkin"));
    if target.is_file() && crate::pumpkin_exe_in_use(app, &target) {
        return false;
    }
    let old = dir.join(format!("{}.old", exe("pumpkin")));
    let _ = fs::remove_file(&old);
    if target.is_file() && fs::rename(&target, &old).is_err() {
        return false;
    }
    if let Err(error) = fs::rename(&new, &target) {
        eprintln!("Monolab: could not put the new PumpkinMC fork in place: {error}");
        let _ = fs::rename(&old, &target);
        return false;
    }
    if let Err(error) = fs::rename(&new_record, dir.join("fork.json")) {
        eprintln!("Monolab: could not record the new fork build: {error}");
    }
    let _ = fs::remove_file(&old);
    true
}

/// Right before a server on the bundled fork starts: swaps in a downloaded fork, then gives the
/// server's own copy the newer program when it's behind.
pub fn before_pumpkin_start(app: &AppHandle, server: &Path, server_jar: Option<&str>) {
    swap_fork(app);
    let dest = server.join(crate::cores::PUMPKIN_EXE);
    if server_jar.is_some_and(|jar| server.join(jar) != dest) {
        return;
    }
    let Some(source) = crate::forks::find("pumpkin") else { return };
    let _guard = SWAP.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (Ok(from), to) = (fs::metadata(&source), fs::metadata(&dest).ok()) else { return };
    let behind = match to {
        None => true,
        Some(to) => from.len() != to.len() || matches!((from.modified(), to.modified()), (Ok(new), Ok(old)) if new > old),
    };
    if !behind {
        return;
    }
    let part = server.join(format!("{}.part", crate::cores::PUMPKIN_EXE));
    let copied = fs::copy(&source, &part).and_then(|_| fs::rename(&part, &dest));
    if let Err(error) = copied {
        let _ = fs::remove_file(&part);
        eprintln!("Monolab: could not refresh {}: {error}", dest.display());
    }
}

static CHECKING: AtomicBool = AtomicBool::new(false);

/// Reads the signed manifest and downloads what's newer and fits this app. One at a time.
pub async fn check(app: &AppHandle) -> Result<(), String> {
    if CHECKING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let result = check_once(app).await;
    CHECKING.store(false, Ordering::SeqCst);
    if let Err(error) = &result {
        eprintln!("Monolab: update check: {error}");
    }
    result
}

async fn check_once(app: &AppHandle) -> Result<(), String> {
    let manifest = fetch_manifest().await?;
    let data = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    if let Some(addon) = manifest.addon.as_ref().filter(|addon| addon.version > have_addon() && app_fits(&addon.min_app, app_version())) {
        download_addon(&data.join(ADDONS), addon).await?;
        let _ = app.emit(READY, json!({ "kind": "addon", "version": addon.version }));
    }
    if let Some(fork) = manifest.fork.as_ref().filter(|fork| fork.build > have_fork() && app_fits(&fork.min_app, app_version())) {
        if let Some(file) = fork.files.get(&platform()) {
            download_fork(&data.join(FORKS), fork, file).await?;
            let handle = app.clone();
            let _ = tauri::async_runtime::spawn_blocking(move || swap_fork(&handle)).await;
            let _ = app.emit(READY, json!({ "kind": "fork", "version": fork.build }));
        }
    }
    Ok(())
}

// ── the live connection ────────────────────────────────────────────────────

static GENERATION: AtomicU64 = AtomicU64::new(0);
static CONNECTED: AtomicBool = AtomicBool::new(false);

fn live() -> &'static Mutex<Option<tauri::async_runtime::JoinHandle<()>>> {
    static LIVE: OnceLock<Mutex<Option<tauri::async_runtime::JoinHandle<()>>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(None))
}

/// At app start: settles a waiting fork, opens the live connection and, when it isn't up within
/// 20 seconds, reads the manifest once.
pub fn init(app: &AppHandle) {
    if let Ok(resources) = app.path().resource_dir() {
        let _ = RESOURCES.set(resources);
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        swap_fork(&handle);
    });
    start(app);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        if !CONNECTED.load(Ordering::SeqCst) {
            let _ = check(&handle).await;
        }
    });
}

/// (Re)starts the live connection, e.g. after the anonymous counts switch changed.
pub fn start(app: &AppHandle) {
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let mut wait = 5u64;
        loop {
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            let started = std::time::Instant::now();
            let _ = session(&app).await;
            if started.elapsed() > Duration::from_secs(60) {
                wait = 5;
            }
            tokio::time::sleep(Duration::from_secs(wait)).await;
            wait = (wait * 2).min(300);
        }
    });
    if let Some(old) = live().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).replace(task) {
        old.abort();
    }
}

pub fn reconnect(app: &AppHandle) {
    start(app);
}

/// Closes the live connection (app closing).
pub fn stop() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(task) = live().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take() {
        task.abort();
    }
}

/// The random install id, made the first time; `None` while anonymous counts are off.
fn install_id(app: &AppHandle) -> Option<String> {
    if !crate::read_app_settings(app).anonymous_stats {
        return None;
    }
    let path = app.path().monolab_data_dir().ok()?.join("install-id");
    if let Ok(text) = fs::read_to_string(&path) {
        let text = text.trim().to_string();
        if valid_install_id(&text) {
            return Some(text);
        }
    }
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    let id = hex(&bytes);
    fs::write(&path, &id).ok()?;
    Some(id)
}

fn valid_install_id(text: &str) -> bool {
    text.len() == 32 && text.chars().all(|letter| matches!(letter, '0'..='9' | 'a'..='f'))
}

pub fn hello(app: &AppHandle) -> Value {
    json!({
        "type": "hello",
        "app": app_version(),
        "addon": have_addon(),
        "fork": local_fork_build(),
        "os": platform(),
        "install": install_id(app),
    })
}

async fn session(app: &AppHandle) -> Result<(), String> {
    let base = crate::monolink_account::server();
    let url = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}/v2/app")
    } else {
        format!("ws://{}/v2/app", base.trim_start_matches("http://"))
    };
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
    let (socket, _) =
        tokio::time::timeout(Duration::from_secs(15), tokio_tungstenite::connect_async_tls_with_config(url, None, false, Some(connector)))
            .await
            .map_err(|_| "timeout".to_string())?
            .map_err(|error| error.to_string())?;
    let (mut sink, mut source) = socket.split();
    let hello = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || hello(&app)).await.map_err(|error| error.to_string())?
    };
    sink.send(Message::Text(hello.to_string().into())).await.map_err(|error| error.to_string())?;
    CONNECTED.store(true, Ordering::SeqCst);
    let mut ping = tokio::time::interval(Duration::from_secs(60));
    ping.tick().await;
    loop {
        tokio::select! {
            incoming = tokio::time::timeout(Duration::from_secs(180), source.next()) => {
                let Ok(Some(Ok(message))) = incoming else { return Err("closed".into()) };
                let Message::Text(text) = message else { continue };
                let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };
                if value["type"].as_str() == Some("updates") && newer_than_held(&value) {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = check(&app).await;
                    });
                }
            }
            _ = ping.tick() => {
                if sink.send(Message::Text(json!({ "type": "ping" }).to_string().into())).await.is_err() {
                    return Err("closed".into());
                }
            }
        }
    }
}

/// Whether an `updates` message names something newer than this app holds.
fn newer_than_held(message: &Value) -> bool {
    let addon = message["addon"].as_u64().unwrap_or(0);
    let fork = message["fork"].as_u64().unwrap_or(0);
    addon > u64::from(have_addon()) || fork > u64::from(have_fork())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Signer;

    const SAMPLE: &str = r#"{
      "schema": 1,
      "addon": {
        "version": 42, "minApp": "1.0.2", "notes": "short English line",
        "base": "https://github.com/chicodev11/Monolab/releases/download/addon-42/",
        "files": { "Monolab-42.jar": { "sha256": "ab", "size": 640706 },
                   "Monolab-pumpkin-26.63-42.wasm": { "sha256": "cd", "size": 1, "minFork": 3 } }
      },
      "fork": {
        "build": 3, "minApp": "1.0.2", "notes": "",
        "base": "https://github.com/chicodev11/Monolab/releases/download/fork-3/",
        "files": { "windows-x86_64": { "name": "pumpkin-windows-x86_64.exe", "sha256": "ef", "size": 1 } }
      }
    }"#;

    #[test]
    fn parses_the_manifest_and_compares_min_app() {
        let manifest = parse_manifest(SAMPLE.as_bytes()).unwrap();
        let addon = manifest.addon.as_ref().unwrap();
        assert_eq!(addon.version, 42);
        assert_eq!(addon.files["Monolab-pumpkin-26.63-42.wasm"].min_fork, Some(3));
        assert_eq!(addon.files["Monolab-42.jar"].min_fork, None);
        let fork = manifest.fork.as_ref().unwrap();
        assert_eq!(fork.build, 3);
        assert_eq!(fork.files["windows-x86_64"].name, "pumpkin-windows-x86_64.exe");
        assert!(app_fits("1.0.2", "1.0.2"));
        assert!(app_fits("1.0.2", "1.0.10"));
        assert!(app_fits("1.0.2", "1.1.0"));
        assert!(!app_fits("1.0.2", "1.0.1"));
        assert!(!app_fits("1.0.2", "1.0.2-beta.1"));
        assert!(!app_fits("soon", "1.0.2"));
        assert!(parse_manifest(br#"{"schema":2}"#).is_err());
        assert!(parse_manifest(b"not json").is_err());
    }

    /// A minisign key pair made in the test, so a good signature can be checked too.
    fn keys() -> (ed25519_dalek::SigningKey, String, [u8; 8]) {
        use base64::Engine;
        let secret = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let keynum = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut raw = b"Ed".to_vec();
        raw.extend_from_slice(&keynum);
        raw.extend_from_slice(&secret.verifying_key().to_bytes());
        let file = format!("untrusted comment: test key\n{}\n", base64::engine::general_purpose::STANDARD.encode(raw));
        (secret, base64::engine::general_purpose::STANDARD.encode(file), keynum)
    }

    fn sign(secret: &ed25519_dalek::SigningKey, keynum: [u8; 8], data: &[u8]) -> String {
        use base64::Engine;
        let engine = base64::engine::general_purpose::STANDARD;
        let signature = secret.sign(data).to_bytes();
        let mut line = b"Ed".to_vec();
        line.extend_from_slice(&keynum);
        line.extend_from_slice(&signature);
        let comment = "timestamp:1";
        let mut global = signature.to_vec();
        global.extend_from_slice(comment.as_bytes());
        let global = secret.sign(&global).to_bytes();
        let file = format!("untrusted comment: sig\n{}\ntrusted comment: {comment}\n{}\n", engine.encode(line), engine.encode(global));
        engine.encode(file)
    }

    #[test]
    fn rejects_a_bad_signature() {
        let (secret, pubkey, keynum) = keys();
        let good = sign(&secret, keynum, SAMPLE.as_bytes());
        assert!(verify_signature(SAMPLE.as_bytes(), &good, &pubkey).is_ok());
        // The same signature over changed bytes.
        assert!(verify_signature(b"{\"schema\":1}", &good, &pubkey).is_err());
        // Signed by another key.
        let other = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
        assert!(verify_signature(SAMPLE.as_bytes(), &sign(&other, keynum, SAMPLE.as_bytes()), &pubkey).is_err());
        // Garbage and nothing.
        assert!(verify_signature(SAMPLE.as_bytes(), "not a signature", &pubkey).is_err());
        assert!(verify_signature(SAMPLE.as_bytes(), "", &pubkey).is_err());
        // The app's real key doesn't accept the test key's signature.
        assert!(verify_signature(SAMPLE.as_bytes(), &good, &updater_pubkey().unwrap()).is_err());
    }

    fn entry(version: u32, files: &[(&str, Option<u32>)]) -> AddonEntry {
        AddonEntry {
            version,
            min_app: "1.0.0".into(),
            notes: String::new(),
            base: "https://example.invalid/".into(),
            files: files
                .iter()
                .map(|(name, min_fork)| (name.to_string(), AddonFile { sha256: "00".into(), size: 1, min_fork: *min_fork }))
                .collect(),
        }
    }

    #[test]
    fn chooses_the_version_per_kind() {
        let fork = crate::forks::PUMPKIN_FORK;
        let downloaded = vec![
            entry(43, &[("Monolab-43.jar", None), ("Monolab-pumpkin-26.63-43.wasm", Some(5))]),
            entry(42, &[("Monolab-42.jar", None), ("Monolab-pumpkin-26.63-42.wasm", Some(3))]),
        ];
        let all = |_: &str| true;
        // Paper takes the newest downloaded jar.
        assert_eq!(choose(AddonKind::Paper, "1.21.1", 41, &downloaded, 0, all), Some((43, "Monolab-43.jar".into())));
        // Pumpkin: 43 needs fork 5, 42 needs fork 3.
        assert_eq!(choose(AddonKind::Pumpkin, fork, 41, &downloaded, 5, all), Some((43, "Monolab-pumpkin-26.63-43.wasm".into())));
        assert_eq!(choose(AddonKind::Pumpkin, fork, 41, &downloaded, 4, all), Some((42, "Monolab-pumpkin-26.63-42.wasm".into())));
        // An older fork falls back to the bundled wasm, while Paper still gets 43.
        assert_eq!(choose(AddonKind::Pumpkin, fork, 41, &downloaded, 0, all), Some((41, "Monolab-pumpkin-26.63-41.wasm".into())));
        // A file the release didn't list, or that isn't on disk, is skipped.
        let only_42 = |name: &str| name.contains("-42");
        assert_eq!(choose(AddonKind::Paper, "1.21.1", 41, &downloaded, 0, only_42), Some((42, "Monolab-42.jar".into())));
        assert_eq!(choose(AddonKind::NeoForge, "1.21.1", 41, &downloaded, 0, all), Some((41, "Monolab-mod-mc1_21_1-41.jar".into())));
        // Downloaded versions not newer than the bundled one are ignored.
        assert_eq!(choose(AddonKind::Paper, "1.21.1", 43, &downloaded, 0, all), Some((43, "Monolab-43.jar".into())));
        assert_eq!(choose(AddonKind::Paper, "1.21.1", 44, &downloaded, 0, all), Some((44, "Monolab-44.jar".into())));
        // Nothing built for this game.
        assert_eq!(choose(AddonKind::NeoForge, "1.20.1", 41, &downloaded, 0, all), None);
    }

    #[test]
    fn rejects_a_sha256_or_size_mismatch() {
        let bytes = b"monolab add-on";
        let right = hex(&Sha256::digest(bytes));
        assert!(check_digest(bytes.len() as u64, bytes.len() as u64, &right, &right).is_ok());
        assert!(check_digest(bytes.len() as u64, bytes.len() as u64, &right, &right.to_uppercase()).is_ok());
        let wrong = hex(&Sha256::digest(b"something else"));
        assert!(check_digest(bytes.len() as u64, bytes.len() as u64, &right, &wrong).is_err());
        assert!(check_digest(bytes.len() as u64, 3, &right, &right).is_err());
        let dir = std::env::temp_dir().join(format!("monolab-updates-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Monolab-42.jar");
        fs::write(&path, bytes).unwrap();
        assert!(file_matches(&path, &right, bytes.len() as u64));
        assert!(!file_matches(&path, &wrong, bytes.len() as u64));
        assert!(!file_matches(&path, &right, 1));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn keeps_the_newest_two_versions() {
        let dir = std::env::temp_dir().join(format!("monolab-updates-prune-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for version in [42u32, 43, 44] {
            let name = format!("Monolab-{version}.jar");
            fs::write(dir.join(&name), b"x").unwrap();
            fs::write(dir.join(record_name(version)), serde_json::to_vec(&entry(version, &[(&name, None)])).unwrap()).unwrap();
        }
        fs::write(dir.join("Monolab-45.jar.part"), b"x").unwrap();
        prune_addons(&dir);
        assert!(!dir.join("Monolab-42.jar").exists());
        assert!(!dir.join(record_name(42)).exists());
        assert!(dir.join("Monolab-43.jar").is_file());
        assert!(dir.join("Monolab-44.jar").is_file());
        assert!(!dir.join("Monolab-45.jar.part").exists());
        assert_eq!(downloaded_addons(&dir).iter().map(|one| one.version).collect::<Vec<_>>(), vec![44, 43]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_and_ids() {
        assert!(safe_addon_name("Monolab-pumpkin-26.63-42.wasm"));
        assert!(!safe_addon_name("../Monolab-42.jar"));
        assert!(!safe_addon_name("Other-42.jar"));
        assert!(valid_install_id("0123456789abcdef0123456789abcdef"));
        assert!(!valid_install_id("0123456789ABCDEF0123456789abcdef"));
        assert!(file_url("http://x/", "a.jar").is_err());
        assert_eq!(file_url("https://x/r/", "Monolab-42.jar").unwrap(), "https://x/r/Monolab-42.jar");
    }
}

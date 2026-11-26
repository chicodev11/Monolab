//! Installs and starts a game client instance in offline mode: the version
//! files, libraries, natives and assets the official launcher would fetch,
//! Fabric or Quilt on top when asked, then the game with an offline name.
//! Shared files live in `<data>/minecraft`; each instance keeps its own
//! game folder (saves, mods, options).

use crate::data_root::MonolabDirs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};
use tauri::{AppHandle, Manager};

const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const ASSETS: &str = "https://resources.download.minecraft.net";
const WORKERS: usize = 8;

// ---------- offline account ----------

#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ClientAccount {
    /// The name shown in game when playing offline. Empty until set.
    #[serde(default)]
    pub offline_name: String,
}

fn account_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().monolab_data_dir().map_err(|e| e.to_string())?.join("client_account.json"))
}

fn load_account(app: &AppHandle) -> Result<ClientAccount, String> {
    Ok(crate::store::read(&account_path(app)?)?.unwrap_or_default())
}

pub fn valid_name(name: &str) -> Result<(), String> {
    let ok = (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(())
    } else {
        Err("A player name is 3 to 16 letters, numbers or underscores.".into())
    }
}

/// The UUID an offline server gives this name: a v3 UUID of `OfflinePlayer:<name>`.
pub fn offline_uuid(name: &str) -> String {
    use md5::{Digest as _, Md5};
    let mut bytes: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}").as_bytes()).into();
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

#[tauri::command(async)]
#[specta::specta]
pub fn get_client_account(app: AppHandle) -> Result<ClientAccount, String> {
    load_account(&app)
}

#[tauri::command(async)]
#[specta::specta]
pub fn set_offline_name(app: AppHandle, name: String) -> Result<ClientAccount, String> {
    let name = name.trim().to_string();
    valid_name(&name)?;
    let account = ClientAccount { offline_name: name };
    let text = serde_json::to_string_pretty(&account).map_err(|e| e.to_string())?;
    crate::store::write(&account_path(&app)?, &text).map_err(|e| format!("Could not save the account: {e}"))?;
    Ok(account)
}

// ---------- progress and running games ----------

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ClientRun {
    pub id: String,
    /// "preparing", "java", "libraries", "assets", "starting", "running", "failed", "" when idle.
    pub stage: String,
    pub done: u32,
    pub total: u32,
    pub error: String,
}

struct Slot {
    run: ClientRun,
    child: Option<Arc<Mutex<Child>>>,
}

fn runs() -> &'static Mutex<HashMap<String, Slot>> {
    static ALL: OnceLock<Mutex<HashMap<String, Slot>>> = OnceLock::new();
    ALL.get_or_init(Default::default)
}

fn set_stage(id: &str, stage: &str, done: u32, total: u32) {
    if let Ok(mut all) = runs().lock() {
        let slot = all.entry(id.to_string()).or_insert_with(|| Slot { run: ClientRun::default(), child: None });
        slot.run = ClientRun { id: id.to_string(), stage: stage.into(), done, total, error: String::new() };
    }
}

fn fail(id: &str, error: String) {
    if let Ok(mut all) = runs().lock() {
        let slot = all.entry(id.to_string()).or_insert_with(|| Slot { run: ClientRun::default(), child: None });
        slot.run = ClientRun { id: id.to_string(), stage: "failed".into(), error, ..Default::default() };
        slot.child = None;
    }
}

/// What every instance is doing right now; idle ones are left out.
#[tauri::command(async)]
#[specta::specta]
pub fn client_runs() -> Vec<ClientRun> {
    runs().lock().map(|all| all.values().map(|slot| slot.run.clone()).filter(|run| !run.stage.is_empty()).collect()).unwrap_or_default()
}

#[tauri::command(async)]
#[specta::specta]
pub fn stop_client(id: String) -> Result<(), String> {
    let child = runs().lock().ok().and_then(|all| all.get(&id).and_then(|slot| slot.child.clone()));
    let child = child.ok_or("This instance is not running.")?;
    let mut child = child.lock().map_err(|_| "The game is busy.".to_string())?;
    child.kill().map_err(|e| format!("Could not stop the game: {e}"))
}

/// Clears a failed state once the page has shown it.
#[tauri::command(async)]
#[specta::specta]
pub fn dismiss_client_run(id: String) {
    if let Ok(mut all) = runs().lock() {
        if all.get(&id).is_some_and(|slot| slot.child.is_none()) {
            all.remove(&id);
        }
    }
}

/// Installs what is missing and starts the game. Returns at once; progress is
/// read with `client_runs`.
#[tauri::command(async)]
#[specta::specta]
pub fn launch_client(app: AppHandle, id: String) -> Result<(), String> {
    let instance = crate::clients::find(&app, &id)?;
    let account = load_account(&app)?;
    valid_name(&account.offline_name).map_err(|_| "Set an offline player name first.".to_string())?;
    {
        let mut all = runs().lock().map_err(|_| "The launcher is busy.".to_string())?;
        if let Some(slot) = all.get(&id) {
            if slot.child.is_some() || !matches!(slot.run.stage.as_str(), "" | "failed") {
                return Err("This instance is already starting or running.".into());
            }
        }
        all.insert(id.clone(), Slot { run: ClientRun { id: id.clone(), stage: "preparing".into(), ..Default::default() }, child: None });
    }
    std::thread::spawn(move || {
        if let Err(error) = install_and_start(&app, &instance, &account.offline_name) {
            fail(&id, error);
        }
    });
    Ok(())
}

// ---------- install ----------

fn http() -> Result<reqwest::blocking::Client, String> {
    crate::net::builder().timeout(Duration::from_secs(120)).build().map_err(|e| e.to_string())
}

fn get_json(client: &reqwest::blocking::Client, url: &str) -> Result<Value, String> {
    crate::net::check_download_url(url)?;
    client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Could not download {url}: {e}"))?
        .json()
        .map_err(|e| format!("Could not read {url}: {e}"))
}

fn sha1_of(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha1::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Some(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Clone, Debug)]
struct Fetch {
    url: String,
    to: PathBuf,
    sha1: Option<String>,
}

impl Fetch {
    fn present(&self) -> bool {
        match &self.sha1 {
            Some(want) => sha1_of(&self.to).is_some_and(|got| got.eq_ignore_ascii_case(want)),
            None => self.to.is_file(),
        }
    }
}

fn fetch_one(client: &reqwest::blocking::Client, one: &Fetch) -> Result<(), String> {
    crate::net::check_download_url(&one.url)?;
    if let Some(parent) = one.to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
    }
    let mut last = String::new();
    for _ in 0..3 {
        let result = client.get(&one.url).send().and_then(|r| r.error_for_status()).and_then(|r| r.bytes());
        match result {
            Ok(bytes) => {
                if let Some(want) = &one.sha1 {
                    let got: String = Sha1::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
                    if !got.eq_ignore_ascii_case(want) {
                        last = format!("{} did not match its checksum.", one.url);
                        continue;
                    }
                }
                let temp = one.to.with_extension("part");
                fs::write(&temp, &bytes)
                    .and_then(|_| fs::rename(&temp, &one.to))
                    .map_err(|e| format!("Could not save {}: {e}", one.to.display()))?;
                return Ok(());
            }
            Err(error) => last = format!("Could not download {}: {error}", one.url),
        }
    }
    Err(last)
}

/// Downloads everything missing on a few threads, reporting `stage` progress.
fn fetch_all(client: &reqwest::blocking::Client, id: &str, stage: &str, list: Vec<Fetch>) -> Result<(), String> {
    let missing: Vec<Fetch> = list.into_iter().filter(|one| !one.present()).collect();
    let total = missing.len() as u32;
    set_stage(id, stage, 0, total);
    if missing.is_empty() {
        return Ok(());
    }
    let queue = Arc::new(Mutex::new(missing));
    let done = Arc::new(AtomicU64::new(0));
    let error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let mut workers = Vec::new();
    for _ in 0..WORKERS {
        let (queue, done, error, client, id, stage) =
            (queue.clone(), done.clone(), error.clone(), client.clone(), id.to_string(), stage.to_string());
        workers.push(std::thread::spawn(move || loop {
            if error.lock().map(|e| e.is_some()).unwrap_or(true) {
                return;
            }
            let Some(next) = queue.lock().ok().and_then(|mut q| q.pop()) else { return };
            if let Err(message) = fetch_one(&client, &next) {
                if let Ok(mut slot) = error.lock() {
                    slot.get_or_insert(message);
                }
                return;
            }
            let now = done.fetch_add(1, Ordering::Relaxed) + 1;
            set_stage(&id, &stage, now as u32, total);
        }));
    }
    for worker in workers {
        let _ = worker.join();
    }
    let failed = error.lock().ok().and_then(|mut e| e.take());
    match failed {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

fn os_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

/// Mojang's allow/disallow rules. Features (demo, custom size, quick play) are all off.
fn allowed(rules: Option<&Value>) -> bool {
    let Some(rules) = rules.and_then(Value::as_array) else { return true };
    let mut ok = false;
    for rule in rules {
        let os = rule.get("os");
        let os_match = os.is_none_or(|os| {
            let name = os.get("name").and_then(Value::as_str).is_none_or(|n| n == os_name());
            let arch = os.get("arch").and_then(Value::as_str).is_none_or(|a| (a == "x86") == cfg!(target_arch = "x86"));
            name && arch
        });
        let features_match = rule.get("features").and_then(Value::as_object).is_none_or(|f| f.values().all(|v| v == false));
        if os_match && features_match {
            ok = rule.get("action").and_then(Value::as_str) == Some("allow");
        }
    }
    ok
}

/// `group:artifact:version[:classifier]` → `group/path/artifact/version/artifact-version[-classifier].jar`.
pub fn maven_path(name: &str) -> Option<String> {
    let (coords, ext) = name.split_once('@').unwrap_or((name, "jar"));
    let parts: Vec<&str> = coords.split(':').collect();
    let (group, artifact, version) = (parts.first()?, parts.get(1)?, parts.get(2)?);
    let classifier = parts.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    Some(format!("{}/{artifact}/{version}/{artifact}-{version}{classifier}.{ext}", group.replace('.', "/")))
}

/// The library's identity without its version, to let a loader's copy replace Mojang's.
fn lib_key(name: &str) -> String {
    let parts: Vec<&str> = name.split(':').collect();
    match parts.as_slice() {
        [g, a, _, c, ..] => format!("{g}:{a}:{c}"),
        [g, a, ..] => format!("{g}:{a}"),
        _ => name.to_string(),
    }
}

struct Plan {
    version_id: String,
    main_class: String,
    classpath: Vec<PathBuf>,
    natives: Vec<PathBuf>,
    fetch: Vec<Fetch>,
    jvm_args: Vec<String>,
    game_args: Vec<String>,
    asset_index: Value,
    java_major: u32,
    logging: Option<(String, PathBuf)>,
    version_type: String,
}

fn args_of(list: Option<&Value>) -> Vec<String> {
    let mut out = Vec::new();
    for one in list.and_then(Value::as_array).into_iter().flatten() {
        match one {
            Value::String(text) => out.push(text.clone()),
            Value::Object(entry) => {
                if !allowed(entry.get("rules")) {
                    continue;
                }
                match entry.get("value") {
                    Some(Value::String(text)) => out.push(text.clone()),
                    Some(Value::Array(items)) => out.extend(items.iter().filter_map(Value::as_str).map(String::from)),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

fn plan(root: &Path, vanilla: &Value, loader: Option<&Value>) -> Result<Plan, String> {
    let libs_dir = root.join("libraries");
    let mc_id = vanilla.get("id").and_then(Value::as_str).ok_or("The version file has no id.")?.to_string();
    let mut fetch = Vec::new();
    let mut classpath = Vec::new();
    let mut natives = Vec::new();
    let mut seen = HashSet::new();

    // A loader's libraries come first and win over Mojang's copy of the same one.
    for lib in loader.and_then(|l| l.get("libraries")).and_then(Value::as_array).into_iter().flatten() {
        let Some(name) = lib.get("name").and_then(Value::as_str) else { continue };
        if !allowed(lib.get("rules")) || !seen.insert(lib_key(name)) {
            continue;
        }
        let path = maven_path(name).ok_or_else(|| format!("Bad library name: {name}"))?;
        let base = lib.get("url").and_then(Value::as_str).unwrap_or("https://libraries.minecraft.net/");
        let to = libs_dir.join(&path);
        fetch.push(Fetch {
            url: format!("{}/{path}", base.trim_end_matches('/')),
            to: to.clone(),
            sha1: lib.get("sha1").and_then(Value::as_str).map(String::from),
        });
        classpath.push(to);
    }
    for lib in vanilla.get("libraries").and_then(Value::as_array).into_iter().flatten() {
        let Some(name) = lib.get("name").and_then(Value::as_str) else { continue };
        if !allowed(lib.get("rules")) || !seen.insert(lib_key(name)) {
            continue;
        }
        let downloads = lib.get("downloads");
        if let Some(artifact) = downloads.and_then(|d| d.get("artifact")) {
            let path = artifact.get("path").and_then(Value::as_str).map(String::from).or_else(|| maven_path(name));
            if let (Some(path), Some(url)) = (path, artifact.get("url").and_then(Value::as_str)) {
                let to = libs_dir.join(&path);
                if !url.is_empty() {
                    fetch.push(Fetch {
                        url: url.into(),
                        to: to.clone(),
                        sha1: artifact.get("sha1").and_then(Value::as_str).map(String::from),
                    });
                }
                if name.contains(":natives-") {
                    natives.push(to.clone());
                }
                classpath.push(to);
            }
        }
        // Old versions keep natives in a classifier per system.
        if let Some(classifier) = lib.get("natives").and_then(|n| n.get(os_name())).and_then(Value::as_str) {
            let classifier = classifier.replace("${arch}", if cfg!(target_pointer_width = "64") { "64" } else { "32" });
            if let Some(native) = downloads.and_then(|d| d.get("classifiers")).and_then(|c| c.get(&classifier)) {
                if let (Some(path), Some(url)) = (native.get("path").and_then(Value::as_str), native.get("url").and_then(Value::as_str)) {
                    let to = libs_dir.join(path);
                    fetch.push(Fetch {
                        url: url.into(),
                        to: to.clone(),
                        sha1: native.get("sha1").and_then(Value::as_str).map(String::from),
                    });
                    natives.push(to);
                }
            }
        }
    }

    let client = vanilla.pointer("/downloads/client").ok_or("This version has no client download.")?;
    let jar = root.join("versions").join(&mc_id).join(format!("{mc_id}.jar"));
    fetch.push(Fetch {
        url: client.get("url").and_then(Value::as_str).ok_or("The client download has no address.")?.into(),
        to: jar.clone(),
        sha1: client.get("sha1").and_then(Value::as_str).map(String::from),
    });
    classpath.push(jar);

    let logging = vanilla.pointer("/logging/client").and_then(|log| {
        let file = log.get("file")?;
        let to = root.join("assets").join("log_configs").join(file.get("id")?.as_str()?);
        fetch.push(Fetch {
            url: file.get("url")?.as_str()?.into(),
            to: to.clone(),
            sha1: file.get("sha1").and_then(Value::as_str).map(String::from),
        });
        Some((log.get("argument")?.as_str()?.to_string(), to))
    });

    let mut jvm_args = args_of(vanilla.pointer("/arguments/jvm"));
    let mut game_args = match vanilla.get("minecraftArguments").and_then(Value::as_str) {
        Some(legacy) => legacy.split_whitespace().map(String::from).collect(),
        None => args_of(vanilla.pointer("/arguments/game")),
    };
    if jvm_args.is_empty() {
        jvm_args = vec!["-Djava.library.path=${natives_directory}".into(), "-cp".into(), "${classpath}".into()];
    }
    if let Some(loader) = loader {
        jvm_args.extend(args_of(loader.pointer("/arguments/jvm")));
        game_args.extend(args_of(loader.pointer("/arguments/game")));
    }
    let main_class = loader
        .and_then(|l| l.get("mainClass"))
        .or_else(|| vanilla.get("mainClass"))
        .and_then(Value::as_str)
        .ok_or("The version file has no main class.")?
        .to_string();
    Ok(Plan {
        version_id: loader.and_then(|l| l.get("id")).and_then(Value::as_str).unwrap_or(&mc_id).to_string(),
        main_class,
        classpath,
        natives,
        fetch,
        jvm_args,
        game_args,
        asset_index: vanilla.get("assetIndex").cloned().ok_or("The version file has no asset index.")?,
        java_major: vanilla.pointer("/javaVersion/majorVersion").and_then(Value::as_u64).unwrap_or(8) as u32,
        logging,
        version_type: vanilla.get("type").and_then(Value::as_str).unwrap_or("release").to_string(),
    })
}

fn loader_profile(client: &reqwest::blocking::Client, loader: &str, mc: &str) -> Result<Option<Value>, String> {
    let base = match loader {
        // Nova reads the vanilla files and prepares its own resources from them.
        "Vanilla" | "Nova" => return Ok(None),
        "Fabric" => "https://meta.fabricmc.net/v2",
        "Quilt" => "https://meta.quiltmc.org/v3",
        other => return Err(format!("{other} instances can't be launched yet. Vanilla, Fabric, Quilt and Nova can.")),
    };
    let list = get_json(client, &format!("{base}/versions/loader/{mc}"))?;
    let items = list.as_array().ok_or("The loader list is malformed.")?;
    let pick = items
        .iter()
        .find(|one| {
            one.pointer("/loader/stable").and_then(Value::as_bool).unwrap_or(true)
                && !one.pointer("/loader/version").and_then(Value::as_str).unwrap_or("").contains("beta")
        })
        .or_else(|| items.first())
        .and_then(|one| one.pointer("/loader/version"))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{loader} has no loader for Minecraft {mc}."))?;
    get_json(client, &format!("{base}/versions/loader/{mc}/{pick}/profile/json")).map(Some)
}

fn extract_natives(jars: &[PathBuf], into: &Path) -> Result<(), String> {
    fs::create_dir_all(into).map_err(|e| format!("Could not create the natives folder: {e}"))?;
    for jar in jars {
        let file = fs::File::open(jar).map_err(|e| format!("Could not open {}: {e}", jar.display()))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("Could not read {}: {e}", jar.display()))?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
            let name = entry.name().unwrap_or_default().to_string();
            let wanted = [".dll", ".so", ".dylib", ".jnilib"].iter().any(|ext| name.ends_with(ext));
            if entry.is_dir() || name.starts_with("META-INF") || !wanted {
                continue;
            }
            // Flatten: LWJGL looks for the library by file name only.
            let Some(file_name) = Path::new(&name).file_name() else { continue };
            let to = into.join(file_name);
            let mut out = fs::File::create(&to).map_err(|e| format!("Could not write {}: {e}", to.display()))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn fill(text: &str, values: &HashMap<&str, String>) -> String {
    let mut out = text.to_string();
    for (key, value) in values {
        out = out.replace(&format!("${{{key}}}"), value);
    }
    out
}

fn install_and_start(app: &AppHandle, instance: &crate::clients::ClientInstance, name: &str) -> Result<(), String> {
    let id = instance.id.as_str();
    let data = app.path().monolab_data_dir().map_err(|e| e.to_string())?;
    let root = data.join("minecraft");
    let game_dir = crate::clients::game_dir(app, id)?;
    fs::create_dir_all(&game_dir).map_err(|e| format!("Could not create the instance folder: {e}"))?;
    let client = http()?;

    set_stage(id, "preparing", 0, 0);
    let mc = &instance.version;
    let version_file = root.join("versions").join(mc).join(format!("{mc}.json"));
    let vanilla: Value = match fs::read_to_string(&version_file).ok().and_then(|t| serde_json::from_str(&t).ok()) {
        Some(found) => found,
        None => {
            let manifest = get_json(&client, MANIFEST)?;
            let entry = manifest
                .get("versions")
                .and_then(Value::as_array)
                .and_then(|all| all.iter().find(|one| one.get("id").and_then(Value::as_str) == Some(mc)))
                .ok_or_else(|| format!("Minecraft {mc} was not found."))?;
            let url = entry.get("url").and_then(Value::as_str).ok_or("The version list is malformed.")?;
            // Kept byte for byte: Nova checks this file against Mojang's own checksum.
            fetch_one(
                &client,
                &Fetch { url: url.into(), to: version_file.clone(), sha1: entry.get("sha1").and_then(Value::as_str).map(String::from) },
            )?;
            let text = fs::read_to_string(&version_file).map_err(|e| e.to_string())?;
            serde_json::from_str(&text).map_err(|e| format!("The version file is damaged: {e}"))?
        }
    };
    let loader = loader_profile(&client, &instance.loader, mc)?;
    let plan = plan(&root, &vanilla, loader.as_ref())?;

    set_stage(id, "java", 0, 0);
    let java = crate::java::java_for_client(app, plan.java_major)?;

    fetch_all(&client, id, "libraries", plan.fetch.clone())?;

    // Assets: the index, then every object it names.
    let index_id = plan.asset_index.get("id").and_then(Value::as_str).unwrap_or("legacy").to_string();
    let assets_dir = root.join("assets");
    let index_file = assets_dir.join("indexes").join(format!("{index_id}.json"));
    let index_fetch = Fetch {
        url: plan.asset_index.get("url").and_then(Value::as_str).ok_or("The asset index has no address.")?.into(),
        to: index_file.clone(),
        sha1: plan.asset_index.get("sha1").and_then(Value::as_str).map(String::from),
    };
    if !index_fetch.present() {
        fetch_one(&client, &index_fetch)?;
    }
    let index: Value = serde_json::from_str(&fs::read_to_string(&index_file).map_err(|e| e.to_string())?)
        .map_err(|e| format!("The asset index is damaged: {e}"))?;
    let objects = index.get("objects").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut asset_fetch = Vec::new();
    for object in objects.values() {
        let Some(hash) = object.get("hash").and_then(Value::as_str) else { continue };
        if hash.len() < 2 {
            continue;
        }
        asset_fetch.push(Fetch {
            url: format!("{ASSETS}/{}/{hash}", &hash[..2]),
            to: assets_dir.join("objects").join(&hash[..2]).join(hash),
            sha1: None,
        });
    }
    fetch_all(&client, id, "assets", asset_fetch)?;
    // Very old versions read assets by name instead of by hash.
    let virtual_dir = if index.get("map_to_resources").and_then(Value::as_bool) == Some(true) {
        Some(game_dir.join("resources"))
    } else if index.get("virtual").and_then(Value::as_bool) == Some(true) {
        Some(assets_dir.join("virtual").join(&index_id))
    } else {
        None
    };
    if let Some(dir) = &virtual_dir {
        for (name, object) in &objects {
            let Some(hash) = object.get("hash").and_then(Value::as_str) else { continue };
            let to = dir.join(name);
            if !to.is_file() && hash.len() >= 2 {
                if let Some(parent) = to.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::copy(assets_dir.join("objects").join(&hash[..2]).join(hash), &to);
            }
        }
    }

    if instance.loader == "Nova" {
        return start_nova(app, id, &root, &vanilla, &plan, &java, &game_dir, name);
    }

    set_stage(id, "starting", 0, 0);
    let natives_dir = root.join("natives").join(&plan.version_id);
    extract_natives(&plan.natives, &natives_dir)?;
    let separator = if cfg!(windows) { ";" } else { ":" };
    let classpath = plan.classpath.iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>().join(separator);
    let uuid = offline_uuid(name);
    let values: HashMap<&str, String> = HashMap::from([
        ("auth_player_name", name.to_string()),
        ("version_name", plan.version_id.clone()),
        ("game_directory", game_dir.to_string_lossy().to_string()),
        ("assets_root", assets_dir.to_string_lossy().to_string()),
        ("game_assets", virtual_dir.as_ref().unwrap_or(&assets_dir).to_string_lossy().to_string()),
        ("assets_index_name", index_id.clone()),
        ("auth_uuid", uuid.replace('-', "")),
        ("auth_access_token", "0".into()),
        ("auth_session", "0".into()),
        ("clientid", String::new()),
        ("auth_xuid", String::new()),
        ("user_type", "legacy".into()),
        ("user_properties", "{}".into()),
        ("version_type", plan.version_type.clone()),
        ("natives_directory", natives_dir.to_string_lossy().to_string()),
        ("launcher_name", "Monolab".into()),
        ("launcher_version", env!("CARGO_PKG_VERSION").into()),
        ("classpath", classpath),
        ("classpath_separator", separator.into()),
        ("library_directory", root.join("libraries").to_string_lossy().to_string()),
    ]);
    let mut args: Vec<String> = vec!["-Xmx4096M".into(), "-Xms512M".into()];
    if let Some((argument, file)) = &plan.logging {
        args.push(argument.replace("${path}", &file.to_string_lossy()));
    }
    args.extend(plan.jvm_args.iter().map(|a| fill(a, &values)));
    args.push(plan.main_class.clone());
    args.extend(plan.game_args.iter().map(|a| fill(a, &values)));

    let mut command = crate::hidden_command(&java);
    command.args(&args);
    run_game(app, id, command, &game_dir, None)
}

/// Starts the game, records the launch, and watches it until it closes.
/// A game that fails within 20 seconds is reported with its log.
fn run_game(app: &AppHandle, id: &str, mut command: std::process::Command, game_dir: &Path, stdin: Option<String>) -> Result<(), String> {
    let logs = game_dir.join("logs");
    fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let log = fs::File::create(logs.join("monolab-launch.log")).map_err(|e| e.to_string())?;
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let program = command.get_program().to_string_lossy().to_string();
    let mut child = command
        .current_dir(game_dir)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(log)
        .stderr(err)
        .spawn()
        .map_err(|e| {
            let shown = program.clone();
            format!("Could not start the game ({shown}): {e}")
        })?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        use std::io::Write;
        let _ = pipe.write_all(text.as_bytes());
        // Dropping the pipe closes it, which tells the game the session is complete.
    }
    crate::clients::mark_played(app, id);
    let child = Arc::new(Mutex::new(child));
    if let Ok(mut all) = runs().lock() {
        all.insert(
            id.to_string(),
            Slot { run: ClientRun { id: id.to_string(), stage: "running".into(), ..Default::default() }, child: Some(child.clone()) },
        );
    }
    let started = std::time::Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(800));
        let status = child.lock().ok().and_then(|mut c| c.try_wait().ok().flatten());
        if let Some(status) = status {
            if !status.success() && started.elapsed() < Duration::from_secs(20) {
                let log = logs.join("monolab-launch.log");
                let log = log.display();
                return Err(format!("The game closed right away ({status}). See {log} for details."));
            }
            if let Ok(mut all) = runs().lock() {
                all.remove(id);
            }
            return Ok(());
        }
    }
}

// ---------- Nova ----------

/// Copies what `horizon-prepare` reads beside itself (the lock file and the exporters'
/// Java sources) into a writable folder, since the bundle may sit in a read-only place.
fn nova_root(data: &Path, bundle: &Path) -> Result<PathBuf, String> {
    let root = data.join("nova").join("root");
    let lock = bundle.join("research-lock.json");
    let same = fs::read(&lock).ok() == fs::read(root.join("research-lock.json")).ok();
    if !same {
        if root.exists() {
            fs::remove_dir_all(&root).map_err(|e| format!("Could not refresh Nova's files: {e}"))?;
        }
        copy_tree(bundle, &root, &["research-lock.json", "mods"])?;
    }
    Ok(root)
}

fn copy_tree(from: &Path, to: &Path, only: &[&str]) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if !only.is_empty() && !only.iter().any(|one| name.to_string_lossy() == *one) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &to.join(&name), &[])?;
        } else {
            fs::copy(&path, to.join(&name)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The folder Java is installed in (the one holding `bin/java`), for a path or a
/// bare `java` found on the PATH; links are followed to the real install.
fn java_home(java: &Path) -> Option<PathBuf> {
    let found = if java.components().count() > 1 {
        java.to_path_buf()
    } else {
        let name = if cfg!(windows) && java.extension().is_none() { java.with_extension("exe") } else { java.to_path_buf() };
        std::env::split_paths(&std::env::var_os("PATH")?).map(|dir| dir.join(&name)).find(|path| path.is_file())?
    };
    let real = fs::canonicalize(&found).unwrap_or(found);
    let home = real.parent()?.parent()?.to_path_buf();
    #[cfg(windows)]
    let home = PathBuf::from(home.to_string_lossy().trim_start_matches(r"\\?\").to_string());
    Some(home)
}

/// A line of `horizon-prepare --progress json`: how far it is, or why it failed.
fn prepare_line(line: &str) -> (Option<f64>, Option<String>) {
    let Ok(value) = serde_json::from_str::<Value>(line) else { return (None, None) };
    (value.get("fraction").and_then(Value::as_f64), value.get("error").and_then(Value::as_str).map(String::from))
}

#[allow(clippy::too_many_arguments)]
fn start_nova(
    app: &AppHandle,
    id: &str,
    root: &Path,
    vanilla: &Value,
    plan: &Plan,
    java: &Path,
    game_dir: &Path,
    name: &str,
) -> Result<(), String> {
    let bundle = crate::forks::nova_dir().ok_or("This copy of Monolab doesn't include Nova.")?;
    let data = app.path().monolab_data_dir().map_err(|e| e.to_string())?;
    let prepared = data.join("nova").join("prepared");
    let prepare = crate::forks::nova_program(&bundle, "horizon-prepare");
    let nova_root = nova_root(&data, &bundle)?;

    let checked = crate::hidden_command(&prepare)
        .args(["--check", "--root"])
        .arg(&nova_root)
        .arg("--output")
        .arg(&prepared)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Could not run Nova's preparer: {e}"))?;
    if !checked.status.success() {
        // The server jar: Nova reads the game's data from it once.
        let mc = plan.version_id.as_str();
        let server = vanilla.pointer("/downloads/server").ok_or("This version has no server download.")?;
        let server_jar = root.join("versions").join(mc).join(format!("{mc}-server.jar"));
        let server_fetch = Fetch {
            url: server.get("url").and_then(Value::as_str).ok_or("The server download has no address.")?.into(),
            to: server_jar.clone(),
            sha1: server.get("sha1").and_then(Value::as_str).map(String::from),
        };
        if !server_fetch.present() {
            set_stage(id, "libraries", 0, 1);
            fetch_one(&http()?, &server_fetch)?;
        }
        let java_home = java_home(java).ok_or("Could not tell where Java is installed.")?;
        set_stage(id, "nova", 0, 1000);
        let mut child = crate::hidden_command(&prepare)
            .args(["--prepare", "--complete", "--progress", "json", "--root"])
            .arg(&nova_root)
            .arg("--output")
            .arg(&prepared)
            .arg("--version-json")
            .arg(root.join("versions").join(mc).join(format!("{mc}.json")))
            .arg("--client-jar")
            .arg(root.join("versions").join(mc).join(format!("{mc}.jar")))
            .arg("--libraries-dir")
            .arg(root.join("libraries"))
            .arg("--assets-dir")
            .arg(root.join("assets"))
            .arg("--server-jar")
            .arg(&server_jar)
            .arg("--java-home")
            .arg(java_home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not run Nova's preparer: {e}"))?;
        let stderr = child.stderr.take();
        let errors = std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = stderr {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        });
        let mut failure = None;
        if let Some(out) = child.stdout.take() {
            use std::io::BufRead;
            for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                let (fraction, error) = prepare_line(&line);
                if let Some(fraction) = fraction {
                    set_stage(id, "nova", (fraction.clamp(0.0, 1.0) * 1000.0) as u32, 1000);
                }
                if error.is_some() {
                    failure = error;
                }
            }
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        let errors = errors.join().unwrap_or_default();
        if !status.success() {
            let detail = failure
                .or_else(|| errors.lines().rev().find(|l| !l.trim().is_empty()).map(String::from))
                .unwrap_or_else(|| status.to_string());
            return Err(format!("Nova could not prepare the game files: {detail}"));
        }
    }

    set_stage(id, "starting", 0, 0);
    let session = serde_json::json!({
        "name": name,
        "uuid": offline_uuid(name).replace('-', ""),
        "access_token": "0",
    })
    .to_string();
    let mut command = crate::hidden_command(crate::forks::nova_program(&bundle, "horizon-client"));
    command.arg("--resources").arg(&prepared).arg("--game-dir").arg(game_dir).arg("--home").arg(game_dir).arg("--session-stdin");
    run_game(app, id, command, game_dir, Some(session))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn offline_uuid_matches_the_server() {
        // What an offline-mode server computes for "Notch".
        assert_eq!(offline_uuid("Notch"), "b50ad385-829d-3141-a216-7e7d7539ba7f");
    }

    #[test]
    fn names_follow_minecraft_rules() {
        assert!(valid_name("Steve_01").is_ok());
        assert!(valid_name("ab").is_err());
        assert!(valid_name("\u{D55C}\u{AE00}\u{C774}\u{B984}").is_err());
        assert!(valid_name("a-b-c").is_err());
    }

    #[test]
    fn prepare_progress_lines_are_read() {
        assert_eq!(prepare_line(r#"{"fraction": 0.25}"#), (Some(0.25), None));
        assert_eq!(prepare_line(r#"{"error": "no java"}"#), (None, Some("no java".into())));
        assert_eq!(prepare_line(r#"{"message": "hi"}"#), (None, None));
        assert_eq!(prepare_line("plain text"), (None, None));
    }

    #[test]
    fn java_home_follows_the_path() {
        let dir = std::env::temp_dir().join(format!("monolab-java-{}", std::process::id()));
        let bin = dir.join("jdk").join("bin");
        fs::create_dir_all(&bin).unwrap();
        let exe = bin.join(if cfg!(windows) { "java.exe" } else { "java" });
        fs::write(&exe, b"").unwrap();
        // Both sides canonical: on Windows one may carry the `\\?\` prefix and the other not, for the same folder.
        let home = java_home(&exe).unwrap();
        assert_eq!(fs::canonicalize(&home).unwrap_or(home), fs::canonicalize(dir.join("jdk")).unwrap_or(dir.join("jdk")));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn maven_names_become_paths() {
        assert_eq!(maven_path("net.fabricmc:fabric-loader:0.16.9").unwrap(), "net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar");
        assert_eq!(maven_path("org.lwjgl:lwjgl:3.3.3:natives-windows").unwrap(), "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-windows.jar");
    }

    #[test]
    fn rules_pick_this_system() {
        let only_here = json!([{ "action": "allow", "os": { "name": os_name() } }]);
        let elsewhere = json!([{ "action": "allow", "os": { "name": "nowhere" } }]);
        let demo = json!([{ "action": "allow", "features": { "is_demo_user": true } }]);
        assert!(allowed(Some(&only_here)));
        assert!(!allowed(Some(&elsewhere)));
        assert!(!allowed(Some(&demo)));
        assert!(allowed(None));
    }

    #[test]
    fn loader_libraries_replace_mojangs_copy() {
        let vanilla = json!({
            "id": "1.21", "mainClass": "net.minecraft.client.main.Main",
            "assetIndex": { "id": "17", "url": "https://piston-meta.mojang.com/x.json" },
            "downloads": { "client": { "url": "https://piston-data.mojang.com/c.jar", "sha1": "aa" } },
            "libraries": [
                { "name": "org.ow2.asm:asm:9.3", "downloads": { "artifact": { "path": "org/ow2/asm/asm/9.3/asm-9.3.jar", "url": "https://libraries.minecraft.net/a.jar" } } },
                { "name": "com.mojang:brigadier:1.0", "downloads": { "artifact": { "path": "b.jar", "url": "https://libraries.minecraft.net/b.jar" } } }
            ],
            "arguments": { "game": ["--username", "${auth_player_name}", { "rules": [{ "action": "allow", "features": { "is_demo_user": true } }], "value": "--demo" }], "jvm": ["-cp", "${classpath}"] }
        });
        let fabric = json!({
            "id": "fabric-loader-0.16-1.21", "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
            "libraries": [{ "name": "org.ow2.asm:asm:9.7", "url": "https://maven.fabricmc.net/" }],
            "arguments": { "game": [], "jvm": ["-DFabricMcEmu= net.minecraft.client.main.Main "] }
        });
        let plan = plan(Path::new("/r"), &vanilla, Some(&fabric)).unwrap();
        assert_eq!(plan.main_class, "net.fabricmc.loader.impl.launch.knot.KnotClient");
        assert_eq!(plan.version_id, "fabric-loader-0.16-1.21");
        let names: Vec<String> = plan.classpath.iter().map(|p| p.to_string_lossy().replace('\\', "/")).collect();
        assert!(names.iter().any(|n| n.ends_with("asm-9.7.jar")));
        assert!(!names.iter().any(|n| n.ends_with("asm-9.3.jar")));
        assert!(names.iter().any(|n| n.ends_with("b.jar")));
        assert!(!plan.game_args.contains(&"--demo".to_string()));
        assert!(plan.jvm_args.iter().any(|a| a.starts_with("-DFabricMcEmu")));
    }
}

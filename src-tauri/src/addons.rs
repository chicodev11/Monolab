
use serde::Serialize;
use sha2::{Digest, Sha512};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::Path,
    time::Duration,
};
use zip::ZipArchive;

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAddon {
    pub file_name: String,
    pub mod_id: String,
    pub display_name: String,
    pub version: String,
    pub enabled: bool,
    pub sha512: String,
    pub project_id: Option<String>,
    pub project_title: Option<String>,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AddonUpdate {
    pub file_name: String,
    pub display_name: String,
    pub current_version: String,
    pub new_version: String,
    pub new_file_name: String,
    pub download_url: String,
    pub sha512: String,
    pub issues: Vec<UpdateIssue>,
}

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIssue {
    pub kind: String,
    pub project: String,
}

fn dependency_issues(
    version: &serde_json::Value,
    installed: &HashSet<String>,
    projects_of_versions: &HashMap<String, String>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for one in version.get("dependencies").and_then(|value| value.as_array()).into_iter().flatten() {
        let project = one.get("project_id").and_then(|value| value.as_str()).or_else(|| {
            one.get("version_id").and_then(|value| value.as_str()).and_then(|id| projects_of_versions.get(id)).map(String::as_str)
        });
        let Some(project) = project else { continue };
        match one.get("dependency_type").and_then(|value| value.as_str()) {
            Some("required") if !installed.contains(project) => out.push(("missing".to_string(), project.to_string())),
            Some("incompatible") if installed.contains(project) => out.push(("incompatible".to_string(), project.to_string())),
            _ => {}
        }
    }
    out
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AddonConflict {
    pub kind: String,
    pub message: String,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DependencyInfo {
    pub project_id: String,
    pub title: String,
    pub icon_url: Option<String>,
    pub version_name: String,
    pub required: bool,
    pub already_installed: bool,
    pub file_name: String,
    pub download_url: String,
    pub sha512: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub title: String,
    pub file_name: String,
    pub download_url: String,
    pub sha512: String,
    pub dependencies: Vec<DependencyInfo>,
}

pub fn client() -> Result<reqwest::blocking::Client, String> {
    crate::net::builder()
        .user_agent("Monolab/0.1.0 (local Windows desktop app)")
        .timeout(Duration::from_secs(30))
        // Fail fast offline: names already come from the jars, Modrinth only adds labels and updates.
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| error.to_string())
}

fn sha512_of(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha512::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[derive(Default)]
struct Descriptors {
    fabric: Option<String>,
    forge: Option<(&'static str, String)>,
    plugin_yml: Option<(&'static str, String)>,
}

fn read_descriptors<R: std::io::Read + std::io::Seek>(zip: &mut ZipArchive<R>) -> Descriptors {
    let mut found = Descriptors::default();
    let pull = |zip: &mut ZipArchive<R>, name: &str| -> Option<String> {
        let mut entry = zip.by_name(name).ok()?;
        let mut text = String::new();
        entry.read_to_string(&mut text).ok()?;
        Some(text)
    };
    found.fabric = pull(zip, "fabric.mod.json");
    for candidate in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        if let Some(text) = pull(zip, candidate) {
            found.forge = Some((candidate, text));
            break;
        }
    }
    for candidate in ["paper-plugin.yml", "plugin.yml"] {
        if let Some(text) = pull(zip, candidate) {
            found.plugin_yml = Some((candidate, text));
            break;
        }
    }
    found
}

fn yml_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&format!("{key}:"))
            .map(|value| value.trim().trim_matches(['"', '\'']).to_string())
    })
}

fn identity_from(found: &Descriptors, fallback: &str) -> (String, String, String) {
    if let Some(text) = &found.fabric {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
            let id = value
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or(fallback)
                .to_string();
            let name = value
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or(&id)
                .to_string();
            let version = value
                .get("version")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            return (id, name, version);
        }
    }
    if let Some((_, text)) = &found.forge {
        if let Some((id, name, version)) = mod_from_toml(text) {
            let id = if id.is_empty() { fallback.to_string() } else { id };
            let name = if name.is_empty() { id.clone() } else { name };
            return (id, name, version);
        }
    }
    if let Some((_, text)) = &found.plugin_yml {
        let name = yml_value(text, "name").unwrap_or_else(|| fallback.to_string());
        let version = yml_value(text, "version").unwrap_or_default();
        return (name.clone(), name, version);
    }
    (fallback.to_string(), fallback.to_string(), String::new())
}

fn icon_from(zip: &mut ZipArchive<fs::File>, found: &Descriptors) -> Option<String> {
    let mut named: Option<String> = None;
    if let Some(text) = &found.fabric {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
            named = value
                .get("icon")
                .and_then(|value| value.as_str())
                .map(str::to_owned);
        }
    }
    if named.is_none() {
        if let Some((_, text)) = &found.forge {
            named = text.lines().find_map(|line| {
                let line = line.trim();
                line.strip_prefix("logoFile=")
                    .or_else(|| line.strip_prefix("logoFile ="))
                    .map(|value| value.trim().trim_matches('"').to_string())
            });
        }
    }

    let mut tries: Vec<String> = Vec::new();
    if let Some(name) = named {
        tries.push(name);
    }
    tries.extend(
        ["icon.png", "logo.png", "pack.png", "assets/icon.png"]
            .into_iter()
            .map(str::to_owned),
    );

    for name in tries {
        let Ok(mut entry) = zip.by_name(&name) else {
            continue;
        };
        if entry.size() > 512 * 1024 {
            continue;
        }
        let mut bytes = Vec::new();
        if entry.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
            continue;
        }
        use base64::Engine;
        return Some(format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        ));
    }
    None
}

pub struct JarFacts {
    pub display_name: String,
    pub version: String,
    pub metadata: String,
    pub listed_name: String,
    pub client_only: bool,
    pub icon: Option<String>,
}

pub fn read_jar(path: &Path, kind: &str) -> JarFacts {
    let fallback = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_string();
    let blank = |metadata: &str| JarFacts {
        display_name: fallback.clone(),
        version: String::new(),
        metadata: metadata.into(),
        listed_name: fallback.clone(),
        client_only: false,
        icon: None,
    };
    let Ok(file) = fs::File::open(path) else {
        return blank("Could not read metadata");
    };
    let Ok(mut zip) = ZipArchive::new(file) else {
        return blank("Not a jar");
    };
    let found = read_descriptors(&mut zip);
    let (_, display_name, version) = identity_from(&found, &fallback);

    let (listed_name, metadata) = if kind == "plugin" {
        match &found.plugin_yml {
            Some((name, text)) => (
                yml_value(text, "name").unwrap_or_else(|| fallback.clone()),
                (*name).to_string(),
            ),
            None => (fallback.clone(), "No metadata".into()),
        }
    } else if let Some(text) = &found.fabric {
        let name = serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|value| {
                value
                    .get("name")
                    .or_else(|| value.get("id"))
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| fallback.clone());
        (name, "fabric.mod.json".into())
    } else if let Some((name, text)) = &found.forge {
        let picked = mod_from_toml(text)
            .map(|(_, shown, _)| shown)
            .filter(|shown| !shown.is_empty())
            .unwrap_or_else(|| fallback.clone());
        (picked, (*name).to_string())
    } else {
        (fallback.clone(), "No metadata".into())
    };

    let client_only = found
        .fabric
        .as_deref()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .and_then(|value| {
            value
                .get("environment")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .is_some_and(|environment| environment.eq_ignore_ascii_case("client"));

    let icon = icon_from(&mut zip, &found);
    JarFacts {
        display_name,
        version,
        metadata,
        listed_name,
        client_only,
        icon,
    }
}

pub fn jar_identity(path: &Path) -> (String, String, String) {
    let fallback = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("unknown")
        .to_string();
    let Ok(file) = fs::File::open(path) else {
        return (fallback.clone(), fallback, String::new());
    };
    let Ok(mut zip) = ZipArchive::new(file) else {
        return (fallback.clone(), fallback, String::new());
    };
    let found = read_descriptors(&mut zip);
    identity_from(&found, &fallback)
}

pub fn identity_of_bytes(bytes: &[u8], fallback: &str) -> Option<(String, String, String)> {
    let mut zip = ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let found = read_descriptors(&mut zip);
    if found.fabric.is_none() && found.forge.is_none() && found.plugin_yml.is_none() {
        return None;
    }
    Some(identity_from(&found, fallback))
}

fn mod_from_toml(text: &str) -> Option<(String, String, String)> {
    let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    let mods = doc.get("mods")?.as_array_of_tables()?;
    let first = mods.get(0)?;
    let read = |key: &str| {
        first
            .get(key)
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string()
    };
    Some((read("modId"), read("displayName"), read("version")))
}

pub fn scan(instance_path: &Path) -> Vec<InstalledAddon> {
    let mut found = Vec::new();
    for folder in ["mods", "plugins"] {
        let Ok(entries) = fs::read_dir(instance_path.join(folder)) else {
            continue;
        };
        let mut jars = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();
            let enabled = file_name.ends_with(".jar");
            if !enabled && !file_name.ends_with(".jar.disabled") {
                continue;
            }
            jars.push((path, file_name, enabled));
        }
        found.extend(
            crate::in_parallel(jars, |(path, file_name, enabled)| {
                let (mod_id, display_name, version) = jar_identity(&path);
                let sha512 = sha512_of(&path).ok()?;
                Some(InstalledAddon {
                    file_name,
                    mod_id,
                    display_name,
                    version,
                    enabled,
                    sha512,
                    project_id: None,
                    project_title: None,
                    loaders: Vec::new(),
                    game_versions: Vec::new(),
                })
            })
            .into_iter()
            .flatten(),
        );
    }
    found.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
    });
    found
}

pub fn identify(
    client: &reqwest::blocking::Client,
    addons: &mut [InstalledAddon],
) -> Result<(), String> {
    let hashes = addons
        .iter()
        .map(|addon| addon.sha512.clone())
        .collect::<Vec<_>>();
    if hashes.is_empty() {
        return Ok(());
    }
    let value: serde_json::Value = client
        .post("https://api.modrinth.com/v2/version_files")
        .json(&serde_json::json!({ "hashes": hashes, "algorithm": "sha512" }))
        .send()
        .map_err(|error| format!("Modrinth lookup failed: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    let Some(map) = value.as_object() else {
        return Ok(());
    };
    let project_ids = map
        .values()
        .filter_map(|version| version.get("project_id").and_then(|id| id.as_str()))
        .collect::<HashSet<_>>();
    let titles = fetch_projects(client, &project_ids).unwrap_or_default();

    for addon in addons.iter_mut() {
        let Some(version) = map.get(&addon.sha512) else {
            continue;
        };
        let project_id = version
            .get("project_id")
            .and_then(|id| id.as_str())
            .map(str::to_owned);
        addon.project_title = project_id
            .as_ref()
            .and_then(|id| titles.get(id))
            .map(|brief| brief.title.clone());
        addon.project_id = project_id;
        let list = |key: &str| {
            version
                .get(key)
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str())
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        addon.loaders = list("loaders");
        addon.game_versions = list("game_versions");
        if addon.version.is_empty() {
            addon.version = version
                .get("version_number")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct ProjectCard {
    pub title: String,
    pub icon: Option<String>,
    pub server_side: String,
}

pub fn fetch_project_cards(
    client: &reqwest::blocking::Client,
    ids: &HashSet<&str>,
) -> Result<HashMap<String, ProjectCard>, String> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let list = serde_json::to_string(&ids.iter().collect::<Vec<_>>())
        .map_err(|error| error.to_string())?;
    let value: serde_json::Value = client
        .get("https://api.modrinth.com/v2/projects")
        .query(&[("ids", list)])
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|project| {
            let id = project.get("id")?.as_str()?.to_string();
            let title = project.get("title")?.as_str()?.to_string();
            let icon = project
                .get("icon_url")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let side = |key: &str| {
                project
                    .get(key)
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown")
                    .to_string()
            };
            Some((
                id,
                ProjectCard {
                    title,
                    icon,
                    server_side: side("server_side"),
                },
            ))
        })
        .collect())
}

#[derive(Clone, Debug)]
pub struct ProjectBrief {
    pub title: String,
    pub icon_url: Option<String>,
}

fn fetch_projects(
    client: &reqwest::blocking::Client,
    ids: &HashSet<&str>,
) -> Result<HashMap<String, ProjectBrief>, String> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let list = serde_json::to_string(&ids.iter().collect::<Vec<_>>())
        .map_err(|error| error.to_string())?;
    let value: serde_json::Value = client
        .get("https://api.modrinth.com/v2/projects")
        .query(&[("ids", list)])
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|project| {
            Some((
                project.get("id")?.as_str()?.to_string(),
                ProjectBrief {
                    title: project.get("title")?.as_str()?.to_string(),
                    icon_url: project
                        .get("icon_url")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned),
                },
            ))
        })
        .collect())
}

pub fn find_updates(
    client: &reqwest::blocking::Client,
    addons: &[InstalledAddon],
    loader: &str,
    game_version: &str,
) -> Result<Vec<AddonUpdate>, String> {
    let known = addons
        .iter()
        .filter(|addon| addon.project_id.is_some())
        .collect::<Vec<_>>();
    if known.is_empty() {
        return Ok(Vec::new());
    }
    let hashes = known
        .iter()
        .map(|addon| addon.sha512.clone())
        .collect::<Vec<_>>();
    let value: serde_json::Value = client
        .post("https://api.modrinth.com/v2/version_files/update")
        .json(&serde_json::json!({
            "hashes": hashes,
            "algorithm": "sha512",
            "loaders": [loader],
            "game_versions": [game_version],
        }))
        .send()
        .map_err(|error| format!("Update check failed: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    let Some(map) = value.as_object() else {
        return Ok(Vec::new());
    };
    let installed: HashSet<String> = known.iter().filter(|addon| addon.enabled).filter_map(|addon| addon.project_id.clone()).collect();
    let unnamed: Vec<String> = addons
        .iter()
        .filter(|addon| addon.enabled && addon.project_id.is_none())
        .flat_map(|addon| [addon.mod_id.to_lowercase(), addon.file_name.to_lowercase()])
        .filter(|one| !one.is_empty())
        .collect();
    let loose: Vec<String> = known
        .iter()
        .filter_map(|addon| map.get(&addon.sha512))
        .flat_map(|latest| latest.get("dependencies").and_then(|value| value.as_array()).cloned().unwrap_or_default())
        .filter(|one| one.get("project_id").and_then(|value| value.as_str()).is_none())
        .filter_map(|one| one.get("version_id").and_then(|value| value.as_str()).map(String::from))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let mut projects_of_versions: HashMap<String, String> = HashMap::new();
    if !loose.is_empty() {
        if let Ok(found) = client
            .get("https://api.modrinth.com/v2/versions")
            .query(&[("ids", serde_json::to_string(&loose).unwrap_or_default())])
            .send()
            .and_then(|answer| answer.json::<serde_json::Value>())
        {
            for one in found.as_array().into_iter().flatten() {
                if let (Some(id), Some(project)) = (one.get("id").and_then(|v| v.as_str()), one.get("project_id").and_then(|v| v.as_str())) {
                    projects_of_versions.insert(id.to_string(), project.to_string());
                }
            }
        }
    }
    let mut updates = Vec::new();
    let mut raw_issues: Vec<Vec<(String, String)>> = Vec::new();
    for addon in known {
        let Some(latest) = map.get(&addon.sha512) else {
            continue;
        };
        let Some(file) = primary_file(latest) else {
            continue;
        };
        if file.2 == addon.sha512 {
            continue;
        }
        updates.push(AddonUpdate {
            file_name: addon.file_name.clone(),
            display_name: addon
                .project_title
                .clone()
                .unwrap_or_else(|| addon.display_name.clone()),
            current_version: addon.version.clone(),
            new_version: latest
                .get("version_number")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string(),
            new_file_name: file.0,
            download_url: file.1,
            sha512: file.2,
            issues: Vec::new(),
        });
        raw_issues.push(dependency_issues(latest, &installed, &projects_of_versions));
    }
    let ids: Vec<String> = raw_issues.iter().flatten().map(|one| one.1.clone()).collect::<HashSet<_>>().into_iter().collect();
    let mut titles: HashMap<String, String> = HashMap::new();
    let mut slugs: HashMap<String, String> = HashMap::new();
    if !ids.is_empty() {
        if let Ok(found) = client
            .get("https://api.modrinth.com/v2/projects")
            .query(&[("ids", serde_json::to_string(&ids).unwrap_or_default())])
            .send()
            .and_then(|answer| answer.json::<serde_json::Value>())
        {
            for one in found.as_array().into_iter().flatten() {
                if let (Some(id), Some(title)) = (one.get("id").and_then(|v| v.as_str()), one.get("title").and_then(|v| v.as_str())) {
                    titles.insert(id.to_string(), title.to_string());
                }
                if let (Some(id), Some(slug)) = (one.get("id").and_then(|v| v.as_str()), one.get("slug").and_then(|v| v.as_str())) {
                    slugs.insert(id.to_string(), slug.to_lowercase());
                }
            }
        }
    }
    for (update, list) in updates.iter_mut().zip(raw_issues) {
        update.issues = list
            .into_iter()
            .filter(|(kind, id)| kind != "missing" || !slugs.get(id).is_some_and(|slug| covers(&unnamed, slug)))
            .map(|(kind, id)| UpdateIssue { kind, project: titles.get(&id).cloned().unwrap_or(id) })
            .collect();
    }
    Ok(updates)
}

fn covers(unnamed: &[String], slug: &str) -> bool {
    let plain = slug.replace('-', "");
    unnamed.iter().any(|one| one == slug || one.replace(['-', '_'], "") == plain || (one.ends_with(".jar") && one.replace(['-', '_'], "").contains(&plain)))
}

fn primary_file(version: &serde_json::Value) -> Option<(String, String, String)> {
    let files = version.get("files")?.as_array()?;
    let file = files
        .iter()
        .find(|file| {
            file.get("primary")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        })
        .or_else(|| files.first())?;
    let name = Path::new(file.get("filename")?.as_str()?)
        .file_name()?
        .to_str()?
        .to_string();
    Some((
        name,
        file.get("url")?.as_str()?.to_string(),
        file.pointer("/hashes/sha512")?.as_str()?.to_string(),
    ))
}

pub fn find_conflicts(addons: &[InstalledAddon]) -> Vec<AddonConflict> {
    let mut conflicts = Vec::new();
    let mut by_id: HashMap<&str, Vec<&InstalledAddon>> = HashMap::new();
    for addon in addons
        .iter()
        .filter(|addon| addon.enabled && !addon.mod_id.trim().is_empty())
    {
        by_id.entry(addon.mod_id.as_str()).or_default().push(addon);
    }
    for (mod_id, group) in &by_id {
        if group.len() > 1 {
            conflicts.push(AddonConflict {
                kind: "Duplicate".into(),
                message: format!(
                    "{mod_id} is enabled {} times. Keep only one.",
                    group.len()
                ),
                files: group
                    .iter()
                    .map(|addon| addon.file_name.clone())
                    .collect(),
            });
        }
    }
    let mut by_project: HashMap<&str, Vec<&InstalledAddon>> = HashMap::new();
    for addon in addons.iter().filter(|addon| addon.enabled) {
        if let Some(project) = addon.project_id.as_deref() {
            by_project.entry(project).or_default().push(addon);
        }
    }
    for group in by_project.values() {
        if group.len() > 1 && group.iter().map(|a| a.mod_id.as_str()).collect::<HashSet<_>>().len() == 1 {
            continue;
        }
        if group.len() > 1 {
            conflicts.push(AddonConflict {
                kind: "Duplicate".into(),
                message: format!(
                    "{} has {} enabled files.",
                    group[0]
                        .project_title
                        .clone()
                        .unwrap_or_else(|| group[0].display_name.clone()),
                    group.len()
                ),
                files: group.iter().map(|a| a.file_name.clone()).collect(),
            });
        }
    }
    conflicts.sort_by(|left, right| left.message.cmp(&right.message));
    conflicts.dedup_by(|left, right| left.message == right.message);
    conflicts
}

pub fn plan_install(
    client: &reqwest::blocking::Client,
    project_id: &str,
    loader: &str,
    game_version: &str,
    installed: &HashSet<String>,
) -> Result<InstallPlan, String> {
    let version = pick_version(client, project_id, loader, game_version)?;
    let (file_name, download_url, sha512) =
        primary_file(&version).ok_or("No release file was found.")?;
    let wanted: Vec<(String, bool)> = version
        .get("dependencies")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|dep| {
            let id = dep.get("project_id").and_then(|id| id.as_str())?;
            let kind = dep.get("dependency_type").and_then(|value| value.as_str()).unwrap_or("required");
            matches!(kind, "required" | "optional").then(|| (id.to_string(), kind == "required"))
        })
        .collect();
    let titles_needed: HashSet<&str> = wanted.iter().map(|(id, _)| id.as_str()).collect();
    let (titles, picked) = std::thread::scope(|scope| {
        let titles = scope.spawn(|| fetch_projects(client, &titles_needed).unwrap_or_default());
        let picks: Vec<_> = wanted
            .iter()
            .map(|(id, _)| scope.spawn(move || pick_version(client, id, loader, game_version).ok()))
            .collect();
        let picked: Vec<Option<serde_json::Value>> = picks.into_iter().map(|handle| handle.join().unwrap_or(None)).collect();
        (titles.join().unwrap_or_default(), picked)
    });

    let mut dependencies = Vec::new();
    for ((dep_project, required), dep_version) in wanted.iter().zip(picked) {
        let Some(dep_version) = dep_version else { continue };
        let Some((name, url, hash)) = primary_file(&dep_version) else { continue };
        let brief = titles.get(dep_project.as_str());
        dependencies.push(DependencyInfo {
            project_id: dep_project.clone(),
            title: brief.map(|one| one.title.clone()).unwrap_or_else(|| dep_project.clone()),
            icon_url: brief.and_then(|one| one.icon_url.clone()),
            version_name: dep_version
                .get("version_number")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string(),
            required: *required,
            already_installed: installed.contains(dep_project),
            file_name: name,
            download_url: url,
            sha512: hash,
        });
    }
    Ok(InstallPlan {
        title: version
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or(project_id)
            .to_string(),
        file_name,
        download_url,
        sha512,
        dependencies,
    })
}

fn pick_version(
    client: &reqwest::blocking::Client,
    project_id: &str,
    loader: &str,
    game_version: &str,
) -> Result<serde_json::Value, String> {
    let versions: serde_json::Value = client
        .get(format!(
            "https://api.modrinth.com/v2/project/{project_id}/version"
        ))
        .send()
        .map_err(|error| format!("Could not fetch the release list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    versions
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| {
                    let matches_game = item
                        .get("game_versions")
                        .and_then(|value| value.as_array())
                        .is_some_and(|list| {
                            list.iter().any(|value| value.as_str() == Some(game_version))
                        });
                    let matches_loader = item
                        .get("loaders")
                        .and_then(|value| value.as_array())
                        .is_some_and(|list| {
                            list.iter().any(|value| value.as_str() == Some(loader))
                        });
                    matches_game && matches_loader
                })
                .cloned()
        })
        .ok_or_else(|| {
            format!("No release matches {game_version} · {loader}.")
        })
}

pub fn download_url_for(sha512: &str) -> Option<String> {
    let client = client().ok()?;
    let value: serde_json::Value = client
        .get(format!(
            "https://api.modrinth.com/v2/version_file/{sha512}?algorithm=sha512"
        ))
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .ok()?;
    let (_, url, _) = primary_file(&value)?;
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::{dependency_issues, find_conflicts, mod_from_toml, primary_file, InstalledAddon};

    #[test]
    fn warns_about_missing_and_clashing_dependencies() {
        let version = serde_json::json!({ "dependencies": [
            { "project_id": "api", "dependency_type": "required" },
            { "project_id": "lib", "dependency_type": "required" },
            { "project_id": "bad", "dependency_type": "incompatible" },
            { "project_id": "opt", "dependency_type": "optional" },
        ]});
        let installed = ["lib", "bad"].into_iter().map(String::from).collect();
        assert_eq!(
            dependency_issues(&version, &installed, &std::collections::HashMap::new()),
            vec![("missing".to_string(), "api".to_string()), ("incompatible".to_string(), "bad".to_string())]
        );
        let by_version = serde_json::json!({ "dependencies": [{ "version_id": "v1", "project_id": null, "dependency_type": "required" }] });
        let projects = [("v1".to_string(), "api".to_string())].into_iter().collect();
        assert_eq!(dependency_issues(&by_version, &installed, &projects), vec![("missing".to_string(), "api".to_string())]);
        let unnamed = vec!["fabric-api".to_string(), "geckolib-fabric-1.21-4.7.jar".to_string()];
        assert!(super::covers(&unnamed, "fabric-api"));
        assert!(super::covers(&unnamed, "geckolib"));
        assert!(!super::covers(&unnamed, "cloth-config"));
    }

    fn addon(file: &str, id: &str, project: Option<&str>) -> InstalledAddon {
        InstalledAddon {
            file_name: file.into(),
            mod_id: id.into(),
            display_name: id.into(),
            version: "1.0".into(),
            enabled: true,
            sha512: file.into(),
            project_id: project.map(str::to_owned),
            project_title: None,
            loaders: Vec::new(),
            game_versions: Vec::new(),
        }
    }

    #[test]
    fn reads_the_mods_own_id_not_what_it_needs() {
        let text = r#"
modLoader="javafml"
loaderVersion="[1,)"
license="MIT"

[[dependencies.configurable]]
    modId="neoforge"
    type="required"
    versionRange="[21,)"

[[mods]]
modId="configurable"
version="3.5.2"
displayName="Configurable"
"#;
        assert_eq!(
            mod_from_toml(text),
            Some(("configurable".into(), "Configurable".into(), "3.5.2".into()))
        );
    }

    #[test]
    fn flags_the_same_mod_installed_twice() {
        let list = vec![
            addon("sodium-0.5.jar", "sodium", None),
            addon("sodium-0.6.jar", "sodium", None),
        ];
        let conflicts = find_conflicts(&list);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].files.len(), 2);
    }

    #[test]
    fn ignores_disabled_duplicates() {
        let mut list = vec![
            addon("sodium-0.5.jar", "sodium", None),
            addon("sodium-0.6.jar", "sodium", None),
        ];
        list[0].enabled = false;
        assert!(find_conflicts(&list).is_empty());
    }

    #[test]
    fn reads_the_primary_release_file() {
        let version = serde_json::json!({
            "files": [
                { "filename": "extra.jar", "url": "https://x/extra.jar", "primary": false,
                  "hashes": { "sha512": "aaa" } },
                { "filename": "main.jar", "url": "https://x/main.jar", "primary": true,
                  "hashes": { "sha512": "bbb" } }
            ]
        });
        let (name, url, hash) = primary_file(&version).expect("should find the file");
        assert_eq!(name, "main.jar");
        assert_eq!(url, "https://x/main.jar");
        assert_eq!(hash, "bbb");
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn jar(tag: &str, inside: &str, body: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("ident-{}-{tag}.jar", std::process::id()));
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(inside, SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
        zip.finish().unwrap();
        path
    }

    #[test]
    fn knows_two_builds_are_the_same_plugin() {
        for inside in ["plugin.yml", "paper-plugin.yml"] {
            let old = jar("old", inside, "name: OurGuild\nversion: 1.0.0\n");
            let new = jar("new", inside, "name: OurGuild\nversion: 1.1.0\n");
            let (old_id, _, old_version) = jar_identity(&old);
            let (new_id, name, new_version) = jar_identity(&new);
            assert_eq!(old_id, new_id, "{inside}: should be the same plugin");
            assert_eq!(name, "OurGuild");
            assert_eq!(old_version, "1.0.0");
            assert_eq!(new_version, "1.1.0");
            let _ = fs::remove_file(old);
            let _ = fs::remove_file(new);
        }
    }

    #[test]
    fn prefers_paper_plugin_yml_over_a_shaded_plugin_yml() {
        let path = std::env::temp_dir().join(format!("ident-{}-shaded.jar", std::process::id()));
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("plugin.yml", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"name: commonnetworking
version: 26.3-1.1.0
").unwrap();
        zip.start_file("paper-plugin.yml", SimpleFileOptions::default()).unwrap();
        zip.write_all(b"name: journeymap
version: 26.3-6.0.8
").unwrap();
        zip.finish().unwrap();
        let (id, name, version) = jar_identity(&path);
        assert_eq!(id, "journeymap");
        assert_eq!(name, "journeymap");
        assert_eq!(version, "26.3-6.0.8");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn falls_back_to_the_file_name_when_nothing_says_otherwise() {
        let one = jar("plain", "plain.txt", "x");
        let (id, _, version) = jar_identity(&one);
        assert!(id.starts_with("ident-"), "{id}");
        assert!(version.is_empty());
        let _ = fs::remove_file(one);
    }

    #[test]
    fn reads_a_jar_held_in_memory() {
        let path = jar("bytes", "plugin.yml", "name: OurGuild
version: 2.0
");
        let bytes = fs::read(&path).unwrap();
        let _ = fs::remove_file(path);
        assert_eq!(
            identity_of_bytes(&bytes, "x"),
            Some(("OurGuild".into(), "OurGuild".into(), "2.0".into()))
        );
        assert_eq!(identity_of_bytes(b"not a zip", "x"), None);
    }
}

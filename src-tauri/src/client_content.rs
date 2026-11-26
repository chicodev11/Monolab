//! What is inside a client instance (the Library detail page): mods, resource packs and shader packs
//! in its game folder. Listing is local and quick; Modrinth names, icons and updates come in a
//! second call so the list shows at once. Turning off renames to `.disabled` (the way launchers
//! do), removing moves the file to `old/clients/<id>/`, and updating keeps the old file there too.

use crate::data_root::MonolabDirs;
use crate::{addons, client_launch, clients};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};
use zip::ZipArchive;

/// The folders the page shows, with the Modrinth loader their updates are looked up for
/// (mods use the instance's own loader).
const KINDS: &[(&str, &str)] = &[("mod", "mods"), ("resourcepack", "resourcepacks"), ("shader", "shaderpacks")];

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ClientContent {
    /// "mod", "resourcepack" or "shader".
    pub kind: String,
    pub file_name: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    /// A folder pack (unzipped) rather than a file.
    pub folder: bool,
    pub size: f64,
    /// The icon inside the file, as a data URL.
    pub icon: Option<String>,
}

/// What Modrinth knows about one file.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ClientContentCard {
    pub kind: String,
    pub file_name: String,
    pub project_id: String,
    pub title: String,
    pub icon_url: Option<String>,
    /// The newer version for this instance's Minecraft version and loader, empty when up to date.
    pub new_version: String,
}

fn folder_of(kind: &str) -> Result<&'static str, String> {
    KINDS.iter().find(|(one, _)| *one == kind).map(|(_, folder)| *folder).ok_or_else(|| "Unknown content kind.".to_string())
}

/// The Modrinth loader whose versions fit this kind in this instance.
fn modrinth_loader(kind: &str, loader: &str) -> Option<String> {
    match kind {
        "mod" => match loader {
            "Vanilla" | "Nova" => None,
            other => Some(other.to_ascii_lowercase()),
        },
        "resourcepack" => Some("minecraft".into()),
        "shader" => Some("iris".into()),
        _ => None,
    }
}

/// One plain file name inside the kind's folder; no paths, no tricks.
fn checked_name(name: &str) -> Result<&str, String> {
    let bad =
        name.is_empty() || name.len() > 255 || name.contains(['/', '\\', ':']) || name == "." || name == ".." || name.starts_with('.');
    if bad {
        Err("That file name is not allowed.".into())
    } else {
        Ok(name)
    }
}

fn shown_extension(kind: &str) -> &'static str {
    if kind == "mod" {
        ".jar"
    } else {
        ".zip"
    }
}

fn is_running(id: &str) -> bool {
    client_launch::client_runs().iter().any(|run| run.id == id)
}

fn old_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("old").join("clients").join(id);
    fs::create_dir_all(&dir).map_err(|error| format!("Could not create the folder for removed files: {error}"))?;
    Ok(dir)
}

fn put_aside(app: &AppHandle, id: &str, path: &Path, file_name: &str) -> Result<(), String> {
    let target = old_dir(app, id)?.join(format!("{}-{file_name}", crate::unix_now()));
    crate::move_file(path, &target).map_err(|error| format!("Could not move {file_name} aside: {error}"))
}

/// pack.png of a resource or shader pack, zipped or a folder.
fn pack_icon(path: &Path, folder: bool) -> Option<String> {
    let bytes = if folder {
        let file = path.join("pack.png");
        (fs::metadata(&file).ok()?.len() <= 512 * 1024).then(|| fs::read(&file).ok())??
    } else {
        let mut zip = ZipArchive::new(fs::File::open(path).ok()?).ok()?;
        let mut entry = zip.by_name("pack.png").ok()?;
        if entry.size() > 512 * 1024 {
            return None;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).ok()?;
        bytes
    };
    use base64::Engine;
    (!bytes.is_empty()).then(|| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

fn scan_kind(game: &Path, kind: &str, folder_name: &str) -> Vec<ClientContent> {
    let Ok(entries) = fs::read_dir(game.join(folder_name)) else {
        return Vec::new();
    };
    let extension = shown_extension(kind);
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let folder = path.is_dir();
        // Folder packs exist for resource and shader packs only.
        if folder && kind == "mod" {
            continue;
        }
        let enabled = !file_name.ends_with(".disabled");
        let plain = file_name.trim_end_matches(".disabled");
        if !folder && !plain.to_ascii_lowercase().ends_with(extension) {
            continue;
        }
        found.push((path, file_name.clone(), plain.to_string(), enabled, folder));
    }
    crate::in_parallel(found, |(path, file_name, plain, enabled, folder)| {
        let size = if folder { 0.0 } else { fs::metadata(&path).map(|meta| meta.len() as f64).unwrap_or(0.0) };
        let stem = plain.strip_suffix(extension).unwrap_or(&plain).to_string();
        let (name, version, icon) = if kind == "mod" {
            let facts = addons::read_jar(&path, "mod");
            (facts.listed_name, facts.version, facts.icon)
        } else {
            (stem, String::new(), pack_icon(&path, folder))
        };
        ClientContent { kind: kind.to_string(), file_name, name, version, enabled, folder, size, icon }
    })
}

#[tauri::command(async)]
#[specta::specta]
pub fn client_content(app: AppHandle, id: String) -> Result<Vec<ClientContent>, String> {
    clients::find(&app, &id)?;
    let game = clients::game_dir(&app, &id)?;
    let mut list: Vec<ClientContent> = KINDS.iter().flat_map(|(kind, folder)| scan_kind(&game, kind, folder)).collect();
    list.sort_by_key(|one| one.name.to_lowercase());
    Ok(list)
}

/// Hashes the files of one kind as the shape `addons` works with.
fn hashed(game: &Path, kind: &str) -> Vec<addons::InstalledAddon> {
    let Ok(folder) = folder_of(kind) else { return Vec::new() };
    let Ok(entries) = fs::read_dir(game.join(folder)) else {
        return Vec::new();
    };
    let extension = shown_extension(kind);
    let files: Vec<(PathBuf, String)> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| (entry.path(), entry.file_name().to_string_lossy().to_string()))
        .filter(|(_, name)| !name.starts_with('.') && name.trim_end_matches(".disabled").to_ascii_lowercase().ends_with(extension))
        .collect();
    crate::in_parallel(files, |(path, file_name)| {
        let sha512 = addons::sha512_of(&path).ok()?;
        let (mod_id, display_name, version) = if kind == "mod" { addons::jar_identity(&path) } else { Default::default() };
        Some(addons::InstalledAddon {
            enabled: !file_name.ends_with(".disabled"),
            file_name,
            mod_id,
            display_name,
            version,
            sha512,
            project_id: None,
            project_title: None,
            loaders: Vec::new(),
            game_versions: Vec::new(),
        })
    })
    .into_iter()
    .flatten()
    .collect()
}

/// Modrinth names, icons and updates for everything in the instance. Files Modrinth doesn't know
/// are left out (the page keeps what the file itself says).
#[tauri::command(async)]
#[specta::specta]
pub fn client_content_cards(app: AppHandle, id: String) -> Result<Vec<ClientContentCard>, String> {
    let instance = clients::find(&app, &id)?;
    let game = clients::game_dir(&app, &id)?;
    crate::off_runtime(move || {
        let client = addons::client()?;
        let mut cards = Vec::new();
        for (kind, _) in KINDS {
            let mut found = hashed(&game, kind);
            if found.is_empty() {
                continue;
            }
            addons::identify(&client, &mut found)?;
            let ids: HashSet<&str> = found.iter().filter_map(|one| one.project_id.as_deref()).collect();
            let projects = addons::fetch_project_cards(&client, &ids).unwrap_or_default();
            let updates: HashMap<String, String> = modrinth_loader(kind, &instance.loader)
                .and_then(|loader| addons::find_updates(&client, &found, &loader, &instance.version).ok())
                .unwrap_or_default()
                .into_iter()
                .map(|update| (update.file_name, update.new_version))
                .collect();
            for one in &found {
                let Some(project) = one.project_id.as_deref() else { continue };
                let card = projects.get(project);
                cards.push(ClientContentCard {
                    kind: (*kind).to_string(),
                    file_name: one.file_name.clone(),
                    project_id: project.to_string(),
                    title: card
                        .map(|card| card.title.clone())
                        .or_else(|| one.project_title.clone())
                        .unwrap_or_else(|| one.display_name.clone()),
                    icon_url: card.and_then(|card| card.icon.clone()),
                    new_version: updates.get(&one.file_name).cloned().unwrap_or_default(),
                });
            }
        }
        Ok(cards)
    })
}

#[tauri::command(async)]
#[specta::specta]
pub fn set_client_content_enabled(app: AppHandle, id: String, kind: String, file_name: String, enabled: bool) -> Result<String, String> {
    clients::find(&app, &id)?;
    if is_running(&id) {
        return Err("Close the game first.".into());
    }
    let folder = clients::game_dir(&app, &id)?.join(folder_of(&kind)?);
    let name = checked_name(&file_name)?;
    let plain = name.trim_end_matches(".disabled");
    let next = if enabled { plain.to_string() } else { format!("{plain}.disabled") };
    if next == name {
        return Ok(next);
    }
    if folder.join(&next).exists() {
        return Err(format!("{next} already exists."));
    }
    fs::rename(folder.join(name), folder.join(&next)).map_err(|error| format!("Could not rename {name}: {error}"))?;
    Ok(next)
}

#[tauri::command(async)]
#[specta::specta]
pub fn remove_client_content(app: AppHandle, id: String, kind: String, file_names: Vec<String>) -> Result<u32, String> {
    clients::find(&app, &id)?;
    if is_running(&id) {
        return Err("Close the game first.".into());
    }
    let folder = clients::game_dir(&app, &id)?.join(folder_of(&kind)?);
    let mut moved = 0;
    for file_name in &file_names {
        let name = checked_name(file_name)?;
        let path = folder.join(name);
        if !path.exists() {
            continue;
        }
        put_aside(&app, &id, &path, name)?;
        moved += 1;
    }
    Ok(moved)
}

/// Copies files the user picked into the kind's folder. App only: the paths are on this computer.
#[tauri::command(async)]
#[specta::specta]
pub fn add_client_content(app: AppHandle, id: String, kind: String, paths: Vec<String>) -> Result<u32, String> {
    clients::find(&app, &id)?;
    let folder = clients::game_dir(&app, &id)?.join(folder_of(&kind)?);
    fs::create_dir_all(&folder).map_err(|error| format!("Could not create the folder: {error}"))?;
    let extension = shown_extension(&kind);
    let mut added = 0;
    for source in &paths {
        let source = Path::new(source);
        let Some(name) = source.file_name().and_then(|name| name.to_str()) else { continue };
        let name = checked_name(name)?;
        if !source.is_file() || !name.to_ascii_lowercase().ends_with(extension) {
            return Err(format!("{name} is not a {extension} file."));
        }
        fs::copy(source, folder.join(name)).map_err(|error| format!("Could not copy {name}: {error}"))?;
        added += 1;
    }
    Ok(added)
}

/// Downloads the newer versions of the named files; the old files go to `old/clients/<id>/`.
#[tauri::command(async)]
#[specta::specta]
pub fn update_client_content(app: AppHandle, id: String, kind: String, file_names: Vec<String>) -> Result<u32, String> {
    let instance = clients::find(&app, &id)?;
    if is_running(&id) {
        return Err("Close the game first.".into());
    }
    let game = clients::game_dir(&app, &id)?;
    let folder = game.join(folder_of(&kind)?);
    let Some(loader) = modrinth_loader(&kind, &instance.loader) else {
        return Ok(0);
    };
    crate::off_runtime(move || {
        let client = addons::client()?;
        let mut found = hashed(&game, &kind);
        addons::identify(&client, &mut found)?;
        let updates = addons::find_updates(&client, &found, &loader, &instance.version)?;
        let mut done = 0;
        for update in updates.into_iter().filter(|update| file_names.contains(&update.file_name)) {
            let current = folder.join(checked_name(&update.file_name)?);
            let new_name = checked_name(&update.new_file_name)?.to_string();
            // A file that was off stays off after the update.
            let target = if update.file_name.ends_with(".disabled") { format!("{new_name}.disabled") } else { new_name };
            let staging = folder.join(format!(".{target}.download"));
            crate::download_file(&client, &update.download_url, &staging)?;
            if let Err(error) = crate::hashes::verify_sha512(&staging, &update.sha512) {
                let _ = fs::remove_file(&staging);
                return Err(error);
            }
            put_aside(&app, &id, &current, &update.file_name)?;
            fs::rename(&staging, folder.join(&target)).map_err(|error| format!("Could not install {target}: {error}"))?;
            done += 1;
        }
        Ok(done)
    })
}

#[tauri::command(async)]
#[specta::specta]
pub fn open_client_content_folder(app: AppHandle, id: String, kind: String) -> Result<(), String> {
    clients::find(&app, &id)?;
    let folder = clients::game_dir(&app, &id)?.join(folder_of(&kind)?);
    fs::create_dir_all(&folder).map_err(|error| format!("Could not create the folder: {error}"))?;
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let program = "xdg-open";
    crate::hidden_command(program).arg(&folder).spawn().map_err(|error| format!("Could not open the folder: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_inside_the_folder() {
        assert!(checked_name("sodium.jar").is_ok());
        assert!(checked_name("sodium.jar.disabled").is_ok());
        for bad in ["", "..", "../x.jar", "a/b.jar", "a\\b.jar", "C:x.jar", ".hidden.jar"] {
            assert!(checked_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn loaders_follow_the_kind() {
        assert_eq!(modrinth_loader("mod", "Fabric").as_deref(), Some("fabric"));
        assert_eq!(modrinth_loader("mod", "NeoForge").as_deref(), Some("neoforge"));
        assert_eq!(modrinth_loader("mod", "Vanilla"), None);
        assert_eq!(modrinth_loader("resourcepack", "Vanilla").as_deref(), Some("minecraft"));
        assert_eq!(modrinth_loader("shader", "Fabric").as_deref(), Some("iris"));
        assert!(folder_of("world").is_err());
    }

    #[test]
    fn scan_reads_mods_packs_and_off_files() {
        let game = std::env::temp_dir().join(format!("monolab-client-content-{}", std::process::id()));
        let _ = fs::remove_dir_all(&game);
        fs::create_dir_all(game.join("mods")).unwrap();
        fs::create_dir_all(game.join("resourcepacks/Folder Pack")).unwrap();
        fs::write(game.join("mods/a.jar"), b"not a zip").unwrap();
        fs::write(game.join("mods/b.jar.disabled"), b"not a zip").unwrap();
        fs::write(game.join("mods/notes.txt"), b"").unwrap();
        fs::write(game.join("resourcepacks/Faithful.zip"), b"").unwrap();
        let mods = scan_kind(&game, "mod", "mods");
        assert_eq!(mods.len(), 2);
        assert!(mods.iter().any(|one| one.file_name == "b.jar.disabled" && !one.enabled));
        let packs = scan_kind(&game, "resourcepack", "resourcepacks");
        assert_eq!(packs.len(), 2);
        assert!(packs.iter().any(|one| one.folder && one.name == "Folder Pack"));
        assert!(packs.iter().any(|one| one.name == "Faithful"));
        let _ = fs::remove_dir_all(&game);
    }
}

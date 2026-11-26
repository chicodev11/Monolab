//! Game client instances for the Home page: a name, a Minecraft version and a
//! mod loader, each with its own game folder under the app data folder.
//! Installing and launching live in `client_launch`.

use crate::data_root::MonolabDirs;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Manager};

pub const LOADERS: &[&str] = &["Vanilla", "Fabric", "Quilt", "Nova", "NeoForge", "Forge"];

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ClientInstance {
    pub id: String,
    pub name: String,
    pub version: String,
    pub loader: String,
    #[serde(default)]
    pub group: String,
    pub created_at: u64,
    /// Unix seconds of the last launch, 0 when never played.
    #[serde(default)]
    pub last_played: u64,
}

static LOCK: Mutex<()> = Mutex::new(());

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| format!("Could not create the app data folder: {error}"))?;
    Ok(dir)
}

fn list_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("clients.json"))
}

/// The instance's game folder (saves, mods, resource packs, options).
pub fn game_dir(app: &AppHandle, id: &str) -> Result<PathBuf, String> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("That instance was not found.".into());
    }
    Ok(data_dir(app)?.join("clients").join(id))
}

fn load(app: &AppHandle) -> Result<Vec<ClientInstance>, String> {
    Ok(crate::store::read(&list_path(app)?)?.unwrap_or_default())
}

fn save(app: &AppHandle, list: &[ClientInstance]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(list).map_err(|error| error.to_string())?;
    crate::store::write(&list_path(app)?, &text).map_err(|error| format!("Could not save the instance list: {error}"))
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Please enter a name.".into());
    }
    if name.chars().count() > 64 {
        return Err("The name can be at most 64 characters.".into());
    }
    Ok(name.to_string())
}

fn new_id() -> Result<String, String> {
    let mut bytes = [0u8; 6];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(format!("c-{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

pub fn find(app: &AppHandle, id: &str) -> Result<ClientInstance, String> {
    load(app)?.into_iter().find(|one| one.id == id).ok_or_else(|| "That instance was not found.".into())
}

/// Records a launch so the instance moves to the top of Jump in.
pub fn mark_played(app: &AppHandle, id: &str) {
    let Ok(_guard) = LOCK.lock() else { return };
    let Ok(mut list) = load(app) else { return };
    if let Some(one) = list.iter_mut().find(|one| one.id == id) {
        one.last_played = crate::unix_now();
        let _ = save(app, &list);
    }
}

/// Client instances, most recently played (then created) first.
#[tauri::command(async)]
#[specta::specta]
pub fn list_clients(app: AppHandle) -> Result<Vec<ClientInstance>, String> {
    let mut list = load(&app)?;
    list.sort_by_key(|one| std::cmp::Reverse(one.last_played.max(one.created_at)));
    Ok(list)
}

#[tauri::command(async)]
#[specta::specta]
pub fn create_client(app: AppHandle, name: String, version: String, loader: String) -> Result<ClientInstance, String> {
    let name = clean_name(&name)?;
    let version = version.trim().to_string();
    if version.is_empty() || version.len() > 64 || !version.chars().all(|c| c.is_ascii_graphic()) {
        return Err("Please choose a Minecraft version.".into());
    }
    if !LOADERS.contains(&loader.as_str()) {
        return Err("Unknown mod loader.".into());
    }
    // Nova is Minecraft rebuilt in Rust for one version only.
    let version = if loader == "Nova" { crate::forks::NOVA_VERSION.to_string() } else { version };
    let _guard = LOCK.lock().map_err(|_| "The instance list is busy.".to_string())?;
    let mut list = load(&app)?;
    let one = ClientInstance { id: new_id()?, name, version, loader, group: String::new(), created_at: crate::unix_now(), last_played: 0 };
    fs::create_dir_all(game_dir(&app, &one.id)?).map_err(|error| format!("Could not create the instance folder: {error}"))?;
    list.push(one.clone());
    save(&app, &list)?;
    Ok(one)
}

#[tauri::command(async)]
#[specta::specta]
pub fn rename_client(app: AppHandle, id: String, name: String) -> Result<(), String> {
    let name = clean_name(&name)?;
    let _guard = LOCK.lock().map_err(|_| "The instance list is busy.".to_string())?;
    let mut list = load(&app)?;
    let one = list.iter_mut().find(|one| one.id == id).ok_or("That instance was not found.")?;
    one.name = name;
    save(&app, &list)
}

/// Removes the instance and its game folder (worlds included).
#[tauri::command(async)]
#[specta::specta]
pub fn delete_client(app: AppHandle, id: String) -> Result<(), String> {
    let _guard = LOCK.lock().map_err(|_| "The instance list is busy.".to_string())?;
    let mut list = load(&app)?;
    let before = list.len();
    list.retain(|one| one.id != id);
    if list.len() == before {
        return Err("That instance was not found.".into());
    }
    let dir = game_dir(&app, &id)?;
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|error| format!("Could not delete the instance folder: {error}"))?;
    }
    save(&app, &list)
}

#[tauri::command(async)]
#[specta::specta]
pub fn open_client_folder(app: AppHandle, id: String) -> Result<(), String> {
    let dir = game_dir(&app, &id)?;
    fs::create_dir_all(&dir).map_err(|error| format!("Could not create the instance folder: {error}"))?;
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let program = "xdg-open";
    crate::hidden_command(program).arg(&dir).spawn().map_err(|error| format!("Could not open the folder: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(clean_name("  My pack ").unwrap(), "My pack");
        assert!(clean_name("   ").is_err());
        assert!(clean_name(&"x".repeat(65)).is_err());
    }

    #[test]
    fn ids_are_folder_safe() {
        let id = new_id().unwrap();
        assert!(id.starts_with("c-") && id.len() == 14);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn old_files_without_new_fields_still_load() {
        let one: ClientInstance =
            serde_json::from_str(r#"{"id":"c-1","name":"A","version":"1.21.5","loader":"Fabric","createdAt":5}"#).unwrap();
        assert_eq!(one.last_played, 0);
        assert!(one.group.is_empty());
    }
}

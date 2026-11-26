use crate::data_root::MonolabDirs;
use crate::off_runtime;
use std::{collections::HashMap, fs, path::PathBuf};
use tauri::{AppHandle, Manager};

fn cache_path(app: &AppHandle, version: &str) -> Option<PathBuf> {
    let dir = app.path().monolab_data_dir().ok()?.join("lang");
    fs::create_dir_all(&dir).ok()?;
    for old in ["advancements-ko", "lang-ko", "lang-ko-v2", "lang-ko-v3"] {
        let _ = fs::remove_file(dir.join(format!("{old}-{version}.json")));
    }
    Some(dir.join(format!("lang-en-{version}.json")))
}

fn worth_keeping(key: &str) -> bool {
    key.starts_with("item.minecraft.") || key.starts_with("block.minecraft.") || key.starts_with("enchantment.minecraft.")
}

fn item_keys(id: &str) -> [String; 2] {
    let bare = id.strip_prefix("minecraft:").unwrap_or(id);
    [format!("item.minecraft.{bare}"), format!("block.minecraft.{bare}")]
}

pub fn table(app: &AppHandle, version: &str) -> HashMap<String, String> {
    let path = cache_path(app, version);
    if let Some(path) = &path {
        if let Ok(text) = fs::read_to_string(path) {
            if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&text) {
                return map;
            }
        }
    }
    let map = fetch(app, version).unwrap_or_default();
    if !map.is_empty() {
        if let Some(path) = &path {
            if let Ok(text) = serde_json::to_string(&map) {
                let _ = fs::write(path, text);
            }
        }
    }
    map
}

/// English names come from assets/minecraft/lang/en_us.json inside the client jar.
fn fetch(app: &AppHandle, version: &str) -> Result<HashMap<String, String>, String> {
    let jar = crate::textures::client_jar(app, version)?;
    let file = fs::File::open(&jar).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let mut entry =
        archive.by_name("assets/minecraft/lang/en_us.json").map_err(|_| "This version has no English language file.".to_string())?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut entry, &mut text).map_err(|error| error.to_string())?;
    let table: HashMap<String, serde_json::Value> =
        serde_json::from_str(&text).map_err(|_| "The language file has an unexpected shape.".to_string())?;
    Ok(table
        .into_iter()
        .filter(|(key, _)| worth_keeping(key))
        .filter_map(|(key, value)| Some((key, value.as_str()?.to_string())))
        .collect())
}

#[tauri::command(async)]
#[specta::specta]
pub fn item_names(app: AppHandle, version: String, ids: Vec<String>) -> Result<HashMap<String, String>, String> {
    let table = off_runtime(|| table(&app, &version));
    if table.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(ids
        .into_iter()
        .filter_map(|id| {
            let name = item_keys(&id).into_iter().find_map(|key| table.get(&key))?.clone();
            Some((id, name))
        })
        .collect())
}

#[tauri::command(async)]
#[specta::specta]
pub fn enchantment_names(app: AppHandle, version: String, ids: Vec<String>) -> Result<HashMap<String, String>, String> {
    let table = off_runtime(|| table(&app, &version));
    Ok(ids
        .into_iter()
        .filter_map(|id| {
            let bare = id.strip_prefix("minecraft:")?;
            let name = table.get(&format!("enchantment.minecraft.{bare}"))?.clone();
            Some((id, name))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_in_both_item_and_block() {
        assert_eq!(
            item_keys("minecraft:netherite_sword"),
            ["item.minecraft.netherite_sword".to_string(), "block.minecraft.netherite_sword".to_string()]
        );
        assert_eq!(item_keys("stone")[0], "item.minecraft.stone");
    }

    /// Don't keep 8,557 lines we won't use.
    #[test]
    fn keeps_only_what_we_show() {
        assert!(worth_keeping("item.minecraft.netherite_sword"));
        assert!(worth_keeping("block.minecraft.diamond_block"));
        assert!(worth_keeping("enchantment.minecraft.sharpness"));
        assert!(!worth_keeping("gui.done"));
        assert!(!worth_keeping("advancements.story.mine_stone.title"));
        assert!(!worth_keeping("entity.minecraft.creeper"));
        assert!(!worth_keeping("death.attack.fall"));
    }
}

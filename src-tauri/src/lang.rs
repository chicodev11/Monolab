
use crate::off_runtime;
use std::{collections::HashMap, fs, path::PathBuf};
use tauri::{AppHandle, Manager};

fn cache_path(app: &AppHandle, version: &str) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?.join("lang");
    fs::create_dir_all(&dir).ok()?;
    let _ = fs::remove_file(dir.join(format!("advancements-ko-{version}.json")));
    let _ = fs::remove_file(dir.join(format!("lang-ko-{version}.json")));
    Some(dir.join(format!("lang-ko-v2-{version}.json")))
}

fn worth_keeping(key: &str) -> bool {
    key.starts_with("item.minecraft.") || key.starts_with("block.minecraft.")
}

fn item_keys(id: &str) -> [String; 2] {
    let bare = id.strip_prefix("minecraft:").unwrap_or(id);
    [
        format!("item.minecraft.{bare}"),
        format!("block.minecraft.{bare}"),
    ]
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
    let map = fetch(version).unwrap_or_default();
    if !map.is_empty() {
        if let Some(path) = &path {
            if let Ok(text) = serde_json::to_string(&map) {
                let _ = fs::write(path, text);
            }
        }
    }
    map
}

fn fetch(version: &str) -> Result<HashMap<String, String>, String> {
    let client = crate::addons::client()?;
    let get = |url: &str| -> Result<serde_json::Value, String> {
        client
            .get(url)
            .send()
            .map_err(|error| error.to_string())?
            .json::<serde_json::Value>()
            .map_err(|error| error.to_string())
    };

    let manifest = get("https://launchermeta.mojang.com/mc/game/version_manifest_v2.json")?;
    let entry = manifest
        .get("versions")
        .and_then(|value| value.as_array())
        .ok_or("Could not read the version list.")?
        .iter()
        .find(|one| one.get("id").and_then(|value| value.as_str()) == Some(version))
        .ok_or("This version is not in Mojang's list.")?;
    let url = entry
        .get("url")
        .and_then(|value| value.as_str())
        .ok_or("The version has no URL.")?;

    let detail = get(url)?;
    let index = detail
        .get("assetIndex")
        .and_then(|value| value.get("url"))
        .and_then(|value| value.as_str())
        .ok_or("The asset index has no URL.")?;

    let assets = get(index)?;
    let hash = assets
        .get("objects")
        .and_then(|value| value.get("minecraft/lang/ko_kr.json"))
        .and_then(|value| value.get("hash"))
        .and_then(|value| value.as_str())
        .ok_or("This version has no Korean language file.")?
        .to_string();

    let head = hash.get(..2).ok_or("The hash is too short.")?;
    let lang = get(&format!(
        "https://resources.download.minecraft.net/{head}/{hash}"
    ))?;

    let table = lang.as_object().ok_or("The language file has an unexpected shape.")?;
    Ok(table
        .iter()
        .filter(|(key, _)| worth_keeping(key))
        .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
        .collect())
}

#[tauri::command(async)]
#[specta::specta]
pub fn item_names(
    app: AppHandle,
    version: String,
    ids: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    let table = off_runtime(|| table(&app, &version));
    if table.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(ids
        .into_iter()
        .filter_map(|id| {
            let korean = item_keys(&id)
                .into_iter()
                .find_map(|key| table.get(&key))?
                .clone();
            Some((id, korean))
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
            [
                "item.minecraft.netherite_sword".to_string(),
                "block.minecraft.netherite_sword".to_string()
            ]
        );
        assert_eq!(item_keys("stone")[0], "item.minecraft.stone");
    }

    /// Don't keep 8,557 lines we won't use.
    #[test]
    fn keeps_only_what_we_show() {
        assert!(worth_keeping("item.minecraft.netherite_sword"));
        assert!(worth_keeping("block.minecraft.diamond_block"));
        assert!(!worth_keeping("gui.done"));
        assert!(!worth_keeping("advancements.story.mine_stone.title"));
        assert!(!worth_keeping("entity.minecraft.creeper"));
        assert!(!worth_keeping("death.attack.fall"));
    }

    #[test]
    #[ignore]
    fn pulls_real_korean_names() {
        let table = fetch("1.21.4").expect("could not download");
        let count = |prefix: &str, suffix: &str| {
            table
                .keys()
                .filter(|key| key.starts_with(prefix) && key.ends_with(suffix))
                .count()
        };
        println!("{} filtered", table.len());
        println!("  items {}", count("item.minecraft.", ""));
        println!("  blocks {}", count("block.minecraft.", ""));
        println!("  sample: {}", table["item.minecraft.netherite_sword"]);
        assert!(table.len() > 2000, "too few were filtered");
    }
}

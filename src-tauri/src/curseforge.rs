//! FTB modpacks from CurseForge. CurseForge's API needs a key of the user's own (console.curseforge.com);
//! it is sealed on this PC like the cloud backup secret and never shown again. Mods whose authors
//! block third-party downloads stay on the pack's download list.

use crate::catalog::CatalogHit;
use crate::data_root::MonolabDirs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub const API: &str = "https://api.curseforge.com/v1";
const MINECRAFT: u32 = 432;
const MODPACKS: u32 = 4471;

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    key: String,
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().monolab_data_dir().map_err(|error| error.to_string())?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir.join("curseforge.json"))
}

fn key(app: &AppHandle) -> Option<String> {
    let saved: Saved = path(app).ok().and_then(|path| crate::store::read_secret(&path).ok().flatten())?;
    Some(saved.key).filter(|key| !key.is_empty())
}

pub fn has_key(app: &AppHandle) -> bool {
    key(app).is_some()
}

/// Saves the key, or forgets it with an empty one.
pub fn set_key(app: &AppHandle, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.chars().any(|letter| letter.is_control() || letter == ' ') || key.len() > 200 {
        return Err("That doesn't look like a CurseForge API key.".into());
    }
    let path = path(app)?;
    if key.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(());
    }
    let text = serde_json::to_string(&Saved { key: key.into() }).map_err(|error| error.to_string())?;
    crate::store::write_secret(&path, &text).map_err(|error| format!("Could not save the key: {error}"))
}

fn client() -> Result<reqwest::blocking::Client, String> {
    crate::net::builder().user_agent(crate::net::USER_AGENT).timeout(Duration::from_secs(120)).build().map_err(|error| error.to_string())
}

fn get(app: &AppHandle, url: &str) -> Result<Value, String> {
    let key = key(app).ok_or("Add a CurseForge API key first.")?;
    let answer = client()?.get(url).header("x-api-key", key).send().map_err(|error| format!("Could not reach CurseForge: {error}"))?;
    if answer.status().as_u16() == 403 || answer.status().as_u16() == 401 {
        return Err("CurseForge refused the API key. Check it in the FTB tab.".into());
    }
    answer.error_for_status().and_then(|answer| answer.json()).map_err(|error| format!("CurseForge answered with an error: {error}"))
}

fn post(app: &AppHandle, url: &str, body: &Value) -> Result<Value, String> {
    let key = key(app).ok_or("Add a CurseForge API key first.")?;
    client()?
        .post(url)
        .header("x-api-key", key)
        .json(body)
        .send()
        .and_then(|answer| answer.error_for_status())
        .and_then(|answer| answer.json())
        .map_err(|error| format!("Could not reach CurseForge: {error}"))
}

/// Whether a search result is one of FTB's own packs.
pub fn is_ftb(project: &Value) -> bool {
    let name = project.get("name").and_then(Value::as_str).unwrap_or_default();
    let by_ftb = project.get("authors").and_then(Value::as_array).is_some_and(|authors| {
        authors.iter().any(|one| {
            one.get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name.eq_ignore_ascii_case("ftb") || name.eq_ignore_ascii_case("ftbteam"))
        })
    });
    by_ftb || name.starts_with("FTB ")
}

pub fn hit(project: &Value) -> Option<CatalogHit> {
    let text = |key: &str| project.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    let id = project.get("id").and_then(Value::as_u64)?;
    let mut versions: Vec<String> = project
        .get("latestFilesIndexes")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|one| one.get("gameVersion").and_then(Value::as_str).map(str::to_string)).collect())
        .unwrap_or_default();
    versions.dedup();
    Some(CatalogHit {
        source: "curseforge".into(),
        project_id: id.to_string(),
        title: text("name"),
        description: text("summary"),
        project_type: "modpack".into(),
        installable: true,
        page_url: project.pointer("/links/websiteUrl").and_then(Value::as_str).unwrap_or_default().to_string(),
        icon_url: project.pointer("/logo/thumbnailUrl").and_then(Value::as_str).map(str::to_string),
        author: project.pointer("/authors/0/name").and_then(Value::as_str).map(str::to_string),
        downloads: project.get("downloadCount").and_then(Value::as_f64).map(|count| count as u64),
        follows: None,
        updated_at: None,
        loaders: Vec::new(),
        versions,
    })
}

/// FTB's packs matching `query`, the most downloaded first.
pub fn search(app: &AppHandle, query: &str) -> Result<Vec<CatalogHit>, String> {
    let mut url = reqwest::Url::parse(&format!("{API}/mods/search")).map_err(|error| error.to_string())?;
    let filter = format!("FTB {}", query.trim());
    url.query_pairs_mut()
        .append_pair("gameId", &MINECRAFT.to_string())
        .append_pair("classId", &MODPACKS.to_string())
        .append_pair("searchFilter", filter.trim())
        .append_pair("sortField", "6")
        .append_pair("sortOrder", "desc")
        .append_pair("pageSize", "50");
    let found = get(app, url.as_str())?;
    Ok(found
        .get("data")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter(|one| is_ftb(one)).filter_map(hit).collect())
        .unwrap_or_default())
}

/// A file name CurseForge gave, without folders or odd characters.
fn safe_name(name: &str) -> String {
    let name: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|letter| !letter.is_control() && !r#"<>:"|?*"#.contains(*letter))
        .collect();
    let name = name.trim().trim_start_matches('.').to_string();
    if name.is_empty() {
        "download".into()
    } else {
        name
    }
}

fn sha1_of(file: &Value) -> Option<String> {
    file.get("hashes")?
        .as_array()?
        .iter()
        .find(|one| one.get("algo").and_then(Value::as_u64) == Some(1))?
        .get("value")?
        .as_str()
        .map(str::to_string)
}

/// Downloads the pack's newest file (the client pack: its manifest lists the mods) and returns where it is.
pub fn download_pack(app: &AppHandle, project: u64) -> Result<PathBuf, String> {
    let info = get(app, &format!("{API}/mods/{project}"))?;
    let main = info.pointer("/data/mainFileId").and_then(Value::as_u64).ok_or("This pack has no file to download.")?;
    let file = get(app, &format!("{API}/mods/{project}/files/{main}"))?;
    let file = file.get("data").ok_or("CurseForge sent no file.")?;
    let url = file
        .get("downloadUrl")
        .and_then(Value::as_str)
        .ok_or("The pack's author doesn't allow downloads outside CurseForge. Download it from the CurseForge site and import the .zip.")?;
    let name = safe_name(file.get("fileName").and_then(Value::as_str).unwrap_or("pack.zip"));
    let folder = std::env::temp_dir().join("monolab-curseforge");
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let target = folder.join(if name.ends_with(".zip") { name } else { format!("{name}.zip") });
    crate::download_file_watched(&client()?, url, &target, &mut |_, _| {})?;
    if let Some(sha1) = sha1_of(file) {
        crate::hashes::verify_sha1(&target, &sha1)?;
    }
    Ok(target)
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FetchedMods {
    pub fetched: usize,
    /// Mods CurseForge won't hand out; they stay on the download list.
    pub left: usize,
    pub client_only: usize,
}

/// A file marked for the client only (CurseForge tags files "Client" / "Server").
pub fn client_only(file: &Value) -> bool {
    let tags: Vec<&str> =
        file.get("gameVersions").and_then(Value::as_array).map(|list| list.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    tags.contains(&"Client") && !tags.contains(&"Server")
}

/// Puts the pack's mods into `mods/` where CurseForge allows it. `folder` is the server folder.
pub fn fetch_mods(app: &AppHandle, files: &[u64], folder: &Path) -> Result<FetchedMods, String> {
    let mods = folder.join("mods");
    fs::create_dir_all(&mods).map_err(|error| error.to_string())?;
    let client = client()?;
    let mut done = FetchedMods { fetched: 0, left: 0, client_only: 0 };
    for chunk in files.chunks(50) {
        let answer = post(app, &format!("{API}/mods/files"), &serde_json::json!({ "fileIds": chunk }))?;
        let list = answer.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
        done.left += chunk.len().saturating_sub(list.len());
        for file in list {
            if client_only(&file) {
                done.client_only += 1;
                continue;
            }
            let (Some(url), Some(name)) = (file.get("downloadUrl").and_then(Value::as_str), file.get("fileName").and_then(Value::as_str))
            else {
                done.left += 1;
                continue;
            };
            let name = safe_name(name);
            let target = mods.join(&name);
            if target.exists() {
                done.fetched += 1;
                continue;
            }
            let staging = mods.join(format!(".{name}.download"));
            let got = crate::download_file_watched(&client, url, &staging, &mut |_, _| {})
                .and_then(|()| sha1_of(&file).map_or(Ok(()), |sha1| crate::hashes::verify_sha1(&staging, &sha1)))
                .and_then(|()| fs::rename(&staging, &target).map_err(|error| error.to_string()));
            if got.is_ok() {
                done.fetched += 1;
            } else {
                let _ = fs::remove_file(&staging);
                done.left += 1;
            }
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ftb_packs_pass() {
        let ftb = serde_json::json!({ "id": 1, "name": "StoneBlock 3", "authors": [{ "name": "FTB" }] });
        let named = serde_json::json!({ "id": 2, "name": "FTB Skies", "authors": [{ "name": "someone" }] });
        let other = serde_json::json!({ "id": 3, "name": "All the Mods 9", "authors": [{ "name": "ATMTeam" }] });
        assert!(is_ftb(&ftb) && is_ftb(&named) && !is_ftb(&other));
    }

    #[test]
    fn hits_read_curseforge_projects() {
        let project = serde_json::json!({
            "id": 7, "name": "FTB Skies", "summary": "Sky", "downloadCount": 1234.0,
            "logo": { "thumbnailUrl": "https://x/y.png" }, "authors": [{ "name": "FTB" }],
            "latestFilesIndexes": [{ "gameVersion": "1.20.1" }, { "gameVersion": "1.20.1" }],
        });
        let hit = hit(&project).unwrap();
        assert_eq!(hit.project_id, "7");
        assert_eq!(hit.source, "curseforge");
        assert_eq!(hit.downloads, Some(1234));
        assert_eq!(hit.versions, vec!["1.20.1".to_string()]);
    }

    #[test]
    fn names_stay_in_the_folder() {
        assert_eq!(safe_name("../../evil.jar"), "evil.jar");
        assert_eq!(safe_name("a\\b.jar"), "b.jar");
        assert_eq!(safe_name(".."), "download");
    }

    #[test]
    fn client_files_are_left_out() {
        assert!(client_only(&serde_json::json!({ "gameVersions": ["1.20.1", "Client", "Forge"] })));
        assert!(!client_only(&serde_json::json!({ "gameVersions": ["1.20.1", "Client", "Server"] })));
        assert!(!client_only(&serde_json::json!({ "gameVersions": ["1.20.1"] })));
    }
}

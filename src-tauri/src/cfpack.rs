
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use zip::ZipArchive;

#[derive(Clone, Debug, Serialize, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    pub name: String,
    pub version: String,
    pub author: String,
    pub game_version: String,
    pub loader: String,
    pub loader_version: String,
    pub mods: Vec<ModRef>,
    pub override_count: usize,
    pub bundled_mods: usize,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModRef {
    pub project: u64,
    pub file: u64,
    pub required: bool,
    pub url: String,
}

pub fn read(path: &Path) -> Result<Pack, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|_| "Not a zip file.".to_string())?;

    let manifest: serde_json::Value = {
        let mut entry = archive
            .by_name("manifest.json")
            .map_err(|_| "Not a CurseForge modpack — manifest.json is missing.".to_string())?;
        let mut text = String::new();
        entry
            .read_to_string(&mut text)
            .map_err(|error| error.to_string())?;
        serde_json::from_str(&text).map_err(|_| "Could not read manifest.json.".to_string())?
    };

    let kind = manifest
        .get("manifestType")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if !kind.is_empty() && kind != "minecraftModpack" {
        return Err("This is a different kind of CurseForge bundle, not a modpack.".into());
    }

    let text = |key: &str| {
        manifest
            .get(key)
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string()
    };

    let minecraft = manifest.get("minecraft");
    let game_version = minecraft
        .and_then(|value| value.get("version"))
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let loaders = minecraft
        .and_then(|value| value.get("modLoaders"))
        .and_then(|value| value.as_array());
    let loader_id = loaders
        .and_then(|list| {
            list.iter()
                .find(|one| one.get("primary").and_then(|v| v.as_bool()) == Some(true))
                .or_else(|| list.first())
        })
        .and_then(|one| one.get("id"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let (loader, loader_version) = match loader_id.split_once('-') {
        Some((kind, version)) => (kind.to_string(), version.to_string()),
        None => (loader_id.to_string(), String::new()),
    };

    let mods = manifest
        .get("files")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|one| {
            let project = one.get("projectID")?.as_u64()?;
            let file = one.get("fileID")?.as_u64()?;
            Some(ModRef {
                project,
                file,
                required: one
                    .get("required")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(true),
                url: format!("https://www.curseforge.com/projects/{project}"),
            })
        })
        .collect();

    let folder = {
        let named = text("overrides");
        if named.is_empty() {
            "overrides".to_string()
        } else {
            named
        }
    };
    let (override_count, bundled_mods) = count_overrides(&mut archive, &folder);

    Ok(Pack {
        name: text("name"),
        version: text("version"),
        author: text("author"),
        game_version,
        loader,
        loader_version,
        mods,
        override_count,
        bundled_mods,
    })
}

fn count_overrides<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    folder: &str,
) -> (usize, usize) {
    let mut all = 0;
    let mut jars = 0;
    for index in 0..archive.len() {
        let Ok(entry) = archive.by_index(index) else {
            continue;
        };
        if entry.is_dir() {
            continue;
        }
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let Ok(inside) = name.strip_prefix(folder) else {
            continue;
        };
        all += 1;
        let looks_like_a_mod = inside.starts_with("mods")
            && inside
                .extension()
                .is_some_and(|one| one.eq_ignore_ascii_case("jar"));
        if looks_like_a_mod {
            jars += 1;
        }
    }
    (all, jars)
}

pub fn extract_overrides(pack: &Path, destination: &Path) -> Result<usize, String> {
    let file = std::fs::File::open(pack).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
    crate::zips::check_budget(&mut archive, crate::zips::MAX_UNPACKED, crate::zips::MAX_ENTRIES)?;

    let folder = {
        let mut named = "overrides".to_string();
        if let Ok(mut entry) = archive.by_name("manifest.json") {
            let mut text = String::new();
            if entry.read_to_string(&mut text).is_ok() {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(one) = value.get("overrides").and_then(|one| one.as_str()) {
                        if !one.is_empty() {
                            named = one.to_string();
                        }
                    }
                }
            }
        }
        named
    };

    let mut written = 0_usize;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let Ok(inside) = name.strip_prefix(&folder) else {
            continue;
        };
        if inside.as_os_str().is_empty() {
            continue;
        }
        let output = destination.join(inside);
        if !output.starts_with(destination) {
            return Err("A modpack path points outside the server folder.".into());
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&output).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut target = std::fs::File::create(&output).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut target).map_err(|error| error.to_string())?;
        written += 1;
    }
    Ok(written)
}

pub fn write_list(pack: &Pack, folder: &Path) -> Result<PathBuf, String> {
    let path = folder.join("modpack-download-list.txt");
    let mut text = String::new();
    text.push_str(&format!("{} {}\n", pack.name, pack.version));
    text.push_str(&format!(
        "Minecraft {} · {} {}\n\n",
        pack.game_version, pack.loader, pack.loader_version
    ));
    text.push_str(
        "Download each file below and put it in the mods folder.\nCurseForge lets authors block third-party downloads, so the app doesn't fetch them for you.\n\n",
    );
    for one in &pack.mods {
        text.push_str(&format!(
            "{}{}\n",
            one.url,
            if one.required { "" } else { "  (optional)" }
        ));
    }
    std::fs::write(&path, text).map_err(|error| format!("Could not write the list: {error}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn make(tag: &str, manifest: &str, extras: &[(&str, &str)]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("cfpack-{}-{tag}.zip", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(manifest.as_bytes()).unwrap();
        for (name, body) in extras {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    const REAL: &str = r#"{
      "minecraft": {
        "version": "1.20.1",
        "modLoaders": [
          {"id": "forge-47.2.0", "primary": true}
        ]
      },
      "manifestType": "minecraftModpack",
      "manifestVersion": 1,
      "name": "All the Mods 9",
      "version": "0.2.60",
      "author": "ATMTeam",
      "files": [
        {"projectID": 238222, "fileID": 4585321, "required": true},
        {"projectID": 306612, "fileID": 5063689, "required": false}
      ],
      "overrides": "overrides"
    }"#;

    #[test]
    fn unpacks_the_config_bundle() {
        let path = make(
            "extract",
            REAL,
            &[
                ("overrides/config/jei/jei.ini", "hide=false"),
                ("overrides/server.properties", "motd=x"),
                ("overrides/mods/local.jar", "jar"),
                ("modlist.html", "<ul></ul>"),
            ],
        );
        let out = std::env::temp_dir().join(format!("cfout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();

        let written = extract_overrides(&path, &out).unwrap();
        assert_eq!(written, 3, "only files inside overrides should be extracted");
        assert!(out.join("config/jei/jei.ini").is_file());
        assert!(out.join("mods/local.jar").is_file());
        assert!(!out.join("modlist.html").exists());
        assert!(!out.join("manifest.json").exists());

        let pack = read(&path).unwrap();
        let list = write_list(&pack, &out).unwrap();
        let text = std::fs::read_to_string(&list).unwrap();
        assert!(text.contains("https://www.curseforge.com/projects/238222"), "{text}");
        assert!(text.contains("(optional)"), "optional ones should be marked: {text}");

        let _ = std::fs::remove_dir_all(&out);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn refuses_paths_that_climb_out() {
        let path = make("climb", REAL, &[("overrides/../../outside.txt", "bad")]);
        let out = std::env::temp_dir().join(format!("cfclimb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        let _ = extract_overrides(&path, &out);
        assert!(!out.parent().unwrap().join("outside.txt").exists());
        let _ = std::fs::remove_dir_all(&out);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn reads_a_curseforge_pack() {
        let path = make(
            "real",
            REAL,
            &[
                ("overrides/config/jei.toml", "x"),
                ("overrides/mods/local.jar", "x"),
            ],
        );
        let pack = read(&path).unwrap();
        assert_eq!(pack.name, "All the Mods 9");
        assert_eq!(pack.game_version, "1.20.1");
        assert_eq!(pack.loader, "forge");
        assert_eq!(pack.loader_version, "47.2.0");
        assert_eq!(pack.mods.len(), 2);
        assert!(pack.mods[0].required);
        assert!(!pack.mods[1].required);
        assert_eq!(pack.mods[0].url, "https://www.curseforge.com/projects/238222");
        assert_eq!(pack.override_count, 2);
        assert_eq!(pack.bundled_mods, 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn refuses_things_that_are_not_modpacks(){
        let path = make("resource", "{\"manifestType\":\"resourcePack\",\"name\":\"something\"}", &[]);
        assert!(read(&path).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn refuses_a_zip_without_a_manifest() {
        let path = make_plain();
        let failed = read(&path).unwrap_err();
        assert!(failed.contains("manifest.json"), "{failed}");
        let _ = std::fs::remove_file(path);
    }

    fn make_plain() -> PathBuf {
        let path = std::env::temp_dir().join(format!("cfpack-{}-plain.zip", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("modrinth.index.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"{}").unwrap();
        zip.finish().unwrap();
        path
    }
}

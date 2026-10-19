
use serde::Serialize;
use std::{
    fs,
    io::{Read, Seek, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

pub const FOLDER: &str = ".monolab-backups";

const SKIP: [&str; 2] = [FOLDER, ".monolab-trash"];

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub file_name: String,
    pub size_mib: f64,
    pub created_at: u64,
    pub note: String,
    pub worlds_only: bool,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}

pub fn world_dirs(server: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(server) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let keep = path.join("level.dat").is_file()
                || name.ends_with("_nether")
                || name.ends_with("_the_end");
            keep.then_some(name)
        })
        .collect()
}

pub fn create(
    server: &Path,
    note: &str,
    vault: &Path,
    worlds_only: bool,
) -> Result<BackupFile, String> {
    let note = if worlds_only {
        format!("{note} worlds only")
    } else {
        note.to_string()
    };
    let note = note.as_str();
    let only = worlds_only.then(|| world_dirs(server));
    if let Some(list) = &only {
        if list.is_empty() {
            return Err("No world folder to back up. Start the server once.".into());
        }
    }
    let out = vault;
    fs::create_dir_all(out)
        .map_err(|error| format!("Could not create the backup folder: {error}"))?;
    let created_at = unix_now();
    let file_name = format!("{created_at}-{}.zip", safe(note));
    let path = out.join(&file_name);

    let staging = out.join(format!(".{file_name}.making"));
    let file = fs::File::create(&staging)
        .map_err(|error| format!("Could not create the backup file: {error}"))?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    if let Err(error) = pack(&mut writer, server, server, options, only.as_deref()) {
        let _ = writer.finish();
        let _ = fs::remove_file(&staging);
        return Err(error);
    }
    writer
        .finish()
        .map_err(|error| format!("Could not finish the backup: {error}"))?;
    fs::rename(&staging, &path)
        .map_err(|error| format!("Could not move the backup file: {error}"))?;

    let size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
    Ok(BackupFile {
        file_name,
        size_mib: (size as f64) / 1024.0 / 1024.0,
        created_at,
        note: note.to_string(),
        worlds_only,
    })
}

fn safe(note: &str) -> String {
    let cleaned: String = note
        .chars()
        .map(|letter| {
            if letter.is_alphanumeric() || letter == '-' {
                letter
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "backup".into()
    } else {
        cleaned.chars().take(40).collect()
    }
}

fn pack<W: Write + Seek>(
    writer: &mut ZipWriter<W>,
    root: &Path,
    here: &Path,
    options: SimpleFileOptions,
    only: Option<&[String]>,
) -> Result<(), String> {
    let entries = fs::read_dir(here).map_err(|error| format!("{}: {error}", here.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        if here == root {
            if let Some(list) = only {
                if !path.is_dir() || !list.iter().any(|one| one == &name) {
                    continue;
                }
            }
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let inside = relative.to_string_lossy().replace('\\', "/");
        if path.is_dir() {
            writer
                .add_directory(format!("{inside}/"), options)
                .map_err(|error| error.to_string())?;
            pack(writer, root, &path, options, only)?;
            continue;
        }
        let Ok(mut source) = fs::File::open(&path) else {
            continue;
        };
        writer
            .start_file(inside, options)
            .map_err(|error| error.to_string())?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = match source.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => read,
                Err(_) => break,
            };
            writer
                .write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

pub fn list(vault: &Path) -> Vec<BackupFile> {
    let Ok(entries) = fs::read_dir(vault) else {
        return Vec::new();
    };
    let mut found: Vec<BackupFile> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let rest = name.strip_suffix(".zip")?;
            let (stamp, note) = rest.split_once('-')?;
            Some(BackupFile {
                size_mib: (entry.metadata().ok()?.len() as f64) / 1024.0 / 1024.0,
                created_at: stamp.parse().ok()?,
                worlds_only: note.ends_with("worlds only"),
                note: note.replace('_', " "),
                file_name: name,
            })
        })
        .collect();
    found.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    found
}

pub fn remove(vault: &Path, file_name: &str) -> Result<(), String> {
    let path = pick(vault, file_name)?;
    fs::remove_file(path).map_err(|error| format!("Could not delete the backup: {error}"))
}

pub fn sweep(vault: &Path, days: u32, keep_auto: u32) {
    let cutoff = unix_now().saturating_sub(u64::from(days) * 24 * 60 * 60);
    let mut autos = 0;
    let mut all = list(vault);
    all.sort_by_key(|one| std::cmp::Reverse(one.created_at));
    for one in all {
        let too_old = days > 0 && one.created_at < cutoff;
        let auto = one.note.starts_with("Auto");
        if auto {
            autos += 1;
        }
        let too_many = auto && keep_auto > 0 && autos > keep_auto;
        if too_old || too_many {
            let _ = remove(vault, &one.file_name);
        }
    }
}

pub fn restore(server: &Path, file_name: &str, vault: &Path) -> Result<BackupFile, String> {
    let path = pick(vault, file_name)?;
    let safety = create(server, "Before rollback", vault, false)?;
    let file = fs::File::open(&path).map_err(|error| format!("Could not open the backup: {error}"))?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| format!("The backup is corrupted: {error}"))?;
    for at in 0..archive.len() {
        let mut item = archive
            .by_index(at)
            .map_err(|error| format!("Could not read the backup: {error}"))?;
        let Some(inside) = item.enclosed_name() else {
            continue;
        };
        if inside.components().any(|part| part == Component::ParentDir) {
            continue;
        }
        let target = server.join(&inside);
        if item.is_dir() {
            let _ = fs::create_dir_all(&target);
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut out = fs::File::create(&target)
            .map_err(|error| format!("Could not roll back {}: {error}", inside.display()))?;
        std::io::copy(&mut item, &mut out)
            .map_err(|error| format!("Could not roll back {}: {error}", inside.display()))?;
    }
    Ok(safety)
}

fn pick(vault: &Path, file_name: &str) -> Result<PathBuf, String> {
    if file_name.contains(['/', '\\']) {
        return Err("The backup name is malformed.".into());
    }
    let path = crate::paths::resolve_in_server(vault, file_name).map_err(|error| {
        match error {
            crate::paths::PathError::Missing => "That backup was not found.".to_string(),
            _ => "The backup name is malformed.".to_string(),
        }
    })?;
    if !path.is_file() {
        return Err("That backup was not found.".into());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_newest_auto_backups() {
        let vault = std::env::temp_dir().join(format!("se-bk-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&vault);
        std::fs::create_dir_all(&vault).unwrap();
        let now = unix_now();
        for at in 0..5 {
            std::fs::write(vault.join(format!("{}-Auto.zip", now - at * 60)), b"x").unwrap();
        }
        std::fs::write(vault.join(format!("{}-Auto_worlds_only.zip", now - 600)), b"x").unwrap();
        std::fs::write(vault.join(format!("{}-Before_upgrade.zip", now - 9000)), b"x").unwrap();
        sweep(&vault, 0, 2);
        let left: Vec<String> = list(&vault).into_iter().map(|one| one.note).collect();
        assert_eq!(left, vec!["Auto", "Auto", "Before upgrade"]);
        sweep(&vault, 0, 0);
        assert_eq!(list(&vault).len(), 3);
        let _ = std::fs::remove_dir_all(&vault);
    }

    #[test]
    fn packs_only_the_worlds_and_puts_them_where_told() {
        let home = std::env::temp_dir().join(format!("se-bk-{}", std::process::id()));
        let server = home.join("server_1");
        let away = home.join("somewhere");
        let make = |path: std::path::PathBuf| std::fs::create_dir_all(&path).unwrap();

        make(server.join("world"));
        std::fs::write(server.join("world").join("level.dat"), b"x").unwrap();
        std::fs::write(server.join("world").join("region.mca"), b"x").unwrap();
        make(server.join("world_nether/DIM-1"));
        std::fs::write(server.join("world_nether/DIM-1/r.mca"), b"x").unwrap();
        make(server.join("mods"));
        std::fs::write(server.join("mods").join("big.jar"), vec![0_u8; 4096]).unwrap();
        std::fs::write(server.join("server.properties"), b"x").unwrap();
        make(away.clone());

        let found = world_dirs(&server);
        assert!(found.iter().any(|one| one == "world"), "{found:?}");
        assert!(found.iter().any(|one| one == "world_nether"), "{found:?}");
        assert!(!found.iter().any(|one| one == "mods"), "{found:?}");

        let made = create(&server, "Auto", &away, true).unwrap();
        assert!(made.worlds_only);
        assert!(made.note.ends_with("worlds only"), "{}", made.note);
        assert!(!server.join(FOLDER).exists(), "a backup was created in the server folder");
        let kept = away.clone();
        assert!(kept.join(&made.file_name).is_file(), "did not land elsewhere");

        let zip = fs::File::open(kept.join(&made.file_name)).unwrap();
        let mut archive = ZipArchive::new(zip).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|at| archive.by_index(at).unwrap().name().unwrap().to_string())
            .collect();
        assert!(names.iter().any(|one| one.contains("world/level.dat")), "{names:?}");
        assert!(names.iter().any(|one| one.contains("world_nether/")), "{names:?}");
        assert!(
            !names.iter().any(|one| one.contains("mods/")),
            "mods were included: {names:?}"
        );
        assert!(
            !names.iter().any(|one| one.contains("server.properties")),
            "top-level files were included: {names:?}"
        );

        assert_eq!(list(&away).len(), 1);
        assert_eq!(list(&server.join(FOLDER)).len(), 0, "looking at the old place");

        let whole = create(&server, "Manual", &away, false).unwrap();
        assert!(!whole.worlds_only);
        let zip = fs::File::open(kept.join(&whole.file_name)).unwrap();
        let mut archive = ZipArchive::new(zip).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|at| archive.by_index(at).unwrap().name().unwrap().to_string())
            .collect();
        assert!(names.iter().any(|one| one.contains("mods/big.jar")), "{names:?}");

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn packs_a_folder_and_puts_it_back() {
        let stamp = unix_now();
        let root = std::env::temp_dir().join(format!("se-backup-test-{stamp}"));
        let vault = std::env::temp_dir().join(format!("se-backup-vault-{stamp}"));
        let world = root.join("world");
        fs::create_dir_all(&world).unwrap();
        fs::write(root.join("server.properties"), b"level-name=world\n").unwrap();
        fs::write(world.join("level.dat"), b"first").unwrap();
        fs::create_dir_all(root.join(FOLDER)).unwrap();
        fs::write(root.join(FOLDER).join("garbage.zip"), b"x").unwrap();

        let made = create(&root, "Manual", &vault, false).unwrap();
        assert!(made.file_name.ends_with(".zip"));
        assert_eq!(list(&vault).len(), 1);
        let zip = fs::File::open(vault.join(&made.file_name)).unwrap();
        let mut archive = ZipArchive::new(zip).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|at| archive.by_index(at).unwrap().name().unwrap().to_string())
            .collect();
        assert!(
            !names.iter().any(|one| one.contains("garbage.zip")),
            "old backups were included: {names:?}"
        );

        fs::write(world.join("level.dat"), b"second").unwrap();
        restore(&root, &made.file_name, &vault).unwrap();
        assert_eq!(fs::read(world.join("level.dat")).unwrap(), b"first");
        assert_eq!(list(&vault).len(), 2);

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&vault);
    }

    #[test]
    fn refuses_a_name_that_climbs_out() {
        let root = std::env::temp_dir().join("se-backup-escape");
        assert!(pick(&root, "../secret.zip").is_err());
        assert!(pick(&root, "a/b.zip").is_err());
    }
}

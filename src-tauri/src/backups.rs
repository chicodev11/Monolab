
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

/// What goes into a backup.
pub enum Pack<'a> {
    All,
    Worlds,
    Picked(&'a [String]),
}

pub fn create(
    server: &Path,
    note: &str,
    vault: &Path,
    worlds_only: bool,
) -> Result<BackupFile, String> {
    make(server, note, vault, if worlds_only { Pack::Worlds } else { Pack::All })
}

/// A backup of only the picked files and folders.
pub fn create_picked(server: &Path, note: &str, vault: &Path, picks: &[String]) -> Result<BackupFile, String> {
    make(server, note, vault, Pack::Picked(picks))
}

fn make(server: &Path, note: &str, vault: &Path, what: Pack) -> Result<BackupFile, String> {
    let worlds_only = matches!(what, Pack::Worlds);
    let note = if worlds_only {
        format!("{note} worlds only")
    } else if matches!(what, Pack::Picked(_)) {
        format!("{note} picked")
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
    let packed = match what {
        Pack::Picked(picks) => pack_picked(&mut writer, server, picks, options),
        _ => pack(&mut writer, server, server, options, only.as_deref()),
    };
    if let Err(error) = packed {
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
        add_file(writer, &path, inside, options)?;
    }
    Ok(())
}

fn add_file<W: Write + Seek>(writer: &mut ZipWriter<W>, path: &Path, inside: String, options: SimpleFileOptions) -> Result<(), String> {
    let Ok(mut source) = fs::File::open(path) else {
        return Ok(());
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
    Ok(())
}

fn pack_picked<W: Write + Seek>(writer: &mut ZipWriter<W>, server: &Path, picks: &[String], options: SimpleFileOptions) -> Result<(), String> {
    for one in picks {
        let path = server.join(one);
        if path.is_dir() {
            writer.add_directory(format!("{one}/"), options).map_err(|error| error.to_string())?;
            pack(writer, server, &path, options, None)?;
        } else if path.is_file() {
            add_file(writer, &path, one.clone(), options)?;
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
    if is_worlds_only(file_name) {
        // A world backup replaces those world folders whole, so chunks made after it don't linger.
        let mut tops: Vec<String> = archive.file_names().filter_map(|name| name.ok()?.split_once('/').map(|(top, _)| top.to_string())).collect();
        tops.sort();
        tops.dedup();
        for top in tops.iter().filter(|top| !top.is_empty() && *top != "." && *top != ".." && !top.contains('\\')) {
            let dir = server.join(top);
            if dir.is_dir() {
                fs::remove_dir_all(&dir).map_err(|error| format!("Could not roll back {top}: {error}"))?;
            }
        }
    }
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

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub path: String,
    pub size_bytes: u64,
    /// `Same`, `Changed`, `Removed` (only in the backup) or `Added` (only on the server now).
    pub state: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupContents {
    pub entries: Vec<BackupEntry>,
    /// False when files of the same size are taken as unchanged without reading them (SSH).
    pub exact: bool,
}

/// Lines up a backup's files with the server's: `same` settles files whose sizes match.
pub fn compare(
    then: Vec<(String, u64)>,
    mut now: std::collections::HashMap<String, u64>,
    worlds_only: bool,
    mut same: impl FnMut(&str, usize) -> bool,
) -> Vec<BackupEntry> {
    let top = |path: &str| path.split('/').next().unwrap_or(path).to_string();
    let tops: std::collections::HashSet<String> = then.iter().map(|(path, _)| top(path)).collect();
    let mut out = Vec::with_capacity(then.len());
    for (at, (path, size)) in then.into_iter().enumerate() {
        let state = match now.remove(&path) {
            None => "Removed",
            Some(here) if here != size => "Changed",
            Some(_) if same(&path, at) => "Same",
            Some(_) => "Changed",
        };
        out.push(BackupEntry { path, size_bytes: size, state: state.into() });
    }
    // A worlds-only backup says nothing about the rest of the server.
    for (path, size) in now {
        if !worlds_only || tops.contains(&top(&path)) {
            out.push(BackupEntry { path, size_bytes: size, state: "Added".into() });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

pub fn is_worlds_only(file_name: &str) -> bool {
    file_name.trim_end_matches(".zip").trim_end_matches(".tar.gz").ends_with("worlds_only")
}

/// Worlds-only and picked backups hold part of the server, so files outside them aren't "added since".
pub fn is_partial(file_name: &str) -> bool {
    is_worlds_only(file_name) || file_name.trim_end_matches(".zip").trim_end_matches(".tar.gz").ends_with("picked")
}

fn walk(root: &Path, here: &Path, out: &mut std::collections::HashMap<String, u64>) {
    let Ok(entries) = fs::read_dir(here) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if here == root && SKIP.contains(&name.as_str()) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out);
        } else if let (Ok(meta), Ok(relative)) = (entry.metadata(), path.strip_prefix(root)) {
            out.insert(relative.to_string_lossy().replace('\\', "/"), meta.len());
        }
    }
}

fn crc_of(path: &Path) -> Option<u32> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = crc32fast::Hasher::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => return Some(hasher.finalize()),
            Ok(read) => hasher.update(&buffer[..read]),
            Err(_) => return None,
        }
    }
}

fn open_archive(vault: &Path, file_name: &str) -> Result<ZipArchive<fs::File>, String> {
    let path = pick(vault, file_name)?;
    let file = fs::File::open(&path).map_err(|error| format!("Could not open the backup: {error}"))?;
    ZipArchive::new(file).map_err(|error| format!("The backup is corrupted: {error}"))
}

/// What a backup holds next to the server as it is now; same-size files are compared by checksum.
pub fn contents(server: &Path, vault: &Path, file_name: &str) -> Result<BackupContents, String> {
    let mut archive = open_archive(vault, file_name)?;
    let mut then = Vec::new();
    let mut sums = Vec::new();
    for at in 0..archive.len() {
        let item = archive.by_index_raw(at).map_err(|error| format!("Could not read the backup: {error}"))?;
        if item.is_dir() {
            continue;
        }
        let Some(inside) = item.enclosed_name() else { continue };
        then.push((inside.to_string_lossy().replace('\\', "/"), item.size()));
        sums.push(item.crc32());
    }
    let mut now = std::collections::HashMap::new();
    walk(server, server, &mut now);
    let entries = compare(then, now, is_partial(file_name), |path, at| crc_of(&server.join(path)) == Some(sums[at]));
    Ok(BackupContents { entries, exact: true })
}

/// One file from a backup as text, or None when it is large or binary.
pub fn entry_text(vault: &Path, file_name: &str, inside: &str) -> Result<Option<String>, String> {
    let mut archive = open_archive(vault, file_name)?;
    let mut item = match archive.by_name(inside) {
        Ok(item) => item,
        Err(_) => return Err("That file isn't in the backup.".into()),
    };
    if item.size() > TEXT_LIMIT {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    item.read_to_end(&mut bytes).map_err(|error| format!("Could not read the backup: {error}"))?;
    Ok(as_text(bytes))
}

pub const TEXT_LIMIT: u64 = 1024 * 1024;

pub fn as_text(bytes: Vec<u8>) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Checked relative paths with forward slashes, without duplicates or entries already covered by a picked folder.
pub fn clean_picks(paths: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for one in paths {
        let path = crate::paths::check_relative(one).map_err(|_| format!("Not a path inside the server: {one}"))?;
        out.push(path.to_string_lossy().replace('\\', "/"));
    }
    out.sort();
    out.dedup();
    let all = out.clone();
    out.retain(|one| !all.iter().any(|other| other != one && one.starts_with(&format!("{other}/"))));
    if out.is_empty() {
        return Err("Choose at least one file to restore.".into());
    }
    Ok(out)
}

pub fn picked(picks: &[String], inside: &str) -> bool {
    picks.iter().any(|one| inside == one || inside.starts_with(&format!("{one}/")))
}

/// Puts back only the picked files and folders; their current versions are backed up first.
pub fn restore_paths(server: &Path, file_name: &str, vault: &Path, picks: &[String]) -> Result<BackupFile, String> {
    let picks = clean_picks(picks)?;
    let mut archive = open_archive(vault, file_name)?;
    let safety = make(server, "Before partial rollback", vault, Pack::Picked(&picks))?;
    for at in 0..archive.len() {
        let mut item = archive.by_index(at).map_err(|error| format!("Could not read the backup: {error}"))?;
        let Some(inside) = item.enclosed_name() else { continue };
        let name = inside.to_string_lossy().replace('\\', "/");
        if !picked(&picks, name.trim_end_matches('/')) {
            continue;
        }
        let relative = crate::paths::check_relative(&name).map_err(|_| format!("Not a path inside the server: {name}"))?;
        if item.is_dir() {
            let _ = fs::create_dir_all(server.join(&relative));
            continue;
        }
        if let Some(parent) = server.join(&relative).parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let target = crate::paths::resolve_new_in_server(server, &name).map_err(|_| format!("Not a path inside the server: {name}"))?;
        let mut out = fs::File::create(&target).map_err(|error| format!("Could not roll back {name}: {error}"))?;
        std::io::copy(&mut item, &mut out).map_err(|error| format!("Could not roll back {name}: {error}"))?;
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

        std::fs::write(server.join("world").join("later.mca"), b"y").unwrap();
        std::fs::write(server.join("mods").join("new.jar"), b"y").unwrap();
        restore(&server, &made.file_name, &away).unwrap();
        assert!(server.join("world").join("level.dat").is_file());
        assert!(!server.join("world").join("later.mca").exists(), "a chunk made after the world backup stayed");
        assert!(server.join("mods").join("new.jar").is_file(), "a world restore touched mods");

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
    fn compares_and_puts_back_only_picked_files() {
        let stamp = unix_now();
        let root = std::env::temp_dir().join(format!("se-bk-pick-{stamp}"));
        let vault = std::env::temp_dir().join(format!("se-bk-pick-vault-{stamp}"));
        fs::create_dir_all(root.join("world/region")).unwrap();
        fs::write(root.join("server.properties"), b"pvp=true\n").unwrap();
        fs::write(root.join("world/level.dat"), b"one").unwrap();
        fs::write(root.join("world/region/r.0.0.mca"), b"abc").unwrap();
        fs::write(root.join("gone.txt"), b"bye").unwrap();
        let made = create(&root, "Manual", &vault, false).unwrap();

        fs::write(root.join("server.properties"), b"pvp=false\n").unwrap();
        fs::write(root.join("world/region/r.0.0.mca"), b"xyz").unwrap();
        fs::remove_file(root.join("gone.txt")).unwrap();
        fs::write(root.join("new.txt"), b"hi").unwrap();
        let seen = contents(&root, &vault, &made.file_name).unwrap();
        let state = |path: &str| seen.entries.iter().find(|one| one.path == path).map(|one| one.state.clone()).unwrap_or_default();
        assert_eq!(state("server.properties"), "Changed");
        assert_eq!(state("world/region/r.0.0.mca"), "Changed", "same size, other bytes");
        assert_eq!(state("world/level.dat"), "Same");
        assert_eq!(state("gone.txt"), "Removed");
        assert_eq!(state("new.txt"), "Added");
        assert_eq!(entry_text(&vault, &made.file_name, "server.properties").unwrap().as_deref(), Some("pvp=true\n"));

        std::thread::sleep(std::time::Duration::from_millis(1100));
        let safety = restore_paths(&root, &made.file_name, &vault, &["world".into(), "world/level.dat".into(), "gone.txt".into()]).unwrap();
        assert_eq!(fs::read(root.join("world/region/r.0.0.mca")).unwrap(), b"abc");
        assert_eq!(fs::read(root.join("gone.txt")).unwrap(), b"bye");
        assert_eq!(fs::read(root.join("server.properties")).unwrap(), b"pvp=false\n", "not picked");
        let kept = contents(&root, &vault, &safety.file_name).unwrap();
        assert!(kept.entries.iter().any(|one| one.path == "world/region/r.0.0.mca"), "{:?}", kept.entries);
        assert!(!kept.entries.iter().any(|one| one.path == "server.properties" && one.state != "Added"));
        assert!(restore_paths(&root, &made.file_name, &vault, &["../x".into()]).is_err());
        assert_eq!(clean_picks(&["a/b".into(), "a".into(), "a".into()]).unwrap(), vec!["a".to_string()]);

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

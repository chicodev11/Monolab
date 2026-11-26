//! The app's own JSON files: written so a crash or power cut mid-save never leaves half a file,
//! and read back from the last good copy when the file is damaged anyway.

use serde::de::DeserializeOwned;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

fn beside(path: &Path, tail: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(tail);
    path.with_file_name(name)
}

/// The app data folder: its files keep their backups in `backups/` instead of next to them.
static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

pub fn set_root(root: PathBuf) {
    let _ = ROOT.set(root);
}

/// Where the last good copy of a file goes.
fn backup_of(path: &Path) -> PathBuf {
    match (ROOT.get(), path.parent()) {
        (Some(root), Some(parent)) if parent == root => {
            let mut name = path.file_name().unwrap_or_default().to_os_string();
            name.push(".bak");
            root.join("backups").join(name)
        }
        _ => beside(path, ".bak"),
    }
}

/// The backup to read: the current place, else one left beside the file by an older version.
fn backup_to_read(path: &Path) -> PathBuf {
    let now = backup_of(path);
    if now.exists() { now } else { beside(path, ".bak") }
}

fn is_json(text: &str) -> bool {
    serde_json::from_str::<serde::de::IgnoredAny>(text).is_ok()
}

/// Writes next to the file and swaps it in. The copy being replaced is kept as `<name>.bak` while it was valid JSON.
pub fn write(path: &Path, text: &str) -> std::io::Result<()> {
    let temp = beside(path, ".tmp");
    {
        let mut file = fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    if fs::read_to_string(path).is_ok_and(|old| is_json(&old)) {
        let backup = backup_of(path);
        if let Some(folder) = backup.parent() {
            let _ = fs::create_dir_all(folder);
        }
        let _ = fs::copy(path, backup);
    }
    fs::rename(&temp, path)
}

/// `Ok(None)` when the file doesn't exist. A damaged file falls back to `<name>.bak` (and is kept as `<name>.broken-<time>`);
/// with no good copy it is an error, never an empty value, so a caller can't save over what was there.
pub fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return restore_backup(path, &name),
        Err(error) => return Err(format!("Could not read {name}: {error}")),
    };
    match serde_json::from_str(&text) {
        Ok(value) => Ok(Some(value)),
        Err(error) => {
            let backup = backup_to_read(path);
            let saved = fs::read_to_string(&backup).ok().and_then(|old| serde_json::from_str::<T>(&old).ok());
            match saved {
                Some(value) => {
                    set_aside(path);
                    let _ = fs::copy(&backup, path);
                    Ok(Some(value))
                }
                None => Err(format!("{name} is damaged ({error}). It was left as it is.")),
            }
        }
    }
}

/// What a sealed file starts with; the rest is base64 of the DPAPI-sealed JSON.
const SEALED: &str = "monolab-sealed-1:";

/// Reads a file of secrets (SSH, panel and database passwords). On disk it is sealed to this
/// Windows user (DPAPI), so a copy of the app data folder (cloud sync, a backup, another program)
/// doesn't give the passwords away. A plain file from an older version is read and sealed at once.
pub fn read_secret<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    use base64::Engine;
    let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not read {name}: {error}")),
    };
    if let Some(rest) = text.trim().strip_prefix(SEALED) {
        let sealed = base64::engine::general_purpose::STANDARD.decode(rest).map_err(|_| format!("{name} is damaged."))?;
        let plain = crate::device::unseal(&sealed).ok_or(format!("{name} was saved by another Windows user and can't be opened here."))?;
        return serde_json::from_slice(&plain).map(Some).map_err(|error| format!("{name} is damaged ({error})."));
    }
    let value = serde_json::from_str(&text).map_err(|error| format!("{name} is damaged ({error}). It was left as it is."))?;
    let _ = write_secret(path, &text);
    Ok(Some(value))
}

/// Writes a file of secrets sealed (see [`read_secret`]), and removes any plain copy an older
/// version left as a backup.
pub fn write_secret(path: &Path, json: &str) -> std::io::Result<()> {
    use base64::Engine;
    let sealed = crate::device::seal(json.as_bytes()).ok_or_else(|| std::io::Error::other("Windows could not seal the passwords."))?;
    let text = format!("{SEALED}{}", base64::engine::general_purpose::STANDARD.encode(sealed));
    let temp = beside(path, ".tmp");
    {
        let mut file = fs::File::create(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&temp, path)?;
    for plain in [backup_of(path), beside(path, ".bak")] {
        let _ = fs::remove_file(plain);
    }
    Ok(())
}

/// A save that died between swapping files can leave only the backup.
fn restore_backup<T: DeserializeOwned>(path: &Path, name: &str) -> Result<Option<T>, String> {
    let backup = backup_to_read(path);
    let Ok(text) = fs::read_to_string(&backup) else {
        return Ok(None);
    };
    let value = serde_json::from_str(&text).map_err(|error| format!("{name} is missing and its backup is damaged ({error})."))?;
    let _ = fs::copy(&backup, path);
    Ok(Some(value))
}

/// Moves a damaged file out of the way so the next save can't overwrite it; returns where it went.
pub fn set_aside(path: &Path) -> Option<PathBuf> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_secs())
        .unwrap_or_default();
    let target = beside(path, &format!(".broken-{stamp}"));
    fs::rename(path, &target).ok().map(|_| target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("monolab-store-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn keeps_the_last_good_copy_and_falls_back_to_it() {
        let dir = folder("fallback");
        let file = dir.join("instances.json");
        assert_eq!(read::<Vec<u32>>(&file).unwrap(), None);
        write(&file, "[1,2]").unwrap();
        write(&file, "[1,2,3]").unwrap();
        assert_eq!(fs::read_to_string(dir.join("instances.json.bak")).unwrap(), "[1,2]");

        fs::write(&file, "[1,2,3").unwrap();
        assert_eq!(read::<Vec<u32>>(&file).unwrap(), Some(vec![1, 2]));
        assert_eq!(read::<Vec<u32>>(&file).unwrap(), Some(vec![1, 2]));
        assert!(fs::read_dir(&dir).unwrap().flatten().any(|one| one.file_name().to_string_lossy().contains(".broken-")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn secrets_are_sealed_and_old_plain_files_too() {
        let dir = folder("secrets");
        let file = dir.join("ssh-secrets.json");
        fs::write(&file, r#"{"host":"hunter2"}"#).unwrap();
        fs::write(dir.join("ssh-secrets.json.bak"), r#"{"host":"old"}"#).unwrap();
        let read: std::collections::HashMap<String, String> = read_secret(&file).unwrap().unwrap();
        assert_eq!(read["host"], "hunter2");
        let disk = fs::read_to_string(&file).unwrap();
        assert!(disk.starts_with(SEALED) && !disk.contains("hunter2"), "sealed on read: {disk}");
        assert!(!dir.join("ssh-secrets.json.bak").exists(), "the plain backup is gone");
        write_secret(&file, r#"{"host":"new"}"#).unwrap();
        let again: std::collections::HashMap<String, String> = read_secret(&file).unwrap().unwrap();
        assert_eq!(again["host"], "new");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn never_reads_damage_as_empty() {
        let dir = folder("damaged");
        let file = dir.join("settings.json");
        fs::write(&file, "{\"a\":").unwrap();
        assert!(read::<serde_json::Value>(&file).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\"a\":");

        write(&file, "{}").unwrap();
        assert!(!dir.join("settings.json.bak").exists(), "a damaged file must not become the backup");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovers_when_only_the_backup_survived() {
        let dir = folder("only-backup");
        let file = dir.join("tasks.json");
        fs::write(dir.join("tasks.json.bak"), "[7]").unwrap();
        assert_eq!(read::<Vec<u32>>(&file).unwrap(), Some(vec![7]));
        assert!(file.is_file());
        let _ = fs::remove_dir_all(&dir);
    }
}

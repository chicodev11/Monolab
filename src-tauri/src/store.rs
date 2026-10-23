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
        let _ = fs::copy(path, beside(path, ".bak"));
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
            let backup = beside(path, ".bak");
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

/// A save that died between swapping files can leave only the backup.
fn restore_backup<T: DeserializeOwned>(path: &Path, name: &str) -> Result<Option<T>, String> {
    let backup = beside(path, ".bak");
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

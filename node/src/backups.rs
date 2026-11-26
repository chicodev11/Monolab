//! Backups of a server folder, kept in `<data>/backups/<server>/` as `.tar.gz` made by the system's
//! `tar`, with a small `.json` beside each for the note. Shapes are the app's `BackupFile`.

use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::config::ServerConfig;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub file_name: String,
    pub size_mib: f64,
    /// Seconds.
    pub created_at: u64,
    pub note: String,
    pub worlds_only: bool,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Side {
    #[serde(default)]
    note: String,
    #[serde(default)]
    worlds_only: bool,
    #[serde(default)]
    created_at: u64,
}

pub struct Backups {
    pub dir: PathBuf,
    pub tar: String,
}

/// The backup's name in its folder: what this node made, nothing else.
fn backup_name(name: &str) -> Result<&str, String> {
    let ok = name.ends_with(".tar.gz") && !name.starts_with('.') && name.chars().all(|one| one.is_ascii_alphanumeric() || matches!(one, '-' | '_' | '.'));
    if ok { Ok(name) } else { Err("Unknown backup.".into()) }
}

/// The top folders that hold a world (a `level.dat`).
fn world_folders(root: &Path) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|one| one.path().join("level.dat").is_file())
        .filter_map(|one| one.file_name().to_str().map(str::to_string))
        .collect();
    found.sort();
    found
}

impl Backups {
    fn folder(&self, server: &str) -> PathBuf {
        self.dir.join(server)
    }

    pub fn list(&self, server: &str) -> Vec<BackupFile> {
        let folder = self.folder(server);
        let mut list: Vec<BackupFile> = fs::read_dir(&folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_str()?.to_string();
                backup_name(&name).ok()?;
                let meta = entry.metadata().ok()?;
                let side: Side = fs::read_to_string(folder.join(format!("{name}.json"))).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
                let made = meta.modified().ok().and_then(|when| when.duration_since(std::time::UNIX_EPOCH).ok()).map(|gap| gap.as_secs()).unwrap_or(0);
                Some(BackupFile {
                    file_name: name,
                    size_mib: meta.len() as f64 / 1024.0 / 1024.0,
                    created_at: if side.created_at > 0 { side.created_at } else { made },
                    note: side.note,
                    worlds_only: side.worlds_only,
                })
            })
            .collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.file_name.cmp(&a.file_name)));
        list
    }

    /// `create_backup` `{id,note,worldsOnly,paths}`. `paths` (inside the server folder) narrows it; else
    /// the worlds, or the whole folder. A server that keeps running is fine: files that changed while
    /// `tar` read them are kept as read.
    pub fn create(&self, server: &ServerConfig, note: &str, worlds_only: bool, paths: &[String]) -> Result<BackupFile, String> {
        let root = server.path.canonicalize().map_err(|error| format!("Server folder not found: {error}"))?;
        let items: Vec<String> = if !paths.is_empty() {
            let mut out = Vec::new();
            for one in paths {
                let real = crate::files::resolve_in_server(&root, one).map_err(|error| error.to_string())?;
                out.push(real.strip_prefix(&root).unwrap_or(&real).to_string_lossy().into_owned());
            }
            out
        } else if worlds_only {
            let worlds = world_folders(&root);
            if worlds.is_empty() {
                return Err("No world folders found (a folder with level.dat).".into());
            }
            worlds
        } else {
            vec![".".into()]
        };
        let folder = self.folder(&server.id);
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&folder).map_err(|error| format!("Could not make the backup folder: {error}"))?;
        let now = crate::serve::unix_now();
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let mut name = format!("{}-{stamp}.tar.gz", server.id);
        let mut count = 2;
        while folder.join(&name).exists() {
            name = format!("{}-{stamp}-{count}.tar.gz", server.id);
            count += 1;
        }
        let target = folder.join(&name);
        let partial = folder.join(format!(".{name}.part"));
        let mut command = Command::new(&self.tar);
        command
            .arg("-czf")
            .arg(&partial)
            .arg("-C")
            .arg(&root)
            .args(["--exclude=./.monolab-trash", "--exclude=.monolab-trash", "--warning=no-file-changed", "--"])
            .args(&items)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let out = command.output().map_err(|error| format!("Could not run tar: {error}"))?;
        // GNU tar: 1 means some files changed while they were read; the backup is still whole.
        if !matches!(out.status.code(), Some(0) | Some(1)) {
            let _ = fs::remove_file(&partial);
            return Err(format!("The backup failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        fs::rename(&partial, &target).map_err(|error| error.to_string())?;
        let side = Side { note: note.trim().chars().take(200).collect(), worlds_only: worlds_only && paths.is_empty(), created_at: now };
        let _ = fs::write(folder.join(format!("{name}.json")), serde_json::to_string(&side).unwrap_or_default());
        self.list(&server.id).into_iter().find(|one| one.file_name == name).ok_or_else(|| "The backup was made but can't be read.".into())
    }

    pub fn delete(&self, server: &str, name: &str) -> Result<(), String> {
        let name = backup_name(name)?;
        let path = self.folder(server).join(name);
        if !path.is_file() {
            return Err("Unknown backup.".into());
        }
        fs::remove_file(&path).map_err(|error| format!("Could not delete the backup: {error}"))?;
        let _ = fs::remove_file(self.folder(server).join(format!("{name}.json")));
        Ok(())
    }

    /// `restore_backup` `{id,fileName}`, with the server stopped: a backup of the folder as it is now
    /// ("Before restoring …") comes first, then the backup's files are written over the folder.
    /// Files the backup doesn't have stay. Gives the safety backup, as the app does.
    pub fn restore(&self, server: &ServerConfig, name: &str) -> Result<BackupFile, String> {
        let name = backup_name(name)?;
        let path = self.folder(&server.id).join(name);
        if !path.is_file() {
            return Err("Unknown backup.".into());
        }
        // A tar made elsewhere could hold `../` or absolute names; this node's own never do, but check.
        let listed = Command::new(&self.tar).arg("-tzf").arg(&path).stdin(Stdio::null()).output().map_err(|error| format!("Could not run tar: {error}"))?;
        if !listed.status.success() {
            return Err("The backup can't be read.".into());
        }
        for entry in String::from_utf8_lossy(&listed.stdout).lines() {
            let entry = entry.trim_start_matches("./");
            if entry.is_empty() || entry == "." {
                continue;
            }
            if entry.starts_with('/') || entry.split('/').any(|part| part == "..") {
                return Err("The backup holds paths outside the server folder.".into());
            }
        }
        let before = self.create(server, &format!("Before restoring {name}"), false, &[])?;
        let out = Command::new(&self.tar)
            .arg("-xzf")
            .arg(&path)
            .arg("-C")
            .arg(&server.path)
            .args(["--no-same-owner", "--no-overwrite-dir"])
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| format!("Could not run tar: {error}"))?;
        if !out.status.success() {
            return Err(format!("Restoring failed part way: {}. The folder as it was is in {}.", String::from_utf8_lossy(&out.stderr).trim(), before.file_name));
        }
        Ok(before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(root: &Path) -> ServerConfig {
        ServerConfig { id: "s".into(), name: "S".into(), path: root.to_path_buf(), tmux: String::new(), log: "logs/latest.log".into(), start: String::new(), stop: "stop".into() }
    }

    #[test]
    fn names_are_ours_only() {
        assert!(backup_name("s-20261121-101010.tar.gz").is_ok());
        assert!(backup_name("../x.tar.gz").is_err());
        assert!(backup_name(".s.tar.gz.part").is_err());
        assert!(backup_name("x.zip").is_err());
    }

    /// Needs `tar` (every Linux has it).
    #[test]
    fn back_up_list_restore_delete() {
        let base = crate::files::tests::sandbox("backups");
        let root = base.join("server");
        fs::create_dir_all(root.join("world")).unwrap();
        fs::create_dir_all(root.join(".monolab-trash")).unwrap();
        fs::write(root.join("world/level.dat"), "w1").unwrap();
        fs::write(root.join("server.properties"), "motd=a\n").unwrap();
        fs::write(root.join(".monolab-trash/gone.txt"), "x").unwrap();
        let backups = Backups { dir: base.join("state/backups"), tar: "tar".into() };
        let full = backups.create(&server(&root), "first", false, &[]).unwrap();
        assert_eq!(full.note, "first");
        let worlds = backups.create(&server(&root), "", true, &[]).unwrap();
        assert!(worlds.worlds_only);
        assert!(backups.create(&server(&root), "", false, &["../x".into()]).unwrap_err().contains("outside"));
        assert_eq!(backups.list("s").len(), 2);

        fs::write(root.join("world/level.dat"), "w2").unwrap();
        fs::write(root.join("server.properties"), "motd=b\n").unwrap();
        let before = backups.restore(&server(&root), &full.file_name).unwrap();
        assert!(before.note.starts_with("Before restoring"));
        assert_eq!(fs::read_to_string(root.join("world/level.dat")).unwrap(), "w1");
        assert_eq!(fs::read_to_string(root.join("server.properties")).unwrap(), "motd=a\n");
        assert_eq!(backups.list("s").len(), 3);

        backups.delete("s", &worlds.file_name).unwrap();
        assert_eq!(backups.list("s").len(), 2);
        assert!(backups.delete("s", "../../etc.tar.gz").is_err());
        assert!(backups.restore(&server(&root), "nope.tar.gz").is_err());
        fs::remove_dir_all(&base).unwrap();
    }
}

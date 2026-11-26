//! Rewind: a light history of a server's text files (configs, scripts, datapack functions).
//!
//! A watcher stats the files every few seconds and, when one was added, changed or deleted, keeps the
//! new content (gzipped, stored once per content hash) and appends a timeline entry. Worlds, logs,
//! jars and other large or binary files are left out, so this stays small and fast; it is a safety net
//! for edits, not a backup.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Files bigger than this are not tracked.
pub const MAX_FILE: u64 = 512 * 1024;
/// Stop walking after this many files so a huge folder can't stall the watcher.
const MAX_FILES: usize = 20_000;
const MAX_DEPTH: usize = 10;
/// Entries kept per server; older ones are dropped with the content only they used.
pub const KEEP: usize = 3_000;

const TEXT_EXTENSIONS: &[&str] = &[
    "properties", "yml", "yaml", "json", "json5", "jsonc", "toml", "txt", "cfg", "conf", "config", "ini", "xml", "csv", "md",
    "sk", "js", "mjs", "ts", "lua", "py", "mcfunction", "mcmeta", "snbt", "lang", "zs", "sh", "bat", "cmd", "ps1", "env", "hocon",
    "secret", "list", "kts", "gradle",
];

/// Folder names never walked into, at any depth.
const SKIP_DIRS: &[&str] = &[
    ".git", ".rewind", "logs", "crash-reports", "cache", ".cache", "libraries", "versions", "backups", "node_modules", "debug",
    ".fabric", ".mixin.out", "bluemap", "dynmap", "squaremap", "region", "entities", "poi", "playerdata", "stats",
    "DIM-1", "DIM1", "bundler", "jre", "java", "runtime", "server-data",
];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
struct Seen {
    mtime: u64,
    size: u64,
    hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RewindEntry {
    /// Unique and increasing: milliseconds plus a counter.
    pub id: String,
    /// Unix seconds.
    pub at: u64,
    /// Relative to the server folder, with forward slashes.
    pub path: String,
    /// "added", "changed", "deleted" or "restored".
    pub kind: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub size: u64,
    /// Who made the change, when the host knew (`@amy`, `@amy's AI`, `host`, `AI`; see `authorship`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RewindVersions {
    pub before: Option<String>,
    pub after: Option<String>,
    /// What the file holds now; None when it is gone.
    pub current: Option<String>,
}

pub struct Store {
    dir: PathBuf,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|one| one.as_secs()).unwrap_or(0)
}

fn now_millis() -> u128 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|one| one.as_millis()).unwrap_or(0)
}

pub fn hash_of(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().take(16).map(|byte| format!("{byte:02x}")).collect()
}

fn tracked_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == "eula.txt" || lower.ends_with(".lock") {
        return false;
    }
    if matches!(lower.as_str(), "ops.json" | "whitelist.json" | "banned-players.json" | "banned-ips.json" | "usercache.json" | "usernamecache.json") {
        // The server rewrites these by itself all the time; still worth having, they're small.
        return true;
    }
    lower.rsplit_once('.').is_some_and(|(_, ext)| TEXT_EXTENSIONS.contains(&ext))
}

fn skip_dir(dir: &Path, name: &str) -> bool {
    SKIP_DIRS.iter().any(|one| one.eq_ignore_ascii_case(name)) || name.starts_with('.') && name != ".minecraft" || dir.join("level.dat").is_file()
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8_000).any(|byte| *byte == 0)
}

/// Relative paths (forward slashes) of the files Rewind keeps, with their modified time and size.
pub fn walk(root: &Path) -> Vec<(String, u64, u64)> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(read) = fs::read_dir(&dir) else { continue };
        for one in read.flatten() {
            if out.len() >= MAX_FILES {
                return out;
            }
            let Ok(kind) = one.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            let name = one.file_name().to_string_lossy().to_string();
            let path = one.path();
            if kind.is_dir() {
                if depth < MAX_DEPTH && !skip_dir(&path, &name) {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if !tracked_name(&name) {
                continue;
            }
            let Ok(meta) = one.metadata() else { continue };
            if meta.len() > MAX_FILE {
                continue;
            }
            let mtime = meta.modified().ok().and_then(|at| at.duration_since(UNIX_EPOCH).ok()).map(|at| at.as_millis() as u64).unwrap_or(0);
            let Ok(relative) = path.strip_prefix(root) else { continue };
            let relative = relative.to_string_lossy().replace('\\', "/");
            out.push((relative, mtime, meta.len()));
        }
    }
    out
}

impl Store {
    pub fn open(dir: PathBuf) -> Self {
        Store { dir }
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("index.json")
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join("log.jsonl")
    }

    fn object_path(&self, hash: &str) -> Option<PathBuf> {
        if hash.len() < 3 || !hash.chars().all(|one| one.is_ascii_hexdigit()) {
            return None;
        }
        Some(self.dir.join("objects").join(&hash[..2]).join(hash))
    }

    fn read_index(&self) -> Option<HashMap<String, Seen>> {
        let text = fs::read_to_string(self.index_path()).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn write_index(&self, index: &HashMap<String, Seen>) -> Result<(), String> {
        fs::create_dir_all(&self.dir).map_err(|error| error.to_string())?;
        let text = serde_json::to_string(index).map_err(|error| error.to_string())?;
        let temp = self.dir.join("index.json.tmp");
        fs::write(&temp, text).map_err(|error| error.to_string())?;
        fs::rename(&temp, self.index_path()).map_err(|error| error.to_string())
    }

    fn keep_object(&self, hash: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.object_path(hash).ok_or("Bad content id.")?;
        if path.is_file() {
            return Ok(());
        }
        fs::create_dir_all(path.parent().ok_or("Bad content path.")?).map_err(|error| error.to_string())?;
        let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).map_err(|error| error.to_string())?;
        let packed = encoder.finish().map_err(|error| error.to_string())?;
        let temp = path.with_extension("tmp");
        fs::write(&temp, packed).map_err(|error| error.to_string())?;
        fs::rename(&temp, &path).map_err(|error| error.to_string())
    }

    pub fn read_object(&self, hash: &str) -> Option<String> {
        let packed = fs::read(self.object_path(hash)?).ok()?;
        let mut text = String::new();
        GzDecoder::new(packed.as_slice()).read_to_string(&mut text).ok()?;
        Some(text)
    }

    fn append(&self, entries: &[RewindEntry]) -> Result<(), String> {
        if entries.is_empty() {
            return Ok(());
        }
        fs::create_dir_all(&self.dir).map_err(|error| error.to_string())?;
        let mut file = fs::OpenOptions::new().create(true).append(true).open(self.log_path()).map_err(|error| error.to_string())?;
        let mut text = String::new();
        for one in entries {
            text.push_str(&serde_json::to_string(one).map_err(|error| error.to_string())?);
            text.push('\n');
        }
        file.write_all(text.as_bytes()).map_err(|error| error.to_string())
    }

    /// All entries, oldest first.
    pub fn entries(&self) -> Vec<RewindEntry> {
        let Ok(file) = fs::File::open(self.log_path()) else { return Vec::new() };
        BufReader::new(file).lines().map_while(Result::ok).filter_map(|line| serde_json::from_str(&line).ok()).collect()
    }

    /// Newest first, optionally only one file, at most `limit`.
    pub fn list(&self, path: Option<&str>, limit: usize) -> Vec<RewindEntry> {
        let mut all = self.entries();
        all.reverse();
        all.into_iter().filter(|one| path.is_none_or(|wanted| one.path == wanted)).take(limit).collect()
    }

    /// Compares the folder with the last look. The first look only remembers what is there.
    pub fn scan(&self, root: &Path, by: &dyn Fn(&str, u64) -> Option<String>) -> Result<Vec<RewindEntry>, String> {
        let first = self.read_index().is_none();
        let mut index = self.read_index().unwrap_or_default();
        let mut found = HashSet::new();
        let mut out = Vec::new();
        let mut counter = 0u32;
        let mut stamp = || {
            counter += 1;
            format!("{}-{counter:04}", now_millis())
        };
        let mut changed_index = first;
        for (relative, mtime, size) in walk(root) {
            found.insert(relative.clone());
            let known = index.get(&relative);
            if known.is_some_and(|one| one.mtime == mtime && one.size == size) {
                continue;
            }
            let Ok(bytes) = fs::read(root.join(&relative)) else { continue };
            if looks_binary(&bytes) {
                continue;
            }
            let hash = hash_of(&bytes);
            changed_index = true;
            let before = known.map(|one| one.hash.clone());
            if before.as_deref() == Some(hash.as_str()) {
                index.insert(relative, Seen { mtime, size, hash });
                continue;
            }
            self.keep_object(&hash, &bytes)?;
            if !first {
                out.push(RewindEntry {
                    id: stamp(),
                    at: now_secs(),
                    path: relative.clone(),
                    kind: if before.is_some() { "changed" } else { "added" }.into(),
                    before,
                    after: Some(hash.clone()),
                    size,
                    by: by(&relative, now_secs()),
                });
            }
            index.insert(relative, Seen { mtime, size, hash });
        }
        let gone: Vec<String> = index.keys().filter(|one| !found.contains(*one)).cloned().collect();
        for relative in gone {
            let Some(seen) = index.remove(&relative) else { continue };
            changed_index = true;
            let who = by(&relative, now_secs());
            out.push(RewindEntry { id: stamp(), at: now_secs(), path: relative, kind: "deleted".into(), before: Some(seen.hash), after: None, size: 0, by: who });
        }
        if changed_index {
            self.write_index(&index)?;
        }
        self.append(&out)?;
        if !out.is_empty() {
            self.trim(KEEP)?;
        }
        Ok(out)
    }

    /// Keeps the newest `keep` entries and deletes content nothing points to any more.
    pub fn trim(&self, keep: usize) -> Result<(), String> {
        let all = self.entries();
        if all.len() <= keep + keep / 10 {
            return Ok(());
        }
        let kept = &all[all.len() - keep..];
        let temp = self.dir.join("log.jsonl.tmp");
        let mut text = String::new();
        for one in kept {
            text.push_str(&serde_json::to_string(one).map_err(|error| error.to_string())?);
            text.push('\n');
        }
        fs::write(&temp, text).map_err(|error| error.to_string())?;
        fs::rename(&temp, self.log_path()).map_err(|error| error.to_string())?;
        let mut used: HashSet<String> = kept.iter().flat_map(|one| [one.before.clone(), one.after.clone()]).flatten().collect();
        used.extend(self.read_index().unwrap_or_default().into_values().map(|one| one.hash));
        let Ok(buckets) = fs::read_dir(self.dir.join("objects")) else { return Ok(()) };
        for bucket in buckets.flatten() {
            let Ok(files) = fs::read_dir(bucket.path()) else { continue };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().to_string();
                if !used.contains(&name) {
                    let _ = fs::remove_file(file.path());
                }
            }
        }
        Ok(())
    }

    pub fn find(&self, id: &str) -> Option<RewindEntry> {
        self.entries().into_iter().find(|one| one.id == id)
    }

    pub fn versions(&self, root: &Path, id: &str) -> Result<RewindVersions, String> {
        let entry = self.find(id).ok_or("That change is no longer in the history.")?;
        let text = |hash: &Option<String>| hash.as_deref().and_then(|one| self.read_object(one));
        let current = crate::paths::check_relative(&entry.path).ok().and_then(|rel| fs::read_to_string(root.join(rel)).ok());
        Ok(RewindVersions { before: text(&entry.before), after: text(&entry.after), current })
    }

    /// Puts the file back as it was before (`side` "before") or right after (`side` "after") a change.
    /// Restoring to "no file" deletes it. The restore is itself recorded, so it can be undone.
    pub fn restore(&self, root: &Path, id: &str, side: &str) -> Result<RewindEntry, String> {
        let entry = self.find(id).ok_or("That change is no longer in the history.")?;
        let hash = match side {
            "before" => entry.before.clone(),
            "after" => entry.after.clone(),
            _ => return Err("Choose before or after.".into()),
        };
        let relative = crate::paths::check_relative(&entry.path)?;
        let target = root.join(&relative);
        let mut index = self.read_index().unwrap_or_default();
        let previous = index.get(&entry.path).map(|one| one.hash.clone());
        match &hash {
            Some(hash) => {
                let text = self.read_object(hash).ok_or("The saved content is missing.")?;
                if let Some(parent) = relative.parent().filter(|one| !one.as_os_str().is_empty()) {
                    fs::create_dir_all(root.join(parent)).map_err(|error| format!("Could not create the folder: {error}"))?;
                }
                let target = crate::paths::resolve_new_in_server(root, &entry.path)?;
                fs::write(&target, text.as_bytes()).map_err(|error| format!("Could not write the file: {error}"))?;
                let meta = fs::metadata(&target).map_err(|error| error.to_string())?;
                let mtime = meta.modified().ok().and_then(|at| at.duration_since(UNIX_EPOCH).ok()).map(|at| at.as_millis() as u64).unwrap_or(0);
                index.insert(entry.path.clone(), Seen { mtime, size: meta.len(), hash: hash.clone() });
            }
            None => {
                if target.exists() {
                    let real = crate::paths::resolve_in_server(root, &entry.path)?;
                    fs::remove_file(real).map_err(|error| format!("Could not delete the file: {error}"))?;
                }
                index.remove(&entry.path);
            }
        }
        self.write_index(&index)?;
        let done = RewindEntry {
            id: format!("{}-r", now_millis()),
            at: now_secs(),
            path: entry.path.clone(),
            kind: "restored".into(),
            before: previous,
            after: hash,
            size: entry.size,
            by: None,
        };
        self.append(std::slice::from_ref(&done))?;
        Ok(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> (PathBuf, Store) {
        let base = std::env::temp_dir().join(format!("se-rewind-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("server");
        fs::create_dir_all(root.join("plugins/Essentials")).unwrap();
        fs::create_dir_all(root.join("world")).unwrap();
        fs::write(root.join("world/level.dat"), b"\0\0").unwrap();
        fs::write(root.join("world/notes.txt"), b"inside a world").unwrap();
        fs::write(root.join("server.properties"), "motd=hi\n").unwrap();
        fs::write(root.join("plugins/Essentials/config.yml"), "a: 1\n").unwrap();
        fs::write(root.join("server.jar"), b"PK\0\0").unwrap();
        (root.canonicalize().map(crate::plain_path).unwrap(), Store::open(base.join("store")))
    }

    fn touch(path: &Path, text: &str) {
        // Keep the size different or wait for the clock, so the change is seen even on coarse file systems.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(path, text).unwrap();
    }

    #[test]
    fn walks_text_files_but_not_worlds_or_jars() {
        let (root, _) = sandbox("walk");
        let mut names: Vec<String> = walk(&root).into_iter().map(|one| one.0).collect();
        names.sort();
        assert_eq!(names, vec!["plugins/Essentials/config.yml".to_string(), "server.properties".to_string()]);
    }

    #[test]
    fn first_look_is_quiet_then_changes_are_recorded() {
        let (root, store) = sandbox("record");
        assert!(store.scan(&root, &|_, _| None).unwrap().is_empty());
        assert!(store.scan(&root, &|_, _| None).unwrap().is_empty());

        touch(&root.join("server.properties"), "motd=changed\npvp=false\n");
        fs::write(root.join("plugins/new.yml"), "x: y\n").unwrap();
        fs::remove_file(root.join("plugins/Essentials/config.yml")).unwrap();
        let mut seen = store.scan(&root, &|_, _| None).unwrap();
        seen.sort_by(|a, b| a.path.cmp(&b.path));
        let kinds: Vec<(&str, &str)> = seen.iter().map(|one| (one.path.as_str(), one.kind.as_str())).collect();
        assert_eq!(kinds, vec![("plugins/Essentials/config.yml", "deleted"), ("plugins/new.yml", "added"), ("server.properties", "changed")]);

        let changed = seen.iter().find(|one| one.path == "server.properties").unwrap();
        let versions = store.versions(&root, &changed.id).unwrap();
        assert_eq!(versions.before.as_deref(), Some("motd=hi\n"));
        assert_eq!(versions.after.as_deref(), Some("motd=changed\npvp=false\n"));
        assert_eq!(store.list(None, 10).len(), 3);
        assert_eq!(store.list(Some("server.properties"), 10).len(), 1);
    }

    #[test]
    fn restores_both_ways_and_records_it() {
        let (root, store) = sandbox("restore");
        store.scan(&root, &|_, _| None).unwrap();
        touch(&root.join("server.properties"), "motd=broken settings\n");
        fs::remove_file(root.join("plugins/Essentials/config.yml")).unwrap();
        fs::remove_dir(root.join("plugins/Essentials")).unwrap();
        let seen = store.scan(&root, &|_, _| None).unwrap();
        let changed = seen.iter().find(|one| one.kind == "changed").unwrap();
        let deleted = seen.iter().find(|one| one.kind == "deleted").unwrap();

        store.restore(&root, &changed.id, "before").unwrap();
        assert_eq!(fs::read_to_string(root.join("server.properties")).unwrap(), "motd=hi\n");
        store.restore(&root, &deleted.id, "before").unwrap();
        assert_eq!(fs::read_to_string(root.join("plugins/Essentials/config.yml")).unwrap(), "a: 1\n");
        // The restore was written to the index, so the next look sees nothing new.
        assert!(store.scan(&root, &|_, _| None).unwrap().is_empty());
        assert_eq!(store.list(None, 1)[0].kind, "restored");

        store.restore(&root, &deleted.id, "after").unwrap();
        assert!(!root.join("plugins/Essentials/config.yml").exists());
        assert!(store.restore(&root, "no-such-id", "before").is_err());
    }

    #[test]
    fn trimming_keeps_the_newest_and_their_content() {
        let (root, store) = sandbox("trim");
        store.scan(&root, &|_, _| None).unwrap();
        for at in 0..30 {
            touch(&root.join("server.properties"), &format!("motd={at}\n{}", "x".repeat(at)));
            store.scan(&root, &|_, _| None).unwrap();
        }
        store.trim(5).unwrap();
        let left = store.entries();
        assert_eq!(left.len(), 5);
        let last = left.last().unwrap();
        assert!(store.versions(&root, &last.id).unwrap().after.is_some());
        let objects: usize = fs::read_dir(store.dir.join("objects")).unwrap().flatten().map(|one| fs::read_dir(one.path()).unwrap().count()).sum();
        assert!(objects <= 8, "old content should be dropped, found {objects}");
    }

    #[test]
    fn content_ids_are_checked() {
        let (_, store) = sandbox("ids");
        assert!(store.object_path("../../etc").is_none());
        assert!(store.read_object("zz").is_none());
    }

    #[test]
    fn rewind_entry_reads_old_files() {
        let old = r#"{"id":"1-0001","at":1,"path":"a.yml","kind":"changed","before":null,"after":"x","size":1}"#;
        let entry: RewindEntry = serde_json::from_str(old).unwrap();
        assert_eq!(entry.by, None);
        let named = RewindEntry { by: Some("@amy".into()), ..entry };
        assert!(serde_json::to_string(&named).unwrap().contains(r#""by":"@amy""#));
    }

}

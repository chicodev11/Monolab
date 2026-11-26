//! Team Storage (F6): files a team keeps for later — a friend's .bbmodel, a resource pack, a plugin jar, a bundle
//! of settings — outside any server folder, so deleting a server doesn't take them. They live on the PC that
//! hosts the team's servers (the place teammates can reach over Monolink), in
//! `<app data>/team-storage/<team>/`: `index.json` and `files/<id>-<version>`. The same name again adds a version.
//! Trash keeps files until emptied. Roles: Viewer lists and downloads, Developer adds and puts files into servers,
//! Admin and Owner trash, restore and change the limit.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Default space per team.
pub const DEFAULT_LIMIT_GB: u64 = 5;
/// Biggest single file.
pub const MOST_FILE: u64 = 4 * 1024 * 1024 * 1024;
const GB: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageVersion {
    pub n: u32,
    #[specta(type = f64)]
    pub size: u64,
    pub by: String,
    #[specta(type = f64)]
    pub at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub versions: Vec<StorageVersion>,
    #[serde(default)]
    pub trashed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageIndex {
    #[serde(default)]
    pub items: Vec<StorageItem>,
    /// Space for the team in GB (0 = the default).
    #[serde(default)]
    #[specta(type = f64)]
    pub limit_gb: u64,
}

/// What the screens show: the items, the space used and the limit (bytes).
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageView {
    pub items: Vec<StorageItem>,
    #[specta(type = f64)]
    pub used: u64,
    #[specta(type = f64)]
    pub limit: u64,
}

/// One team's storage folder.
pub struct Storage {
    dir: PathBuf,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|time| time.as_secs()).unwrap_or(0)
}

/// A team id as a folder name (ids are the account server's, but never trust a path piece).
fn safe(team: &str) -> Result<&str, String> {
    if !team.is_empty() && team.len() <= 64 && team.chars().all(|one| one.is_ascii_alphanumeric() || one == '-' || one == '_') {
        Ok(team)
    } else {
        Err("This isn't a team.".into())
    }
}

/// A file name people see: no folders, no control characters, up to 120 characters.
fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim().rsplit(['/', '\\']).next().unwrap_or_default().trim().to_string();
    if name.is_empty() || name == "." || name == ".." || name.chars().any(char::is_control) || name.chars().count() > 120 {
        return Err("Give the file a name (up to 120 characters).".into());
    }
    Ok(name)
}

impl Storage {
    pub fn open(root: &Path, team: &str) -> Result<Self, String> {
        let dir = root.join(safe(team)?);
        fs::create_dir_all(dir.join("files")).map_err(|error| format!("Could not open the team's storage: {error}"))?;
        Ok(Storage { dir })
    }

    fn index_path(&self) -> PathBuf {
        self.dir.join("index.json")
    }

    pub fn index(&self) -> StorageIndex {
        fs::read_to_string(self.index_path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    fn save(&self, index: &StorageIndex) -> Result<(), String> {
        let text = serde_json::to_string_pretty(index).map_err(|error| error.to_string())?;
        let temp = self.dir.join("index.json.tmp");
        fs::write(&temp, text).map_err(|error| format!("Could not save the storage list: {error}"))?;
        fs::rename(&temp, self.index_path()).map_err(|error| format!("Could not save the storage list: {error}"))
    }

    pub fn limit(&self) -> u64 {
        let gb = self.index().limit_gb;
        (if gb == 0 { DEFAULT_LIMIT_GB } else { gb }) * GB
    }

    /// Bytes kept, every version and the trash included.
    pub fn used(&self) -> u64 {
        self.index().items.iter().flat_map(|item| item.versions.iter()).map(|one| one.size).sum()
    }

    pub fn view(&self) -> StorageView {
        StorageView { items: self.index().items, used: self.used(), limit: self.limit() }
    }

    pub fn file(&self, id: &str, version: u32) -> Result<PathBuf, String> {
        let index = self.index();
        let item = index.items.iter().find(|one| one.id == id).ok_or("There is no such file in Storage.")?;
        let version = if version == 0 { item.versions.last().map(|one| one.n).unwrap_or(0) } else { version };
        if !item.versions.iter().any(|one| one.n == version) {
            return Err("There is no such version.".into());
        }
        Ok(self.dir.join("files").join(format!("{id}-{version}")))
    }

    /// Adds a file (copied from `source`) under `name`; the same name adds a version to that item.
    pub fn put(&self, source: &Path, name: &str, note: &str, tags: &[String], by: &str) -> Result<StorageItem, String> {
        let name = clean_name(name)?;
        let size = fs::metadata(source).map_err(|error| format!("Could not read the file: {error}"))?.len();
        if size > MOST_FILE {
            return Err("Files in Storage can be at most 4 GB.".into());
        }
        if self.used() + size > self.limit() {
            return Err(format!("The team's Storage is full ({} GB). Delete old files or versions, or ask the owner for more space.", self.limit() / GB));
        }
        let mut index = self.index();
        let at = index.items.iter().position(|one| one.name.eq_ignore_ascii_case(&name) && !one.trashed);
        let at = match at {
            Some(at) => at,
            None => {
                let id = format!("{}{:04}", now(), index.items.len() % 10_000);
                index.items.push(StorageItem { id, name: name.clone(), ..Default::default() });
                index.items.len() - 1
            }
        };
        let item = &mut index.items[at];
        let n = item.versions.last().map(|one| one.n + 1).unwrap_or(1);
        fs::copy(source, self.dir.join("files").join(format!("{}-{n}", item.id))).map_err(|error| format!("Could not keep the file: {error}"))?;
        item.versions.push(StorageVersion { n, size, by: by.to_string(), at: now() });
        if !note.trim().is_empty() {
            item.note = note.trim().chars().take(500).collect();
        }
        if !tags.is_empty() {
            item.tags = tags.iter().map(|one| one.trim().chars().take(24).collect::<String>()).filter(|one| !one.is_empty()).take(8).collect();
        }
        let done = item.clone();
        self.save(&index)?;
        Ok(done)
    }

    pub fn set_trashed(&self, id: &str, trashed: bool) -> Result<(), String> {
        let mut index = self.index();
        let item = index.items.iter_mut().find(|one| one.id == id).ok_or("There is no such file in Storage.")?;
        item.trashed = trashed;
        self.save(&index)
    }

    /// Deletes trashed items for good.
    pub fn empty_trash(&self) -> Result<u32, String> {
        let mut index = self.index();
        let gone: Vec<StorageItem> = index.items.iter().filter(|one| one.trashed).cloned().collect();
        for item in &gone {
            for version in &item.versions {
                let _ = fs::remove_file(self.dir.join("files").join(format!("{}-{}", item.id, version.n)));
            }
        }
        index.items.retain(|one| !one.trashed);
        self.save(&index)?;
        Ok(gone.len() as u32)
    }

    pub fn set_limit(&self, gb: u64) -> Result<(), String> {
        if !(1..=1024).contains(&gb) {
            return Err("Pick 1 to 1024 GB.".into());
        }
        let mut index = self.index();
        index.limit_gb = gb;
        self.save(&index)
    }
}

/// What a team role may do with Storage: `list`/`get` (everyone), `put`/`use` (Developer and up),
/// `trash`/`limit` (Admin and Owner).
pub fn may(role: &str, action: &str) -> bool {
    let rank = match role {
        "owner" => 3,
        "admin" => 2,
        "developer" => 1,
        _ => 0,
    };
    match action {
        "list" | "get" => true,
        "put" | "use" => rank >= 1,
        "trash" | "limit" => rank >= 2,
        _ => false,
    }
}

/// The piece of a download starting at `at` (bytes `at..end`), or None past the end.
pub fn part_range(size: u64, at: u64, part: u64) -> Option<(u64, u64)> {
    (at < size).then(|| (at, (at + part).min(size)))
}

/// Where a Storage file goes in a server: a folder inside it (no `..`), and the item's name.
pub fn target_in_server(folder: &str, name: &str) -> Result<String, String> {
    let folder = folder.replace('\\', "/");
    let folder = folder.trim_matches('/');
    if folder.split('/').any(|part| part == ".." || part.contains(':')) {
        return Err("Pick a folder inside the server.".into());
    }
    let name = clean_name(name)?;
    Ok(if folder.is_empty() { name } else { format!("{folder}/{name}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("monolab-storage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn same_name_adds_a_version() {
        let root = sandbox("versions");
        let file = root.join("bot.bbmodel");
        fs::write(&file, b"one").unwrap();
        let storage = Storage::open(&root.join("store"), "team-1").unwrap();
        let first = storage.put(&file, "bot.bbmodel", "first try", &[], "@amy").unwrap();
        fs::write(&file, b"second").unwrap();
        let second = storage.put(&file, "BOT.bbmodel", "", &["mob".into()], "@amy").unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(second.versions.iter().map(|one| one.n).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(second.note, "first try");
        assert_eq!(fs::read(storage.file(&first.id, 0).unwrap()).unwrap(), b"second");
        assert_eq!(fs::read(storage.file(&first.id, 1).unwrap()).unwrap(), b"one");
        assert_eq!(storage.used(), 9);
    }

    #[test]
    fn over_the_limit_is_refused() {
        let root = sandbox("limit");
        let file = root.join("big.bin");
        fs::write(&file, vec![0u8; 16]).unwrap();
        let storage = Storage::open(&root.join("store"), "team-2").unwrap();
        storage.set_limit(1).unwrap();
        let mut index = storage.index();
        index.items.push(StorageItem { id: "x".into(), name: "full".into(), versions: vec![StorageVersion { n: 1, size: GB, by: "@a".into(), at: 0 }], ..Default::default() });
        storage.save(&index).unwrap();
        assert!(storage.put(&file, "big.bin", "", &[], "@amy").unwrap_err().contains("full"));
        assert!(storage.set_limit(0).is_err());
    }

    #[test]
    fn to_server_stays_inside() {
        assert_eq!(target_in_server("plugins/MyPlugin", "a.yml").unwrap(), "plugins/MyPlugin/a.yml");
        assert_eq!(target_in_server("", "a.yml").unwrap(), "a.yml");
        assert!(target_in_server("../outside", "a.yml").is_err());
        assert!(target_in_server("C:/Windows", "a.yml").is_err());
        assert_eq!(target_in_server("plugins", "../../evil.jar").unwrap(), "plugins/evil.jar");
        assert!(Storage::open(Path::new("x"), "../team").is_err());
    }

    #[test]
    fn viewer_cannot_put() {
        assert!(may("viewer", "list") && may("viewer", "get"));
        assert!(!may("viewer", "put") && !may("viewer", "use") && !may("viewer", "trash"));
        assert!(may("developer", "put") && !may("developer", "trash"));
        assert!(may("admin", "trash") && may("owner", "limit"));
    }

    #[test]
    fn download_parts_cover_the_file() {
        let mut at = 0;
        let mut pieces = Vec::new();
        while let Some((from, to)) = part_range(2_500_000, at, 1_048_576) {
            pieces.push((from, to));
            at = to;
        }
        assert_eq!(pieces, [(0, 1_048_576), (1_048_576, 2_097_152), (2_097_152, 2_500_000)]);
        assert_eq!(part_range(0, 0, 10), None);
    }

    #[test]
    fn trash_then_restore() {
        let root = sandbox("trash");
        let file = root.join("a.txt");
        fs::write(&file, b"a").unwrap();
        let storage = Storage::open(&root.join("store"), "team-3").unwrap();
        let item = storage.put(&file, "a.txt", "", &[], "@amy").unwrap();
        storage.set_trashed(&item.id, true).unwrap();
        assert!(storage.index().items[0].trashed);
        storage.set_trashed(&item.id, false).unwrap();
        assert!(!storage.index().items[0].trashed);
        storage.set_trashed(&item.id, true).unwrap();
        assert_eq!(storage.empty_trash().unwrap(), 1);
        assert!(storage.index().items.is_empty());
    }
}

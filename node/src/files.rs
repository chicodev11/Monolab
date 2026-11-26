//! Files inside a server folder, with the app's rules (`src-tauri/src/paths.rs`, `browse_server_files`,
//! `read_editable_file`, `save_editable_file`): no `..`, no absolute paths, and the real path (links
//! followed) must stay inside the server folder.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

/// Text files larger than this aren't opened or saved, as in the app.
pub const MAX_TEXT: u64 = 8 * 1024 * 1024;
const BROWSE_PAGE: usize = 500;

#[derive(Debug, PartialEq, Eq)]
pub enum PathError {
    Escape,
    Invalid,
    Missing,
}

impl std::fmt::Display for PathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PathError::Escape => "Files outside the server folder cannot be accessed.",
            PathError::Invalid => "The file path is malformed.",
            PathError::Missing => "That location could not be found.",
        })
    }
}

fn bad_component(name: &str) -> bool {
    if name.is_empty() || name.contains('\0') {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    lower.contains("%2e") || lower.contains("%2f") || lower.contains("%5c")
}

/// A relative path made of plain names only. Absolute paths and `..` are escapes.
pub fn check_relative(relative: &str) -> Result<PathBuf, PathError> {
    let trimmed = relative.trim();
    if trimmed.starts_with(['/', '\\']) || trimmed.starts_with('~') {
        return Err(PathError::Escape);
    }
    let trimmed = trimmed.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Err(PathError::Invalid);
    }
    let mut out = PathBuf::new();
    for component in Path::new(&trimmed.replace('\\', "/")).components() {
        match component {
            Component::Normal(name) => {
                let name = name.to_str().ok_or(PathError::Invalid)?;
                if bad_component(name) {
                    return Err(PathError::Invalid);
                }
                out.push(name);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return Err(PathError::Escape),
        }
    }
    if out.as_os_str().is_empty() {
        return Err(PathError::Invalid);
    }
    Ok(out)
}

/// The real path of an existing file or folder in the server folder. Links are followed, and where
/// they lead must still be inside.
pub fn resolve_in_server(root: &Path, relative: &str) -> Result<PathBuf, PathError> {
    let root = root.canonicalize().map_err(|_| PathError::Missing)?;
    let relative = check_relative(relative)?;
    let candidate = root.join(&relative).canonicalize().map_err(|_| PathError::Missing)?;
    if !candidate.starts_with(&root) {
        return Err(PathError::Escape);
    }
    Ok(candidate)
}

/// Whether the editor may open a file, by its extension (the app's list).
pub fn editable_file(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|one| one.to_str()) else { return true };
    !matches!(
        extension.to_ascii_lowercase().as_str(),
        "jar"
            | "zip"
            | "gz"
            | "7z"
            | "rar"
            | "exe"
            | "dll"
            | "mca"
            | "dat"
            | "dat_old"
            | "nbt"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "ico"
            | "mp3"
            | "ogg"
            | "wav"
            | "ttf"
            | "otf"
            | "woff"
            | "woff2"
            | "pdf"
            | "db"
            | "mv"
            | "lock"
            | "class"
            | "so"
            | "dylib"
            | "bin"
            | "hprof"
            | "jfr"
            | "pyc"
            | "tar"
            | "xz"
            | "bz2"
            | "zst"
            | "mp4"
            | "webm"
            | "pump"
            | "mcr"
    )
}

/// Bytes that aren't text (NUL early on, or mostly undecodable).
pub fn looks_binary(text: &str) -> bool {
    let head: Vec<char> = text.chars().take(8192).collect();
    let broken = head.iter().filter(|one| **one == '\u{FFFD}').count();
    head.contains(&'\0') || broken * 20 > head.len().max(1)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub editable: bool,
    pub modified_at: u64,
    pub item_count: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePage {
    pub entries: Vec<BrowseEntry>,
    pub total: u64,
    pub offset: u64,
}

fn escape_text(error: PathError) -> String {
    match error {
        PathError::Escape => "Cannot browse outside the server folder.".into(),
        other => other.to_string(),
    }
}

/// One page of a folder, like the app's `browse_server_files`. `""` (or `/`) is the server folder.
pub fn browse(root: &Path, path: &str, offset: Option<u64>, limit: Option<u64>) -> Result<BrowsePage, String> {
    let root = root.canonicalize().map_err(|error| format!("Server folder not found: {error}"))?;
    let requested = path.trim();
    let target =
        if requested.is_empty() || requested == "/" { root.clone() } else { resolve_in_server(&root, requested).map_err(escape_text)? };
    if !target.is_dir() {
        return Err("Not a folder.".into());
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(&target).map_err(|error| error.to_string())? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else { continue };
        if name.starts_with(".monolab-") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        entries.push(BrowseEntry {
            name: name.to_string(),
            path: path.strip_prefix(&root).unwrap_or(&path).to_string_lossy().into_owned(),
            is_dir: meta.is_dir(),
            size_bytes: if meta.is_dir() { 0 } else { meta.len() },
            editable: !meta.is_dir() && editable_file(&path),
            modified_at: meta.modified().ok().and_then(|when| when.duration_since(UNIX_EPOCH).ok()).map(|gap| gap.as_secs()).unwrap_or(0),
            item_count: 0,
        });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    let total = entries.len() as u64;
    let offset = (offset.unwrap_or(0) as usize).min(entries.len());
    let limit = (limit.unwrap_or(BROWSE_PAGE as u64) as usize).clamp(1, BROWSE_PAGE);
    let mut page: Vec<BrowseEntry> = entries.into_iter().skip(offset).take(limit).collect();
    for entry in page.iter_mut().filter(|entry| entry.is_dir) {
        entry.item_count = fs::read_dir(root.join(&entry.path)).map(|list| list.count() as u64).unwrap_or(0);
    }
    Ok(BrowsePage { entries: page, total, offset: offset as u64 })
}

/// An existing text file the editor may open (the app's `safe_editable_path`).
fn editable_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = resolve_in_server(root, relative).map_err(|error| match error {
        PathError::Missing => "File not found.".to_string(),
        other => other.to_string(),
    })?;
    if !path.is_file() || !editable_file(&path) {
        return Err("Only text files can be opened.".into());
    }
    if fs::metadata(&path).map_err(|error| error.to_string())?.len() > MAX_TEXT {
        return Err("Files larger than 8 MB cannot be opened in the editor.".into());
    }
    Ok(path)
}

pub fn read_text(root: &Path, relative: &str) -> Result<String, String> {
    let path = editable_path(root, relative)?;
    let bytes = fs::read(path).map_err(|error| format!("Could not read the text file: {error}"))?;
    let text = String::from_utf8(bytes).map_err(|_| "This file isn't text, so it can't be opened in the editor.".to_string())?;
    if looks_binary(&text) {
        return Err("This file isn't text, so it can't be opened in the editor.".into());
    }
    Ok(text)
}

/// Saves over an existing text file (new files aren't made here, as in the app's editor).
pub fn save_text(root: &Path, relative: &str, content: &str) -> Result<(), String> {
    if content.len() as u64 > MAX_TEXT {
        return Err("Content larger than 8 MB cannot be saved.".into());
    }
    let path = editable_path(root, relative)?;
    fs::write(path, content).map_err(|error| format!("Could not save the text file: {error}"))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A fresh folder for one test.
    pub fn sandbox(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("monolab-node-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn relative_paths_cant_climb_out() {
        assert_eq!(check_relative("../x"), Err(PathError::Escape));
        assert_eq!(check_relative("plugins/../../x"), Err(PathError::Escape));
        assert_eq!(check_relative("..\\x"), Err(PathError::Escape));
        assert_eq!(check_relative("/etc/passwd"), Err(PathError::Escape));
        assert_eq!(check_relative("\\etc\\passwd"), Err(PathError::Escape));
        assert_eq!(check_relative("~/x"), Err(PathError::Escape));
        assert_eq!(check_relative("%2e%2e/x"), Err(PathError::Invalid));
        assert_eq!(check_relative(""), Err(PathError::Invalid));
        assert_eq!(check_relative("./plugins/a.yml").unwrap(), PathBuf::from("plugins/a.yml"));
    }

    #[test]
    fn links_out_of_the_server_folder_are_refused() {
        let base = sandbox("links");
        let root = base.join("server");
        let outside = base.join("outside");
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "no").unwrap();
        fs::write(root.join("server.properties"), "motd=hi\n").unwrap();
        std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("plugins/leak.txt")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("away")).unwrap();
        std::os::unix::fs::symlink(root.join("server.properties"), root.join("plugins/props.txt")).unwrap();

        assert_eq!(resolve_in_server(&root, "plugins/leak.txt"), Err(PathError::Escape));
        assert_eq!(resolve_in_server(&root, "away/secret.txt"), Err(PathError::Escape));
        assert!(read_text(&root, "plugins/leak.txt").unwrap_err().contains("outside"));
        assert!(save_text(&root, "away/secret.txt", "yes").unwrap_err().contains("outside"));
        assert_eq!(fs::read_to_string(outside.join("secret.txt")).unwrap(), "no");
        assert!(browse(&root, "away", None, None).unwrap_err().contains("outside"));
        assert!(browse(&root, "../outside", None, None).unwrap_err().contains("outside"));
        assert!(read_text(&root, "/etc/hostname").unwrap_err().contains("outside"));
        // A link that stays inside is fine.
        assert_eq!(read_text(&root, "plugins/props.txt").unwrap(), "motd=hi\n");
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn browse_read_and_save() {
        let root = sandbox("browse");
        fs::create_dir_all(root.join("plugins/x")).unwrap();
        fs::create_dir_all(root.join(".monolab-trash")).unwrap();
        fs::write(root.join("server.properties"), "motd=hi\n").unwrap();
        fs::write(root.join("server.jar"), [0u8, 1, 2]).unwrap();
        let page = browse(&root, "", None, None).unwrap();
        let names: Vec<&str> = page.entries.iter().map(|one| one.name.as_str()).collect();
        assert_eq!(names, ["plugins", "server.jar", "server.properties"]);
        assert_eq!(page.entries[0].item_count, 1);
        assert!(!page.entries[1].editable && page.entries[2].editable);
        assert_eq!(browse(&root, "plugins", None, None).unwrap().entries[0].path, "plugins/x");
        save_text(&root, "server.properties", "motd=new\n").unwrap();
        assert_eq!(read_text(&root, "server.properties").unwrap(), "motd=new\n");
        assert!(read_text(&root, "server.jar").is_err());
        assert!(save_text(&root, "new.txt", "x").unwrap_err().contains("not found"));
        fs::remove_dir_all(&root).unwrap();
    }
}

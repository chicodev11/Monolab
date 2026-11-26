//! Where Monolab keeps its own files: `<home>/.monolab` (`.monolab-<profile>` for a dev profile).
//! Older versions used Tauri's app data folder (`%APPDATA%\com.monolab.desktop`). The first start of
//! this version moves that folder over once, or copies it when it can't be moved (another drive, a
//! locked file), and then points the absolute paths kept in its JSON files at the new place.
//! Tauri's own webview data stays where Tauri puts it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::Manager;

const IDENTIFIER: &str = "com.monolab.desktop";

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// `app.path().monolab_data_dir()` stands where `app.path().app_data_dir()` used to.
pub trait MonolabDirs {
    fn monolab_data_dir(&self) -> tauri::Result<PathBuf>;
}

impl<R: tauri::Runtime> MonolabDirs for tauri::path::PathResolver<R> {
    fn monolab_data_dir(&self) -> tauri::Result<PathBuf> {
        if let Some(root) = ROOT.get() {
            return Ok(root.clone());
        }
        let old = self.app_data_dir()?;
        let home = self.home_dir()?;
        Ok(ROOT.get_or_init(|| settle(&old, &home)).clone())
    }
}

/// Settles the folder (moving the old one if needed) before anything reads it.
pub fn init(app: &tauri::AppHandle) {
    if let Err(error) = app.path().monolab_data_dir() {
        eprintln!("Monolab: could not find the data folder: {error}");
    }
}

/// The data folder without an app handle (a panic hook, live tests): settled from the environment
/// when the app hasn't settled it yet.
pub fn current() -> Option<PathBuf> {
    if let Some(root) = ROOT.get() {
        return Some(root.clone());
    }
    let old = env_old_dir()?;
    let home = env_home()?;
    Some(ROOT.get_or_init(|| settle(&old, &home)).clone())
}

/// `com.monolab.desktop` → `.monolab`; a dev profile `com.monolab.desktop.b` → `.monolab-b`.
fn folder_name(old: &Path) -> String {
    let name = old.file_name().and_then(|name| name.to_str()).unwrap_or(IDENTIFIER);
    match name.strip_prefix(IDENTIFIER) {
        Some("") => ".monolab".into(),
        Some(rest) => format!(".monolab-{}", rest.trim_start_matches('.')),
        None => format!(".monolab-{name}"),
    }
}

/// Files that only Monolab's own data folder has; a folder with one of them is already settled.
const MARKERS: [&str; 3] = [".monolab-data", "settings.json", "instances.json"];

fn holds_data(dir: &Path) -> bool {
    MARKERS.iter().any(|name| dir.join(name).exists())
}

fn mark(dir: &Path) {
    let _ = fs::write(dir.join(MARKERS[0]), "Monolab's data folder.\n");
}

/// Picks the folder, moving or copying the old one over the first time. The new folder may already
/// exist with other files in it (an updater key, say); then the old folder's entries are moved in
/// one by one and nothing already there is replaced.
fn settle(old: &Path, home: &Path) -> PathBuf {
    let new = home.join(folder_name(old));
    if holds_data(&new) || !old.is_dir() {
        if let Err(error) = fs::create_dir_all(&new) {
            eprintln!("Monolab: could not create {}: {error}", new.display());
        }
        return new;
    }
    if !new.exists() {
        if fs::rename(old, &new).is_ok() {
            rewrite_json_files(&new, old, &new);
            mark(&new);
            note(&new, &format!("Moved from {} to {}.", old.display(), new.display()));
            return new;
        }
        if let Err(error) = fs::create_dir_all(&new) {
            eprintln!("Monolab: could not create {} ({error}); still using {}", new.display(), old.display());
            return old.to_path_buf();
        }
    }
    match merge(old, &new) {
        Ok(report) => {
            rewrite_json_files(&new, old, &new);
            mark(&new);
            let mut text = format!("Moved from {} to {}.", old.display(), new.display());
            if !report.copied.is_empty() {
                text.push_str(" Some entries were copied instead of moved; the old folder was left in place and can be deleted.");
            }
            for one in report.kept {
                text.push_str(&format!("\nAlready in the new folder, left alone: {one}"));
            }
            for one in report.skipped {
                text.push_str(&format!("\nNot copied: {one}"));
            }
            if report.copied.is_empty() {
                // Empty now unless something was left alone or skipped; then it stays.
                let _ = fs::remove_dir(old);
            }
            note(&new, &text);
            new
        }
        Err(error) => {
            eprintln!("Monolab: could not move {} into {} ({error}); still using it", old.display(), new.display());
            old.to_path_buf()
        }
    }
}

#[derive(Default)]
struct Merged {
    copied: Vec<String>,
    kept: Vec<String>,
    skipped: Vec<String>,
}

/// Moves each entry of `old` into `new` (copying when a move fails), never over an entry `new` has.
fn merge(old: &Path, new: &Path) -> std::io::Result<Merged> {
    let mut report = Merged::default();
    for entry in fs::read_dir(old)? {
        let entry = entry?;
        let from = entry.path();
        let target = new.join(entry.file_name());
        if target.exists() {
            report.kept.push(target.display().to_string());
            continue;
        }
        if fs::rename(&from, &target).is_ok() {
            continue;
        }
        report.copied.push(from.display().to_string());
        if entry.file_type()?.is_dir() {
            // Copied beside first, so a copy cut short never looks finished.
            let partial = new.join(format!("{}.partial", entry.file_name().to_string_lossy()));
            let _ = fs::remove_dir_all(&partial);
            copy_tree(&from, &partial, &mut report.skipped).and_then(|()| fs::rename(&partial, &target))?;
        } else if let Err(error) = fs::copy(&from, &target) {
            report.skipped.push(format!("{} ({error})", from.display()));
        }
    }
    Ok(report)
}

/// Leaves a line about the move in `logs/data-folder.txt`.
fn note(root: &Path, text: &str) {
    eprintln!("Monolab: {text}");
    let logs = root.join("logs");
    if fs::create_dir_all(&logs).is_ok() {
        let _ = fs::write(logs.join("data-folder.txt"), format!("{text}\n"));
    }
}

/// Copies a folder; a file that can't be read (held open by something) is skipped and listed.
fn copy_tree(from: &Path, to: &Path, skipped: &mut Vec<String>) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target, skipped)?;
        } else if kind.is_file() {
            if let Err(error) = fs::copy(entry.path(), &target) {
                skipped.push(format!("{} ({error})", entry.path().display()));
            }
        } else {
            skipped.push(format!("{} (a link)", entry.path().display()));
        }
    }
    Ok(())
}

/// Points the paths in the folder's own JSON (and text) files at the new place, e.g. a Java runtime.
fn rewrite_json_files(root: &Path, old: &Path, new: &Path) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let wanted = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json") || ext.eq_ignore_ascii_case("txt"));
        if !wanted || !path.is_file() {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let changed = rewrite_paths(&text, old, new);
        if changed != text {
            if let Err(error) = fs::write(&path, changed) {
                eprintln!("Monolab: could not update {}: {error}", path.display());
            }
        }
    }
}

/// Replaces the old folder's path with the new one in all three spellings a file can hold it:
/// as is (`C:\a\b`), JSON-escaped (`C:\\a\\b`) and with forward slashes (`C:/a/b`). A longer
/// name that only starts with the old one (`com.monolab.desktop.b`) is left alone.
pub fn rewrite_paths(text: &str, old: &Path, new: &Path) -> String {
    let old = old.to_string_lossy().trim_end_matches(['\\', '/']).to_string();
    let new = new.to_string_lossy().trim_end_matches(['\\', '/']).to_string();
    if old.is_empty() {
        return text.to_string();
    }
    let forms = [
        (old.replace('\\', "\\\\"), new.replace('\\', "\\\\")),
        (old.clone(), new.clone()),
        (old.replace('\\', "/"), new.replace('\\', "/")),
    ];
    let mut out = text.to_string();
    let mut done: Vec<&str> = Vec::new();
    for (from, to) in &forms {
        if done.contains(&from.as_str()) {
            continue;
        }
        done.push(from);
        out = replace_whole(&out, from, to);
    }
    out
}

fn replace_whole(text: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(from) {
        let end = at + from.len();
        let next = rest[end..].chars().next();
        let whole = !matches!(next, Some(letter) if letter.is_alphanumeric() || matches!(letter, '.' | '-' | '_'));
        out.push_str(&rest[..at]);
        out.push_str(if whole { to } else { from });
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn env_home() -> Option<PathBuf> {
    let first = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(first).or_else(|| std::env::var_os("HOME")).map(PathBuf::from).filter(|path| !path.as_os_str().is_empty())
}

/// Tauri's app data folder, worked out the way Tauri does, for when there's no app handle.
fn env_old_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?)
    } else if cfg!(target_os = "macos") {
        env_home()?.join("Library").join("Application Support")
    } else {
        match std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => env_home()?.join(".local").join("share"),
        }
    };
    Some(base.join(identifier()))
}

/// The identifier `app_context` in lib.rs gives the app (a dev profile adds `.<name>`).
fn identifier() -> String {
    let profile = cfg!(debug_assertions)
        .then(|| std::env::var("MONOLAB_PROFILE").ok())
        .flatten()
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty() && name.len() <= 16 && name.chars().all(|letter| letter.is_ascii_alphanumeric()));
    match profile {
        Some(name) => format!("{IDENTIFIER}.{name}"),
        None => IDENTIFIER.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_paths_in_every_spelling() {
        let old = Path::new(r"C:\Users\me\AppData\Roaming\com.monolab.desktop");
        let new = Path::new(r"C:\Users\me\.monolab");
        let text = r#"{"java":"C:\\Users\\me\\AppData\\Roaming\\com.monolab.desktop\\java\\jdk-21\\bin\\java.exe","slash":"C:/Users/me/AppData/Roaming/com.monolab.desktop/forks","root":"C:\\Users\\me\\AppData\\Roaming\\com.monolab.desktop","other":"C:\\Users\\me\\AppData\\Roaming\\com.monolab.desktop.b\\x","servers":"C:\\Users\\me\\AppData\\Roaming\\Monolab\\servers"}"#;
        let out = rewrite_paths(text, old, new);
        assert_eq!(
            out,
            r#"{"java":"C:\\Users\\me\\.monolab\\java\\jdk-21\\bin\\java.exe","slash":"C:/Users/me/.monolab/forks","root":"C:\\Users\\me\\.monolab","other":"C:\\Users\\me\\AppData\\Roaming\\com.monolab.desktop.b\\x","servers":"C:\\Users\\me\\AppData\\Roaming\\Monolab\\servers"}"#
        );
        let raw = r"C:\Users\me\AppData\Roaming\com.monolab.desktop\servers";
        assert_eq!(rewrite_paths(raw, old, new), r"C:\Users\me\.monolab\servers");
        assert_eq!(rewrite_paths("nothing here", old, new), "nothing here");
    }

    #[test]
    fn moves_into_a_folder_that_already_has_other_files() {
        let base = std::env::temp_dir().join(format!("monolab-data-root-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let old = base.join("roaming").join("com.monolab.desktop");
        let home = base.join("home");
        fs::create_dir_all(old.join("servers")).unwrap();
        fs::write(old.join("settings.json"), "{}").unwrap();
        fs::write(old.join("servers").join("a.txt"), "a").unwrap();
        fs::write(old.join("updater.key"), "old").unwrap();
        fs::create_dir_all(home.join(".monolab")).unwrap();
        fs::write(home.join(".monolab").join("updater.key"), "keep").unwrap();
        let new = settle(&old, &home);
        assert_eq!(new, home.join(".monolab"));
        assert!(new.join("settings.json").is_file());
        assert_eq!(fs::read_to_string(new.join("servers").join("a.txt")).unwrap(), "a");
        assert_eq!(fs::read_to_string(new.join("updater.key")).unwrap(), "keep");
        // The old folder kept only what the new one already had.
        assert!(old.join("updater.key").is_file());
        assert!(!old.join("settings.json").exists());
        // A second start leaves everything as it is.
        assert_eq!(settle(&old, &home), new);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn names_the_folder_after_the_profile() {
        assert_eq!(folder_name(Path::new("/x/com.monolab.desktop")), ".monolab");
        assert_eq!(folder_name(Path::new("/x/com.monolab.desktop.b")), ".monolab-b");
    }
}

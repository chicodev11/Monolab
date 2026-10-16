
use std::path::{Component, Path, PathBuf};

use crate::plain_path;

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

impl From<PathError> for String {
    fn from(error: PathError) -> Self {
        error.to_string()
    }
}

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn bad_component(name: &str) -> bool {
    if name.is_empty() || name.ends_with('.') || name.ends_with(' ') {
        return true;
    }
    if name.contains('\0') || name.contains(':') {
        return true;
    }
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    lower.contains("%2e") || lower.contains("%2f") || lower.contains("%5c")
}

pub fn check_relative(relative: &str) -> Result<PathBuf, PathError> {
    let trimmed = relative.trim().trim_matches(['/', '\\']);
    if trimmed.is_empty() {
        return Err(PathError::Invalid);
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err(PathError::Escape);
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => {
                let name = name.to_str().ok_or(PathError::Invalid)?;
                if bad_component(name) {
                    return Err(PathError::Invalid);
                }
                out.push(name);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(PathError::Escape);
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(PathError::Invalid);
    }
    Ok(out)
}

pub fn resolve_in_server(root: &Path, relative: &str) -> Result<PathBuf, PathError> {
    let root = root.canonicalize().map(plain_path).map_err(|_| PathError::Missing)?;
    let rel = check_relative(relative)?;
    let candidate = root
        .join(&rel)
        .canonicalize()
        .map(plain_path)
        .map_err(|_| PathError::Missing)?;
    if !candidate.starts_with(&root) {
        return Err(PathError::Escape);
    }
    Ok(candidate)
}

pub fn resolve_new_in_server(root: &Path, relative: &str) -> Result<PathBuf, PathError> {
    let root = root.canonicalize().map(plain_path).map_err(|_| PathError::Missing)?;
    let rel = check_relative(relative)?;
    let joined = root.join(&rel);
    if joined.exists() {
        let real = joined.canonicalize().map(plain_path).map_err(|_| PathError::Missing)?;
        return if real.starts_with(&root) { Ok(real) } else { Err(PathError::Escape) };
    }
    let parent = joined
        .parent()
        .ok_or(PathError::Invalid)?
        .canonicalize()
        .map(plain_path)
        .map_err(|_| PathError::Missing)?;
    if !parent.starts_with(&root) {
        return Err(PathError::Escape);
    }
    Ok(parent.join(joined.file_name().ok_or(PathError::Invalid)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sandbox() -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("se-paths-{}-{}", std::process::id(), unix()));
        let root = base.join("server");
        fs::create_dir_all(root.join("plugins")).unwrap();
        fs::create_dir_all(base.join("outside")).unwrap();
        fs::write(root.join("plugins").join("a.jar"), b"x").unwrap();
        fs::write(base.join("outside").join("secret.txt"), b"s").unwrap();
        (base, root)
    }

    fn unix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn rejects_every_known_escape() {
        let (base, root) = sandbox();
        let escapes = [
            "../outside/secret.txt",
            "..\\outside\\secret.txt",
            "plugins/../../outside/secret.txt",
            "plugins\\..\\..\\outside\\secret.txt",
            "C:\\Windows\\system.ini",
            "C:outside",
        ];
        for bad in escapes {
            assert_eq!(
                resolve_in_server(&root, bad),
                Err(PathError::Escape),
                "should be blocked: {bad}"
            );
        }
        let invalid = [
            "%2e%2e%2fsecret.txt",
            "plugins/%2E%2E/x",
            "plugins/a%2fb",
            "....//outside/secret.txt",
            "...",
            "CON",
            "nul.txt",
            "plugins/COM1.jar",
            "plugins/a.jar.",
            "plugins /a.jar",
            "",
            "   ",
            "plugins/a:b",
            "\\\\?\\C:\\Windows\\system.ini",
        ];
        for bad in invalid {
            assert_eq!(
                resolve_in_server(&root, bad),
                Err(PathError::Invalid),
                "should be blocked by format: {bad:?}"
            );
        }
        let real = root.join("plugins").join("a.jar").canonicalize().map(plain_path).unwrap();
        for good in ["plugins/a.jar", "plugins\\a.jar", "./plugins/a.jar", "/plugins/a.jar"] {
            assert_eq!(resolve_in_server(&root, good), Ok(real.clone()), "{good}");
        }
        assert_eq!(resolve_in_server(&root, "plugins/none.jar"), Err(PathError::Missing));
        assert_eq!(resolve_in_server(&root, "/etc/passwd"), Err(PathError::Missing));
        assert_eq!(resolve_in_server(&root, "\\outside\\secret.txt"), Err(PathError::Missing));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn follows_symlinks_out_of_the_server() {
        let (base, root) = sandbox();
        let link = root.join("plugins").join("sneaky");
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(base.join("outside"), &link).is_ok();
        #[cfg(not(windows))]
        let made = std::os::unix::fs::symlink(base.join("outside"), &link).is_ok();
        if made {
            assert_eq!(
                resolve_in_server(&root, "plugins/sneaky/secret.txt"),
                Err(PathError::Escape)
            );
            assert_eq!(
                resolve_new_in_server(&root, "plugins/sneaky/new.txt"),
                Err(PathError::Escape)
            );
        }
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn new_files_need_a_parent_inside() {
        let (base, root) = sandbox();
        let made = resolve_new_in_server(&root, "plugins/new.jar").unwrap();
        assert!(made.starts_with(root.canonicalize().map(plain_path).unwrap()));
        assert_eq!(made.file_name().unwrap(), "new.jar");
        assert_eq!(resolve_new_in_server(&root, "nope/new.jar"), Err(PathError::Missing));
        assert_eq!(resolve_new_in_server(&root, "../new.jar"), Err(PathError::Escape));
        assert_eq!(resolve_new_in_server(&root, "plugins/NUL"), Err(PathError::Invalid));
        let _ = fs::remove_dir_all(base);
    }
}

#[cfg(test)]
mod guard {
    #[test]
    fn nobody_checks_paths_by_string_again() {
        for file in ["lib.rs", "logs.rs", "backups.rs", "web/api.rs"] {
            let text = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file),
            )
            .unwrap();
            assert!(
                !text.contains("contains(\"..\")"),
                "{file}: use paths::resolve_in_server instead of checking for `..`."
            );
        }
    }
}

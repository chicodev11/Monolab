//! ACP agents the registry ships as downloads (`distribution.binary.<platform>`): fetched only when the
//! user presses Download, checked against the registry's sha256 when it gives one, and unpacked into
//! `<data>/acp-agents/<agent>/<version>/`. Older versions of the same agent are removed after.

use crate::data_root::MonolabDirs;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const DONE: &str = ".monolab-installed";
/// Agent downloads are a few hundred MB at most; anything bigger is refused.
const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;

#[derive(Serialize, Clone, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AcpInstallProgress {
    pub agent: String,
    /// Bytes so far and in all (0 when the server didn't say).
    pub done: f64,
    pub total: f64,
    /// `download`, `unpack` or `done`.
    pub step: String,
}

pub fn root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().monolab_data_dir().map_err(|error| error.to_string())?.join("acp-agents"))
}

fn safe(name: &str) -> String {
    let out: String = name.chars().map(|one| if one.is_ascii_alphanumeric() || one == '-' || one == '_' || one == '.' { one } else { '_' }).collect();
    out.trim_matches('.').to_string()
}

pub fn folder(root: &Path, agent: &str, version: &str) -> PathBuf {
    root.join(safe(agent)).join(if version.is_empty() { "current".to_string() } else { safe(version) })
}

pub fn installed(root: &Path, agent: &str, version: &str) -> bool {
    folder(root, agent, version).join(DONE).is_file()
}

/// The program inside the agent's folder; `cmd` must not leave it.
pub fn program(dir: &Path, cmd: &str) -> Result<PathBuf, String> {
    let relative = cmd.trim().replace('\\', "/");
    let relative = relative.trim_start_matches("./");
    let path = Path::new(relative);
    if relative.is_empty() || path.is_absolute() || path.components().any(|one| !matches!(one, Component::Normal(_) | Component::CurDir)) {
        return Err("The agent list gives a strange program path for this agent.".into());
    }
    Ok(dir.join(path))
}

enum Kind {
    Zip,
    TarGz,
    Plain(String),
}

fn kind_of(url: &str, cmd: &str) -> Kind {
    let path = reqwest::Url::parse(url).map(|one| one.path().to_ascii_lowercase()).unwrap_or_default();
    if path.ends_with(".zip") {
        Kind::Zip
    } else if path.ends_with(".tar.gz") || path.ends_with(".tgz") {
        Kind::TarGz
    } else {
        // A bare program: saved under the name the registry runs it by.
        let name = cmd.trim().replace('\\', "/");
        Kind::Plain(name.rsplit('/').next().unwrap_or("agent").to_string())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|one| format!("{one:02x}")).collect()
}

fn unpack_zip(file: &Path, into: &Path) -> Result<(), String> {
    let reader = std::fs::File::open(file).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipArchive::new(reader).map_err(|error| format!("The download isn't a valid zip: {error}"))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        // `enclosed_name` drops entries that would land outside the folder.
        let Some(name) = entry.enclosed_name() else { continue };
        let target = into.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut out = std::fs::File::create(&target).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o755));
        }
    }
    Ok(())
}

fn unpack_tar_gz(file: &Path, into: &Path) -> Result<(), String> {
    let reader = std::fs::File::open(file).map_err(|error| error.to_string())?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(reader));
    for entry in archive.entries().map_err(|error| format!("The download isn't a valid archive: {error}"))? {
        let mut entry = entry.map_err(|error| error.to_string())?;
        // `unpack_in` refuses paths that climb out of the folder; links are skipped altogether.
        if entry.header().entry_type().is_symlink() || entry.header().entry_type().is_hard_link() {
            continue;
        }
        entry.unpack_in(into).map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Downloads and unpacks the agent's build for this computer. Progress goes out as `acp-install` events.
pub fn install(app: &AppHandle, registry: &Value, agent: &str) -> Result<(), String> {
    let entry = registry
        .get("agents")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().find(|one| one.get("id").and_then(Value::as_str) == Some(agent)))
        .ok_or("That agent isn't in the agent list.")?;
    let version = entry.get("version").and_then(Value::as_str).unwrap_or_default().to_string();
    let build = entry
        .pointer("/distribution/binary")
        .and_then(|one| one.get(crate::acp::platform()))
        .ok_or("This agent has no download for this computer.")?;
    let url = build.get("archive").and_then(Value::as_str).unwrap_or_default();
    let cmd = build.get("cmd").and_then(Value::as_str).unwrap_or_default();
    if !url.starts_with("https://") {
        return Err("This agent's download isn't over a secure connection, so Monolab won't fetch it.".into());
    }
    let expected = build.get("sha256").and_then(Value::as_str).map(str::to_ascii_lowercase);
    let root = root(app)?;
    let dir = folder(&root, agent, &version);
    program(&dir, cmd)?;

    let tell = |done: u64, total: u64, step: &str| {
        let _ = app.emit("acp-install", AcpInstallProgress { agent: agent.to_string(), done: done as f64, total: total as f64, step: step.to_string() });
    };
    let client = crate::net::builder()
        .user_agent("Monolab")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30 * 60))
        .build()
        .map_err(|error| error.to_string())?;
    let mut response = client.get(url).send().and_then(|one| one.error_for_status()).map_err(|error| format!("Could not download the agent: {error}"))?;
    let total = response.content_length().unwrap_or(0);
    if total > MAX_DOWNLOAD {
        return Err("This agent's download is too big.".into());
    }
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let part = root.join(format!("{}.download", safe(agent)));
    let mut out = std::fs::File::create(&part).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    let mut told = std::time::Instant::now();
    loop {
        let read = response.read(&mut buffer).map_err(|error| format!("The download broke off: {error}"))?;
        if read == 0 {
            break;
        }
        done += read as u64;
        if done > MAX_DOWNLOAD {
            let _ = std::fs::remove_file(&part);
            return Err("This agent's download is too big.".into());
        }
        hasher.update(&buffer[..read]);
        out.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
        if told.elapsed() > Duration::from_millis(200) {
            tell(done, total, "download");
            told = std::time::Instant::now();
        }
    }
    drop(out);
    let got = hex(&hasher.finalize());
    if let Some(expected) = expected.filter(|one| !one.is_empty()) {
        if expected != got {
            let _ = std::fs::remove_file(&part);
            return Err("The download doesn't match the agent list's checksum, so it was thrown away.".into());
        }
    }

    tell(done, total, "unpack");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let unpacked = match kind_of(url, cmd) {
        Kind::Zip => unpack_zip(&part, &dir),
        Kind::TarGz => unpack_tar_gz(&part, &dir),
        Kind::Plain(name) => std::fs::rename(&part, dir.join(name)).map_err(|error| error.to_string()),
    };
    let _ = std::fs::remove_file(&part);
    if let Err(error) = unpacked {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!("Could not unpack the agent: {error}"));
    }
    let program = program(&dir, cmd)?;
    if !program.is_file() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err("The download doesn't contain the agent's program.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755));
    }
    std::fs::write(dir.join(DONE), &got).map_err(|error| error.to_string())?;
    // Only the newest version of an agent is kept.
    if let Ok(siblings) = std::fs::read_dir(root.join(safe(agent))) {
        for one in siblings.flatten() {
            if one.path() != dir {
                let _ = std::fs::remove_dir_all(one.path());
            }
        }
    }
    tell(done, total, "done");
    Ok(())
}

/// Removes a downloaded agent (every version).
pub fn remove(app: &AppHandle, agent: &str) -> Result<(), String> {
    let dir = root(app)?.join(safe(agent));
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Could not remove the agent: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_paths_stay_in_the_folder() {
        let dir = PathBuf::from("C:/data/acp-agents/x/1");
        assert_eq!(program(&dir, "./bin\\x.exe").unwrap(), dir.join("bin/x.exe"));
        assert_eq!(program(&dir, "x.exe").unwrap(), dir.join("x.exe"));
        assert!(program(&dir, "../x.exe").is_err());
        assert!(program(&dir, "/usr/bin/x").is_err());
        assert!(program(&dir, "").is_err());
    }

    #[test]
    fn knows_the_archive_kind() {
        assert!(matches!(kind_of("https://x/a.zip", "a.exe"), Kind::Zip));
        assert!(matches!(kind_of("https://x/a-1.tar.gz", "a.exe"), Kind::TarGz));
        assert!(matches!(kind_of("https://x/dl/?file=a.exe", "./a.exe"), Kind::Plain(name) if name == "a.exe"));
        assert_eq!(safe("../x"), "_x");
        assert_eq!(folder(Path::new("r"), "a b", "1.0+2"), Path::new("r").join("a_b").join("1.0_2"));
    }

    #[test]
    fn zip_entries_cant_climb_out() {
        let base = std::env::temp_dir().join(format!("monolab-acp-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("into")).unwrap();
        let file = base.join("a.zip");
        {
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&file).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("bin/agent.exe", options).unwrap();
            zip.write_all(b"x").unwrap();
            zip.start_file("../escape.txt", options).unwrap();
            zip.write_all(b"x").unwrap();
            zip.finish().unwrap();
        }
        unpack_zip(&file, &base.join("into")).unwrap();
        assert!(base.join("into/bin/agent.exe").is_file());
        assert!(!base.join("escape.txt").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}


use crate::{download_file_watched, hashes::verify_sha256, hidden_command, off_runtime};
use std::{fs, path::{Path, PathBuf}, time::Duration};
use tauri::{AppHandle, Emitter, Manager};
use zip::ZipArchive;

pub(crate) fn required_java_major(version: &str) -> u32 {
    let parts = version
        .trim_start_matches('v')
        .split('.')
        .filter_map(|part| part.parse::<u32>().ok())
        .collect::<Vec<_>>();
    let first = parts.first().copied().unwrap_or_default();
    if first >= 26 {
        25
    } else {
        let minor = if first == 1 {
            parts.get(1).copied().unwrap_or_default()
        } else {
            first
        };
        let patch = if first == 1 {
            parts.get(2).copied().unwrap_or_default()
        } else {
            parts.get(1).copied().unwrap_or_default()
        };
        if minor >= 21 || (minor == 20 && patch >= 5) {
            21
        } else if minor >= 18 {
            17
        } else if minor == 17 {
            16
        } else {
            8
        }
    }
}

pub(crate) fn required_java_major_for(core: &str, version: &str) -> u32 {
    if core == "Velocity" {
        25
    } else {
        required_java_major(version)
    }
}

pub(crate) fn java_major_from_output(output: &[u8]) -> Option<u32> {
    let text = String::from_utf8_lossy(output);
    let quoted = text.split('"').nth(1)?;
    let mut parts = quoted.split('.');
    let first = parts.next()?.parse::<u32>().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

pub(crate) const MADE_BY_US: &str = "Downloaded by Monolab";

fn runtime_folder_of(root: &Path, executable: &str) -> Result<(PathBuf, u32), String> {
    let path = PathBuf::from(executable);
    let inside = path
        .ancestors()
        .find(|one| one.parent() == Some(root))
        .ok_or("This Java was not downloaded by Monolab and cannot be removed here.")?
        .to_path_buf();
    let major = inside
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.rsplit('-').next())
        .and_then(|number| number.parse::<u32>().ok())
        .unwrap_or(0);
    Ok((inside, major))
}

pub(crate) fn runtimes_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("runtimes"))
}

pub(crate) fn remove_runtime(app: &AppHandle, executable: &str) -> Result<u32, String> {
    let root = runtimes_dir(app)?;
    let (inside, major) = runtime_folder_of(&root, executable)?;
    fs::remove_dir_all(&inside).map_err(|error| {
        format!("Could not remove Java. Is a server still running on it? ({error})")
    })?;
    Ok(major)
}

fn runtime_root(app: &AppHandle, major: u32) -> Result<PathBuf, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("runtimes")
        .join(format!("temurin-{major}"));
    fs::create_dir_all(&root)
        .map_err(|error| format!("Could not create the Java runtime folder: {error}"))?;
    Ok(root)
}

fn find_java_executable(root: &Path) -> Option<PathBuf> {
    let direct = root.join("bin").join("java.exe");
    if direct.is_file() {
        return Some(direct);
    }
    fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find_map(|path| {
            let executable = path.join("bin").join("java.exe");
            executable.is_file().then_some(executable)
        })
}

fn extract_zip(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = fs::File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("Could not open the Java archive: {error}"))?;
    crate::zips::check_budget(&mut archive, u64::MAX, crate::zips::MAX_ENTRIES)?;
    let mut total_size = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        total_size = total_size.saturating_add(entry.size());
        if total_size > 1_500 * 1024 * 1024 {
            return Err("The extracted Java size exceeds the allowed limit.".into());
        }
        let Some(relative) = entry.enclosed_name()else {
            continue;
        };
        let output = destination.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(|error| error.to_string())?;
        } else {
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            let mut target = fs::File::create(&output).map_err(|error| error.to_string())?;
            std::io::copy(&mut entry, &mut target).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JavaRuntime {
    pub path: String,
    pub major: u32,
    pub source: String,
    pub managed: bool,
}

pub(crate) fn ask_java_major(executable: &Path) -> Option<u32> {
    let output = hidden_command(executable).arg("-version").output().ok()?;
    let text = if output.stderr.is_empty() {
        output.stdout
    } else {
        output.stderr
    };
    java_major_from_output(&text)
}

fn java_search_roots() -> Vec<(PathBuf, String)> {
    let mut roots: Vec<(PathBuf, String)> = Vec::new();
    let mut push = |path: PathBuf, label: &str| {
        if path.is_dir() {
            roots.push((path, label.to_string()));
        }
    };
    for (variable, label) in [
        ("ProgramFiles", "Program Files"),
        ("ProgramFiles(x86)", "Program Files (x86)"),
        ("LOCALAPPDATA", "User program"),
    ] {
        let Ok(base) = std::env::var(variable) else {
            continue;
        };
        let base = PathBuf::from(base);
        for folder in [
            "Java",
            "Eclipse Adoptium",
            "Eclipse Foundation",
            "AdoptOpenJDK",
            "Zulu",
            "Amazon Corretto",
            "BellSoft",
            "Microsoft",
            "Semeru",
            "RedHat",
            "Programs/Eclipse Adoptium",
            "Programs/Java",
        ] {
            push(base.join(folder), label);
        }
        push(base.join("Minecraft Launcher/runtime"), "Minecraft launcher");
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        push(PathBuf::from(appdata).join(".minecraft/runtime"), "Minecraft");
    }
    roots
}

fn collect_java_from(root: &Path, depth: u32, found: &mut Vec<PathBuf>) {
    let direct = root.join("bin").join("java.exe");
    if direct.is_file() {
        found.push(direct);
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_java_from(&path, depth - 1, found);
        }
    }
}

pub(crate) fn scan_installed_javas(app: &AppHandle) -> Vec<JavaRuntime> {
    let mut candidates: Vec<(PathBuf, String)> = Vec::new();

    if let Ok(home) = std::env::var("JAVA_HOME") {
        let executable = PathBuf::from(home).join("bin").join("java.exe");
        if executable.is_file() {
            candidates.push((executable, "JAVA_HOME".into()));
        }
    }
    for (root, label) in java_search_roots() {
        let mut found = Vec::new();
        collect_java_from(&root, 2, &mut found);
        for path in found {
            candidates.push((path, label.clone()));
        }
    }
    if let Ok(data_dir) = app.path().app_data_dir() {
        let mut found = Vec::new();
        collect_java_from(&data_dir.join("runtimes"), 2, &mut found);
        for path in found {
            candidates.push((path, MADE_BY_US.into()));
        }
    }
    if let Some(major) = ask_java_major(Path::new("java")) {
        let _ = major;
        candidates.push((PathBuf::from("java"), "PATH".into()));
    }

    let mut seen = std::collections::HashSet::new();
    let mut runtimes: Vec<JavaRuntime> = Vec::new();
    for (path, source) in candidates {
        let key = path.to_string_lossy().to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let Some(major) = ask_java_major(&path) else {
            continue;
        };
        runtimes.push(JavaRuntime {
            managed: source == MADE_BY_US,
            path: path.to_string_lossy().to_string(),
            major,
            source,
        });
    }
    runtimes.sort_by(|a, b| b.major.cmp(&a.major).then(a.path.cmp(&b.path)));
    runtimes
}

pub(crate) fn resolve_java_runtime(
    app: &AppHandle,
    core: &str,
    version: &str,
    java_path: &str,
) -> Result<PathBuf, String> {
    off_runtime(|| {
        if java_path != "java" {
            let path = PathBuf::from(java_path);
            if path.is_file() {
                crate::launch::verify_java(&path, ask_java_major)?;
                return Ok(path);
            }
        }
        let required = required_java_major_for(core, version);
        if let Ok(output) = hidden_command("java").arg("-version").output() {
            let version_output = if output.stderr.is_empty() {
                &output.stdout
            } else {
                &output.stderr
            };
            if java_major_from_output(version_output).is_some_and(|major| major >= required) {
                return Ok(PathBuf::from("java"));
            }
        }
        let root = runtime_root(app, required)?;
        if let Some(executable) = find_java_executable(&root) {
            return Ok(executable);
        }
        if let Some(found) = scan_installed_javas(app)
            .into_iter()
            .find(|runtime| runtime.major >= required)
        {
            return Ok(PathBuf::from(found.path));
        }
        let runtime = fetch_runtime(app, required)?;
        Ok(PathBuf::from(runtime.path))
    })
}

pub(crate) fn fetch_runtime(app: &AppHandle, major: u32) -> Result<JavaRuntime, String> {
    let result = off_runtime(|| {
        let root = runtime_root(app, major)?;
        if let Some(executable) = find_java_executable(&root) {
            return Ok(JavaRuntime {
                path: executable.to_string_lossy().to_string(),
                major,
                source: MADE_BY_US.into(),
                managed: true,
            });
        }
        let _ = app.emit(
            "java-download",
            serde_json::json!({ "stage": "downloading", "major": major }),
        );
        let client = crate::net::builder()
            .user_agent("Monolab/0.1.0 (local Windows desktop app)")
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|error| error.to_string())?;
        let endpoint = format!("https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture=x64&image_type=jdk&os=windows&vendor=eclipse");
        let releases: serde_json::Value = client
            .get(&endpoint)
            .send()
            .map_err(|error| format!("Could not fetch Java download info: {error}"))?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .map_err(|error| error.to_string())?;
        let release = releases
            .as_array()
            .and_then(|items| items.first())
            .ok_or_else(|| format!("No Java {major} runtime was found."))?;
        let url = release
            .pointer("/binary/package/link")
            .and_then(|value| value.as_str())
            .ok_or("The Java download URL is malformed.")?;
        let checksum = release
            .pointer("/binary/package/checksum")
            .and_then(|value| value.as_str())
            .ok_or("The Java download has no SHA‑256.")?;
        let archive = root.join("runtime.zip");
        let mut shown = u64::MAX;
        download_file_watched(&client, url, &archive, &mut |got, total| {
            let percent = total.filter(|all| *all > 0).map(|all| got * 100 / all).unwrap_or(0);
            if percent != shown {
                shown = percent;
                let _ = app.emit(
                    "java-download",
                    serde_json::json!({ "stage": "downloading", "major": major, "received": got, "total": total }),
                );
            }
        })?;
        if let Err(error) = verify_sha256(&archive, checksum) {
            let _ = fs::remove_file(&archive);
            return Err(error);
        }
        let _ = app.emit(
            "java-download",
            serde_json::json!({ "stage": "extracting", "major": major }),
        );
        extract_zip(&archive, &root)?;
        let _ = fs::remove_file(&archive);
        let executable =
            find_java_executable(&root).ok_or("java.exe was not found in the installed Java runtime.")?;
        let _ = app.emit(
            "java-download",
            serde_json::json!({ "stage": "complete", "major": major }),
        );
        Ok(JavaRuntime {
            path: executable.to_string_lossy().to_string(),
            major,
            source: MADE_BY_US.into(),
            managed: true,
        })
    });
    if result.is_err() {
        let _ = app.emit("java-download", serde_json::json!({ "stage": "failed", "major": major }));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{required_java_major, required_java_major_for, runtime_folder_of};
    use std::path::PathBuf;

    #[test]
    fn only_removes_what_it_downloaded_itself() {
        let root = PathBuf::from(r"C:\data\runtimes");
        let (folder, major) =
            runtime_folder_of(&root, r"C:\data\runtimes\temurin-21\bin\java.exe").unwrap();
        assert_eq!(folder, root.join("temurin-21"));
        assert_eq!(major, 21);

        let (folder, _) = runtime_folder_of(
            &root,
            r"C:\data\runtimes\temurin-25\jdk-25.0.1\bin\java.exe",
        )
        .unwrap();
        assert_eq!(folder, root.join("temurin-25"));

        for outside in [
            r"C:\Program Files\Java\jdk-21\bin\java.exe",
            r"C:\data\runtimes",
            "java",
        ] {
            assert!(
                runtime_folder_of(&root, outside).is_err(),
                "tried to delete {outside}"
            );
        }
    }

    #[test]
    fn reads_the_new_year_style_version_numbers() {
        assert_eq!(required_java_major_for("Paper", "26.2"), 25);
        assert_eq!(required_java_major_for("Paper", "1.21.4"), 21);
        assert_eq!(required_java_major_for("Velocity", "1.16.5"), 25);
    }

    #[test]
    fn chooses_the_supported_java_runtime_for_minecraft_releases() {
        assert_eq!(required_java_major("1.20.4"), 17);
        assert_eq!(required_java_major("1.20.5"), 21);
        assert_eq!(required_java_major("1.21.4"), 21);
        assert_eq!(required_java_major("1.17.1"), 16);
        assert_eq!(required_java_major("1.16.5"), 8);
        assert_eq!(required_java_major("26.1"), 25);
    }
}

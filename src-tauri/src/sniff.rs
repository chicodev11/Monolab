
use std::fs;
use std::io::Read;
use std::path::Path;

use zip::ZipArchive;

pub const UNKNOWN: &str = "Custom";

#[derive(Debug, PartialEq, Eq)]
pub struct Sniffed {
    pub core: String,
    pub version: Option<String>,
}

pub fn sniff(folder: &Path, jar: Option<&Path>) -> Sniffed {
    if let Some(core) = jar.and_then(by_name) {
        let version = if core == "Pumpkin" {
            crate::pumpkin::version_from_log(folder)
        } else {
            jar.and_then(version_in_jar)
        };
        return Sniffed { core, version };
    }
    if let Some(found) = jar.and_then(by_contents) {
        return found;
    }
    if let Some(found) = by_folder(folder) {
        return found;
    }
    Sniffed {
        core: UNKNOWN.into(),
        version: jar.and_then(version_in_jar),
    }
}

pub fn core_in_name(name: &str) -> Option<String> {
    let name = name.to_lowercase();
    core_in_lowercase(&name)
}

fn by_name(path: &Path) -> Option<String> {
    core_in_lowercase(&path.file_name()?.to_string_lossy().to_lowercase())
}

fn core_in_lowercase(name: &str) -> Option<String> {
    for (needle, core) in [
        ("pumpkin", "Pumpkin"),
        ("purpur", "Purpur"),
        ("folia", "Folia"),
        ("paper", "Paper"),
        ("fabric", "Fabric"),
        ("neoforge", "NeoForge"),
        ("forge", "Forge"),
        ("velocity", "Velocity"),
        ("bungee", "BungeeCord"),
    ] {
        if name.contains(needle) {
            return Some(core.into());
        }
    }
    None
}

fn read_entry<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>, name: &str) -> Option<String> {
    let mut entry = zip.by_name(name).ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    Some(text)
}

fn by_contents(jar: &Path) -> Option<Sniffed> {
    let file = fs::File::open(jar).ok()?;
    let mut zip = ZipArchive::new(file).ok()?;
    let manifest = read_entry(&mut zip, "META-INF/MANIFEST.MF").unwrap_or_default();
    let main = manifest
        .lines()
        .find_map(|line| line.strip_prefix("Main-Class:"))
        .map(|value| value.trim().to_string())
        .unwrap_or_default();

    if main.starts_with("io.papermc.paperclip") {
        let list = read_entry(&mut zip, "META-INF/versions.list")
            .unwrap_or_default()
            .to_lowercase();
        let core = if list.contains("purpur") {
            "Purpur"
        } else if list.contains("folia") {
            "Folia"
        } else {
            "Paper"
        };
        return Some(Sniffed {
            core: core.into(),
            version: version_in_zip(&mut zip),
        });
    }
    if main.starts_with("net.fabricmc.installer") {
        let props = read_entry(&mut zip, "install.properties").unwrap_or_default();
        let version = props
            .lines()
            .find_map(|line| line.trim().strip_prefix("game-version="))
            .map(|value| value.trim().to_string());
        return Some(Sniffed {
            core: "Fabric".into(),
            version,
        });
    }
    if main.starts_with("com.velocitypowered") {
        return Some(Sniffed {
            core: "Velocity".into(),
            version: None,
        });
    }
    if main.starts_with("net.md_5.bungee") {
        return Some(Sniffed {
            core: "BungeeCord".into(),
            version: None,
        });
    }
    if main.starts_with("net.minecraft.bundler") || main.starts_with("net.minecraft.server") {
        return Some(Sniffed {
            core: "Vanilla".into(),
            version: version_in_zip(&mut zip),
        });
    }
    None
}

fn version_in_zip<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>) -> Option<String> {
    if let Some(text) = read_entry(zip, "version.json") {
        if let Some(version) = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|value| {
                value
                    .get("id")
                    .or_else(|| value.get("name"))
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
        {
            return Some(version);
        }
    }
    let text = read_entry(zip, "version.properties")?;
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("minecraftVersion=")
            .or_else(|| line.trim().strip_prefix("minecraft.version="))
            .map(str::to_owned)
    })
}

pub fn version_in_jar(jar: &Path) -> Option<String> {
    let file = fs::File::open(jar).ok()?;
    let mut zip = ZipArchive::new(file).ok()?;
    version_in_zip(&mut zip)
}

fn by_folder(folder: &Path) -> Option<Sniffed> {
    if folder.join(crate::pumpkin::CONFIG).is_file() {
        return Some(Sniffed {
            core: "Pumpkin".into(),
            version: crate::pumpkin::version_from_log(folder),
        });
    }
    let libraries = folder.join("libraries").join("net");
    if let Some(build) = first_dir_name(&libraries.join("neoforged").join("neoforge")) {
        return Some(Sniffed {
            core: "NeoForge".into(),
            version: crate::cores::neoforge_game_version(&build),
        });
    }
    if let Some(build) = first_dir_name(&libraries.join("minecraftforge").join("forge")) {
        let version = build.split('-').next().map(str::to_owned);
        return Some(Sniffed {
            core: "Forge".into(),
            version,
        });
    }
    if folder.join(".fabric").is_dir() || folder.join("fabric-server-launch.properties").is_file() {
        return Some(Sniffed {
            core: "Fabric".into(),
            version: None,
        });
    }
    if folder.join("plugins").is_dir() && !folder.join("mods").is_dir() {
        return Some(Sniffed {
            core: "Paper".into(),
            version: None,
        });
    }
    None
}

fn first_dir_name(parent: &Path) -> Option<String> {
    let mut names: Vec<String> = fs::read_dir(parent)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names.pop()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn jar(dir: &Path, name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
        let path = dir.join(name);
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (entry, body) in files {
            zip.start_file(*entry, SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("se-sniff-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_the_core_out_of_a_renamed_jar() {
        let dir = scratch("renamed");
        let paper = jar(
            &dir,
            "server.jar",
            &[
                ("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\nMain-Class: io.papermc.paperclip.Main\n"),
                ("META-INF/versions.list", "abc\tpaper-1.21.4.jar\tversions/1.21.4/paper-1.21.4.jar\n"),
                ("version.json", r#"{"id":"1.21.4","name":"1.21.4"}"#),
            ],
        );
        assert_eq!(
            sniff(&dir, Some(&paper)),
            Sniffed { core: "Paper".into(), version: Some("1.21.4".into()) }
        );

        let purpur = jar(
            &dir,
            "srv.jar",
            &[
                ("META-INF/MANIFEST.MF", "Main-Class: io.papermc.paperclip.Main\n"),
                ("META-INF/versions.list", "abc\tpurpur-1.21.4.jar\tx\n"),
            ],
        );
        assert_eq!(sniff(&dir, Some(&purpur)).core, "Purpur");

        let fabric = jar(
            &dir,
            "launch.jar",
            &[
                ("META-INF/MANIFEST.MF", "Main-Class: net.fabricmc.installer.ServerLauncher\n"),
                ("install.properties", "fabric-loader-version=0.16.9\ngame-version=1.21.1\n"),
            ],
        );
        assert_eq!(
            sniff(&dir, Some(&fabric)),
            Sniffed { core: "Fabric".into(), version: Some("1.21.1".into()) }
        );

        let vanilla = jar(
            &dir,
            "minecraft_server.jar",
            &[
                ("META-INF/MANIFEST.MF", "Main-Class: net.minecraft.bundler.Main\n"),
                ("version.json", r#"{"id":"1.21.5"}"#),
            ],
        );
        assert_eq!(sniff(&dir, Some(&vanilla)).core, "Vanilla");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn knows_an_installed_forge_folder_without_a_jar() {
        let dir = scratch("installed");
        fs::create_dir_all(dir.join("libraries/net/neoforged/neoforge/21.4.157")).unwrap();
        assert_eq!(
            sniff(&dir, None),
            Sniffed { core: "NeoForge".into(), version: Some("1.21.4".into()) }
        );

        let dir = scratch("forge");
        fs::create_dir_all(dir.join("libraries/net/minecraftforge/forge/1.21.1-52.0.1")).unwrap();
        assert_eq!(
            sniff(&dir, None),
            Sniffed { core: "Forge".into(), version: Some("1.21.1".into()) }
        );

        let dir = scratch("plugins");
        fs::create_dir_all(dir.join("plugins")).unwrap();
        assert_eq!(sniff(&dir, None).core, "Paper");

        let dir = scratch("empty");
        assert_eq!(sniff(&dir, None).core, UNKNOWN);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn trusts_the_file_name_first() {
        let dir = scratch("named");
        assert_eq!(sniff(&dir, Some(Path::new("pumpkin.exe"))).core, "Pumpkin");
        assert_eq!(sniff(&dir, Some(Path::new("paper-1.21.4.jar"))).core, "Paper");
        assert_eq!(sniff(&dir, Some(Path::new("neoforge-21.4.jar"))).core, "NeoForge");
        assert_eq!(sniff(&dir, Some(Path::new("purpur-1.21.jar"))).core, "Purpur");
    }
}

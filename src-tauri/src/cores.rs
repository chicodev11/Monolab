
use crate::{
    download_server_file, hidden_command, java::resolve_java_runtime, off_runtime, DownloadSource,
};
use std::{fs, path::{Path, PathBuf}, time::Duration};
use tauri::{AppHandle, Emitter};

pub fn list_core_versions(core: &str, snapshots: bool) -> Result<Vec<String>, String> {
    let client = crate::net::builder()
        .user_agent("Monolab/0.1.0 (local Windows desktop app)")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?;
    match core {
        "Vanilla" => mojang_releases(&client, snapshots),
        "Paper" => papermc_versions(&client, "paper", snapshots),
        "Folia" => papermc_versions(&client, "folia", snapshots),
        "Purpur" => purpur_versions(&client),
        "Fabric" => fabric_versions(&client, snapshots),
        "Forge" => forge_versions(&client),
        "NeoForge" => neoforge_versions(&client),
        "Pumpkin" => pumpkin_versions(&client),
        "Velocity" | "BungeeCord" => Ok(Vec::new()),
        _ => mojang_releases(&client, snapshots),
    }
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CoreBuild {
    pub id: String,
    pub label: String,
    pub channel: String,
    pub stable: bool,
    pub when: String,
}

fn plain_build(id: String, label: String) -> CoreBuild {
    CoreBuild {
        id,
        label,
        channel: String::new(),
        when: String::new(),
        // Never said to be a pre-release, so treated as stable.
        stable: true,
    }
}

pub fn list_core_builds(core: &str, version: &str) -> Result<Vec<CoreBuild>, String> {
    let client = crate::net::builder()
        .user_agent("Monolab/0.1.0 (local Windows desktop app)")
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?;
    match core {
        "Paper" => fill_builds(&client, "paper", version),
        "Folia" => fill_builds(&client, "folia", version),
        "Purpur" => purpur_builds(&client, version),
        "Fabric" => fabric_builds(&client),
        "NeoForge" => neoforge_builds(&client, version),
        "Forge" => forge_builds(&client, version),
        _ => Ok(Vec::new()),
    }
}

fn fill_builds(
    client: &reqwest::blocking::Client,
    project: &str,
    version: &str,
) -> Result<Vec<CoreBuild>, String> {
    let builds: serde_json::Value = client
        .get(format!(
            "https://fill.papermc.io/v3/projects/{project}/versions/{version}/builds"
        ))
        .send()
        .map_err(|error| format!("Could not fetch the {project} build list: {error}"))?
        .error_for_status()
        .map_err(|_| format!("{project} has no build for Minecraft {version}."))?
        .json()
        .map_err(|error| error.to_string())?;
    Ok(builds
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = item.get("id")?.as_i64()?;
            let channel = item.get("channel")?.as_str()?.to_string();
            Some(CoreBuild {
                id: id.to_string(),
                label: id.to_string(),
                stable: channel.eq_ignore_ascii_case("STABLE"),
                channel,
                when: item
                    .get("time")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect())
}

fn purpur_builds(
    client: &reqwest::blocking::Client,
    version: &str,
) -> Result<Vec<CoreBuild>, String> {
    let data: serde_json::Value = client
        .get(format!("https://api.purpurmc.org/v2/purpur/{version}"))
        .send()
        .map_err(|error| format!("Could not fetch the Purpur build list: {error}"))?
        .error_for_status()
        .map_err(|_| format!("Purpur has no build for Minecraft {version}."))?
        .json()
        .map_err(|error| error.to_string())?;
    let mut list: Vec<CoreBuild> = data
        .pointer("/builds/all")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let id = item.as_str()?.to_string();
            Some(plain_build(id.clone(), id))
        })
        .collect();
    list.reverse();
    Ok(list)
}

fn fabric_builds(client: &reqwest::blocking::Client) -> Result<Vec<CoreBuild>, String> {
    let loaders: serde_json::Value = client
        .get("https://meta.fabricmc.net/v2/versions/loader")
        .send()
        .map_err(|error| format!("Could not fetch the Fabric loader list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    Ok(loaders
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let version = item.get("version")?.as_str()?.to_string();
            let stable = item.get("stable").and_then(|value| value.as_bool()) == Some(true);
            Some(CoreBuild {
                label: version.clone(),
                id: version,
                channel: if stable { String::new() } else { "unstable".into() },
                when: String::new(),
                stable,
            })
        })
        .collect())
}

fn neoforge_all_builds(client: &reqwest::blocking::Client) -> Result<Vec<String>, String> {
    let value: serde_json::Value = client
        .get("https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge")
        .send()
        .map_err(|error| format!("Could not fetch the NeoForge build list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| format!("Could not read the NeoForge build list: {error}"))?;
    let list: Vec<String> = value
        .get("versions")
        .and_then(|one| one.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|one| one.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    if list.is_empty() {
        return Err("The NeoForge build list is empty.".into());
    }
    Ok(list)
}

fn neoforge_builds(
    client: &reqwest::blocking::Client,
    version: &str,
) -> Result<Vec<CoreBuild>, String> {
    let mut list: Vec<CoreBuild> = neoforge_all_builds(client)?
        .into_iter()
        .filter(|build| neoforge_game_version(build).as_deref() == Some(version))
        .map(|build| {
            CoreBuild {
                stable: !build.ends_with("-beta"),
                label: build.clone(),
                id: build,
                channel: String::new(),
                when: String::new(),
            }
        })
        .collect();
    list.reverse();
    Ok(list)
}

fn forge_builds(
    client: &reqwest::blocking::Client,
    version: &str,
) -> Result<Vec<CoreBuild>, String> {
    let text = client
        .get("https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml")
        .send()
        .map_err(|error| format!("Could not fetch the Forge build list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .text()
        .map_err(|error| error.to_string())?;
    let head = format!("{version}-");
    let mut list: Vec<CoreBuild> = maven_versions(&text)
        .into_iter()
        .filter_map(|full| {
            let rest = full.strip_prefix(&head)?.to_string();
            Some(plain_build(full, rest))
        })
        .collect();
    list.reverse();
    Ok(list)
}

fn maven_versions(xml: &str) -> Vec<String> {
    xml.split("<version>")
        .skip(1)
        .filter_map(|chunk| chunk.split_once("</version>"))
        .map(|(value, _)| value.trim().to_string())
        .collect()
}

fn pumpkin_versions(client: &reqwest::blocking::Client) -> Result<Vec<String>, String> {
    let releases: serde_json::Value = client
        .get("https://api.github.com/repos/Pumpkin-MC/Pumpkin/releases?per_page=30")
        .send()
        .map_err(|error| format!("Could not fetch the Pumpkin release list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    let all = releases
        .as_array()
        .ok_or("The Pumpkin release list is malformed.")?;
    let tags = |want_nightly: bool| {
        all.iter().filter_map(move |one| {
            let tag = one.get("tag_name").and_then(|tag| tag.as_str())?;
            let prerelease = one.get("prerelease").and_then(|v| v.as_bool()).unwrap_or(false);
            let nightly = tag == "nightly" || prerelease;
            (nightly == want_nightly).then(|| tag.to_string())
        })
    };
    let list: Vec<String> = tags(false).chain(tags(true)).collect();
    if list.is_empty() {
        return Err("No Pumpkin release was found.".into());
    }
    Ok(list)
}

fn pumpkin_sha256(
    client: &reqwest::blocking::Client,
    release: &serde_json::Value,
    asset_name: &str,
) -> Option<String> {
    let url = release
        .get("assets")?
        .as_array()?
        .iter()
        .find(|one| one.get("name").and_then(|name| name.as_str()) == Some("checksums.sha256"))?
        .get("browser_download_url")?
        .as_str()?
        .to_string();
    let text = client.get(url).send().ok()?.error_for_status().ok()?.text().ok()?;
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let name = parts.next()?;
        (name == asset_name && hash.len() == 64).then(|| hash.to_lowercase())
    })
}

fn pumpkin_download_source(
    client: &reqwest::blocking::Client,
    version: &str,
) -> Result<DownloadSource, String> {
    pumpkin_asset_source(client, version, PUMPKIN_WINDOWS_ASSET)
}

/// Downloads the Linux build of a Pumpkin release for an SSH server (x86-64 or ARM64).
pub fn pumpkin_linux_download(version: &str, arm: bool, destination: &Path) -> Result<(), String> {
    let client = crate::net::builder()
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|error| error.to_string())?;
    let asset = if arm { "pumpkin-ARM64-Linux" } else { "pumpkin-X64-Linux" };
    let source = pumpkin_asset_source(&client, version, asset)?;
    crate::net::check_download_url(&source.url)?;
    let bytes = client
        .get(&source.url)
        .send()
        .and_then(|reply| reply.error_for_status())
        .and_then(|reply| reply.bytes())
        .map_err(|error| format!("Could not download Pumpkin for Linux: {error}"))?;
    if bytes.len() < 1024 * 1024 || !bytes.starts_with(b"\x7fELF") {
        return Err("The downloaded Pumpkin file isn't a Linux program.".into());
    }
    fs::write(destination, &bytes).map_err(|error| error.to_string())?;
    if let Some(sum) = source.sha256 {
        crate::hashes::verify_sha256(destination, &sum)?;
    }
    Ok(())
}

fn pumpkin_asset_source(
    client: &reqwest::blocking::Client,
    version: &str,
    asset: &str,
) -> Result<DownloadSource, String> {
    let release: serde_json::Value = client
        .get(format!(
            "https://api.github.com/repos/Pumpkin-MC/Pumpkin/releases/tags/{version}"
        ))
        .send()
        .map_err(|error| format!("Could not fetch Pumpkin release info: {error}"))?
        .error_for_status()
        .map_err(|_| format!("Pumpkin release {version} was not found."))?
        .json()
        .map_err(|error| error.to_string())?;
    let url = release
        .get("assets")
        .and_then(|assets| assets.as_array())
        .and_then(|assets| {
            assets.iter().find(|one| {
                one.get("name").and_then(|name| name.as_str()) == Some(asset)
            })
        })
        .and_then(|one| one.get("browser_download_url"))
        .and_then(|url| url.as_str())
        .ok_or_else(|| format!("This Pumpkin release has no {asset} file."))?
        .to_string();
    Ok(DownloadSource {
        url,
        sha1: None,
        sha256: pumpkin_sha256(client, &release, asset),
        md5: None,
    })
}

pub const PUMPKIN_WINDOWS_ASSET: &str = "pumpkin-X64-Windows.exe";
pub const PUMPKIN_EXE: &str = "pumpkin.exe";

fn newest_first(mut list: Vec<String>) -> Vec<String> {
    list.reverse();
    list.dedup();
    list
}

fn mojang_releases(client: &reqwest::blocking::Client, snapshots: bool) -> Result<Vec<String>, String> {
    let data: serde_json::Value = client
        .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .send()
        .map_err(|error| format!("Could not fetch the version list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    Ok(data
        .get("versions")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter(|item| {
            let kind = item.get("type").and_then(|v| v.as_str());
            kind == Some("release") || (snapshots && kind == Some("snapshot"))
        })
        .filter_map(|item| Some(item.get("id")?.as_str()?.to_string()))
        .collect())
}

fn papermc_versions(
    client: &reqwest::blocking::Client,
    project: &str,
    snapshots: bool,
) -> Result<Vec<String>, String> {
    let data: serde_json::Value = client
        .get(format!("https://fill.papermc.io/v3/projects/{project}"))
        .send()
        .map_err(|error| format!("Could not fetch the {project} version list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    let groups = data
        .get("versions")
        .and_then(|value| value.as_object())
        .ok_or_else(|| format!("The {project} version list is malformed."))?;
    let mut list: Vec<String> = groups
        .values()
        .filter_map(|value| value.as_array())
        .flatten()
        .filter_map(|item| item.as_str())
        .filter(|item| snapshots || !item.contains('-'))
        .map(str::to_owned)
        .collect();
    list.sort_by(|a, b| compare_versions(b, a));
    list.dedup();
    Ok(list)
}

fn purpur_versions(client: &reqwest::blocking::Client) -> Result<Vec<String>, String> {
    let data: serde_json::Value = client
        .get("https://api.purpurmc.org/v2/purpur")
        .send()
        .map_err(|error| format!("Could not fetch the Purpur version list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    Ok(newest_first(
        data.get("versions")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|item| Some(item.as_str()?.to_string()))
            .collect(),
    ))
}

fn fabric_versions(client: &reqwest::blocking::Client, snapshots: bool) -> Result<Vec<String>, String> {
    let data: serde_json::Value = client
        .get("https://meta.fabricmc.net/v2/versions/game")
        .send()
        .map_err(|error| format!("Could not fetch the Fabric version list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    Ok(data
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| snapshots || item.get("stable").and_then(|v| v.as_bool()) == Some(true))
        .filter_map(|item| Some(item.get("version")?.as_str()?.to_string()))
        .collect())
}

fn forge_versions(client: &reqwest::blocking::Client) -> Result<Vec<String>, String> {
    let data: serde_json::Value = client
        .get("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json")
        .send()
        .map_err(|error| format!("Could not fetch the Forge version list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    let promos = data
        .get("promos")
        .and_then(|value| value.as_object())
        .ok_or("The Forge version list is malformed.")?;
    let mut list: Vec<String> = promos
        .keys()
        .filter_map(|key| key.split_once('-').map(|(version, _)| version.to_string()))
        .collect();
    list.sort_by(|a, b| compare_versions(b, a));
    list.dedup();
    Ok(list)
}

pub(crate) fn neoforge_game_version(build: &str) -> Option<String> {
    let parts: Vec<&str> = build.split('.').collect();
    match parts.len() {
        3 => Some(if parts[1] == "0" {
            format!("1.{}", parts[0])
        } else {
            format!("1.{}.{}", parts[0], parts[1])
        }),
        4 => Some(if parts[2] == "0" {
            format!("{}.{}", parts[0], parts[1])
        } else {
            format!("{}.{}.{}", parts[0], parts[1], parts[2])
        }),
        _ => None,
    }
}

fn neoforge_versions(client: &reqwest::blocking::Client) -> Result<Vec<String>, String> {
    let mut list: Vec<String> = Vec::new();
    for build in neoforge_all_builds(client)? {
        if build.contains("SNAPSHOT") {
            continue;
        }
        let Some(game) = neoforge_game_version(&build) else {
            continue;
        };
        if !list.contains(&game) {
            list.push(game);
        }
    }
    list.sort_by(|a, b| compare_versions(b, a));
    Ok(list)
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let numbers = |value: &str| {
        value
            .split('.')
            .map(|piece| piece.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    numbers(a).cmp(&numbers(b))
}

fn vanilla_download_source(
    client: &reqwest::blocking::Client,
    version: &str,
) -> Result<DownloadSource, String> {
    let manifest: serde_json::Value = client
        .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .send()
        .map_err(|error| format!("Could not fetch the Minecraft version list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    let metadata_url = manifest
        .get("versions")
        .and_then(|value| value.as_array())
        .and_then(|versions| {
            versions
                .iter()
                .find(|item| item.get("id").and_then(|id| id.as_str()) == Some(version))
        })
        .and_then(|item| item.get("url"))
        .and_then(|url| url.as_str())
        .ok_or("The chosen Minecraft release was not found.")?;
    let metadata: serde_json::Value = client
        .get(metadata_url)
        .send()
        .map_err(|error| format!("Could not fetch Minecraft download info: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    let server = metadata
        .pointer("/downloads/server")
        .ok_or("This Minecraft version has no dedicated server file.")?;
    Ok(DownloadSource {
        url: server
            .get("url")
            .and_then(|url| url.as_str())
            .map(str::to_owned)
            .ok_or("This Minecraft version has no dedicated server file.")?,
        sha1: server
            .get("sha1")
            .and_then(|value| value.as_str())
            .map(str::to_owned),
        sha256: None,
        md5: None,
    })
}

fn fill_download_source(
    client: &reqwest::blocking::Client,
    project: &str,
    version: &str,
    build: Option<&str>,
) -> Result<DownloadSource, String> {
    let url = format!("https://fill.papermc.io/v3/projects/{project}/versions/{version}/builds");
    let builds: serde_json::Value = client
        .get(&url)
        .send()
        .map_err(|error| format!("Could not fetch the {project} build list: {error}"))?
        .error_for_status()
        .map_err(|_| {
            format!("{project} has no stable build for Minecraft {version}.")
        })?
        .json()
        .map_err(|error| error.to_string())?;
    let picked = builds.as_array().and_then(|items| match build {
        Some(wanted) => items
            .iter()
            .find(|item| item.get("id").and_then(|id| id.as_i64()).map(|id| id.to_string())
                == Some(wanted.to_string())),
        None => items.iter().find(|item| {
            item.get("channel").and_then(|channel| channel.as_str()) == Some("STABLE")
        }),
    });
    let picked = match (picked, build) {
        (Some(item), _) => item,
        (None, Some(wanted)) => {
            return Err(format!("{project} build {wanted} was not found."))
        }
        (None, None) => return Err(format!("No stable {project} build was found.")),
    };
    let download = picked
        .pointer("/downloads/server:default")
        .ok_or(format!("The {project} build has no download info."))?;
    Ok(DownloadSource {
        url: download
            .get("url")
            .and_then(|url| url.as_str())
            .map(str::to_owned)
            .ok_or(format!("The {project} download URL is malformed."))?,
        sha1: None,
        sha256: download
            .pointer("/checksums/sha256")
            .and_then(|value| value.as_str())
            .map(str::to_owned),
        md5: None,
    })
}

fn fabric_download_source(
    client: &reqwest::blocking::Client,
    version: &str,
    build: Option<&str>,
) -> Result<DownloadSource, String> {
    let url = format!("https://meta.fabricmc.net/v2/versions/loader/{version}");
    let loaders: serde_json::Value = client
        .get(&url)
        .send()
        .map_err(|error| format!("Could not fetch the Fabric version list: {error}"))?
        .error_for_status()
        .map_err(|_| format!("Fabric does not support Minecraft {version}."))?
        .json()
        .map_err(|error| error.to_string())?;
    let items = loaders.as_array().ok_or("fabric-loader info was not found.")?;
    let found = match build {
        Some(wanted) => items
            .iter()
            .find(|item| item.pointer("/loader/version").and_then(|v| v.as_str()) == Some(wanted))
            .ok_or_else(|| format!("fabric-loader {wanted} is not available for Minecraft {version}."))?,
        None => items.first().ok_or("fabric-loader info was not found.")?,
    };
    let loader = found
        .pointer("/loader/version")
        .and_then(|value| value.as_str())
        .ok_or("The fabric-loader version is malformed.")?;
    let installer = fabric_installer(client)?;
    Ok(DownloadSource::unchecked(format!(
        "https://meta.fabricmc.net/v2/versions/loader/{version}/{loader}/{installer}/server/jar"
    )))
}

fn fabric_installer(client: &reqwest::blocking::Client) -> Result<String, String> {
    let installers: serde_json::Value = client
        .get("https://meta.fabricmc.net/v2/versions/installer")
        .send()
        .map_err(|error| format!("Could not fetch the Fabric installer list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    installers
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item.get("stable").and_then(|v| v.as_bool()) == Some(true))
        .and_then(|item| item.get("version")?.as_str())
        .map(|version| version.to_string())
        .ok_or_else(|| "No Fabric installer was found.".to_string())
}

fn purpur_download_source(
    client: &reqwest::blocking::Client,
    version: &str,
    build: Option<&str>,
) -> Result<DownloadSource, String> {
    let which = build.unwrap_or("latest");
    let url = format!("https://api.purpurmc.org/v2/purpur/{version}/{which}");
    let latest: serde_json::Value = client
        .get(&url)
        .send()
        .map_err(|error| format!("Could not fetch Purpur build info: {error}"))?
        .error_for_status()
        .map_err(|_| format!("Purpur {version} build {which} was not found."))?
        .json()
        .map_err(|error| error.to_string())?;
    let chosen = latest
        .get("build")
        .and_then(|value| value.as_str())
        .ok_or("The Purpur build info is malformed.")?;
    Ok(DownloadSource {
        url: format!("https://api.purpurmc.org/v2/purpur/{version}/{chosen}/download"),
        sha1: None,
        sha256: None,
        md5: latest
            .get("md5")
            .and_then(|value| value.as_str())
            .map(str::to_owned),
    })
}

fn maven_sha1(client: &reqwest::blocking::Client, url: &str) -> Result<String, String> {
    let checksum_url = format!("{url}.sha1");
    let text = client
        .get(&checksum_url)
        .send()
        .map_err(|error| format!("Could not fetch the official SHA‑1: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Could not fetch the official SHA‑1: {error}"))?
        .text()
        .map_err(|error| error.to_string())?;
    text.split_whitespace()
        .next()
        .filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_owned)
        .ok_or("The official SHA‑1 info is malformed.".into())
}

fn neoforge_installer_url(
    client: &reqwest::blocking::Client,
    version: &str,
    build: Option<&str>,
) -> Result<String, String> {
    let requested = version.strip_prefix("1.").unwrap_or(version);
    let mut builds = Vec::new();
    for build in neoforge_all_builds(client)? {
        if build.starts_with(&format!("{requested}.")) {
            builds.push(build);
        }
    }
    let build = match build {
        Some(wanted) => builds
            .iter()
            .find(|item| item.as_str() == wanted)
            .ok_or_else(|| format!("NeoForge build {wanted} was not found."))?,
        None => builds
            .iter()
            .rev()
            .find(|build| !build.contains("SNAPSHOT") && !build.ends_with("-beta"))
            .or_else(|| builds.iter().rev().find(|build| !build.contains("SNAPSHOT")))
            .or_else(|| builds.last())
            .ok_or_else(|| {
                format!("NeoForge has no installer build for Minecraft {version}.")
            })?,
    };
    Ok(format!("https://maven.neoforged.net/releases/net/neoforged/neoforge/{build}/neoforge-{build}-installer.jar"))
}

fn forge_installer_url(
    client: &reqwest::blocking::Client,
    version: &str,
    build: Option<&str>,
) -> Result<String, String> {
    if let Some(release) = build {
        return Ok(format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{release}/forge-{release}-installer.jar"));
    }
    let promotions: serde_json::Value = client
        .get("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json")
        .send()
        .map_err(|error| format!("Could not fetch the Forge build list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    let values = promotions
        .get("promos")
        .and_then(|value| value.as_object())
        .ok_or("The Forge build list is malformed.")?;
    let build = values
        .get(&format!("{version}-recommended"))
        .or_else(|| values.get(&format!("{version}-latest")))
        .and_then(|value| value.as_str())
        .ok_or(format!(
            "Forge has no installer build for Minecraft {version}."
        ))?;
    let release = format!("{version}-{build}");
    Ok(format!("https://maven.minecraftforge.net/net/minecraftforge/forge/{release}/forge-{release}-installer.jar"))
}

fn run_installer(
    java: &Path,
    installer: &Path,
    folder: &Path,
    label: &str,
) -> Result<(), String> {
    let output = hidden_command(java)
        .current_dir(folder)
        .arg("-jar")
        .arg(installer)
        .arg("--installServer")
        .output()
        .map_err(|error| format!("Could not run the {label} installer: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let log = folder.join("installer-log.txt");
    let _ = fs::write(&log, &text);
    let tail: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(6)
        .collect();
    let tail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    let java_note = match hidden_command(java).arg("-version").output() {
        Ok(probe) => {
            let raw = if probe.stderr.is_empty() {
                probe.stdout
            } else {
                probe.stderr
            };
            String::from_utf8_lossy(&raw)
                .lines()
                .next()
                .unwrap_or("")
                .to_string()
        }
        Err(_) => String::new(),
    };
    Err(format!(
        "The {label} installer did not finish.\n\nJava used: {java_note}\n\n{tail}\n\nFull log: {}",
        log.display()
    ))
}

fn install_forge(
    app: &AppHandle,
    version: &str,
    build: Option<&str>,
    folder: &Path,
    client: &reqwest::blocking::Client,
    operation_id: &str,
) -> Result<(), String> {
    let installer = folder.join("forge-installer.jar");
    let url = forge_installer_url(client, version, build)?;
    let source = DownloadSource {
        sha1: Some(maven_sha1(client, &url)?),
        sha256: None,
        md5: None,
        url,
    };
    download_server_file(
        app,
        client,
        &source,
        &installer,
        "Forge",
        version,
        operation_id,
    )?;
    let _ = app.emit(
        "server-download",
        serde_json::json!({ "stage": "installing", "core": "Forge", "version": version }),
    );
    let java = resolve_java_runtime(app, "Forge", version, "java")?;
    let result = run_installer(&java, &installer, folder, "Forge");
    let _ = fs::remove_file(installer);
    result
}

fn install_neoforge(
    app: &AppHandle,
    version: &str,
    build: Option<&str>,
    folder: &Path,
    client: &reqwest::blocking::Client,
    operation_id: &str,
) -> Result<(), String> {
    let installer = folder.join("neoforge-installer.jar");
    let url = neoforge_installer_url(client, version, build)?;
    let source = DownloadSource {
        sha1: Some(maven_sha1(client, &url)?),
        sha256: None,
        md5: None,
        url,
    };
    download_server_file(
        app,
        client,
        &source,
        &installer,
        "NeoForge",
        version,
        operation_id,
    )?;
    let _ = app.emit(
        "server-download",
        serde_json::json!({ "stage": "installing", "core": "NeoForge", "version": version }),
    );
    let java = resolve_java_runtime(app, "NeoForge", version, "java")?;
    let result = run_installer(&java, &installer, folder, "NeoForge");
    let _ = fs::remove_file(installer);
    result
}

fn fill_latest_download_source(
    client: &reqwest::blocking::Client,
    project: &str,
) -> Result<DownloadSource, String> {
    let url = format!("https://fill.papermc.io/v3/projects/{project}");
    let project_data: serde_json::Value = client
        .get(&url)
        .send()
        .map_err(|error| format!("Could not fetch the {project} version list: {error}"))?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    let version = project_data
        .get("versions")
        .and_then(|value| value.as_object())
        .and_then(|families| families.values().next())
        .and_then(|versions| versions.as_array())
        .and_then(|versions| {
            versions
                .iter()
                .find(|value| {
                    value
                        .as_str()
                        .is_some_and(|version| !version.contains("SNAPSHOT"))
                })
                .or_else(|| versions.first())
        })
        .and_then(|version| version.as_str())
        .ok_or_else(|| format!("No stable {project} version was found."))?;
    fill_download_source(client, project, version, None)
}

pub(crate) fn download_server_jar(
    app: &AppHandle,
    core: &str,
    version: &str,
    build: Option<&str>,
    folder: &Path,
    operation_id: &str,
) -> Result<Option<PathBuf>, String> {
    off_runtime(|| {
        let client = crate::net::builder()
            .user_agent("Monolab/0.1.0 (local Windows desktop app)")
            .timeout(Duration::from_secs(90))
            .build()
            .map_err(|error| error.to_string())?;
        let _ = app.emit(
            "server-download",
            serde_json::json!({ "stage": "resolving", "core": core, "version": version }),
        );
        let source = match core {
            "Vanilla" => vanilla_download_source(&client, version)?,
            "Paper" => fill_download_source(&client, "paper", version, build)?,
            "Folia" => fill_download_source(&client, "folia", version, build)?,
            "Purpur" => purpur_download_source(&client, version, build)?,
            "Fabric" => fabric_download_source(&client, version, build)?,
            "Forge" => { install_forge(app, version, build, folder, &client, operation_id)?; return Ok(None); }
            "NeoForge" => { install_neoforge(app, version, build, folder, &client, operation_id)?; return Ok(None); }
            "Velocity" => fill_latest_download_source(&client, "velocity")?,
            "Pumpkin" => pumpkin_download_source(&client, version)?,
            _ => return Err("Unknown server core.".into()),
        };
        let destination = folder.join(if core == "Pumpkin" {
            PUMPKIN_EXE
        } else {
            "server.jar"
        });
        let _ = app.emit(
            "server-download",
            serde_json::json!({ "stage": "downloading", "core": core, "version": version }),
        );
        download_server_file(
            app,
            &client,
            &source,
            &destination,
            core,
            version,
            operation_id,
        )?;
        let _ = app.emit(
            "server-download",
            serde_json::json!({ "stage": "complete", "core": core, "version": version }),
        );
        Ok(Some(destination))
    })
}

#[cfg(test)]
mod tests {
    use super::{compare_versions, neoforge_game_version};

    #[test]
    #[ignore]
    fn lists_real_core_builds() {
        for (core, version) in [
            ("Paper", "1.21.4"),
            ("Purpur", "1.21.4"),
            ("Fabric", "1.21.4"),
            ("NeoForge", "1.21.4"),
            ("Forge", "1.21.4"),
        ] {
            let builds = super::list_core_builds(core, version).expect("should list builds");
            assert!(!builds.is_empty(), "{core} {version} has no builds");
            let newest = &builds[0];
            let marked = builds.iter().filter(|one| !one.channel.is_empty()).count();
            assert!(
                builds.iter().any(|one| one.stable),
                "{core} {version} has no stable build"
            );
            println!(
                "{core} {version}: {} · first {} ({}) · with a channel {marked}",
                builds.len(),
                newest.label,
                newest.channel
            );
            assert!(builds.iter().all(|one| !one.id.is_empty()));
            assert!(
                builds
                    .iter()
                    .all(|one| !one.label.trim().is_empty() && !one.label.contains(' ')),
                "the {core} label was given a made-up name"
            );
        }
        assert!(super::list_core_builds("Vanilla", "1.21.4").unwrap().is_empty());
        assert!(super::list_core_builds("Pumpkin", "nightly").unwrap().is_empty());
    }

    #[test]
    #[ignore]
    fn downloads_the_build_you_picked() {
        let client = crate::net::builder()
            .user_agent("Monolab test")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        let builds = super::list_core_builds("Paper", "1.21.4").unwrap();
        let older = &builds[builds.len() / 2];
        let source = super::fill_download_source(&client, "paper", "1.21.4", Some(&older.id))
            .expect("should resolve the picked build's URL");
        println!("picked {} -> {}", older.label, source.url);
        assert!(
            source.url.contains(&older.id),
            "the picked build is not in the URL: {}",
            source.url
        );
        let missing = super::fill_download_source(&client, "paper", "1.21.4", Some("999999"));
        assert!(missing.is_err(), "a missing build was accepted");
    }

    #[test]
    fn reads_the_game_version_from_a_neoforge_build() {
        assert_eq!(neoforge_game_version("21.1.248").as_deref(), Some("1.21.1"));
        assert_eq!(neoforge_game_version("21.11.45").as_deref(), Some("1.21.11"));
        assert_eq!(neoforge_game_version("20.0.17").as_deref(), Some("1.20"));
        assert_eq!(neoforge_game_version("26.1.2.97").as_deref(), Some("26.1.2"));
        assert_eq!(neoforge_game_version("26.2.0.64").as_deref(), Some("26.2"));
        assert_eq!(neoforge_game_version("weirdvalue").as_deref(), None);
    }

    #[test]
    #[ignore]
    fn downloads_pumpkin_for_linux() {
        let target = std::env::temp_dir().join("monolab-pumpkin-linux-test");
        super::pumpkin_linux_download("nightly", false, &target).expect("should download the Linux build");
        assert!(std::fs::metadata(&target).unwrap().len() > 1024 * 1024);
        let _ = std::fs::remove_file(&target);
    }

    #[test]
    #[ignore]
    fn finds_a_real_pumpkin_build() {
        let client = crate::net::builder()
            .user_agent("Monolab/0.1.0 (local Windows desktop app)")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        let versions = super::pumpkin_versions(&client).expect("should read the release list");
        println!("{} releases: {:?}", versions.len(), versions);
        assert!(!versions.is_empty());
        let source = super::pumpkin_download_source(&client, &versions[0])
            .expect("should find the Windows file");
        println!("download URL: {}", source.url);
        assert!(source.url.ends_with(super::PUMPKIN_WINDOWS_ASSET));
        assert_ne!(versions[0], "nightly", "nightly came first even though a stable release exists");
        let first_pre = versions.iter().position(|one| one == "nightly" || one == "canary").expect("prereleases are listed");
        assert!(versions[first_pre..].iter().all(|one| one == "nightly" || one == "canary"), "prereleases come after every stable release");
        println!("sha256: {:?}", source.sha256);
        assert!(source.sha256.is_some(), "stable releases ship checksums.sha256");
    }

    #[test]
    #[ignore]
    fn finds_a_real_fabric_server() {
        let client = crate::net::builder()
            .user_agent("Monolab/0.1.0 (local Windows desktop app)")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();
        let installer = super::fabric_installer(&client).expect("should find the installer");
        println!("installer: {installer}");
        for game in ["1.21.1", "26.2"] {
            let source = super::fabric_download_source(&client, game, None).expect("should print an address");
            println!("{game}: {}", source.url);
            let head = client.head(&source.url).send().expect("should probe it");
            assert!(head.status().is_success(), "{} on {game}", head.status());
        }
    }

    #[test]
    fn sorts_versions_by_number_not_by_letter() {
        use std::cmp::Ordering;
        assert_eq!(compare_versions("1.21.10", "1.21.9"), Ordering::Greater);
        assert_eq!(compare_versions("26.2", "1.21.11"), Ordering::Greater);
    }
}

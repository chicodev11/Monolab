//! "Let Bedrock players join": Geyser (translates Bedrock to Java) and Floodgate (Bedrock players need no Java
//! account), from GeyserMC's own download API with its SHA-256. Mods come from Modrinth instead (Floodgate's mod
//! builds are only there), through the normal Modrinth install.

use serde_json::Value;

pub const API: &str = "https://download.geysermc.org/v2/projects";
pub const GEYSER_MODRINTH: &str = "wKkoqHrH";
pub const FLOODGATE_MODRINTH: &str = "bWrNNfkb";
/// The port Bedrock players connect to (UDP), unless Geyser's config says otherwise.
pub const BEDROCK_PORT: u16 = 19132;
/// Geyser speaks only the newest Java version; ViaVersion lets it into a server on an older one.
pub const VIAVERSION_MODRINTH: &str = "viaversion";
/// Geyser's plugin doesn't run on servers older than this.
pub const OLDEST: &str = "1.12.2";

fn numbers(version: &str) -> Vec<u32> {
    version
        .split(|letter: char| !letter.is_ascii_digit())
        .filter(|piece| !piece.is_empty())
        .map(|piece| piece.parse().unwrap_or(0))
        .collect()
}

/// Whether `version` is older than `than` ("1.20.4" < "1.21" < "26.3").
pub fn older(version: &str, than: &str) -> bool {
    let (a, b) = (numbers(version), numbers(than));
    !a.is_empty() && !b.is_empty() && a < b
}

/// Proxies have their own version numbers; the servers behind them need ViaVersion instead.
pub fn proxy(core: &str) -> bool {
    matches!(core, "Velocity" | "BungeeCord" | "Waterfall")
}

/// Where the jars go for this core: GeyserMC's platform names for Geyser and Floodgate (they differ for
/// BungeeCord), or `None` for a mod core (use Modrinth) or a core Geyser doesn't run on.
pub fn platforms(core: &str) -> Option<(&'static str, &'static str)> {
    match core {
        "Paper" | "Purpur" | "Folia" | "Leaf" | "Spigot" => Some(("spigot", "spigot")),
        "Velocity" => Some(("velocity", "velocity")),
        "BungeeCord" | "Waterfall" => Some(("bungeecord", "bungee")),
        _ => None,
    }
}

pub fn modded(core: &str) -> bool {
    matches!(core, "Fabric" | "NeoForge")
}

/// The latest build's file for one platform: (download url, file name, sha256).
pub fn pick(project: &str, build: &Value, platform: &str) -> Result<(String, String, String), String> {
    let version = build.get("version").and_then(Value::as_str).ok_or("GeyserMC's answer has no version.")?;
    let number = build.get("build").and_then(Value::as_u64).ok_or("GeyserMC's answer has no build number.")?;
    let file = build.pointer(&format!("/downloads/{platform}")).ok_or(format!("GeyserMC has no {project} build for {platform}."))?;
    let name = file
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| name.ends_with(".jar") && !name.contains(['/', '\\']))
        .ok_or("GeyserMC's file name is malformed.")?;
    let sha256 = file.get("sha256").and_then(Value::as_str).filter(|hash| hash.len() == 64).ok_or("GeyserMC's file has no SHA-256.")?;
    Ok((format!("{API}/{project}/versions/{version}/builds/{number}/downloads/{platform}"), name.to_string(), sha256.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_the_platform_file() {
        let build =
            json!({"version": "2.12.1", "build": 1306, "downloads": {"spigot": {"name": "Geyser-Spigot.jar", "sha256": "a".repeat(64)}}});
        let (url, name, sha) = pick("geyser", &build, "spigot").unwrap();
        assert_eq!(url, "https://download.geysermc.org/v2/projects/geyser/versions/2.12.1/builds/1306/downloads/spigot");
        assert_eq!(name, "Geyser-Spigot.jar");
        assert_eq!(sha.len(), 64);
        assert!(pick("geyser", &build, "velocity").is_err());
        let bad = json!({"version": "1", "build": 1, "downloads": {"spigot": {"name": "../x.jar", "sha256": "a".repeat(64)}}});
        assert!(pick("geyser", &bad, "spigot").is_err());
    }

    #[test]
    fn knows_where_it_runs() {
        assert_eq!(platforms("Paper"), Some(("spigot", "spigot")));
        assert_eq!(platforms("BungeeCord"), Some(("bungeecord", "bungee")));
        assert_eq!(platforms("Fabric"), None);
        assert!(modded("NeoForge") && !modded("Forge"));
        assert_eq!(platforms("Vanilla"), None);
    }
}

#[cfg(test)]
mod version_tests {
    use super::older;

    #[test]
    fn versions_compare_by_number() {
        assert!(older("1.20.4", "1.21"));
        assert!(older("1.21.4", "26.3"));
        assert!(!older("26.3", "26.3"));
        assert!(!older("1.21.10", "1.21.9"));
        assert!(older("1.8.8", "1.12.2"));
        assert!(!older("", "1.21"));
    }
}


use serde::Serialize;
use std::{collections::HashMap, fs, path::Path};

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CrashReport {
    pub file_name: String,
    pub created_at: u64,
    pub description: String,
    pub exception: String,
    pub suspects: Vec<String>,
    pub excerpt: String,
}

fn package_hint(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("at ")?;
    let path = rest.split('(').next()?;
    let parts = path.split('.').collect::<Vec<_>>();
    if parts.len() < 3 {
        return None;
    }
    let root = parts[0];
    if matches!(root, "java" | "javax" | "jdk" | "sun" | "kotlin" | "scala") {
        return None;
    }
    let candidate = if matches!(root, "net" | "com" | "org" | "io" | "me" | "dev" | "xyz") {
        parts.get(1)?
    } else {
        parts.first()?
    };
    if matches!(*candidate, "minecraft" | "minecraftforge" | "neoforged" | "fabricmc" | "bukkit" | "spigotmc" | "papermc") {
        return None;
    }
    Some(candidate.to_ascii_lowercase())
}

fn installed_addons(instance_path: &Path) -> Vec<String> {
    let mut names = Vec::new();
    for folder in ["mods", "plugins"] {
        let Ok(entries) = fs::read_dir(instance_path.join(folder)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.to_ascii_lowercase().ends_with(".jar") {
                names.push(name);
            }
        }
    }
    names
}

fn match_suspects(hints: &[String], addons: &[String]) -> Vec<String> {
    let mut scored: HashMap<String, usize> = HashMap::new();
    for (rank, hint) in hints.iter().enumerate() {
        if hint.len() < 3 {
            continue;
        }
        for addon in addons {
            let lowered = addon.to_ascii_lowercase();
            if lowered.contains(hint.as_str()) {
                let weight = hints.len().saturating_sub(rank);
                *scored.entry(addon.clone()).or_default() += weight;
            }
        }
    }
    let mut ranked = scored.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked.into_iter().map(|(name, _)| name).take(5).collect()
}

pub fn parse_report(path: &Path, instance_path: &Path) -> Result<CrashReport, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("Could not read the crash report: {error}"))?;
    let created_at = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(parse_text(&text, file_name, created_at, &installed_addons(instance_path)))
}

pub fn parse_text(text: &str, file_name: String, created_at: u64, addons: &[String]) -> CrashReport {
    let description = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("Description:"))
        .map(|value| value.trim().to_string())
        .unwrap_or_else(|| "No description found.".into());
    let exception = text
        .lines()
        .find(|line| {
            let trimmed = line.trim();
            (trimmed.contains("Exception") || trimmed.contains("Error"))
                && trimmed.contains(':')
                && !trimmed.starts_with("at ")
                && !trimmed.starts_with("--")
        })
        .map(|line| line.trim().to_string())
        .unwrap_or_default();

    let hints = text
        .lines()
        .take(400)
        .filter_map(package_hint)
        .collect::<Vec<_>>();
    let mut unique = Vec::new();
    for hint in hints {
        if !unique.contains(&hint) {
            unique.push(hint);
        }
    }
    let suspects = match_suspects(&unique, addons);

    CrashReport {
        file_name,
        created_at,
        description,
        exception,
        suspects,
        excerpt: text.lines().take(60).collect::<Vec<_>>().join("\n"),
    }
}

pub fn list_reports(instance_path: &Path) -> Vec<CrashReport> {
    let folder = instance_path.join("crash-reports");
    let Ok(entries) = fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut reports = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"))
        })
        .filter_map(|path| parse_report(&path, instance_path).ok())
        .collect::<Vec<_>>();
    reports.sort_by_key(|left| std::cmp::Reverse(left.created_at));
    reports.truncate(20);
    reports
}

#[cfg(test)]
mod tests {
    use super::{match_suspects, package_hint};

    #[test]
    fn skips_the_game_and_the_java_runtime() {
        assert_eq!(package_hint("\tat java.util.HashMap.get(HashMap.java:1)"), None);
        assert_eq!(
            package_hint("\tat net.minecraft.server.MinecraftServer.run(MinecraftServer.java:1)"),
            None
        );
        assert_eq!(
            package_hint("\tat net.fabricmc.loader.Loader.load(Loader.java:1)"),
            None
        );
    }

    #[test]
    fn pulls_the_project_name_out_of_a_stack_line() {
        assert_eq!(
            package_hint("\tat com.terraforged.mod.Main.tick(Main.java:42)"),
            Some("terraforged".into())
        );
        assert_eq!(
            package_hint("\tat journeymap.client.Core.run(Core.java:7)"),
            Some("journeymap".into())
        );
    }

    #[test]
    fn ranks_addons_that_appear_higher_in_the_stack() {
        let hints = vec!["terraforged".to_string(), "journeymap".to_string()];
        let addons = vec![
            "journeymap-1.21.jar".to_string(),
            "TerraForged-1.21.jar".to_string(),
        ];
        assert_eq!(
            match_suspects(&hints, &addons),
            vec!["TerraForged-1.21.jar", "journeymap-1.21.jar"]
        );
    }
}


use crate::addons;
use serde::Serialize;
use std::path::Path;

pub const MODRINTH_ID: &str = "l6YH9Als";

const MOD_ID: &str = "spark";

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SparkInfo {
    pub available: bool,
    pub bundled: bool,
    pub installable: bool,
    pub report: Option<SparkReport>,
}

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SparkReport {
    pub url: String,
    pub at: u64,
}

fn bundled_in(core: &str, version: &str) -> bool {
    if !matches!(core, "Paper" | "Purpur" | "Folia") {
        return false;
    }
    let mut parts = version.split('.').map(|part| part.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    major > 1 || (major == 1 && minor >= 21)
}

pub fn info(core: &str, version: &str, server: &Path) -> SparkInfo {
    let has_jar = matches!(core, "Paper" | "Purpur" | "Folia" | "NeoForge" | "Fabric" | "Forge")
        && addons::scan(server)
            .iter()
            .any(|one| one.enabled && one.mod_id.eq_ignore_ascii_case(MOD_ID));
    info_with(core, version, has_jar)
}

pub fn jar_name_is_spark(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("spark") && lower.ends_with(".jar")
}

pub fn info_with(core: &str, version: &str, has_jar: bool) -> SparkInfo {
    let bundled = bundled_in(core, version);
    let installable = matches!(core, "NeoForge" | "Fabric" | "Forge");
    let installed = bundled || has_jar;
    SparkInfo {
        available: installed,
        bundled,
        installable,
        report: None,
    }
}

pub fn report_url(line: &str) -> Option<String> {
    let start = line.find("https://spark.lucko.me/")?;
    let rest = &line[start..];
    let end = rest
        .find(|character: char| !(character.is_ascii_alphanumeric() || matches!(character, '/' | ':' | '.' | '-' | '_' | '#' | '?' | '=')))
        .unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches(['.', ',', ')']);
    (url.len() > "https://spark.lucko.me/".len()).then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_where_paper_bundles_it() {
        assert!(bundled_in("Paper", "1.21"));
        assert!(bundled_in("Purpur", "1.21.8"));
        assert!(bundled_in("Folia", "26.2"));
        assert!(!bundled_in("Paper", "1.20.6"));
        assert!(!bundled_in("NeoForge", "26.2"));
        assert!(!bundled_in("Vanilla", "26.2"));
    }

    #[test]
    fn picks_the_result_url_out_of_a_console_line() {
        assert_eq!(
            report_url("[12:00:01 INFO]: [⚡] https://spark.lucko.me/AbCdEf12").as_deref(),
            Some("https://spark.lucko.me/AbCdEf12")
        );
        assert_eq!(
            report_url("[12:00:01 INFO]: [⚡] Profiler results: https://spark.lucko.me/xyz. Done").as_deref(),
            Some("https://spark.lucko.me/xyz")
        );
        assert_eq!(report_url("visit https://spark.lucko.me/ for docs"), None);
        assert_eq!(report_url("[⚡] Profiler has been started, please wait..."), None);
    }

    #[test]
    fn a_fresh_folder_has_no_spark_unless_bundled() {
        let folder = std::env::temp_dir().join(format!("se-spark-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("mods")).unwrap();
        let none = info("NeoForge", "1.21.1", &folder);
        assert!(!none.available && none.installable && !none.bundled);
        let paper = info("Paper", "26.2", &folder);
        assert!(paper.available && paper.bundled && !paper.installable);
        assert!(!info("Vanilla", "26.2", &folder).available);
        std::fs::remove_dir_all(&folder).ok();
    }
}

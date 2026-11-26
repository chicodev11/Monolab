//! Pumpkin Market (market.pumpkinmc.org): where Pumpkin's wasm plugins are published.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

pub const SOURCE: &str = "Pumpkin Market";
const SITE: &str = "https://market.pumpkinmc.org";
const PREFIX: &str = "market-";
pub const PAGE: u32 = 20;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Installed {
    pub plugin: u64,
    pub version: String,
    pub title: String,
    #[serde(default)]
    pub icon: Option<String>,
}

pub fn plugin_of(project_id: &str) -> Option<u64> {
    project_id.strip_prefix(PREFIX)?.parse().ok()
}

pub fn project_id(plugin: u64) -> String {
    format!("{PREFIX}{plugin}")
}

pub fn download_url(plugin: u64) -> String {
    format!("{SITE}/api/v1/rest/plugins/{plugin}/download")
}

pub fn page_url(plugin: u64) -> String {
    format!("{SITE}/plugin/{plugin}")
}

/// Market names hold spaces and symbols ("PumpkinTAB | Tab & Scoreboard").
pub fn file_name(title: &str) -> String {
    let mut out = String::new();
    for letter in title.chars() {
        if letter.is_ascii_alphanumeric() || matches!(letter, '-' | '_' | '.') {
            out.push(letter);
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    let stem = out.trim_matches(['_', '.']);
    format!("{}.wasm", if stem.is_empty() { "plugin" } else { stem })
}

pub fn is_wasm(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0_u8; 4];
    std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut head)).is_ok() && &head == b"\0asm"
}

pub fn same_version(one: &str, other: &str) -> bool {
    one.trim().trim_start_matches(['v', 'V']) == other.trim().trim_start_matches(['v', 'V'])
}

/// Whether the market's `latest` should replace what is `installed`. Plain dotted numbers
/// (`0.6.1`) are compared as numbers, so an older listing never offers a "downgrade"; anything else
/// that differs counts as an update.
pub fn needs_update(installed: &str, latest: &str) -> bool {
    if latest.trim().is_empty() || same_version(installed, latest) {
        return false;
    }
    match (numbers(installed), numbers(latest)) {
        (Some(have), Some(next)) => {
            let width = have.len().max(next.len());
            let pad = |list: Vec<u64>| list.into_iter().chain(std::iter::repeat(0)).take(width).collect::<Vec<_>>();
            pad(next) > pad(have)
        }
        _ => true,
    }
}

fn numbers(version: &str) -> Option<Vec<u64>> {
    version.trim().trim_start_matches(['v', 'V']).split('.').map(|part| part.parse().ok()).collect()
}

pub fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

pub fn icon_of(value: &Value) -> Option<String> {
    ["preview_path", "preview_url"]
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|one| one.starts_with("https://"))
        .map(str::to_owned)
}

pub fn description_of(value: &Value) -> String {
    let Some(all) = value.get("translated_descriptions").and_then(Value::as_object) else {
        return String::new();
    };
    ["ko-KR", "ko", "en-US"]
        .iter()
        .find_map(|key| all.get(*key))
        .or_else(|| all.values().next())
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub fn is_free(value: &Value) -> bool {
    text(value, "type") == "free" && value.get("price_cents").and_then(Value::as_u64).unwrap_or(0) == 0
}

/// The description on one line, without Markdown marks, for cards and the page head.
pub fn short_of(value: &Value) -> String {
    let plain = description_of(value).replace(['#', '*', '`'], "");
    plain.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(220).collect()
}

pub fn title_of(value: &Value) -> String {
    text(value, "name").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The plugin's source link, only when it is a web address.
pub fn source_of(value: &Value) -> Option<String> {
    Some(text(value, "source_link")).filter(|one| one.starts_with("https://"))
}

pub fn hit_of(value: &Value) -> Option<crate::catalog::CatalogHit> {
    let plugin = value.get("id")?.as_u64()?;
    Some(crate::catalog::CatalogHit {
        source: SOURCE.into(),
        project_id: project_id(plugin),
        title: title_of(value),
        description: short_of(value),
        project_type: "plugin".into(),
        installable: is_free(value),
        page_url: page_url(plugin),
        icon_url: icon_of(value),
        author: Some(text(value, "dev_name")).filter(|one| !one.is_empty()),
        downloads: value.get("downloads").and_then(Value::as_u64),
        follows: None,
        updated_at: None,
        loaders: vec!["pumpkin".into()],
        versions: Vec::new(),
    })
}

/// The listing the site itself uses: unlike the REST one, it carries descriptions.
pub fn search(client: &reqwest::blocking::Client, query: &str, page: u32, category: &str) -> Result<Vec<crate::catalog::CatalogHit>, String> {
    let offset = (page * PAGE).to_string();
    let limit = PAGE.to_string();
    let mut asked = vec![("limit", limit.as_str()), ("offset", offset.as_str())];
    if !query.trim().is_empty() {
        asked.push(("q", query.trim()));
    }
    if !category.is_empty() {
        asked.push(("category", category));
    }
    let found: Value = client
        .get(format!("{SITE}/api/plugins"))
        .query(&asked)
        .send()
        .map_err(|error| format!("Pumpkin Market search failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Pumpkin Market search failed: {error}"))?
        .json()
        .map_err(|error| format!("Pumpkin Market search failed: {error}"))?;
    Ok(found.as_array().into_iter().flatten().filter_map(hit_of).collect())
}

pub fn detail(client: &reqwest::blocking::Client, plugin: u64) -> Result<Value, String> {
    client
        .get(format!("{SITE}/api/plugins/{plugin}"))
        .send()
        .map_err(|error| format!("Could not fetch the plugin details: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Could not fetch the plugin details: {error}"))?
        .json()
        .map_err(|error| format!("Could not fetch the plugin details: {error}"))
}

pub fn latest_version(client: &reqwest::blocking::Client, plugin: u64) -> Result<String, String> {
    let found: Value = client
        .get(format!("{SITE}/api/v1/rest/plugins/{plugin}/metadata"))
        .send()
        .map_err(|error| format!("Could not fetch the plugin details: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Could not fetch the plugin details: {error}"))?
        .json()
        .map_err(|error| format!("Could not fetch the plugin details: {error}"))?;
    Ok(text(&found, "version"))
}

/// The page text: the description, then the plugin's commands.
pub fn body_of(value: &Value) -> String {
    let mut body = description_of(value);
    let rows: Vec<String> = value
        .get("commands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|one| {
            let name = one.get("name")?.as_str()?;
            let about = one.get("description").map(description_of_map).unwrap_or_default();
            Some(if about.is_empty() { format!("- `/{name}`") } else { format!("- `/{name}`: {about}") })
        })
        .collect();
    if !rows.is_empty() {
        body.push_str("\n\n### Commands\n\n");
        body.push_str(&rows.join("\n"));
    }
    body
}

fn description_of_map(value: &Value) -> String {
    let Some(all) = value.as_object() else {
        return String::new();
    };
    all.get("ko-KR").or_else(|| all.get("en-US")).or_else(|| all.values().next()).and_then(Value::as_str).unwrap_or_default().to_string()
}

pub fn read_installed(file: &Path) -> HashMap<String, Installed> {
    crate::store::read(file).ok().flatten().unwrap_or_default()
}

pub fn write_installed(file: &Path, all: &HashMap<String, Installed>) -> Result<(), String> {
    let text = serde_json::to_string_pretty(all).map_err(|error| error.to_string())?;
    crate::store::write(file, &text).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_a_listing_row() {
        let hit = hit_of(&json!({
            "id": 77, "name": "PumpkinPIE  | Placeholders", "type": "free", "price_cents": 0, "downloads": 40, "dev_name": "Rennex",
            "preview_path": "https://assets.pumpkinmc.org/i.webp", "translated_descriptions": { "en-US": "Line one.\n\nLine two." },
        }))
        .unwrap();
        assert_eq!(hit.project_id, "market-77");
        assert_eq!(hit.title, "PumpkinPIE | Placeholders");
        assert_eq!(hit.description, "Line one. Line two.");
        assert!(hit.installable);
        assert_eq!(hit.source, SOURCE);
        assert_eq!(plugin_of(&hit.project_id), Some(77));
    }

    #[test]
    fn paid_plugins_only_open_their_page() {
        assert!(!is_free(&json!({ "type": "paid", "price_cents": 500 })));
        assert!(!is_free(&json!({ "type": "adwall" })));
        assert!(is_free(&json!({ "type": "free", "price_cents": 0 })));
    }

    #[test]
    fn ignores_other_project_ids() {
        assert_eq!(plugin_of("AANobbMI"), None);
        assert_eq!(plugin_of("market-x"), None);
    }

    #[test]
    fn names_a_safe_file() {
        assert_eq!(file_name("PumpkinTAB | Tab & Scoreboard"), "PumpkinTAB_Tab_Scoreboard.wasm");
        assert_eq!(file_name("../../evil"), "evil.wasm");
        assert_eq!(file_name("   "), "plugin.wasm");
        assert_eq!(file_name("wasm!"), "wasm.wasm");
    }

    #[test]
    fn a_v_prefix_is_the_same_version() {
        assert!(same_version("v0.2.4", "0.2.4"));
        assert!(!same_version("0.2.4", "0.2.5"));
    }

    #[test]
    fn offers_only_newer_versions() {
        assert!(needs_update("0.6.0", "0.6.1"));
        assert!(needs_update("v0.9", "0.10.0"));
        assert!(needs_update("1.2", "1.2.1"));
        assert!(!needs_update("0.6.1", "v0.6.1"));
        assert!(!needs_update("1.2.0", "1.2"));
        assert!(!needs_update("0.6.1", "0.6.0"), "an older listing is not an update");
        assert!(!needs_update("0.6.1", ""), "no version given");
        assert!(needs_update("0.6.1-beta", "0.6.1"), "versions that aren't plain numbers just differ");
        assert!(needs_update("", "1.0.0"));
    }

    #[test]
    fn reads_a_plugin_page() {
        let about = json!({ "name": "Multi  version", "source_link": "https://github.com/x/y", "translated_descriptions": { "en-US": "# Title\n\n**Bold** text" } });
        assert_eq!(title_of(&about), "Multi version");
        assert_eq!(short_of(&about), "Title Bold text");
        assert_eq!(source_of(&about).as_deref(), Some("https://github.com/x/y"));
        assert_eq!(source_of(&json!({ "source_link": "javascript:alert(1)" })), None);
    }

    #[test]
    fn lists_commands_on_the_page() {
        let body = body_of(&json!({
            "translated_descriptions": { "en-US": "About" },
            "commands": [{ "name": "pie parse <text>", "description": { "en-US": "Resolves it" } }],
        }));
        assert!(body.starts_with("About"));
        assert!(body.contains("- `/pie parse <text>`: Resolves it"));
    }
}

use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table, Value};

use crate::properties::{PropertyInfo, ValueKind};

pub const CONFIG: &str = "pumpkin.toml";
pub const DATA_DIR: &str = "data";
pub const DISABLED: &str = ".deactivated";
const PLUGIN_EXTENSIONS: &[&str] = &["wasm", "dll"];

pub fn is_pumpkin(core: &str) -> bool {
    core == "Pumpkin"
}

fn read(folder: &Path) -> Result<DocumentMut, String> {
    let path = folder.join(CONFIG);
    let text = fs::read_to_string(&path).map_err(|error| format!("Could not read {CONFIG}: {error}"))?;
    text.parse::<DocumentMut>().map_err(|error| format!("{CONFIG} is not valid: {error}"))
}

fn write(folder: &Path, document: &DocumentMut) -> Result<(), String> {
    fs::write(folder.join(CONFIG), document.to_string()).map_err(|error| format!("Could not save {CONFIG}: {error}"))
}

fn lookup<'a>(document: &'a DocumentMut, key: &str) -> Option<&'a Item> {
    let mut item: &Item = document.as_item();
    for part in key.split('.') {
        item = item.get(part)?;
    }
    Some(item)
}

fn lookup_mut<'a>(document: &'a mut DocumentMut, key: &str) -> Option<&'a mut Item> {
    let mut item: &mut Item = document.as_item_mut();
    for part in key.split('.') {
        item = item.get_mut(part)?;
    }
    Some(item)
}

fn port_of_address(address: &str) -> Option<u16> {
    address.trim().rsplit_once(':')?.1.parse().ok()
}

pub fn port(folder: &Path) -> Option<u16> {
    let document = read(folder).ok()?;
    lookup(&document, "networking.java.address").and_then(Item::as_str).and_then(port_of_address)
}

pub fn write_port(folder: &Path, port: u16) -> Result<(), String> {
    if !folder.join(CONFIG).is_file() {
        return Ok(());
    }
    let mut document = read(folder)?;
    let host = lookup(&document, "networking.java.address")
        .and_then(Item::as_str)
        .and_then(|address| address.rsplit_once(':').map(|(host, _)| host.to_string()))
        .unwrap_or_else(|| "0.0.0.0".into());
    set_value(&mut document, "networking.java.address", &format!("{host}:{port}"))?;
    write(folder, &document)
}

pub fn level_name(folder: &Path) -> String {
    read(folder)
        .ok()
        .and_then(|document| lookup(&document, "default_level_name").and_then(Item::as_str).map(str::to_owned))
        .filter(|value| !value.is_empty() && !value.contains(['/', '\\', ':']))
        .unwrap_or_else(|| "world".into())
}

pub fn rcon(folder: &Path) -> Option<(u16, String)> {
    let document = read(folder).ok()?;
    let enabled = lookup(&document, "networking.rcon.enabled").and_then(Item::as_bool)?;
    if !enabled {
        return None;
    }
    let port = lookup(&document, "networking.rcon.address").and_then(Item::as_str).and_then(port_of_address).unwrap_or(25575);
    let password = lookup(&document, "networking.rcon.password").and_then(Item::as_str).unwrap_or_default().to_string();
    Some((port, password))
}

pub fn chunk_format(text: &str) -> Option<String> {
    let document: DocumentMut = text.parse().ok()?;
    lookup(&document, "world.chunk.type").and_then(Item::as_str).map(|kind| kind.to_ascii_lowercase())
}

pub fn set_favicon(folder: &Path, file_name: Option<&str>) -> Result<(), String> {
    if !folder.join(CONFIG).is_file() {
        return Ok(());
    }
    let mut document = read(folder)?;
    match file_name {
        Some(name) => {
            document["favicon_path"] = toml_edit::value(name);
            document["use_favicon"] = toml_edit::value(true);
        }
        None => {
            document.remove("favicon_path");
        }
    }
    write(folder, &document)
}

/// `pumpkin.toml` text with `[world.generators]` `"minecraft:overworld"` set to `monolab:<id>` (the name the add-on
/// offers with the fork's `register-world-generator`), or (`None`) with Monolab's taken out. Needs the `[world]`
/// settings Pumpkin writes: a `[world]` with only generators in it wouldn't load.
pub fn set_world_generator(text: &str, id: Option<&str>) -> Result<String, String> {
    const KEY: &str = "minecraft:overworld";
    let mut document = text.parse::<DocumentMut>().map_err(|error| format!("{CONFIG} is not valid: {error}"))?;
    match id {
        Some(id) => {
            let Some(world) = document.get_mut("world").and_then(Item::as_table_like_mut) else {
                return Err(format!(
                    "{CONFIG} has no [world] settings yet. Start the server once so Pumpkin writes them, then save again."
                ));
            };
            if world.get("generators").is_none() {
                world.insert("generators", Item::Table(Table::new()));
            }
            let Some(generators) = world.get_mut("generators").and_then(Item::as_table_like_mut) else {
                return Err(format!("world.generators in {CONFIG} isn't a table."));
            };
            generators.insert(KEY, toml_edit::value(format!("{}:{id}", crate::events::PUMPKIN_PLUGIN_NAME)));
        }
        None => {
            let ours = lookup(&document, "world.generators")
                .and_then(|one| one.get(KEY))
                .and_then(Item::as_str)
                .is_some_and(|one| one.starts_with(&format!("{}:", crate::events::PUMPKIN_PLUGIN_NAME)));
            if !ours {
                return Ok(text.to_string());
            }
            if let Some(generators) = lookup_mut(&mut document, "world.generators").and_then(Item::as_table_like_mut) {
                generators.remove(KEY);
            }
        }
    }
    Ok(document.to_string())
}

/// Pre-approves (or takes back) one permission for one plugin in `pumpkin.toml` text, under
/// `[plugins.overrides.<plugin>] allowed_permissions`. Pumpkin asks at the console otherwise, and nobody is
/// there when Monolab starts the server, so it refuses. Only that plugin's own list changes.
pub fn set_plugin_permission(text: &str, plugin: &str, permission: &str, allow: bool) -> Result<String, String> {
    let mut document = text.parse::<DocumentMut>().map_err(|error| format!("{CONFIG} is not valid: {error}"))?;
    if !allow {
        // Walking with `get_mut` would add empty tables, so look first and leave the text alone when there's nothing to take back.
        let key = format!("plugins.overrides.{plugin}.allowed_permissions");
        let listed =
            lookup(&document, &key).and_then(Item::as_array).is_some_and(|list| list.iter().any(|one| one.as_str() == Some(permission)));
        if !listed {
            return Ok(text.to_string());
        }
        if let Some(list) = lookup_mut(&mut document, &key).and_then(Item::as_array_mut) {
            list.retain(|one| one.as_str() != Some(permission));
        }
        return Ok(document.to_string());
    }
    let plugins = document
        .entry("plugins")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| "[plugins] is not a table".to_string())?;
    let overrides = plugins
        .entry("overrides")
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| "[plugins.overrides] is not a table".to_string())?;
    let own = overrides
        .entry(plugin)
        .or_insert(Item::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| format!("[plugins.overrides.{plugin}] is not a table"))?;
    let list = own
        .entry("allowed_permissions")
        .or_insert(toml_edit::value(toml_edit::Array::new()))
        .as_array_mut()
        .ok_or_else(|| "allowed_permissions is not an array".to_string())?;
    if !list.iter().any(|one| one.as_str() == Some(permission)) {
        list.push(permission);
    }
    Ok(document.to_string())
}

/// What a Pumpkin console line says about plugin permissions (lines as Monolab shows them, colors taken out).
#[derive(Debug, PartialEq)]
pub enum PermissionLine {
    /// `Plugin "Name" (1.0) requests the following permissions:`
    Asks(String),
    /// `  - fs.read.data: Read files…` under that line.
    Wants(String),
    /// `Permission denied for plugin "Name", skipping loading.`
    Denied(String),
}

pub fn permission_line(line: &str) -> Option<PermissionLine> {
    let quoted =
        |text: &str, after: &str| text.split_once(after).and_then(|(_, rest)| rest.split_once('"')).map(|(name, _)| name.to_string());
    if line.contains("requests the following permissions:") {
        return quoted(line, "Plugin \"").map(PermissionLine::Asks);
    }
    if line.contains("Permission denied for plugin \"") && line.contains("skipping loading") {
        return quoted(line, "Permission denied for plugin \"").map(PermissionLine::Denied);
    }
    let rest = line.trim_start().strip_prefix("- ")?;
    let (name, _) = rest.split_once(": ")?;
    let fine =
        !name.is_empty() && name.len() <= 64 && name.chars().all(|one| one.is_ascii_alphanumeric() || matches!(one, '.' | '_' | '-' | ':'));
    fine.then(|| PermissionLine::Wants(name.to_string()))
}

/// Permissions in `plugins/permission_cache.json` that Pumpkin turned down (entries are by file hash, not
/// name, so this is every refused plugin's together).
pub fn refused_permissions(cache: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(cache) else { return Vec::new() };
    let mut out: Vec<String> = value
        .get("entries")
        .and_then(serde_json::Value::as_object)
        .map(|entries| {
            entries
                .values()
                .filter(|one| one.get("approved").and_then(serde_json::Value::as_bool) == Some(false))
                .filter_map(|one| one.get("permissions_requested").and_then(serde_json::Value::as_array))
                .flatten()
                .filter_map(|one| one.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out.dedup();
    out
}

/// Whether `pumpkin.toml` text pre-approves this permission for this plugin (or trusts the plugin fully).
pub fn plugin_permission_allowed(text: &str, plugin: &str, permission: &str) -> bool {
    let Ok(document) = text.parse::<DocumentMut>() else { return false };
    let listed = lookup(&document, &format!("plugins.overrides.{plugin}.allowed_permissions"))
        .and_then(Item::as_array)
        .is_some_and(|list| list.iter().any(|one| one.as_str() == Some(permission)));
    let everyone = lookup(&document, "plugins.allowed_permissions")
        .and_then(Item::as_array)
        .is_some_and(|list| list.iter().any(|one| one.as_str() == Some(permission)));
    let trusted = lookup(&document, &format!("plugins.overrides.{plugin}.trusted")).and_then(Item::as_bool) == Some(true);
    listed || everyone || trusted
}

const COMMON: &[&str] = &[
    "default_gamemode",
    "default_difficulty",
    "force_gamemode",
    "hardcore",
    "white_list",
    "enforce_whitelist",
    "allow_nether",
    "allow_end",
    "seed",
    "tps",
    "networking.java.address",
    "networking.java.online_mode",
    "networking.java.max_players",
    "networking.java.view_distance",
    "networking.java.simulation_distance",
    "networking.java.motd",
    "networking.bedrock.enabled",
    "networking.rcon.enabled",
    "networking.rcon.address",
    "networking.rcon.password",
    "networking.query.enabled",
    "pvp.enabled",
    "chat.format",
    "plugins.enabled",
    "plugins.hot_reload",
];

fn choices(key: &str) -> Option<&'static [&'static str]> {
    match key {
        "default_difficulty" => Some(&["Peaceful", "Easy", "Normal", "Hard"]),
        "default_gamemode" => Some(&["Survival", "Creative", "Adventure", "Spectator"]),
        "logging.level" => Some(&["trace", "debug", "info", "warn", "error"]),
        _ => None,
    }
}

fn bounds(key: &str) -> (Option<i64>, Option<i64>) {
    match key.rsplit('.').next().unwrap_or(key) {
        "view_distance" | "simulation_distance" => (Some(2), Some(32)),
        "max_players" => (Some(1), None),
        "op_permission_level" | "default_op_level" => (Some(0), Some(4)),
        "level" if key.contains("compression") => (Some(0), Some(9)),
        _ => (None, None),
    }
}

fn value_text(value: &Value) -> Option<String> {
    Some(match value {
        Value::String(text) => text.value().to_string(),
        Value::Integer(number) => number.value().to_string(),
        Value::Float(number) => {
            let number = *number.value();
            if number.fract() == 0.0 {
                format!("{number:.1}")
            } else {
                number.to_string()
            }
        }
        Value::Boolean(flag) => flag.value().to_string(),
        Value::Array(array) => {
            let mut parts = Vec::new();
            for one in array.iter() {
                parts.push(one.as_str()?.to_string());
            }
            parts.join(", ")
        }
        Value::Datetime(_) | Value::InlineTable(_) => return None,
    })
}

fn kind_of(value: &Value) -> ValueKind {
    match value {
        Value::Boolean(_) => ValueKind::Toggle,
        Value::Integer(_) | Value::Float(_) => ValueKind::Number,
        _ => ValueKind::Text,
    }
}

fn walk(table: &Table, prefix: &str, out: &mut Vec<PropertyInfo>) {
    for (name, item) in table.iter() {
        let key = if prefix.is_empty() { name.to_string() } else { format!("{prefix}.{name}") };
        match item {
            Item::Value(value) => {
                let Some(text) = value_text(value) else {
                    continue;
                };
                let (kind, options) = match choices(&key) {
                    Some(list) => (ValueKind::Choice, list.iter().map(|one| one.to_string()).collect()),
                    None => (kind_of(value), Vec::new()),
                };
                let (min, max) = bounds(&key);
                out.push(PropertyInfo { key: key.clone(), value: text, kind, options, min, max, common: COMMON.contains(&key.as_str()) });
            }
            Item::Table(inner) => walk(inner, &key, out),
            Item::ArrayOfTables(_) | Item::None => {}
        }
    }
}

pub fn describe(folder: &Path) -> Result<Vec<PropertyInfo>, String> {
    if !folder.join(CONFIG).is_file() {
        return Ok(Vec::new());
    }
    let document = read(folder)?;
    let mut out = Vec::new();
    walk(document.as_table(), "", &mut out);
    Ok(out)
}

fn set_value(document: &mut DocumentMut, key: &str, text: &str) -> Result<(), String> {
    let Some(item) = lookup_mut(document, key) else {
        return Err(format!("{key} is not in {CONFIG}."));
    };
    let Some(current) = item.as_value_mut() else {
        return Err(format!("{key} is a table, not a value."));
    };
    let text = text.trim();
    let next: Value = match current {
        Value::Boolean(_) => match text {
            "true" => true.into(),
            "false" => false.into(),
            _ => return Err(format!("{key} only accepts true or false.")),
        },
        Value::Integer(_) => text.parse::<i64>().map(Value::from).map_err(|_| format!("{key} only accepts a whole number."))?,
        Value::Float(_) => text.parse::<f64>().map(Value::from).map_err(|_| format!("{key} only accepts a number."))?,
        Value::String(_) => text.into(),
        Value::Array(_) => {
            let mut array = toml_edit::Array::new();
            for part in text.split(',').map(str::trim).filter(|part| !part.is_empty()) {
                array.push(part);
            }
            Value::Array(array)
        }
        Value::Datetime(_) | Value::InlineTable(_) => {
            return Err(format!("{key} cannot be edited here."));
        }
    };
    let decor = current.decor().clone();
    *current = next;
    *current.decor_mut() = decor;
    Ok(())
}

pub fn save(folder: &Path, entries: &[(String, String)]) -> Result<(), String> {
    let mut document = read(folder)?;
    for (key, value) in entries {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        set_value(&mut document, key, value)?;
    }
    write(folder, &document)
}

pub fn plugin_state(file_name: &str) -> Option<bool> {
    let lower = file_name.to_ascii_lowercase();
    let (stem, enabled) = match lower.strip_suffix(DISABLED) {
        Some(stem) => (stem.to_string(), false),
        None => (lower, true),
    };
    PLUGIN_EXTENSIONS.iter().any(|ext| stem.ends_with(&format!(".{ext}"))).then_some(enabled)
}

pub fn toggled_name(file_name: &str) -> String {
    match file_name.strip_suffix(DISABLED) {
        Some(stem) => stem.to_string(),
        None => format!("{file_name}{DISABLED}"),
    }
}

pub fn plugin_display_name(file_name: &str) -> String {
    let stem = file_name.strip_suffix(DISABLED).unwrap_or(file_name);
    stem.rsplit_once('.').map(|(name, _)| name).unwrap_or(stem).to_string()
}

pub fn player_lists_dir(folder: &Path) -> PathBuf {
    folder.join(DATA_DIR)
}

pub fn version_from_log(folder: &Path) -> Option<String> {
    let text = fs::read_to_string(folder.join("logs").join("latest.log")).ok()?;
    text.lines().find_map(|line| {
        let rest = line.split("Starting Pumpkin ").nth(1)?;
        let version = rest.split_whitespace().next()?;
        (!version.is_empty()).then(|| version.to_string())
    })
}

pub fn level_of(line: &str) -> Option<&'static str> {
    let trimmed = line.trim_start();
    let head = if let Some(rest) = trimmed.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        let mut words = trimmed.split_whitespace();
        let first = words.next()?;
        let second = words.next()?;
        if first.len() == 10 && first.matches('-').count() == 2 {
            words.next()?
        } else if first.len() == 8 && first.matches(':').count() == 2 {
            second
        } else {
            return None;
        }
    };
    match head {
        "ERROR" => Some("error"),
        "WARN" => Some("warn"),
        "INFO" | "DEBUG" | "TRACE" => Some("info"),
        _ => None,
    }
}

pub fn body_of(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix('[') {
        let (level, body) = rest.split_once("] ")?;
        return matches!(level, "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE").then(|| body.trim_start());
    }
    level_of(trimmed)?;
    let mut rest = trimmed;
    let mut words = 0;
    while words < 3 {
        let (word, tail) = rest.trim_start().split_once(' ')?;
        rest = tail;
        words += 1;
        if matches!(word, "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE") {
            break;
        }
    }
    let rest = rest.trim_start();
    if let Some((_, after)) = rest.split_once("ThreadId(") {
        let after = after.split_once(')')?.1.trim_start();
        return Some(after.split_once(": ").map(|(_, body)| body).unwrap_or(after));
    }
    Some(rest)
}

#[cfg(test)]
mod permission_line_tests {
    use super::*;

    #[test]
    fn reads_what_pumpkin_says_about_permissions() {
        assert_eq!(
            permission_line("Plugin \"Chat Plus\" (1.2.0) requests the following permissions:"),
            Some(PermissionLine::Asks("Chat Plus".into()))
        );
        assert_eq!(
            permission_line("  - fs.write.data: Write files in the plugin's data folder"),
            Some(PermissionLine::Wants("fs.write.data".into()))
        );
        assert_eq!(
            permission_line("[2026-11-21T10:00:00Z WARN  pumpkin::plugin] Permission denied for plugin \"Chat Plus\", skipping loading."),
            Some(PermissionLine::Denied("Chat Plus".into()))
        );
        assert_eq!(permission_line("[INFO] Steve joined the game"), None);
        assert_eq!(permission_line("  - some text: with a colon"), None);
    }

    #[test]
    fn lists_refused_permissions_from_the_cache() {
        let cache = r#"{"entries":{"a":{"permissions_requested":["fs.read.data","fs.write.data"],"approved":false},"b":{"permissions_requested":["net.http"],"approved":true},"c":{"permissions_requested":["fs.read.data"],"approved":false}}}"#;
        assert_eq!(refused_permissions(cache), ["fs.read.data", "fs.write.data"]);
        assert!(refused_permissions("not json").is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"seed = "1788633241162500297"
default_difficulty = "Normal"
tps = 20.0
default_gamemode = "Survival" # starting mode
white_list = false

[networking.java]
address = "0.0.0.0:25565"
online_mode = true
max_players = 1000
motd = "A blazingly fast Pumpkin server!"

[networking.java.authentication]
fallbacks = []

[networking.rcon]
enabled = true
address = "0.0.0.0:25575"
password = "hunter2"

[plugins]
enabled = true
allowed_permissions = []

[plugins.overrides]
"#;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("se-pumpkin-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(CONFIG), SAMPLE).unwrap();
        dir
    }

    #[test]
    fn knows_the_chunk_format() {
        assert_eq!(
            chunk_format(
                "[world.chunk]
type = \"pump\"
"
            )
            .as_deref(),
            Some("pump")
        );
        assert_eq!(
            chunk_format(
                "[world.chunk]
type = \"anvil\"
"
            )
            .as_deref(),
            Some("anvil")
        );
        assert_eq!(
            chunk_format(
                "java_edition = true
"
            ),
            None
        );
    }

    #[test]
    fn reads_the_toml_as_rows() {
        let dir = folder("rows");
        let rows = describe(&dir).unwrap();
        let find = |key: &str| rows.iter().find(|row| row.key == key).unwrap();
        assert_eq!(find("default_gamemode").value, "Survival");
        assert_eq!(find("default_gamemode").kind, ValueKind::Choice);
        assert_eq!(find("white_list").kind, ValueKind::Toggle);
        assert_eq!(find("tps").value, "20.0");
        assert_eq!(find("networking.java.max_players").kind, ValueKind::Number);
        assert!(find("networking.java.max_players").common);
        assert_eq!(find("networking.java.authentication.fallbacks").value, "");
        assert_eq!(port(&dir), Some(25565));
        assert_eq!(rcon(&dir), Some((25575, "hunter2".into())));
        assert_eq!(level_name(&dir), "world");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn saves_in_place_and_keeps_types() {
        let dir = folder("save");
        save(
            &dir,
            &[
                ("default_gamemode".into(), "Creative".into()),
                ("white_list".into(), "true".into()),
                ("tps".into(), "30".into()),
                ("networking.java.max_players".into(), "20".into()),
                ("networking.java.authentication.fallbacks".into(), "a, b".into()),
            ],
        )
        .unwrap();
        let text = fs::read_to_string(dir.join(CONFIG)).unwrap();
        assert!(text.contains("default_gamemode = \"Creative\" # starting mode"));
        assert!(text.contains("white_list = true"));
        assert!(text.contains("tps = 30.0"));
        assert!(text.contains("max_players = 20"));
        assert!(text.contains("fallbacks = [\"a\", \"b\"]"));
        assert!(text.starts_with("seed = "));
        assert!(save(&dir, &[("white_list".into(), "yes".into())]).is_err());
        assert!(save(&dir, &[("networking.java.max_players".into(), "many".into())]).is_err());

        write_port(&dir, 25599).unwrap();
        assert_eq!(port(&dir), Some(25599));
        set_favicon(&dir, Some("server-icon.png")).unwrap();
        assert!(fs::read_to_string(dir.join(CONFIG)).unwrap().contains("favicon_path = \"server-icon.png\""));
        set_favicon(&dir, None).unwrap();
        assert!(!fs::read_to_string(dir.join(CONFIG)).unwrap().contains("favicon_path"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn preapproves_only_our_plugin() {
        let dir = folder("perm");
        let original = fs::read_to_string(dir.join(CONFIG)).unwrap();
        assert!(!plugin_permission_allowed(&original, "monolab", "fs.write.data"));
        let once = set_plugin_permission(&original, "monolab", "fs.write.data", true).unwrap();
        let text = set_plugin_permission(&once, "monolab", "fs.write.data", true).unwrap();
        assert!(plugin_permission_allowed(&text, "monolab", "fs.write.data"));
        assert!(!plugin_permission_allowed(&text, "other", "fs.write.data"));
        // Taking it back leaves the rest of the file as it was.
        let back = set_plugin_permission(&text, "monolab", "fs.write.data", false).unwrap();
        assert!(!plugin_permission_allowed(&back, "monolab", "fs.write.data"));
        assert!(back.starts_with("seed = "));
        assert_eq!(set_plugin_permission(&original, "monolab", "fs.write.data", false).unwrap(), original);
        assert!(plugin_permission_allowed(
            "[plugins.overrides.monolab]
trusted = true
",
            "monolab",
            "fs.write.data"
        ));
        assert!(text.contains("[plugins.overrides.monolab]"));
        assert_eq!(text.matches("fs.write.data").count(), 1);
        // The global list stays empty.
        let document: DocumentMut = text.parse().unwrap();
        assert!(lookup(&document, "plugins.allowed_permissions").and_then(|item| item.as_array()).is_some_and(|list| list.is_empty()));
        assert!(text.starts_with("seed = "));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore]
    fn preapproves_in_a_real_pumpkin_folder() {
        let Ok(dir) = std::env::var("SE_PUMPKIN_DIR") else {
            return;
        };
        let path = Path::new(&dir).join(CONFIG);
        let text = set_plugin_permission(&fs::read_to_string(&path).unwrap(), "monolab", "fs.write.data", true).unwrap();
        fs::write(&path, &text).unwrap();
        println!("{}", &text[text.find("[plugins]").unwrap_or(0)..]);
    }

    #[test]
    fn knows_its_plugin_files() {
        assert_eq!(plugin_state("hello.wasm"), Some(true));
        assert_eq!(plugin_state("hello.WASM.deactivated"), Some(false));
        assert_eq!(plugin_state("native.dll"), Some(true));
        assert_eq!(plugin_state("permission_cache.json"), None);
        assert_eq!(plugin_state("old.jar"), None);
        assert_eq!(toggled_name("hello.wasm"), "hello.wasm.deactivated");
        assert_eq!(toggled_name("hello.wasm.deactivated"), "hello.wasm");
        assert_eq!(plugin_display_name("hello_pumpkin.wasm.deactivated"), "hello_pumpkin");
    }

    #[test]
    #[ignore]
    fn reads_a_real_pumpkin_folder() {
        let dir = PathBuf::from(std::env::var("SE_PUMPKIN_DIR").expect("SE_PUMPKIN_DIR"));
        let rows = describe(&dir).unwrap();
        for row in &rows {
            println!("{:?} {} = {} {:?}", row.kind, row.key, row.value, row.options);
        }
        assert!(rows.iter().any(|row| row.key == "networking.java.address"));
        println!("port {:?} rcon {:?} level {} version {:?}", port(&dir), rcon(&dir), level_name(&dir), version_from_log(&dir));
    }

    #[test]
    fn reads_both_line_shapes() {
        assert_eq!(level_of("[WARN] Pumpkin is currently under heavy development!"), Some("warn"));
        assert_eq!(body_of("[INFO] 5657 joined the game"), Some("5657 joined the game"));
        assert_eq!(level_of("19:33:00  INFO Started server; took 57ms"), Some("info"));
        assert_eq!(body_of("19:33:00 ERROR Failed to save level.dat: Info not found!"), Some("Failed to save level.dat: Info not found!"));
        assert_eq!(body_of("2026-08-26 15:36:37  INFO main ThreadId(01) pumpkin::server: All worlds loaded"), Some("All worlds loaded"));
        assert_eq!(level_of("[13:45:22] [Server thread/INFO]: hi"), None);
        assert_eq!(body_of("[13:45:22 INFO]: hi"), None);
        assert_eq!(body_of("Preparing spawn area"), None);
        // The fork's Paper-style latest.log is left to the Minecraft readers.
        assert_eq!(level_of("[00:42:56] [main/INFO]: Starting Pumpkin"), None);
        assert_eq!(body_of("[00:42:56] [main/INFO]: Starting Pumpkin"), None);
        assert_eq!(body_of("[main/INFO]: Starting Pumpkin"), None);
    }
}

#[cfg(test)]
mod world_generator_tests {
    use super::*;

    #[test]
    fn sets_and_takes_back_the_overworld_generator() {
        let base = "seed = \"1\"\n\n[world.chunk]\ntype = \"anvil\"\n";
        let set = set_world_generator(base, Some("spiral")).unwrap();
        assert!(set.contains("[world.generators]\n\"minecraft:overworld\" = \"monolab:spiral\""), "{set}");
        assert!(set.contains("[world.chunk]"));
        let again = set_world_generator(&set, Some("isles")).unwrap();
        assert!(again.contains("\"minecraft:overworld\" = \"monolab:isles\"") && !again.contains("spiral"), "{again}");
        let back = set_world_generator(&set, None).unwrap();
        assert!(!back.contains("monolab:"), "{back}");
        let other = "[world.chunk]\ntype = \"anvil\"\n\n[world.generators]\n\"minecraft:overworld\" = \"terra:x\"\n";
        assert_eq!(set_world_generator(other, None).unwrap(), other);
        assert!(set_world_generator("seed = \"1\"\n", Some("a")).is_err());
        assert_eq!(set_world_generator("seed = \"1\"\n", None).unwrap(), "seed = \"1\"\n");
    }
}

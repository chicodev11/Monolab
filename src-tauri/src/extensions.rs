//! `monolab.json` in a plugin or mod folder: declared settings, console actions and status values the app shows.

use crate::integrations::{parse, Yaml};
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Declared {
    name: String,
    settings: Vec<SettingDef>,
    actions: Vec<ActionDef>,
    status: Vec<StatusDef>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct SettingDef {
    file: String,
    key: String,
    label: String,
    r#type: String,
    options: Vec<String>,
    min: Option<f64>,
    max: Option<f64>,
    hint: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ActionDef {
    label: String,
    command: String,
    args: Vec<ArgDef>,
    confirm: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ArgDef {
    name: String,
    label: String,
    r#type: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct StatusDef {
    file: String,
    key: String,
    label: String,
    unit: String,
}

#[derive(serde::Serialize, specta::Type, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Extension {
    pub folder: String,
    pub name: String,
    pub settings: Vec<ExtSetting>,
    pub actions: Vec<ExtAction>,
    pub status: Vec<ExtStatus>,
    /// Why the file couldn't be used, when it couldn't.
    pub problem: String,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtSetting {
    pub label: String,
    /// number, text, bool or choice
    pub kind: String,
    pub options: Vec<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub hint: String,
    pub file: String,
    pub key: String,
    pub value: Option<String>,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtAction {
    pub label: String,
    /// Console command template (monolab.json); empty for MonolabApi buttons, which call the plugin instead.
    pub command: String,
    /// True for MonolabApi buttons: the app leaves a request for the add-on rather than typing in the console.
    pub call: bool,
    pub args: Vec<ExtArg>,
    pub confirm: String,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtArg {
    pub name: String,
    pub label: String,
    /// player, text or number
    pub kind: String,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExtStatus {
    pub label: String,
    pub unit: String,
    pub value: Option<String>,
}

const KINDS: &[&str] = &["number", "text", "bool", "choice"];
const ARG_KINDS: &[&str] = &["player", "text", "number"];

fn format_of(file: &str) -> &'static str {
    match file.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).as_deref() {
        Some("yml" | "yaml") => "yaml",
        Some("json" | "json5") => "json",
        Some("toml") => "toml",
        Some("properties" | "cfg" | "conf") => "properties",
        _ => "",
    }
}

/// Files a declaration may point at: inside its own folder, a supported format.
pub fn usable_file(file: &str) -> bool {
    !file.is_empty() && !format_of(file).is_empty() && crate::paths::check_relative(file).is_ok() && !file.contains("..")
}

/// Tabs the add-on's MonolabApi wrote to integrations.json; empty when the file is old (server stopped).
pub fn from_api(json: &str, now_ms: u64) -> Vec<Extension> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else { return Vec::new() };
    let at = root.get("at").and_then(|v| v.as_u64()).unwrap_or(0);
    if now_ms.saturating_sub(at) > 120_000 {
        return Vec::new();
    }
    let text = |value: &serde_json::Value, key: &str| value.get(key).and_then(|v| v.as_str()).unwrap_or("").to_string();
    root.get("owners")
        .and_then(|v| v.as_object())
        .map(|owners| {
            owners
                .iter()
                .map(|(owner, data)| {
                    let list = |key: &str| data.get(key).and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    Extension {
                        folder: format!("api:{owner}"),
                        name: owner.clone(),
                        settings: Vec::new(),
                        status: list("status")
                            .iter()
                            .map(|one| ExtStatus { label: text(one, "label"), unit: text(one, "unit"), value: Some(text(one, "value")) })
                            .collect(),
                        actions: list("actions")
                            .iter()
                            .filter(|one| !text(one, "label").is_empty())
                            .map(|one| ExtAction {
                                label: text(one, "label"),
                                command: String::new(),
                                call: true,
                                args: one
                                    .get("args")
                                    .and_then(|v| v.as_array())
                                    .map(|args| {
                                        args.iter()
                                            .map(|arg| ExtArg {
                                                name: text(arg, "name"),
                                                label: text(arg, "label"),
                                                kind: match text(arg, "type").as_str() {
                                                    "player" => "player".into(),
                                                    "number" => "number".into(),
                                                    _ => "text".into(),
                                                },
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default(),
                                confirm: String::new(),
                            })
                            .collect(),
                        problem: String::new(),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Files a declaration reads (settings and status), so the caller can fetch them first.
pub fn declared_files(json: &str) -> Vec<String> {
    let Ok(declared) = serde_json::from_str::<Declared>(json) else { return Vec::new() };
    let mut files: Vec<String> = declared
        .settings
        .into_iter()
        .map(|one| one.file)
        .chain(declared.status.into_iter().map(|one| one.file))
        .filter(|file| usable_file(file))
        .collect();
    files.sort();
    files.dedup();
    files
}

/// A folder that may hold a declaration: one plugin or mod config folder.
pub fn usable_folder(folder: &str) -> bool {
    let Some((base, name)) = folder.split_once('/') else { return false };
    matches!(base, "plugins" | "config") && !name.is_empty() && !name.contains(['/', '\\']) && name != ".." && name != "."
}

/// Turns a declaration into what the app shows; `read` returns a declared file's text (or None when missing).
pub fn build(folder: &str, json: &str, mut read: impl FnMut(&str) -> Option<String>) -> Extension {
    let fallback = folder.rsplit('/').next().unwrap_or(folder).to_string();
    let declared: Declared = match serde_json::from_str(json) {
        Ok(found) => found,
        Err(error) => {
            return Extension { folder: folder.into(), name: fallback, problem: format!("monolab.json: {error}"), ..Default::default() };
        }
    };
    let mut texts: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
    let mut text_of = |file: &str| -> Option<String> {
        if !usable_file(file) {
            return None;
        }
        texts.entry(file.to_string()).or_insert_with(|| read(file)).clone()
    };
    let settings = declared
        .settings
        .into_iter()
        .filter(|one| usable_file(&one.file) && !one.key.is_empty())
        .map(|one| {
            let kind = if KINDS.contains(&one.r#type.as_str()) { one.r#type } else { "text".to_string() };
            let value = text_of(&one.file).and_then(|text| value_at(&one.file, &text, &one.key));
            ExtSetting {
                label: if one.label.is_empty() { one.key.clone() } else { one.label },
                kind,
                options: one.options,
                min: one.min,
                max: one.max,
                hint: one.hint,
                file: one.file,
                key: one.key,
                value,
            }
        })
        .collect();
    let actions = declared
        .actions
        .into_iter()
        .filter(|one| !one.command.trim().is_empty() && !one.command.contains(['\r', '\n']))
        .map(|one| ExtAction {
            label: if one.label.is_empty() { one.command.clone() } else { one.label },
            command: one.command.trim().trim_start_matches('/').to_string(),
            call: false,
            args: one
                .args
                .into_iter()
                .filter(|arg| !arg.name.is_empty())
                .map(|arg| ExtArg {
                    label: if arg.label.is_empty() { arg.name.clone() } else { arg.label },
                    kind: if ARG_KINDS.contains(&arg.r#type.as_str()) { arg.r#type } else { "text".to_string() },
                    name: arg.name,
                })
                .collect(),
            confirm: one.confirm,
        })
        .collect();
    let status = declared
        .status
        .into_iter()
        .filter(|one| usable_file(&one.file) && !one.key.is_empty())
        .map(|one| ExtStatus {
            value: text_of(&one.file).and_then(|text| value_at(&one.file, &text, &one.key)),
            label: if one.label.is_empty() { one.key } else { one.label },
            unit: one.unit,
        })
        .collect();
    Extension {
        folder: folder.into(),
        name: if declared.name.trim().is_empty() { fallback } else { declared.name.trim().to_string() },
        settings,
        actions,
        status,
        problem: String::new(),
    }
}

fn scalar(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

pub fn value_at(file: &str, text: &str, key: &str) -> Option<String> {
    let path: Vec<&str> = key.split('.').collect();
    match format_of(file) {
        "yaml" => {
            let mut node = parse(text);
            for part in &path {
                node = node.get(part)?.clone();
            }
            match node {
                Yaml::Scalar(value) => Some(value),
                _ => None,
            }
        }
        "json" => {
            let mut node: serde_json::Value = serde_json::from_str(text).ok()?;
            for part in &path {
                node = node.get(part)?.clone();
            }
            scalar(&node)
        }
        "toml" => {
            let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
            let mut item = doc.as_item();
            for part in &path {
                item = item.get(part)?;
            }
            let value = item.as_value()?;
            match value {
                toml_edit::Value::String(text) => Some(text.value().clone()),
                other => {
                    let mut bare = other.clone();
                    bare.decor_mut().clear();
                    Some(bare.to_string())
                }
            }
        }
        "properties" => text.lines().find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| value.trim().to_string())
        }),
        _ => None,
    }
}

/// Checks a new value against the declared kind and limits; returns it normalised.
pub fn check(setting: &ExtSetting, value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.contains(['\r', '\n']) || value.len() > 500 {
        return Err("Enter a value on one line.".into());
    }
    match setting.kind.as_str() {
        "number" => {
            let number: f64 = value.parse().map_err(|_| "Enter a number.".to_string())?;
            if setting.min.is_some_and(|min| number < min) || setting.max.is_some_and(|max| number > max) {
                return Err("The number is out of range.".into());
            }
            Ok(value.to_string())
        }
        "bool" if value == "true" || value == "false" => Ok(value.to_string()),
        "bool" => Err("Choose true or false.".into()),
        "choice" if setting.options.iter().any(|one| one == value) => Ok(value.to_string()),
        "choice" => Err("Choose one of the listed values.".into()),
        _ => Ok(value.to_string()),
    }
}

fn yaml_scalar(kind: &str, value: &str) -> String {
    let plain = kind == "number" || kind == "bool" || (!value.is_empty() && value.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ' ')) && !value.starts_with(' ') && !value.ends_with(' '));
    if plain { value.to_string() } else { format!("'{}'", value.replace('\'', "''")) }
}

/// Writes a value at a dotted key, keeping the rest of the file as it was.
pub fn set_at(file: &str, text: &str, key: &str, kind: &str, value: &str) -> Result<String, String> {
    let cannot = || "The value couldn't be changed safely in this file. Edit it in the Files tab.".to_string();
    let path: Vec<&str> = key.split('.').collect();
    let changed = match format_of(file) {
        "yaml" => set_yaml(text, &path, &yaml_scalar(kind, value)).ok_or_else(cannot)?,
        "json" => {
            let leaf = path.last().copied().unwrap_or(key);
            let needle = format!("\"{leaf}\"");
            if text.matches(&needle).count() != 1 {
                return Err(cannot());
            }
            let at = text.find(&needle).ok_or_else(cannot)? + needle.len();
            let rest = &text[at..];
            let colon = rest.find(':').ok_or_else(cannot)?;
            let start = at + colon + 1 + (rest[colon + 1..].len() - rest[colon + 1..].trim_start().len());
            let tail = &text[start..];
            let end = if tail.starts_with('"') {
                let mut escaped = false;
                let mut close = None;
                for (offset, c) in tail.char_indices().skip(1) {
                    match (escaped, c) {
                        (true, _) => escaped = false,
                        (false, '\\') => escaped = true,
                        (false, '"') => {
                            close = Some(offset + 1);
                            break;
                        }
                        _ => {}
                    }
                }
                start + close.ok_or_else(cannot)?
            } else {
                start + tail.find([',', '}', '\n', '\r', ']']).unwrap_or(tail.len())
            };
            let literal = if kind == "number" || kind == "bool" { value.to_string() } else { serde_json::to_string(value).map_err(|error| error.to_string())? };
            let mut out = text.to_string();
            out.replace_range(start..end.min(text.len()), &literal);
            out
        }
        "toml" => {
            let mut doc = text.parse::<toml_edit::DocumentMut>().map_err(|_| cannot())?;
            let (leaf, parents) = path.split_last().ok_or_else(cannot)?;
            let mut table = doc.as_table_mut() as &mut dyn toml_edit::TableLike;
            for part in parents {
                table = table.get_mut(part).and_then(|item| item.as_table_like_mut()).ok_or_else(cannot)?;
            }
            let item = match kind {
                "number" if value.contains('.') => toml_edit::value(value.parse::<f64>().map_err(|_| cannot())?),
                "number" => toml_edit::value(value.parse::<i64>().map_err(|_| cannot())?),
                "bool" => toml_edit::value(value == "true"),
                _ => toml_edit::value(value),
            };
            let slot = table.get_mut(leaf).ok_or_else(cannot)?;
            let decor = slot.as_value().map(|old| old.decor().clone());
            *slot = item;
            if let (Some(decor), Some(fresh)) = (decor, slot.as_value_mut()) {
                *fresh.decor_mut() = decor;
            }
            doc.to_string()
        }
        "properties" => {
            let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
            let mut found = false;
            let lines: Vec<String> = text
                .split(newline)
                .map(|line| match line.split_once('=') {
                    Some((name, _)) if !found && name.trim() == key => {
                        found = true;
                        format!("{}={value}", name.trim_end())
                    }
                    _ => line.to_string(),
                })
                .collect();
            if !found {
                return Err(cannot());
            }
            lines.join(newline)
        }
        _ => return Err(cannot()),
    };
    if value_at(file, &changed, key).as_deref() != Some(value) {
        return Err(cannot());
    }
    Ok(changed)
}

fn set_yaml(text: &str, path: &[&str], literal: &str) -> Option<String> {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.split(newline).map(str::to_owned).collect();
    let mut depth = 0;
    let mut indent: Option<usize> = None;
    let mut parent_indent: Option<usize> = None;
    for line in lines.iter_mut() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let own = line.len() - trimmed.len();
        if let Some(parent) = parent_indent {
            if own <= parent {
                return None;
            }
        }
        if indent.is_none() {
            indent = Some(own);
        }
        if indent.is_some_and(|wanted| own != wanted) {
            continue;
        }
        let name = path[depth];
        let matches = trimmed.strip_prefix(name).is_some_and(|rest| rest.starts_with(':'))
            || trimmed.strip_prefix(&format!("'{name}'")).is_some_and(|rest| rest.starts_with(':'))
            || trimmed.strip_prefix(&format!("\"{name}\"")).is_some_and(|rest| rest.starts_with(':'));
        if !matches {
            continue;
        }
        if depth + 1 == path.len() {
            let colon = own + trimmed.find(':')?;
            let comment = trimmed.find(" #").map(|at| trimmed[at..].to_string()).unwrap_or_default();
            *line = format!("{}: {literal}{comment}", &line[..colon]);
            return Some(lines.join(newline));
        }
        parent_indent = Some(own);
        indent = None;
        depth += 1;
        continue;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECL: &str = r#"{
      "name": "Shop",
      "settings": [
        {"file": "config.yml", "key": "economy.start", "label": "Start money", "type": "number", "min": 0},
        {"file": "config.yml", "key": "economy.enabled", "label": "Economy", "type": "bool"},
        {"file": "../../server.properties", "key": "motd", "label": "Escapes"}
      ],
      "actions": [{"label": "Give", "command": "/shop give {player} {amount}", "args": [{"name": "player", "type": "player"}, {"name": "amount", "type": "number"}]}],
      "status": [{"file": "stats.json", "key": "sales.today", "label": "Sales today"}]
    }"#;

    #[test]
    fn builds_declared_controls() {
        let built = build("plugins/Shop", DECL, |file| match file {
            "config.yml" => Some("economy:\n  start: 100 # coins\n  enabled: true\nother: x\n".into()),
            "stats.json" => Some(r#"{"sales": {"today": 12}}"#.into()),
            _ => None,
        });
        assert_eq!(built.name, "Shop");
        assert_eq!(built.settings.len(), 2);
        assert_eq!(built.settings[0].value.as_deref(), Some("100"));
        assert_eq!(built.settings[1].value.as_deref(), Some("true"));
        assert_eq!(built.actions[0].command, "shop give {player} {amount}");
        assert_eq!(built.actions[0].args[1].kind, "number");
        assert_eq!(built.status[0].value.as_deref(), Some("12"));
        assert_eq!(check(&built.settings[0], "-1"), Err("The number is out of range.".into()));
    }

    #[test]
    fn reads_api_tabs_while_fresh() {
        let json = r#"{"at":1000,"owners":{"Shop":{"status":[{"key":"sales","label":"Sales","value":"12","unit":""}],"actions":[{"label":"Give","args":[{"name":"player","label":"Player","type":"player"}]}]}}}"#;
        let tabs = from_api(json, 5000);
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].folder, "api:Shop");
        assert_eq!(tabs[0].status[0].value.as_deref(), Some("12"));
        assert!(tabs[0].actions[0].call && tabs[0].actions[0].command.is_empty());
        assert_eq!(tabs[0].actions[0].args[0].kind, "player");
        assert!(from_api(json, 1000 + 200_000).is_empty());
    }

    #[test]
    fn changes_values_in_place() {
        let yaml = "# top\r\neconomy:\r\n  start: 100 # coins\r\n  name: shop\r\nstart: 5\r\n";
        assert_eq!(set_at("c.yml", yaml, "economy.start", "number", "250").unwrap(), "# top\r\neconomy:\r\n  start: 250 # coins\r\n  name: shop\r\nstart: 5\r\n");
        assert_eq!(set_at("c.yml", yaml, "economy.name", "text", "big: shop").unwrap(), "# top\r\neconomy:\r\n  start: 100 # coins\r\n  name: 'big: shop'\r\nstart: 5\r\n");
        let json = "{\n  \"a\": {\"limit\": 5, \"title\": \"Hi \\\"x\\\"\"}\n}";
        assert_eq!(set_at("c.json", json, "a.limit", "number", "7").unwrap(), "{\n  \"a\": {\"limit\": 7, \"title\": \"Hi \\\"x\\\"\"}\n}");
        assert_eq!(set_at("c.json", json, "a.title", "text", "Yo").unwrap(), "{\n  \"a\": {\"limit\": 5, \"title\": \"Yo\"}\n}");
        let toml = "# keep\n[general]\nlimit = 3 # c\n";
        assert_eq!(set_at("c.toml", toml, "general.limit", "number", "9").unwrap(), "# keep\n[general]\nlimit = 9 # c\n");
        assert_eq!(set_at("c.properties", "a=1\nb=2\n", "b", "number", "3").unwrap(), "a=1\nb=3\n");
        assert!(set_at("c.yml", yaml, "economy.missing", "number", "1").is_err());
    }
}

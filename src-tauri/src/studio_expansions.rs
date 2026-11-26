//! Studio M6 (`docs/studio-maker.md`): what other plugins add to the studio. A plugin registers action and
//! condition types, placeholders and rule events with `MonolabApi` in the add-on, and describes them for the app in
//! `plugins/<folder>/monolab-studio.json` (Pumpkin: also `plugins/data/<folder>/`), so the step editor can draw a
//! form for each:
//!
//! ```json
//! { "owner": "Shop",
//!   "actions":     [ { "type": "shop:give_money", "label": "Give money",
//!                      "fields": [ { "key": "amount", "label": "Amount", "type": "number", "default": 10 } ] } ],
//!   "conditions":  [ { "type": "shop:has_money", "label": "Has money", "fields": [] } ],
//!   "triggers":    [ { "event": "shop:buy", "label": "Player buys", "vars": ["item", "price"] } ],
//!   "placeholders":[ { "key": "shop:balance", "label": "Balance" } ] }
//! ```
//!
//! Files are read leniently: what can't be used is left out and named in `problems`. The add-on's
//! `integrations.json` says which names are registered on the running server right now.

use serde::Serialize;
use serde_json::{Map, Value};
use tauri::AppHandle;

pub const FILE: &str = "monolab-studio.json";
pub const FIELD_TYPES: [&str; 5] = ["text", "number", "bool", "choice", "player"];
/// Keys a step already uses for itself.
const RESERVED: [&str; 2] = ["type", "negate"];
const MOST_ENTRIES: usize = 50;
const MOST_FIELDS: usize = 20;
const MOST_CHOICES: usize = 50;
const MOST_TEXT: usize = 200;
/// Plugin folders looked at, like the Integrations tab's monolab.json search.
const MOST_FOLDERS: usize = 80;
/// The add-on rewrites integrations.json every 30 seconds while the server runs; older means it's stopped.
const FRESH_MS: u64 = 120_000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioExpansionField {
    pub key: String,
    pub label: String,
    /// text | number | bool | choice | player
    #[serde(rename = "type")]
    pub kind: String,
    /// The default as text ("10", "true"); empty = none.
    pub default: String,
    /// For `choice`.
    pub choices: Vec<String>,
}

/// An action or condition type a plugin adds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioExpansionStep {
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
    pub fields: Vec<StudioExpansionField>,
    /// The running add-on says the plugin registered it (only meaningful when the expansion is `live`).
    pub registered: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioExpansionTrigger {
    pub event: String,
    pub label: String,
    /// Placeholders the event fills in its rules (`<item>`).
    pub vars: Vec<String>,
    /// The plugin has fired it since the server started.
    pub registered: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioExpansionPlaceholder {
    /// `shop:balance`, written `<ext:shop:balance>` in studio text.
    pub key: String,
    pub label: String,
    pub registered: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioExpansion {
    /// Where the file is, from the server folder (`plugins/Shop`).
    pub folder: String,
    pub owner: String,
    pub actions: Vec<StudioExpansionStep>,
    pub conditions: Vec<StudioExpansionStep>,
    pub triggers: Vec<StudioExpansionTrigger>,
    pub placeholders: Vec<StudioExpansionPlaceholder>,
    pub problems: Vec<String>,
    /// The running add-on reported what is registered (Paper, server on); `registered` flags only mean something then.
    pub live: bool,
}

/// "plugin:name": lower-case a-z, 0-9, `_` and `-`, one colon (the add-on's MonolabApi rule).
pub fn studio_name(name: &str) -> bool {
    let good = |part: &str, most: usize| !part.is_empty() && part.len() <= most && part.bytes().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == b'_' || one == b'-');
    match name.split_once(':') {
        Some((plugin, rest)) => good(plugin, 32) && good(rest, 64),
        None => false,
    }
}

fn cut(text: &str) -> String {
    let clean: String = text.chars().map(|one| if one.is_control() { ' ' } else { one }).collect();
    let clean = clean.trim();
    match clean.char_indices().nth(MOST_TEXT) {
        Some((at, _)) => clean[..at].to_string(),
        None => clean.to_string(),
    }
}

fn text(data: &Map<String, Value>, key: &str) -> String {
    data.get(key).and_then(Value::as_str).map(cut).unwrap_or_default()
}

fn scalar(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(one)) => cut(one),
        Some(Value::Number(one)) => one.to_string(),
        Some(Value::Bool(one)) => one.to_string(),
        _ => String::new(),
    }
}

/// The objects in a list (anything else is a problem).
fn objects<'a>(root: &'a Map<String, Value>, key: &str, problems: &mut Vec<String>) -> Vec<&'a Map<String, Value>> {
    match root.get(key) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(list)) => {
            if list.len() > MOST_ENTRIES {
                problems.push(format!("\"{key}\": only the first {MOST_ENTRIES} are used."));
            }
            let mut out = Vec::new();
            for one in list.iter().take(MOST_ENTRIES) {
                match one.as_object() {
                    Some(map) => out.push(map),
                    None => problems.push(format!("\"{key}\": every entry must be an object.")),
                }
            }
            out
        }
        Some(_) => {
            problems.push(format!("\"{key}\" must be a list."));
            Vec::new()
        }
    }
}

fn fields(step: &str, data: &Map<String, Value>, problems: &mut Vec<String>) -> Vec<StudioExpansionField> {
    let mut out: Vec<StudioExpansionField> = Vec::new();
    let mut local = Vec::new();
    for one in objects(data, "fields", &mut local) {
        let key = text(one, "key");
        if key.is_empty() || key.len() > 64 || !key.bytes().all(|one| one.is_ascii_alphanumeric() || one == b'_' || one == b'-') {
            problems.push(format!("{step}: a field needs a \"key\" of letters, digits, _ and -."));
            continue;
        }
        if RESERVED.contains(&key.as_str()) {
            problems.push(format!("{step}: the field key \"{key}\" is taken by the studio."));
            continue;
        }
        if out.iter().any(|field| field.key == key) {
            problems.push(format!("{step}: the field \"{key}\" is there twice."));
            continue;
        }
        if out.len() >= MOST_FIELDS {
            problems.push(format!("{step}: only the first {MOST_FIELDS} fields are used."));
            break;
        }
        let mut kind = text(one, "type");
        if kind.is_empty() {
            kind = "text".into();
        } else if !FIELD_TYPES.contains(&kind.as_str()) {
            problems.push(format!("{step}: the field \"{key}\" has type \"{kind}\"; use {}. It is shown as text.", FIELD_TYPES.join(", ")));
            kind = "text".into();
        }
        let choices: Vec<String> = match one.get("choices") {
            Some(Value::Array(list)) => list.iter().map(|one| scalar(Some(one))).filter(|one| !one.is_empty()).take(MOST_CHOICES).collect(),
            _ => Vec::new(),
        };
        if kind == "choice" && choices.is_empty() {
            problems.push(format!("{step}: the field \"{key}\" is a choice with no \"choices\". It is shown as text."));
            kind = "text".into();
        }
        let label = text(one, "label");
        out.push(StudioExpansionField { label: if label.is_empty() { key.clone() } else { label }, key, kind, default: scalar(one.get("default")), choices });
    }
    problems.extend(local.into_iter().map(|problem| format!("{step}: {problem}")));
    out
}

fn steps(root: &Map<String, Value>, key: &str, problems: &mut Vec<String>) -> Vec<StudioExpansionStep> {
    let mut out: Vec<StudioExpansionStep> = Vec::new();
    for one in objects(root, key, problems) {
        let kind = text(one, "type");
        if !studio_name(&kind) {
            problems.push(format!("\"{key}\": \"{kind}\" isn't a plugin:name type (a-z, 0-9, _ and -, one colon)."));
            continue;
        }
        if out.iter().any(|step| step.kind == kind) {
            problems.push(format!("\"{key}\": {kind} is there twice."));
            continue;
        }
        let label = text(one, "label");
        let fields = fields(&kind, one, problems);
        out.push(StudioExpansionStep { label: if label.is_empty() { kind.clone() } else { label }, kind, fields, registered: false });
    }
    out
}

/// One plugin's `monolab-studio.json`. `folder` is where it was found (`plugins/Shop`).
pub fn parse(folder: &str, json: &str) -> StudioExpansion {
    let fallback = folder.rsplit('/').next().unwrap_or(folder).to_string();
    let mut out = StudioExpansion { folder: folder.to_string(), owner: fallback, ..Default::default() };
    let root = match serde_json::from_str::<Value>(json) {
        Ok(Value::Object(root)) => root,
        Ok(_) => {
            out.problems.push(format!("{FILE} must be a JSON object."));
            return out;
        }
        Err(error) => {
            out.problems.push(format!("{FILE} isn't valid JSON: {error}"));
            return out;
        }
    };
    let owner = text(&root, "owner");
    if !owner.is_empty() {
        out.owner = owner;
    }
    let mut problems = Vec::new();
    out.actions = steps(&root, "actions", &mut problems);
    out.conditions = steps(&root, "conditions", &mut problems);
    for one in objects(&root, "triggers", &mut problems) {
        let event = text(one, "event");
        if !studio_name(&event) {
            problems.push(format!("\"triggers\": \"{event}\" isn't a plugin:name event (a-z, 0-9, _ and -, one colon)."));
            continue;
        }
        if out.triggers.iter().any(|trigger| trigger.event == event) {
            problems.push(format!("\"triggers\": {event} is there twice."));
            continue;
        }
        let vars: Vec<String> = match one.get("vars") {
            Some(Value::Array(list)) => list
                .iter()
                .filter_map(Value::as_str)
                .map(cut)
                .filter(|var| !var.is_empty() && var.len() <= 64 && var.bytes().all(|one| one.is_ascii_alphanumeric() || matches!(one, b'_' | b'.' | b'-')))
                .take(MOST_FIELDS)
                .collect(),
            _ => Vec::new(),
        };
        let label = text(one, "label");
        out.triggers.push(StudioExpansionTrigger { label: if label.is_empty() { event.clone() } else { label }, event, vars, registered: false });
    }
    for one in objects(&root, "placeholders", &mut problems) {
        let key = text(one, "key");
        if !studio_name(&key) {
            problems.push(format!("\"placeholders\": \"{key}\" isn't a plugin:name key (a-z, 0-9, _ and -, one colon)."));
            continue;
        }
        if out.placeholders.iter().any(|placeholder| placeholder.key == key) {
            problems.push(format!("\"placeholders\": {key} is there twice."));
            continue;
        }
        let label = text(one, "label");
        out.placeholders.push(StudioExpansionPlaceholder { label: if label.is_empty() { key.clone() } else { label }, key, registered: false });
    }
    problems.dedup();
    out.problems = problems;
    out
}

/// What the running add-on lists under `studio` in integrations.json, or `None` when the file is old.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Registered {
    pub actions: Vec<String>,
    pub conditions: Vec<String>,
    pub placeholders: Vec<String>,
    pub triggers: Vec<String>,
}

pub fn registered(integrations_json: &str, now_ms: u64) -> Option<Registered> {
    let root: Value = serde_json::from_str(integrations_json).ok()?;
    let at = root.get("at").and_then(Value::as_u64).unwrap_or(0);
    if now_ms.saturating_sub(at) > FRESH_MS {
        return None;
    }
    let studio = root.get("studio");
    let list = |key: &str| -> Vec<String> {
        studio.and_then(|one| one.get(key)).and_then(Value::as_array).map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
    };
    Some(Registered { actions: list("actions"), conditions: list("conditions"), placeholders: list("placeholders"), triggers: list("triggers") })
}

/// Marks what is registered, and names that two plugins both describe.
pub fn finish(mut all: Vec<StudioExpansion>, live: Option<&Registered>) -> Vec<StudioExpansion> {
    all.sort_by(|a, b| a.owner.to_lowercase().cmp(&b.owner.to_lowercase()).then_with(|| a.folder.cmp(&b.folder)));
    let mut seen: Vec<(String, String)> = Vec::new();
    for one in &mut all {
        one.live = live.is_some();
        if let Some(live) = live {
            for step in &mut one.actions {
                step.registered = live.actions.contains(&step.kind);
            }
            for step in &mut one.conditions {
                step.registered = live.conditions.contains(&step.kind);
            }
            for trigger in &mut one.triggers {
                trigger.registered = live.triggers.contains(&trigger.event);
            }
            for placeholder in &mut one.placeholders {
                placeholder.registered = live.placeholders.contains(&placeholder.key);
            }
        }
        let names: Vec<String> = one
            .actions
            .iter()
            .chain(one.conditions.iter())
            .map(|step| step.kind.clone())
            .chain(one.triggers.iter().map(|trigger| trigger.event.clone()))
            .chain(one.placeholders.iter().map(|placeholder| placeholder.key.clone()))
            .collect();
        for name in names {
            if let Some((_, other)) = seen.iter().find(|(seen_name, _)| *seen_name == name) {
                one.problems.push(format!("{name} is also described by {other}."));
            } else {
                seen.push((name, one.owner.clone()));
            }
        }
    }
    all
}

/// Plugin folders that may hold a `monolab-studio.json`.
async fn folders(app: &AppHandle, instance: &crate::ServerInstance) -> Vec<String> {
    let mut bases = vec!["plugins"];
    if crate::pumpkin::is_pumpkin(&instance.core) {
        bases.push("plugins/data");
    }
    let mut out = Vec::new();
    for base in bases {
        let mut names: Vec<String> = crate::server_names(app, instance, base)
            .await
            .into_iter()
            .filter(|(name, dir, _)| *dir && !name.eq_ignore_ascii_case("monolab") && !name.starts_with('.') && !(base == "plugins" && name == "data" && crate::pumpkin::is_pumpkin(&instance.core)))
            .map(|(name, _, _)| name)
            .collect();
        names.sort();
        out.extend(names.into_iter().take(MOST_FOLDERS).map(|name| format!("{base}/{name}")));
    }
    out
}

/// The studio additions other plugins on this server describe (local, SSH and panel servers alike).
#[tauri::command(async)]
#[specta::specta]
pub async fn studio_expansions(app: AppHandle, id: String) -> Result<Vec<StudioExpansion>, String> {
    let instance = crate::instance_by_id(&app, &id)?;
    if crate::studio_root(&instance).is_none() {
        return Err("The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.".into());
    }
    let mut all = Vec::new();
    for folder in folders(&app, &instance).await {
        if let Ok(Some(json)) = crate::server_text(&app, &instance, &format!("{folder}/{FILE}")).await {
            all.push(parse(&folder, &json));
        }
    }
    let live = match crate::addon_config_path(&instance).filter(|_| !crate::pumpkin::is_pumpkin(&instance.core)) {
        Some(config) => {
            let path = config.replace("config.yml", "integrations.json");
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|time| time.as_millis() as u64).unwrap_or(0);
            crate::server_text(&app, &instance, &path).await.ok().flatten().and_then(|text| registered(&text, now))
        }
        None => None,
    };
    Ok(finish(all, live.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_plugin_colon_name() {
        assert!(studio_name("shop:give_money"));
        assert!(studio_name("my-shop:a1"));
        assert!(!studio_name("give_money"));
        assert!(!studio_name("Shop:give"));
        assert!(!studio_name("shop:give:more"));
        assert!(!studio_name("shop:"));
        assert!(!studio_name(":x"));
    }

    #[test]
    fn reads_a_file_leniently() {
        let json = r#"{ "owner": "Shop",
            "actions": [
                { "type": "shop:give_money", "label": "Give money", "fields": [
                    { "key": "amount", "label": "Amount", "type": "number", "default": 10 },
                    { "key": "who", "type": "player" },
                    { "key": "mode", "type": "choice", "choices": ["add", "set"], "default": "add" },
                    { "key": "loud", "type": "bool", "default": true },
                    { "key": "type" },
                    { "key": "odd", "type": "colour" },
                    { "key": "pick", "type": "choice" }
                ] },
                { "type": "give_money" },
                { "type": "shop:give_money" },
                "nope"
            ],
            "conditions": [ { "type": "shop:has_money" } ],
            "triggers": [ { "event": "shop:buy", "label": "Player buys", "vars": ["item", "price", "bad var"] }, { "event": "buy" } ],
            "placeholders": [ { "key": "shop:balance", "label": "Balance" }, { "key": "balance" } ] }"#;
        let one = parse("plugins/Shop", json);
        assert_eq!(one.owner, "Shop");
        assert_eq!(one.actions.len(), 1);
        let fields = &one.actions[0].fields;
        assert_eq!(fields.iter().map(|field| field.key.as_str()).collect::<Vec<_>>(), ["amount", "who", "mode", "loud", "odd", "pick"]);
        assert_eq!(fields[0].default, "10");
        assert_eq!(fields[0].kind, "number");
        assert_eq!(fields[1].label, "who");
        assert_eq!(fields[2].choices, ["add", "set"]);
        assert_eq!(fields[3].default, "true");
        assert_eq!(fields[4].kind, "text");
        assert_eq!(fields[5].kind, "text");
        assert_eq!(one.conditions[0].label, "shop:has_money");
        assert_eq!(one.triggers.len(), 1);
        assert_eq!(one.triggers[0].vars, ["item", "price"]);
        assert_eq!(one.placeholders.len(), 1);
        // reserved key, unknown type, choice without choices, bad action type, twice, not an object, bad event, bad key
        assert_eq!(one.problems.len(), 8, "{:?}", one.problems);
    }

    #[test]
    fn broken_files_say_so() {
        let one = parse("plugins/Shop", "{");
        assert_eq!(one.owner, "Shop");
        assert!(one.problems[0].contains("isn't valid JSON"));
        assert!(parse("plugins/Shop", "[]").problems[0].contains("JSON object"));
        assert!(parse("plugins/Shop", r#"{"actions": 3}"#).problems[0].contains("must be a list"));
    }

    #[test]
    fn marks_what_the_addon_registered() {
        let one = parse("plugins/Shop", r#"{"actions":[{"type":"shop:a"},{"type":"shop:b"}],"triggers":[{"event":"shop:buy"}]}"#);
        let two = parse("plugins/Bank", r#"{"actions":[{"type":"shop:a"}]}"#);
        let json = r#"{"at": 1000, "owners": {}, "studio": {"actions": ["shop:a"], "triggers": ["shop:buy"]}}"#;
        let live = registered(json, 2000).unwrap();
        assert!(registered(json, 1000 + FRESH_MS + 1).is_none());
        let all = finish(vec![one, two], Some(&live));
        assert_eq!(all[0].owner, "Bank");
        assert!(all[0].live && all[0].actions[0].registered);
        assert!(all[1].problems.iter().any(|problem| problem.contains("also described by Bank")));
        assert!(all[1].actions[0].registered && !all[1].actions[1].registered);
        assert!(all[1].triggers[0].registered);
        let off = finish(vec![parse("plugins/Shop", r#"{"actions":[{"type":"shop:a"}]}"#)], None);
        assert!(!off[0].live && !off[0].actions[0].registered);
    }
}

//! Read-side of plugin integrations (EssentialsX files, LuckPerms exports). Changes go through console commands.

/// Just enough YAML for plugin data files: nested maps, scalars and `- item` lists.
#[derive(Debug, Clone, PartialEq)]
pub enum Yaml {
    Map(Vec<(String, Yaml)>),
    List(Vec<String>),
    Scalar(String),
}

impl Yaml {
    pub fn get(&self, key: &str) -> Option<&Yaml> {
        match self {
            Yaml::Map(items) => items.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn text(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Yaml::Scalar(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        self.text(key)?.parse().ok()
    }

    pub fn entries(&self) -> &[(String, Yaml)] {
        match self {
            Yaml::Map(items) => items,
            _ => &[],
        }
    }
}

fn unquote(raw: &str) -> String {
    let raw = raw.trim();
    if raw.len() >= 2 && ((raw.starts_with('\'') && raw.ends_with('\'')) || (raw.starts_with('"') && raw.ends_with('"'))) {
        return raw[1..raw.len() - 1].replace("''", "'");
    }
    raw.to_string()
}

fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    for (at, c) in line.char_indices() {
        match (quote, c) {
            (None, '\'' | '"') => quote = Some(c),
            (Some(open), _) if c == open => quote = None,
            (None, '#') if at == 0 || line[..at].ends_with(' ') => return &line[..at],
            _ => {}
        }
    }
    line
}

pub fn parse(text: &str) -> Yaml {
    let lines: Vec<(usize, String)> = text
        .lines()
        .map(strip_comment)
        .filter(|line| !line.trim().is_empty() && line.trim() != "---")
        .map(|line| (line.len() - line.trim_start().len(), line.trim().to_string()))
        .collect();
    let mut at = 0;
    block(&lines, &mut at, 0)
}

fn block(lines: &[(usize, String)], at: &mut usize, indent: usize) -> Yaml {
    if lines.get(*at).is_some_and(|line| line.1.starts_with("- ") || line.1 == "-") {
        let mut items = Vec::new();
        let own = lines[*at].0;
        while *at < lines.len() && lines[*at].0 == own && lines[*at].1.starts_with('-') {
            items.push(unquote(lines[*at].1.trim_start_matches('-')));
            *at += 1;
            while *at < lines.len() && lines[*at].0 > own {
                *at += 1;
            }
        }
        return Yaml::List(items);
    }
    let mut items = Vec::new();
    while *at < lines.len() {
        let (own, line) = &lines[*at];
        if *own < indent {
            break;
        }
        let Some((key, rest)) = split_key(line) else {
            *at += 1;
            continue;
        };
        *at += 1;
        let rest = rest.trim();
        if rest.is_empty() {
            let deeper = lines.get(*at).map(|next| next.0).filter(|next| *next > *own || lines[*at].1.starts_with('-'));
            match deeper {
                Some(next) => items.push((key, block(lines, at, next))),
                None => items.push((key, Yaml::Scalar(String::new()))),
            }
        } else if rest.starts_with('[') && rest.ends_with(']') {
            let inner = &rest[1..rest.len() - 1];
            let list = inner.split(',').map(unquote).filter(|one| !one.is_empty()).collect();
            items.push((key, Yaml::List(list)));
        } else if rest == "{}" {
            items.push((key, Yaml::Map(Vec::new())));
        } else {
            items.push((key, Yaml::Scalar(unquote(rest))));
        }
    }
    Yaml::Map(items)
}

fn split_key(line: &str) -> Option<(String, &str)> {
    if let Some(quoted) = line.strip_prefix('\'').or_else(|| line.strip_prefix('"')) {
        let close = quoted.find(['\'', '"'])?;
        let rest = quoted[close + 1..].strip_prefix(':')?;
        return Some((quoted[..close].to_string(), rest));
    }
    let colon = line.find(": ").or_else(|| line.strip_suffix(':').map(|head| head.len()))?;
    Some((line[..colon].trim().to_string(), line.get(colon + 1..).unwrap_or("")))
}

/// Replaces the value of a top-level `key: value` line, keeping everything else (comments, order) as it was.
pub fn set_top_level(text: &str, key: &str, value: &str) -> Option<String> {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut found = false;
    let lines: Vec<String> = text
        .split(newline)
        .map(|line| {
            if !found && line.starts_with(&format!("{key}:")) {
                found = true;
                format!("{key}: {value}")
            } else {
                line.to_string()
            }
        })
        .collect();
    found.then(|| lines.join(newline))
}

#[derive(serde::Serialize, specta::Type, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Integrations {
    pub essentials: bool,
    /// LuckPerms data folder, empty when it isn't installed.
    pub luckperms: String,
    /// Plugin or mod folders that hold a monolab.json.
    pub extensions: Vec<String>,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Spot {
    pub name: String,
    pub world: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EssKit {
    pub name: String,
    pub delay: String,
    pub items: u32,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EssUser {
    pub uuid: String,
    pub name: String,
    pub money: String,
    pub homes: Vec<Spot>,
    pub last_seen: f64,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EssSetting {
    pub key: String,
    pub value: String,
}

#[derive(serde::Serialize, specta::Type, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EssentialsData {
    pub warps: Vec<Spot>,
    pub kits: Vec<EssKit>,
    pub users: Vec<EssUser>,
    pub users_total: u32,
    pub settings: Vec<EssSetting>,
}

/// Top-level config.yml keys the app shows as fields.
pub const ESS_SETTINGS: &[&str] = &[
    "teleport-cooldown",
    "teleport-delay",
    "heal-cooldown",
    "starting-balance",
    "currency-symbol",
    "min-money",
    "max-money",
    "auto-afk",
    "auto-afk-kick",
    "change-displayname",
];

fn spot(name: &str, node: &Yaml) -> Option<Spot> {
    Some(Spot {
        name: name.to_string(),
        world: node.text("world-name").or_else(|| node.text("world")).unwrap_or_default(),
        x: node.number("x")?,
        y: node.number("y")?,
        z: node.number("z")?,
    })
}

pub fn warp(file_stem: &str, text: &str) -> Option<Spot> {
    let node = parse(text);
    let name = node.text("name").unwrap_or_else(|| file_stem.to_string());
    spot(&name, &node)
}

pub fn user(uuid: &str, text: &str) -> EssUser {
    let node = parse(text);
    let homes = node
        .get("homes")
        .map(|homes| homes.entries().iter().filter_map(|(name, one)| spot(name, one)).collect())
        .unwrap_or_default();
    let last_seen = node
        .get("timestamps")
        .and_then(|times| times.number("logout").or_else(|| times.number("login")))
        .unwrap_or(0.0);
    EssUser {
        uuid: uuid.to_string(),
        name: node
            .text("last-account-name")
            .or_else(|| node.text("lastAccountName"))
            .unwrap_or_else(|| uuid.to_string()),
        money: node.text("money").unwrap_or_default(),
        homes,
        last_seen,
    }
}

pub fn kits(text: &str) -> Vec<EssKit> {
    let node = parse(text);
    node.get("kits")
        .map(|kits| {
            kits.entries()
                .iter()
                .map(|(name, kit)| EssKit {
                    name: name.clone(),
                    delay: kit.text("delay").unwrap_or_default(),
                    items: match kit.get("items") {
                        Some(Yaml::List(items)) => items.len() as u32,
                        _ => 0,
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn settings(config: &str) -> Vec<EssSetting> {
    let node = parse(config);
    ESS_SETTINGS
        .iter()
        .filter_map(|key| node.text(key).map(|value| EssSetting { key: key.to_string(), value }))
        .collect()
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LpPermission {
    pub node: String,
    pub value: bool,
    pub context: String,
    pub temporary: bool,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LpGroup {
    pub name: String,
    pub weight: Option<i32>,
    pub prefix: String,
    pub parents: Vec<String>,
    pub permissions: Vec<LpPermission>,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LpUser {
    pub uuid: String,
    pub name: String,
    pub primary: String,
    pub groups: Vec<String>,
    pub permissions: u32,
}

#[derive(serde::Serialize, specta::Type, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LpTrack {
    pub name: String,
    pub groups: Vec<String>,
}

#[derive(serde::Serialize, specta::Type, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LuckPermsData {
    /// Console command root: `lp`, or `lpv` on Velocity.
    pub command: String,
    pub groups: Vec<LpGroup>,
    pub users: Vec<LpUser>,
    pub tracks: Vec<LpTrack>,
}

fn context_text(value: Option<&serde_json::Value>) -> String {
    let mut pairs: Vec<String> = Vec::new();
    match value {
        Some(serde_json::Value::Object(map)) => {
            for (key, value) in map {
                match value {
                    serde_json::Value::Array(all) => pairs.extend(all.iter().filter_map(|one| one.as_str()).map(|one| format!("{key}={one}"))),
                    other => pairs.push(format!("{key}={}", other.as_str().unwrap_or(&other.to_string()))),
                }
            }
        }
        Some(serde_json::Value::Array(all)) => {
            for one in all {
                if let (Some(key), Some(value)) = (one.get("key").and_then(|v| v.as_str()), one.get("value").and_then(|v| v.as_str())) {
                    pairs.push(format!("{key}={value}"));
                }
            }
        }
        _ => {}
    }
    pairs.sort();
    pairs.join(", ")
}

struct Nodes {
    parents: Vec<String>,
    weight: Option<i32>,
    prefix: (i32, String),
    permissions: Vec<LpPermission>,
}

fn nodes(list: Option<&serde_json::Value>) -> Nodes {
    let mut out = Nodes { parents: Vec::new(), weight: None, prefix: (i32::MIN, String::new()), permissions: Vec::new() };
    for node in list.and_then(|v| v.as_array()).into_iter().flatten() {
        let Some(key) = node.get("key").and_then(|v| v.as_str()) else { continue };
        let value = node.get("value").and_then(|v| v.as_bool()).unwrap_or(true);
        let context = context_text(node.get("context"));
        if let Some(group) = key.strip_prefix("group.") {
            if value && context.is_empty() {
                out.parents.push(group.to_string());
            }
        } else if let Some(weight) = key.strip_prefix("weight.") {
            out.weight = weight.parse().ok();
        } else if let Some(rest) = key.strip_prefix("prefix.") {
            if let Some((priority, text)) = rest.split_once('.') {
                let priority = priority.parse().unwrap_or(0);
                if priority > out.prefix.0 {
                    out.prefix = (priority, text.to_string());
                }
            }
        } else if !["suffix.", "meta.", "displayname."].iter().any(|skip| key.starts_with(skip)) {
            out.permissions.push(LpPermission {
                node: key.to_string(),
                value,
                context,
                temporary: node.get("expiry").is_some_and(|v| !v.is_null()),
            });
        }
    }
    out.permissions.sort_by(|left, right| left.node.cmp(&right.node));
    out
}

/// Reads the JSON written by `lp export <name>` (LuckPerms 5).
pub fn luckperms(json: &str, command: &str) -> Result<LuckPermsData, String> {
    let root: serde_json::Value = serde_json::from_str(json).map_err(|error| format!("Could not read the LuckPerms export: {error}"))?;
    let mut groups: Vec<LpGroup> = root
        .get("groups")
        .and_then(|v| v.as_object())
        .map(|all| {
            all.iter()
                .map(|(name, group)| {
                    let read = nodes(group.get("nodes"));
                    LpGroup { name: name.clone(), weight: read.weight, prefix: read.prefix.1, parents: read.parents, permissions: read.permissions }
                })
                .collect()
        })
        .unwrap_or_default();
    groups.sort_by(|left, right| right.weight.unwrap_or(0).cmp(&left.weight.unwrap_or(0)).then(left.name.cmp(&right.name)));
    let mut users: Vec<LpUser> = root
        .get("users")
        .and_then(|v| v.as_object())
        .map(|all| {
            all.iter()
                .map(|(uuid, user)| {
                    let read = nodes(user.get("nodes"));
                    LpUser {
                        uuid: uuid.clone(),
                        name: user.get("username").and_then(|v| v.as_str()).unwrap_or(uuid).to_string(),
                        primary: user.get("primaryGroup").and_then(|v| v.as_str()).unwrap_or("default").to_string(),
                        groups: read.parents,
                        permissions: read.permissions.len() as u32,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    users.sort_by_key(|one| one.name.to_lowercase());
    let tracks = root
        .get("tracks")
        .and_then(|v| v.as_object())
        .map(|all| {
            all.iter()
                .map(|(name, track)| LpTrack {
                    name: name.clone(),
                    groups: track
                        .get("groups")
                        .and_then(|v| v.as_array())
                        .map(|list| list.iter().filter_map(|one| one.as_str().map(str::to_owned)).collect())
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(LuckPermsData { command: command.to_string(), groups, users, tracks })
}

/// Users ordered newest first, so the list can stop early on servers with many players.
pub fn newest_first(mut files: Vec<(String, u64)>) -> Vec<String> {
    files.sort_by_key(|one| std::cmp::Reverse(one.1));
    files.into_iter().map(|(name, _)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_essentials_user_with_homes() {
        let text = "timestamps:\n  login: 1700000000000\n  logout: 1700000500000\nmoney: '125.5'\nhomes:\n  base:\n    world: 1b2c\n    world-name: world\n    x: 10.5\n    y: 64.0\n    z: -3.25\n    yaw: 0.0\n  nether:\n    world: world_nether\n    x: 1\n    y: 2\n    z: 3\nlast-account-name: Steve\n";
        let read = user("abc", text);
        assert_eq!(read.name, "Steve");
        assert_eq!(read.money, "125.5");
        assert_eq!(read.last_seen, 1700000500000.0);
        assert_eq!(read.homes.len(), 2);
        assert_eq!(read.homes[0], Spot { name: "base".into(), world: "world".into(), x: 10.5, y: 64.0, z: -3.25 });
        assert_eq!(read.homes[1].world, "world_nether");
    }

    #[test]
    fn reads_kits_and_settings() {
        let kits_text = "# comment\nkits:\n  tools:\n    delay: 10\n    items:\n    - stone_pickaxe 1\n    - stone_shovel 1\n  food:\n    delay: 0\n    items: []\n";
        let read = kits(kits_text);
        assert_eq!(read, vec![
            EssKit { name: "tools".into(), delay: "10".into(), items: 2 },
            EssKit { name: "food".into(), delay: "0".into(), items: 0 },
        ]);
        let config = "teleport-cooldown: 0 # seconds\nteleport-delay: 3\ncurrency-symbol: '$'\nnested:\n  teleport-delay: 9\n";
        assert_eq!(settings(config), vec![
            EssSetting { key: "teleport-cooldown".into(), value: "0".into() },
            EssSetting { key: "teleport-delay".into(), value: "3".into() },
            EssSetting { key: "currency-symbol".into(), value: "$".into() },
        ]);
        let changed = set_top_level("a: 1\r\nteleport-delay: 3\r\nnested:\r\n  teleport-delay: 9\r\n", "teleport-delay", "5").unwrap();
        assert_eq!(changed, "a: 1\r\nteleport-delay: 5\r\nnested:\r\n  teleport-delay: 9\r\n");
    }

    #[test]
    fn reads_luckperms_export() {
        let json = r#"{"metadata":{},"groups":{"default":{"nodes":[{"type":"permission","key":"essentials.home","value":true}]},"admin":{"nodes":[{"key":"weight.100","value":true},{"key":"prefix.100.&c[Admin] ","value":true},{"key":"group.default","value":true},{"key":"*","value":true},{"key":"essentials.fly","value":false,"context":{"world":["world_nether"]},"expiry":1999999999}]}},"tracks":{"staff":{"groups":["default","admin"]}},"users":{"u1":{"username":"steve","primaryGroup":"admin","nodes":[{"key":"group.admin","value":true}]}}}"#;
        let read = luckperms(json, "lp").unwrap();
        assert_eq!(read.groups[0].name, "admin");
        assert_eq!(read.groups[0].weight, Some(100));
        assert_eq!(read.groups[0].prefix, "&c[Admin] ");
        assert_eq!(read.groups[0].parents, vec!["default"]);
        assert_eq!(read.groups[0].permissions[1], LpPermission { node: "essentials.fly".into(), value: false, context: "world=world_nether".into(), temporary: true });
        assert_eq!(read.users[0].groups, vec!["admin"]);
        assert_eq!(read.tracks[0].groups, vec!["default", "admin"]);
    }
}

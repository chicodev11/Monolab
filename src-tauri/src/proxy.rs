//! Velocity networks: the proxy's server list in velocity.toml and modern forwarding on the servers behind it.

use toml_edit::{value, Array, DocumentMut, Item, Table};

pub struct VelocityConfig {
    pub modern: bool,
    pub servers: Vec<(String, String)>,
    pub first: Vec<String>,
    pub secret_file: String,
}

pub fn read_velocity(text: &str) -> Result<VelocityConfig, String> {
    let doc: DocumentMut = text.parse().map_err(|error| format!("velocity.toml could not be read: {error}"))?;
    let modern = doc.get("player-info-forwarding-mode").and_then(Item::as_str).is_some_and(|mode| mode.eq_ignore_ascii_case("modern"));
    let mut servers = Vec::new();
    let mut first = Vec::new();
    if let Some(table) = doc.get("servers").and_then(Item::as_table_like) {
        for (key, item) in table.iter() {
            if key == "try" {
                if let Some(list) = item.as_array() {
                    first = list.iter().filter_map(|one| one.as_str().map(str::to_string)).collect();
                }
            } else if let Some(address) = item.as_str() {
                servers.push((key.to_string(), address.to_string()));
            }
        }
    }
    let secret_file = doc.get("forwarding-secret-file").and_then(Item::as_str).unwrap_or("forwarding.secret").to_string();
    Ok(VelocityConfig { modern, servers, first, secret_file })
}

/// Turns on modern forwarding and replaces the server list; forced hosts naming the old servers are cleared.
pub fn write_velocity(text: &str, servers: &[(String, String)], first: &str) -> Result<String, String> {
    let mut doc: DocumentMut = text.parse().map_err(|error| format!("velocity.toml could not be read: {error}"))?;
    doc["player-info-forwarding-mode"] = value("modern");
    let mut table = Table::new();
    for (name, address) in servers {
        table.insert(name, value(address.as_str()));
    }
    let mut order = Array::new();
    if !first.is_empty() {
        order.push(first);
    }
    for (name, _) in servers {
        if name != first {
            order.push(name.as_str());
        }
    }
    table.insert("try", value(order));
    doc.insert("servers", Item::Table(table));
    doc.insert("forced-hosts", Item::Table(Table::new()));
    Ok(doc.to_string())
}

/// A name Velocity accepts as a server key, unique among `taken`.
pub fn server_key(name: &str, taken: &[String]) -> String {
    let mut base: String = name
        .to_lowercase()
        .chars()
        .map(|one| if one.is_ascii_alphanumeric() || one == '-' || one == '_' { one } else { '-' })
        .collect();
    base = base.trim_matches('-').to_string();
    if base.is_empty() {
        base = "server".into();
    }
    let mut key = base.clone();
    let mut at = 2;
    while taken.contains(&key) {
        key = format!("{base}-{at}");
        at += 1;
    }
    key
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn meaningful(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && !trimmed.starts_with('#')
}

/// The lines after `start` that belong to the block opened there (deeper indent, or blank/comment lines).
fn block_end(lines: &[String], start: usize) -> usize {
    let base = indent(&lines[start]);
    let mut end = start + 1;
    while end < lines.len() && (!meaningful(&lines[end]) || indent(&lines[end]) > base) {
        end += 1;
    }
    while end > start + 1 && !meaningful(&lines[end - 1]) {
        end -= 1;
    }
    end
}

fn find_key(lines: &[String], from: usize, to: usize, depth: usize, key: &str) -> Option<usize> {
    (from..to).find(|&at| indent(&lines[at]) == depth && lines[at].trim_start().strip_prefix(key).is_some_and(|rest| rest.starts_with(':')))
}

/// paper-global.yml with `proxies.velocity` set to on or off, keeping everything else as written.
pub fn paper_velocity(yaml: &str, on: bool, secret: &str) -> String {
    let eol = if yaml.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = yaml.lines().map(str::to_string).collect();
    let wanted = [
        ("enabled", on.to_string()),
        ("online-mode", "true".to_string()),
        ("secret", format!("'{}'", secret.replace('\'', "''"))),
    ];
    let proxies = match find_key(&lines, 0, lines.len(), 0, "proxies") {
        Some(at) => at,
        None => {
            lines.push("proxies:".into());
            lines.len() - 1
        }
    };
    let end = block_end(&lines, proxies);
    let velocity = match find_key(&lines, proxies + 1, end, 2, "velocity") {
        Some(at) => at,
        None => {
            lines.insert(end, "  velocity:".into());
            end
        }
    };
    for (key, wanted) in wanted {
        let end = block_end(&lines, velocity);
        let row = format!("    {key}: {wanted}");
        match find_key(&lines, velocity + 1, end, 4, key) {
            Some(at) => lines[at] = row,
            None => lines.insert(end, row),
        }
    }
    let mut out = lines.join(eol);
    out.push_str(eol);
    out
}

/// Whether paper-global.yml already forwards from a Velocity proxy with this secret.
pub fn paper_velocity_on(yaml: &str, secret: &str) -> bool {
    let lines: Vec<String> = yaml.lines().map(str::to_string).collect();
    let Some(proxies) = find_key(&lines, 0, lines.len(), 0, "proxies") else { return false };
    let Some(velocity) = find_key(&lines, proxies + 1, block_end(&lines, proxies), 2, "velocity") else { return false };
    let end = block_end(&lines, velocity);
    let read = |key: &str| {
        find_key(&lines, velocity + 1, end, 4, key).map(|at| {
            let rest = lines[at].trim_start()[key.len() + 1..].trim();
            rest.trim_matches(['\'', '"']).replace("''", "'")
        })
    };
    read("enabled").as_deref() == Some("true") && read("secret").as_deref() == Some(secret)
}

/// server.properties with one key set, added when missing.
pub fn set_property(text: &str, key: &str, wanted: &str) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|line| match line.split_once('=') {
            Some((name, _)) if name.trim() == key && !line.trim_start().starts_with('#') => {
                found = true;
                format!("{key}={wanted}")
            }
            _ => line.to_string(),
        })
        .collect();
    if !found {
        lines.push(format!("{key}={wanted}"));
    }
    let mut out = lines.join(eol);
    out.push_str(eol);
    out
}

pub fn property(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (name, rest) = line.split_once('=')?;
        (name.trim() == key && !line.trim_start().starts_with('#')).then(|| rest.trim().to_string())
    })
}

/// FabricProxy-Lite's config with forwarding from Velocity on and this secret.
pub fn fabric_proxy(text: &str, secret: &str) -> Result<String, String> {
    let mut doc: DocumentMut = text.parse().map_err(|error| format!("FabricProxy-Lite.toml could not be read: {error}"))?;
    doc["hackOnlineMode"] = value(true);
    doc["secret"] = value(secret);
    Ok(doc.to_string())
}

pub fn fabric_proxy_on(text: &str, secret: &str) -> bool {
    text.parse::<DocumentMut>().ok().and_then(|doc| doc.get("secret").and_then(Item::as_str).map(|one| one == secret)).unwrap_or(false)
}

/// A new forwarding secret: 24 hex characters.
pub fn new_secret() -> Result<String, String> {
    let mut bytes = [0_u8; 12];
    getrandom::fill(&mut bytes).map_err(|error| format!("Could not make a secret: {error}"))?;
    Ok(bytes.iter().map(|one| format!("{one:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VELOCITY: &str = r#"config-version = "2.7"
bind = "0.0.0.0:25577"
player-info-forwarding-mode = "NONE"
forwarding-secret-file = "forwarding.secret"

[servers]
lobby = "127.0.0.1:30066"
factions = "127.0.0.1:30067"
try = [
  "lobby"
]

[forced-hosts]
"lobby.example.com" = [
  "lobby"
]

[advanced]
compression-threshold = 256
"#;

    #[test]
    fn rewrites_the_velocity_server_list() {
        let read = read_velocity(VELOCITY).unwrap();
        assert!(!read.modern);
        assert_eq!(read.servers.len(), 2);
        assert_eq!(read.first, vec!["lobby"]);
        let out = write_velocity(VELOCITY, &[("survival".into(), "127.0.0.1:25565".into()), ("lobby".into(), "127.0.0.1:25566".into())], "lobby").unwrap();
        let again = read_velocity(&out).unwrap();
        assert!(again.modern);
        assert_eq!(again.servers, vec![("survival".to_string(), "127.0.0.1:25565".to_string()), ("lobby".to_string(), "127.0.0.1:25566".to_string())]);
        assert_eq!(again.first, vec!["lobby", "survival"]);
        assert!(!out.contains("lobby.example.com"), "{out}");
        assert!(out.contains("compression-threshold = 256"));
        assert_eq!(server_key("My Server 1", &["my-server-1".into()]), "my-server-1-2");
        assert_eq!(server_key("한글", &[]), "server");
    }

    #[test]
    fn turns_on_velocity_in_paper_global() {
        let yaml = "_version: 29\nproxies:\n  bungee-cord:\n    online-mode: true\n  proxy-protocol: false\n  velocity:\n    enabled: false\n    online-mode: true\n    secret: ''\nscoreboards:\n  save-empty-scoreboard-teams: true\n";
        let out = paper_velocity(yaml, true, "abc'd");
        assert!(out.contains("  velocity:\n    enabled: true\n    online-mode: true\n    secret: 'abc''d'\nscoreboards:"), "{out}");
        assert!(paper_velocity_on(&out, "abc'd"));
        assert!(!paper_velocity_on(&out, "other"));
        assert!(!paper_velocity_on(&paper_velocity(&out, false, "abc'd"), "abc'd"));
        let bare = paper_velocity("_version: 29\n", true, "s");
        assert!(paper_velocity_on(&bare, "s"), "{bare}");
        let crlf = paper_velocity(&yaml.replace('\n', "\r\n"), true, "s");
        assert!(crlf.contains("enabled: true\r\n") && !crlf.contains("\r\r"));
    }

    /// SE_PAPER_GLOBAL=<a real paper-global.yml>: only the velocity lines change.
    #[test]
    #[ignore]
    fn edits_a_real_paper_global() {
        let before = std::fs::read_to_string(std::env::var("SE_PAPER_GLOBAL").unwrap()).unwrap();
        let after = paper_velocity(&before, true, "s3cret");
        assert!(paper_velocity_on(&after, "s3cret"));
        let changed: Vec<(&str, &str)> = before.lines().zip(after.lines()).filter(|(a, b)| a != b).collect();
        eprintln!("{changed:#?}");
        assert_eq!(before.lines().count(), after.lines().count());
        assert!(changed.len() <= 2, "{changed:?}");
        assert_eq!(paper_velocity(&paper_velocity(&after, false, ""), true, "s3cret"), after);
    }

    #[test]
    fn edits_properties_and_fabric_proxy() {
        let props = "#Minecraft\nonline-mode=true\nmotd=hi\n";
        let out = set_property(props, "online-mode", "false");
        assert_eq!(out, "#Minecraft\nonline-mode=false\nmotd=hi\n");
        assert_eq!(property(&out, "online-mode").as_deref(), Some("false"));
        assert_eq!(set_property("motd=hi\n", "online-mode", "false"), "motd=hi\nonline-mode=false\n");
        let fabric = fabric_proxy("hackOnlineMode = false\nsecret = \"\"\n", "s3").unwrap();
        assert!(fabric_proxy_on(&fabric, "s3") && fabric.contains("hackOnlineMode = true"));
        assert_eq!(new_secret().unwrap().len(), 24);
    }
}

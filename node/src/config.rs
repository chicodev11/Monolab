//! `/etc/monolab-node/config.toml`: which account-server keys to trust, which server folders this
//! machine has, and which team may reach which of them.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const DEFAULT_CONFIG: &str = "/etc/monolab-node/config.toml";
pub const DEFAULT_DATA: &str = "/var/lib/monolab-node";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The account server's grant keys, `kid:hex` (what `monolab-relay --data … --grant-pin` prints).
    pub grant_keys: Vec<String>,
    /// How many console lines a teammate gets at most.
    #[serde(default = "default_console_lines")]
    pub console_lines: usize,
    /// The tmux socket the servers run under (`tmux -S`). Empty: the service user's own tmux.
    #[serde(default)]
    pub tmux_socket: String,
    /// The account server, for pairing and saying where this node is.
    #[serde(default = "default_accounts")]
    pub accounts_url: String,
    #[serde(default)]
    pub servers: Vec<ServerConfig>,
    #[serde(default)]
    pub teams: Vec<TeamConfig>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// The server's id here; grants name servers by it.
    pub id: String,
    pub name: String,
    /// The server folder (absolute).
    pub path: PathBuf,
    /// The tmux session the server's console runs in. Empty: the id.
    #[serde(default)]
    pub tmux: String,
    /// The console log, inside the server folder.
    #[serde(default = "default_log")]
    pub log: String,
    /// What starts the server in its tmux session, run by `sh -c` in the server folder
    /// (`java -Xmx4G -jar server.jar nogui`). Empty: teammates can't start it from the app.
    #[serde(default)]
    pub start: String,
    /// What is typed into the console to stop the server.
    #[serde(default = "default_stop")]
    pub stop: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamConfig {
    /// The team's id on the account server.
    pub id: String,
    /// Server ids (above) attached to this team.
    pub servers: Vec<String>,
    /// Of those, the ones where console commands are for Admins and the Owner only.
    #[serde(default)]
    pub admin_only: Vec<String>,
}

fn default_accounts() -> String {
    "https://mnlab.cc".into()
}

fn default_console_lines() -> usize {
    500
}

fn default_stop() -> String {
    "stop".into()
}

fn default_log() -> String {
    "logs/latest.log".into()
}

impl ServerConfig {
    pub fn session(&self) -> &str {
        if self.tmux.is_empty() { &self.id } else { &self.tmux }
    }
}

fn plain_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|one| one.is_ascii_alphanumeric() || matches!(one, '-' | '_' | '.'))
}

/// `1:<hex>`; `None` when malformed.
pub fn parse_key(text: &str) -> Option<(u8, [u8; 32])> {
    let (kid, key) = text.trim().split_once(':')?;
    Some((kid.trim().parse().ok()?, relay_proto::unhex(key.trim())?.try_into().ok()?))
}

impl Config {
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        Config::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Config, String> {
        let config: Config = toml::from_str(text).map_err(|error| format!("The config is not valid: {error}"))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if self.grant_keys.is_empty() {
            return Err("grant_keys is empty: add the account server's key (`kid:hex`).".into());
        }
        for key in &self.grant_keys {
            if parse_key(key).is_none() {
                return Err(format!("grant_keys: \"{key}\" is not `kid:hex` with a 32-byte key."));
            }
        }
        let mut ids = HashSet::new();
        for server in &self.servers {
            if !plain_id(&server.id) {
                return Err(format!("Server id \"{}\" may use only letters, digits, - _ and . (64 at most).", server.id));
            }
            if !ids.insert(server.id.as_str()) {
                return Err(format!("Server id \"{}\" is used twice.", server.id));
            }
            if !server.path.is_absolute() {
                return Err(format!("Server \"{}\": path must be absolute.", server.id));
            }
            if !plain_id(server.session()) {
                return Err(format!("Server \"{}\": the tmux session name may use only letters, digits, - _ and .", server.id));
            }
            if server.start.contains(['\n', '\r', '\0']) || server.stop.contains(['\n', '\r', '\0']) {
                return Err(format!("Server \"{}\": start and stop must be one line each.", server.id));
            }
            if server.stop.trim().is_empty() {
                return Err(format!("Server \"{}\": stop is empty (Minecraft servers stop with `stop`).", server.id));
            }
            crate::files::check_relative(&server.log).map_err(|_| format!("Server \"{}\": log must be a path inside the server folder.", server.id))?;
        }
        let mut teams = HashSet::new();
        for team in &self.teams {
            if !plain_id(&team.id) {
                return Err(format!("Team id \"{}\" is not valid.", team.id));
            }
            if !teams.insert(team.id.as_str()) {
                return Err(format!("Team \"{}\" is listed twice.", team.id));
            }
            for id in team.servers.iter().chain(&team.admin_only) {
                if !ids.contains(id.as_str()) {
                    return Err(format!("Team \"{}\" names server \"{id}\", which is not in [[servers]].", team.id));
                }
            }
        }
        Ok(())
    }

    pub fn trusted_keys(&self) -> Vec<(u8, [u8; 32])> {
        self.grant_keys.iter().filter_map(|one| parse_key(one)).collect()
    }

    pub fn server(&self, id: &str) -> Option<&ServerConfig> {
        self.servers.iter().find(|one| one.id == id)
    }

    pub fn team(&self, id: &str) -> Option<&TeamConfig> {
        self.teams.iter().find(|one| one.id == id)
    }

    pub fn admin_only(&self, team: &str, server: &str) -> bool {
        self.team(team).is_some_and(|one| one.admin_only.iter().any(|id| id == server))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> String {
        format!("1:{}", relay_proto::hex(&[3u8; 32]))
    }

    #[test]
    fn a_full_config_reads() {
        let text = format!(
            r#"
grant_keys = ["{}"]
[[servers]]
id = "survival"
name = "Survival"
path = "/srv/minecraft/survival"
[[teams]]
id = "01TEAM"
servers = ["survival"]
admin_only = ["survival"]
"#,
            key()
        );
        let config = Config::parse(&text).unwrap();
        assert_eq!(config.trusted_keys(), vec![(1, [3u8; 32])]);
        assert_eq!(config.server("survival").unwrap().session(), "survival");
        assert_eq!(config.server("survival").unwrap().log, "logs/latest.log");
        assert!(config.admin_only("01TEAM", "survival"));
        assert_eq!(config.console_lines, 500);
        assert_eq!(config.server("survival").unwrap().stop, "stop");
        assert!(config.server("survival").unwrap().start.is_empty());
    }

    #[test]
    fn mistakes_are_named() {
        assert!(Config::parse("grant_keys = []").unwrap_err().contains("grant_keys"));
        assert!(Config::parse(r#"grant_keys = ["1:zz"]"#).is_err());
        let base = format!("grant_keys = [\"{}\"]\n", key());
        let relative = format!("{base}[[servers]]\nid = \"a\"\nname = \"A\"\npath = \"srv/a\"\n");
        assert!(Config::parse(&relative).unwrap_err().contains("absolute"));
        let unknown = format!("{base}[[teams]]\nid = \"t\"\nservers = [\"nope\"]\n");
        assert!(Config::parse(&unknown).unwrap_err().contains("nope"));
        let escape = format!("{base}[[servers]]\nid = \"a\"\nname = \"A\"\npath = \"/srv/a\"\nlog = \"../x.log\"\n");
        assert!(Config::parse(&escape).is_err());
        let two_lines = format!("{base}[[servers]]\nid = \"a\"\nname = \"A\"\npath = \"/srv/a\"\nstart = \"java -jar a.jar\\nrm -rf /\"\n");
        assert!(Config::parse(&two_lines).unwrap_err().contains("one line"));
        let typo = format!("{base}colsole_lines = 3\n");
        assert!(Config::parse(&typo).is_err());
    }
}

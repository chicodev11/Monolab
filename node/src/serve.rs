//! What a teammate may do here, by their role grant: the rules of the app's `monolink_team::serve`
//! and `web::api::run`. Requests and answers have the app's JSON shapes, so the app's Monolink window
//! can talk to a node like it talks to a host PC.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use relay_proto::DeviceId;
use relay_proto::grant::{Grant, Role};
use serde_json::{Value, json};

use crate::backups::Backups;
use crate::config::{Config, ServerConfig};
use crate::console::Tmux;
use crate::events::Events;
use crate::power::Power;
use crate::teamlog::TeamLog;
use crate::transfer::Uploads;

/// The app-wide admin commands that touch one server (the app's `SERVER_ADMINS`, `SERVER_OWNER`).
/// Only `restore_backup` runs on a node, but the role rule comes first for all of them, as on a host PC.
const SERVER_ADMINS: &[&str] = &["restore_backup", "restore_backup_paths", "upgrade_instance", "rollback_last_upgrade"];
const SERVER_OWNER: &[&str] = &["trash_instance"];

/// What changes a server: Developers and up (a Viewer only looks).
const WRITES: &[&str] = &[
    "save_editable_file",
    "send_console_command",
    "start_instance",
    "stop_instance",
    "restart_instance",
    "force_stop_instance",
    "__upload_begin",
    "__upload_part",
    "__upload_end",
    "add_server_uploads",
    "create_backup",
    "delete_backup",
    "toggle_addon",
    "delete_addon",
    "add_addon_upload",
];

const NOT_HERE: &str = "This isn't available on a Monolab node yet.";
/// How long `__events` waits for something new (the app's wait).
const EVENTS_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

pub struct Node {
    pub config: Config,
    pub keys: Vec<(u8, [u8; 32])>,
    /// This machine's device id (the hash of its iroh key).
    pub me: DeviceId,
    pub log: TeamLog,
    pub tmux: Tmux,
    pub events: Arc<Events>,
    pub power: Power,
    pub uploads: Uploads,
    pub backups: Backups,
    /// What the team hears besides server status (plugins added); `activity::report` reads the other end.
    pub activity: std::sync::mpsc::Sender<crate::activity::Item>,
    pub activity_out: std::sync::Mutex<Option<std::sync::mpsc::Receiver<crate::activity::Item>>>,
}

impl Node {
    /// A node with its state under `data` (team log, uploads, backups).
    pub fn new(config: Config, me: DeviceId, data: &Path, tmux: Tmux) -> Node {
        let (activity, activity_out) = std::sync::mpsc::channel();
        Node {
            activity,
            activity_out: std::sync::Mutex::new(Some(activity_out)),
            keys: config.trusted_keys(),
            log: TeamLog::new(data),
            uploads: Uploads::new(data),
            backups: Backups { dir: data.join("backups"), tar: "tar".into() },
            events: Arc::new(Events::default()),
            power: Power::default(),
            config,
            me,
            tmux,
        }
    }

    fn running(&self, server: &ServerConfig) -> bool {
        server.path.is_dir() && self.tmux.running(server.session())
    }
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_secs()).unwrap_or(0)
}

/// The servers a grant may reach here: named in the grant, attached to that team in the config, and
/// configured on this machine.
pub fn reach(config: &Config, grant: &Grant) -> Vec<String> {
    let Some(team) = config.team(&grant.team) else { return Vec::new() };
    grant.servers.iter().filter(|id| team.servers.contains(id) && config.server(id).is_some()).cloned().collect()
}

/// Whether a grant (text) lets `peer` in now: signature, key, device, host, time, team version, and at
/// least one server shared here. Gives the grant or a reason code (the app's codes).
pub fn admit(node: &Node, text: &str, peer: &DeviceId, now: u64, seen: u64) -> Result<Grant, String> {
    let grant = relay_proto::grant::open(text, &node.keys).map_err(|refused| refused.code().to_string())?;
    grant.check(peer, &node.me, now, seen).map_err(|refused| refused.code().to_string())?;
    if reach(&node.config, &grant).is_empty() {
        return Err("not_shared".into());
    }
    Ok(grant)
}

/// What a teammate's app learns about the servers when it connects (the app's `welcome`).
pub fn welcome(config: &Config, grant: &Grant) -> Value {
    let list: Vec<Value> = reach(config, grant)
        .iter()
        .filter_map(|id| config.server(id))
        .map(|server| json!({ "instance": server.id, "name": server.name, "adminOnly": config.admin_only(&grant.team, &server.id) }))
        .collect();
    // `node`: the window hides what a node can't do yet (start, stop, uploads, backups, plugins).
    json!({ "role": grant.role.as_str(), "servers": list, "node": true })
}

/// Answers one request and writes it to the team log.
pub fn serve(node: &Node, peer: &DeviceId, grant: &Grant, command: &str, body: &Value) -> Result<Value, String> {
    let result = serve_one(node, peer, grant, command, body);
    // The window's long wait and each piece of an upload would fill the log (the app leaves them out too).
    if matches!(command, "__events" | "__upload_part") {
        return result;
    }
    // A plugin's bytes are not something to keep in a log.
    let mut body = body.clone();
    if let Some(files) = body.get_mut("files").and_then(Value::as_array_mut) {
        for one in files.iter_mut().filter_map(Value::as_object_mut) {
            one.remove("data");
        }
    }
    let body = &body;
    node.log.record(&format!("@{}", grant.handle), &format!("team · {}", grant.role.as_str()), &peer.text(), command, body, &result);
    result
}

fn text_arg<'a>(body: &'a Value, name: &str) -> &'a str {
    body.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn serve_one(node: &Node, peer: &DeviceId, grant: &Grant, command: &str, body: &Value) -> Result<Value, String> {
    let config = &node.config;
    let servers = reach(config, grant);
    if servers.is_empty() {
        return Err("No server here is shared with this team any more.".into());
    }
    let viewer = grant.role == Role::Viewer;
    if viewer && WRITES.contains(&command) {
        return Err("You can look at this server but not change it.".into());
    }
    // The server a request is about, if the teammate may open it.
    let server = || -> Result<&ServerConfig, String> {
        let id = text_arg(body, "id");
        if !servers.iter().any(|one| one == id) {
            return Err("You don't have permission for this server.".into());
        }
        config.server(id).ok_or_else(|| "You don't have permission for this server.".to_string())
    };
    let mut value = match command {
        "__whoami" => {
            let role = if viewer { "Viewer" } else { "Manager" };
            json!({ "name": format!("@{}", grant.handle), "role": role, "teamRole": grant.role.as_str(), "node": true })
        }
        // F7: the team's Owner reads this node's teammate log (the host app shows it like its own).
        "__team_log" => {
            if grant.role < Role::Owner {
                return Err("Only the team owner can read this node's teammate log.".into());
            }
            let limit = body.get("limit").and_then(Value::as_u64).unwrap_or(200).clamp(1, 1000) as usize;
            let before = body.get("before").and_then(Value::as_f64);
            serde_json::to_value(node.log.read(text_arg(body, "server"), text_arg(body, "who"), text_arg(body, "text"), before, limit)).map_err(|error| error.to_string())?
        }
        "list_instances" => Value::Array(servers.iter().filter_map(|id| config.server(id)).enumerate().map(|(at, one)| instance_view(node, one, at)).collect()),
        // Console lines and status changes of the servers this grant reaches, as they happen.
        "__events" => {
            let after = body.get("after").and_then(Value::as_u64).unwrap_or(0);
            node.events.wait(after, &servers, EVENTS_WAIT)
        }
        "start_instance" => {
            node.power.start(&node.tmux, &node.events, server()?)?;
            Value::Null
        }
        "stop_instance" | "restart_instance" => {
            node.power.stop(&node.tmux, &node.events, server()?, command == "restart_instance", crate::power::STOP_WAIT)?;
            Value::Null
        }
        "force_stop_instance" => {
            node.power.force(&node.tmux, &node.events, server()?)?;
            Value::Null
        }
        "__upload_begin" | "__upload_part" | "__upload_end" => node.uploads.request(peer, command, body)?,
        "add_server_uploads" => {
            let found = server()?;
            let tokens: Vec<String> = body.get("uploads").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
            json!(crate::transfer::add_to_server(&node.uploads, peer, &found.path, text_arg(body, "path"), &tokens)?)
        }
        "read_server_file_download" => {
            let found = server()?;
            let path = text_arg(body, "path");
            match crate::files::read_text(&found.path, path) {
                // A Viewer gets text files with their secrets hidden, like in the editor.
                Ok(text) if viewer => {
                    let mut text = Value::String(text);
                    hide_secrets(&mut text);
                    let name = Path::new(path).file_name().map(|one| one.to_string_lossy().into_owned()).unwrap_or_else(|| "File".into());
                    json!({ "name": name, "data": crate::transfer::encode(text.as_str().unwrap_or_default().as_bytes()) })
                }
                _ => crate::transfer::download(&found.path, path)?,
            }
        }
        "list_backups" => serde_json::to_value(node.backups.list(&server()?.id)).map_err(|error| error.to_string())?,
        "create_backup" => {
            let found = server()?;
            let paths: Vec<String> = body.get("paths").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
            let worlds = body.get("worldsOnly").and_then(Value::as_bool).unwrap_or(false);
            serde_json::to_value(node.backups.create(found, text_arg(body, "note"), worlds, &paths)?).map_err(|error| error.to_string())?
        }
        "delete_backup" => {
            node.backups.delete(&server()?.id, text_arg(body, "fileName"))?;
            Value::Null
        }
        "restore_backup" => {
            if grant.role < Role::Admin {
                return Err("Only admins can do this.".into());
            }
            let found = server()?;
            if node.running(found) || node.power.is_stopping(&found.id) {
                return Err("Stop the server completely before changing data.".into());
            }
            serde_json::to_value(node.backups.restore(found, text_arg(body, "fileName"))?).map_err(|error| error.to_string())?
        }
        "scan_addons" => serde_json::to_value(crate::addons::scan(&server()?.path)).map_err(|error| error.to_string())?,
        "toggle_addon" | "delete_addon" => {
            let found = server()?;
            if node.running(found) {
                return Err("Stop the server completely before changing data.".into());
            }
            let (name, kind) = (text_arg(body, "fileName"), text_arg(body, "kind"));
            if command == "toggle_addon" {
                Value::String(crate::addons::toggle(&found.path, kind, name)?)
            } else {
                crate::addons::delete(&found.path, kind, name)?;
                Value::Null
            }
        }
        "add_addon_upload" => {
            let found = server()?;
            let files = body.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
            let added = crate::addons::add(&found.path, &files, node.running(found))?;
            for name in added["added"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                let _ = node.activity.send((found.id.clone(), "plugin_added", json!({ "name": name })));
            }
            added
        }
        // The Monolab add-on (studio, map) isn't managed by a node: the plugins page shows it as not here.
        "addon_status" => {
            server()?;
            json!({ "supported": false, "reason": "A Monolab node doesn't manage the Monolab add-on.", "kind": "", "installed": false, "installedVersion": null, "latestVersion": 0, "needsUpdate": false, "running": false, "folder": "" })
        }
        "browse_server_files" => {
            let found = server()?;
            let offset = body.get("offset").and_then(Value::as_u64);
            let limit = body.get("limit").and_then(Value::as_u64);
            serde_json::to_value(crate::files::browse(&found.path, text_arg(body, "path"), offset, limit)?).map_err(|error| error.to_string())?
        }
        "read_editable_file" => Value::String(crate::files::read_text(&server()?.path, text_arg(body, "path"))?),
        "get_console_logs" => serde_json::to_value(crate::console::tail(server()?, config.console_lines)?).map_err(|error| error.to_string())?,
        "save_editable_file" | "send_console_command" => {
            let found = server()?;
            if command == "save_editable_file" {
                crate::files::save_text(&found.path, text_arg(body, "path"), text_arg(body, "content"))?;
            } else {
                if grant.role < Role::Admin && config.admin_only(&grant.team, &found.id) {
                    return Err("Only admins can run commands on this server.".into());
                }
                node.tmux.send(found.session(), text_arg(body, "command"))?;
            }
            Value::Null
        }
        _ if SERVER_OWNER.contains(&command) => {
            if grant.role < Role::Owner {
                return Err("Only the team owner can do this.".into());
            }
            return Err(NOT_HERE.into());
        }
        _ if SERVER_ADMINS.contains(&command) => {
            if grant.role < Role::Admin {
                return Err("Only admins can do this.".into());
            }
            return Err(NOT_HERE.into());
        }
        _ => return Err(NOT_HERE.into()),
    };
    if viewer {
        hide_secrets(&mut value);
    }
    Ok(value)
}

/// The `server-port` in `server.properties`, or Minecraft's default.
fn server_port(path: &Path) -> u16 {
    std::fs::read_to_string(path.join("server.properties"))
        .ok()
        .and_then(|text| text.lines().find_map(|line| line.trim().strip_prefix("server-port=").and_then(|port| port.trim().parse().ok())))
        .unwrap_or(25565)
}

/// One server in the app's `InstanceView` shape (a `ServerInstance` with its state). Fields the node
/// doesn't know are left at their defaults.
fn instance_view(node: &Node, server: &ServerConfig, at: usize) -> Value {
    let missing = !server.path.is_dir();
    let running = node.running(server);
    let status = if running && node.power.is_stopping(&server.id) {
        "stopping"
    } else if running {
        "running"
    } else {
        "stopped"
    };
    json!({
        "id": server.id,
        "name": server.name,
        "path": server.path.to_string_lossy(),
        "core": "",
        "version": "",
        "launchMode": "custom",
        "serverJar": null,
        "javaPath": "",
        "maxMemoryMib": 0,
        "minMemoryMib": 0,
        "jvmArgs": "",
        "port": server_port(&server.path),
        "sortOrder": at,
        "group": "",
        "startWithApp": false,
        "crashRestart": false,
        "coreBuild": "",
        "createdAt": 0,
        "lastRunAt": 0,
        "customCommand": "",
        "startScript": "",
        "startScriptHash": "",
        "remoteHost": "",
        "linked": false,
        "panelHost": "",
        "panelServer": "",
        "panelDaemon": "",
        "named": true,
        "status": status,
        "uptimeSeconds": null,
        "crashReason": null,
        "ready": running,
        "checking": false,
        "unreachable": false,
        "missing": missing,
    })
}

/// A setting name that holds a secret (the app's `secret_name`).
fn secret_name(name: &str) -> bool {
    let name = name.trim().trim_matches('"').trim_matches('\'').to_ascii_lowercase();
    ["password", "passwd", "secret", "token", "apikey", "api-key", "api_key", "private-key", "private_key", "webhook"].iter().any(|one| name.contains(one))
}

/// What a Viewer gets back: secret values in file text and console lines replaced by asterisks
/// (the app's `hide_secrets`).
pub fn hide_secrets(value: &mut Value) {
    static LINE: OnceLock<regex::Regex> = OnceLock::new();
    let line = LINE.get_or_init(|| regex::Regex::new(r#"(?m)^([ \t]*"?([A-Za-z0-9_.\-]+)"?[ \t]*[:=][ \t]*)(.+?)([ \t]*,?)\r?$"#).expect("secret line regex"));
    match value {
        Value::String(text) => {
            if line.is_match(text) {
                let hidden = line.replace_all(text, |found: &regex::Captures| {
                    if secret_name(&found[2]) && !found[3].trim().is_empty() { format!("{}********{}", &found[1], &found[4]) } else { found[0].to_string() }
                });
                *text = hidden.into_owned();
            }
        }
        Value::Array(list) => list.iter_mut().for_each(hide_secrets),
        Value::Object(map) => {
            let named = map.get("key").or_else(|| map.get("name")).and_then(Value::as_str).is_some_and(secret_name);
            for (name, one) in map.iter_mut() {
                if (secret_name(name) || (named && name == "value")) && one.as_str().is_some_and(|text| !text.is_empty()) {
                    *one = Value::String("********".into());
                } else {
                    hide_secrets(one);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use relay_proto::grant::{LIFE_SECS, sign};
    use std::path::PathBuf;

    const NOW: u64 = 1_800_000_000;
    const GUEST: DeviceId = DeviceId([1; 16]);
    const HOST: DeviceId = DeviceId([2; 16]);

    fn signing() -> SigningKey {
        SigningKey::from_bytes(&[7; 32])
    }

    /// A node with two servers, `survival` (attached, admin-only commands) and `creative` (attached)
    /// in team `T1`, and `secret` attached to no team. `tmux` is a stand-in that always fails.
    fn node(name: &str) -> (Node, PathBuf) {
        let data = crate::files::tests::sandbox(name);
        for server in ["survival", "creative", "secret"] {
            std::fs::create_dir_all(data.join(server)).unwrap();
            std::fs::write(data.join(server).join("server.properties"), "server-port=25570\nrcon.password=hunter2\nmotd=hi\n").unwrap();
        }
        let text = format!(
            r#"
grant_keys = ["1:{key}"]
[[servers]]
id = "survival"
name = "Survival"
path = "{root}/survival"
[[servers]]
id = "creative"
name = "Creative"
path = "{root}/creative"
[[servers]]
id = "secret"
name = "Secret"
path = "{root}/secret"
[[teams]]
id = "T1"
servers = ["survival", "creative"]
admin_only = ["survival"]
"#,
            key = relay_proto::hex(&signing().verifying_key().to_bytes()),
            root = data.display()
        );
        let config = Config::parse(&text).unwrap();
        let node = Node::new(config, HOST, &data.join("state"), Tmux { program: "false".into(), socket: String::new() });
        (node, data)
    }

    fn grant(role: Role) -> Grant {
        Grant {
            v: 1,
            kid: 1,
            user: "01USER".into(),
            handle: "bob".into(),
            device: GUEST,
            team: "T1".into(),
            team_version: 3,
            role,
            host: HOST,
            servers: vec!["survival".into(), "creative".into(), "secret".into()],
            iat: NOW - 10,
            exp: NOW - 10 + LIFE_SECS,
        }
    }

    #[test]
    fn grants_are_checked() {
        let (node, data) = node("admit");
        let good = sign(&grant(Role::Developer), &signing());
        let admitted = admit(&node, &good, &GUEST, NOW, 0).unwrap();
        assert_eq!(reach(&node.config, &admitted), ["survival", "creative"], "a server attached to no team stays shut");

        // Signed with a key this node doesn't trust, or under a key number it doesn't know.
        let stranger = SigningKey::from_bytes(&[8; 32]);
        assert_eq!(admit(&node, &sign(&grant(Role::Developer), &stranger), &GUEST, NOW, 0).unwrap_err(), "bad_signature");
        let mut other_kid = grant(Role::Developer);
        other_kid.kid = 2;
        assert_eq!(admit(&node, &sign(&other_kid, &signing()), &GUEST, NOW, 0).unwrap_err(), "unknown_key");
        assert_eq!(admit(&node, "garbage", &GUEST, NOW, 0).unwrap_err(), "malformed");

        // Expired, or made in the future.
        assert_eq!(admit(&node, &good, &GUEST, NOW + LIFE_SECS, 0).unwrap_err(), "expired");
        assert_eq!(admit(&node, &good, &GUEST, NOW - 1000, 0).unwrap_err(), "clock");

        // Another team, another device, another host, an older team version.
        let mut other_team = grant(Role::Owner);
        other_team.team = "T2".into();
        assert_eq!(admit(&node, &sign(&other_team, &signing()), &GUEST, NOW, 0).unwrap_err(), "not_shared");
        assert_eq!(admit(&node, &good, &DeviceId([9; 16]), NOW, 0).unwrap_err(), "wrong_device");
        let mut other_host = grant(Role::Developer);
        other_host.host = DeviceId([9; 16]);
        assert_eq!(admit(&node, &sign(&other_host, &signing()), &GUEST, NOW, 0).unwrap_err(), "wrong_host");
        assert_eq!(admit(&node, &good, &GUEST, NOW, 4).unwrap_err(), "outdated");

        // Only servers this node never attached for the team.
        let mut elsewhere = grant(Role::Developer);
        elsewhere.servers = vec!["secret".into()];
        assert_eq!(admit(&node, &sign(&elsewhere, &signing()), &GUEST, NOW, 0).unwrap_err(), "not_shared");
        std::fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn viewers_only_look() {
        let (node, data) = node("roles");
        let viewer = grant(Role::Viewer);
        let save = json!({ "id": "creative", "path": "server.properties", "content": "motd=changed\n" });
        let refused = serve(&node, &GUEST, &viewer, "save_editable_file", &save).unwrap_err();
        assert!(refused.contains("not change"), "{refused}");
        assert!(serve(&node, &GUEST, &viewer, "send_console_command", &json!({ "id": "creative", "command": "op bob" })).unwrap_err().contains("not change"));
        assert_eq!(std::fs::read_to_string(data.join("creative/server.properties")).unwrap().lines().last(), Some("motd=hi"));

        // Viewers read, with secrets hidden.
        let text = serve(&node, &GUEST, &viewer, "read_editable_file", &json!({ "id": "creative", "path": "server.properties" })).unwrap();
        assert!(text.as_str().unwrap().contains("rcon.password=********"));
        assert!(!text.as_str().unwrap().contains("hunter2"));
        let whoami = serve(&node, &GUEST, &viewer, "__whoami", &json!({})).unwrap();
        assert_eq!(whoami, json!({ "name": "@bob", "role": "Viewer", "teamRole": "viewer", "node": true }));

        // A Developer saves; the file changes.
        let developer = grant(Role::Developer);
        serve(&node, &GUEST, &developer, "save_editable_file", &save).unwrap();
        assert_eq!(std::fs::read_to_string(data.join("creative/server.properties")).unwrap(), "motd=changed\n");
        let text = serve(&node, &GUEST, &developer, "read_editable_file", &json!({ "id": "survival", "path": "server.properties" })).unwrap();
        assert!(text.as_str().unwrap().contains("hunter2"), "secrets are hidden from Viewers only");

        // "Commands: admins only" on survival: a Developer is refused, an Admin gets past the role check
        // (and then finds no tmux session, as the stand-in tmux always fails).
        let command = json!({ "id": "survival", "command": "list" });
        assert!(serve(&node, &GUEST, &developer, "send_console_command", &command).unwrap_err().contains("Only admins"));
        assert_eq!(serve(&node, &GUEST, &grant(Role::Admin), "send_console_command", &command).unwrap_err(), "The server is not running.");
        assert_eq!(serve(&node, &GUEST, &developer, "send_console_command", &json!({ "id": "creative", "command": "list" })).unwrap_err(), "The server is not running.");

        // Owner-only and admin-only app commands keep their role rule.
        assert!(serve(&node, &GUEST, &grant(Role::Admin), "trash_instance", &json!({ "id": "creative" })).unwrap_err().contains("owner"));
        assert!(serve(&node, &GUEST, &developer, "restore_backup", &json!({ "id": "creative" })).unwrap_err().contains("Only admins"));
        assert_eq!(serve(&node, &GUEST, &grant(Role::Owner), "trash_instance", &json!({ "id": "creative" })).unwrap_err(), NOT_HERE);
        assert_eq!(serve(&node, &GUEST, &grant(Role::Owner), "get_app_settings", &json!({})).unwrap_err(), NOT_HERE);
        std::fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn second_round_commands() {
        let (node, data) = node("second");
        let viewer = grant(Role::Viewer);
        let developer = grant(Role::Developer);
        let at = |name: &str| json!({ "id": "creative", "fileName": name, "kind": "plugin", "path": "server.properties" });

        // A Viewer looks: no power, uploads, backups or plugin changes.
        for command in ["start_instance", "stop_instance", "restart_instance", "force_stop_instance", "__upload_begin", "add_server_uploads", "create_backup", "delete_backup", "toggle_addon", "delete_addon", "add_addon_upload"] {
            assert!(serve(&node, &GUEST, &viewer, command, &at("a.jar")).unwrap_err().contains("not change"), "{command}");
        }
        // Downloads: a Viewer's text file has its secrets hidden.
        let handed = serve(&node, &GUEST, &viewer, "read_server_file_download", &at("")).unwrap();
        let text = String::from_utf8(crate::transfer::decode(handed["data"].as_str().unwrap()).unwrap()).unwrap();
        assert!(text.contains("rcon.password=********") && !text.contains("hunter2"), "{text}");
        let handed = serve(&node, &GUEST, &developer, "read_server_file_download", &at("")).unwrap();
        assert!(String::from_utf8(crate::transfer::decode(handed["data"].as_str().unwrap()).unwrap()).unwrap().contains("hunter2"));

        // Starting needs a start command (none in this config); the stand-in tmux runs nothing.
        assert!(serve(&node, &GUEST, &developer, "start_instance", &at("")).unwrap_err().contains("start command"));
        assert!(serve(&node, &GUEST, &developer, "stop_instance", &at("")).unwrap_err().contains("not running"));

        // Plugins: list, turn off, the add-on card says "not here".
        std::fs::create_dir_all(data.join("creative/plugins")).unwrap();
        std::fs::write(data.join("creative/plugins/A.jar"), "a").unwrap();
        let list = serve(&node, &GUEST, &viewer, "scan_addons", &at("")).unwrap();
        assert_eq!(list[0]["fileName"], "A.jar");
        assert_eq!(serve(&node, &GUEST, &developer, "toggle_addon", &at("A.jar")).unwrap(), "A.jar.disabled");
        assert_eq!(serve(&node, &GUEST, &viewer, "addon_status", &at("")).unwrap()["supported"], false);

        // Backups (needs `tar`): made by a Developer, restored only by an Admin.
        let made = serve(&node, &GUEST, &developer, "create_backup", &json!({ "id": "creative", "note": "n", "worldsOnly": false, "paths": [] })).unwrap();
        let name = made["fileName"].as_str().unwrap().to_string();
        assert_eq!(serve(&node, &GUEST, &viewer, "list_backups", &at("")).unwrap().as_array().unwrap().len(), 1);
        serve(&node, &GUEST, &developer, "save_editable_file", &json!({ "id": "creative", "path": "server.properties", "content": "motd=changed\n" })).unwrap();
        assert!(serve(&node, &GUEST, &developer, "restore_backup", &at(&name)).unwrap_err().contains("Only admins"));
        let before = serve(&node, &GUEST, &grant(Role::Admin), "restore_backup", &at(&name)).unwrap();
        assert!(before["note"].as_str().unwrap().starts_with("Before restoring"));
        assert!(std::fs::read_to_string(data.join("creative/server.properties")).unwrap().contains("motd=hi"));
        serve(&node, &GUEST, &developer, "delete_backup", &at(&name)).unwrap();

        // Events: only the servers this grant reaches.
        node.events.status("secret", "running");
        node.events.status("creative", "running");
        let heard = serve(&node, &GUEST, &viewer, "__events", &json!({ "after": 0 })).unwrap();
        let about: Vec<&str> = heard["events"].as_array().unwrap().iter().map(|one| one["payload"]["serverId"].as_str().unwrap()).collect();
        assert_eq!(about, ["creative"]);
        std::fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn team_log_owner_only() {
        let (node, data) = node("teamlog-read");
        serve(&node, &GUEST, &grant(Role::Developer), "list_instances", &json!({})).unwrap();
        assert!(serve(&node, &GUEST, &grant(Role::Admin), "__team_log", &json!({})).unwrap_err().contains("owner"));
        let read = serve(&node, &GUEST, &grant(Role::Owner), "__team_log", &json!({ "limit": 5 })).unwrap();
        let rows = read.as_array().unwrap();
        assert!(rows.iter().any(|one| one["command"] == "list_instances"), "{read}");
        std::fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn only_shared_servers_are_reached() {
        let (node, data) = node("reach");
        let owner = grant(Role::Owner);
        let list = serve(&node, &GUEST, &owner, "list_instances", &json!({})).unwrap();
        let ids: Vec<&str> = list.as_array().unwrap().iter().map(|one| one["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["survival", "creative"]);
        assert_eq!(list[0]["port"], 25570);
        assert_eq!(list[0]["status"], "stopped");
        for command in ["browse_server_files", "read_editable_file", "get_console_logs", "save_editable_file"] {
            let refused = serve(&node, &GUEST, &owner, command, &json!({ "id": "secret", "path": "server.properties", "content": "x" })).unwrap_err();
            assert!(refused.contains("permission"), "{command}: {refused}");
        }
        assert!(serve(&node, &GUEST, &owner, "browse_server_files", &json!({ "id": "creative", "path": "../secret" })).unwrap_err().contains("outside"));
        assert!(serve(&node, &GUEST, &owner, "read_editable_file", &json!({ "id": "creative", "path": "/etc/hostname" })).unwrap_err().contains("outside"));
        let page = serve(&node, &GUEST, &owner, "browse_server_files", &json!({ "id": "creative", "path": "" })).unwrap();
        assert_eq!(page["entries"][0]["name"], "server.properties");

        // A grant whose servers aren't attached here gets nothing at all.
        let mut stranger = grant(Role::Owner);
        stranger.servers = vec!["secret".into()];
        assert!(serve(&node, &GUEST, &stranger, "list_instances", &json!({})).unwrap_err().contains("No server"));

        // Every request above went to the team log.
        let file = std::fs::read_dir(data.join("state/team-log")).unwrap().next().unwrap().unwrap().path();
        let lines = std::fs::read_to_string(file).unwrap();
        assert_eq!(lines.lines().count(), 9);
        assert!(lines.contains("\"who\":\"@bob\"") && lines.contains("\"via\":\"team · owner\""));
        std::fs::remove_dir_all(&data).unwrap();
    }
}

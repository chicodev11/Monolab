use super::auth::{self, Account};
use tauri::{AppHandle, Manager};

static DIST: include_dir::Dir<'_> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/../dist");

pub fn index() -> Vec<u8> {
    if cfg!(debug_assertions) {
        if let Ok(text) = std::fs::read(disk().join("index.html")) {
            return text;
        }
    }
    include_str!("../../../dist/index.html").as_bytes().to_vec()
}

fn disk() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist")
}

pub fn asset(path: &str) -> Option<(&'static str, Vec<u8>)> {
    let name = path.trim_start_matches('/');
    if name.is_empty() {
        return None;
    }
    let body = if cfg!(debug_assertions) {
        std::fs::read(crate::paths::resolve_in_server(&disk(), name).ok()?).ok()?
    } else {
        DIST.get_file(name)?.contents().to_vec()
    };
    let kind = match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("json") => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    };
    Some((kind, body))
}

const READS: &[&str] = &[
    "panel_usage",
    "read_server_agents_md",
    // Studio: the files under plugins/Monolab/studio and the add-on's live line (joins, rule runs).
    "studio_overview",
    "studio_assets",
    "studio_asset_list",
    "studio_asset_image",
    "studio_expansions",
    "studio_read",
    "studio_template",
    "studio_worldgen_preview",
    "studio_watch",
    "studio_unwatch",
    "list_instances",
    "get_app_settings",
    "app_version",
    "read_live",
    "read_loaded_chunks",
    "map_surface_height",
    "disk_usage",
    "chunky_status",
    "chunk_stats",
    "list_runs",
    "server_memo",
    "agent_read_memo",
    "memo_image",
    "diagnose_lag",
    "addon_status",
    "pumpkin_addon_allowed",
    "pumpkin_refused_permissions",
    "read_server_properties",
    "list_worlds",
    "browse_server_files",
    "read_editable_file",
    "bundled_forks",
    "read_server_image",
    "plugin_integrations",
    "essentials_data",
    "extension_data",
    "addon_config",
    "view_server_file",
    "read_texture_animation",
    "read_server_file_download",
    "preview_share_log",
    "list_backups",
    "backup_contents",
    "proxy_network",
    "backup_entry_text",
    "list_log_files",
    "list_server_trash",
    "list_crash_reports",
    "inspect_addons",
    "list_event_days",
    "perf_history",
    "server_commands",
    "list_java_runtimes",
    "required_java",
    "list_tasks",
    "list_audit_entries",
    "get_console_logs",
    "get_resource_snapshot",
    "system_memory_mib",
    "get_server_icon",
    "list_players",
    "read_world_settings",
    "list_bookmarks",
    "summarize_logs",
    "read_log_timeline",
    "list_log_entries",
    "list_player_activity",
    "list_core_versions",
    "precheck_instance",
    "list_remote_javas",
    "scan_addons",
    "list_addon_rollbacks",
    "list_automation_history",
    "list_upgrade_history",
    "get_resource_history",
    "get_tps_snapshot",
    "list_core_builds",
    "check_core_build",
    "get_rcon_status",
    "get_server_ping",
    "list_addon_versions",
    "modrinth_project_detail",
    "list_player_spots",
    "lookup_mojang_profile",
    "plan_addon_install",
    "read_player_nbt",
    "render_map_tile",
    "world_map_bounds",
    "map_changes",
    "search_addon_catalog",
    "check_addon_updates",
    "enrich_addons",
    "get_server_icon",
    "list_core_builds",
    "list_pending_addons",
    "item_names",
    "enchantment_names",
    "item_icons",
    "item_ids",
    "game_files",
    "playit_state",
    "lan_ip",
    "datapack_folder",
    "search_datapacks",
    "rewind_list",
    "rewind_versions",
    "rewind_enabled",
    "msmp_state",
    "msmp_settings",
    "msmp_rules",
    "web_online",
];

/// Whether a command only reads: on the read list and not on a change list (some, like rewind, are
/// on both). The teammate log folds repeats of these into one line, and teammates' agents use them
/// without asking the host.
pub fn is_read(command: &str) -> bool {
    READS.contains(&command) && !WRITES.contains(&command) && !ADMINS.contains(&command)
}

const ADMINS: &[&str] = &[
    // The shared AGENTS.md: web panel admins; team Admins and the Owner through `monolink_team`.
    "save_server_agents_md",
    "list_clients",
    "get_client_account",
    "client_runs",
    "create_client",
    "rename_client",
    "delete_client",
    "set_offline_name",
    "launch_client",
    "stop_client",
    "dismiss_client_run",
    "client_content",
    "client_content_cards",
    "set_client_content_enabled",
    "remove_client_content",
    "update_client_content",
    "create_instance",
    "create_from_modpack",
    "kill_java_process",
    "duplicate_instance",
    "trash_instance",
    "restore_trash_instance",
    "purge_trash_instance",
    "empty_trash",
    "reorder_instances",
    "upgrade_instance",
    "rollback_last_upgrade",
    "restore_backup",
    "restore_backup_paths",
    "link_proxy",
    "set_app_settings",
    "list_web_accounts",
    "save_web_account",
    "delete_web_account",
    // The host's own roles: web panel admins manage them like accounts.
    "role_catalog",
    "save_custom_role",
    "delete_custom_role",
    "download_java",
    "remove_java_runtime",
    "cancel_create_download",
    "create_web_invite",
    "list_web_invites",
    "revoke_web_invite",
    "server_people",
    "remove_server_access",
    "get_servers_root",
    "list_trash_instances",
    "suggest_server_name",
    "list_java_processes",
    "rename_server_group",
];

const WRITES: &[&str] = &[
    "start_instance",
    "stop_instance",
    "restart_instance",
    "send_console_command",
    "essentials_set_setting",
    "luckperms_data",
    "extension_set",
    "run_extension_action",
    "set_addon_config",
    "save_server_properties",
    "save_editable_file",
    "nbt_open",
    "nbt_open_chunk",
    "nbt_act",
    "nbt_copy",
    "nbt_save",
    "nbt_close",
    "rename_server_path",
    "trash_server_path",
    "restore_server_trash",
    "delete_server_trash",
    "empty_server_trash",
    "clean_disk",
    "chunky_run",
    "create_server_folder",
    "create_server_file",
    "add_server_uploads",
    "zip_server_paths",
    "unzip_server_path",
    "create_backup",
    "update_launch_settings",
    "toggle_addon",
    "save_task",
    "delete_task",
    "run_task_now",
    "save_bookmark",
    "save_world_settings",
    "toggle_player_access",
    "edit_player_item",
    "apply_addon_updates",
    "delete_addon",
    "delete_backup",
    "delete_bookmark",
    "force_stop_instance",
    "install_addon",
    // Granting a plugin a permission is the same kind of change as installing it.
    "set_pumpkin_addon_allowed",
    "allow_pumpkin_plugin",
    "install_addon_with_dependencies",
    "install_modrinth",
    "remove_addon",
    "rollback_addon_update",
    "send_rcon_command",
    "clear_server_icon",
    "share_log",
    "set_start_with_app",
    "set_crash_restart",
    "rename_instance",
    "set_server_group",
    "set_server_memo",
    "agent_update_todo",
    "add_memo_image",
    "delete_memo_image",
    "set_server_icon_upload",
    "add_addon_upload",
    "stash_upload",
    "preview_mrpack",
    "search_modpacks",
    "preview_cfpack",
    "import_cfpack",
    "import_mrpack",
    "download_modpack",
    "export_mrpack_download",
    "add_addon_files",
    "install_datapack",
    "cancel_pending_addon",
    "playit_expose",
    "playit_unexpose",
    "rewind_scan",
    "rewind_restore",
    "set_rewind_enabled",
    "msmp_setup",
    "msmp_set_setting",
    "msmp_set_rule",
    "studio_save",
    "download_studio_assets",
    "studio_asset_add_upload",
    "studio_asset_remove",
    "studio_pack_build",
    "studio_bundle_peek_upload",
    "studio_bundle_import_upload",
];

/// The permissions (any one of them) a command needs beyond opening its server; empty = none.
/// Every command on the change lists needs one (a test checks), so a role with nothing ticked changes nothing.
pub fn needs(command: &str) -> &'static [&'static str] {
    match command {
        "get_console_logs" | "list_log_files" | "summarize_logs" | "read_log_timeline" | "list_log_entries" | "list_crash_reports"
        | "preview_share_log" => &["console.view"],
        "send_console_command" | "send_rcon_command" | "share_log" => &["console.send"],
        "start_instance" | "stop_instance" | "restart_instance" | "force_stop_instance" | "set_start_with_app" | "set_crash_restart" => {
            &["power"]
        }
        "browse_server_files"
        | "read_editable_file"
        | "view_server_file"
        | "read_server_image"
        | "read_texture_animation"
        | "list_server_trash" => &["files.read"],
        "save_editable_file"
        | "save_server_properties"
        | "nbt_open"
        | "nbt_open_chunk"
        | "nbt_act"
        | "nbt_copy"
        | "nbt_save"
        | "nbt_close"
        | "rename_server_path"
        | "trash_server_path"
        | "restore_server_trash"
        | "delete_server_trash"
        | "empty_server_trash"
        | "clean_disk"
        | "chunky_run"
        | "create_server_folder"
        | "create_server_file"
        | "add_server_uploads"
        | "zip_server_paths"
        | "unzip_server_path" => &["files.write"],
        // Whole files and packs as they are: secrets in them can't be hidden.
        "read_server_file_download" | "export_mrpack_download" => &["secrets"],
        "set_addon_config"
        | "toggle_addon"
        | "apply_addon_updates"
        | "delete_addon"
        | "install_addon"
        | "set_pumpkin_addon_allowed"
        | "allow_pumpkin_plugin"
        | "install_addon_with_dependencies"
        | "install_modrinth"
        | "remove_addon"
        | "rollback_addon_update"
        | "add_addon_upload"
        | "add_addon_files"
        | "install_datapack"
        | "cancel_pending_addon"
        | "preview_mrpack"
        | "search_modpacks"
        | "preview_cfpack"
        | "import_cfpack"
        | "import_mrpack"
        | "download_modpack" => &["plugins"],
        "create_backup"
        | "delete_backup"
        | "rewind_scan"
        | "rewind_restore"
        | "set_rewind_enabled"
        | "restore_backup"
        | "restore_backup_paths" => &["backups"],
        "studio_overview" | "studio_read" | "studio_watch" | "studio_unwatch" | "studio_template" | "studio_assets"
        | "studio_asset_list" | "studio_asset_image" | "studio_expansions" => &["studio.read"],
        "studio_save"
        | "download_studio_assets"
        | "studio_asset_add_upload"
        | "studio_asset_remove"
        | "studio_pack_build"
        | "studio_bundle_peek_upload"
        | "studio_bundle_import_upload" => &["studio.write"],
        "toggle_player_access"
        | "edit_player_item"
        | "essentials_set_setting"
        | "luckperms_data"
        | "extension_set"
        | "run_extension_action" => &["players"],
        "update_launch_settings"
        | "save_task"
        | "delete_task"
        | "run_task_now"
        | "save_bookmark"
        | "delete_bookmark"
        | "save_world_settings"
        | "clear_server_icon"
        | "rename_instance"
        | "set_server_group"
        | "set_server_memo"
        | "agent_update_todo"
        | "add_memo_image"
        | "delete_memo_image"
        | "set_server_icon_upload"
        | "playit_expose"
        | "playit_unexpose"
        | "upgrade_instance"
        | "rollback_last_upgrade"
        | "trash_instance"
        | "save_server_agents_md"
        | "msmp_setup"
        | "msmp_set_setting"
        | "msmp_set_rule" => &["settings"],
        // A file waiting to be used by another change (an icon, a plugin, an upload): any change will do.
        "stash_upload" => &["files.write", "plugins", "settings", "studio.write"],
        _ => &[],
    }
}

/// Whether permissions cover a command.
pub fn permits(perms: &[String], command: &str) -> bool {
    let need = needs(command);
    need.is_empty() || need.iter().any(|one| perms.iter().any(|mine| mine == one))
}

/// The permissions of whoever is asking: everything for an Admin, else their role's (built-in or the host's own).
pub fn perms_for(app: &AppHandle, who: &Account) -> Vec<String> {
    if who.is_admin() {
        return auth::PERMS.iter().map(|one| one.to_string()).collect();
    }
    auth::perms_of(&who.role, &crate::custom_roles(app))
}

/// Takes console lines out of an events answer for a role that may not watch the console.
pub fn without_console(mut value: serde_json::Value, perms: &[String]) -> serde_json::Value {
    if perms.iter().any(|one| one == "console.view") {
        return value;
    }
    if let Some(events) = value.get_mut("events").and_then(|one| one.as_array_mut()) {
        events.retain(|event| event.get("event").and_then(|one| one.as_str()) != Some(crate::logbatch::EVENT));
    }
    value
}

fn server_of(body: &serde_json::Value) -> Option<String> {
    body.get("id").and_then(|value| value.as_str()).map(str::to_string)
}

pub fn run(app: &AppHandle, command: &str, body: &serde_json::Value, who: Option<&Account>) -> Result<serde_json::Value, String> {
    if !READS.contains(&command) && !WRITES.contains(&command) && !ADMINS.contains(&command) {
        return Err("Not available from the web.".into());
    }
    let Some(who) = who else {
        return Err("Please sign in.".into());
    };
    if ADMINS.contains(&command) && !who.is_admin() {
        return Err("Admins only.".into());
    }
    let servers = if who.is_admin() { Vec::new() } else { crate::web_servers_of(app, command, body) };
    let perms = perms_for(app, who);
    if !permits(&perms, command) {
        return Err(if who.role == auth::VIEWER && WRITES.contains(&command) {
            "You can look at this server but not change it.".into()
        } else {
            "Your role doesn't allow this here.".into()
        });
    }
    if !who.is_admin() {
        // Every request about a server is checked, reads too; requests about no server must be harmless.
        if servers.is_empty() && !ANY_SERVER.contains(&command) {
            return Err("Admins only.".into());
        }
        for id in servers.iter() {
            if !who.may(id) {
                return Err("You don't have permission for this server.".into());
            }
        }
    }
    if WRITES.contains(&command) {
        if let Some(id) = server_of(body).or_else(|| servers.first().cloned()) {
            if !who.may(&id) {
                return Err("You don't have permission for this server.".into());
            }
        }
    }
    let mut value = crate::web_invoke(app, command, body)?;
    if !who.is_admin() {
        hide_other_servers(app, command, &mut value, who);
    }
    if !perms.iter().any(|one| one == "secrets") {
        hide_secrets(&mut value);
    }
    if WRITES.contains(&command) {
        if let Some(id) = servers.first().cloned().or_else(|| server_of(body)) {
            crate::web_audit(app, &id, &who.name, command);
            crate::note_writes(app, &id, command, body, &who.name);
        }
    }
    Ok(value)
}

/// A setting name that holds a secret: `rcon.password`, `management-server-secret`, `token`,
/// `discord.bot-token`, `api_key`, `password` in a plugin's config…
fn secret_name(name: &str) -> bool {
    let name = name.trim().trim_matches('"').trim_matches('\'').to_ascii_lowercase();
    ["password", "passwd", "secret", "token", "apikey", "api-key", "api_key", "private-key", "private_key", "webhook"]
        .iter()
        .any(|one| name.contains(one))
}

/// What a role without `secrets` (a Viewer) gets back: secret values in settings and in file text replaced by asterisks.
/// Lines like `key=value`, `key: value` and `"key": "value"` are covered, and `{key, value}` pairs.
fn hide_secrets(value: &mut serde_json::Value) {
    use std::sync::OnceLock;
    static LINE: OnceLock<regex::Regex> = OnceLock::new();
    let line = LINE.get_or_init(|| {
        regex::Regex::new(r#"(?m)^([ \t]*"?([A-Za-z0-9_.\-]+)"?[ \t]*[:=][ \t]*)(.+?)([ \t]*,?)\r?$"#).expect("secret line regex")
    });
    match value {
        serde_json::Value::String(text) => {
            if line.is_match(text) {
                let hidden = line.replace_all(text, |found: &regex::Captures| {
                    if secret_name(&found[2]) && !found[3].trim().is_empty() {
                        format!("{}********{}", &found[1], &found[4])
                    } else {
                        found[0].to_string()
                    }
                });
                *text = hidden.into_owned();
            }
        }
        serde_json::Value::Array(list) => list.iter_mut().for_each(hide_secrets),
        serde_json::Value::Object(map) => {
            let named = map.get("key").or_else(|| map.get("name")).and_then(|one| one.as_str()).is_some_and(secret_name);
            for (name, one) in map.iter_mut() {
                if (secret_name(name) || (named && name == "value")) && one.as_str().is_some_and(|text| !text.is_empty()) {
                    *one = serde_json::Value::String("********".into());
                } else {
                    hide_secrets(one);
                }
            }
        }
        _ => {}
    }
}

/// Commands that name no server and are safe for anyone signed in. Lists are trimmed by [`hide_other_servers`].
const ANY_SERVER: &[&str] = &[
    "list_instances",
    "get_app_settings",
    "app_version",
    "list_tasks",
    "list_core_versions",
    "list_core_builds",
    "system_memory_mib",
    "modrinth_project_detail",
    "lookup_mojang_profile",
    "item_names",
    "enchantment_names",
    "item_icons",
    "item_ids",
    "game_files",
    "lan_ip",
    "web_online",
    "list_java_runtimes",
    "playit_state",
    "bundled_forks",
    // NBT editor sessions are reached by an unguessable key that nbt_open (checked per server) hands out.
    "nbt_open_chunk",
    "nbt_act",
    "nbt_copy",
    "nbt_close",
    "stash_upload",
    "search_modpacks",
    "preview_mrpack",
    "preview_cfpack",
    "download_modpack",
    // A starting file for a new studio resource, made from its kind and name only.
    "studio_template",
    // A custom world generator's preview, made from the file's text only.
    "studio_worldgen_preview",
];

/// Leaves only the servers this account may open in answers that cover every server.
fn hide_other_servers(app: &AppHandle, command: &str, value: &mut serde_json::Value, who: &Account) {
    let keep = |item: &serde_json::Value, key: &str| item.get(key).and_then(|one| one.as_str()).is_some_and(|id| who.may(id));
    match command {
        "list_instances" => {
            if let Some(list) = value.as_array_mut() {
                list.retain(|item| keep(item, "id"));
            }
        }
        "list_tasks" => {
            if let Some(list) = value.as_array_mut() {
                list.retain(|item| keep(item, "serverId"));
            }
        }
        "web_online" => {
            if let Some(list) = value.as_array_mut() {
                list.retain(|item| item.get("server").and_then(|one| one.as_str()).is_some_and(|id| id.is_empty() || who.may(id)));
            }
        }
        "playit_state" => {
            // Tunnels are matched to servers by port; only show the ones for servers this account may open.
            let ports: Vec<u64> =
                crate::web_server_ports(app).into_iter().filter(|(id, _)| who.may(id)).map(|(_, port)| port as u64).collect();
            if let Some(list) = value.get_mut("tunnels").and_then(|one| one.as_array_mut()) {
                list.retain(|item| item.get("localPort").and_then(|one| one.as_u64()).is_some_and(|port| ports.contains(&port)));
            }
            if let Some(map) = value.as_object_mut() {
                map.insert("claim".into(), serde_json::Value::Null);
            }
        }
        "get_app_settings" => {
            if let Some(map) = value.as_object_mut() {
                map.insert("webAccounts".into(), serde_json::json!([]));
                map.insert("webInvites".into(), serde_json::json!([]));
                // Who else was let in through Monolink, and the open codes, are the host's business.
                map.insert("monolinkPeople".into(), serde_json::json!([]));
                map.insert("monolinkInvites".into(), serde_json::json!([]));
                map.insert("monolinkTeams".into(), serde_json::json!([]));
            }
        }
        _ => {}
    }
}

/// App-wide admin commands that touch only one server. A Monolink team Admin or Owner may run these
/// on the team's servers (section 7 of `docs/monolink.md`); nothing else app-wide ever opens.
pub const SERVER_ADMINS: &[&str] = &["restore_backup", "restore_backup_paths", "upgrade_instance", "rollback_last_upgrade"];
/// Only a team Owner: putting the server in the trash.
pub const SERVER_OWNER: &[&str] = &["trash_instance"];

/// Runs one of [`SERVER_ADMINS`] / [`SERVER_OWNER`] for someone who may change every server it names.
pub fn run_server_admin(app: &AppHandle, command: &str, body: &serde_json::Value, who: &Account) -> Result<serde_json::Value, String> {
    if !SERVER_ADMINS.contains(&command) && !SERVER_OWNER.contains(&command) && command != "save_server_agents_md" {
        return Err("Admins only.".into());
    }
    let servers = crate::web_servers_of(app, command, body);
    if servers.is_empty() || servers.iter().any(|id| !who.may(id)) {
        return Err("You don't have permission for this server.".into());
    }
    if !permits(&perms_for(app, who), command) {
        return Err("Your role doesn't allow this here.".into());
    }
    let value = crate::web_invoke(app, command, body)?;
    crate::web_audit(app, &servers[0], &who.name, command);
    Ok(value)
}

/// Keeps only events about `servers` in an `events_since` answer. Events that name no server are dropped,
/// so nothing about other servers on this PC leaks to a guest.
pub fn events_for(text: &str, servers: &[String]) -> serde_json::Value {
    let mut value: serde_json::Value = serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!({ "events": [], "at": 0 }));
    let ours = |item: &serde_json::Value| {
        item.get("serverId").and_then(|one| one.as_str()).is_some_and(|id| servers.iter().any(|mine| mine == id))
    };
    if let Some(events) = value.get_mut("events").and_then(|one| one.as_array_mut()) {
        events.retain_mut(|event| match event.get_mut("payload") {
            Some(serde_json::Value::Array(items)) => {
                items.retain(ours);
                !items.is_empty()
            }
            Some(payload) => ours(payload),
            None => false,
        });
    }
    value
}

pub fn wait_for_events(app: &AppHandle, after: u64, most: std::time::Duration) -> String {
    let began = std::time::Instant::now();
    loop {
        {
            let held = app.state::<crate::WebEvents>();
            let queue = held.lock().expect("web events poisoned");
            let newest = queue.last().map(|(at, _, _)| *at).unwrap_or(0);
            if newest != after {
                drop(queue);
                return events_since(app, after);
            }
        }
        if began.elapsed() >= most {
            return events_since(app, after);
        }
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
}

pub fn events_since(app: &AppHandle, after: u64) -> String {
    let held = app.state::<crate::WebEvents>();
    let queue = held.lock().expect("web events poisoned");
    let from = queue.iter().position(|(at, _, _)| *at > after).unwrap_or(queue.len());
    let events: Vec<serde_json::Value> =
        queue[from..].iter().map(|(_, event, payload)| serde_json::json!({ "event": event, "payload": payload })).collect();
    let at = queue.last().map(|(at, _, _)| *at).unwrap_or(after);
    serde_json::json!({ "events": events, "at": at }).to_string()
}

#[cfg(test)]
mod secret_tests {
    use serde_json::json;

    /// F1: teammates (and the web panel) can upload studio assets and rebuild the pack, through uploads only.
    #[test]
    fn studio_assets_open_to_teammates() {
        for read in ["studio_asset_list", "studio_asset_image", "studio_expansions"] {
            assert!(super::is_read(read), "{read}");
            assert_eq!(super::needs(read), &["studio.read"], "{read}");
        }
        for write in [
            "studio_asset_add_upload",
            "studio_asset_remove",
            "studio_pack_build",
            "studio_bundle_peek_upload",
            "studio_bundle_import_upload",
        ] {
            assert!(super::WRITES.contains(&write) && !super::is_read(write), "{write}");
            assert_eq!(super::needs(write), &["studio.write"], "{write}");
        }
    }

    /// F1: commands that take a path on the host (or raw base64, or the pack's sending setting) stay closed.
    #[test]
    fn bundle_paths_stay_closed() {
        for closed in ["studio_bundle_export", "studio_bundle_peek", "studio_bundle_import", "studio_asset_add", "studio_pack_sending"] {
            assert!(!super::is_read(closed) && !super::WRITES.contains(&closed), "{closed}");
        }
    }

    /// S8: guests may look at studio files and watch the live line; saving is a change (no Viewers), and nothing
    /// studio-related is admin-only.
    #[test]
    fn studio_is_open_to_guests() {
        for read in ["studio_overview", "studio_read", "studio_watch", "studio_unwatch"] {
            assert!(super::is_read(read), "{read}");
        }
        assert!(super::WRITES.contains(&"studio_save") && !super::is_read("studio_save"));
        assert!(super::ANY_SERVER.contains(&"studio_template"));
        assert!(!super::ADMINS.iter().any(|one| one.starts_with("studio_")));
    }

    fn perms(role: &str) -> Vec<String> {
        let custom = vec![super::auth::CustomRole {
            id: "role:console".into(),
            name: "Console".into(),
            perms: vec!["console.view".into(), "console.send".into()],
        }];
        super::auth::perms_of(role, &custom)
    }

    /// Every change needs a permission, so a role with nothing ticked can change nothing.
    #[test]
    fn every_change_needs_a_permission() {
        for one in super::WRITES.iter().chain(super::SERVER_ADMINS).chain(super::SERVER_OWNER).chain(["save_server_agents_md"].iter()) {
            assert!(!super::needs(one).is_empty(), "{one} needs no permission");
            assert!(super::needs(one).iter().all(|perm| super::auth::PERMS.contains(perm)), "{one}");
            assert!(!super::permits(&[], one), "{one}");
        }
    }

    /// Built-in roles act as before: Manager does everything on its servers, Viewer only looks.
    #[test]
    fn built_in_roles_keep_their_rights() {
        let manager = perms(super::auth::MANAGER);
        for one in super::WRITES.iter().chain(super::READS) {
            assert!(super::permits(&manager, one), "Manager lost {one}");
        }
        let viewer = perms(super::auth::VIEWER);
        for one in super::WRITES {
            assert!(!super::permits(&viewer, one), "Viewer may {one}");
        }
        for one in super::READS.iter().filter(|one| super::is_read(one) && **one != "read_server_file_download") {
            assert!(super::permits(&viewer, one), "Viewer lost {one}");
        }
        assert!(!super::permits(&viewer, "read_server_file_download"));
    }

    #[test]
    fn custom_roles_open_only_what_was_ticked() {
        let console = perms("role:console");
        assert!(super::permits(&console, "send_console_command") && super::permits(&console, "get_console_logs"));
        assert!(!super::permits(&console, "start_instance"));
        assert!(!super::permits(&console, "browse_server_files"));
        assert!(!super::permits(&console, "studio_read"));
        assert!(!super::permits(&console, "stash_upload"));
        assert!(super::permits(&console, "list_players"), "the overview needs nothing");
        assert!(super::permits(&perms("role:gone"), "list_instances"));
        assert!(!super::permits(&perms("role:gone"), "get_console_logs"), "a deleted role has nothing");
    }

    #[test]
    fn console_lines_stay_with_who_may_watch() {
        let events = json!({ "at": 3, "events": [
            { "event": "log:batch", "payload": [{ "serverId": "a", "line": "x" }] },
            { "event": "server-status", "payload": { "serverId": "a" } },
        ] });
        let kept = super::without_console(events.clone(), &perms("role:gone"));
        assert_eq!(kept["events"].as_array().unwrap().len(), 1);
        assert_eq!(super::without_console(events, &perms(super::auth::VIEWER))["events"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn viewers_dont_see_secrets() {
        let mut value = json!({
            "text": "motd=Hi\nrcon.password=hunter2\nenable-rcon=true\nmanagement-server-secret=abc\n",
            "yaml": "bot:\n  token: xyz\n  prefix: '!'\n",
            "props": [{ "key": "rcon.password", "value": "hunter2" }, { "key": "motd", "value": "Hi" }],
            "dbPassword": "p",
        });
        super::hide_secrets(&mut value);
        let text = value["text"].as_str().unwrap();
        assert!(text.contains("motd=Hi") && text.contains("rcon.password=********") && text.contains("management-server-secret=********"));
        assert!(text.contains("enable-rcon=true"));
        assert!(value["yaml"].as_str().unwrap().contains("token: ********"));
        assert!(value["yaml"].as_str().unwrap().contains("prefix: '!'"));
        assert_eq!(value["props"][0]["value"], "********");
        assert_eq!(value["props"][1]["value"], "Hi");
        assert_eq!(value["dbPassword"], "********");
    }
}

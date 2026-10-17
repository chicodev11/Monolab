
use super::auth::Account;
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
    "list_instances",
    "get_app_settings",
    "app_version",
    "get_servers_root",
    "list_trash_instances",
    "read_live",
    "read_loaded_chunks",
    "list_runs",
    "diagnose_lag",
    "addon_status",
    "read_server_properties",
    "list_worlds",
    "browse_server_files",
    "read_editable_file",
    "read_server_image",
    "read_texture_animation",
    "read_server_file_download",
    "preview_share_log",
    "list_backups",
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
    "suggest_server_name",
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
    "search_addon_catalog",
    "check_addon_updates",
    "enrich_addons",
    "get_server_icon",
    "list_core_builds",
    "list_java_processes",
    "list_pending_addons",
    "item_names",
    "item_icons",
    "playit_state",
];

const ADMINS: &[&str] = &[
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
    "set_app_settings",
    "list_web_accounts",
    "save_web_account",
    "delete_web_account",
    "download_java",
    "remove_java_runtime",
    "cancel_create_download",
];

const WRITES: &[&str] = &[
    "start_instance",
    "stop_instance",
    "restart_instance",
    "send_console_command",
    "save_server_properties",
    "save_editable_file",
    "rename_server_path",
    "trash_server_path",
    "restore_server_trash",
    "delete_server_trash",
    "empty_server_trash",
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
    "apply_addon_updates",
    "delete_addon",
    "delete_backup",
    "delete_bookmark",
    "force_stop_instance",
    "install_addon",
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
    "cancel_pending_addon",
    "playit_expose",
    "playit_unexpose",
];

fn server_of(body: &serde_json::Value) -> Option<String> {
    body.get("id")
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

pub fn run(
    app: &AppHandle,
    command: &str,
    body: &serde_json::Value,
    who: Option<&Account>,
) -> Result<serde_json::Value, String> {
    if !READS.contains(&command) && !WRITES.contains(&command) && !ADMINS.contains(&command) {
        return Err("Not available from the web.".into());
    }
    let Some(who) = who else {
        return Err("Please sign in.".into());
    };
    if ADMINS.contains(&command) && !who.is_admin() {
        return Err("Admins only.".into());
    }
    if WRITES.contains(&command) {
        if let Some(id) = server_of(body) {
            if !who.may(&id) {
                return Err("You don't have permission for this server.".into());
            }
        }
    }
    crate::web_invoke(app, command, body)
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
    let from = queue
        .iter()
        .position(|(at, _, _)| *at > after)
        .unwrap_or(queue.len());
    let events: Vec<serde_json::Value> = queue[from..]
        .iter()
        .map(|(_, event, payload)| serde_json::json!({ "event": event, "payload": payload }))
        .collect();
    let at = queue.last().map(|(at, _, _)| *at).unwrap_or(after);
    serde_json::json!({ "events": events, "at": at }).to_string()
}

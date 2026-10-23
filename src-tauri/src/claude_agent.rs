
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeInfo {
    pub path: String,
    pub workspace: String,
    pub history: bool,
    pub codex: String,
    pub codex_logged_in: bool,
    pub codex_email: String,
    pub codex_plan: String,
    pub codex_history: bool,
}

pub fn find_claude(app: &AppHandle) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["claude.exe", "claude.cmd"] } else { &["claude"] };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in names {
                let one = dir.join(name);
                if one.is_file() {
                    return Some(one);
                }
            }
        }
    }
    let home = app.path().home_dir().ok()?;
    let mut spots = vec![home.join(".local").join("bin").join(names[0])];
    if cfg!(windows) {
        if let Ok(roaming) = app.path().data_dir() {
            spots.push(roaming.join("npm").join("claude.cmd"));
        }
    }
    spots.into_iter().find(|one| one.is_file())
}

fn folder_name(name: &str, id: &str) -> String {
    let clean: String = name
        .chars()
        .map(|letter| if "<>:\"/\\|?*".contains(letter) || letter.is_control() { '_' } else { letter })
        .collect();
    let clean = clean.trim().trim_end_matches(['.', ' ']).to_string();
    let reserved = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "LPT1", "LPT2", "LPT3"];
    if clean.is_empty() || reserved.contains(&clean.to_ascii_uppercase().as_str()) {
        id.to_string()
    } else {
        clean
    }
}

pub(crate) fn history_dir(app: &AppHandle, dir: &Path) -> Option<PathBuf> {
    let shared = crate::claude_accounts::shared_root(app).ok()?;
    let slug: String = dir
        .to_string_lossy()
        .chars()
        .map(|letter| if letter.is_ascii_alphanumeric() { letter } else { '-' })
        .collect();
    Some(shared.join("projects").join(slug))
}

fn move_agent_folder(app: &AppHandle, from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::rename(from, to).map_err(|error| format!("Could not move the agent folder: {error}"))?;
    if let (Some(old), Some(new)) = (history_dir(app, from), history_dir(app, to)) {
        if old.is_dir() && !new.exists() {
            let _ = std::fs::rename(old, new);
        }
    }
    Ok(())
}

fn folder_map_file(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|error| error.to_string())?.join("agent-folders.json"))
}

pub(crate) fn workspace(app: &AppHandle, server: &str) -> Result<PathBuf, String> {
    let instance = crate::instance_by_id(app, server)?;
    let root = app
        .path()
        .document_dir()
        .map_err(|error| format!("Could not find the Documents folder: {error}"))?
        .join("Monolab");
    let map_file = folder_map_file(app)?;
    let mut map: HashMap<String, String> = std::fs::read_to_string(&map_file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let base = folder_name(&instance.name, server);
    let taken = |candidate: &Path, map: &HashMap<String, String>| {
        candidate.exists() || map.iter().any(|(id, path)| id != server && Path::new(path) == candidate)
    };
    let fresh_spot = |map: &HashMap<String, String>| {
        let mut spot = root.join(&base);
        let mut n = 2;
        while taken(&spot, map) {
            spot = root.join(format!("{base} ({n})"));
            n += 1;
        }
        spot
    };

    let dir = match map.get(server).map(PathBuf::from) {
        Some(known) if known.is_dir() => {
            let wanted = root.join(&base);
            let named = known.file_name().map(|one| one.to_string_lossy().into_owned()).unwrap_or_default();
            let already = named == base || named.starts_with(&format!("{base} ("));
            if !already && known.parent() == Some(root.as_path()) && !taken(&wanted, &map) {
                move_agent_folder(app, &known, &wanted)?;
                wanted
            } else {
                known
            }
        }
        _ => {
            let spot = fresh_spot(&map);
            let old = crate::instance_data_dir(app, server)?.join("claude");
            if old.is_dir() {
                move_agent_folder(app, &old, &spot)?;
            }
            spot
        }
    };
    std::fs::create_dir_all(&dir).map_err(|error| format!("Could not create the agent folder: {error}"))?;
    if map.get(server).map(String::as_str) != Some(&*dir.to_string_lossy()) {
        map.insert(server.to_string(), dir.to_string_lossy().into_owned());
        if let Ok(text) = serde_json::to_string_pretty(&map) {
            let _ = std::fs::write(&map_file, text);
        }
    }
    write_memos(&dir);
    let link = dir.join("Server folder.url");
    if instance.remote_host.is_empty() && instance.panel_host.is_empty() && !link.exists() {
        let target = instance.path.replace('\\', "/");
        let _ = std::fs::write(&link, format!("[InternetShortcut]\r\nURL=file:///{target}\r\n"));
    }
    Ok(dir)
}

pub fn open_folder(app: &AppHandle, server: &str) -> Result<(), String> {
    let dir = workspace(app, server)?;
    let program = if cfg!(windows) { "explorer" } else { "xdg-open" };
    std::process::Command::new(program)
        .arg(&dir)
        .spawn()
        .map_err(|error| format!("Could not open Explorer: {error}"))?;
    Ok(())
}

const CLAUDE_POINTER: &str = "@AGENTS.md
";

/// AGENTS.md is the one memory both agents read (Codex natively, Claude through the import in CLAUDE.md).
fn write_memos(dir: &Path) {
    let agents = dir.join("AGENTS.md");
    let claude = dir.join("CLAUDE.md");
    if !agents.exists() {
        let old = std::fs::read_to_string(&claude).ok().filter(|text| text.trim() != CLAUDE_POINTER.trim());
        let _ = std::fs::write(&agents, old.as_deref().unwrap_or(MEMO));
    }
    if std::fs::read_to_string(&claude).map(|text| text.trim() != CLAUDE_POINTER.trim()).unwrap_or(true) && agents.exists() {
        let _ = std::fs::write(&claude, CLAUDE_POINTER);
    }
}

const MEMO: &str = "# Notes for this server

This is the workspace Monolab keeps for one server. The real server files live elsewhere (for a local server, see the `Server folder` shortcut).
Agents (Claude or Codex) keep what they learn about this server here: what they built or installed, your rules and preferences, and unfinished work. You can edit it too.
Keep the source of custom plugins and mods in project folders here; only the built jar goes on the server.

## Rules and preferences

## Built

## Unfinished
";

pub(crate) fn has_history(app: &AppHandle, dir: &Path) -> bool {
    let Some(place) = history_dir(app, dir) else { return false };
    std::fs::read_dir(place)
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
        })
        .unwrap_or(false)
}

pub(crate) fn mcp_config(app: &AppHandle, server: &str) -> Result<PathBuf, String> {
    let (url, token) = crate::mcp::ensure_running(app)?;
    let config = serde_json::json!({
        "mcpServers": {
            "monolab": {
                "type": "http",
                "url": format!("{url}?server={server}"),
                "headers": { "Authorization": format!("Bearer {token}") },
            }
        }
    });
    let dir = app.path().app_data_dir().map_err(|error| error.to_string())?;
    let safe: String = server.chars().map(|letter| if letter.is_ascii_alphanumeric() { letter } else { '_' }).collect();
    let file = dir.join(format!("claude-mcp-{safe}.json"));
    std::fs::write(&file, config.to_string()).map_err(|error| error.to_string())?;
    Ok(file)
}

/// Shell deletes go through delete_file instead.
pub(crate) const DENIED_DELETES: [&str; 6] =
    ["--disallowedTools", "Bash(rm:*)", "Bash(rmdir:*)", "Bash(del:*)", "Bash(rd:*)", "NotebookEdit"];

pub(crate) fn briefing(instance: &crate::ServerInstance, dir: &Path) -> String {
    let place = if !instance.panel_host.is_empty() {
        "a Hydrodactyl panel server (files live on the panel, not on this computer)".to_string()
    } else if !instance.remote_host.is_empty() {
        format!("an SSH home server (files at {} on that machine, not on this computer)", instance.path)
    } else {
        format!("a local server on this computer (folder {})", instance.path)
    };
    format!(
        "You manage ONE Minecraft server inside the Monolab app: \"{name}\" (id {id}), {core} {version}, {place}. \
The `monolab` MCP tools act only on it; reach its files only through them, never by guessing paths. \
Never delete with shell commands; delete_file moves things to a restorable trash. \
Your working folder {work} is a workspace, not the server files: build plugins/mods there as Gradle projects in ASCII-named \
subfolders, then add_addon_file the jar and restart_server (ask first). start/restart_server already wait and summarize, so don't poll \
read_console or search_logs after. Save usage: keep replies concise, don't re-read files, search logs for specific text, use queries, \
run Gradle with -q and pipe long output through tail, and do small lookups yourself instead of spawning subagents. Suggest create_backup before risky changes. \
Keep lasting notes (what you built, the user's rules, unfinished work) in AGENTS.md. The user is not a developer: reply in Korean \
and say briefly what you'll do first.",
        name = instance.name,
        id = instance.id,
        core = instance.core,
        version = instance.version,
        work = dir.display(),
    )
}

pub fn info(app: &AppHandle, server: &str) -> Result<ClaudeInfo, String> {
    let dir = workspace(app, server)?;
    Ok(ClaudeInfo {
        path: find_claude(app).map(|one| one.to_string_lossy().into_owned()).unwrap_or_default(),
        workspace: dir.to_string_lossy().into_owned(),
        history: has_history(app, &dir),
        codex: crate::codex_chat::find_codex(app).map(|one| one.to_string_lossy().into_owned()).unwrap_or_default(),
        codex_logged_in: crate::codex_chat::logged_in(app),
        codex_email: crate::codex_chat::account(app).0,
        codex_plan: crate::codex_chat::account(app).1,
        codex_history: crate::codex_chat::has_history(app, server),
    })
}

pub fn attach(app: &AppHandle, server: &str, name: &str, bytes: &[u8]) -> Result<String, String> {
    const LIMIT: usize = 64 * 1024 * 1024;
    if bytes.len() > LIMIT {
        return Err(format!("{name} is too large (64 MB max)."));
    }
    let clean: String = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("file")
        .chars()
        .map(|letter| if "<>:\"|?*".contains(letter) || letter.is_control() { '_' } else { letter })
        .collect();
    let clean = clean.trim().trim_matches('.').to_string();
    let clean = if clean.is_empty() { "file".to_string() } else { clean };
    let folder = workspace(app, server)?.join("attachments");
    std::fs::create_dir_all(&folder).map_err(|error| format!("Could not create the attachments folder: {error}"))?;
    let (stem, ext) = match clean.rfind('.') {
        Some(at) if at > 0 => (clean[..at].to_string(), clean[at..].to_string()),
        _ => (clean.clone(), String::new()),
    };
    let mut pick = clean.clone();
    let mut number = 2;
    while folder.join(&pick).exists() {
        pick = format!("{stem} ({number}){ext}");
        number += 1;
    }
    std::fs::write(folder.join(&pick), bytes).map_err(|error| format!("Could not attach {name}: {error}"))?;
    Ok(format!("attachments/{pick}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn makes_a_folder_name_from_a_server_name() {
        assert_eq!(super::folder_name("Server 1", "server_1"), "Server 1");
        assert_eq!(super::folder_name("a/b:c?", "x"), "a_b_c_");
        assert_eq!(super::folder_name("  endpoint.. ", "x"), "endpoint");
        assert_eq!(super::folder_name("con", "server_9"), "server_9");
        assert_eq!(super::folder_name("", "server_9"), "server_9");
    }
}

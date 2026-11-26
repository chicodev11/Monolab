//! Keeps the app's data folder readable: save backups (`*.bak`) live in `backups/`, the agent's
//! per-server MCP files in `mcp/`, the crash log in `logs/`. Run once at start; files left over from
//! older versions (and MCP files of servers that are gone) move to `old/`, which is safe to delete.

use std::fs;
use std::path::Path;

/// Bump to move things again after the layout changes.
const DONE: &str = ".tidy-1";

/// The name `claude_agent::mcp_config` gives a server's file.
pub fn mcp_name(server: &str) -> String {
    let safe: String = server.chars().map(|letter| if letter.is_ascii_alphanumeric() { letter } else { '_' }).collect();
    format!("claude-mcp-{safe}.json")
}

fn leftover(name: &str) -> bool {
    name.ends_with(".maptest-backup")
        || name.ends_with(".before-roles")
        || name == "stdout-sync.rb"
        || name.contains(".broken-")
        || name.ends_with(".tmp")
}

fn move_into(file: &Path, folder: &Path) {
    if fs::create_dir_all(folder).is_err() {
        return;
    }
    let Some(name) = file.file_name() else { return };
    let mut target = folder.join(name);
    let mut count = 2;
    while target.exists() {
        target = folder.join(format!("{}-{count}", name.to_string_lossy()));
        count += 1;
    }
    let _ = fs::rename(file, target);
}

/// Sorts the loose files in `root`. `servers` are the ids of the servers in the list.
pub fn tidy(root: &Path, servers: &[String]) {
    if root.join(DONE).exists() {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else { return };
    let live: Vec<String> = servers.iter().map(|one| mcp_name(one)).collect();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("claude-mcp-") && name.ends_with(".json") {
            move_into(&path, &root.join(if live.contains(&name) { "mcp" } else { "old" }));
        } else if name.ends_with(".bak") {
            move_into(&path, &root.join("backups"));
        } else if name == "crash-log.txt" {
            move_into(&path, &root.join("logs"));
        } else if leftover(&name) {
            move_into(&path, &root.join("old"));
        }
    }
    if root.join("old").is_dir() {
        let _ = fs::write(
            root.join("old").join("README.txt"),
            "Files Monolab no longer uses, kept in case you want them. You can delete this folder.\n",
        );
    }
    let _ = fs::write(root.join(DONE), "");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_the_loose_files_once() {
        let root = std::env::temp_dir().join(format!("monolab-tidy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for name in [
            "instances.json",
            "instances.json.bak",
            "claude-mcp-a1.json",
            "claude-mcp-gone.json",
            "crash-log.txt",
            "settings.json.before-roles",
            "stdout-sync.rb",
        ] {
            fs::write(root.join(name), "x").unwrap();
        }
        tidy(&root, &["a1".to_string()]);
        assert!(root.join("instances.json").exists());
        assert!(root.join("backups/instances.json.bak").exists());
        assert!(root.join("mcp/claude-mcp-a1.json").exists());
        assert!(root.join("old/claude-mcp-gone.json").exists());
        assert!(root.join("logs/crash-log.txt").exists());
        assert!(root.join("old/settings.json.before-roles").exists());
        assert!(root.join("old/stdout-sync.rb").exists());
        // Second run does nothing, even with a new loose file.
        fs::write(root.join("claude-mcp-later.json"), "x").unwrap();
        tidy(&root, &[]);
        assert!(root.join("claude-mcp-later.json").exists());
        let _ = fs::remove_dir_all(&root);
    }
}

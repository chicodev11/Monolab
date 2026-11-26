use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// A Claude Code skill in a server's agent workspace: `.claude/skills/<name>/SKILL.md` (turned off: `.claude/skills-off`).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    pub name: String,
    pub description: String,
    pub body: String,
    pub enabled: bool,
    pub updated: f64,
}

const BODY_LIMIT: usize = 256 * 1024;

fn folders(app: &AppHandle, server: &str) -> Result<(PathBuf, PathBuf), String> {
    let base = crate::claude_agent::workspace(app, server)?.join(".claude");
    Ok((base.join("skills"), base.join("skills-off")))
}

pub fn check_name(name: &str) -> Result<(), String> {
    let fine = !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == '-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if fine {
        Ok(())
    } else {
        Err("A skill name uses only lowercase letters, numbers and hyphens (64 at most).".into())
    }
}

/// Splits SKILL.md into its front matter lines and the body.
fn split(text: &str) -> (Vec<String>, String) {
    let text = text.trim_start_matches('\u{feff}');
    let normal = text.replace("\r\n", "\n");
    if let Some(rest) = normal.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let head = rest[..end].lines().map(str::to_string).collect();
            let body = rest[end + 4..].trim_start_matches(['-']).trim_start_matches('\n').to_string();
            return (head, body);
        }
    }
    (Vec::new(), normal)
}

fn field(head: &[String], key: &str) -> String {
    head.iter()
        .find_map(|line| line.strip_prefix(&format!("{key}:")))
        .map(|value| {
            let value = value.trim();
            if value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'') {
                value[1..value.len() - 1].replace("''", "'")
            } else if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
                value[1..value.len() - 1].to_string()
            } else {
                value.to_string()
            }
        })
        .unwrap_or_default()
}

fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// SKILL.md text with the given name and description; other front matter keys (allowed-tools…) are kept.
pub fn compose(old: &str, name: &str, description: &str, body: &str) -> String {
    let (head, _) = split(old);
    let mut lines = vec![format!("name: {name}"), format!("description: {}", quoted(description.trim()))];
    lines.extend(head.into_iter().filter(|line| !line.starts_with("name:") && !line.starts_with("description:")));
    format!("---\n{}\n---\n\n{}\n", lines.join("\n"), body.trim_end())
}

fn read_one(dir: &Path, enabled: bool) -> Option<AgentSkill> {
    let file = dir.join("SKILL.md");
    let text = fs::read_to_string(&file).ok()?;
    let (head, body) = split(&text);
    let folder = dir.file_name()?.to_string_lossy().into_owned();
    let name = Some(field(&head, "name")).filter(|one| !one.is_empty()).unwrap_or(folder);
    let updated = file
        .metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|at| at.as_millis() as f64)
        .unwrap_or(0.0);
    Some(AgentSkill { name, description: field(&head, "description"), body, enabled, updated })
}

pub fn list(app: &AppHandle, server: &str) -> Result<Vec<AgentSkill>, String> {
    let (on, off) = folders(app, server)?;
    let mut out = Vec::new();
    for (place, enabled) in [(on, true), (off, false)] {
        let Ok(entries) = fs::read_dir(place) else { continue };
        for entry in entries.flatten() {
            if entry.path().is_dir() && check_name(&entry.file_name().to_string_lossy()).is_ok() {
                out.extend(read_one(&entry.path(), enabled));
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Where a skill lives now, on or off.
fn locate(app: &AppHandle, server: &str, name: &str) -> Result<Option<(PathBuf, bool)>, String> {
    check_name(name)?;
    let (on, off) = folders(app, server)?;
    Ok([(on.join(name), true), (off.join(name), false)].into_iter().find(|(dir, _)| dir.is_dir()))
}

pub fn save(app: &AppHandle, server: &str, old: &str, name: &str, description: &str, body: &str) -> Result<(), String> {
    check_name(name)?;
    if description.trim().is_empty() {
        return Err("Add a short description so the agent knows when to use this skill.".into());
    }
    if body.len() > BODY_LIMIT {
        return Err("This skill is too long (256 KB max).".into());
    }
    let (on, _) = folders(app, server)?;
    let current = if old.is_empty() { None } else { locate(app, server, old)? };
    if old != name && locate(app, server, name)?.is_some() {
        return Err(format!("A skill named {name} already exists."));
    }
    let dir = match &current {
        Some((dir, _)) if old != name => {
            let next = dir.with_file_name(name);
            fs::rename(dir, &next).map_err(|error| format!("Could not rename the skill: {error}"))?;
            next
        }
        Some((dir, _)) => dir.clone(),
        None => on.join(name),
    };
    fs::create_dir_all(&dir).map_err(|error| format!("Could not save the skill: {error}"))?;
    let file = dir.join("SKILL.md");
    let previous = fs::read_to_string(&file).unwrap_or_default();
    fs::write(&file, compose(&previous, name, description, body)).map_err(|error| format!("Could not save the skill: {error}"))
}

pub fn set_enabled(app: &AppHandle, server: &str, name: &str, enabled: bool) -> Result<(), String> {
    let Some((dir, now)) = locate(app, server, name)? else { return Err(format!("There is no skill named {name}.")) };
    if now == enabled {
        return Ok(());
    }
    let (on, off) = folders(app, server)?;
    let target = if enabled { on } else { off }.join(name);
    if target.exists() {
        return Err(format!("A skill named {name} already exists."));
    }
    crate::move_tree(&dir, &target).map_err(|error| format!("Could not move the skill: {error}"))
}

/// Moves the skill into the server's app data, so it can still be found by hand.
pub fn delete(app: &AppHandle, server: &str, name: &str) -> Result<(), String> {
    let Some((dir, _)) = locate(app, server, name)? else { return Ok(()) };
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|one| one.as_millis()).unwrap_or(0);
    let target = crate::instance_data_dir(app, server)?.join("trash-skills").join(format!("{name}-{stamp}"));
    crate::move_tree(&dir, &target).map_err(|error| format!("Could not delete the skill: {error}"))
}

pub fn copy(app: &AppHandle, server: &str, name: &str, to: &str) -> Result<(), String> {
    let Some((dir, enabled)) = locate(app, server, name)? else { return Err(format!("There is no skill named {name}.")) };
    crate::instance_by_id(app, to)?;
    if locate(app, to, name)?.is_some() {
        return Err(format!("That server already has a skill named {name}."));
    }
    let (on, off) = folders(app, to)?;
    crate::copy_tree_counting(&dir, &if enabled { on } else { off }.join(name), &|| {}).map(|_| ())
}

/// One line per turned-on skill, for an agent that doesn't read skill folders itself (Codex).
pub fn summary(app: &AppHandle, server: &str) -> String {
    let lines: Vec<String> = list(app, server)
        .unwrap_or_default()
        .into_iter()
        .filter(|one| one.enabled)
        .map(|one| format!("- {}: {} (.claude/skills/{}/SKILL.md)", one.name, one.description, one.name))
        .collect();
    if lines.is_empty() {
        String::new()
    } else {
        format!("\n\nSkills in this workspace; read the SKILL.md before a task one fits:\n{}", lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_extra_front_matter_and_reads_it_back() {
        let old = "---\nname: old\ndescription: x\nallowed-tools: Bash\n---\n\nold body\n";
        let text = super::compose(old, "build-plugin", "Build it: with 'quotes'", "Step 1\n");
        assert!(text.contains("allowed-tools: Bash"));
        let (head, body) = super::split(&text);
        assert_eq!(super::field(&head, "name"), "build-plugin");
        assert_eq!(super::field(&head, "description"), "Build it: with 'quotes'");
        assert_eq!(body.trim(), "Step 1");
    }

    #[test]
    fn checks_names() {
        assert!(super::check_name("paper-build-2").is_ok());
        for bad in ["", "Bad", "a b", "-a", "a-", "../x", "\u{d55c}"] {
            assert!(super::check_name(bad).is_err(), "{bad}");
        }
    }
}

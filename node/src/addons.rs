//! Plugins and mods in a server's `plugins/` and `mods/`, in the app's shapes (`scan_addons`,
//! `toggle_addon`, `delete_addon`, `add_addon_upload`). Turning one off renames `x.jar` to
//! `x.jar.disabled`; deleting moves it to the server's `.monolab-trash/addons/`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AddonEntry {
    pub file_name: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub metadata: String,
    pub compatibility: String,
    pub version: String,
    pub icon: Option<String>,
}

fn folder_of(kind: &str) -> &'static str {
    if kind == "mod" { "mods" } else { "plugins" }
}

fn is_addon(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".jar") || lower.ends_with(".jar.disabled")
}

pub fn scan(root: &Path) -> Vec<AddonEntry> {
    let mut out = Vec::new();
    for kind in ["plugin", "mod"] {
        let Ok(list) = fs::read_dir(root.join(folder_of(kind))) else { continue };
        let mut found: Vec<AddonEntry> = list
            .flatten()
            .filter(|one| one.path().is_file())
            .filter_map(|one| one.file_name().to_str().map(str::to_string))
            .filter(|name| is_addon(name))
            .map(|name| AddonEntry {
                enabled: !name.ends_with(".disabled"),
                name: name.trim_end_matches(".disabled").trim_end_matches(".jar").to_string(),
                file_name: name,
                kind: kind.into(),
                metadata: String::new(),
                compatibility: String::new(),
                version: String::new(),
                icon: None,
            })
            .collect();
        found.sort_by_key(|one| one.name.to_lowercase());
        out.extend(found);
    }
    out
}

/// An add-on file of this server, by its name in `plugins/` or `mods/` (links that lead out are refused).
fn addon_path(root: &Path, kind: &str, file_name: &str) -> Result<PathBuf, String> {
    let name = crate::transfer::plain_name(file_name)?;
    if name != file_name || !is_addon(&name) {
        return Err("Unknown plugin file.".into());
    }
    let path = crate::files::resolve_in_server(root, &format!("{}/{name}", folder_of(kind))).map_err(|error| match error {
        crate::files::PathError::Missing => "That plugin file isn't there any more.".to_string(),
        other => other.to_string(),
    })?;
    if !path.is_file() {
        return Err("Unknown plugin file.".into());
    }
    Ok(path)
}

/// Turns an add-on on or off; gives its new file name.
pub fn toggle(root: &Path, kind: &str, file_name: &str) -> Result<String, String> {
    let path = addon_path(root, kind, file_name)?;
    let to = if let Some(plain) = file_name.strip_suffix(".disabled") { plain.to_string() } else { format!("{file_name}.disabled") };
    let target = path.with_file_name(&to);
    if target.exists() {
        return Err(format!("{to} is already there."));
    }
    fs::rename(&path, &target).map_err(|error| format!("Could not rename the file: {error}"))?;
    Ok(to)
}

/// Moves an add-on to `.monolab-trash/addons/<time>-<name>` (the file browser hides that folder).
pub fn delete(root: &Path, kind: &str, file_name: &str) -> Result<(), String> {
    let path = addon_path(root, kind, file_name)?;
    let bin = root.join(".monolab-trash").join("addons");
    fs::create_dir_all(&bin).map_err(|error| error.to_string())?;
    crate::transfer::move_file(&path, &bin.join(format!("{}-{file_name}", crate::serve::unix_now()))).map_err(|error| format!("Could not move the file: {error}"))
}

/// `add_addon_upload` `{id, files:[{name,data}]}`: base64 files into `plugins/` (or `mods/` on a
/// server that has only that). A file with the same name is replaced; the old one goes to the trash.
/// `{added, replaced, queued}`, as the app answers (nothing is queued: a node writes straight away,
/// and the server loads it when it starts next).
pub fn add(root: &Path, files: &[Value], running: bool) -> Result<Value, String> {
    let kind = if !root.join("plugins").is_dir() && root.join("mods").is_dir() { "mod" } else { "plugin" };
    let folder = root.join(folder_of(kind));
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    if crate::files::resolve_in_server(root, folder_of(kind)).is_err() {
        return Err("Files outside the server folder cannot be accessed.".into());
    }
    let mut decoded = Vec::new();
    for one in files {
        let name = crate::transfer::plain_name(one.get("name").and_then(Value::as_str).unwrap_or_default())?;
        if !name.to_ascii_lowercase().ends_with(".jar") {
            return Err(format!("{name} isn't a .jar file."));
        }
        // A running server has the old jar open; swapping it under the server breaks it.
        if running && folder.join(&name).is_file() {
            return Err(format!("Stop the server before replacing {name}."));
        }
        let bytes = crate::transfer::decode(one.get("data").and_then(Value::as_str).unwrap_or_default()).ok_or_else(|| format!("{name} was damaged on the way."))?;
        decoded.push((name, bytes));
    }
    let (mut added, mut replaced) = (Vec::new(), Vec::new());
    for (name, bytes) in decoded {
        if folder.join(&name).is_file() {
            delete(root, kind, &name)?;
            replaced.push(name.clone());
        } else {
            added.push(name.clone());
        }
        fs::write(folder.join(&name), bytes).map_err(|error| format!("Could not write {name}: {error}"))?;
    }
    Ok(serde_json::json!({ "added": added, "replaced": replaced, "queued": [] }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugins_on_off_add_and_delete() {
        let root = crate::files::tests::sandbox("addons");
        fs::create_dir_all(root.join("plugins/Essentials")).unwrap();
        fs::write(root.join("plugins/Essentials.jar"), "e").unwrap();
        fs::write(root.join("plugins/Old.jar.disabled"), "o").unwrap();
        fs::write(root.join("plugins/notes.txt"), "n").unwrap();
        let list = scan(&root);
        assert_eq!(list.iter().map(|one| (one.name.as_str(), one.enabled)).collect::<Vec<_>>(), [("Essentials", true), ("Old", false)]);

        assert_eq!(toggle(&root, "plugin", "Essentials.jar").unwrap(), "Essentials.jar.disabled");
        assert_eq!(toggle(&root, "plugin", "Old.jar.disabled").unwrap(), "Old.jar");
        assert!(toggle(&root, "plugin", "../server.properties").is_err());
        assert!(toggle(&root, "plugin", "notes.txt").is_err());

        let files = [serde_json::json!({ "name": "New.jar", "data": crate::transfer::encode(b"n") }), serde_json::json!({ "name": "Old.jar", "data": crate::transfer::encode(b"o2") })];
        assert!(add(&root, &files, true).unwrap_err().contains("Stop the server"));
        assert!(!root.join("plugins/New.jar").exists(), "nothing is written when one is refused");
        let added = add(&root, &files, false).unwrap();
        assert_eq!(added, serde_json::json!({ "added": ["New.jar"], "replaced": ["Old.jar"], "queued": [] }));
        assert_eq!(fs::read_to_string(root.join("plugins/Old.jar")).unwrap(), "o2");
        assert!(add(&root, &[serde_json::json!({ "name": "x.sh", "data": "" })], false).is_err());

        delete(&root, "plugin", "New.jar").unwrap();
        assert!(!root.join("plugins/New.jar").exists());
        assert_eq!(fs::read_dir(root.join(".monolab-trash/addons")).unwrap().count(), 2);
        fs::remove_dir_all(&root).unwrap();
    }
}

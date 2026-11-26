
use std::collections::HashMap;

use crate::panel::{split_path, Api, Target};
use crate::remote::{plain_trash_name, FileRow, TRASH_NOTE};

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|one| one.as_secs())
        .unwrap_or(0)
}

pub async fn list_dir(api: &Api, target: &Target, relative: &str) -> Result<Vec<FileRow>, String> {
    api.list(target, relative).await
}

pub async fn move_path(api: &Api, target: &Target, from: &str, to: &str) -> Result<(), String> {
    if api.exists(target, to).await?.is_some() {
        return Err("That name already exists.".into());
    }
    api.rename(target, from, to).await
}

async fn read_trash_notes(api: &Api, target: &Target) -> Option<HashMap<String, String>> {
    let at = format!(".monolab-trash/{TRASH_NOTE}");
    match api.read_text(target, &at).await {
        Ok(text) => serde_json::from_str(&text).ok(),
        Err(error) if crate::remote::is_missing(&error) => Some(HashMap::new()),
        Err(_) => None,
    }
}

async fn edit_trash_notes(api: &Api, target: &Target, change: impl FnOnce(&mut HashMap<String, String>)) {
    if let Some(mut note) = read_trash_notes(api, target).await {
        change(&mut note);
        write_trash_notes(api, target, &note).await;
    }
}

async fn write_trash_notes(api: &Api, target: &Target, note: &HashMap<String, String>) {
    if let Ok(text) = serde_json::to_string_pretty(note) {
        let _ = api
            .write_text(target, &format!(".monolab-trash/{TRASH_NOTE}"), &text)
            .await;
    }
}

pub async fn trash_path(api: &Api, target: &Target, relative: &str) -> Result<(), String> {
    let (_, name) = split_path(relative)?;
    if api.exists(target, ".monolab-trash").await?.is_none() {
        api.make_dir(target, "", ".monolab-trash").await?;
    }
    let stored = format!("{}-{name}", unix_secs());
    api.rename(target, relative, &format!(".monolab-trash/{stored}")).await?;
    let from = relative.trim_matches('/').to_string();
    edit_trash_notes(api, target, |note| {
        note.insert(stored, from);
    })
    .await;
    Ok(())
}

pub async fn trash_list(api: &Api, target: &Target) -> Result<Vec<(FileRow, String)>, String> {
    let rows = match api.exists(target, ".monolab-trash").await? {
        Some(true) => api.list(target, ".monolab-trash").await?,
        _ => return Ok(Vec::new()),
    };
    let note = read_trash_notes(api, target).await.unwrap_or_default();
    Ok(rows
        .into_iter()
        .filter(|one| one.name != TRASH_NOTE)
        .map(|one| {
            let came = note.get(&one.name).cloned().unwrap_or_else(|| plain_trash_name(&one.name));
            (one, came)
        })
        .collect())
}

pub async fn trash_restore(api: &Api, target: &Target, stored: &str) -> Result<String, String> {
    crate::paths::check_relative(stored)?;
    let note = read_trash_notes(api, target)
        .await
        .ok_or("Could not read the trash list. Please try again shortly.")?;
    let back = note.get(stored).cloned().unwrap_or_else(|| plain_trash_name(stored));
    if api.exists(target, &back).await?.is_some() {
        return Err(format!("Something is already at {back}. Move it out of the way first."));
    }
    api.rename(target, &format!(".monolab-trash/{stored}"), &back).await?;
    edit_trash_notes(api, target, |note| {
        note.remove(stored);
    })
    .await;
    Ok(back)
}

pub async fn trash_delete(api: &Api, target: &Target, stored: &str) -> Result<(), String> {
    crate::paths::check_relative(stored)?;
    api.delete(target, &[format!(".monolab-trash/{stored}")]).await?;
    edit_trash_notes(api, target, |note| {
        note.remove(stored);
    })
    .await;
    Ok(())
}

pub async fn trash_empty(api: &Api, target: &Target) -> Result<u32, String> {
    let rows = trash_list(api, target).await?;
    let count = rows.len() as u32;
    let names: Vec<String> = rows
        .iter()
        .map(|(one, _)| format!(".monolab-trash/{}", one.name))
        .chain(std::iter::once(format!(".monolab-trash/{TRASH_NOTE}")))
        .collect();
    if count > 0 {
        api.delete(target, &names).await?;
    }
    Ok(count)
}

pub async fn make_dir(api: &Api, target: &Target, relative: &str) -> Result<(), String> {
    if api.exists(target, relative).await?.is_some() {
        return Err("That name already exists.".into());
    }
    let (dir, name) = split_path(relative)?;
    api.make_dir(target, &dir, &name).await
}

pub async fn log_files(api: &Api, target: &Target) -> Vec<FileRow> {
    let mut rows = api.list(target, "logs").await.unwrap_or_default();
    rows.retain(|one| !one.directory && (one.name.ends_with(".log") || one.name.ends_with(".log.gz")));
    rows.sort_by_key(|one| std::cmp::Reverse(one.modified_at));
    rows
}

pub async fn read_log(api: &Api, target: &Target, name: &str, lines: u32) -> Result<String, String> {
    crate::paths::check_relative(name)?;
    let at = format!("logs/{name}");
    let text = if name.ends_with(".gz") {
        use std::io::Read;
        let bytes = api.read_bytes(target, &at, 64 * 1024 * 1024).await?;
        let mut out = String::new();
        flate2::read::GzDecoder::new(&bytes[..])
            .read_to_string(&mut out)
            .map_err(|error| format!("Could not extract {name}: {error}"))?;
        out
    } else {
        match api.read_text(target, &at).await {
            Ok(text) => text,
            Err(_) => String::from_utf8_lossy(&api.read_bytes(target, &at, 64 * 1024 * 1024).await?).into_owned(),
        }
    };
    let all: Vec<&str> = text.lines().collect();
    let from = all.len().saturating_sub(lines as usize);
    Ok(all[from..].join("\n"))
}

pub async fn addon_files(api: &Api, target: &Target, folder: &str) -> Vec<FileRow> {
    let mut rows = api.list(target, folder).await.unwrap_or_default();
    rows.retain(|one| !one.directory && (one.name.ends_with(".jar") || one.name.ends_with(".jar.disabled")));
    rows
}

pub async fn worlds(api: &Api, target: &Target) -> Vec<(String, u64)> {
    let rows = api.list(target, "").await.unwrap_or_default();
    let mut out = Vec::new();
    for one in rows.iter().filter(|one| one.directory) {
        let looks = one.name.ends_with("_nether") || one.name.ends_with("_the_end");
        let has_level = looks
            || api
                .list(target, &one.name)
                .await
                .map(|inside| inside.iter().any(|file| file.name == "level.dat"))
                .unwrap_or(false);
        if has_level {
            out.push((one.name.clone(), one.modified_at));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::panel::{panel_path, split_path};

    #[test]
    fn turns_file_tab_paths_into_panel_paths() {
        assert_eq!(panel_path("").unwrap(), "/");
        assert_eq!(panel_path("plugins/Foo.jar").unwrap(), "/plugins/Foo.jar");
        assert_eq!(panel_path("/config\\a.yml").unwrap(), "/config/a.yml");
        assert!(panel_path("../etc/passwd").is_err());
        assert_eq!(split_path("plugins/Foo.jar").unwrap(), ("/plugins".into(), "Foo.jar".into()));
        assert_eq!(split_path("server.properties").unwrap(), ("/".into(), "server.properties".into()));
        assert!(split_path("").is_err());
    }
}

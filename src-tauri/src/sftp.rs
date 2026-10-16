
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::remote::FileRow;

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Side {
    Local,
    Ssh { host_id: String },
    Panel { instance_id: String },
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FileRow>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub job: String,
    pub file: String,
    pub sent: u64,
    pub total: u64,
    pub index: u32,
    pub count: u32,
    pub stage: String,
    pub note: String,
}

fn cancelled() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn cancel(job: &str) {
    cancelled().lock().expect("sftp poisoned").insert(job.to_string());
}

fn is_cancelled(job: &str) -> bool {
    cancelled().lock().expect("sftp poisoned").contains(job)
}

fn secs(time: std::io::Result<std::time::SystemTime>) -> u64 {
    time.ok()
        .and_then(|one| one.duration_since(UNIX_EPOCH).ok())
        .map(|one| one.as_secs())
        .unwrap_or(0)
}

fn sort(rows: &mut [FileRow]) {
    rows.sort_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

fn parent_of(path: &str, root: &str) -> Option<String> {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() || trimmed == root.trim_end_matches(['/', '\\']) {
        return None;
    }
    let cut = trimmed.rfind(['/', '\\'])?;
    let up = &trimmed[..cut];
    Some(if up.is_empty() { "/".into() } else if up.ends_with(':') { format!("{up}\\") } else { up.to_string() })
}

fn join(dir: &str, name: &str, windows: bool) -> String {
    let sep = if windows { '\\' } else { '/' };
    if dir.is_empty() {
        name.to_string()
    } else if dir.ends_with(['/', '\\']) {
        format!("{dir}{name}")
    } else {
        format!("{dir}{sep}{name}")
    }
}

/// Must be a single name (new folder, rename).
fn plain_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) || name.contains('\0') {
        return Err("Names cannot contain slashes or ..".into());
    }
    Ok(name.to_string())
}

fn local_list(path: &str) -> Result<Listing, String> {
    if path.trim().is_empty() {
        let entries = (b'A'..=b'Z')
            .map(|letter| format!("{}:\\", letter as char))
            .filter(|drive| Path::new(drive).exists())
            .map(|drive| FileRow { name: drive, directory: true, size_bytes: 0, modified_at: 0 })
            .collect();
        return Ok(Listing { path: String::new(), parent: None, entries });
    }
    let dir = Path::new(path);
    let read = std::fs::read_dir(dir).map_err(|error| format!("Cannot open {path}: {error}"))?;
    let mut entries: Vec<FileRow> = read
        .flatten()
        .map(|one| {
            let meta = one.metadata().ok();
            FileRow {
                name: one.file_name().to_string_lossy().to_string(),
                directory: meta.as_ref().is_some_and(|m| m.is_dir()),
                size_bytes: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified_at: meta.map(|m| secs(m.modified())).unwrap_or(0),
            }
        })
        .collect();
    sort(&mut entries);
    let parent = parent_of(path, "").or_else(|| (path.len() <= 3).then(String::new));
    Ok(Listing { path: path.to_string(), parent, entries })
}

pub async fn list(app: &AppHandle, side: &Side, path: &str) -> Result<Listing, String> {
    match side {
        Side::Local => {
            let path = if path.is_empty() {
                std::env::var("USERPROFILE").unwrap_or_default()
            } else if path == "::drives" {
                String::new()
            } else {
                path.to_string()
            };
            local_list(&path)
        }
        Side::Ssh { host_id } => {
            let at = if path.trim().is_empty() {
                crate::remote::server_root(app, host_id).await?
            } else {
                path.to_string()
            };
            let mut entries = crate::remote::list_dir(app, host_id, "/", &at).await?;
            sort(&mut entries);
            Ok(Listing { parent: parent_of(&at, "/"), path: at, entries })
        }
        Side::Panel { instance_id } => {
            let (api, target) = panel_side(app, instance_id)?;
            let at = path.trim_matches('/').to_string();
            let mut entries = crate::pfs::list_dir(&api, &target, &at).await?;
            sort(&mut entries);
            Ok(Listing {
                parent: (!at.is_empty()).then(|| at.rsplit_once('/').map(|(up, _)| up.to_string()).unwrap_or_default()),
                path: at,
                entries,
            })
        }
    }
}

fn panel_side(app: &AppHandle, instance_id: &str) -> Result<(crate::panel::Api, crate::panel::Target), String> {
    let instance = crate::instance_by_id(app, instance_id)?;
    if instance.panel_host.is_empty() {
        return Err("Not a panel server.".into());
    }
    Ok((crate::panel::api(app, &instance.panel_host)?, crate::panel_live::target_of(&instance)))
}

pub async fn make_dir(app: &AppHandle, side: &Side, dir: &str, name: &str) -> Result<(), String> {
    let name = plain_name(name)?;
    match side {
        Side::Local => std::fs::create_dir(join(dir, &name, true)).map_err(|error| format!("Cannot create the folder: {error}")),
        Side::Ssh { host_id } => {
            let conn = crate::remote::connect(app, host_id).await?;
            let sftp = conn.sftp().await?;
            sftp.create_dir(join(dir, &name, false))
                .await
                .map_err(|error| format!("Cannot create the folder: {error}"))
        }
        Side::Panel { instance_id } => {
            let (api, target) = panel_side(app, instance_id)?;
            api.make_dir(&target, dir.trim_matches('/'), &name).await
        }
    }
}

pub async fn rename(app: &AppHandle, side: &Side, dir: &str, from: &str, to: &str) -> Result<(), String> {
    let from = plain_name(from)?;
    let to = plain_name(to)?;
    match side {
        Side::Local => {
            let target = PathBuf::from(join(dir, &to, true));
            if target.exists() {
                return Err(format!("{to} already exists."));
            }
            std::fs::rename(join(dir, &from, true), target).map_err(|error| format!("Cannot rename: {error}"))
        }
        Side::Ssh { host_id } => {
            let conn = crate::remote::connect(app, host_id).await?;
            let q = crate::remote::quote;
            conn.run(&format!("mv -n {} {}", q(&join(dir, &from, false)), q(&join(dir, &to, false))))
                .await?
                .ok()
                .map(|_| ())
        }
        Side::Panel { instance_id } => {
            let (api, target) = panel_side(app, instance_id)?;
            let dir = dir.trim_matches('/');
            let whole = |name: &str| if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") };
            api.rename(&target, &whole(&from), &whole(&to)).await
        }
    }
}

pub async fn delete(app: &AppHandle, side: &Side, dir: &str, names: &[String]) -> Result<(), String> {
    let names: Vec<String> = names.iter().map(|one| plain_name(one)).collect::<Result<_, _>>()?;
    match side {
        Side::Local => {
            for name in &names {
                let path = PathBuf::from(join(dir, name, true));
                let done = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
                done.map_err(|error| format!("Cannot delete {name}: {error}"))?;
            }
            Ok(())
        }
        Side::Ssh { host_id } => {
            if dir.trim_end_matches('/').matches('/').count() < 2 {
                return Err("Cannot delete from here. Delete from a deeper folder.".into());
            }
            let conn = crate::remote::connect(app, host_id).await?;
            let list = names
                .iter()
                .map(|name| crate::remote::quote(&join(dir, name, false)))
                .collect::<Vec<_>>()
                .join(" ");
            conn.run(&format!("rm -rf -- {list}")).await?.ok().map(|_| ())
        }
        Side::Panel { instance_id } => {
            let (api, target) = panel_side(app, instance_id)?;
            let dir = dir.trim_matches('/');
            let whole: Vec<String> = names
                .iter()
                .map(|name| if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") })
                .collect();
            api.delete(&target, &whole).await
        }
    }
}

struct Item {
    from: String,
    relative: String,
    size: u64,
}

fn walk_local(dir: &str, names: &[String]) -> Result<(Vec<Item>, Vec<String>), String> {
    let mut files = Vec::new();
    let mut folders = Vec::new();
    fn go(path: &Path, relative: &str, files: &mut Vec<Item>, folders: &mut Vec<String>) {
        if path.is_dir() {
            folders.push(relative.to_string());
            if let Ok(read) = std::fs::read_dir(path) {
                for one in read.flatten() {
                    let name = one.file_name().to_string_lossy().to_string();
                    go(&one.path(), &format!("{relative}/{name}"), files, folders);
                }
            }
        } else if let Ok(meta) = path.metadata() {
            files.push(Item { from: path.to_string_lossy().to_string(), relative: relative.to_string(), size: meta.len() });
        }
    }
    for name in names {
        let name = plain_name(name)?;
        go(&PathBuf::from(join(dir, &name, true)), &name, &mut files, &mut folders);
    }
    Ok((files, folders))
}

async fn walk_ssh(sftp: &russh_sftp::client::SftpSession, dir: &str, names: &[String]) -> Result<(Vec<Item>, Vec<String>), String> {
    let mut files = Vec::new();
    let mut folders = Vec::new();
    let mut queue: Vec<(String, String)> = Vec::new();
    for name in names {
        let name = plain_name(name)?;
        queue.push((join(dir, &name, false), name));
    }
    while let Some((path, relative)) = queue.pop() {
        let meta = sftp.metadata(&path).await.map_err(|error| format!("{path}: {error}"))?;
        if meta.is_dir() {
            folders.push(relative.clone());
            for one in sftp.read_dir(&path).await.map_err(|error| format!("{path}: {error}"))? {
                let name = one.file_name();
                queue.push((format!("{path}/{name}"), format!("{relative}/{name}")));
            }
        } else {
            files.push(Item { from: path, relative, size: meta.size.unwrap_or(0) });
        }
        if files.len() > 50_000 {
            return Err("Too many files (over 50,000). Move the folder in parts.".into());
        }
    }
    Ok((files, folders))
}

async fn walk_panel(
    api: &crate::panel::Api,
    target: &crate::panel::Target,
    dir: &str,
    names: &[String],
) -> Result<(Vec<Item>, Vec<String>), String> {
    let dir = dir.trim_matches('/').to_string();
    let mut files = Vec::new();
    let mut folders = Vec::new();
    let here = crate::pfs::list_dir(api, target, &dir).await?;
    let mut queue: Vec<(String, String, bool, u64)> = Vec::new();
    for name in names {
        let name = plain_name(name)?;
        let row = here.iter().find(|one| one.name == name).ok_or(format!("Could not find {name}."))?;
        let path = if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") };
        queue.push((path, name, row.directory, row.size_bytes));
    }
    while let Some((path, relative, directory, size)) = queue.pop() {
        if directory {
            folders.push(relative.clone());
            for one in crate::pfs::list_dir(api, target, &path).await? {
                queue.push((format!("{path}/{}", one.name), format!("{relative}/{}", one.name), one.directory, one.size_bytes));
            }
        } else {
            files.push(Item { from: path, relative, size });
        }
        if files.len() > 20_000 {
            return Err("Too many files (over 20,000). Move the folder in parts.".into());
        }
    }
    Ok((files, folders))
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TransferResult {
    pub sent: u32,
    pub skipped: u32,
    pub failed: u32,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

pub async fn transfer(
    app: &AppHandle,
    job: &str,
    from: &Side,
    from_dir: &str,
    names: &[String],
    to: &Side,
    to_dir: &str,
) -> Result<TransferResult, String> {
    cancelled().lock().expect("sftp poisoned").remove(job);
    let result = match (from, to) {
        (Side::Local, Side::Local) => Err("Both sides are local. Pick an SSH or panel server on one side.".into()),
        (Side::Local, remote) => upload(app, job, from_dir, names, remote, to_dir).await,
        (remote, Side::Local) => download(app, job, remote, from_dir, names, to_dir).await,
        (from, to) => relay(app, job, from, from_dir, names, to, to_dir).await,
    };
    let summary = result.as_ref().cloned().unwrap_or_default();
    let stage = if summary.cancelled { "cancelled" } else { "done" };
    tell(app, job, "", 0, 0, 0, 0, stage, result.as_ref().err().map(String::as_str).unwrap_or(""));
    cancelled().lock().expect("sftp poisoned").remove(job);
    result
}

#[allow(clippy::too_many_arguments)]
fn tell(app: &AppHandle, job: &str, file: &str, sent: u64, total: u64, index: u32, count: u32, stage: &str, note: &str) {
    let _ = app.emit(
        "sftp-progress",
        Progress {
            job: job.into(),
            file: file.into(),
            sent,
            total,
            index,
            count,
            stage: stage.into(),
            note: note.into(),
        },
    );
}

async fn relay(
    app: &AppHandle,
    job: &str,
    from: &Side,
    from_dir: &str,
    names: &[String],
    to: &Side,
    to_dir: &str,
) -> Result<TransferResult, String> {
    let stage = std::env::temp_dir().join(format!("monolab-relay-{}", job.replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "_")));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|error| format!("Could not create a temporary folder: {error}"))?;
    let place = stage.to_string_lossy().into_owned();
    let result = async {
        let got = download(app, job, from, from_dir, names, &place).await?;
        if got.cancelled {
            return Ok(got);
        }
        let have: Vec<String> = names.iter().filter(|name| stage.join(name).exists()).cloned().collect();
        let mut sent = upload(app, job, &place, &have, to, to_dir).await?;
        sent.failed += got.failed;
        sent.skipped += got.skipped;
        let mut errors = got.errors;
        errors.extend(sent.errors);
        errors.truncate(5);
        sent.errors = errors;
        Ok(sent)
    }
    .await;
    let _ = std::fs::remove_dir_all(&stage);
    result
}

async fn upload(app: &AppHandle, job: &str, from_dir: &str, names: &[String], to: &Side, to_dir: &str) -> Result<TransferResult, String> {
    let (files, folders) = walk_local(from_dir, names)?;
    let count = files.len() as u32;
    let mut out = TransferResult::default();
    match to {
        Side::Ssh { host_id } => {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let conn = crate::remote::connect(app, host_id).await?;
            let sftp = conn.sftp().await?;
            for folder in &folders {
                let _ = sftp.create_dir(join(to_dir, folder, false)).await;
            }
            for (index, item) in files.iter().enumerate() {
                let index = index as u32 + 1;
                if is_cancelled(job) {
                    out.cancelled = true;
                    break;
                }
                let dest = join(to_dir, &item.relative, false);
                if sftp.try_exists(&dest).await.unwrap_or(false) {
                    out.skipped += 1;
                    tell(app, job, &item.relative, 0, item.size, index, count, "skipped", "Something with the same name is already there.");
                    continue;
                }
                let sent = async {
                    let mut src = tokio::fs::File::open(&item.from).await.map_err(|error| error.to_string())?;
                    let mut dst = sftp.create(&dest).await.map_err(|error| format!("{dest}: {error}"))?;
                    let mut buffer = vec![0u8; 256 * 1024];
                    let mut done = 0u64;
                    loop {
                        if is_cancelled(job) {
                            return Err("cancel".to_string());
                        }
                        let read = src.read(&mut buffer).await.map_err(|error| error.to_string())?;
                        if read == 0 {
                            break;
                        }
                        dst.write_all(&buffer[..read]).await.map_err(|error| error.to_string())?;
                        done += read as u64;
                        tell(app, job, &item.relative, done, item.size, index, count, "running", "");
                    }
                    dst.shutdown().await.map_err(|error| error.to_string())?;
                    Ok::<(), String>(())
                }
                .await;
                match sent {
                    Ok(()) => out.sent += 1,
                    Err(error) => {
                        let _ = sftp.remove_file(&dest).await;
                        if error == "cancel" {
                            out.cancelled = true;
                            break;
                        }
                        out.failed += 1;
                        if out.errors.len() < 5 {
                            out.errors.push(format!("{}: {error}", item.relative));
                        }
                        tell(app, job, &item.relative, 0, item.size, index, count, "failed", &error);
                    }
                }
            }
        }
        Side::Panel { instance_id } => {
            let (api, target) = panel_side(app, instance_id)?;
            let base = to_dir.trim_matches('/').to_string();
            let under = |relative: &str| if base.is_empty() { relative.to_string() } else { format!("{base}/{relative}") };
            let mut sorted = folders.clone();
            sorted.sort_by_key(|one| one.matches('/').count());
            for folder in &sorted {
                let (up, name) = folder.rsplit_once('/').map(|(a, b)| (under(a), b.to_string())).unwrap_or((base.clone(), folder.clone()));
                let _ = api.make_dir(&target, &up, &name).await;
            }
            for (index, item) in files.iter().enumerate() {
                let index = index as u32 + 1;
                if is_cancelled(job) {
                    out.cancelled = true;
                    break;
                }
                let whole = under(&item.relative);
                if api.exists(&target, &whole).await.ok().flatten().is_some() {
                    out.skipped += 1;
                    tell(app, job, &item.relative, 0, item.size, index, count, "skipped", "Something with the same name is already there.");
                    continue;
                }
                let (dir, name) = whole.rsplit_once('/').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((String::new(), whole.clone()));
                tell(app, job, &item.relative, 0, item.size, index, count, "running", "");
                let sent = match std::fs::read(&item.from) {
                    Ok(bytes) => api.upload(&target, &dir, &name, bytes).await,
                    Err(error) => Err(error.to_string()),
                };
                match sent {
                    Ok(()) => {
                        out.sent += 1;
                        tell(app, job, &item.relative, item.size, item.size, index, count, "running", "");
                    }
                    Err(error) => {
                        out.failed += 1;
                        if out.errors.len() < 5 {
                            out.errors.push(format!("{}: {error}", item.relative));
                        }
                        tell(app, job, &item.relative, 0, item.size, index, count, "failed", &error);
                    }
                }
            }
        }
        Side::Local => unreachable!(),
    }
    Ok(out)
}

async fn download(app: &AppHandle, job: &str, from: &Side, from_dir: &str, names: &[String], to_dir: &str) -> Result<TransferResult, String> {
    if to_dir.trim().is_empty() {
        return Err("Open a local folder to receive into first.".into());
    }
    let mut out = TransferResult::default();
    match from {
        Side::Ssh { host_id } => {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let conn = crate::remote::connect(app, host_id).await?;
            let sftp = conn.sftp().await?;
            let (files, folders) = walk_ssh(&sftp, from_dir, names).await?;
            let count = files.len() as u32;
            for folder in &folders {
                let _ = std::fs::create_dir_all(join(to_dir, &folder.replace('/', "\\"), true));
            }
            for (index, item) in files.iter().enumerate() {
                let index = index as u32 + 1;
                if is_cancelled(job) {
                    out.cancelled = true;
                    break;
                }
                let dest = PathBuf::from(join(to_dir, &item.relative.replace('/', "\\"), true));
                if dest.exists() {
                    out.skipped += 1;
                    tell(app, job, &item.relative, 0, item.size, index, count, "skipped", "Something with the same name is already there.");
                    continue;
                }
                let part = dest.with_extension("monolab-part");
                let got = async {
                    let mut src = sftp.open(&item.from).await.map_err(|error| format!("{}: {error}", item.from))?;
                    let mut dst = tokio::fs::File::create(&part).await.map_err(|error| error.to_string())?;
                    let mut buffer = vec![0u8; 64 * 1024];
                    let mut done = 0u64;
                    loop {
                        if is_cancelled(job) {
                            return Err("cancel".to_string());
                        }
                        let read = src.read(&mut buffer).await.map_err(|error| error.to_string())?;
                        if read == 0 {
                            break;
                        }
                        dst.write_all(&buffer[..read]).await.map_err(|error| error.to_string())?;
                        done += read as u64;
                        tell(app, job, &item.relative, done, item.size, index, count, "running", "");
                    }
                    dst.flush().await.map_err(|error| error.to_string())?;
                    Ok::<(), String>(())
                }
                .await;
                match got.and_then(|()| std::fs::rename(&part, &dest).map_err(|error| error.to_string())) {
                    Ok(()) => out.sent += 1,
                    Err(error) => {
                        let _ = std::fs::remove_file(&part);
                        if error == "cancel" {
                            out.cancelled = true;
                            break;
                        }
                        out.failed += 1;
                        if out.errors.len() < 5 {
                            out.errors.push(format!("{}: {error}", item.relative));
                        }
                        tell(app, job, &item.relative, 0, item.size, index, count, "failed", &error);
                    }
                }
            }
        }
        Side::Panel { instance_id } => {
            let (api, target) = panel_side(app, instance_id)?;
            let (files, folders) = walk_panel(&api, &target, from_dir, names).await?;
            let count = files.len() as u32;
            for folder in &folders {
                let _ = std::fs::create_dir_all(join(to_dir, &folder.replace('/', "\\"), true));
            }
            for (index, item) in files.iter().enumerate() {
                let index = index as u32 + 1;
                if is_cancelled(job) {
                    out.cancelled = true;
                    break;
                }
                let dest = PathBuf::from(join(to_dir, &item.relative.replace('/', "\\"), true));
                if dest.exists() {
                    out.skipped += 1;
                    tell(app, job, &item.relative, 0, item.size, index, count, "skipped", "Something with the same name is already there.");
                    continue;
                }
                let size = item.size;
                let got = api
                    .download_to(&target, &item.from, &dest, |done, total| {
                        tell(app, job, &item.relative, done, if total > 0 { total } else { size }, index, count, "running", "");
                    })
                    .await;
                match got {
                    Ok(_) => out.sent += 1,
                    Err(error) => {
                        out.failed += 1;
                        if out.errors.len() < 5 {
                            out.errors.push(format!("{}: {error}", item.relative));
                        }
                        tell(app, job, &item.relative, 0, item.size, index, count, "failed", &error);
                    }
                }
            }
        }
        Side::Local => unreachable!(),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_up_paths_both_ways() {
        assert_eq!(parent_of("/home/me/server", "/"), Some("/home/me".into()));
        assert_eq!(parent_of("/home", "/"), Some("/".into()));
        assert_eq!(parent_of("/", "/"), None);
        assert_eq!(parent_of("C:\\Users\\me", ""), Some("C:\\Users".into()));
        assert_eq!(parent_of("C:\\Users", ""), Some("C:\\".into()));
        assert_eq!(join("C:\\", "a", true), "C:\\a");
        assert_eq!(join("/home", "a", false), "/home/a");
    }

    #[test]
    fn refuses_names_that_are_paths() {
        assert!(plain_name("../etc").is_err());
        assert!(plain_name("a/b").is_err());
        assert!(plain_name("..").is_err());
        assert!(plain_name("world backup.zip").is_ok());
    }
}

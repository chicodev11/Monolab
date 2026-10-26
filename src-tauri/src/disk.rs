use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const OLD_DAYS: u64 = 30;

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiskEntry {
    pub path: String,
    pub bytes: u64,
    pub directory: bool,
}

#[derive(Clone, Debug, Serialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Cleanable {
    pub kind: String,
    pub files: u32,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiskReport {
    pub entries: Vec<DiskEntry>,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub backups_bytes: u64,
    pub trash_bytes: u64,
    pub cleanable: Vec<Cleanable>,
}

pub const KINDS: [&str; 3] = ["old-logs", "old-crash-reports", "heap-dumps"];

pub fn tree_size(path: &Path) -> (u64, u32) {
    let Ok(meta) = fs::symlink_metadata(path) else { return (0, 0) };
    if meta.file_type().is_symlink() {
        return (0, 0);
    }
    if meta.is_file() {
        return (meta.len(), 1);
    }
    let mut total = (0, 0);
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let (bytes, files) = tree_size(&entry.path());
            total.0 += bytes;
            total.1 += files;
        }
    }
    total
}

pub fn scan_local(root: &Path) -> Vec<DiskEntry> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(root) else { return out };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        let (bytes, _) = tree_size(&entry.path());
        out.push(DiskEntry { path: name.clone(), bytes, directory: kind.is_dir() });
        if kind.is_dir() {
            for child in fs::read_dir(entry.path()).into_iter().flatten().flatten() {
                let Ok(inner) = child.file_type() else { continue };
                if inner.is_symlink() {
                    continue;
                }
                let (bytes, _) = tree_size(&child.path());
                out.push(DiskEntry {
                    path: format!("{name}/{}", child.file_name().to_string_lossy()),
                    bytes,
                    directory: inner.is_dir(),
                });
            }
        }
    }
    out
}

fn is_old(path: &Path) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|when| SystemTime::now().duration_since(when).ok())
        .is_some_and(|age| age > Duration::from_secs(OLD_DAYS * 24 * 60 * 60))
}

fn files_in(folder: &Path) -> Vec<PathBuf> {
    fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .collect()
}

pub fn cleanable_files(root: &Path, kind: &str) -> Vec<PathBuf> {
    let name_of = |path: &Path| path.file_name().map(|one| one.to_string_lossy().to_lowercase()).unwrap_or_default();
    match kind {
        "old-logs" => files_in(&root.join("logs")).into_iter().filter(|one| name_of(one).ends_with(".log.gz") && is_old(one)).collect(),
        "old-crash-reports" => files_in(&root.join("crash-reports")).into_iter().filter(|one| is_old(one)).collect(),
        "heap-dumps" => files_in(root)
            .into_iter()
            .chain(files_in(&root.join("logs")))
            .filter(|one| name_of(one).ends_with(".hprof"))
            .collect(),
        _ => Vec::new(),
    }
}

pub fn local_cleanable(root: &Path) -> Vec<Cleanable> {
    KINDS
        .iter()
        .map(|kind| {
            let files = cleanable_files(root, kind);
            let bytes = files.iter().filter_map(|one| fs::metadata(one).ok()).map(|meta| meta.len()).sum();
            Cleanable { kind: (*kind).to_string(), files: files.len() as u32, bytes }
        })
        .collect()
}

pub fn remote_script() -> String {
    [
        "du -ak --max-depth=2 . 2>/dev/null",
        "echo @dirs",
        "find . -mindepth 1 -maxdepth 2 -type d 2>/dev/null",
        "echo @clean",
        "find logs -maxdepth 1 -type f -name '*.log.gz' -mtime +30 -printf 'old-logs %s\\n' 2>/dev/null",
        "find crash-reports -maxdepth 1 -type f -mtime +30 -printf 'old-crash-reports %s\\n' 2>/dev/null",
        "find . logs -maxdepth 1 -type f -name '*.hprof' -printf 'heap-dumps %s\\n' 2>/dev/null",
        "echo @backups",
        "du -sk \"../.monolab-backups/${PWD##*/}\" 2>/dev/null",
        "echo @trash",
        "du -sk .monolab-trash 2>/dev/null",
        "echo @free",
        "df -Pk . 2>/dev/null | awk 'NR==2{print $4}'",
    ]
    .join("\n")
}

pub fn remote_clean_command(kind: &str) -> Option<&'static str> {
    match kind {
        "old-logs" => Some("find logs -maxdepth 1 -type f -name '*.log.gz' -mtime +30 -delete"),
        "old-crash-reports" => Some("find crash-reports -maxdepth 1 -type f -mtime +30 -delete"),
        "heap-dumps" => Some("find . logs -maxdepth 1 -type f -name '*.hprof' -delete"),
        _ => None,
    }
}

pub fn parse_remote(text: &str) -> DiskReport {
    let mut part = "du";
    let mut sizes: Vec<(String, u64)> = Vec::new();
    let mut dirs = HashSet::new();
    let mut clean: HashMap<String, (u32, u64)> = HashMap::new();
    let mut report = DiskReport::default();
    for line in text.lines() {
        let line = line.trim_end();
        if let Some(next) = line.strip_prefix('@') {
            part = match next {
                "dirs" => "dirs",
                "clean" => "clean",
                "backups" => "backups",
                "trash" => "trash",
                "free" => "free",
                _ => part,
            };
            continue;
        }
        match part {
            "du" => {
                let Some((size, path)) = line.split_once('\t') else { continue };
                let Some(path) = path.strip_prefix("./") else { continue };
                if let Ok(kib) = size.trim().parse::<u64>() {
                    sizes.push((path.to_string(), kib * 1024));
                }
            }
            "dirs" => {
                if let Some(path) = line.strip_prefix("./") {
                    dirs.insert(path.to_string());
                }
            }
            "clean" => {
                let Some((kind, size)) = line.split_once(' ') else { continue };
                let entry = clean.entry(kind.to_string()).or_default();
                entry.0 += 1;
                entry.1 += size.trim().parse::<u64>().unwrap_or(0);
            }
            "backups" => {
                if let Some(kib) = line.split_whitespace().next().and_then(|one| one.parse::<u64>().ok()) {
                    report.backups_bytes = kib * 1024;
                }
            }
            "trash" => {
                if let Some(kib) = line.split_whitespace().next().and_then(|one| one.parse::<u64>().ok()) {
                    report.trash_bytes = kib * 1024;
                }
            }
            "free" => {
                if let Ok(kib) = line.trim().parse::<u64>() {
                    report.free_bytes = kib * 1024;
                }
            }
            _ => {}
        }
    }
    report.entries = sizes
        .into_iter()
        .map(|(path, bytes)| DiskEntry { directory: dirs.contains(&path), path, bytes })
        .collect();
    report.cleanable = KINDS
        .iter()
        .map(|kind| {
            let (files, bytes) = clean.get(*kind).copied().unwrap_or((0, 0));
            Cleanable { kind: (*kind).to_string(), files, bytes }
        })
        .collect();
    report.total_bytes = top_total(&report.entries);
    report
}

pub fn top_total(entries: &[DiskEntry]) -> u64 {
    entries.iter().filter(|one| !one.path.contains('/')).map(|one| one.bytes).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_du_output() {
        let text = "1024\t./world/region\n2048\t./world\n4\t./server.properties\n3000\t.\n@dirs\n./world\n./world/region\n@clean\nold-logs 500\nold-logs 700\nheap-dumps 9\n@backups\n10\t../.monolab-backups\n@trash\n3\t.monolab-trash\n@free\n100\n";
        let report = parse_remote(text);
        assert_eq!(report.entries.len(), 3);
        assert_eq!(report.total_bytes, (2048 + 4) * 1024);
        assert!(report.entries.iter().any(|one| one.path == "world/region" && one.directory));
        assert!(report.entries.iter().any(|one| one.path == "server.properties" && !one.directory));
        assert_eq!(report.cleanable[0], Cleanable { kind: "old-logs".into(), files: 2, bytes: 1200 });
        assert_eq!(report.cleanable[1].files, 0);
        assert_eq!(report.cleanable[2].bytes, 9);
        assert_eq!(report.backups_bytes, 10 * 1024);
        assert_eq!(report.free_bytes, 100 * 1024);
        assert_eq!(report.trash_bytes, 3 * 1024);
    }

    #[test]
    fn scans_a_folder_two_levels_deep() {
        let root = std::env::temp_dir().join(format!("se-disk-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("world/region")).unwrap();
        fs::create_dir_all(root.join("logs")).unwrap();
        fs::write(root.join("world/region/r.0.0.mca"), vec![0u8; 5000]).unwrap();
        fs::write(root.join("world/level.dat"), vec![0u8; 100]).unwrap();
        fs::write(root.join("logs/dump.hprof"), vec![0u8; 50]).unwrap();
        fs::write(root.join("logs/2020-01-01-1.log.gz"), vec![0u8; 10]).unwrap();
        let entries = scan_local(&root);
        assert_eq!(entries.iter().find(|one| one.path == "world").unwrap().bytes, 5100);
        assert_eq!(entries.iter().find(|one| one.path == "world/region").unwrap().bytes, 5000);
        assert_eq!(top_total(&entries), 5160);
        let clean = local_cleanable(&root);
        assert_eq!(clean.iter().find(|one| one.kind == "heap-dumps").unwrap().bytes, 50);
        assert_eq!(clean.iter().find(|one| one.kind == "old-logs").unwrap().files, 0);
        let _ = fs::remove_dir_all(&root);
    }
}

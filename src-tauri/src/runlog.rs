use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FILE: &str = "runs.jsonl";
const KEEP: usize = 500;

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunEntry {
    pub at: u64,
    pub kind: String,
    pub detail: String,
}

static ASKED: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

pub fn asked_to_stop(id: &str) {
    ASKED.lock().expect("run log poisoned").get_or_insert_with(HashMap::new).insert(id.to_string(), Instant::now());
}

pub fn take_asked(id: &str) -> bool {
    ASKED
        .lock()
        .expect("run log poisoned")
        .get_or_insert_with(HashMap::new)
        .remove(id)
        .is_some_and(|when| when.elapsed() < Duration::from_secs(600))
}

pub fn record(folder: &Path, kind: &str, detail: &str) {
    let at = SystemTime::now().duration_since(UNIX_EPOCH).map(|one| one.as_secs()).unwrap_or(0);
    let entry = RunEntry { at, kind: kind.into(), detail: detail.chars().take(300).collect() };
    let Ok(line) = serde_json::to_string(&entry) else { return };
    let path = folder.join(FILE);
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{line}");
    }
    if fs::metadata(&path).is_ok_and(|meta| meta.len() > 200 * 1024) {
        let kept = read(folder);
        let text: String = kept
            .iter()
            .rev()
            .take(KEEP)
            .rev()
            .filter_map(|one| serde_json::to_string(one).ok())
            .map(|one| one + "\n")
            .collect();
        let _ = fs::write(&path, text);
    }
}

fn read(folder: &Path) -> Vec<RunEntry> {
    fs::read_to_string(folder.join(FILE))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

pub fn list(folder: &Path) -> Vec<RunEntry> {
    let mut all = read(folder);
    all.reverse();
    all.truncate(KEEP);
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_runs_newest_first() {
        let folder = std::env::temp_dir().join(format!("se-runs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        record(&folder, "start", "");
        record(&folder, "crash", "exit code 1");
        let got = list(&folder);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].kind, "crash");
        assert_eq!(got[1].kind, "start");
        asked_to_stop("a");
        assert!(take_asked("a"));
        assert!(!take_asked("a"));
        let _ = fs::remove_dir_all(&folder);
    }
}

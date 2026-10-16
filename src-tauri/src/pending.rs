
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Waiting {
    pub id: String,
    #[serde(default)]
    pub replaces: String,
    pub file_name: String,
    pub display_name: String,
    #[serde(default)]
    pub from_version: String,
    #[serde(default)]
    pub to_version: String,
    pub staged: String,
    pub queued_at: u64,
    pub source: String,
}

fn ledger(resting: &Path) -> PathBuf {
    resting.join("queue.json")
}

pub fn list(resting: &Path) -> Vec<Waiting> {
    fs::read_to_string(ledger(resting))
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<Waiting>>(&text).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|one| resting.join(&one.staged).is_file())
        .collect()
}

fn save(resting: &Path, items: &[Waiting]) -> Result<(), String> {
    fs::create_dir_all(resting)
        .map_err(|error| format!("Could not create the queue folder: {error}"))?;
    let text = serde_json::to_string_pretty(items).map_err(|error| error.to_string())?;
    fs::write(ledger(resting), text)
        .map_err(|error| format!("Could not save the queue: {error}"))
}

fn safe_name(name: &str) -> String {
    Path::new(name)
        .file_name()
        .map(|one| one.to_string_lossy().to_string())
        .unwrap_or_else(|| "addon.jar".into())
        .chars()
        .filter(|letter| !matches!(letter, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect()
}

pub fn queue(resting: &Path, mut entry: Waiting, source: &Path) -> Result<(), String> {
    let folder = resting;
    fs::create_dir_all(folder)
        .map_err(|error| format!("Could not create the queue folder: {error}"))?;

    entry.file_name = safe_name(&entry.file_name);
    entry.replaces = if entry.replaces.is_empty() {
        String::new()
    } else {
        safe_name(&entry.replaces)
    };
    entry.staged = format!("{}-{}", entry.id, entry.file_name);

    let bed = folder.join(&entry.staged);
    if fs::rename(source, &bed).is_err() {
        fs::copy(source, &bed)
            .map_err(|error| format!("Could not queue the downloaded file: {error}"))?;
        let _ = fs::remove_file(source);
    }

    let mut items = list(resting);
    if let Some(old) = items.iter().position(|one| {
        (!entry.replaces.is_empty() && one.replaces == entry.replaces)
            || one.file_name == entry.file_name
    }) {
        let _ = fs::remove_file(folder.join(&items[old].staged));
        items.remove(old);
    }
    items.push(entry);
    save(resting, &items)
}

pub fn cancel(resting: &Path, id: &str) -> Result<(), String> {
    let mut items = list(resting);
    let Some(at) = items.iter().position(|one| one.id == id) else {
        return Err("Already handled.".into());
    };
    let _ = fs::remove_file(resting.join(&items[at].staged));
    items.remove(at);
    save(resting, &items)
}

pub struct Applied {
    pub display_name: String,
    pub from_version: String,
    pub to_version: String,
}

pub fn apply(resting: &Path, folder: &Path, keep: &Path, now: u64) -> Vec<Applied> {
    let items = list(resting);
    if items.is_empty() {
        return Vec::new();
    }
    let mut done = Vec::new();
    let mut left = Vec::new();

    for one in items {
        let staged = resting.join(&one.staged);
        let target = folder.join(&one.file_name);
        if folder.metadata().is_err() && fs::create_dir_all(folder).is_err() {
            left.push(one);
            continue;
        }
        if !one.replaces.is_empty() {
            let old = folder.join(&one.replaces);
            if old.is_file() {
                let _ = fs::create_dir_all(keep);
                let kept = keep.join(format!("{now}-{}", one.replaces));
                if fs::rename(&old, &kept).is_err() {
                    left.push(one);
                    continue;
                }
            }
        }
        if fs::rename(&staged, &target).is_err() {
            if fs::copy(&staged, &target).is_err() {
                left.push(one);
                continue;
            }
            let _ = fs::remove_file(&staged);
        }
        done.push(Applied {
            display_name: one.display_name,
            from_version: one.from_version,
            to_version: one.to_version,
        });
    }

    let _ = save(resting, &left);
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn ground(tag: &str) -> (PathBuf, PathBuf) {
        let path = std::env::temp_dir().join(format!("pending-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("server/plugins")).unwrap();
        fs::create_dir_all(path.join("data/pending")).unwrap();
        (path.join("server"), path.join("data/pending"))
    }

    fn make(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = fs::File::create(path).unwrap();
        file.write_all(body.as_bytes()).unwrap();
    }

    fn waiting(id: &str, replaces: &str, file_name: &str) -> Waiting {
        Waiting {
            id: id.into(),
            replaces: replaces.into(),
            file_name: file_name.into(),
            display_name: "My plugin".into(),
            from_version: "1.0".into(),
            to_version: "1.1".into(),
            staged: String::new(),
            queued_at: 1,
            source: "Added manually".into(),
        }
    }

    #[test]
    fn holds_a_jar_until_the_next_start() {
        let (home, rest) = ground("basic");
        let folder = home.join("plugins");
        let keep = rest.parent().unwrap().join("restore/addons");
        make(&folder.join("mine-1.0.jar"), "old");
        let fresh = home.join("received.jar");
        make(&fresh, "new");

        queue(&rest, waiting("a1", "mine-1.0.jar", "mine-1.1.jar"), &fresh).unwrap();
        assert!(folder.join("mine-1.0.jar").is_file());
        assert!(!folder.join("mine-1.1.jar").exists());
        assert_eq!(list(&rest).len(), 1);

        let done = apply(&rest, &folder, &keep, 100);
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].to_version, "1.1");
        assert!(!folder.join("mine-1.0.jar").exists());
        assert_eq!(
            fs::read_to_string(folder.join("mine-1.1.jar")).unwrap(),
            "new"
        );
        assert!(keep.join("100-mine-1.0.jar").is_file());
        assert!(list(&rest).is_empty());
        let _ = fs::remove_dir_all(home.parent().unwrap());
    }

    #[test]
    fn leaves_what_it_cannot_place_and_puts_the_rest() {
        let (home, rest) = ground("locked");
        let folder = home.join("plugins");
        let keep = rest.parent().unwrap().join("restore/addons");
        make(&folder.join("a-1.0.jar"), "a-old");
        make(&folder.join("b-1.0.jar"), "b-old");
        for (id, old, new) in [
            ("a1", "a-1.0.jar", "a-1.1.jar"),
            ("a2", "b-1.0.jar", "b-1.1.jar"),
        ] {
            let fresh = home.join(format!("{id}.jar"));
            make(&fresh, "new");
            queue(&rest, waiting(id, old, new), &fresh).unwrap();
        }
        let blocker = keep.join("100-a-1.0.jar");
        fs::create_dir_all(&blocker).unwrap();
        make(&blocker.join("inside.txt"), "x");

        let done = apply(&rest, &folder, &keep, 100);
        assert_eq!(done.len(), 1, "working ones should be added");
        assert_eq!(done[0].display_name, "My plugin");
        assert!(folder.join("b-1.1.jar").is_file(), "b should be added");
        assert!(
            folder.join("a-1.0.jar").is_file(),
            "a should stay"
        );
        let left = list(&rest);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].replaces, "a-1.0.jar");
        let _ = fs::remove_dir_all(home.parent().unwrap());
    }

    #[test]
    fn keeps_only_the_newest_for_one_slot() {
        let (home, rest) = ground("twice");
        let first = home.join("first.jar");
        make(&first, "first");
        queue(&rest, waiting("a1", "mine-1.0.jar", "mine-1.1.jar"), &first).unwrap();
        let second = home.join("second.jar");
        make(&second, "two");
        queue(
            &rest,
            waiting("a2", "mine-1.0.jar", "mine-1.2.jar"),
            &second,
        )
        .unwrap();

        let items = list(&rest);
        assert_eq!(items.len(), 1, "only one should remain");
        assert_eq!(items[0].file_name, "mine-1.2.jar");
        let _ = fs::remove_dir_all(home.parent().unwrap());
    }

    #[test]
    fn forgets_entries_whose_file_vanished() {
        let (home, rest) = ground("gone");
        let fresh = home.join("new.jar");
        make(&fresh, "new");
        queue(&rest, waiting("a1", "", "new.jar"), &fresh).unwrap();
        assert_eq!(list(&rest).len(), 1);
        fs::remove_file(rest.join(list(&rest)[0].staged.clone())).unwrap();
        assert!(list(&rest).is_empty());
        let _ = fs::remove_dir_all(home.parent().unwrap());
    }

    #[test]
    fn refuses_names_that_climb_out() {
        let (home, rest) = ground("climb");
        let folder = home.join("plugins");
        let keep = rest.parent().unwrap().join("restore/addons");
        let fresh = home.join("bad.jar");
        make(&fresh, "bad");
        queue(&rest, waiting("a1", "", "../../outside.jar"), &fresh).unwrap();
        apply(&rest, &folder, &keep, 100);
        assert!(!home.parent().unwrap().join("outside.jar").exists());
        assert!(
            folder.join("outside.jar").is_file(),
            "only tidies the name and puts it in place"
        );
        let _ = fs::remove_dir_all(home.parent().unwrap());
    }
}

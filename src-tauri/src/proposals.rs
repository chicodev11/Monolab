//! Change proposals (F8, like a pull request): with the server's "Review changes" switch on, a Developer's file and
//! studio changes (theirs or their agent's) don't go in right away. The host keeps each as a proposal — the request,
//! who sent it, and the hash of every file it touches as it was then — in `<app data>/proposals/<server>.json`
//! (uploaded files in `proposals/files/`). The host's window, or a team Admin or Owner, merges it (the request runs
//! as the host's own, refused when one of those files changed since: "Conflicts with a newer change.") or rejects it
//! with a reason. Console commands and starting or stopping are never held.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// Requests that become proposals.
pub const PROPOSABLE: &[&str] =
    &["save_editable_file", "create_server_file", "trash_server_path", "studio_save", "studio_asset_add_upload", "studio_asset_remove"];
/// Open proposals kept per server.
const MOST_OPEN: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FileBefore {
    pub path: String,
    /// SHA-256 of the file's bytes when the proposal came, None when there was no file.
    pub hash: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub id: String,
    pub server: String,
    /// `@amy` or `@amy's AI`.
    pub by: String,
    #[specta(type = f64)]
    pub at: u64,
    pub command: String,
    /// The request as it came (an upload's bytes are kept beside it, see `file`).
    #[specta(type = String)]
    pub body: Value,
    pub before: Vec<FileBefore>,
    /// `open`, `merged` or `rejected`.
    pub state: String,
    #[serde(default)]
    pub reason: String,
    /// The uploaded file this proposal carries, if any (a studio asset).
    #[serde(default)]
    pub file: Option<String>,
}

pub struct Proposals {
    dir: PathBuf,
}

fn safe(server: &str) -> Result<&str, String> {
    if !server.is_empty() && server.len() <= 80 && server.chars().all(|one| one.is_ascii_alphanumeric() || one == '-' || one == '_') {
        Ok(server)
    } else {
        Err("This isn't a server.".into())
    }
}

impl Proposals {
    pub fn open(root: &Path) -> Result<Self, String> {
        fs::create_dir_all(root.join("files")).map_err(|error| format!("Could not keep proposals: {error}"))?;
        Ok(Proposals { dir: root.to_path_buf() })
    }

    fn path(&self, server: &str) -> Result<PathBuf, String> {
        Ok(self.dir.join(format!("{}.json", safe(server)?)))
    }

    pub fn list(&self, server: &str) -> Vec<Proposal> {
        self.path(server)
            .ok()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self, server: &str, list: &[Proposal]) -> Result<(), String> {
        let text = serde_json::to_string_pretty(list).map_err(|error| error.to_string())?;
        fs::write(self.path(server)?, text).map_err(|error| format!("Could not save the proposal: {error}"))
    }

    pub fn files_dir(&self) -> PathBuf {
        self.dir.join("files")
    }

    /// Keeps a new proposal. `upload` is a file to move in beside it (a studio asset's bytes).
    pub fn add(&self, mut proposal: Proposal, upload: Option<&Path>) -> Result<Proposal, String> {
        let mut list = self.list(&proposal.server);
        if list.iter().filter(|one| one.state == "open").count() >= MOST_OPEN {
            return Err("Too many changes wait for review on this server. Ask the host to go through them.".into());
        }
        if let Some(source) = upload {
            let kept = self.files_dir().join(format!("{}-upload", proposal.id));
            fs::rename(source, &kept)
                .or_else(|_| fs::copy(source, &kept).map(|_| ()))
                .map_err(|error| format!("Could not keep the file: {error}"))?;
            proposal.file = Some(kept.to_string_lossy().to_string());
        }
        proposal.state = "open".into();
        list.push(proposal.clone());
        self.save(&proposal.server, &list)?;
        Ok(proposal)
    }

    pub fn get(&self, server: &str, id: &str) -> Result<Proposal, String> {
        self.list(server).into_iter().find(|one| one.id == id).ok_or_else(|| "There is no such proposal.".to_string())
    }

    /// Marks a proposal merged or rejected (with a reason) and drops its kept file.
    pub fn close(&self, server: &str, id: &str, state: &str, reason: &str) -> Result<Proposal, String> {
        let mut list = self.list(server);
        let one = list.iter_mut().find(|one| one.id == id).ok_or("There is no such proposal.")?;
        if one.state != "open" {
            return Err("This proposal was already answered.".into());
        }
        one.state = state.into();
        one.reason = reason.trim().chars().take(300).collect();
        if let Some(file) = one.file.take() {
            let _ = fs::remove_file(file);
        }
        let done = one.clone();
        self.save(server, &list)?;
        Ok(done)
    }
}

/// Whether a request is held for review: the switch is on, the sender is a Developer, and it changes files.
pub fn held(switch_on: bool, role: &str, command: &str) -> bool {
    switch_on && role == "developer" && PROPOSABLE.contains(&command)
}

/// Whether a merge must stop: a file the proposal touches is not as it was when the proposal came.
pub fn moved_on(before: &[FileBefore], now: &dyn Fn(&str) -> Option<String>) -> bool {
    before.iter().any(|one| now(&one.path) != one.hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sandbox(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("monolab-proposals-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn proposal(id: &str) -> Proposal {
        Proposal {
            id: id.into(),
            server: "srv-1".into(),
            by: "@amy".into(),
            command: "save_editable_file".into(),
            body: json!({ "id": "srv-1", "path": "a.yml", "content": "x: 2" }),
            before: vec![FileBefore { path: "a.yml".into(), hash: Some("h1".into()) }],
            ..Default::default()
        }
    }

    #[test]
    fn developer_writes_become_proposals() {
        let store = Proposals::open(&sandbox("add")).unwrap();
        store.add(proposal("p1"), None).unwrap();
        let list = store.list("srv-1");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].state, "open");
        assert!(
            PROPOSABLE.contains(&"studio_save") && !PROPOSABLE.contains(&"send_console_command") && !PROPOSABLE.contains(&"start_instance")
        );
    }

    #[test]
    fn admin_writes_go_through() {
        assert!(held(true, "developer", "save_editable_file"));
        assert!(!held(true, "admin", "save_editable_file"));
        assert!(!held(true, "owner", "studio_save"));
        assert!(!held(true, "developer", "send_console_command"));
    }

    #[test]
    fn off_by_default() {
        assert!(!held(false, "developer", "save_editable_file"));
        assert!(!crate::monolink_team::TeamServerLink::default().proposals);
    }

    #[test]
    fn merge_refuses_when_the_file_moved_on() {
        let before = proposal("p").before;
        assert!(!moved_on(&before, &|_| Some("h1".into())));
        assert!(moved_on(&before, &|_| Some("h2".into())));
        assert!(moved_on(&before, &|_| None));
        assert!(!moved_on(&[FileBefore { path: "new.yml".into(), hash: None }], &|_| None));
    }

    #[test]
    fn reject_keeps_the_file() {
        let root = sandbox("reject");
        let store = Proposals::open(&root).unwrap();
        let upload = root.join("bot.bbmodel");
        fs::write(&upload, b"{}").unwrap();
        let kept = store.add(proposal("p2"), Some(&upload)).unwrap();
        assert!(Path::new(kept.file.as_deref().unwrap()).exists());
        let done = store.close("srv-1", "p2", "rejected", "not yet").unwrap();
        assert_eq!((done.state.as_str(), done.reason.as_str()), ("rejected", "not yet"));
        assert!(!Path::new(kept.file.as_deref().unwrap()).exists());
        assert!(store.close("srv-1", "p2", "merged", "").is_err());
        assert!(Proposals::open(&root).unwrap().path("../x").is_err());
    }
}

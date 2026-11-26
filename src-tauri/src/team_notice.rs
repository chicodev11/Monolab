//! Team notices (F4): small events for the people working on a server — a to-do given to someone, a file saved
//! while others have it open, a picture or model added to the studio, a change proposal. They go out as
//! `team-notice { serverId, kind, to, text }` to the host's window and, filtered by server, to teammates' windows;
//! each window shows the ones meant for it (`to` empty = everyone) when its switch is on (Settings → Notifications).

use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    /// A to-do was given to `to`.
    Assigned,
    /// A file was saved; whoever else has it open should reload. `text` is the path.
    SavedOver,
    /// A picture, model or sound went into the studio. `text` is the file name.
    AssetAdded,
    /// A teammate's change waits for review (`to` empty: the host's window and admins decide).
    ProposalNew,
    /// A change proposal was merged or rejected. `to` is whoever proposed it.
    ProposalDone,
}

impl NoticeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NoticeKind::Assigned => "assigned",
            NoticeKind::SavedOver => "savedOver",
            NoticeKind::AssetAdded => "assetAdded",
            NoticeKind::ProposalNew => "proposalNew",
            NoticeKind::ProposalDone => "proposalDone",
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub server_id: String,
    pub kind: &'static str,
    /// `@handle` it is for, or empty for everyone on the server.
    pub to: String,
    pub text: String,
}

pub fn notice(server: &str, kind: NoticeKind, to: Option<&str>, text: &str) -> Notice {
    Notice { server_id: server.to_string(), kind: kind.as_str(), to: to.unwrap_or_default().to_string(), text: text.to_string() }
}

/// Sends a notice to the host's window and to teammates on that server.
pub fn send(app: &AppHandle, server: &str, kind: NoticeKind, to: Option<&str>, text: &str) {
    if server.is_empty() {
        return;
    }
    let one = notice(server, kind, to, text);
    let _ = app.emit("team-notice", &one);
    crate::remember_for_web(app, "team-notice", &one);
}

/// To-dos whose assignee changed to someone between two versions of a memo: (assignee, to-do text).
pub fn newly_assigned(before: &serde_json::Value, after: &serde_json::Value) -> Vec<(String, String)> {
    let old = |id: &str| before["todos"].as_array().and_then(|list| list.iter().find(|one| one["id"].as_str() == Some(id))).and_then(|one| one["assignee"].as_str()).unwrap_or_default().to_string();
    after["todos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|one| {
            let who = one["assignee"].as_str().unwrap_or_default();
            let id = one["id"].as_str().unwrap_or_default();
            (!who.is_empty() && who != old(id)).then(|| (who.to_string(), one["text"].as_str().unwrap_or_default().to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn notice_payload_shape() {
        let value = serde_json::to_value(notice("srv", NoticeKind::Assigned, Some("@amy"), "Golem model")).unwrap();
        assert_eq!(value, json!({ "serverId": "srv", "kind": "assigned", "to": "@amy", "text": "Golem model" }));
        assert_eq!(serde_json::to_value(notice("srv", NoticeKind::AssetAdded, None, "bot.bbmodel")).unwrap()["to"], "");
    }

    #[test]
    fn finds_new_assignees_only() {
        let before = json!({ "todos": [{ "id": "a", "text": "A", "assignee": "@amy" }, { "id": "b", "text": "B" }] });
        let after = json!({ "todos": [{ "id": "a", "text": "A", "assignee": "@amy" }, { "id": "b", "text": "B", "assignee": "@bob" }, { "id": "c", "text": "C", "assignee": "" }] });
        assert_eq!(newly_assigned(&before, &after), vec![("@bob".to_string(), "B".to_string())]);
    }
}

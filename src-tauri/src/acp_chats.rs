//! Saved conversations with ACP agents, one JSON file each under `<data>/acp-chats/<server>/`.
//! The app keeps what was shown (the lines) so a conversation can be read again later; the agent's
//! own `sessionId` is kept too, to pick the conversation up with `session/load` when the agent can.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AcpChatMeta {
    pub id: String,
    pub agent: String,
    pub title: String,
    /// Seconds since 1970.
    pub updated: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AcpChat {
    pub id: String,
    pub agent: String,
    pub title: String,
    pub updated: f64,
    /// The agent's session, for `session/load`.
    #[serde(default)]
    pub session_id: String,
    /// What the screen showed, as JSON text (the app's own line format).
    #[serde(default)]
    pub lines: String,
}

/// Chat ids the app makes: letters, digits and dashes.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|letter| letter.is_ascii_alphanumeric() || letter == '-')
}

fn safe(server: &str) -> String {
    server.chars().map(|letter| if letter.is_ascii_alphanumeric() || letter == '-' || letter == '_' { letter } else { '_' }).collect()
}

pub fn folder(root: &Path, server: &str) -> PathBuf {
    root.join("acp-chats").join(safe(server))
}

pub fn list(root: &Path, server: &str) -> Vec<AcpChatMeta> {
    let Ok(entries) = fs::read_dir(folder(root, server)) else { return Vec::new() };
    let mut out: Vec<AcpChatMeta> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|one| one == "json"))
        .filter_map(|entry| crate::store::read::<AcpChat>(&entry.path()).ok().flatten())
        .map(|one| AcpChatMeta { id: one.id, agent: one.agent, title: one.title, updated: one.updated })
        .collect();
    out.sort_by(|a, b| b.updated.total_cmp(&a.updated));
    out
}

pub fn read(root: &Path, server: &str, id: &str) -> Result<AcpChat, String> {
    if !valid_id(id) {
        return Err("Unknown conversation.".into());
    }
    crate::store::read::<AcpChat>(&folder(root, server).join(format!("{id}.json")))?.ok_or_else(|| "That conversation is gone.".into())
}

pub fn save(root: &Path, server: &str, chat: &AcpChat) -> Result<(), String> {
    if !valid_id(&chat.id) {
        return Err("Unknown conversation.".into());
    }
    if chat.lines.len() > 8 * 1024 * 1024 {
        return Err("The conversation is too long to save.".into());
    }
    let dir = folder(root, server);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let text = serde_json::to_string(chat).map_err(|error| error.to_string())?;
    crate::store::write(&dir.join(format!("{}.json", chat.id)), &text).map_err(|error| format!("Could not save the conversation: {error}"))
}

pub fn delete(root: &Path, server: &str, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err("Unknown conversation.".into());
    }
    let path = folder(root, server).join(format!("{id}.json"));
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_lists_and_deletes() {
        let root = std::env::temp_dir().join(format!("monolab-acp-chats-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let chat = |id: &str, updated: f64| AcpChat {
            id: id.into(),
            agent: "gemini".into(),
            title: format!("chat {id}"),
            updated,
            session_id: "s1".into(),
            lines: "[]".into(),
        };
        save(&root, "server/1", &chat("a1", 10.0)).unwrap();
        save(&root, "server/1", &chat("b2", 20.0)).unwrap();
        let listed = list(&root, "server/1");
        assert_eq!(listed.iter().map(|one| one.id.as_str()).collect::<Vec<_>>(), ["b2", "a1"]);
        assert_eq!(read(&root, "server/1", "a1").unwrap().session_id, "s1");
        assert!(read(&root, "server/1", "../x").is_err());
        delete(&root, "server/1", "a1").unwrap();
        assert_eq!(list(&root, "server/1").len(), 1);
        let _ = fs::remove_dir_all(&root);
    }
}

//! The studio's resource pack on Pumpkin. The app builds it into `studio/pack/monolab.zip` and says in `pack.json`
//! how players get it: `url` (an address the user uploaded the zip to) or `self`, where the fork serves the zip on
//! the game port itself (`server.host-resource-pack`) and each player gets the address they joined with. The `port`
//! field of `self` is the Paper add-on's own web server and means nothing here. Like on Paper, the pack goes next to
//! the server's own pack (`pumpkin.toml`), never instead of it, and a rebuilt pack replaces the old one.

use pumpkin_plugin_api::player::JavaResourcePack;
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::uuid::Uuid;
use pumpkin_plugin_api::{Player, Server};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Wanted {
    /// The pack's address; none = served from this server.
    pub url: Option<String>,
    pub sha1: String,
    pub required: bool,
}

pub struct Pack {
    file: PathBuf,
    zip: PathBuf,
    stamp: Option<u64>,
    wanted: Option<Wanted>,
    /// The SHA-1 of the zip we serve now (`self`), so a rebuilt one replaces it.
    hosted: Option<String>,
    /// What each player was sent, by uuid.
    sent: HashMap<(u64, u64), Wanted>,
}

impl Pack {
    pub fn new(studio_root: &Path) -> Self {
        let folder = studio_root.join("pack");
        Self { file: folder.join("pack.json"), zip: folder.join("monolab.zip"), stamp: None, wanted: None, hosted: None, sent: HashMap::new() }
    }

    /// Rereads `pack.json` when it changed.
    pub fn check(&mut self) {
        let stamp = fs::metadata(&self.file)
            .ok()
            .map(|meta| meta.modified().ok().and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |at| at.as_millis() as u64) ^ meta.len())
            .unwrap_or(0);
        if self.stamp == Some(stamp) {
            return;
        }
        self.stamp = Some(stamp);
        self.wanted = fs::read_to_string(&self.file).ok().and_then(|text| wanted(&text));
    }

    /// Serves the built zip for `self` (and stops serving an old build). Returns the SHA-1 players check it with:
    /// the zip's own, which is the one `pack.json` names unless the zip was rebuilt since.
    fn host(&mut self, server: &Server, sha1: &str) -> Option<String> {
        if let Some(hosted) = &self.hosted
            && hosted == sha1
        {
            return Some(hosted.clone());
        }
        let Ok(bytes) = fs::read(&self.zip) else {
            tracing::warn!("The studio pack isn't built yet ({}): build it in the app.", self.zip.display());
            return None;
        };
        let served = server.host_resource_pack(&bytes).to_ascii_lowercase();
        if let Some(old) = self.hosted.replace(served.clone())
            && old != served
        {
            server.unhost_resource_pack(&old);
        }
        Some(served)
    }

    /// Stops serving the zip (sending turned off, or the add-on stops).
    pub fn unhost(&mut self, server: &Server) {
        if let Some(old) = self.hosted.take() {
            server.unhost_resource_pack(&old);
        }
    }

    /// Sends the pack to whoever doesn't have the current one (just joined, or it was rebuilt), and takes it off
    /// players when sending was turned off.
    pub async fn send(&mut self, server: &Server, players: &[&Player]) {
        let wanted = match self.wanted.clone() {
            Some(Wanted { url: None, sha1, required }) => self.host(server, &sha1).map(|sha1| Wanted { url: None, sha1, required }),
            other => {
                self.unhost(server);
                other
            }
        };
        for player in players {
            let id = player.get_id();
            let key = (id.high, id.low);
            if self.sent.get(&key) == wanted.as_ref() {
                continue;
            }
            let Some(java) = player.as_java() else { continue };
            if let Some(old) = self.sent.remove(&key) {
                java.remove_resource_pack(pack_id(&old.sha1)).await;
            }
            let Some(next) = &wanted else { continue };
            let url = match &next.url {
                Some(url) => url.clone(),
                None => match java.get_hosted_resource_pack_url(&next.sha1) {
                    Some(url) => url,
                    None => continue,
                },
            };
            java.send_resource_pack(JavaResourcePack {
                id: pack_id(&next.sha1),
                url,
                hash: next.sha1.clone(),
                forced: next.required,
                prompt_message: Some(TextComponent::text("This server's own items, sounds and pictures.")),
            })
            .await;
            self.sent.insert(key, next.clone());
        }
    }

    pub fn left(&mut self, player: &Player) {
        let id = player.get_id();
        self.sent.remove(&(id.high, id.low));
    }
}

/// What `pack.json` asks to send: `url` with an http(s) address, or `self`, for a built pack.
pub fn wanted(text: &str) -> Option<Wanted> {
    let data: Value = serde_json::from_str(text).ok()?;
    let sha1 = data.get("sha1")?.as_str()?.to_ascii_lowercase();
    let sending = data.get("sending")?;
    if sha1.len() != 40 || !sha1.bytes().all(|one| one.is_ascii_hexdigit()) {
        return None;
    }
    let required = sending.get("required").and_then(Value::as_bool) == Some(true);
    match sending.get("mode")?.as_str()? {
        "self" => Some(Wanted { url: None, sha1, required }),
        "url" => {
            let url = sending.get("address")?.as_str()?.trim().to_string();
            (url.starts_with("https://") || url.starts_with("http://")).then_some(Wanted { url: Some(url), sha1, required })
        }
        _ => None,
    }
}

/// One id per pack build (from its hash), so a rebuilt pack is a new pack the old one can be taken off for.
pub fn pack_id(sha1: &str) -> Uuid {
    let half = |at: usize| sha1.get(at..at + 16).and_then(|hex| u64::from_str_radix(hex, 16).ok()).unwrap_or(0);
    // A name-based (version 3-style) uuid: version and variant bits set.
    Uuid { high: (half(0) & !0xF000) | 0x3000, low: (half(16) & !(0b11 << 62)) | (0b10 << 62) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "66d9dcd3d91de592f730bf76627c7f6b0083b080";

    #[test]
    fn reads_how_to_send() {
        let make = |mode: &str, address: &str| format!(r#"{{"sha1":"{SHA}","sending":{{"mode":"{mode}","port":25590,"address":"{address}","required":true}}}}"#);
        assert_eq!(wanted(&make("url", " https://example.com/p.zip ")), Some(Wanted { url: Some("https://example.com/p.zip".into()), sha1: SHA.into(), required: true }));
        assert_eq!(wanted(&make("self", "")), Some(Wanted { url: None, sha1: SHA.into(), required: true }));
        assert_eq!(wanted(&make("off", "https://example.com/p.zip")), None);
        assert_eq!(wanted(&make("url", "ftp://example.com/p.zip")), None);
        assert_eq!(wanted(r#"{"sha1":"abc","sending":{"mode":"self"}}"#), None);
        assert_eq!(wanted("not json"), None);
    }

    #[test]
    fn ids_follow_the_build() {
        let id = pack_id(SHA);
        assert_eq!(id.high >> 12 & 0xF, 3);
        assert_eq!(id.low >> 62, 0b10);
        let same = pack_id(SHA);
        assert_eq!((id.high, id.low), (same.high, same.low));
        let other = pack_id("0000000000000000000000000000000000000001");
        assert_ne!((id.high, id.low), (other.high, other.low));
    }
}

//! Studio maker M0 (`docs/studio-maker.md`): the studio's own assets (`studio/assets/`) built into one resource
//! pack (`studio/pack/monolab.zip`) that the add-on hands to players. The app builds it, so Paper, Pumpkin, SSH and
//! panel servers all get the same pack, and the editor's previews can follow the same rules.
//!
//! - `assets/textures/<name>.png` (+ `<name>.png.mcmeta` for animation) → `monolab:studio/<name>`
//! - `assets/models/<name>.json` → `monolab:studio/<name>` (Blockbench `.bbmodel` → `studio_pack_models.rs`)
//! - `assets/sounds/<name>.ogg` → sound event `monolab:studio.<name>`
//!
//! The zip is the same bytes for the same assets (sorted, fixed times), so an unchanged pack keeps its SHA-1 and
//! players aren't asked to download it again.

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;
use std::io::{Cursor, Write};

pub const NAMESPACE: &str = "monolab";
pub const FOLDERS: [&str; 3] = ["textures", "models", "sounds"];
pub const MOST_TEXTURE: usize = 2 * 1024 * 1024;
pub const MOST_SOUND: usize = 8 * 1024 * 1024;
pub const MOST_MODEL: usize = 512 * 1024;
/// Blockbench files carry their pictures inside, so they get more room.
pub const MOST_BBMODEL: usize = 8 * 1024 * 1024;
/// The whole pack. Clients refuse packs much over 250 MB anyway; a studio pack this big is a mistake.
pub const MOST_PACK: usize = 100 * 1024 * 1024;

/// What a folder takes: (file ending, most bytes).
pub fn kind_of(folder: &str) -> Option<(&'static [&'static str], usize)> {
    match folder {
        "textures" => Some((&[".png", ".png.mcmeta"], MOST_TEXTURE)),
        "models" => Some((&[".json", ".bbmodel"], MOST_MODEL)),
        "sounds" => Some((&[".ogg"], MOST_SOUND)),
        _ => None,
    }
}

/// `ruby_sword.png` in `textures` → `ruby_sword`. Names follow the studio's file names (a-z, 0-9, `-`, `_`).
pub fn asset_name<'a>(folder: &str, file: &'a str) -> Option<&'a str> {
    let (endings, _) = kind_of(folder)?;
    let name = endings.iter().find_map(|ending| file.strip_suffix(ending))?;
    crate::studio::valid_id(name).then_some(name)
}

/// Checks an upload before it is written.
pub fn check_upload(folder: &str, file: &str, bytes: &[u8]) -> Result<(), String> {
    let Some((_, most)) = kind_of(folder) else { return Err("Assets go in textures, models or sounds.".into()) };
    if asset_name(folder, file).is_none() {
        return Err(format!("{file}: names use a-z, 0-9, - and _ (up to 48), ending in {}.", kind_of(folder).map(|(one, _)| one.join(" or ")).unwrap_or_default()));
    }
    let most = if file.ends_with(".bbmodel") { MOST_BBMODEL } else { most };
    if bytes.len() > most {
        return Err(if most >= 1024 * 1024 { format!("{file} is over {} MB.", most / 1024 / 1024) } else { format!("{file} is over {} KB.", most / 1024) });
    }
    let fits = match folder {
        "textures" if file.ends_with(".png") => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "textures" | "models" => serde_json::from_slice::<serde_json::Value>(bytes).is_ok(),
        "sounds" => bytes.starts_with(b"OggS"),
        _ => false,
    };
    if !fits {
        return Err(format!("{file} isn't a real {}.", if file.ends_with(".png") { "PNG" } else if file.ends_with(".ogg") { "Ogg sound" } else { "JSON file" }));
    }
    Ok(())
}

/// The resource pack format for a Minecraft version, and the oldest the pack says it works with. Newer versions than
/// the table get the newest known number plus an open range, which clients accept.
pub fn pack_format(version: &str) -> u32 {
    let mut parts = version.split(|one: char| !one.is_ascii_digit()).filter(|one| !one.is_empty()).map(|one| one.parse::<u32>().unwrap_or(0));
    let (major, minor, patch) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    if major >= 26 {
        return 75;
    }
    match (minor, patch) {
        (21, 11..) => 75,
        (21, 9 | 10) => 69,
        (21, 7 | 8) => 64,
        (21, 6) => 63,
        (21, 5) => 55,
        (21, 4) => 46,
        (21, 2 | 3) => 42,
        (21, _) => 34,
        (20, 5 | 6) => 32,
        _ => 34,
    }
}

pub fn mcmeta(version: &str) -> String {
    let format = pack_format(version);
    // pack_format + supported_formats for clients before 1.21.9, min/max_format for 1.21.9 and later.
    serde_json::to_string_pretty(&serde_json::json!({
        "pack": {
            "description": "Monolab studio",
            "pack_format": format,
            "supported_formats": [34, 999],
            "min_format": 34,
            "max_format": 999,
        }
    }))
    .unwrap_or_default()
}

/// One file the pack is built from: folder, file name, bytes.
pub type Source = (String, String, Vec<u8>);

/// How the add-on gets the pack to players. Kept in `pack.json` next to the zip.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Sending {
    /// `self`: the add-on's own small web server · `url`: an address the user uploads the zip to · `off`.
    pub mode: String,
    /// The add-on's web server port (`self`).
    pub port: u16,
    /// `url` mode's address, or for `self` a fixed public host (empty = the address the player joined with).
    pub address: String,
    /// Players who decline are kicked.
    pub required: bool,
}

impl Default for Sending {
    fn default() -> Self {
        Self { mode: "off".into(), port: 25590, address: String::new(), required: false }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PackInfo {
    pub schema: u32,
    /// SHA-1 of the zip (hex), what the client checks. Empty before the first build.
    pub sha1: String,
    pub bytes: u64,
    /// Unix seconds.
    pub built: u64,
    pub format: u32,
    /// How many textures, models and sounds went in.
    pub counts: BTreeMap<String, u32>,
    #[serde(default)]
    pub sending: Sending,
}

/// A sound bigger than this streams from disk in the game (`"stream": true`).
const STREAM_BYTES: usize = 256 * 1024;

/// The pack zip. Unknown or misnamed files are skipped (the screen only lets good ones in).
/// `extra`: files other studio kinds add (item models, fonts), by their path in the pack; they win over assets.
pub fn build(version: &str, sources: &[Source], extra: &[(String, Vec<u8>)]) -> Result<(Vec<u8>, PackInfo), String> {
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut sounds = serde_json::Map::new();
    files.insert("pack.mcmeta".into(), mcmeta(version).into_bytes());
    // A model only finds textures that are in an atlas, and the game's atlases list just its own folders
    // (`block/`, `item/`, …): add ours (`studio/` assets, `model/` Blockbench skins) to the block atlas. Only that
    // one: items take block-atlas textures too (like saplings), but 26.x rejects block models whose texture is also in
    // the item atlas (it keeps one copy of a sprite and may pick the item one).
    let atlas = serde_json::to_vec_pretty(&serde_json::json!({
        "sources": [
            { "type": "directory", "source": "studio", "prefix": "studio/" },
            { "type": "directory", "source": "model", "prefix": "model/" },
        ]
    }))
    .unwrap_or_default();
    files.insert("assets/minecraft/atlases/blocks.json".into(), atlas);
    for (folder, file, bytes) in sources {
        let Some(name) = asset_name(folder, file) else { continue };
        let target = match folder.as_str() {
            "textures" => format!("assets/{NAMESPACE}/textures/studio/{name}{}", if file.ends_with(".mcmeta") { ".png.mcmeta" } else { ".png" }),
            "models" if file.ends_with(".json") => format!("assets/{NAMESPACE}/models/studio/{name}.json"),
            "sounds" => {
                // Long sounds (music discs, over 256 KB) stream, as the game's own music does, instead of loading whole.
                let sound = if bytes.len() > STREAM_BYTES { serde_json::json!({ "name": format!("{NAMESPACE}:studio/{name}"), "stream": true }) } else { serde_json::json!(format!("{NAMESPACE}:studio/{name}")) };
                sounds.insert(format!("studio.{name}"), serde_json::json!({ "sounds": [sound] }));
                format!("assets/{NAMESPACE}/sounds/studio/{name}.ogg")
            }
            _ => continue,
        };
        if !file.ends_with(".mcmeta") {
            *counts.entry(folder.clone()).or_default() += 1;
        }
        files.insert(target, bytes.clone());
    }
    if !sounds.is_empty() {
        files.insert(format!("assets/{NAMESPACE}/sounds.json"), serde_json::to_vec_pretty(&sounds).unwrap_or_default());
    }
    for (path, bytes) in extra {
        if path.starts_with("assets/") {
            files.insert(path.clone(), bytes.clone());
        }
    }
    let mut out = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut out);
        let stamp = zip::DateTime::default();
        for (path, bytes) in &files {
            // Pictures and sounds are already compressed; storing them is faster and the zip barely grows.
            let method = if path.ends_with(".png") || path.ends_with(".ogg") { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated };
            let options = zip::write::SimpleFileOptions::default().compression_method(method).last_modified_time(stamp);
            zip.start_file(path.as_str(), options).map_err(|error| error.to_string())?;
            zip.write_all(bytes).map_err(|error| error.to_string())?;
        }
        zip.finish().map_err(|error| error.to_string())?;
    }
    let bytes = out.into_inner();
    if bytes.len() > MOST_PACK {
        return Err("The studio pack would be over 100 MB. Remove some sounds or big pictures.".into());
    }
    let sha1 = Sha1::digest(&bytes).iter().map(|one| format!("{one:02x}")).collect();
    let info = PackInfo { schema: 1, sha1, bytes: bytes.len() as u64, built: 0, format: pack_format(version), counts, sending: Sending::default() };
    Ok((bytes, info))
}

/// Numbers that must never change once given out (glyph characters, block states, model data), by kind and name.
/// A removed name keeps its number so it is never handed to something else (M1/M2 use this).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Ids {
    pub schema: u32,
    /// kind → name → number
    pub given: BTreeMap<String, BTreeMap<String, u32>>,
    /// kind → the next number to give (so a removed name's number is never given again)
    #[serde(default)]
    pub next: BTreeMap<String, u32>,
}

impl Ids {
    /// The name's number, giving it the next free one (from `first`) the first time.
    pub fn number(&mut self, kind: &str, name: &str, first: u32) -> u32 {
        let given = self.given.entry(kind.to_string()).or_default();
        if let Some(one) = given.get(name) {
            return *one;
        }
        let seen = given.values().copied().max().map_or(first, |most| most + 1);
        let next = self.next.get(kind).copied().unwrap_or(first).max(seen).max(first);
        given.insert(name.to_string(), next);
        self.next.insert(kind.to_string(), next + 1);
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::RgbaImage::new(16, 16).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn same_assets_same_pack() {
        let sources = vec![
            ("textures".to_string(), "ruby.png".to_string(), png()),
            ("sounds".to_string(), "ding.ogg".to_string(), b"OggS....".to_vec()),
            ("models".to_string(), "ruby.json".to_string(), br#"{"parent":"item/generated"}"#.to_vec()),
            ("models".to_string(), "dragon.bbmodel".to_string(), b"{}".to_vec()),
            ("textures".to_string(), "Bad Name.png".to_string(), png()),
        ];
        let (first, info) = build("1.21.4", &sources, &[]).unwrap();
        let (again, again_info) = build("1.21.4", &sources, &[]).unwrap();
        assert_eq!(first, again);
        assert_eq!(info.sha1, again_info.sha1);
        assert_eq!(info.format, 46);
        assert_eq!(info.counts.get("textures"), Some(&1));
        assert_eq!(info.counts.get("models"), Some(&1));
        let mut zip = zip::ZipArchive::new(Cursor::new(first)).unwrap();
        let names: Vec<String> = (0..zip.len()).map(|at| zip.by_index(at).unwrap().name().unwrap().to_string()).collect();
        assert!(names.contains(&"assets/monolab/textures/studio/ruby.png".to_string()), "{names:?}");
        assert!(names.contains(&"assets/monolab/sounds/studio/ding.ogg".to_string()));
        assert!(names.contains(&"assets/monolab/sounds.json".to_string()));
        assert!(!names.iter().any(|one| one.contains("dragon") || one.contains("Bad")));
        let atlas: serde_json::Value = serde_json::from_reader(zip.by_name("assets/minecraft/atlases/blocks.json").unwrap()).unwrap();
        assert_eq!(atlas["sources"][0]["source"], "studio");
        assert_eq!(atlas["sources"][1]["prefix"], "model/");
        assert!(!names.contains(&"assets/minecraft/atlases/items.json".to_string()));
        let sounds: serde_json::Value = serde_json::from_reader(zip.by_name("assets/monolab/sounds.json").unwrap()).unwrap();
        assert_eq!(sounds["studio.ding"]["sounds"][0], "monolab:studio/ding");
    }

    #[test]
    fn uploads_are_checked() {
        assert!(check_upload("textures", "ruby.png", &png()).is_ok());
        assert!(check_upload("textures", "ruby.png", b"not a png").is_err());
        assert!(check_upload("textures", "Ruby.png", &png()).is_err());
        assert!(check_upload("textures", "ruby.png.mcmeta", br#"{"animation":{}}"#).is_ok());
        assert!(check_upload("sounds", "ding.ogg", b"OggS").is_ok());
        assert!(check_upload("sounds", "ding.mp3", b"ID3").is_err());
        assert!(check_upload("models", "x.json", b"{").is_err());
        assert!(check_upload("worlds", "x.png", &png()).is_err());
        assert!(check_upload("textures", "big.png", &vec![0; MOST_TEXTURE + 1]).is_err());
    }

    #[test]
    fn formats_by_version() {
        assert_eq!(pack_format("1.21.1"), 34);
        assert_eq!(pack_format("1.21.8"), 64);
        assert_eq!(pack_format("26.3"), 75);
        assert!(mcmeta("1.21.4").contains("\"pack_format\": 46"));
    }

    #[test]
    fn numbers_never_move() {
        let mut ids = Ids::default();
        assert_eq!(ids.number("glyph", "ruby", 0xE000), 0xE000);
        assert_eq!(ids.number("glyph", "coin", 0xE000), 0xE001);
        ids.given.get_mut("glyph").unwrap().remove("coin");
        assert_eq!(ids.number("glyph", "gem", 0xE000), 0xE002);
        assert_eq!(ids.number("glyph", "ruby", 0xE000), 0xE000);
        assert_eq!(ids.number("glyph", "coin", 0xE000), 0xE003);
    }
}

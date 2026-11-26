//! Studio M6 (`docs/studio-maker.md`): sharing what was made. A bundle (`.monolab`, a zip) carries studio files, the assets
//! they use and their pack numbers, so a set of items, models or mobs moves to another server in one file:
//!
//! - `bundle.json`: `{ "format": 1, "name", "made": <unix secs>, "resources": [{kind,id}], "assets": [{folder,name}], "ids": {...} }`
//! - `studio/<kind>/<id>.json`: the files, as they are on the server
//! - `studio/assets/<folder>/<file>`: textures, models and sounds they use
//!
//! `ids` is the part of `studio/pack/ids.json` for these files. On import a number is kept only when the target hasn't
//! given it out; otherwise the pack builder hands out a new one, as for a new file. Exporting pulls in what the
//! chosen files point at (a mob's model, loot and equipment, a menu's items, the glyphs a dialog draws...).

use crate::studio_pack::{self, Ids};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};
use tauri::AppHandle;

pub const FORMAT: u32 = 1;
/// The bundle file, and everything in it unpacked.
pub const MOST_BUNDLE: u64 = 100 * 1024 * 1024;
const MOST_ENTRIES: usize = 10_000;
/// Files one export may pull in, references and all.
const MOST_RESOURCES: usize = 2_000;
pub const EXTENSION: &str = "monolab";
const MANIFEST: &str = "bundle.json";

/// One studio file: `studio/<kind>/<id>.json`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioRef {
    pub kind: String,
    pub id: String,
}

impl StudioRef {
    pub fn new(kind: &str, id: &str) -> Self {
        Self { kind: kind.to_string(), id: id.to_string() }
    }

    fn label(&self) -> String {
        format!("{}/{}", self.kind, self.id)
    }
}

/// One asset file: `studio/assets/<folder>/<name>` (`name` is the file name, `ruby.png`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioBundleAsset {
    pub folder: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub made: u64,
    #[serde(default)]
    pub resources: Vec<StudioRef>,
    #[serde(default)]
    pub assets: Vec<StudioBundleAsset>,
    #[serde(default)]
    pub ids: Ids,
}

/// What an export or import did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioBundleInfo {
    pub name: String,
    /// The bundle file on this PC.
    pub path: String,
    pub bytes: u64,
    /// Every file that went in (export) or was written (import).
    pub resources: Vec<StudioRef>,
    pub assets: Vec<StudioBundleAsset>,
    /// Export: files that came along because a chosen one points at them ("also included").
    pub also: Vec<StudioRef>,
    /// Chosen files that couldn't be read, with why.
    pub missing: Vec<String>,
    /// Import: "kind/id" and "assets/folder/name" left alone (they were there already, or the server can't use them).
    pub skipped: Vec<String>,
    /// Import: the running server took the new files at once.
    pub applied: bool,
}

/// What a bundle holds, before importing it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StudioBundlePeek {
    pub name: String,
    pub made: u64,
    pub resources: Vec<StudioRef>,
    pub assets: Vec<StudioBundleAsset>,
    /// "kind/id" or "assets/folder/name" that already exist on the server.
    pub clashes: Vec<String>,
    /// Files in the bundle that can't be used, with why.
    pub problems: Vec<String>,
}

/// A bundle read back: its files, checked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bundle {
    pub manifest: Manifest,
    pub resources: Vec<(StudioRef, String)>,
    /// (folder, file name, bytes)
    pub assets: Vec<(String, String, Vec<u8>)>,
    pub problems: Vec<String>,
}

impl Bundle {
    fn refs(&self) -> Vec<StudioRef> {
        self.resources.iter().map(|(one, _)| one.clone()).collect()
    }

    fn asset_list(&self) -> Vec<StudioBundleAsset> {
        self.assets.iter().map(|(folder, name, _)| StudioBundleAsset { folder: folder.clone(), name: name.clone() }).collect()
    }
}

// ---------- pack numbers ----------

/// The `ids.json` entries (ids kind, name) a studio file owns, as the pack builders name them.
pub fn ids_keys(kind: &str, id: &str) -> Vec<(&'static str, String)> {
    match kind {
        "item" => vec![("cmd", id.to_string())],
        "block" => vec![
            (studio_format::block::IDS_CUBE, id.to_string()),
            (studio_format::block::IDS_PLANT, id.to_string()),
            ("cmd", format!("block:{id}")),
        ],
        "furniture" => vec![("cmd", format!("{}{id}", studio_format::furniture::CMD_PREFIX))],
        "font" => vec![("glyph", id.to_string())],
        _ => Vec::new(),
    }
}

/// The numbers these files have, for `bundle.json`.
pub fn ids_subset(ids: &Ids, refs: &[StudioRef]) -> Ids {
    let mut out = Ids { schema: 1, ..Default::default() };
    for one in refs {
        for (kind, name) in ids_keys(&one.kind, &one.id) {
            if let Some(number) = ids.given.get(kind).and_then(|given| given.get(&name)) {
                out.given.entry(kind.to_string()).or_default().insert(name, *number);
            }
        }
    }
    out
}

/// Takes the bundle's numbers for the written files where the target is free: the name has none there yet, no other
/// name has the number, and it isn't below what the target gives out next (a number once given is never reused).
/// The rest are left to the pack builder. True when `target` changed.
pub fn merge_ids(target: &mut Ids, from: &Ids, written: &[StudioRef]) -> bool {
    let mut changed = false;
    for one in written {
        for (kind, name) in ids_keys(&one.kind, &one.id) {
            let Some(number) = from.given.get(kind).and_then(|given| given.get(&name)).copied() else { continue };
            let next = target.next.get(kind).copied().unwrap_or(0);
            let given = target.given.entry(kind.to_string()).or_default();
            if given.contains_key(&name) || number < next || given.values().any(|taken| *taken == number) {
                continue;
            }
            given.insert(name, number);
            target.next.insert(kind.to_string(), number + 1);
            changed = true;
        }
    }
    changed
}

// ---------- what a file points at ----------

/// What one studio file uses: other studio files, asset names by folder, and glyph names (`:ruby:`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Refs {
    pub resources: BTreeSet<StudioRef>,
    pub textures: BTreeSet<String>,
    pub models: BTreeSet<String>,
    pub sounds: BTreeSet<String>,
    pub glyphs: BTreeSet<String>,
}

impl Refs {
    fn resource(&mut self, kind: &str, id: &str) {
        let id = id.trim();
        if studio_format::valid_kind(kind) && studio_format::valid_id(id) {
            self.resources.insert(StudioRef::new(kind, id));
        }
    }

    fn asset(set: &mut BTreeSet<String>, name: &str) {
        let name = name.trim();
        let name =
            name.strip_suffix(".png").or_else(|| name.strip_suffix(".bbmodel")).or_else(|| name.strip_suffix(".json")).unwrap_or(name);
        if studio_format::valid_id(name) {
            set.insert(name.to_string());
        }
    }

    /// `studio:<id>` (an item) or `studio:<kind>/<id>`, the way the give action and equipment name them.
    fn studio_item(&mut self, rest: &str) {
        match rest.split_once('/') {
            Some((kind, id)) => self.resource(kind, id),
            None => self.resource("item", rest),
        }
    }
}

fn str_of<'a>(data: &'a Map<String, Value>, key: &str) -> &'a str {
    data.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// `:name:` glyph tokens in a text.
fn glyph_tokens(text: &str, out: &mut BTreeSet<String>) {
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() < 3 {
        return;
    }
    for name in &parts[1..parts.len() - 1] {
        if studio_format::glyphs::valid_name(name) {
            out.insert((*name).to_string());
        }
    }
}

fn walk(value: &Value, refs: &mut Refs) {
    match value {
        Value::String(text) => {
            if let Some(rest) = text.trim().strip_prefix("studio:") {
                refs.studio_item(rest);
            } else if let Some(sound) = text.trim().strip_prefix("monolab:studio.") {
                Refs::asset(&mut refs.sounds, sound);
            }
            glyph_tokens(text, &mut refs.glyphs);
        }
        Value::Array(list) => list.iter().for_each(|one| walk(one, refs)),
        Value::Object(map) => {
            // An action or condition step names other files by its type.
            let step = str_of(map, "type");
            let named = match step {
                "menu" | "dialog" | "loot" | "advancement" => Some((step, step)),
                "animation" => Some(("model", "model")),
                "summon" => Some(("mob", "mob")),
                _ => None,
            };
            if let Some((kind, key)) = named {
                refs.resource(kind, str_of(map, key));
            }
            if let Some(glyph) = map.get("glyph").and_then(Value::as_str) {
                if studio_format::glyphs::valid_name(glyph.trim()) {
                    refs.glyphs.insert(glyph.trim().to_string());
                }
            }
            map.values().for_each(|one| walk(one, refs));
        }
        _ => {}
    }
}

/// What a studio file points at. Names only: the caller keeps those that exist.
pub fn references(kind: &str, data: &Map<String, Value>) -> Refs {
    let mut refs = Refs::default();
    walk(&Value::Object(data.clone()), &mut refs);
    let look = data.get("look").and_then(Value::as_object);
    match kind {
        "item" | "furniture" => {
            if let Some(look) = look {
                Refs::asset(&mut refs.textures, str_of(look, "texture"));
                Refs::asset(&mut refs.models, str_of(look, "model"));
                for frame in studio_format::fields::lines(look, "pulling") {
                    Refs::asset(&mut refs.textures, &frame);
                }
            }
            if let Some(equip) = data.get("equip").and_then(Value::as_object) {
                Refs::asset(&mut refs.textures, str_of(equip, "texture"));
                Refs::asset(&mut refs.textures, str_of(equip, "legsTexture"));
            }
        }
        "block" => {
            if let Some(look) = look {
                for key in ["texture", "top", "bottom"] {
                    Refs::asset(&mut refs.textures, str_of(look, key));
                }
                Refs::asset(&mut refs.models, str_of(look, "model"));
            }
        }
        "font" => Refs::asset(&mut refs.textures, str_of(data, "image")),
        "model" => Refs::asset(&mut refs.models, str_of(data, "model")),
        "mob" => {
            refs.resource("model", str_of(data, "model"));
            refs.resource("loot", str_of(data, "loot"));
        }
        _ => {}
    }
    refs
}

/// Textures (and parent models) a JSON model asset uses: `monolab:studio/<name>`.
pub fn model_assets(bytes: &[u8]) -> BTreeSet<String> {
    fn find(value: &Value, out: &mut BTreeSet<String>) {
        match value {
            Value::String(text) => {
                if let Some(name) = text.strip_prefix("monolab:studio/") {
                    Refs::asset(out, name);
                }
            }
            Value::Array(list) => list.iter().for_each(|one| find(one, out)),
            Value::Object(map) => map.values().for_each(|one| find(one, out)),
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        find(&value, &mut out);
    }
    out
}

// ---------- the zip ----------

/// A path inside the bundle that stays inside it: relative, no `..`, no drive or backslash.
pub fn safe_entry(name: &str) -> bool {
    let trimmed = name.strip_suffix('/').unwrap_or(name);
    !trimmed.is_empty()
        && !trimmed.starts_with('/')
        && !trimmed.contains('\\')
        && !trimmed.contains(':')
        && !trimmed.contains('\0')
        && trimmed.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// The bundle's bytes. Same files, same bytes (sorted, fixed times).
pub fn write(manifest: &Manifest, resources: &[(StudioRef, String)], assets: &[(String, String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut files: BTreeMap<String, &[u8]> = BTreeMap::new();
    for (one, text) in resources {
        if !studio_format::valid_kind(&one.kind) || !studio_format::valid_id(&one.id) {
            return Err(format!("{} isn't a studio file name.", one.label()));
        }
        files.insert(format!("studio/{}/{}.json", one.kind, one.id), text.as_bytes());
    }
    for (folder, file, bytes) in assets {
        if studio_pack::asset_name(folder, file).is_none() {
            return Err(format!("{folder}/{file} isn't a studio asset."));
        }
        files.insert(format!("studio/assets/{folder}/{file}"), bytes.as_slice());
    }
    let head = serde_json::to_vec_pretty(manifest).map_err(|error| error.to_string())?;
    let mut out = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut out);
        let stamp = zip::DateTime::default();
        let options = |path: &str| {
            let method = if path.ends_with(".png") || path.ends_with(".ogg") {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            };
            zip::write::SimpleFileOptions::default().compression_method(method).last_modified_time(stamp)
        };
        zip.start_file(MANIFEST, options(MANIFEST)).map_err(|error| error.to_string())?;
        zip.write_all(&head).map_err(|error| error.to_string())?;
        for (path, bytes) in &files {
            zip.start_file(path.as_str(), options(path.as_str())).map_err(|error| error.to_string())?;
            zip.write_all(bytes).map_err(|error| error.to_string())?;
        }
        zip.finish().map_err(|error| error.to_string())?;
    }
    let bytes = out.into_inner();
    if bytes.len() as u64 > MOST_BUNDLE {
        return Err("The bundle would be over 100 MB. Leave out some sounds or big pictures.".into());
    }
    Ok(bytes)
}

/// Reads a bundle and checks every file in it the way saving one would. A path that leaves the bundle refuses the
/// whole file; a file that can't be used is left out and named in `problems`.
pub fn read(bytes: &[u8]) -> Result<Bundle, String> {
    if bytes.len() as u64 > MOST_BUNDLE {
        return Err("The bundle is over 100 MB.".into());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "This isn't a Monolab bundle (not a zip file).".to_string())?;
    crate::zips::check_budget(&mut archive, MOST_BUNDLE, MOST_ENTRIES)?;
    let mut manifest: Option<Manifest> = None;
    let mut resources: BTreeMap<StudioRef, String> = BTreeMap::new();
    let mut assets: BTreeMap<(String, String), Vec<u8>> = BTreeMap::new();
    let mut problems = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry.name().unwrap_or_default().to_string();
        if !safe_entry(&name) || entry.enclosed_name().is_none() {
            return Err(format!("The bundle has a file outside its folders ({name}), so it wasn't opened."));
        }
        if entry.is_dir() {
            continue;
        }
        let mut data = Vec::new();
        (&mut entry).take(MOST_BUNDLE + 1).read_to_end(&mut data).map_err(|_| format!("{name} in the bundle couldn't be read."))?;
        let parts: Vec<&str> = name.split('/').collect();
        match parts.as_slice() {
            [MANIFEST] => {
                let read: Manifest = serde_json::from_slice(&data).map_err(|_| "The bundle's bundle.json is broken.".to_string())?;
                if read.format > FORMAT {
                    return Err("This bundle was made by a newer Monolab. Update the app first.".into());
                }
                manifest = Some(read);
            }
            ["studio", "assets", folder, file] => match studio_pack::check_upload(folder, file, &data) {
                Ok(()) => {
                    assets.entry((folder.to_string(), file.to_string())).or_insert(data);
                }
                Err(problem) => problems.push(format!("assets/{folder}/{file}: {problem}")),
            },
            ["studio", kind, file] if file.ends_with(".json") => {
                let id = file.trim_end_matches(".json");
                if !studio_format::valid_kind(kind) {
                    problems.push(format!("{kind}/{id}: this Monolab has no \"{kind}\" kind."));
                    continue;
                }
                let Ok(text) = String::from_utf8(data) else {
                    problems.push(format!("{kind}/{id}: not text."));
                    continue;
                };
                match studio_format::check(id, &text) {
                    Ok(_) => {
                        resources.entry(StudioRef::new(kind, id)).or_insert(text);
                    }
                    Err(problem) => problems.push(format!("{kind}/{id}: {problem}")),
                }
            }
            _ => problems.push(format!("{name}: not a studio file, left out.")),
        }
    }
    let manifest = manifest.ok_or("This isn't a Monolab bundle (bundle.json is missing).")?;
    Ok(Bundle {
        manifest,
        resources: resources.into_iter().collect(),
        assets: assets.into_iter().map(|((folder, file), bytes)| (folder, file, bytes)).collect(),
        problems,
    })
}

/// "kind/id" and "assets/folder/name" in the bundle that the server already has.
pub fn clashes(bundle: &Bundle, has_resource: impl Fn(&StudioRef) -> bool, has_asset: impl Fn(&str, &str) -> bool) -> Vec<String> {
    let mut out: Vec<String> = bundle.resources.iter().filter(|(one, _)| has_resource(one)).map(|(one, _)| one.label()).collect();
    out.extend(
        bundle
            .assets
            .iter()
            .filter(|(folder, file, _)| has_asset(folder.as_str(), file.as_str()))
            .map(|(folder, file, _)| format!("assets/{folder}/{file}")),
    );
    out
}

/// What an import writes: everything but clashes (unless `replace`) and kinds the server's add-on doesn't run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub resources: Vec<(StudioRef, String)>,
    pub assets: Vec<(String, String, Vec<u8>)>,
    pub skipped: Vec<String>,
}

pub fn plan(bundle: Bundle, clashes: &[String], replace: bool, kinds: &[&str]) -> Plan {
    let mut out = Plan::default();
    for (one, text) in bundle.resources {
        let label = one.label();
        if !kinds.contains(&one.kind.as_str()) {
            out.skipped.push(format!("{label} (this server's add-on doesn't run {})", one.kind));
        } else if !replace && clashes.contains(&label) {
            out.skipped.push(label);
        } else {
            out.resources.push((one, text));
        }
    }
    for (folder, file, bytes) in bundle.assets {
        let label = format!("assets/{folder}/{file}");
        if !replace && clashes.contains(&label) {
            out.skipped.push(label);
        } else {
            out.assets.push((folder, file, bytes));
        }
    }
    out
}

// ---------- commands ----------

const NO_STUDIO: &str = "The studio needs a Paper, Purpur, Folia, Leaf or Pumpkin server.";

fn read_file(path: &str) -> Result<Vec<u8>, String> {
    let size = std::fs::metadata(path).map_err(|_| "The bundle file isn't there anymore.".to_string())?.len();
    if size > MOST_BUNDLE {
        return Err("The bundle is over 100 MB.".into());
    }
    std::fs::read(path).map_err(|error| format!("Could not read the bundle: {error}"))
}

async fn read_bundle(path: String) -> Result<Bundle, String> {
    tauri::async_runtime::spawn_blocking(move || read_file(&path).and_then(|bytes| read(&bytes)))
        .await
        .map_err(|error| error.to_string())?
}

/// What the server has: studio files of the bundle's kinds, and its assets.
async fn existing(
    app: &AppHandle,
    instance: &crate::ServerInstance,
    root: &str,
    bundle: &Bundle,
) -> (BTreeSet<StudioRef>, BTreeSet<(String, String)>) {
    let kinds: BTreeSet<&str> = bundle.resources.iter().map(|(one, _)| one.kind.as_str()).collect();
    let mut have = BTreeSet::new();
    for kind in kinds {
        for (file, dir, _) in crate::server_names(app, instance, &format!("{root}/{kind}")).await {
            if let Some(id) = file.strip_suffix(".json").filter(|id| !dir && studio_format::valid_id(id)) {
                have.insert(StudioRef::new(kind, id));
            }
        }
    }
    let assets = crate::studio_asset_rows(app, instance, root).await.into_iter().map(|row| (row.folder, row.file)).collect();
    (have, assets)
}

async fn read_ids(app: &AppHandle, instance: &crate::ServerInstance, root: &str) -> Ids {
    crate::server_text(app, instance, &format!("{root}/pack/ids.json"))
        .await
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Saves the chosen studio files, what they point at and (with `include_assets`) the assets they use as a
/// `.monolab` bundle at `path` on this PC.
#[tauri::command(async)]
#[specta::specta]
pub async fn studio_bundle_export(
    app: AppHandle,
    id: String,
    name: String,
    resources: Vec<StudioRef>,
    include_assets: bool,
    path: String,
) -> Result<StudioBundleInfo, String> {
    let instance = crate::instance_by_id(&app, &id)?;
    let root = crate::studio_root(&instance).ok_or(NO_STUDIO)?;
    let name: String = name.trim().chars().filter(|one| !one.is_control()).take(64).collect();
    if name.is_empty() {
        return Err("Give the bundle a name.".into());
    }
    if resources.is_empty() {
        return Err("Pick something to put in the bundle.".into());
    }
    let path = if path.to_lowercase().ends_with(&format!(".{EXTENSION}")) { path } else { format!("{path}.{EXTENSION}") };

    // Glyph names (:ruby:) point at font files by their name, not their id.
    let glyph_fonts: BTreeMap<String, String> = crate::studio_kind_files(&app, &instance, root, "font")
        .await
        .into_iter()
        .map(|(id, data)| (studio_format::glyphs::name_of(&id, &data), id))
        .collect();
    let asked: BTreeSet<StudioRef> = resources.into_iter().collect();
    let mut queue: Vec<StudioRef> = asked.iter().cloned().collect();
    let mut seen: BTreeSet<StudioRef> = asked.clone();
    let mut loaded: Vec<(StudioRef, String)> = Vec::new();
    let mut missing = Vec::new();
    let (mut textures, mut models, mut sounds) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    while let Some(one) = queue.pop() {
        let wanted = asked.contains(&one);
        let Ok(file) = crate::studio::path_of(root, &one.kind, &one.id) else {
            if wanted {
                missing.push(format!("{} (not a studio file name)", one.label()));
            }
            continue;
        };
        let text = match crate::server_text(&app, &instance, &file).await {
            Ok(Some(text)) => text,
            // A name that isn't a studio file (summon "zombie", a vanilla loot table...) is simply not one of ours.
            Ok(None) if !wanted => continue,
            Ok(None) => {
                missing.push(format!("{} (it is gone)", one.label()));
                continue;
            }
            Err(error) => {
                missing.push(format!("{} ({error})", one.label()));
                continue;
            }
        };
        let data = match studio_format::check(&one.id, &text) {
            Ok(data) => data,
            Err(problem) => {
                missing.push(format!("{} ({problem})", one.label()));
                continue;
            }
        };
        let refs = references(&one.kind, &data);
        let fonts = refs.glyphs.iter().filter_map(|glyph| glyph_fonts.get(glyph)).map(|id| StudioRef::new("font", id));
        for next in refs.resources.iter().cloned().chain(fonts) {
            if seen.len() < MOST_RESOURCES && seen.insert(next.clone()) {
                queue.push(next);
            }
        }
        textures.extend(refs.textures);
        models.extend(refs.models);
        sounds.extend(refs.sounds);
        loaded.push((one, text));
    }
    if loaded.is_empty() {
        return Err(format!("Nothing could be read: {}", missing.join(", ")));
    }
    loaded.sort_by(|a, b| a.0.cmp(&b.0));

    let mut assets: Vec<(String, String, Vec<u8>)> = Vec::new();
    if include_assets {
        let rows = crate::studio_asset_rows(&app, &instance, root).await;
        // Models first: a JSON model names the textures it uses.
        for folder in ["models", "textures", "sounds"] {
            let names = match folder {
                "models" => &models,
                "textures" => &textures,
                _ => &sounds,
            };
            let mut found = Vec::new();
            for row in rows.iter().filter(|row| row.folder == folder && names.contains(&row.name)) {
                let bytes =
                    crate::server_bytes(&app, &instance, &format!("{root}/assets/{folder}/{}", row.file), studio_pack::MOST_SOUND as u64)
                        .await?;
                found.push((folder.to_string(), row.file.clone(), bytes));
            }
            if folder == "models" {
                for (_, file, bytes) in &found {
                    if file.ends_with(".json") {
                        let used = model_assets(bytes);
                        textures.extend(used.iter().cloned());
                        models.extend(used);
                    }
                }
            }
            assets.extend(found);
        }
        // Parent models a JSON model named after the first pass.
        let have: BTreeSet<String> = assets.iter().filter(|(folder, _, _)| folder == "models").map(|(_, file, _)| file.clone()).collect();
        for row in rows.iter().filter(|row| row.folder == "models" && models.contains(&row.name) && !have.contains(&row.file)) {
            let bytes =
                crate::server_bytes(&app, &instance, &format!("{root}/assets/models/{}", row.file), studio_pack::MOST_SOUND as u64).await?;
            assets.push(("models".to_string(), row.file.clone(), bytes));
        }
        assets.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    }

    let refs: Vec<StudioRef> = loaded.iter().map(|(one, _)| one.clone()).collect();
    let ids = ids_subset(&read_ids(&app, &instance, root).await, &refs);
    let manifest = Manifest {
        format: FORMAT,
        name: name.clone(),
        made: crate::unix_now(),
        resources: refs.clone(),
        assets: assets.iter().map(|(folder, file, _)| StudioBundleAsset { folder: folder.clone(), name: file.clone() }).collect(),
        ids,
    };
    let asset_list = manifest.assets.clone();
    let target = path.clone();
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let bytes = write(&manifest, &loaded, &assets)?;
        std::fs::write(&target, &bytes).map_err(|error| format!("Could not save the bundle: {error}"))?;
        Ok::<_, String>(bytes.len() as u64)
    })
    .await
    .map_err(|error| error.to_string())??;
    let also = refs.iter().filter(|one| !asked.contains(*one)).cloned().collect();
    Ok(StudioBundleInfo { name, path, bytes, resources: refs, assets: asset_list, also, missing, ..Default::default() })
}

/// What a bundle on this PC holds and which of its files this server already has.
#[tauri::command(async)]
#[specta::specta]
pub async fn studio_bundle_peek(app: AppHandle, id: String, path: String) -> Result<StudioBundlePeek, String> {
    let instance = crate::instance_by_id(&app, &id)?;
    let root = crate::studio_root(&instance).ok_or(NO_STUDIO)?;
    let bundle = read_bundle(path).await?;
    let (have, have_assets) = existing(&app, &instance, root, &bundle).await;
    let clashes = clashes(&bundle, |one| have.contains(one), |folder, file| have_assets.contains(&(folder.to_string(), file.to_string())));
    let kinds = crate::studio_kinds(&instance);
    let mut problems = bundle.problems.clone();
    problems.extend(
        bundle
            .resources
            .iter()
            .filter(|(one, _)| !kinds.contains(&one.kind.as_str()))
            .map(|(one, _)| format!("{}: this server's add-on doesn't run {}.", one.label(), one.kind)),
    );
    Ok(StudioBundlePeek {
        name: bundle.manifest.name.clone(),
        made: bundle.manifest.made,
        resources: bundle.refs(),
        assets: bundle.asset_list(),
        clashes,
        problems,
    })
}

/// Puts a bundle's files on this server. Files it already has are left alone unless `replace`. Pack numbers
/// are kept where they are free, then the pack is built again and the running server rereads.
/// `studio_bundle_peek` for the web panel and teammates: the bundle came in pieces and is named by its upload.
#[tauri::command(async)]
#[specta::specta]
pub async fn studio_bundle_peek_upload(app: AppHandle, id: String, upload: String) -> Result<StudioBundlePeek, String> {
    let path = crate::remote_upload_path(&app, &upload)?;
    studio_bundle_peek(app, id, path).await
}

/// `studio_bundle_import` for the web panel and teammates; the upload is removed afterwards.
#[tauri::command(async)]
#[specta::specta]
pub async fn studio_bundle_import_upload(app: AppHandle, id: String, upload: String, replace: bool) -> Result<StudioBundleInfo, String> {
    let path = crate::remote_upload_path(&app, &upload)?;
    let result = studio_bundle_import(app, id, path.clone(), replace).await;
    if result.is_ok() {
        crate::unstash(std::path::Path::new(&path));
    }
    result
}

#[tauri::command(async)]
#[specta::specta]
pub async fn studio_bundle_import(app: AppHandle, id: String, path: String, replace: bool) -> Result<StudioBundleInfo, String> {
    let instance = crate::instance_by_id(&app, &id)?;
    let root = crate::studio_root(&instance).ok_or(NO_STUDIO)?;
    {
        let (app, instance) = (app.clone(), instance.clone());
        tauri::async_runtime::spawn_blocking(move || crate::studio_assets_ready(&app, &instance))
            .await
            .map_err(|error| error.to_string())??;
    }
    let bytes = std::fs::metadata(&path).map(|one| one.len()).unwrap_or(0);
    let bundle = read_bundle(path.clone()).await?;
    let (have, have_assets) = existing(&app, &instance, root, &bundle).await;
    let found = clashes(&bundle, |one| have.contains(one), |folder, file| have_assets.contains(&(folder.to_string(), file.to_string())));
    let name = bundle.manifest.name.clone();
    let from_ids = bundle.manifest.ids.clone();
    let mut skipped: Vec<String> = bundle.problems.clone();
    let planned = plan(bundle, &found, replace, crate::studio_kinds(&instance));
    skipped.extend(planned.skipped);
    if planned.resources.is_empty() && planned.assets.is_empty() {
        return Err(if found.is_empty() {
            "There is nothing in this bundle this server can use.".into()
        } else {
            "Everything in this bundle is already on this server. Replace to bring it in anyway.".into()
        });
    }

    crate::studio_guide(&app, &instance, root).await;
    // Assets and numbers first, so the add-on finds them when it sees the new files.
    for (folder, file, data) in &planned.assets {
        crate::server_write_bytes(&app, &instance, &format!("{root}/assets/{folder}/{file}"), data).await?;
    }
    let written: Vec<StudioRef> = planned.resources.iter().map(|(one, _)| one.clone()).collect();
    let mut ids = read_ids(&app, &instance, root).await;
    ids.schema = 1;
    if merge_ids(&mut ids, &from_ids, &written) {
        let text = serde_json::to_string_pretty(&ids).map_err(|error| error.to_string())?;
        crate::server_write_text(&app, &instance, &format!("{root}/pack/ids.json"), &text).await?;
    }
    for (one, text) in &planned.resources {
        let file = crate::studio_path(&instance, &one.kind, &one.id)?;
        crate::server_write_text(&app, &instance, &file, text).await?;
    }

    let pumpkin = crate::pumpkin::is_pumpkin(&instance.core);
    // Same as saving an item: looks, fonts and models live in the resource pack.
    crate::studio_pack_write(&app, &instance, root).await?;
    let applied = if pumpkin {
        crate::pumpkin_reload(&app, &instance).await
    } else {
        let apply = async {
            let (line, token, _conn) = crate::studio_line(&app, &instance).await?;
            crate::studio::reload_on(line, &token).await
        };
        matches!(tokio::time::timeout(std::time::Duration::from_secs(6), apply).await, Ok(Ok(_)))
    };
    Ok(StudioBundleInfo {
        name,
        path,
        bytes,
        resources: written,
        assets: planned.assets.iter().map(|(folder, file, _)| StudioBundleAsset { folder: folder.clone(), name: file.clone() }).collect(),
        skipped,
        applied,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn png() -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::RgbaImage::new(16, 16).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// A manifest, its resource files and its asset files (folder, name, bytes).
    type Sample = (Manifest, Vec<(StudioRef, String)>, Vec<(String, String, Vec<u8>)>);

    fn sample() -> Sample {
        let resources = vec![
            (StudioRef::new("item", "ruby_sword"), r#"{"schema":1,"id":"ruby_sword","look":{"texture":"ruby_sword"}}"#.to_string()),
            (StudioRef::new("font", "ruby"), r#"{"schema":1,"image":"ruby"}"#.to_string()),
        ];
        let assets = vec![
            ("textures".to_string(), "ruby_sword.png".to_string(), png()),
            ("sounds".to_string(), "ding.ogg".to_string(), b"OggS....".to_vec()),
        ];
        let mut ids = Ids { schema: 1, ..Default::default() };
        ids.given.entry("cmd".into()).or_default().insert("ruby_sword".into(), 1000);
        ids.given.entry("glyph".into()).or_default().insert("ruby".into(), 0xE003);
        let manifest = Manifest {
            format: FORMAT,
            name: "Ruby set".into(),
            made: 1_700_000_000,
            resources: resources.iter().map(|(one, _)| one.clone()).collect(),
            assets: assets.iter().map(|(folder, file, _)| StudioBundleAsset { folder: folder.clone(), name: file.clone() }).collect(),
            ids,
        };
        (manifest, resources, assets)
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut out);
            for (name, bytes) in entries {
                zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                zip.write_all(bytes).unwrap();
            }
            zip.finish().unwrap();
        }
        out.into_inner()
    }

    #[test]
    fn round_trip() {
        let (manifest, resources, assets) = sample();
        let bytes = write(&manifest, &resources, &assets).unwrap();
        assert_eq!(bytes, write(&manifest, &resources, &assets).unwrap(), "same files, same bytes");
        let back = read(&bytes).unwrap();
        assert_eq!(back.manifest, manifest);
        assert!(back.problems.is_empty(), "{:?}", back.problems);
        let mut sorted = resources.clone();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(back.resources, sorted);
        assert_eq!(back.assets.len(), 2);
        assert!(back.assets.iter().any(|(folder, file, data)| folder == "textures" && file == "ruby_sword.png" && *data == png()));
        assert_eq!(back.manifest.ids.given["glyph"]["ruby"], 0xE003);
    }

    #[test]
    fn bad_files_are_left_out() {
        let head = serde_json::to_vec(&Manifest { format: 1, name: "x".into(), ..Default::default() }).unwrap();
        let bytes = zip_of(&[
            (MANIFEST, head.as_slice()),
            ("studio/item/good.json", br#"{"schema":1}"#),
            ("studio/item/Bad Name.json", br#"{"schema":1}"#),
            ("studio/item/broken.json", b"{"),
            ("studio/nope/x.json", br#"{"schema":1}"#),
            ("studio/assets/textures/fake.png", b"not a png"),
            ("readme.txt", b"hi"),
        ]);
        let back = read(&bytes).unwrap();
        assert_eq!(back.refs(), vec![StudioRef::new("item", "good")]);
        assert_eq!(back.problems.len(), 5, "{:?}", back.problems);
        assert!(read(&zip_of(&[("studio/item/good.json", br#"{"schema":1}"#)])).unwrap_err().contains("bundle.json"));
        let newer = serde_json::to_vec(&json!({ "format": 2 })).unwrap();
        assert!(read(&zip_of(&[(MANIFEST, newer.as_slice())])).unwrap_err().contains("newer"));
        assert!(read(b"not a zip").is_err());
    }

    #[test]
    fn paths_stay_inside() {
        assert!(safe_entry("studio/item/a.json"));
        assert!(safe_entry("studio/item/"));
        assert!(!safe_entry("../evil.json"));
        assert!(!safe_entry("studio/../../evil.json"));
        assert!(!safe_entry("/etc/passwd"));
        assert!(!safe_entry("C:/Windows/x"));
        assert!(!safe_entry("studio\\item\\a.json"));
        assert!(!safe_entry("studio//a.json"));
        assert!(!safe_entry("./a"));
        let head = serde_json::to_vec(&Manifest { format: 1, ..Default::default() }).unwrap();
        for bad in ["../evil.json", "studio/../../evil.json", "/abs.json"] {
            let bytes = zip_of(&[(MANIFEST, head.as_slice()), (bad, b"{}")]);
            assert!(read(&bytes).unwrap_err().contains("outside"), "{bad}");
        }
    }

    #[test]
    fn finds_clashes_and_plans() {
        let (manifest, resources, assets) = sample();
        let bundle = read(&write(&manifest, &resources, &assets).unwrap()).unwrap();
        let found = clashes(&bundle, |one| one.kind == "item", |folder, file| folder == "sounds" && file == "ding.ogg");
        assert_eq!(found, vec!["item/ruby_sword".to_string(), "assets/sounds/ding.ogg".to_string()]);
        let kept = plan(bundle.clone(), &found, false, &studio_format::KINDS);
        assert_eq!(kept.resources.iter().map(|(one, _)| one.label()).collect::<Vec<_>>(), ["font/ruby"]);
        assert_eq!(kept.assets.len(), 1);
        assert_eq!(kept.skipped, ["item/ruby_sword", "assets/sounds/ding.ogg"]);
        let all = plan(bundle.clone(), &found, true, &studio_format::KINDS);
        assert_eq!((all.resources.len(), all.assets.len(), all.skipped.len()), (2, 2, 0));
        let no_fonts = plan(bundle, &[], false, &["item"]);
        assert_eq!(no_fonts.resources.len(), 1);
        assert!(no_fonts.skipped[0].starts_with("font/ruby"));
    }

    #[test]
    fn numbers_kept_only_when_free() {
        let mut from = Ids { schema: 1, ..Default::default() };
        from.given.entry("cmd".into()).or_default().insert("ruby_sword".into(), 1000);
        from.given.entry("cmd".into()).or_default().insert("taken".into(), 1001);
        from.given.entry("cmd".into()).or_default().insert("low".into(), 5);
        from.given.entry("cmd".into()).or_default().insert("there".into(), 1002);
        from.given.entry("glyph".into()).or_default().insert("ruby".into(), 0xE003);
        let mut target = Ids { schema: 1, ..Default::default() };
        target.number("cmd", "other", 1001); // 1001 is someone else's
        target.number("cmd", "there", 1001); // already has a number of its own (1002)
        target.next.insert("cmd".into(), 1003);
        target.number("glyph", "old", 0xE000);
        let written = ["ruby_sword", "taken", "low", "there"]
            .map(|id| StudioRef::new("item", id))
            .into_iter()
            .chain([StudioRef::new("font", "ruby")])
            .collect::<Vec<_>>();
        assert!(merge_ids(&mut target, &from, &written));
        let cmd = &target.given["cmd"];
        assert_eq!(cmd.get("ruby_sword"), None, "1000 is below what the target gives out next");
        assert_eq!(cmd.get("taken"), None, "1001 belongs to another name");
        assert_eq!(cmd.get("low"), None);
        assert_eq!(cmd.get("there"), Some(&1002));
        assert_eq!(target.given["glyph"]["ruby"], 0xE003);
        assert_eq!(target.next["glyph"], 0xE004);
        // The builder hands the rest new numbers that don't collide.
        assert_eq!(target.number("glyph", "next_one", 0xE000), 0xE004);
        assert_eq!(target.number("cmd", "ruby_sword", 1001), 1003);
        // A fresh target takes the bundle's numbers as they are; nothing changes the second time.
        let mut fresh = Ids::default();
        assert!(merge_ids(&mut fresh, &from, &[StudioRef::new("item", "ruby_sword")]));
        assert_eq!(fresh.given["cmd"]["ruby_sword"], 1000);
        assert!(!merge_ids(&mut fresh, &from, &[StudioRef::new("item", "ruby_sword")]));
        // Only the files' own entries go into a bundle.
        let subset = ids_subset(&from, &[StudioRef::new("item", "low"), StudioRef::new("font", "ruby"), StudioRef::new("mob", "x")]);
        assert_eq!(subset.given["cmd"].len(), 1);
        assert_eq!(subset.given["glyph"]["ruby"], 0xE003);
    }

    #[test]
    fn follows_references() {
        let mob = json!({
            "schema": 1, "model": "golem", "loot": "golem_drops",
            "equipment": { "hand": "studio:item/ruby_sword", "head": "diamond_helmet", "chest": "studio:gem" },
            "skills": [{ "actions": [
                { "type": "summon", "mob": "zombie" },
                { "type": "sound", "sound": "monolab:studio.roar" },
                { "type": "message", "text": "Hi :coin: and <red>:ruby:" },
                { "type": "animation", "model": "" },
                { "type": "menu", "menu": "shop" }
            ] }]
        });
        let refs = references("mob", mob.as_object().unwrap());
        let names: Vec<String> = refs.resources.iter().map(StudioRef::label).collect();
        assert_eq!(names, ["item/gem", "item/ruby_sword", "loot/golem_drops", "menu/shop", "mob/zombie", "model/golem"]);
        assert_eq!(refs.sounds.iter().collect::<Vec<_>>(), ["roar"]);
        assert!(refs.glyphs.contains("coin") && refs.glyphs.contains("ruby"));

        let item = json!({ "schema": 1, "look": { "texture": "ruby_sword", "pulling": ["bow_1", "bow_2"] }, "equip": { "texture": "ruby_armor" } });
        let refs = references("item", item.as_object().unwrap());
        assert_eq!(refs.textures.iter().collect::<Vec<_>>(), ["bow_1", "bow_2", "ruby_armor", "ruby_sword"]);
        let block = json!({ "schema": 1, "look": { "texture": "side", "top": "top", "model": "" } });
        assert_eq!(references("block", block.as_object().unwrap()).textures.len(), 2);
        assert_eq!(references("model", json!({ "model": "golem" }).as_object().unwrap()).models.iter().collect::<Vec<_>>(), ["golem"]);
        assert_eq!(references("font", json!({ "image": "ruby.png" }).as_object().unwrap()).textures.iter().collect::<Vec<_>>(), ["ruby"]);
        let canvas = json!({ "elements": [{ "type": "image", "glyph": "bg" }] });
        assert!(references("dialog", canvas.as_object().unwrap()).glyphs.contains("bg"));
        let model = br#"{"parent":"item/generated","textures":{"layer0":"monolab:studio/ruby","1":"minecraft:block/stone"}}"#;
        assert_eq!(model_assets(model).into_iter().collect::<Vec<_>>(), ["ruby"]);
    }
}

//! The NBT editor behind the Files tab: open sessions keep the parsed file,
//! its open rows and undo history here, and the page asks for rows to draw.
//! Editing works on local servers; remote ones keep the read-only view.

use monolab_nbt::{region_coords, ChunkInfo, Editor, FileKind, Kind, NbtFile, Region, Step, Tag};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::SystemTime,
};
use tauri::AppHandle;

/// Region files run larger than the text viewer's limit; a full one is ~10 MB.
const OPEN_LIMIT: u64 = 256 * 1024 * 1024;

struct RegionDoc {
    region: Region,
    current: Option<usize>,
    /// Chunks changed in memory but not yet written to disk.
    changed: bool,
}

struct Session {
    path: PathBuf,
    /// Modification time when opened or last saved, to catch outside writes.
    stamp: Option<SystemTime>,
    region: Option<RegionDoc>,
    editor: Option<Editor>,
}

impl Session {
    fn dirty(&self) -> bool {
        self.region.as_ref().is_some_and(|one| one.changed)
            || self.editor.as_ref().is_some_and(Editor::dirty)
    }

    /// Folds the open chunk's edits back into the region in memory.
    fn stash_chunk(&mut self) -> Result<(), String> {
        let (Some(region), Some(editor)) = (self.region.as_mut(), self.editor.as_mut()) else {
            return Ok(());
        };
        if !editor.dirty() {
            return Ok(());
        }
        let index = region.current.ok_or("No chunk is open.")?;
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs() as u32)
            .unwrap_or(0);
        region.region.store(index, &editor.file(), stamp)?;
        region.changed = true;
        editor.mark_saved();
        Ok(())
    }
}

fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    static ALL: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
    ALL.get_or_init(Default::default)
}

fn with<T>(session: &str, work: impl FnOnce(&mut Session) -> Result<T, String>) -> Result<T, String> {
    let mut all = sessions().lock().map_err(|_| "The NBT editor is busy.".to_string())?;
    let one = all
        .get_mut(session)
        .ok_or("This file is no longer open. Open it again.")?;
    work(one)
}

/// One step into a container: a compound key (string) or a list index (number).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(untagged)]
pub enum NbtStep {
    Index(u32),
    Key(String),
}

fn to_steps(path: &[NbtStep]) -> Vec<Step> {
    path.iter()
        .map(|step| match step {
            NbtStep::Index(at) => Step::Index(*at as usize),
            NbtStep::Key(key) => Step::Key(key.clone()),
        })
        .collect()
}

fn from_steps(path: &[Step]) -> Vec<NbtStep> {
    path.iter()
        .map(|step| match step {
            Step::Index(at) => NbtStep::Index(*at as u32),
            Step::Key(key) => NbtStep::Key(key.clone()),
        })
        .collect()
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NbtRow {
    pub path: Vec<NbtStep>,
    pub depth: u32,
    pub label: String,
    /// The NBT type id (1 Byte … 12 Long array).
    pub kind: u8,
    pub preview: String,
    pub children: u32,
    pub open: bool,
    /// Matched the search; ancestors of matches are listed with this false.
    pub hit: bool,
    /// What an inline edit starts from, for values short enough to type.
    pub text: Option<String>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NbtChunk {
    pub index: u32,
    pub x: u32,
    pub z: u32,
    pub timestamp: u32,
    pub bytes: u32,
    pub external: bool,
}

impl From<ChunkInfo> for NbtChunk {
    fn from(info: ChunkInfo) -> Self {
        NbtChunk {
            index: info.index as u32,
            x: info.x as u32,
            z: info.z as u32,
            timestamp: info.timestamp,
            bytes: info.bytes as u32,
            external: info.external,
        }
    }
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NbtView {
    pub session: String,
    pub rows: Vec<NbtRow>,
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub compression: String,
    pub is_region: bool,
    /// Region coordinates from the file name, when it is a region file.
    pub region_at: Option<[i32; 2]>,
    /// The chunk being edited, for region files.
    pub chunk: Option<u32>,
    /// Sent only when the chunk list may have changed.
    pub chunks: Option<Vec<NbtChunk>>,
    /// The row an action produced (a new value), to select and scroll to.
    pub focus: Option<Vec<NbtStep>>,
}

fn text_of(tag: &Tag) -> Option<String> {
    match tag {
        Tag::Compound(_) | Tag::List(_) => None,
        Tag::ByteArray(_) | Tag::IntArray(_) | Tag::LongArray(_) if tag.len() > 4096 => None,
        other => Some(monolab_nbt::snbt::edit_text(other)),
    }
}

fn view(id: &str, one: &Session, search: &str, chunks: bool) -> NbtView {
    let rows = one
        .editor
        .as_ref()
        .map(|editor| {
            editor
                .rows(search)
                .into_iter()
                .map(|row| NbtRow {
                    text: editor.get(&row.path).and_then(text_of),
                    path: from_steps(&row.path),
                    depth: row.depth as u32,
                    label: row.label,
                    kind: row.kind.id(),
                    preview: row.preview,
                    children: row.children as u32,
                    open: row.open,
                    hit: row.hit,
                })
                .collect()
        })
        .unwrap_or_default();
    let name = one.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    NbtView {
        session: id.to_string(),
        rows,
        dirty: one.dirty(),
        can_undo: one.editor.as_ref().is_some_and(Editor::can_undo),
        can_redo: one.editor.as_ref().is_some_and(Editor::can_redo),
        compression: one
            .editor
            .as_ref()
            .map(|editor| editor.compression.name().to_string())
            .unwrap_or_default(),
        is_region: one.region.is_some(),
        region_at: region_coords(name).map(|(x, z)| [x, z]),
        chunk: one.region.as_ref().and_then(|r| r.current).map(|c| c as u32),
        chunks: chunks
            .then(|| one.region.as_ref().map(|r| r.region.chunks().into_iter().map(NbtChunk::from).collect()))
            .flatten(),
        focus: None,
    }
}

fn stamp_of(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

/// Opens a local server's NBT or region file for editing.
#[tauri::command(async)]
#[specta::specta]
pub fn nbt_open(app: AppHandle, id: String, path: String) -> Result<NbtView, String> {
    let instance = crate::instance_by_id(&app, &id)?;
    if !instance.panel_host.is_empty() || !instance.remote_host.is_empty() {
        return Err("NBT files can be edited on servers on this computer.".into());
    }
    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
    let kind = monolab_nbt::file_kind(&name).ok_or("This is not an NBT file.")?;
    let at = crate::paths::resolve_in_server(Path::new(&instance.path), &path).map_err(|error| match error {
        crate::paths::PathError::Missing => "File not found.".to_string(),
        other => other.to_string(),
    })?;
    let size = fs::metadata(&at).map_err(|error| error.to_string())?.len();
    if size > OPEN_LIMIT {
        return Err("This file is too large to open in the app.".into());
    }
    let bytes = fs::read(&at).map_err(|error| format!("Could not read the file: {error}"))?;
    let mut session = Session {
        stamp: stamp_of(&at),
        path: at,
        region: None,
        editor: None,
    };
    match kind {
        FileKind::Region => {
            session.region = Some(RegionDoc {
                region: Region::read(&bytes)?,
                current: None,
                changed: false,
            });
        }
        FileKind::Document => {
            session.editor = Some(Editor::new(NbtFile::read(&bytes)?));
        }
    }
    // Unguessable, since web panel users reach sessions by this key alone.
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    let key = format!("nbt-{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>());
    let out = view(&key, &session, "", true);
    sessions()
        .lock()
        .map_err(|_| "The NBT editor is busy.".to_string())?
        .insert(key, session);
    Ok(out)
}

/// Switches a region session to one of its chunks, keeping earlier chunk edits.
#[tauri::command(async)]
#[specta::specta]
pub fn nbt_open_chunk(session: String, index: u32) -> Result<NbtView, String> {
    with(&session, |one| {
        one.stash_chunk()?;
        let path = one.path.clone();
        let region = one.region.as_mut().ok_or("This is not a region file.")?;
        let index = index as usize;
        let info = region.region.chunks().into_iter().find(|c| c.index == index);
        let file = region.region.load(index, || {
            let (rx, rz) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(region_coords)
                .ok_or("The region file name does not say where it is.")?;
            let info = info.ok_or("No such chunk.")?;
            let external = path.with_file_name(format!(
                "c.{}.{}.mcc",
                rx * 32 + info.x as i32,
                rz * 32 + info.z as i32
            ));
            fs::read(&external).map_err(|error| format!("Could not read {}: {error}", external.display()))
        })?;
        region.current = Some(index);
        one.editor = Some(Editor::new(file));
        Ok(view(&session, one, "", true))
    })
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum NbtAction {
    /// List the rows again, chunks included (on mount or a search change).
    Refresh,
    Toggle { path: Vec<NbtStep> },
    ExpandAll { path: Vec<NbtStep> },
    CollapseAll,
    /// Replace a value with typed text, read as the value's own type.
    Set { path: Vec<NbtStep>, text: String },
    /// Replace a value with SNBT of any type, e.g. a pasted compound.
    SetSnbt { path: Vec<NbtStep>, snbt: String },
    Rename { path: Vec<NbtStep>, name: String },
    Remove { path: Vec<NbtStep> },
    /// Add a new value inside `path` (a container) or after it.
    Add { path: Vec<NbtStep>, kind: u8, name: String },
    /// Insert pasted SNBT inside or after `path`.
    Paste { path: Vec<NbtStep>, snbt: String, name: String },
    Undo,
    Redo,
}

/// Applies one action and returns the rows to show. Failed edits change nothing.
#[tauri::command(async)]
#[specta::specta]
pub fn nbt_act(session: String, action: NbtAction, search: String) -> Result<NbtView, String> {
    with(&session, |one| {
        if matches!(action, NbtAction::Refresh) {
            return Ok(view(&session, one, &search, true));
        }
        let editor = one.editor.as_mut().ok_or("Pick a chunk first.")?;
        let mut focus = None;
        match action {
            NbtAction::Refresh => {}
            NbtAction::Toggle { path } => editor.toggle(&to_steps(&path)),
            NbtAction::ExpandAll { path } => editor.expand_all(&to_steps(&path)),
            NbtAction::CollapseAll => editor.collapse_all(),
            NbtAction::Set { path, text } => editor.set_text(&to_steps(&path), &text)?,
            NbtAction::SetSnbt { path, snbt } => {
                editor.set(&to_steps(&path), monolab_nbt::snbt::parse(&snbt)?)?
            }
            NbtAction::Rename { path, name } => {
                let name = name.trim();
                if name.is_empty() {
                    return Err("Please enter a name.".into());
                }
                editor.rename(&to_steps(&path), name)?
            }
            NbtAction::Remove { path } => editor.remove(&to_steps(&path))?,
            NbtAction::Add { path, kind, name } => {
                let kind = Kind::from_id(kind)
                    .filter(|kind| *kind != Kind::End)
                    .ok_or("Unknown NBT type.")?;
                let added = editor.add(&to_steps(&path), kind.default_tag(), name.trim())?;
                focus = Some(from_steps(&added));
            }
            NbtAction::Paste { path, snbt, name } => {
                let tag = monolab_nbt::snbt::parse(snbt.trim())?;
                let added = editor.add(&to_steps(&path), tag, name.trim())?;
                focus = Some(from_steps(&added));
            }
            NbtAction::Undo => editor.undo()?,
            NbtAction::Redo => editor.redo()?,
        }
        let mut out = view(&session, one, &search, false);
        out.focus = focus;
        Ok(out)
    })
}

/// The value at `path` as SNBT, for copying.
#[tauri::command(async)]
#[specta::specta]
pub fn nbt_copy(session: String, path: Vec<NbtStep>, pretty: bool) -> Result<String, String> {
    with(&session, |one| {
        let editor = one.editor.as_ref().ok_or("Pick a chunk first.")?;
        let tag = editor.get(&to_steps(&path)).ok_or("That value no longer exists.")?;
        Ok(monolab_nbt::snbt::to_snbt(tag, pretty))
    })
}

/// Writes the file back. Refuses when something else wrote it since it was
/// opened, unless `force`; the server rewrites level.dat and player data while
/// it runs.
#[tauri::command(async)]
#[specta::specta]
pub fn nbt_save(app: AppHandle, id: String, session: String, force: bool) -> Result<NbtView, String> {
    let out = with(&session, |one| {
        if !force && one.stamp.is_some() && stamp_of(&one.path) != one.stamp {
            return Err("CHANGED_ON_DISK".into());
        }
        one.stash_chunk()?;
        let bytes = match (&one.region, &one.editor) {
            (Some(region), _) => region.region.write(),
            (None, Some(editor)) => editor.file().write(),
            _ => return Err("Nothing to save.".into()),
        };
        // Write beside the file, then swap, so a crash never leaves half a file.
        let temp = one.path.with_extension("monolab-tmp");
        fs::write(&temp, &bytes)
            .and_then(|_| fs::rename(&temp, &one.path))
            .map_err(|error| {
                let _ = fs::remove_file(&temp);
                format!("Could not save: {error}")
            })?;
        one.stamp = stamp_of(&one.path);
        if let Some(region) = one.region.as_mut() {
            region.changed = false;
        }
        if let Some(editor) = one.editor.as_mut() {
            editor.mark_saved();
        }
        Ok((view(&session, one, "", true), one.path.display().to_string()))
    })?;
    crate::record_audit(&app, &id, "File", "NBT file saved", &out.1);
    Ok(out.0)
}

#[tauri::command(async)]
#[specta::specta]
pub fn nbt_close(session: String) {
    if let Ok(mut all) = sessions().lock() {
        all.remove(&session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monolab_nbt::Compound;

    fn session_with(root: Compound) -> Session {
        Session {
            path: PathBuf::from("level.dat"),
            stamp: None,
            region: None,
            editor: Some(Editor::new(NbtFile::new(root))),
        }
    }

    #[test]
    fn steps_round_trip_through_json() {
        let path = vec![NbtStep::Key("Data".into()), NbtStep::Index(3), NbtStep::Key("7".into())];
        let json = serde_json::to_string(&path).unwrap();
        assert_eq!(json, r#"["Data",3,"7"]"#);
        let back: Vec<NbtStep> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, path);
        assert_eq!(from_steps(&to_steps(&back)), path);
    }

    #[test]
    fn rows_carry_edit_text_for_scalars_only() {
        let mut root = Compound::new();
        root.insert("Health", Tag::Float(20.0));
        root.insert("Name", Tag::String("Steve".into()));
        root.insert("Inv", Tag::Compound(Compound::new()));
        let one = session_with(root);
        let out = view("s", &one, "", false);
        let find = |label: &str| out.rows.iter().find(|row| row.label == label).unwrap();
        assert_eq!(find("Health").text.as_deref(), Some("20.0"));
        assert_eq!(find("Name").text.as_deref(), Some("Steve"));
        assert_eq!(find("Inv").text, None);
        assert!(!out.dirty);
    }

    #[test]
    fn actions_parse_from_page_json() {
        let action: NbtAction =
            serde_json::from_str(r#"{"type":"set","path":["Health"],"text":"5"}"#).unwrap();
        assert!(matches!(action, NbtAction::Set { .. }));
        let action: NbtAction = serde_json::from_str(r#"{"type":"collapseAll"}"#).unwrap();
        assert!(matches!(action, NbtAction::CollapseAll));
    }
}

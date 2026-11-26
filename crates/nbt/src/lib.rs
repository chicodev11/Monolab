//! NBT for Monolab: an order-preserving model of Java edition NBT, its binary
//! and SNBT forms, Anvil region files, and the state behind an NBT editor.

pub mod binary;
pub mod editor;
pub mod region;
pub mod snbt;
pub mod tag;

pub use binary::{Compression, NbtFile};
pub use editor::{Editor, Row};
pub use region::{region_coords, ChunkInfo, Region};
pub use tag::{Compound, Kind, List, Step, Tag};

/// What kind of NBT file a name points to, from its extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    /// A single NBT document: `level.dat`, player data, structures…
    Document,
    /// An Anvil region holding up to 1024 chunk documents.
    Region,
}

pub fn file_kind(name: &str) -> Option<FileKind> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".mca") || lower.ends_with(".mcr") {
        return Some(FileKind::Region);
    }
    [".dat", ".dat_old", ".nbt", ".schem", ".schematic", ".litematic", ".mcstructure_java"]
        .iter()
        .any(|ext| lower.ends_with(ext))
        .then_some(FileKind::Document)
}

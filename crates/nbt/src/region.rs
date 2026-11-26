//! Anvil region files (`r.X.Z.mca`): 32×32 chunks, each its own NBT document.
//! Chunks stay compressed until opened, so a region opens instantly.

use crate::binary::{self, Compression, NbtFile};

const SECTOR: usize = 4096;

#[derive(Clone, Debug)]
struct Slot {
    timestamp: u32,
    /// The scheme byte from the chunk header: 1 gzip, 2 zlib, 3 none, 4 LZ4.
    scheme: u8,
    payload: Vec<u8>,
    /// Too big for the region; the data lives in a `c.X.Z.mcc` file beside it.
    external: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Region {
    slots: Vec<Option<Slot>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkInfo {
    pub index: usize,
    /// Position inside the region, 0..32.
    pub x: usize,
    pub z: usize,
    pub timestamp: u32,
    pub bytes: usize,
    pub external: bool,
}

/// Reads `r.X.Z.mca` into its region coordinates.
pub fn region_coords(file_name: &str) -> Option<(i32, i32)> {
    let mut parts = file_name.strip_prefix("r.")?.strip_suffix(".mca")?.split('.');
    let x = parts.next()?.parse().ok()?;
    let z = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((x, z))
}

impl Region {
    pub fn empty() -> Region {
        Region { slots: vec![None; 1024] }
    }

    pub fn read(bytes: &[u8]) -> Result<Region, String> {
        if bytes.is_empty() {
            return Ok(Region::empty());
        }
        if bytes.len() < SECTOR * 2 {
            return Err("This region file is too short to be valid.".into());
        }
        let mut slots = vec![None; 1024];
        for (index, slot) in slots.iter_mut().enumerate() {
            let entry = u32::from_be_bytes(bytes[index * 4..index * 4 + 4].try_into().expect("4"));
            let offset = (entry >> 8) as usize * SECTOR;
            let sectors = (entry & 0xff) as usize;
            if offset == 0 || sectors == 0 {
                continue;
            }
            let at = SECTOR + index * 4;
            let timestamp = u32::from_be_bytes(bytes[at..at + 4].try_into().expect("4"));
            // A damaged entry only loses that chunk, never the region.
            let Some(head) = bytes.get(offset..offset + 5) else {
                continue;
            };
            let length = u32::from_be_bytes(head[..4].try_into().expect("4")) as usize;
            if length == 0 {
                continue;
            }
            let scheme = head[4];
            let Some(payload) = bytes.get(offset + 5..offset + 4 + length) else {
                continue;
            };
            *slot = Some(Slot { timestamp, scheme: scheme & 0x7f, payload: payload.to_vec(), external: scheme & 0x80 != 0 });
        }
        Ok(Region { slots })
    }

    pub fn chunks(&self) -> Vec<ChunkInfo> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let slot = slot.as_ref()?;
                Some(ChunkInfo {
                    index,
                    x: index % 32,
                    z: index / 32,
                    timestamp: slot.timestamp,
                    bytes: slot.payload.len(),
                    external: slot.external,
                })
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Decodes one chunk. `external` supplies the `.mcc` bytes for chunks too
    /// large for the region, and is only called for those.
    pub fn load(&self, index: usize, external: impl FnOnce() -> Result<Vec<u8>, String>) -> Result<NbtFile, String> {
        let slot = self.slots.get(index).and_then(Option::as_ref).ok_or("That chunk is empty.")?;
        let owned;
        let bytes = if slot.external {
            owned = external()?;
            &owned
        } else {
            &slot.payload
        };
        let compression = match slot.scheme {
            1 => Compression::Gzip,
            2 => Compression::Zlib,
            3 => Compression::None,
            4 => return Err("This chunk uses LZ4 compression, which is not supported yet.".into()),
            other => return Err(format!("This chunk uses an unknown compression ({other}).")),
        };
        let raw = binary::decompress(bytes, compression)?;
        let (name, root) = binary::read_root(&raw)?;
        Ok(NbtFile { name, root, compression })
    }

    /// Replaces a chunk with `file`, compressed the way `file` says.
    pub fn store(&mut self, index: usize, file: &NbtFile, timestamp: u32) -> Result<(), String> {
        let scheme = match file.compression {
            Compression::Gzip => 1,
            Compression::Zlib => 2,
            Compression::None => 3,
        };
        let payload = file.write();
        if payload.len() + 5 > 255 * SECTOR {
            return Err("This chunk is over 1 MB once saved, which needs an external file; that is not supported yet.".into());
        }
        let slot = self.slots.get_mut(index).ok_or("No such chunk.")?;
        *slot = Some(Slot { timestamp, scheme, payload, external: false });
        Ok(())
    }

    pub fn remove(&mut self, index: usize) {
        if let Some(slot) = self.slots.get_mut(index) {
            *slot = None;
        }
    }

    /// Lays the region out again with no gaps between chunks.
    pub fn write(&self) -> Vec<u8> {
        let mut out = vec![0u8; SECTOR * 2];
        for (index, slot) in self.slots.iter().enumerate() {
            let Some(slot) = slot else { continue };
            let body = if slot.external { 1 } else { slot.payload.len() + 1 };
            let sectors = (body + 4).div_ceil(SECTOR);
            let offset = out.len() / SECTOR;
            let entry = ((offset as u32) << 8) | sectors as u32;
            out[index * 4..index * 4 + 4].copy_from_slice(&entry.to_be_bytes());
            let at = SECTOR + index * 4;
            out[at..at + 4].copy_from_slice(&slot.timestamp.to_be_bytes());
            out.extend_from_slice(&(body as u32).to_be_bytes());
            out.push(slot.scheme | if slot.external { 0x80 } else { 0 });
            if !slot.external {
                out.extend_from_slice(&slot.payload);
            }
            out.resize((offset + sectors) * SECTOR, 0);
        }
        out
    }
}

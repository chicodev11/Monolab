//! Java edition binary NBT: big-endian, strings in Java's modified UTF-8.

use crate::tag::{Compound, Kind, List, Tag};
use flate2::{read::GzDecoder, read::ZlibDecoder, write::GzEncoder, write::ZlibEncoder};
use std::io::{Read, Write};

/// Nesting deeper than this is refused, as the game itself does.
const MAX_DEPTH: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Zlib,
}

impl Compression {
    pub fn sniff(bytes: &[u8]) -> Compression {
        match bytes {
            [0x1f, 0x8b, ..] => Compression::Gzip,
            [0x78, 0x01 | 0x5e | 0x9c | 0xda, ..] => Compression::Zlib,
            _ => Compression::None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Compression::None => "Uncompressed",
            Compression::Gzip => "GZip",
            Compression::Zlib => "Zlib",
        }
    }
}

pub fn decompress(bytes: &[u8], how: Compression) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(bytes.len() * 4);
    match how {
        Compression::None => return Ok(bytes.to_vec()),
        Compression::Gzip => GzDecoder::new(bytes).read_to_end(&mut out),
        Compression::Zlib => ZlibDecoder::new(bytes).read_to_end(&mut out),
    }
    .map_err(|error| format!("Could not decompress the file: {error}"))?;
    Ok(out)
}

pub fn compress(bytes: &[u8], how: Compression) -> Vec<u8> {
    let level = flate2::Compression::default();
    match how {
        Compression::None => bytes.to_vec(),
        Compression::Gzip => {
            let mut writer = GzEncoder::new(Vec::new(), level);
            writer.write_all(bytes).expect("writing to memory");
            writer.finish().expect("writing to memory")
        }
        Compression::Zlib => {
            let mut writer = ZlibEncoder::new(Vec::new(), level);
            writer.write_all(bytes).expect("writing to memory");
            writer.finish().expect("writing to memory")
        }
    }
}

/// A whole NBT document: the named root compound and how it was stored.
#[derive(Clone, Debug, PartialEq)]
pub struct NbtFile {
    pub name: String,
    pub root: Compound,
    pub compression: Compression,
}

impl NbtFile {
    pub fn new(root: Compound) -> NbtFile {
        NbtFile { name: String::new(), root, compression: Compression::Gzip }
    }

    /// Reads a file's bytes, compressed or not.
    pub fn read(bytes: &[u8]) -> Result<NbtFile, String> {
        let compression = Compression::sniff(bytes);
        let raw = decompress(bytes, compression)?;
        let (name, root) = read_root(&raw)?;
        Ok(NbtFile { name, root, compression })
    }

    pub fn write(&self) -> Vec<u8> {
        compress(&write_root(&self.name, &self.root), self.compression)
    }
}

/// Parses an uncompressed document with a named root compound.
pub fn read_root(raw: &[u8]) -> Result<(String, Compound), String> {
    let mut reader = Reader { bytes: raw, at: 0 };
    let kind = reader.u8()?;
    if kind != Kind::Compound.id() {
        return Err(if kind == 0 { "The file is empty.".into() } else { "This is not an NBT file (the root is not a compound).".into() });
    }
    let name = reader.string()?;
    let root = reader.compound(0)?;
    Ok((name, root))
}

pub fn write_root(name: &str, root: &Compound) -> Vec<u8> {
    let mut out = Vec::with_capacity(4096);
    out.push(Kind::Compound.id());
    put_string(&mut out, name);
    put_compound(&mut out, root);
    out
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(count).filter(|end| *end <= self.bytes.len());
        let end = end.ok_or("The file ends too early; it may be damaged.")?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], String> {
        Ok(self.take(N)?.try_into().expect("exact length"))
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn len(&mut self) -> Result<usize, String> {
        let n = i32::from_be_bytes(self.array()?);
        if n < 0 {
            return Err("A negative length was found; the file may be damaged.".into());
        }
        Ok(n as usize)
    }

    fn string(&mut self) -> Result<String, String> {
        let n = u16::from_be_bytes(self.array()?) as usize;
        decode_mutf8(self.take(n)?)
    }

    fn compound(&mut self, depth: usize) -> Result<Compound, String> {
        let mut out = Compound::new();
        loop {
            let id = self.u8()?;
            let kind = Kind::from_id(id).ok_or_else(|| format!("Unknown tag type {id}."))?;
            if kind == Kind::End {
                return Ok(out);
            }
            let key = self.string()?;
            let value = self.payload(kind, depth + 1)?;
            out.insert(key, value);
        }
    }

    fn payload(&mut self, kind: Kind, depth: usize) -> Result<Tag, String> {
        if depth > MAX_DEPTH {
            return Err("The data is nested too deeply.".into());
        }
        Ok(match kind {
            Kind::End => return Err("An End tag appeared where a value was expected.".into()),
            Kind::Byte => Tag::Byte(self.u8()? as i8),
            Kind::Short => Tag::Short(i16::from_be_bytes(self.array()?)),
            Kind::Int => Tag::Int(i32::from_be_bytes(self.array()?)),
            Kind::Long => Tag::Long(i64::from_be_bytes(self.array()?)),
            Kind::Float => Tag::Float(f32::from_be_bytes(self.array()?)),
            Kind::Double => Tag::Double(f64::from_be_bytes(self.array()?)),
            Kind::ByteArray => {
                let n = self.len()?;
                Tag::ByteArray(self.take(n)?.iter().map(|b| *b as i8).collect())
            }
            Kind::String => Tag::String(self.string()?),
            Kind::List => {
                let id = self.u8()?;
                let kind = Kind::from_id(id).ok_or_else(|| format!("Unknown tag type {id}."))?;
                let n = self.len()?;
                if kind == Kind::End && n > 0 {
                    return Err("A list of End tags is not valid.".into());
                }
                // Guard the allocation: each item takes at least one byte.
                let mut items = Vec::with_capacity(n.min(self.bytes.len() - self.at));
                for _ in 0..n {
                    items.push(self.payload(kind, depth + 1)?);
                }
                Tag::List(List { kind, items })
            }
            Kind::Compound => Tag::Compound(self.compound(depth)?),
            Kind::IntArray => {
                let n = self.len()?;
                let raw = self.take(n.checked_mul(4).ok_or("Array too large.")?)?;
                Tag::IntArray(raw.as_chunks::<4>().0.iter().map(|c| i32::from_be_bytes(*c)).collect())
            }
            Kind::LongArray => {
                let n = self.len()?;
                let raw = self.take(n.checked_mul(8).ok_or("Array too large.")?)?;
                Tag::LongArray(raw.as_chunks::<8>().0.iter().map(|c| i64::from_be_bytes(*c)).collect())
            }
        })
    }
}

fn put_string(out: &mut Vec<u8>, text: &str) {
    let bytes = encode_mutf8(text);
    // Longer strings cannot be stored; the game truncates the same way.
    let n = bytes.len().min(u16::MAX as usize);
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&bytes[..n]);
}

fn put_compound(out: &mut Vec<u8>, map: &Compound) {
    for (key, tag) in map.iter() {
        out.push(tag.kind().id());
        put_string(out, key);
        put_payload(out, tag);
    }
    out.push(Kind::End.id());
}

fn put_payload(out: &mut Vec<u8>, tag: &Tag) {
    match tag {
        Tag::Byte(v) => out.push(*v as u8),
        Tag::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Float(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Double(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::ByteArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            out.extend(v.iter().map(|b| *b as u8));
        }
        Tag::String(text) => put_string(out, text),
        Tag::List(list) => {
            let kind = if list.items.is_empty() { list.kind } else { list.items[0].kind() };
            out.push(kind.id());
            out.extend_from_slice(&(list.items.len() as i32).to_be_bytes());
            for item in &list.items {
                put_payload(out, item);
            }
        }
        Tag::Compound(map) => put_compound(out, map),
        Tag::IntArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for n in v {
                out.extend_from_slice(&n.to_be_bytes());
            }
        }
        Tag::LongArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for n in v {
                out.extend_from_slice(&n.to_be_bytes());
            }
        }
    }
}

/// Java's modified UTF-8: NUL is two bytes and characters outside the BMP are
/// written as two encoded UTF-16 surrogates.
pub fn encode_mutf8(text: &str) -> Vec<u8> {
    if text.bytes().all(|b| b != 0 && b < 0x80) {
        return text.as_bytes().to_vec();
    }
    let mut out = Vec::with_capacity(text.len() + 8);
    for unit in text.encode_utf16() {
        match unit {
            0x0001..=0x007f => out.push(unit as u8),
            0x0000 | 0x0080..=0x07ff => {
                out.push(0xc0 | (unit >> 6) as u8);
                out.push(0x80 | (unit & 0x3f) as u8);
            }
            _ => {
                out.push(0xe0 | (unit >> 12) as u8);
                out.push(0x80 | ((unit >> 6) & 0x3f) as u8);
                out.push(0x80 | (unit & 0x3f) as u8);
            }
        }
    }
    out
}

pub fn decode_mutf8(bytes: &[u8]) -> Result<String, String> {
    if bytes.iter().all(|b| *b < 0x80) {
        return Ok(String::from_utf8(bytes.to_vec()).expect("ascii"));
    }
    let bad = || "A text value is not valid modified UTF-8.".to_string();
    let mut units = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let a = bytes[at] as u16;
        let next = |i: usize| -> Result<u16, String> {
            let b = *bytes.get(i).ok_or_else(bad)? as u16;
            if b & 0xc0 != 0x80 {
                return Err(bad());
            }
            Ok(b & 0x3f)
        };
        if a < 0x80 {
            units.push(a);
            at += 1;
        } else if a & 0xe0 == 0xc0 {
            units.push(((a & 0x1f) << 6) | next(at + 1)?);
            at += 2;
        } else if a & 0xf0 == 0xe0 {
            units.push(((a & 0x0f) << 12) | (next(at + 1)? << 6) | next(at + 2)?);
            at += 3;
        } else if a & 0xf8 == 0xf0 {
            // Plain UTF-8 four-byte form: some tools write it, so accept it.
            let s = std::str::from_utf8(bytes.get(at..at + 4).ok_or_else(bad)?).map_err(|_| bad())?;
            units.extend(s.encode_utf16());
            at += 4;
        } else {
            return Err(bad());
        }
    }
    Ok(String::from_utf16_lossy(&units))
}

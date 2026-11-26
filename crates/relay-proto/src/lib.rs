//! Monolink types shared by the Monolab app and the Monolink account server: device ids, device
//! names, hex for carrying bytes in JSON, and signed role grants.

pub mod grant;

use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Longest device name the account server keeps.
pub const MAX_NAME: usize = 40;

/// A device: the first 16 bytes of the SHA-256 of its Ed25519 public key.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DeviceId(pub [u8; 16]);

impl DeviceId {
    pub fn of(public_key: &[u8; 32]) -> Self {
        let digest: [u8; 32] = Sha256::digest(public_key).into();
        let mut id = [0u8; 16];
        id.copy_from_slice(&digest[..16]);
        Self(id)
    }

    /// Full id, 26 base32 letters, for storage and logs.
    pub fn text(&self) -> String {
        base32(&self.0)
    }

    /// The short form people compare by eye, like `K7QX-M2PA`.
    pub fn short(&self) -> String {
        let text = self.text();
        format!("{}-{}", &text[..4], &text[4..8])
    }

    pub fn parse(text: &str) -> Option<Self> {
        let bytes = unbase32(text)?;
        let id: [u8; 16] = bytes.try_into().ok()?;
        Some(Self(id))
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceId({})", self.short())
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.short())
    }
}

/// Keeps a device name short and printable.
pub fn clean_name(name: &str) -> String {
    let cleaned: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(MAX_NAME).collect();
    if cleaned.is_empty() { "Monolab".into() } else { cleaned }
}

const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn base32(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in bytes {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn unbase32(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for letter in text.bytes().filter(|letter| *letter != b'-') {
        let value = ALPHABET.iter().position(|one| *one == letter.to_ascii_uppercase())? as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// Lowercase hex, for carrying bytes in JSON.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn device_ids_round_trip_as_text() {
        let id = DeviceId::of(&key(7).verifying_key().to_bytes());
        assert_eq!(id.text().len(), 26);
        assert_eq!(DeviceId::parse(&id.text()), Some(id));
        assert_eq!(id.short().len(), 9);
        assert!(DeviceId::parse("not base32 !").is_none());
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  Na\n\u{7}  "), "Na");
        assert_eq!(clean_name(""), "Monolab");
        assert_eq!(clean_name(&"a".repeat(100)).chars().count(), MAX_NAME);
    }
}
